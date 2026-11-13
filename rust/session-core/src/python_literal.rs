//! Parse Python literal syntax into data without evaluating expressions.
use rustpython_ast::{self as ast, Constant, Expr, UnaryOp};
use rustpython_parser::{lexer, Mode, Parse, Tok};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

const NODE_LIMIT: usize = 20_000;
const DEPTH_LIMIT: usize = 64;
const INTEGER_DIGITS: usize = 4300;

fn excessive_integer(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                let triple =
                    bytes.get(index + 1) == Some(&quote) && bytes.get(index + 2) == Some(&quote);
                index += if triple { 3 } else { 1 };
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                        continue;
                    }
                    if bytes[index] == quote
                        && (!triple
                            || bytes.get(index + 1) == Some(&quote)
                                && bytes.get(index + 2) == Some(&quote))
                    {
                        index += if triple { 3 } else { 1 };
                        break;
                    }
                    index += 1;
                }
            }
            b'#' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            byte if byte.is_ascii_digit() => {
                let start = index;
                if start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric()
                        || matches!(bytes[start - 1], b'_' | b'.')
                        || (matches!(bytes[start - 1], b'+' | b'-')
                            && start > 1
                            && matches!(bytes[start - 2], b'e' | b'E')))
                {
                    index += 1;
                    while bytes
                        .get(index)
                        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
                    {
                        index += 1;
                    }
                    continue;
                }
                let base = if byte == b'0' {
                    match bytes.get(index + 1).copied() {
                        Some(b'x' | b'X') => 16,
                        Some(b'o' | b'O') => 8,
                        Some(b'b' | b'B') => 2,
                        _ => 10,
                    }
                } else {
                    10
                };
                if base != 10 {
                    index += 2;
                    while bytes
                        .get(index)
                        .is_some_and(|byte| byte.is_ascii_hexdigit() || *byte == b'_')
                    {
                        index += 1;
                    }
                } else {
                    while bytes
                        .get(index)
                        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
                    {
                        index += 1;
                    }
                }
                let floating = base == 10
                    && bytes
                        .get(index)
                        .is_some_and(|byte| matches!(*byte, b'.' | b'e' | b'E' | b'j' | b'J'));
                if !floating {
                    let digits = if base == 10 {
                        bytes[start..index]
                            .iter()
                            .filter(|byte| **byte != b'_')
                            .count()
                    } else {
                        bytes[start + 2..index]
                            .iter()
                            .filter(|byte| **byte != b'_')
                            .skip_while(|byte| **byte == b'0')
                            .count()
                    };
                    let limit = match base {
                        16 => 3600,
                        8 => 4800,
                        2 => 14400,
                        _ => INTEGER_DIGITS,
                    };
                    if digits > limit {
                        return true;
                    }
                }
            }
            _ => index += 1,
        }
    }
    false
}

