//! Bounded parsing of recorded data, without evaluation or external references.
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

const LIMIT: usize = 1024 * 1024;
const LINES: usize = 20_000;
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$").unwrap());
static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+#").unwrap());
static TOML_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[[^\n]+\]|[A-Za-z_]\w*\s*=)").unwrap());

pub(super) fn parse(source: &str) -> Option<Value> {
    let source = source.trim();
    if source.len() > LIMIT {
        return None;
    }
    if source.starts_with(['{', '[']) {
        if let Ok(value) = serde_json::from_str::<Value>(source) {
            if value.is_object() || value.is_array() {
                return Some(value);
            }
        }
    }
    if source.starts_with(['{', '[', '(']) {
        if let Some(value) = lam_session_core::parse_python_literal(source) {
            return Some(value);
        }
        if source.starts_with(['{', '[']) {
            if let Some(value) = lam_session_core::parse_data_literal(source) {
                return Some(value);
            }
        }
    }
    let lines: Vec<_> = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() > 1
        && lines
            .iter()
            .all(|line| line.trim_start().starts_with(['{', '[']))
    {
        if let Some(values) = lines
            .iter()
            .map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Option<Vec<_>>>()
        {
            return Some(Value::Array(values));
        }
        return yaml_candidate(source);
    }
    if source.starts_with('<')
        && !source.to_ascii_lowercase().contains("<!doctype")
        && !source.to_ascii_lowercase().contains("<!entity")
    {
        return match roxmltree::Document::parse(source) {
            Ok(document) => Some(xml_element(document.root_element(), 0)),
            Err(_) => yaml_candidate(source),
        };
    }
    if TOML_START.is_match(source) {
        match toml::from_str::<toml::Value>(source) {
            Ok(value) if !valid_toml(&value) => return yaml_candidate(source),
            Ok(value) if value.as_table().is_some_and(|table| !table.is_empty()) => {
                return Some(toml_value(value, 0))
            }
            Err(_) => return yaml_candidate(source),
            _ => {}
        }
    }
    if lines.len() > 1 && (lines[0].contains(',') || lines[0].contains('\t')) {
        if let Some(dialect) = super::csv_dialect::sniff(source) {
            if let Some(value) = csv_value(source, &dialect) {
                return Some(value);
            }
        }
    }
    yaml_candidate(source)
}

pub(super) fn parse_document(source: &str, format: &str) -> Option<(Value, &'static str)> {
    let source = source.trim();
    if source.len() > LIMIT {
        return None;
    }
    if source.starts_with(['{', '[']) {
        if let Ok(value) = serde_json::from_str::<Value>(source) {
            if value.is_object() || value.is_array() {
                return Some((value, "JSON"));
            }
        }
    }
    if source.starts_with(['{', '[', '(']) {
        if let Some(value) = lam_session_core::parse_python_literal(source) {
            return Some((value, "Python literal"));
        }
        if source.starts_with(['{', '[']) {
            if let Some(value) = lam_session_core::parse_data_literal(source) {
                return Some((value, "Object literal"));
            }
        }
    }
    let lines: Vec<_> = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() > LINES {
        return None;
    }
    if lines.len() > 1
        && lines
            .iter()
            .all(|line| line.trim_start().starts_with(['{', '[']))
    {
        if let Some(values) = lines
            .iter()
            .map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Option<Vec<_>>>()
        {
            return Some((Value::Array(values), "JSONL"));
        }
    }
    if source.starts_with('<')
        && !source.to_ascii_lowercase().contains("<!doctype")
        && !source.to_ascii_lowercase().contains("<!entity")
    {
        if let Ok(document) = roxmltree::Document::parse(source) {
            return Some((xml_element(document.root_element(), 0), "XML"));
        }
    }
    if format.eq_ignore_ascii_case("toml") || TOML_START.is_match(source) {
        if let Ok(value) = toml::from_str::<toml::Value>(source) {
            if valid_toml(&value) && value.as_table().is_some_and(|table| !table.is_empty()) {
                return Some((toml_value(value, 0), "TOML"));
            }
        }
    }
    if lines.len() > 1
        && (matches!(format, "csv" | "tsv") || lines[0].contains(',') || lines[0].contains('\t'))
    {
        if let Some(dialect) = super::csv_dialect::sniff(source) {
            if let Some(value) = csv_value(source, &dialect) {
                return Some((
                    value,
                    if dialect.delimiter == b'\t' {
                        "TSV"
                    } else {
                        "CSV"
                    },
                ));
            }
        }
    }
    if matches!(format, "yaml" | "yml") {
        return yaml(source).map(|value| (value, "YAML subset"));
    }
    yaml_candidate(source).map(|value| (value, "YAML subset"))
}

