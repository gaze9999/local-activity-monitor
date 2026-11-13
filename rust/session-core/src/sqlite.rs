//! Classify SQLite observations from recorded arguments. This module never executes SQL.
mod diagnostic;
mod python;

pub use diagnostic::{project_diagnostic, SqlDiagnostic};

use crate::{commands::shell_parts, database_path};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::LazyLock;

const COMMAND_LIMIT: usize = 65_536;
const EVENT_LIMIT: usize = 80;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SqlMetadata(pub Vec<SqlEvent>);

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlEvent {
    pub statement: String,
    pub operation: Kind,
    pub database: Option<String>,
    pub tool: String,
    pub nested: bool,
    pub workdir: Option<String>,
    pub recognition: Recognition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Open,
    Close,
    Read,
    Write,
    Schema,
    Pragma,
    Maintenance,
    Transaction,
    Attach,
    Detach,
    Operation,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Recognition {
    McpCall,
    RecordedCode,
}

struct Operation {
    database: Option<String>,
    statement: String,
    kind: Kind,
    sql: Option<String>,
}

impl Operation {
    fn new(database: Option<&str>, statement: &str, kind: Kind) -> Self {
        Self {
            database: database.map(str::to_owned),
            statement: statement.into(),
            kind,
            sql: None,
        }
    }
}

static SQL_HIDDEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r#"'(?:''|[^'])*'|"(?:""|[^"])*"|`(?:``|[^`])*`|\[(?:\]\]|[^\]])*\]|--[^\n]*|/\*[\s\S]*?\*/"#
).expect("SQL comments and quoted values")
});
static SQL_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_]+|[()]").expect("SQL token"));
static HERE_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@['"]\r?\n([\s\S]*?)\r?\n['"]@"#).expect("PowerShell here-string")
});
static HERE_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<<['"]?(\w+)['"]?\r?\n"#).expect("shell heredoc"));
static PYTHON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^python(?:3(?:\.[0-9]+)?)?(?:\.exe)?$").expect("Python command"));

impl SqlMetadata {
    pub fn observe(&mut self, tool: &str, args: Option<&Map<String, Value>>, nested: bool) {
        if self.0.len() >= EVENT_LIMIT {
            return;
        }
        let Some(args) = args else { return };
        let sqlite = tool.to_ascii_lowercase().contains("sqlite");
        let events = observed_operations(tool, args);
        let workdir = args
            .get("workdir")
            .and_then(Value::as_str)
            .and_then(database_path);
        let recognition = if sqlite && !nested {
            Recognition::McpCall
        } else {
            Recognition::RecordedCode
        };
        self.0.extend(
            events
                .into_iter()
                .take(EVENT_LIMIT - self.0.len())
                .map(|event| SqlEvent {
                    statement: event.statement,
                    operation: event.kind,
                    database: event.database,
                    tool: tool.into(),
                    nested,
                    workdir: workdir.map(str::to_owned),
                    recognition,
                }),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn valid(&self) -> bool {
        self.0.len() <= EVENT_LIMIT
            && self.0.iter().all(|event| {
                let sqlite = event.tool.to_ascii_lowercase().contains("sqlite");
                let tool = (1..=160).contains(&event.tool.len())
                    && event
                        .tool
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte));
                let recognition = if sqlite && !event.nested {
                    Recognition::McpCall
                } else {
                    Recognition::RecordedCode
                };
                tool && (sqlite
                    || matches!(
                        event.tool.as_str(),
                        "exec_command" | "functions.exec_command"
                    ))
                    && event.recognition == recognition
                    && (kind(&event.statement) == Some(event.operation)
                        || sqlite
                            && event.operation == Kind::Operation
                            && event.tool.rsplit("__").next() == Some(&event.statement))
                    && [&event.database, &event.workdir].iter().all(|value| {
                        value
                            .as_deref()
                            .is_none_or(|path| database_path(path) == Some(path))
                    })
            })
    }
}

fn observed_operations(tool: &str, args: &Map<String, Value>) -> Vec<Operation> {
    let sqlite = tool.to_ascii_lowercase().contains("sqlite");
    let events = if sqlite {
        let database = ["database", "db_path", "database_path", "path"]
            .iter()
            .find_map(|key| args.get(*key))
            .and_then(Value::as_str)
            .and_then(database_path);
        let query = args
            .get("query")
            .or_else(|| args.get("sql"))
            .and_then(Value::as_str);
        let mut events = query.map_or_else(Vec::new, |query| statements(query, database));
        if events.is_empty() {
            events.push(Operation::new(
                database,
                tool.rsplit("__").next().unwrap_or(tool),
                Kind::Operation,
            ));
        }
        events
    } else if matches!(tool, "exec_command" | "functions.exec_command") {
        let Some(command) = args.get("cmd").and_then(Value::as_str).filter(|command| {
            command.chars().take(COMMAND_LIMIT + 1).count() <= COMMAND_LIMIT
                && command.contains("sqlite")
        }) else {
            return Vec::new();
        };
        shell_operations(command)
    } else {
        return Vec::new();
    };
    events
}

/// Extract transient SQL details using the same classifier as stored metadata.
pub fn sqlite_details(
    tool: &str,
    args: Option<&Map<String, Value>>,
) -> Vec<(String, Option<String>)> {
    let Some(args) = args else { return Vec::new() };
    observed_operations(tool, args)
        .into_iter()
        .take(EVENT_LIMIT)
        .map(|item| (item.statement, item.sql))
        .collect()
}

fn shell_operations(command: &str) -> Vec<Operation> {
    let mut result = Vec::new();
    let mut candidates = vec![command];
    candidates.extend(
        HERE_STRING
            .captures_iter(command)
            .filter_map(|captures| captures.get(1).map(|body| body.as_str())),
    );
    candidates.extend(HERE_START.captures_iter(command).filter_map(|captures| {
        let name = captures.get(1)?.as_str();
        let tail = &command[captures.get(0)?.end()..];
        let mut offset = 0;
        for line in tail.split_inclusive('\n') {
            if line.trim_end_matches(['\r', '\n']) == name {
                return Some(tail[..offset].trim_end_matches(['\r', '\n']));
            }
            offset += line.len();
        }
        None
    }));
    let mut inline = Vec::new();
    for part in shell_parts(command) {
        let Some(tokens) = shlex::split(part) else {
            continue;
        };
        for (index, token) in tokens.iter().enumerate() {
            let binary = token
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(token)
                .to_ascii_lowercase();
            let args = &tokens[index + 1..];
            if PYTHON.is_match(&binary) && args.first().map(String::as_str) == Some("-c") {
                if let Some(code) = args.get(1) {
                    inline.push(code.clone());
                }
            }
            if !matches!(binary.as_str(), "sqlite3" | "sqlite3.exe") {
                continue;
            }
            let start = args
                .iter()
                .take_while(|value| {
                    matches!(
                        value.as_str(),
                        "-readonly"
                            | "-readwrite"
                            | "-batch"
                            | "-json"
                            | "-csv"
                            | "-header"
                            | "-noheader"
                    )
                })
                .count();
            let Some(db) = args.get(start).filter(|arg| !arg.starts_with('-')) else {
                continue;
            };
            let database = database_path(db);
            result.push(Operation::new(database, "CONNECT", Kind::Open));
            if let Some(query) = args.get(start + 1) {
                result.extend(statements(query, database));
            }
            if result.len() >= EVENT_LIMIT {
                result.truncate(EVENT_LIMIT);
                return result;
            }
        }
    }
    let mut seen = HashSet::new();
    for code in candidates
        .into_iter()
        .chain(inline.iter().map(String::as_str))
    {
        if seen.insert(code) {
            result.extend(python::operations(code));
        }
        if result.len() >= EVENT_LIMIT {
            break;
        }
    }
    result.truncate(EVENT_LIMIT);
    result
}

fn kind(statement: &str) -> Option<Kind> {
    Some(match statement {
        "CONNECT" => Kind::Open,
        "CLOSE" => Kind::Close,
        "SELECT" | "EXPLAIN" => Kind::Read,
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" => Kind::Write,
        "CREATE" | "ALTER" | "DROP" => Kind::Schema,
        "PRAGMA" => Kind::Pragma,
        "VACUUM" | "ANALYZE" | "REINDEX" => Kind::Maintenance,
        "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" => Kind::Transaction,
        "ATTACH" => Kind::Attach,
        "DETACH" => Kind::Detach,
        _ => return None,
    })
}

fn statements(sql: &str, database: Option<&str>) -> Vec<Operation> {
    let clean = SQL_HIDDEN.replace_all(sql, |captures: &regex::Captures<'_>| {
        " ".repeat(captures[0].len())
    });
    let mut offset = 0;
    clean
        .split(';')
        .take(EVENT_LIMIT)
        .filter_map(|statement| {
            let start = offset;
            offset += statement.len() + 1;
            let original = sql.get(start..start + statement.len())?.trim();
            let mut tokens = SQL_TOKEN.find_iter(statement);
            let first = tokens.next()?.as_str().to_ascii_uppercase();
            let (name, kind) = if !matches!(first.as_str(), "WITH" | "CONNECT" | "CLOSE") {
                let kind = kind(&first)?;
                (first, kind)
            } else {
                if first != "WITH" {
                    return None;
                }
                let mut depth = 0_i32;
                let mut found = None;
                for token in tokens {
                    match token.as_str() {
                        "(" => depth += 1,
                        ")" => depth -= 1,
                        word if depth == 0 => {
                            let word = word.to_ascii_uppercase();
                            if let Some(kind) =
                                kind(&word).filter(|kind| !matches!(kind, Kind::Open | Kind::Close))
                            {
                                found = Some((word, kind));
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                found?
            };
            let mut result = Operation::new(database, &name, kind);
            result.sql = Some(original.to_owned());
            Some(result)
        })
        .collect()
}

/// Report whether recorded text contains a recognized SQL statement.
pub fn recognized_sql(sql: &str) -> bool {
    !statements(sql, None).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transient_details_match_statement_order_without_splitting_quoted_semicolons() {
        let args = json!({"query":"SELECT 'one;two'; UPDATE users SET password='PRIVATE'"});
        let details = sqlite_details("mcp__sqlite__query", args.as_object());
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].0, "SELECT");
        assert_eq!(details[0].1.as_deref(), Some("SELECT 'one;two'"));
        assert_eq!(details[1].0, "UPDATE");
        assert!(details[1].1.as_deref().unwrap().contains("PRIVATE"));
        let mut metadata = SqlMetadata::default();
        metadata.observe("mcp__sqlite__query", args.as_object(), false);
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("PRIVATE"));
    }

    #[test]
    fn classifies_ctes_and_excludes_sql_values_from_metadata() {
        let args = json!({
            "database":"file:demo.db?password=PRIVATE_TOKEN",
            "query":"WITH a AS (SELECT 'PRIVATE') UPDATE demo SET field='PRIVATE; DROP TABLE demo'; SELECT 1; -- DELETE\nPRAGMA query_only=ON"
        });
        let mut metadata = SqlMetadata::default();
        metadata.observe("mcp__sqlite__query", args.as_object(), false);
        assert_eq!(
            metadata
                .0
                .iter()
                .map(|row| row.statement.as_str())
                .collect::<Vec<_>>(),
            ["UPDATE", "SELECT", "PRAGMA"]
        );
        assert!(metadata
            .0
            .iter()
            .all(|row| row.database.as_deref() == Some("file:demo.db")
                && row.recognition == Recognition::McpCall));
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("PRIVATE"));
        assert!(statements("documentation mentions SELECT", None).is_empty());
        assert!(metadata.valid());
    }

    #[test]
    fn classifies_cli_and_python_without_executing_dynamic_arguments() {
        let mut metadata = SqlMetadata::default();
        let command = json!({"cmd":r#"sqlite3 -readonly "demo.db" "SELECT 1; SELECT 2""#});
        metadata.observe("exec_command", command.as_object(), false);
        let code = "from sqlite3 import connect as connect_db\nwith closing(connect_db(database='fixture.db')) as conn:\n    cur = conn.cursor()\n    cur.execute('SELECT PRIVATE_SQL')\n    cur.execute(dynamic)\n    conn.commit()\n    conn.close()";
        let args = json!({"cmd":format!("@'\n{code}\n'@ | python -"),"workdir":"C:/fixture"});
        metadata.observe("exec_command", args.as_object(), true);
        assert_eq!(
            metadata
                .0
                .iter()
                .map(|row| row.statement.as_str())
                .collect::<Vec<_>>(),
            ["CONNECT", "SELECT", "SELECT", "CONNECT", "SELECT", "COMMIT", "CLOSE"]
        );
        assert!(metadata.0[3..].iter().all(|row| row.nested
            && row.database.as_deref() == Some("fixture.db")
            && row.recognition == Recognition::RecordedCode));
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("PRIVATE"));
        assert!(metadata.valid());
    }

    #[test]
    fn stops_at_limits_and_preserves_unknown_mcp_operations() {
        let mut metadata = SqlMetadata::default();
        let args = json!({"database":"https://private.example/path","sql":"SELECT 1;".repeat(100)});
        metadata.observe("mcp__sqlite__read", args.as_object(), false);
        assert_eq!(metadata.0.len(), 80);
        assert!(metadata.0.iter().all(|row| row.database.is_none()));

        let mut metadata = SqlMetadata::default();
        let args = json!({"path":"demo.db"});
        metadata.observe("mcp__sqlite__unknown", args.as_object(), false);
        assert_eq!(metadata.0[0].statement, "unknown");
        assert_eq!(metadata.0[0].operation, Kind::Operation);
        assert!(metadata.valid());
    }
}