#[derive(Clone)]
enum Scalar {
    None,
    Bool(bool),
    String(String),
    Int(String),
    Float(f64),
}
#[derive(Hash, Eq, PartialEq)]
enum Identity {
    None,
    String(String),
    Integer(String),
    Float(u64),
}
fn integer(value: &ast::bigint::BigInt) -> Option<String> {
    let value = value.to_string();
    (value.trim_start_matches('-').len() <= INTEGER_DIGITS).then_some(value)
}
fn scalar(constant: &Constant) -> Option<Scalar> {
    match constant {
        Constant::None => Some(Scalar::None),
        Constant::Bool(value) => Some(Scalar::Bool(*value)),
        Constant::Str(value) => Some(Scalar::String(value.clone())),
        Constant::Int(value) => Some(Scalar::Int(integer(value)?)),
        Constant::Float(value) => Some(Scalar::Float(*value)),
        _ => None,
    }
}
fn float_text(value: f64) -> String {
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .into();
    }
    let text = format!("{value:?}");
    if let Some((base, exponent)) = text.split_once('e') {
        if let Ok(number) = exponent.parse::<i32>() {
            return format!(
                "{base}e{}{number:02}",
                if number >= 0 { '+' } else { '-' },
                number = number.unsigned_abs()
            );
        }
    }
    text
}
impl Scalar {
    fn identity(&self) -> Identity {
        match self {
            Self::None => Identity::None,
            Self::String(value) => Identity::String(value.clone()),
            Self::Bool(value) => Identity::Integer(if *value { "1" } else { "0" }.into()),
            Self::Int(value) => Identity::Integer(value.clone()),
            Self::Float(value) if value.is_finite() && value.fract() == 0.0 => {
                Identity::Integer(if *value == 0.0 {
                    "0".into()
                } else {
                    format!("{value:.0}")
                })
            }
            Self::Float(value) => Identity::Float(value.to_bits()),
        }
    }
    fn label(&self) -> String {
        match self {
            Self::None => "None".into(),
            Self::Bool(value) => if *value { "True" } else { "False" }.into(),
            Self::String(value) | Self::Int(value) => value.clone(),
            Self::Float(value) => float_text(*value),
        }
    }
    fn value(self) -> Option<Value> {
        match self {
            Self::None => Some(Value::Null),
            Self::Bool(value) => Some(Value::Bool(value)),
            Self::String(value) => Some(Value::String(value)),
            Self::Int(value) => serde_json::from_str::<Value>(&value).ok(),
            Self::Float(value) => Some(Number::from_f64(value).map_or(Value::Null, Value::Number)),
        }
    }
}
fn scalar_expr(expr: &Expr) -> Option<Scalar> {
    match expr {
        Expr::Constant(value) => scalar(&value.value),
        Expr::UnaryOp(value) => {
            let Expr::Constant(inner) = value.operand.as_ref() else {
                return None;
            };
            match (value.op, scalar(&inner.value)?) {
                (UnaryOp::UAdd, number @ (Scalar::Int(_) | Scalar::Float(_))) => Some(number),
                (UnaryOp::USub, Scalar::Int(value)) => Some(Scalar::Int(if value == "0" {
                    value
                } else if let Some(value) = value.strip_prefix('-') {
                    value.into()
                } else {
                    format!("-{value}")
                })),
                (UnaryOp::USub, Scalar::Float(value)) => Some(Scalar::Float(-value)),
                _ => None,
            }
        }
        _ => None,
    }
}
fn convert(expr: &Expr, depth: usize, nodes: &mut usize) -> Option<Value> {
    *nodes += 1;
    if *nodes > NODE_LIMIT || depth > DEPTH_LIMIT {
        return None;
    }
    match expr {
        Expr::Constant(value) => {
            if let Constant::Tuple(values) = &value.value {
                return Some(Value::Array(
                    values
                        .iter()
                        .map(|value| scalar(value)?.value())
                        .collect::<Option<_>>()?,
                ));
            }
            scalar(&value.value)?.value()
        }
        Expr::UnaryOp(value) => {
            *nodes += 2;
            if *nodes > NODE_LIMIT || !matches!(value.op, UnaryOp::UAdd | UnaryOp::USub) {
                return None;
            }
            scalar_expr(expr)?.value()
        }
        Expr::List(value) => {
            *nodes += 1;
            if *nodes > NODE_LIMIT {
                return None;
            }
            Some(Value::Array(
                value
                    .elts
                    .iter()
                    .map(|item| convert(item, depth + 1, nodes))
                    .collect::<Option<_>>()?,
            ))
        }
        Expr::Tuple(value) => {
            *nodes += 1;
            if *nodes > NODE_LIMIT {
                return None;
            }
            Some(Value::Array(
                value
                    .elts
                    .iter()
                    .map(|item| convert(item, depth + 1, nodes))
                    .collect::<Option<_>>()?,
            ))
        }
        Expr::Dict(value) => {
            let mut positions = HashMap::<Identity, usize>::new();
            let mut pairs = Vec::<(String, Value)>::new();
            for (key, value) in value.keys.iter().zip(&value.values) {
                let key = key.as_ref()?;
                *nodes += 1;
                if matches!(key, Expr::UnaryOp(_)) {
                    *nodes += 2;
                }
                if *nodes > NODE_LIMIT {
                    return None;
                }
                let key = scalar_expr(key)?;
                let result = convert(value, depth + 1, nodes)?;
                let identity = key.identity();
                if let Some(index) = positions.get(&identity) {
                    pairs[*index].1 = result;
                } else {
                    positions.insert(identity, pairs.len());
                    pairs.push((key.label(), result));
                }
            }
            let mut output = Map::new();
            for (key, value) in pairs {
                output.insert(key, value);
            }
            Some(Value::Object(output))
        }
        _ => None,
    }
}