fn yaml_candidate(source: &str) -> Option<Value> {
    if source.lines().count() > 1 {
        let first = source.lines().next().unwrap_or("").trim_start();
        if first.starts_with("- ") || pair(first).is_some() {
            return yaml(source);
        }
    }
    None
}

fn xml_element(node: roxmltree::Node<'_, '_>, depth: usize) -> Value {
    if depth > 64 {
        return json!("[內容截斷]");
    }
    let name = |namespace: Option<&str>, local: &str| {
        namespace.map_or_else(
            || local.to_owned(),
            |namespace| format!("{{{namespace}}}{local}"),
        )
    };
    let mut item = Map::new();
    let attributes: Map<String, Value> = node
        .attributes()
        .map(|attribute| {
            (
                name(attribute.namespace(), attribute.name()),
                json!(attribute.value()),
            )
        })
        .collect();
    if !attributes.is_empty() {
        item.insert("attributes".into(), Value::Object(attributes));
    }
    if let Some(text) = node.text().map(str::trim).filter(|text| !text.is_empty()) {
        item.insert("text".into(), json!(text));
    }
    let children: Vec<_> = node
        .children()
        .filter(|child| child.is_element())
        .map(|child| xml_element(child, depth + 1))
        .collect();
    if !children.is_empty() {
        item.insert("children".into(), Value::Array(children));
    }
    let mut result = Map::new();
    result.insert(
        name(node.tag_name().namespace(), node.tag_name().name()),
        Value::Object(item),
    );
    Value::Object(result)
}

fn toml_date(value: toml::value::Datetime) -> String {
    let mut result = String::new();
    if let Some(date) = value.date {
        result.push_str(&format!(
            "{:04}-{:02}-{:02}",
            date.year, date.month, date.day
        ));
    }
    if let Some(time) = value.time {
        if value.date.is_some() {
            result.push('T');
        }
        result.push_str(&format!(
            "{:02}:{:02}:{:02}",
            time.hour, time.minute, time.second
        ));
        let micros = time.nanosecond / 1000;
        if micros > 0 {
            result.push_str(&format!(".{micros:06}"));
        }
    }
    if let Some(offset) = value.offset {
        let minutes = match offset {
            toml::value::Offset::Z => 0,
            toml::value::Offset::Custom { minutes } => minutes,
        };
        result.push_str(&format!(
            "{}{:02}:{:02}",
            if minutes < 0 { '-' } else { '+' },
            minutes.unsigned_abs() / 60,
            minutes.unsigned_abs() % 60
        ));
    }
    result
}
fn valid_toml(value: &toml::Value) -> bool {
    match value {
        toml::Value::Datetime(value) => {
            let date = value.date.is_none_or(|date| {
                date.year > 0
                    && chrono::NaiveDate::from_ymd_opt(
                        i32::from(date.year),
                        u32::from(date.month),
                        u32::from(date.day),
                    )
                    .is_some()
            });
            let time = value.time.is_none_or(|time| {
                chrono::NaiveTime::from_hms_micro_opt(
                    u32::from(time.hour),
                    u32::from(time.minute),
                    u32::from(time.second),
                    time.nanosecond / 1000,
                )
                .is_some()
            });
            let offset = value.offset.is_none_or(|offset| match offset {
                toml::value::Offset::Z => true,
                toml::value::Offset::Custom { minutes } => minutes.unsigned_abs() < 1440,
            });
            date && time && offset
        }
        toml::Value::Array(values) => values.iter().all(valid_toml),
        toml::Value::Table(values) => values.values().all(valid_toml),
        _ => true,
    }
}
fn toml_value(value: toml::Value, depth: usize) -> Value {
    if depth > 64 {
        return json!("[內容過深]");
    }
    match value {
        toml::Value::String(value) => json!(value),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) => {
            if value.is_finite() {
                json!(value)
            } else {
                Value::Null
            }
        }
        toml::Value::Boolean(value) => json!(value),
        toml::Value::Datetime(value) => json!(toml_date(value)),
        toml::Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| toml_value(value, depth + 1))
                .collect(),
        ),
        toml::Value::Table(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, toml_value(value, depth + 1)))
                .collect(),
        ),
    }
}

