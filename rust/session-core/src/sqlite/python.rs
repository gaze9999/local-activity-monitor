use rustpython_ast::{self as ast, Ranged, Visitor};
use rustpython_parser::{lexer, Mode, Parse};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::{database_path, statements, Kind, Operation, EVENT_LIMIT};

const TOKEN_LIMIT: usize = 4096;

struct Call {
    start: u32,
    receiver: Option<String>,
    function: String,
    literal: Option<String>,
    database: Option<String>,
}

enum Binding {
    Assignment(usize),
    With(Range<usize>),
}

struct Bind {
    start: u32,
    targets: Vec<String>,
    source: Binding,
}

#[derive(Default)]
struct Scan {
    modules: HashSet<String>,
    connectors: HashSet<String>,
    calls: Vec<Call>,
    bindings: Vec<Bind>,
}

pub fn operations(code: &str) -> Vec<Operation> {
    if !code.contains("sqlite")
        || lexer::lex(code, Mode::Module).take(TOKEN_LIMIT + 1).count() > TOKEN_LIMIT
    {
        return Vec::new();
    }
    let Ok(tree) = ast::Suite::parse(code, "<recorded>") else {
        return Vec::new();
    };
    let mut scan = Scan::default();
    scan.modules.insert("sqlite3".into());
    for node in tree {
        scan.visit_stmt(node);
    }
    scan.operations()
}

fn name(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Name(name) = expr else {
        return None;
    };
    Some(name.id.as_str())
}

fn literal(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Constant(value) = expr else {
        return None;
    };
    let ast::Constant::Str(value) = &value.value else {
        return None;
    };
    Some(value)
}

fn head(expr: &ast::Expr) -> Option<(Option<&str>, &str)> {
    match expr {
        ast::Expr::Attribute(field)
            if matches!(
                field.attr.as_str(),
                "connect"
                    | "cursor"
                    | "execute"
                    | "executemany"
                    | "executescript"
                    | "commit"
                    | "rollback"
                    | "close"
            ) =>
        {
            Some((Some(name(&field.value)?), field.attr.as_str()))
        }
        ast::Expr::Name(value) => Some((None, value.id.as_str())),
        _ => None,
    }
}

impl Visitor for Scan {
    fn visit_stmt_import(&mut self, node: ast::StmtImport) {
        for alias in node.names {
            if alias.name.as_str() == "sqlite3" {
                self.modules
                    .insert(alias.asname.unwrap_or(alias.name).to_string());
            }
        }
    }

    fn visit_stmt_import_from(&mut self, node: ast::StmtImportFrom) {
        if node
            .module
            .as_ref()
            .is_some_and(|module| module.as_str() == "sqlite3")
        {
            for alias in node.names {
                if alias.name.as_str() == "connect" {
                    self.connectors
                        .insert(alias.asname.unwrap_or(alias.name).to_string());
                }
            }
        }
    }

    fn visit_stmt_assign(&mut self, node: ast::StmtAssign) {
        let start = u32::from(node.start());
        let index = self.calls.len();
        let supported = matches!(&*node.value, ast::Expr::Call(call) if head(&call.func).is_some());
        self.visit_expr(*node.value);
        if supported {
            let targets: Vec<_> = node
                .targets
                .iter()
                .filter_map(name)
                .map(str::to_owned)
                .collect();
            if !targets.is_empty() {
                self.bindings.push(Bind {
                    start,
                    targets,
                    source: Binding::Assignment(index),
                });
            }
        }
        for target in node.targets {
            self.visit_expr(target);
        }
    }