/// Extract a module's first literal string without executing the module.
pub fn python_module_docstring(source: &str) -> Option<String> {
    if source.len() > 65536 || excessive_integer(source) {
        return None;
    }
    let mut depth = 0_usize;
    for token in lexer::lex(source, Mode::Module) {
        match token.ok()?.0 {
            Tok::Lpar | Tok::Lsqb | Tok::Lbrace => {
                depth += 1;
                if depth > 200 {
                    return None;
                }
            }
            Tok::Rpar | Tok::Rsqb | Tok::Rbrace => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let statements = ast::Suite::parse(source, "<introduction>").ok()?;
    let ast::Stmt::Expr(statement) = statements.first()? else {
        return None;
    };
    let Expr::Constant(value) = statement.value.as_ref() else {
        return None;
    };
    let Constant::Str(text) = &value.value else {
        return None;
    };
    (!text.is_empty()).then(|| text.clone())
}

pub fn parse_python_literal(source: &str) -> Option<Value> {
    if source.len() > crate::LINE_LIMIT || excessive_integer(source) {
        return None;
    }
    let mut depth = 0usize;
    for token in lexer::lex(source, Mode::Expression) {
        match token.ok()?.0 {
            Tok::Lpar | Tok::Lsqb | Tok::Lbrace => {
                depth += 1;
                if depth > 200 {
                    return None;
                }
            }
            Tok::Rpar | Tok::Rsqb | Tok::Rbrace => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    let expr = Expr::parse(source, "<recorded>").ok()?;
    let mut nodes = 1;
    let value = convert(&expr, 0, &mut nodes)?;
    (value.is_object() || value.is_array()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn module_docstring_requires_valid_source_and_literal_first_statement() {
        assert_eq!(
            python_module_docstring("# comment\n'''Purpose\n  details'''\nvalue = 1\n").as_deref(),
            Some("Purpose\n  details")
        );
        assert!(python_module_docstring("value = 'not a docstring'\n").is_none());
        assert!(python_module_docstring("f'{danger()}'\n").is_none());
        assert!(python_module_docstring("'''valid text'''\nif\n").is_none());
    }

    #[test]
    fn python_literals_preserve_data_and_reject_expressions() {
        assert_eq!(
            parse_python_literal("{'ok': True, 'value': None, 'items': (0, False)}"),
            Some(json!({"ok":true,"value":null,"items":[0,false]}))
        );
        assert_eq!(
            parse_python_literal("{True: 'first', 1: 'second', 'True': 'third', -2: +1.5}"),
            Some(json!({"True":"third","-2":1.5}))
        );
        assert_eq!(
            parse_python_literal(r#"{'escaped': '\u0041', 'raw': r'\n', 'hex': 0x10}"#),
            Some(json!({"escaped":"A","raw":"\\n","hex":16}))
        );
        assert_eq!(
            parse_python_literal("('one', 2, None)"),
            Some(json!(["one", 2, null]))
        );
        assert_eq!(
            parse_python_literal("{-0.0:'first',0:'second',1e20:'large',1e-5:'small'}"),
            Some(json!({"-0.0":"second","1e+20":"large","1e-05":"small"}))
        );
        for value in [
            "{'value': run()}",
            "{'value': variable}",
            "{'value': __import__('os').system('anything')}",
            "{'value': b'bytes'}",
            "{('tuple',): 1}",
            "{1, 2}",
        ] {
            assert_eq!(parse_python_literal(value), None, "{value}");
        }
    }
    #[test]
    fn python_literals_bound_depth_nodes_and_numeric_conversion() {
        let deep = format!("{}0{}", "[".repeat(201), "]".repeat(201));
        assert!(parse_python_literal(&deep).is_none());
        let many = format!("[{}]", "0,".repeat(20_000));
        assert!(parse_python_literal(&many).is_none());
        let large = format!("[{}]", "9".repeat(4301));
        assert!(parse_python_literal(&large).is_none());
        let text = format!("['{}']", "9".repeat(5000));
        assert!(parse_python_literal(&text).is_some());
        let fractional = format!("[0.{}]", "9".repeat(5000));
        assert!(parse_python_literal(&fractional).is_some());
    }
}