fn csv_value(source: &str, dialect: &super::csv_dialect::Dialect) -> Option<Value> {
    let delimiter = dialect.delimiter;
    let quote = dialect.quote;
    let skip_initial_space = dialect.skip_initial_space;
    let mut normalized = Vec::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut quoted = false;
    let mut field_start = true;
    let mut line_nonempty = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if !quoted
            && ((byte == b'\n') || (byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')))
            && !line_nonempty
        {
            return None;
        }
        if !matches!(byte, b'\n' | b'\r') {
            line_nonempty = true;
        }
        if skip_initial_space && field_start && !quoted && byte == b' ' {
            index += 1;
            continue;
        }
        if byte == quote && (quoted || field_start) {
            if quoted && dialect.double_quote && bytes.get(index + 1) == Some(&quote) {
                normalized.push(byte);
                normalized.push(byte);
                index += 2;
                continue;
            }
            quoted = !quoted;
        }
        normalized.push(byte);
        index += 1;
        if !quoted && byte == delimiter {
            if skip_initial_space {
                while bytes.get(index) == Some(&b' ') {
                    index += 1;
                }
            }
            field_start = true;
        } else if !quoted && matches!(byte, b'\n' | b'\r') {
            field_start = true;
        } else {
            field_start = false;
        }
        if byte == b'\n' || (byte == b'\r' && bytes.get(index) != Some(&b'\n')) {
            line_nonempty = false;
        }
    }
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .quote(quote)
        .double_quote(dialect.double_quote)
        .has_headers(false)
        .from_reader(normalized.as_slice());
    let rows: Vec<Vec<String>> = reader
        .records()
        .map(|row| row.ok().map(|row| row.iter().map(str::to_owned).collect()))
        .collect::<Option<_>>()?;
    if rows.len() < 2
        || rows[0].len() < 2
        || rows[0].len() > 100
        || rows.iter().any(|row| row.len() != rows[0].len())
    {
        return None;
    }
    let headers = &rows[0];
    let unique = headers
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == headers.len();
    if !unique {
        return Some(json!(rows));
    }
    let values: Vec<Value> = rows
        .iter()
        .skip(1)
        .map(|row| {
            Value::Object(
                headers
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned().map(Value::String))
                    .collect(),
            )
        })
        .collect();
    Some(json!({"columns":headers,"rows":values}))
}