    fn visit_withitem(&mut self, node: ast::WithItem) {
        let start = u32::from(node.context_expr.start());
        let index = self.calls.len();
        self.visit_expr(node.context_expr);
        if let Some(target) = node.optional_vars {
            if let Some(name) = name(&target) {
                self.bindings.push(Bind {
                    start,
                    targets: vec![name.into()],
                    source: Binding::With(index..self.calls.len()),
                });
            }
            self.visit_expr(*target);
        }
    }

    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        if let Some((receiver, function)) = head(&node.func) {
            let value = node.args.first().and_then(literal);
            let database = if node.args.is_empty() {
                node.keywords
                    .iter()
                    .find(|key| {
                        key.arg
                            .as_ref()
                            .is_some_and(|arg| arg.as_str() == "database")
                    })
                    .and_then(|key| literal(&key.value))
            } else {
                value
            };
            self.calls.push(Call {
                start: u32::from(node.start()),
                receiver: receiver.map(str::to_owned),
                function: function.into(),
                literal: value.map(str::to_owned),
                database: database.and_then(database_path).map(str::to_owned),
            });
        }
        self.generic_visit_expr_call(node);
    }

    fn visit_keyword(&mut self, node: ast::Keyword) {
        self.visit_expr(node.value);
    }

    fn visit_comprehension(&mut self, node: ast::Comprehension) {
        self.visit_expr(node.target);
        self.visit_expr(node.iter);
        for condition in node.ifs {
            self.visit_expr(condition);
        }
    }

    fn visit_arguments(&mut self, node: ast::Arguments) {
        for parameter in node
            .posonlyargs
            .into_iter()
            .chain(node.args)
            .chain(node.kwonlyargs)
        {
            self.visit_arg(parameter.def);
            if let Some(default) = parameter.default {
                self.visit_expr(*default);
            }
        }
        for parameter in node.vararg.into_iter().chain(node.kwarg) {
            self.visit_arg(*parameter);
        }
    }

    fn visit_arg(&mut self, node: ast::Arg) {
        if let Some(annotation) = node.annotation {
            self.visit_expr(*annotation);
        }
    }

    fn visit_match_case(&mut self, node: ast::MatchCase) {
        self.visit_pattern(node.pattern);
        if let Some(guard) = node.guard {
            self.visit_expr(*guard);
        }
        for stmt in node.body {
            self.visit_stmt(stmt);
        }
    }
}

impl Scan {
    fn connection(&self, call: &Call) -> bool {
        match call.receiver.as_deref() {
            Some(receiver) => call.function == "connect" && self.modules.contains(receiver),
            None => self.connectors.contains(&call.function),
        }
    }

    fn operations(mut self) -> Vec<Operation> {
        self.bindings.sort_by_key(|binding| binding.start);
        let mut databases = HashMap::<&str, Option<&str>>::new();
        for binding in &self.bindings {
            match &binding.source {
                Binding::Assignment(index) => {
                    let call = &self.calls[*index];
                    if self.connection(call) {
                        let database = call.database.as_deref();
                        for name in &binding.targets {
                            databases
                                .entry(name)
                                .and_modify(|previous| {
                                    if *previous != database {
                                        *previous = None;
                                    }
                                })
                                .or_insert(database);
                        }
                    } else if call.function == "cursor" {
                        if let Some(database) = call
                            .receiver
                            .as_deref()
                            .and_then(|name| databases.get(name))
                            .copied()
                        {
                            for name in &binding.targets {
                                databases.insert(name, database);
                            }
                        }
                    }
                }
                Binding::With(range) => {
                    let mut connects = self.calls[range.clone()]
                        .iter()
                        .filter(|call| self.connection(call));
                    if let Some(call) = connects.next() {
                        if connects.next().is_none() {
                            for name in &binding.targets {
                                databases.insert(name, call.database.as_deref());
                            }
                        }
                    }
                }
            }
        }
        let mut calls: Vec<_> = self.calls.iter().collect();
        calls.sort_by_key(|call| call.start);
        let mut result = Vec::new();
        for call in calls {
            if self.connection(call) {
                result.push(Operation::new(
                    call.database.as_deref(),
                    "CONNECT",
                    Kind::Open,
                ));
            } else if let Some(database) = call
                .receiver
                .as_deref()
                .and_then(|name| databases.get(name))
                .copied()
            {
                match call.function.as_str() {
                    "execute" | "executemany" | "executescript" => {
                        if let Some(query) = &call.literal {
                            result.extend(statements(query, database));
                        }
                    }
                    "commit" => result.push(Operation::new(database, "COMMIT", Kind::Transaction)),
                    "rollback" => {
                        result.push(Operation::new(database, "ROLLBACK", Kind::Transaction))
                    }
                    "close" => result.push(Operation::new(database, "CLOSE", Kind::Close)),
                    _ => {}
                }
            }
            if result.len() >= EVENT_LIMIT {
                result.truncate(EVENT_LIMIT);
                break;
            }
        }
        result
    }
}