fn scalar(input: &str) -> Option<Value> {
    let value = input.trim();
    if value.starts_with(['!', '&', '*', '|', '>']) {
        return None;
    }
    if value.starts_with(['{', '[']) {
        return serde_json::from_str::<Value>(value)
            .ok()
            .filter(|value| value.is_object() || value.is_array())
            .or_else(|| lam_session_core::parse_data_literal(value));
    }
    if value.starts_with('"') {
        return serde_json::from_str::<String>(value)
            .ok()
            .map(Value::String);
    }
    if value.starts_with('\'') {
        return value
            .strip_prefix('\'')?
            .strip_suffix('\'')
            .map(|s| json!(s.replace("''", "'")));
    }
    let value = COMMENT
        .find(value)
        .map_or(value, |found| &value[..found.start()])
        .trim_end();
    if matches!(value, "null" | "Null" | "NULL" | "~") {
        return Some(Value::Null);
    }
    if value.eq_ignore_ascii_case("true") {
        return Some(json!(true));
    }
    if value.eq_ignore_ascii_case("false") {
        return Some(json!(false));
    }
    if NUMBER.is_match(value) {
        return serde_json::from_str(value).ok();
    }
    if value.contains(": ") || value.ends_with(':') {
        return None;
    }
    Some(json!(value))
}
fn pair(body: &str) -> Option<(String, &str)> {
    let (key, rest) = body.split_once(':')?;
    if key.is_empty()
        || key
            .chars()
            .any(|ch| matches!(ch, ':' | '{' | '}' | '[' | ']' | ','))
        || (!rest.is_empty() && !rest.starts_with(char::is_whitespace))
    {
        return None;
    }
    let key = match scalar(key)? {
        Value::String(value) => value,
        Value::Bool(value) => {
            if value {
                "True".into()
            } else {
                "False".into()
            }
        }
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if key == "<<" {
        return None;
    }
    Some((key, rest.trim_start()))
}
struct Yaml<'a> {
    lines: Vec<(usize, &'a str)>,
    position: usize,
}
impl Yaml<'_> {
    fn block(&mut self, indent: usize, depth: usize) -> Option<Value> {
        if depth > 12 {
            return None;
        }
        let sequence = self.lines.get(self.position)?.1 == "-"
            || self.lines.get(self.position)?.1.starts_with("- ");
        let mut list = Vec::new();
        let mut map = Map::new();
        while self.position < self.lines.len() && self.lines[self.position].0 == indent {
            let body = self.lines[self.position].1;
            if sequence {
                if body != "-" && !body.starts_with("- ") {
                    return None;
                }
                let content = body[1..].trim();
                self.position += 1;
                let child = if content.is_empty() {
                    if self.position < self.lines.len() && self.lines[self.position].0 > indent {
                        let next = self.lines[self.position].0;
                        self.block(next, depth + 1)?
                    } else {
                        Value::Null
                    }
                } else if let Some((key, value)) = pair(content) {
                    let mut item = Map::new();
                    let value = if !value.is_empty() {
                        scalar(value)?
                    } else if self.position < self.lines.len()
                        && self.lines[self.position].0 > indent + 2
                    {
                        let next = self.lines[self.position].0;
                        self.block(next, depth + 1)?
                    } else {
                        Value::Null
                    };
                    item.insert(key, value);
                    if self.position < self.lines.len() && self.lines[self.position].0 == indent + 2
                    {
                        let Value::Object(extra) = self.block(indent + 2, depth + 1)? else {
                            return None;
                        };
                        for (key, value) in extra {
                            if item.insert(key, value).is_some() {
                                return None;
                            }
                        }
                    }
                    Value::Object(item)
                } else {
                    scalar(content)?
                };
                list.push(child);
            } else {
                let (key, value) = pair(body)?;
                if map.contains_key(&key) {
                    return None;
                }
                self.position += 1;
                let child = if !value.is_empty() {
                    scalar(value)?
                } else if self.position < self.lines.len() && self.lines[self.position].0 > indent {
                    let next = self.lines[self.position].0;
                    self.block(next, depth + 1)?
                } else {
                    Value::Null
                };
                map.insert(key, child);
            }
            if self.position < self.lines.len() && self.lines[self.position].0 > indent {
                return None;
            }
        }
        Some(if sequence {
            Value::Array(list)
        } else {
            Value::Object(map)
        })
    }
}
fn yaml(source: &str) -> Option<Value> {
    let mut lines = Vec::new();
    for raw in source.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        if raw[..indent].contains('\t') {
            return None;
        }
        lines.push((indent, raw.trim_start()));
        if lines.len() > LINES {
            return None;
        }
    }
    let indent = lines.first()?.0;
    let mut parser = Yaml { lines, position: 0 };
    let result = parser.block(indent, 0)?;
    (parser.position == parser.lines.len()).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bounded_data_without_evaluating_expressions() {
        assert_eq!(
            parse("{\"a\":1}\n{\"b\":2}"),
            Some(json!([{"a":1},{"b":2}]))
        );
        let object = parse("{ok: true, count: 1e2, items: [0, false]}").unwrap();
        assert_eq!(object["count"].as_f64(), Some(100.0));
        assert_eq!(object["items"], json!([0, false]));
        assert_eq!(parse("name: fixture\nitems:\n  - name: first\n    password: PRIVATE\n  - name: second\n    value: null"),Some(json!({"name":"fixture","items":[{"name":"first","password":"PRIVATE"},{"name":"second","value":null}]})));
        for source in [
            "{value: run()}",
            "{value: variable}",
            "{value: `text ${secret}`}",
            "name: x\nvalue: &anchor 1",
            "name: x\nvalue: *anchor",
            "name: x\nvalue: !!python/object thing",
            "name: x\nvalue: |\n  line",
            "name: x\nname: duplicate",
        ] {
            assert_eq!(parse(source), None, "{source}");
        }
    }

    #[test]
    fn parses_xml_toml_and_tabular_data_without_external_entities() {
        assert_eq!(
            parse("<result><password>PRIVATE</password><count>2</count></result>"),
            Some(
                json!({"result":{"children":[{"password":{"text":"PRIVATE"}},{"count":{"text":"2"}}]}})
            )
        );
        assert!(
            parse("<!DOCTYPE foo [<!ENTITY x SYSTEM \"file:///private\">]><foo>&x;</foo>")
                .is_none()
        );
        assert_eq!(
            parse("updated = 2026-10-05T12:00:00Z\ncount = 0"),
            Some(json!({"updated":"2026-10-05T12:00:00+00:00","count":0}))
        );
        assert_eq!(
            parse("name,password\nexample,PRIVATE"),
            Some(
                json!({"columns":["name","password"],"rows":[{"name":"example","password":"PRIVATE"}]})
            )
        );
        assert_eq!(
            parse("name\tcount\nexample\t2"),
            Some(json!({"columns":["name","count"],"rows":[{"name":"example","count":"2"}]}))
        );
        assert_eq!(
            parse("name,name\na,b"),
            Some(json!([["name", "name"], ["a", "b"]]))
        );
        assert!(parse("a,b\n\n1,2").is_none());
        assert_eq!(
            parse("a,b\n\"x\n\nz\",y"),
            Some(json!({"columns":["a","b"],"rows":[{"a":"x\n\nz","b":"y"}]}))
        );
        assert_eq!(
            parse("name, note\nAlice, 'hello, world'"),
            Some(
                json!({"columns":["name","note"],"rows":[{"name":"Alice","note":"hello, world"}]})
            )
        );
        assert_eq!(
            parse("a,b\n'v', x"),
            Some(json!({"columns":["a","b"],"rows":[{"a":"v","b":"x"}]}))
        );
        assert_eq!(
            parse("a,b\n\"x\", 'y'"),
            Some(json!({"columns":["a","b"],"rows":[{"a":"x","b":"'y'"}]}))
        );
        assert_eq!(
            parse("a,b\n\"x\"\"y\",z"),
            Some(json!({"columns":["a","b"],"rows":[{"a":"x\"y","b":"z"}]}))
        );
        assert!(parse("name,count\nexample,2,extra").is_none());
    }
}
