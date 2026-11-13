use super::{checkpoint::Anchor, project::Projection, Session};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Position {
    pub offset: u64,
    pub hash: String,
}
impl Position {
    pub fn new(offset: u64, record: &[u8]) -> Self {
        Self {
            offset,
            hash: format!("{:x}", Sha256::digest(record)),
        }
    }
    pub fn valid(&self) -> bool {
        self.hash.len() == 64
            && self
                .hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}
#[derive(Clone)]
pub(crate) struct CallPosition {
    pub tool: String,
    pub request: Option<Position>,
    pub response: Option<Position>,
}
#[derive(Clone)]
pub(crate) struct DetailFile {
    pub root: PathBuf,
    pub path: PathBuf,
    pub anchor: Anchor,
    pub calls: BTreeMap<String, CallPosition>,
    guard: Option<CheckpointGuard>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CheckpointGuard {
    pub directory: PathBuf,
    pub source_key: String,
    pub signature: String,
}

impl CheckpointGuard {
    fn valid(&self) -> bool {
        let path = self.directory.join(lam_history_store::FILE_NAME);
        let Ok(before) = fs::symlink_metadata(&path) else {
            return false;
        };
        if !before.is_file() || super::redirected(&before) {
            return false;
        }
        let Ok(Some(store)) =
            lam_history_store::Store::open(&self.directory, lam_history_store::Access::ReadOnly)
        else {
            return false;
        };
        let matches = store
            .checkpoint_signature(&self.source_key)
            .ok()
            .flatten()
            .as_deref()
            == Some(self.signature.as_str());
        let Ok(after) = fs::symlink_metadata(&path) else {
            return false;
        };
        matches
            && after.is_file()
            && !super::redirected(&after)
            && super::file_identity(&before) == super::file_identity(&after)
    }
}

impl DetailFile {
    fn permitted(&self) -> bool {
        self.guard.as_ref().is_none_or(CheckpointGuard::valid)
    }

    pub(super) fn source_signature(
        &self,
    ) -> Option<((u64, u64), u64, Option<std::time::SystemTime>)> {
        if !self.permitted() {
            return None;
        }
        let root = fs::symlink_metadata(&self.root).ok()?;
        if !root.is_dir() || super::redirected(&root) {
            return None;
        }
        let before = fs::symlink_metadata(&self.path).ok()?;
        if !before.is_file() || super::redirected(&before) {
            return None;
        }
        let mut file = super::open_source(&self.root, &self.path).ok()?;
        let current = file.metadata().ok()?;
        let identity = super::file_identity(&current);
        if !current.is_file()
            || super::redirected(&current)
            || super::file_identity(&before) != identity
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        let after = fs::symlink_metadata(&self.path).ok()?;
        let opened = file.metadata().ok()?;
        if !after.is_file()
            || super::redirected(&after)
            || super::file_identity(&after) != identity
            || super::file_identity(&opened) != identity
            || after.len() != current.len()
            || opened.len() != current.len()
            || after.modified().ok() != current.modified().ok()
            || opened.modified().ok() != current.modified().ok()
            || !self.permitted()
        {
            return None;
        }
        Some((identity, current.len(), current.modified().ok()))
    }

    pub(super) fn workdir(&self, position: &Position) -> Option<String> {
        let record = self.verified_record(position)?;
        if record["type"] != "session_meta" {
            return None;
        }
        let cwd = record["payload"]["cwd"].as_str()?;
        if cwd.is_empty() || cwd.chars().count() > 4096 || cwd.chars().any(char::is_control) {
            return None;
        }
        Some(cwd.to_owned())
    }
}
#[derive(Clone, Default)]
pub(crate) struct Index {
    pub files: BTreeMap<String, Vec<DetailFile>>,
}
impl Index {
    pub(super) fn build(root: &Path, files: &BTreeMap<PathBuf, Session>) -> Self {
        let mut index = Self::default();
        for (path, state) in files {
            let Some(anchor) = state.anchor.as_ref() else {
                continue;
            };
            index.add_projection(root, path, anchor, &state.projection, None);
        }
        index
    }

    /// Conservative retained-position accounting, including allocation/tree
    /// overhead, source identity, readiness keys and optional checkpoint guard.
    /// This is a stage budget, not a claim about measured process RSS.
    pub(super) fn projection_bytes(root: &Path, path: &Path, projection: &Projection, selected: Option<&BTreeSet<String>>) -> usize {
        let mut bytes = 2048usize.saturating_add(root.as_os_str().len())
            .saturating_add(path.as_os_str().len())
            .saturating_add(projection.id.as_ref().map_or(0, |id| id.len().saturating_mul(3)));
        for (id, call) in &projection.calls {
            if selected.is_some_and(|ids| !ids.contains(id)) { continue; }
            bytes = bytes.saturating_add(512).saturating_add(id.len()).saturating_add(call.tool_name().len());
            for position in [&call.request_position, &call.response_position].into_iter().flatten() {
                bytes = bytes.saturating_add(128).saturating_add(position.hash.len());
            }
        }
        bytes
    }

    pub(super) fn add_projection(
        &mut self,
        root: &Path,
        path: &Path,
        anchor: &Anchor,
        projection: &Projection,
        guard: Option<CheckpointGuard>,
    ) -> bool {
        self.add_selected_projection(root, path, anchor, projection, guard, None)
    }

    pub(super) fn add_selected_projection(
        &mut self, root: &Path, path: &Path, anchor: &Anchor, projection: &Projection,
        guard: Option<CheckpointGuard>, selected: Option<&BTreeSet<String>>,
    ) -> bool {
        let Some(id) = projection.id.as_ref() else {
            return false;
        };
        if !path.starts_with(root) {
            return false;
        }
        let calls: BTreeMap<String, CallPosition> = projection
            .calls
            .iter()
            .filter(|(id, _)| selected.is_none_or(|ids| ids.contains(*id)))
            .map(|(id, call)| {
                (
                    id.clone(),
                    CallPosition {
                        tool: call.tool_name().to_owned(),
                        request: call.request_position.clone(),
                        response: call.response_position.clone(),
                    },
                )
            })
            .collect();
        let files = self.files.entry(id.clone()).or_default();
        if let Some(file) = files.iter_mut().find(|file| file.root == root && file.path == path && file.anchor == *anchor && file.guard == guard) {
            file.calls.extend(calls);
            return true;
        }
        files.push(DetailFile {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            anchor: anchor.clone(),
            calls,
            guard,
        });
        true
    }
}

const CONTEXT_LIMIT: u64 = 1024 * 1024;
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*|\bsk-[A-Za-z0-9_-]{12,}").unwrap()
});
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b[A-Za-z0-9_.-]*(?:api[_-]?key|password|secret|authorization|token|credential|cookie|headers?)["']?\s*[:=]\s*(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\r\n]*)"#).unwrap()
});
static DIAGNOSTIC_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:api[_-]?key|password|secret|authorization|access[_-]?token|token)["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)"#).unwrap()
});
static SQL_INSERT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(\bINSERT\s+(?:OR\s+\w+\s+)?INTO\s+[\w."`\[\]]+\s*\(([^()]*)\)\s*VALUES\s*)((?:\((?:'(?:''|[^'])*'|[^'()])*\)\s*,?\s*)+)"#).unwrap()
});
static SQL_ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(((?:'(?:''|[^'])*'|[^'()])*)\)").unwrap());
static SQL_VALUES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:'(?:''|[^'])*'|[^,']+)+").unwrap());
static SQL_SECRET_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:api[_-]?key|password|secret|credential|authorization|cookie|access[_-]?token|token)$").unwrap()
});
static DIAGNOSTIC_SQL: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    ["sql", "statement", "query", "summary"]
        .into_iter()
        .map(|key| Regex::new(&format!(r#"\b{key}="((?:\\.|[^"\\])*)""#)).unwrap())
        .collect()
});
static STRUCTURED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b[A-Za-z0-9_.-]*(?:api[_-]?key|password|secret|authorization|token|credential|cookie|headers?)["']?\s*[:=]\s*[\[{]"#).unwrap()
});
fn sensitive(key: &str) -> bool {
    let normalized: String = key
        .bytes()
        .filter(|byte| byte.is_ascii_alphabetic())
        .map(|byte| (byte as char).to_ascii_lowercase())
        .collect();
    [
        "apikey",
        "password",
        "secret",
        "credential",
        "authorization",
        "cookie",
        "header",
        "headers",
        "token",
    ]
    .iter()
    .any(|suffix| normalized.ends_with(suffix))
}
fn text_value(text: &str, masked: bool, depth: usize) -> serde_json::Value {
    if depth > 64 {
        return json!("[內容過深]");
    }
    let trimmed = text.trim();
    if let Some(parsed) = super::structured::parse(trimmed) {
        return redact_value(parsed, masked, depth + 1);
    }
    if !masked {
        return json!(text);
    }
    let lowercase = trimmed.to_ascii_lowercase();
    let structured_sensitive = (lowercase.starts_with('<')
        || lowercase
            .lines()
            .next()
            .is_some_and(|line| line.contains(',') || line.contains('\t')))
        && [
            "apikey",
            "api_key",
            "password",
            "secret",
            "credential",
            "authorization",
            "cookie",
            "header",
            "token",
        ]
        .iter()
        .any(|word| lowercase.contains(word));
    if structured_sensitive || STRUCTURED_SECRET.is_match(text) {
        return json!("[已隱藏]");
    }
    let first = TOKEN.replace_all(text, "[已隱藏]");
    json!(SECRET.replace_all(&first, "[已隱藏]").into_owned())
}

pub(super) fn redact_description(text: &str) -> String {
    text_value(text, true, 0)
        .as_str()
        .unwrap_or("[已隱藏]")
        .to_owned()
}

pub(super) fn diagnostic_text(text: &str, masked: bool, full: bool) -> serde_json::Value {
    let limit = if full { 1_048_576 } else { 32_768 };
    let end = text
        .char_indices()
        .nth(limit)
        .map_or(text.len(), |(index, _)| index);
    let selected = &text[..end];
    let shown = if masked {
        diagnostic_redact(selected, 0)
    } else {
        selected.to_owned()
    };
    json!({"text":shown,"truncated":end < text.len(),"content_status":"available"})
}

pub(super) fn diagnostic_redact(text: &str, depth: usize) -> String {
    if depth > 64 {
        return "[內容過深]".into();
    }
    if text.trim_start().starts_with(['{', '[']) {
        if let Ok(mut value @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) =
            serde_json::from_str::<serde_json::Value>(text)
        {
            fn clean(value: &mut serde_json::Value, depth: usize) {
                if depth > 64 {
                    *value = json!("[內容過深]");
                    return;
                }
                match value {
                    serde_json::Value::Object(map) => {
                        for (key, value) in map {
                            if sensitive(key) {
                                *value = json!("[已隱藏]");
                            } else {
                                clean(value, depth + 1);
                            }
                        }
                    }
                    serde_json::Value::Array(items) => {
                        for item in items {
                            clean(item, depth + 1);
                        }
                    }
                    serde_json::Value::String(text) => *text = diagnostic_redact(text, depth + 1),
                    _ => {}
                }
            }
            clean(&mut value, depth + 1);
            let encoded = value.to_string();
            let mut result = String::with_capacity(encoded.len());
            let mut quoted = false;
            let mut escaped = false;
            for ch in encoded.chars() {
                result.push(ch);
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        quoted = false;
                    }
                } else if ch == '"' {
                    quoted = true;
                } else if matches!(ch, ',' | ':') {
                    result.push(' ');
                }
            }
            return result;
        }
    }
    let token = TOKEN.replace_all(text, "[已隱藏]");
    DIAGNOSTIC_SECRET
        .replace_all(&token, "[已隱藏]")
        .into_owned()
}

pub(crate) fn sql_content(sql: Option<&str>, masked: bool, full: bool) -> serde_json::Value {
    let Some(sql) = sql else {
        return json!({"sql":null,"truncated":false});
    };
    if !masked {
        let end = if full {
            sql.len()
        } else {
            sql.char_indices()
                .nth(32768)
                .map_or(sql.len(), |(index, _)| index)
        };
        return json!({"sql":&sql[..end],"truncated":end < sql.len()});
    }
    let end = if full {
        sql.len()
    } else {
        sql.char_indices()
            .nth(65536)
            .map_or(sql.len(), |(index, _)| index)
    };
    let bounded = &sql[..end];
    let masked_sql = SQL_INSERT.replace_all(bounded, |captures: &regex::Captures<'_>| {
        let columns: Vec<_> = captures[2]
            .split(',')
            .map(|column| column.trim_matches([' ', '\t', '\r', '\n', '"', '`', '[', ']']))
            .collect();
        let hidden: Vec<_> = columns
            .iter()
            .map(|column| SQL_SECRET_COLUMN.is_match(column))
            .collect();
        let rows = SQL_ROW.replace_all(&captures[3], |row: &regex::Captures<'_>| {
            let values: Vec<_> = SQL_VALUES
                .find_iter(&row[1])
                .map(|value| value.as_str())
                .collect();
            if values.len() != columns.len() {
                return if hidden.iter().any(|value| *value) {
                    "('[已隱藏]')".to_owned()
                } else {
                    row[0].to_owned()
                };
            }
            format!(
                "({})",
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| if hidden[index] {
                        "'[已隱藏]'"
                    } else {
                        *value
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });
        format!("{}{}", &captures[1], rows)
    });
    let clean = diagnostic_redact(&masked_sql, 0);
    let clean_end = if full {
        clean.len()
    } else {
        clean
            .char_indices()
            .nth(32768)
            .map_or(clean.len(), |(index, _)| index)
    };
    json!({"sql":&clean[..clean_end],"truncated":!full && (end < sql.len() || clean.chars().count() >= 32768 && bounded.chars().count() > 32768)})
}

pub(crate) fn diagnostic_sql(
    module: &str,
    body: &str,
    masked: bool,
    full: bool,
) -> serde_json::Value {
    if lam_session_core::project_diagnostic(module, body).is_none() {
        return sql_content(None, masked, full);
    }
    let end = if full {
        body.len()
    } else {
        body.char_indices()
            .nth(65536)
            .map_or(body.len(), |(index, _)| index)
    };
    for pattern in DIAGNOSTIC_SQL.iter() {
        if let Some(found) = pattern
            .captures(&body[..end])
            .and_then(|capture| capture.get(1))
        {
            let encoded = format!("\"{}\"", found.as_str());
            let sql = serde_json::from_str::<String>(&encoded)
                .unwrap_or_else(|_| found.as_str().to_owned());
            return if lam_session_core::recognized_sql(&sql) {
                sql_content(Some(&sql), masked, full)
            } else {
                sql_content(None, masked, full)
            };
        }
    }
    sql_content(None, masked, full)
}
pub(super) fn redact_value(
    value: serde_json::Value,
    masked: bool,
    depth: usize,
) -> serde_json::Value {
    if depth > 64 {
        return json!("[內容過深]");
    }
    match value {
        serde_json::Value::String(text) => text_value(&text, masked, depth + 1),
        serde_json::Value::Array(mut items) => {
            if masked {
                mask_table_rows(&mut items);
            }
            serde_json::Value::Array(
                items
                    .into_iter()
                    .map(|item| redact_value(item, masked, depth + 1))
                    .collect(),
            )
        }
        serde_json::Value::Object(items) => serde_json::Value::Object(
            items
                .into_iter()
                .map(|(key, value)| {
                    let value = if masked && sensitive(&key) {
                        json!("[已隱藏]")
                    } else {
                        redact_value(value, masked, depth + 1)
                    };
                    (key, value)
                })
                .collect(),
        ),
        value => value,
    }
}

fn mask_table_rows(items: &mut [serde_json::Value]) {
    let sensitive_columns: Vec<_> = items
        .first()
        .and_then(serde_json::Value::as_array)
        .map(|headers| {
            headers
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    value.as_str().filter(|name| sensitive(name)).map(|_| index)
                })
                .collect()
        })
        .unwrap_or_default();
    for row in items
        .iter_mut()
        .skip(1)
        .filter_map(serde_json::Value::as_array_mut)
    {
        for index in &sensitive_columns {
            if let Some(value) = row.get_mut(*index) {
                *value = json!("[已隱藏]");
            }
        }
    }
}

struct PayloadBudget {
    remaining: isize,
    nodes: usize,
    truncated: bool,
    masked: bool,
}
impl PayloadBudget {
    fn project(&mut self, value: serde_json::Value, depth: usize) -> serde_json::Value {
        self.nodes += 1;
        if self.nodes > 200 || depth > 10 || self.remaining <= 0 {
            self.truncated = true;
            return json!("[內容截斷]");
        }
        match value {
            serde_json::Value::String(text) => {
                if let Some(value) = super::structured::parse(&text) {
                    return self.project(value, depth + 1);
                }
                let original = text.chars().count();
                let clean = if self.masked {
                    text_value(&text, true, 0).as_str().unwrap_or("").to_owned()
                } else {
                    text.clone()
                };
                let limited: String = clean.chars().take(32768).collect();
                let length = limited.chars().count();
                if (original > length && original > 32768) || length > self.remaining as usize {
                    self.truncated = true;
                }
                let selected: String = limited.chars().take(self.remaining as usize).collect();
                self.remaining -= selected.chars().count() as isize;
                json!(selected)
            }
            serde_json::Value::Object(values) => {
                if values.len() > 100 {
                    self.truncated = true;
                }
                let mut result = serde_json::Map::new();
                for (key, value) in values.into_iter().take(100) {
                    if self.remaining <= 0 || self.nodes >= 200 {
                        self.truncated = true;
                        break;
                    }
                    let label: String = key.chars().take(160).collect();
                    self.remaining -= label.chars().count() as isize;
                    let value = if self.masked && sensitive(&label) {
                        json!("[已隱藏]")
                    } else {
                        self.project(value, depth + 1)
                    };
                    result.insert(label, value);
                }
                serde_json::Value::Object(result)
            }
            serde_json::Value::Array(mut values) => {
                if self.masked {
                    mask_table_rows(&mut values);
                }
                if values.len() > 100 {
                    self.truncated = true;
                }
                let mut result = Vec::new();
                for value in values.into_iter().take(100) {
                    if self.remaining <= 0 || self.nodes >= 200 {
                        self.truncated = true;
                        break;
                    }
                    result.push(self.project(value, depth + 1));
                }
                serde_json::Value::Array(result)
            }
            value => value,
        }
    }
}
fn payload(value: serde_json::Value, masked: bool, full: bool) -> (serde_json::Value, bool) {
    if full {
        return (redact_value(value, masked, 0), false);
    }
    let mut budget = PayloadBudget {
        remaining: 32768,
        nodes: 0,
        truncated: false,
        masked,
    };
    let value = budget.project(value, 0);
    (value, budget.truncated)
}

pub(super) fn redact_document(text: &str) -> String {
    match text_value(text, true, 0) {
        serde_json::Value::String(text) => text,
        value => value.to_string(),
    }
}
pub(crate) fn content_page(
    value: &serde_json::Value,
    offset: usize,
    revision: Option<&str>,
    masked: bool,
) -> Option<serde_json::Value> {
    let (text, format) = if let Some(text) = value.as_str() {
        (text.to_owned(), "text")
    } else {
        (serde_json::to_string_pretty(value).ok()?, "json")
    };
    let mut hash = Sha256::new();
    hash.update(if masked {
        b"masked\0".as_slice()
    } else {
        b"unmasked\0".as_slice()
    });
    hash.update(text.as_bytes());
    let digest = format!("{:x}", hash.finalize());
    if revision.is_some_and(|revision| revision != digest) {
        return None;
    }
    let total = text.chars().count();
    if offset > total {
        return None;
    }
    let end = offset.saturating_add(32768).min(total);
    let part: String = text.chars().skip(offset).take(end - offset).collect();
    Some(
        json!({"content_page":true,"text":part,"format":format,"total":total,"next":if end<total {Some(end)}else{None},"revision":digest}),
    )
}

pub(crate) fn page_tool(
    mut value: serde_json::Value,
    field: Option<&str>,
    offset: usize,
    revision: Option<&str>,
    masked: bool,
) -> Result<serde_json::Value, bool> {
    if let Some(field) = field {
        if !matches!(field, "request" | "response" | "output" | "sql" | "text")
            || value.get(field).is_none()
        {
            return Err(false);
        }
        return content_page(&value[field], offset, revision, masked).ok_or(true);
    }
    for field in ["request", "response", "output", "text", "sql"] {
        if value.get(field).is_none() {
            continue;
        }
        if !value[field].is_null() {
            value[field] = content_page(&value[field], 0, None, masked).ok_or(true)?;
        }
    }
    Ok(value)
}

fn visible(record: &serde_json::Value, offset: u64, masked: bool) -> Option<serde_json::Value> {
    if record["type"] != "response_item" || record["channel"] == "analysis" {
        return None;
    }
    let payload = record.get("payload")?.as_object()?;
    if payload.get("channel").and_then(serde_json::Value::as_str) == Some("analysis") {
        return None;
    }
    let timestamp = record
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .and_then(lam_history_store::parse_utc_stamp)
        .map(super::project::stamp);
    let values = match payload.get("type").and_then(serde_json::Value::as_str)? {
        "reasoning" => {
            let summary: Vec<_> = payload.get("summary")?.as_array()?.iter().filter_map(|item| {
                (item["type"]=="summary_text").then(||item["text"].as_str().map(|text|json!({"type":"summary_text","text":text_value(text,masked,0)}))).flatten()
            }).collect();
            if summary.is_empty() {
                return None;
            }
            json!({"type":"reasoning","summary":summary})
        }
        "message" => {
            let role = payload
                .get("role")
                .and_then(serde_json::Value::as_str)
                .filter(|role| matches!(*role, "user" | "assistant"))?;
            let content: Vec<_> = payload
                .get("content")?
                .as_array()?
                .iter()
                .filter_map(|item| {
                    let kind = item["type"]
                        .as_str()
                        .filter(|kind| matches!(*kind, "input_text" | "output_text" | "text"))?;
                    let text = item["text"].as_str()?;
                    Some(json!({"type":kind,"text":text_value(text,masked,0)}))
                })
                .collect();
            if content.is_empty() {
                return None;
            }
            let channel = redact_value(
                payload
                    .get("channel")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                masked,
                0,
            );
            json!({"type":"message","role":role,"channel":channel,"content":content})
        }
        _ => return None,
    };
    let mut result = values;
    result["record_offset"] = json!(offset);
    result["timestamp"] = json!(timestamp);
    Some(result)
}

pub(super) fn read_record(file: &mut fs::File, position: &Position) -> Option<serde_json::Value> {
    file.seek(SeekFrom::Start(position.offset)).ok()?;
    let mut record = Vec::new();
    BufReader::new(file.take(CONTEXT_LIMIT + 1))
        .read_until(b'\n', &mut record)
        .ok()?;
    if record.last() != Some(&b'\n') {
        return None;
    }
    record.pop();
    if record.len() > CONTEXT_LIMIT as usize
        || format!("{:x}", Sha256::digest(&record)) != position.hash
    {
        return None;
    }
    let raw = record.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&record);
    serde_json::from_slice(raw).ok()
}

fn qualified(payload: &serde_json::Value) -> Option<String> {
    let raw_name = payload["name"].as_str()?;
    let namespace = payload["namespace"].as_str();
    Some(
        namespace
            .filter(|namespace| {
                namespace.len() <= 100
                    && !namespace.is_empty()
                    && namespace
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    && !raw_name.starts_with("mcp__")
                    && !raw_name.starts_with(&format!("{namespace}."))
                    && !raw_name.starts_with(&format!("{namespace}__"))
            })
            .map(|namespace| {
                format!(
                    "{namespace}{}{raw_name}",
                    if namespace.starts_with("mcp__") {
                        "__"
                    } else {
                        "."
                    }
                )
            })
            .unwrap_or_else(|| raw_name.to_owned()),
    )
}

fn valid_record<'a>(
    record: &'a serde_json::Value,
    call: &str,
    response: bool,
    tool: &str,
) -> Option<&'a serde_json::Value> {
    let payload = record.get("payload")?;
    if record["type"] != "response_item"
        || payload
            .get("call_id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .or_else(|| payload.get("id").and_then(serde_json::Value::as_str))
            != Some(call)
    {
        return None;
    }
    if response {
        if !matches!(
            payload["type"].as_str(),
            Some("function_call_output" | "custom_tool_call_output")
        ) {
            return None;
        }
    } else if !matches!(
        payload["type"].as_str(),
        Some("function_call" | "custom_tool_call")
    ) || qualified(payload).as_deref() != Some(tool)
    {
        return None;
    }
    Some(payload)
}
impl Index {
    pub(crate) fn candidates(&self, thread: &str) -> Vec<DetailFile> {
        self.files
            .get(thread)
            .map(|files| {
                files
                    .iter()
                    .filter(|file| file.permitted())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn select(&self, thread: &str) -> Option<DetailFile> {
        self.files
            .get(thread)?
            .iter()
            .find(|file| file.permitted())
            .cloned()
    }
    pub(crate) fn select_call(&self, thread: &str, call: &str) -> Option<DetailFile> {
        self.files
            .get(thread)?
            .iter()
            .find(|file| file.calls.contains_key(call) && file.permitted())
            .cloned()
    }
}
fn recorded_skill_paths(
    tool: &str,
    request: &serde_json::Value,
    skill: &str,
) -> Vec<lam_session_core::SkillRead> {
    let mut result = Vec::new();
    if matches!(tool, "exec" | "functions.exec") {
        if let Some(source) = request.as_str() {
            for invocation in lam_session_core::nested_invocations(source).calls {
                result.extend(lam_session_core::skill_read_paths(
                    invocation.tool,
                    invocation
                        .literal
                        .as_ref()
                        .and_then(serde_json::Value::as_object),
                ));
            }
        }
    } else {
        let decoded = request
            .as_str()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
        result.extend(lam_session_core::skill_read_paths(
            tool,
            request
                .as_object()
                .or_else(|| decoded.as_ref().and_then(serde_json::Value::as_object)),
        ));
    }
    result.retain(|entry| entry.name == skill);
    result
}

impl DetailFile {
    pub(crate) fn skill_paths(&self, call: &str, skill: &str) -> Vec<lam_session_core::SkillRead> {
        let Some(raw) = self.tool(call, false, true) else {
            return Vec::new();
        };
        let Some(tool) = raw["tool"].as_str() else {
            return Vec::new();
        };
        recorded_skill_paths(tool, &raw["request"], skill)
    }

    pub(crate) fn retained_skill_paths(
        &self,
        call: &str,
        skill: &str,
        timestamp: &str,
        budget: &mut usize,
    ) -> Vec<lam_session_core::SkillRead> {
        if !self.permitted() {
            return Vec::new();
        }
        let Ok(before) = fs::symlink_metadata(&self.path) else {
            return Vec::new();
        };
        if !before.is_file() || super::redirected(&before) {
            return Vec::new();
        }
        let Ok(mut file) = super::open_source(&self.root, &self.path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let identity = super::file_identity(&meta);
        if !meta.is_file()
            || super::file_identity(&before) != identity
            || !self.anchor.verify(&mut file, identity).unwrap_or(false)
        {
            return Vec::new();
        }
        let size = meta.len();
        let amount = (*budget).min(1024 * 1024).min(size as usize);
        *budget -= amount;
        if file.seek(SeekFrom::Start(size - amount as u64)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity(amount);
        if (&mut file)
            .take(amount as u64)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Vec::new();
        }
        let bytes = if size > amount as u64 {
            match bytes.iter().position(|byte| *byte == b'\n') {
                Some(index) => &bytes[index + 1..],
                None => &[],
            }
        } else {
            &bytes[..]
        };
        let mut result = Vec::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
                continue;
            };
            let body = &record["payload"];
            if record["type"] != "response_item"
                || !matches!(
                    body["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                )
                || body["call_id"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .or_else(|| body["id"].as_str())
                    != Some(call)
                || record["timestamp"]
                    .as_str()
                    .and_then(lam_history_store::parse_utc_stamp)
                    != lam_history_store::parse_utc_stamp(timestamp)
            {
                continue;
            }
            let Some(tool) = qualified(body) else {
                continue;
            };
            let request = body
                .get("arguments")
                .or_else(|| body.get("input"))
                .unwrap_or(&serde_json::Value::Null);
            result.extend(recorded_skill_paths(&tool, request, skill));
        }
        let Ok(after) = file.metadata() else {
            return Vec::new();
        };
        let Ok(path) = fs::symlink_metadata(&self.path) else {
            return Vec::new();
        };
        if super::redirected(&path)
            || super::file_identity(&path) != identity
            || super::file_identity(&after) != identity
            || after.len() != size
            || after.modified().ok() != meta.modified().ok()
            || !self.anchor.verify(&mut file, identity).unwrap_or(false)
        {
            return Vec::new();
        }
        if !self.permitted() {
            return Vec::new();
        }
        result
    }
    pub(crate) fn sql(
        &self,
        event: &serde_json::Value,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        let call = event["call_id"].as_str()?;
        let index = usize::try_from(event["index"].as_u64()?).ok()?;
        let expected = event["statement"].as_str()?;
        let raw = self.tool(call, false, true)?;
        let name = raw["tool"].as_str()?;
        let request = &raw["request"];
        let mut operations = Vec::new();
        if matches!(name, "exec" | "functions.exec") {
            let calls = lam_session_core::nested_invocations(request.as_str()?);
            for call in calls.calls {
                operations.extend(lam_session_core::sqlite_details(
                    call.tool,
                    call.literal.as_ref().and_then(serde_json::Value::as_object),
                ));
                if operations.len() >= 80 {
                    break;
                }
            }
        } else {
            operations = lam_session_core::sqlite_details(name, request.as_object());
        }
        let (statement, sql) = operations.get(index)?;
        if statement != expected {
            return None;
        }
        let mut result = sql_content(sql.as_deref(), masked, full);
        let (response, response_truncated) = payload(raw["response"].clone(), masked, full);
        let sql_truncated = result["truncated"].as_bool().unwrap_or(false);
        result["content_status"] = json!(if sql.is_some() {
            "available"
        } else {
            "unavailable"
        });
        result["response"] = response;
        result["response_scope"] = json!("containing_tool_call");
        result["truncated"] = json!(sql_truncated || response_truncated);
        Some(result)
    }

    fn verified_record(&self, position: &Position) -> Option<serde_json::Value> {
        if !self.permitted() {
            return None;
        }
        if !position.valid() || position.offset >= self.anchor.offset {
            return None;
        }
        let root = fs::symlink_metadata(&self.root).ok()?;
        if !root.is_dir() || super::redirected(&root) {
            return None;
        }
        let before = fs::symlink_metadata(&self.path).ok()?;
        if !before.is_file() || super::redirected(&before) {
            return None;
        }
        let mut file = super::open_source(&self.root, &self.path).ok()?;
        let meta = file.metadata().ok()?;
        let identity = super::file_identity(&meta);
        if !meta.is_file()
            || super::redirected(&meta)
            || super::file_identity(&before) != identity
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        let record = read_record(&mut file, position)?;
        let after = file.metadata().ok()?;
        let path = fs::symlink_metadata(&self.path).ok()?;
        if super::redirected(&path)
            || super::file_identity(&path) != identity
            || super::file_identity(&after) != identity
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        if !self.permitted() {
            return None;
        }
        Some(record)
    }

    pub(crate) fn error(
        &self,
        event: &serde_json::Value,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        let source = event["source"].as_str()?;
        let position = if let (Some(offset), Some(hash)) = (
            event["record_offset"].as_u64(),
            event["record_hash"].as_str(),
        ) {
            Position {
                offset,
                hash: hash.to_owned(),
            }
        } else if source == "tool_result" {
            self.calls
                .get(event["call_id"].as_str()?)?
                .response
                .clone()?
        } else {
            return None;
        };
        let record = self.verified_record(&position)?;
        let timestamp = record["timestamp"]
            .as_str()
            .and_then(lam_history_store::parse_utc_stamp)
            .map(super::project::stamp);
        if timestamp.as_deref() != event["timestamp"].as_str() {
            return None;
        }
        let body = record.get("payload")?.as_object()?;
        let value = match source {
            "session" => {
                let kind = body.get("type")?.as_str()?;
                if record["type"] != "event_msg"
                    || event["reason"].as_str() != Some(kind)
                    || !(matches!(kind, "error" | "turn_aborted" | "turn_failed")
                        || kind.ends_with("_failed"))
                {
                    return None;
                }
                serde_json::Value::Object(
                    ["type", "message", "error", "reason", "error_code"]
                        .iter()
                        .filter_map(|key| {
                            body.get(*key)
                                .map(|value| ((*key).to_owned(), value.clone()))
                        })
                        .collect(),
                )
            }
            "tool_result" => {
                let call = event["call_id"].as_str()?;
                if record["type"] != "response_item"
                    || !matches!(
                        body.get("type").and_then(serde_json::Value::as_str),
                        Some("function_call_output" | "custom_tool_call_output")
                    )
                    || body
                        .get("call_id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .or_else(|| body.get("id").and_then(serde_json::Value::as_str))
                        != Some(call)
                {
                    return None;
                }
                let output = body
                    .get("output")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                if !lam_history_store::mcp::output_failed(&output) {
                    return None;
                }
                output
            }
            _ => return None,
        };
        let (text, truncated) = payload(value, masked, full);
        Some(
            json!({"text":text,"truncated":truncated,"content_status":"available","source":source}),
        )
    }

    pub(crate) fn jev(
        &self,
        call: &str,
        index: usize,
        expected: &str,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        let raw = self.tool(call, false, true)?;
        let tool = raw["tool"].as_str()?;
        let nested = matches!(tool, "exec" | "functions.exec");
        let (name, arguments, isolated) = if nested {
            let source = raw["request"].as_str()?;
            let calls = lam_session_core::nested_invocations(source);
            let isolated = calls.calls.len() == 1 && !calls.truncated;
            let selected = calls
                .calls
                .into_iter()
                .filter(|invocation| super::project::jev_kind(invocation.tool).is_some())
                .nth(index)?;
            (
                selected.tool.to_owned(),
                selected
                    .literal
                    .filter(serde_json::Value::is_object)
                    .unwrap_or(serde_json::Value::Null),
                isolated,
            )
        } else {
            if index != 0 {
                return None;
            }
            (tool.to_owned(), raw["request"].clone(), true)
        };
        if super::project::jev_kind(&name) != Some(expected) {
            return None;
        }
        let (request, request_truncated) = payload(arguments, masked, full);
        let (response, response_truncated) = payload(
            if isolated {
                raw["response"].clone()
            } else {
                serde_json::Value::Null
            },
            masked,
            full,
        );
        Some(
            json!({"operation":expected,"request":request,"response":response,"response_scope":if nested {"containing_tool_call"} else {"jev_tool_call"},"truncated":request_truncated || response_truncated,"content_status":"available"}),
        )
    }

    pub(crate) fn mcp(
        &self,
        call: &str,
        index: usize,
        server: &str,
        operation: &str,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        let raw = self.tool(call, false, true)?;
        let tool = raw["tool"].as_str()?;
        let request = &raw["request"];
        let nested = matches!(tool, "exec" | "functions.exec");
        let (name, arguments, request_source) = if nested {
            let source = request.as_str()?;
            let calls = lam_session_core::nested_invocations(source);
            let selected = calls
                .calls
                .into_iter()
                .filter(|invocation| lam_history_store::mcp::identity(invocation.tool).is_some())
                .nth(index)?;
            let name = selected.tool.to_owned();
            if let Some(literal) = selected.literal.filter(serde_json::Value::is_object) {
                (name, literal, None)
            } else {
                let expression = lam_session_core::invocation_expressions(source)
                    .into_iter()
                    .filter(|(tool, _)| lam_history_store::mcp::identity(tool).is_some())
                    .nth(index)
                    .filter(|(tool, _)| *tool == name)
                    .map(|(_, expression)| expression);
                (
                    name,
                    expression.map_or(serde_json::Value::Null, |value| json!(value)),
                    expression.map(|_| "recorded_expression"),
                )
            }
        } else {
            if index != 0 {
                return None;
            }
            (tool.to_owned(), request.clone(), None)
        };
        if lam_history_store::mcp::identity(&name) != Some((server, operation)) {
            return None;
        }
        let (sent, sent_truncated) = payload(arguments, masked, full);
        let shown = self.tool(call, masked, full)?;
        Some(
            json!({"request":sent,"response":shown["response"],"response_scope":if nested {"containing_tool_call"} else {"mcp_tool_call"},"request_source":request_source,"content_status":"available","truncated":sent_truncated || shown["truncated"].as_bool().unwrap_or(false)}),
        )
    }

    pub(crate) fn command(
        &self,
        call: &str,
        operation: &str,
        checks: bool,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        let raw = self.tool(call, false, true)?;
        let tool = raw["tool"].as_str()?;
        let request = &raw["request"];
        let mut selected = Vec::new();
        let mut part_count = 0_usize;
        let mut outer = false;
        let mut inspect = |name: &str, args: &serde_json::Value, nested: bool| {
            if !matches!(name, "exec_command" | "functions.exec_command") {
                return;
            }
            if let Some(command) = args.get("cmd").and_then(serde_json::Value::as_str) {
                let (parts, count) =
                    lam_session_core::matching_command_parts(command, operation, checks);
                selected.extend(parts);
                part_count += count;
                outer |= nested;
            }
        };
        if matches!(tool, "exec" | "functions.exec") {
            if let Some(source) = request.as_str() {
                let nested = lam_session_core::nested_invocations(source);
                for invocation in nested.calls {
                    if let Some(args) = invocation.literal {
                        inspect(invocation.tool, &args, true);
                    }
                }
            }
        } else {
            inspect(tool, request, false);
        }
        if selected.is_empty() {
            return Some(
                json!({"commands":[],"request":null,"response":null,"output":null,"output_scope":null,"result_scope":null,"truncated":false,"content_status":"unavailable"}),
            );
        }
        let outer = outer || part_count != 1;
        let (command, command_truncated) = payload(json!(selected.join("\n")), masked, full);
        let presented = self.tool(call, masked, full)?;
        let response = presented["response"].clone();
        let category = if checks {
            "check_command"
        } else {
            "git_command"
        };
        Some(
            json!({"commands":[command],"request":presented["request"],"response":response,"output":response,
            "output_scope":if outer {"outer_scope"} else {category},
            "result_scope":if outer {"containing_tool_call"} else {category},
            "content_status":"available","truncated":presented["truncated"].as_bool().unwrap_or(false) || command_truncated}),
        )
    }

    pub(crate) fn tool(&self, call: &str, masked: bool, full: bool) -> Option<serde_json::Value> {
        if !self.permitted() {
            return None;
        }
        let selected = self.calls.get(call)?;
        let request_position = selected.request.as_ref()?;
        let root = fs::symlink_metadata(&self.root).ok()?;
        if !root.is_dir() || super::redirected(&root) {
            return None;
        }
        let before = fs::symlink_metadata(&self.path).ok()?;
        if !before.is_file() || super::redirected(&before) {
            return None;
        }
        let mut file = super::open_source(&self.root, &self.path).ok()?;
        let meta = file.metadata().ok()?;
        let identity = super::file_identity(&meta);
        if !meta.is_file()
            || super::redirected(&meta)
            || super::file_identity(&before) != identity
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        let record = read_record(&mut file, request_position)?;
        let request = valid_record(&record, call, false, &selected.tool)?
            .get("arguments")
            .or_else(|| record["payload"].get("input"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let response = if let Some(position) = &selected.response {
            let record = read_record(&mut file, position)?;
            valid_record(&record, call, true, "")?
                .get("output")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };
        let after = file.metadata().ok()?;
        let path = fs::symlink_metadata(&self.path).ok()?;
        if super::redirected(&path)
            || super::file_identity(&path) != identity
            || super::file_identity(&after) != identity
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        if !self.permitted() {
            return None;
        }
        let (request, request_truncated) = payload(request, masked, full);
        let (response, response_truncated) = payload(response, masked, full);
        Some(
            json!({"tool":selected.tool,"request":request,"response":response,"response_scope":"tool_call","content_status":"available","truncated":request_truncated || response_truncated}),
        )
    }

    pub(crate) fn agent_message(
        &self,
        call: &str,
        index: usize,
        masked: bool,
    ) -> Option<serde_json::Value> {
        if !self.permitted() {
            return None;
        }
        let position = self.calls.get(call)?;
        let request_position = position.request.as_ref()?;
        let initial = fs::symlink_metadata(&self.path).ok()?;
        if !initial.is_file() || super::redirected(&initial) {
            return None;
        }
        let context = self.context(Some(call), masked)?;
        let root = fs::symlink_metadata(&self.root).ok()?;
        if !root.is_dir() || super::redirected(&root) {
            return None;
        }
        let mut file = super::open_source(&self.root, &self.path).ok()?;
        let current = file.metadata().ok()?;
        let identity = super::file_identity(&current);
        if !current.is_file()
            || super::redirected(&current)
            || super::file_identity(&initial) != identity
            || current.len() != initial.len()
            || current.modified().ok() != initial.modified().ok()
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        let record = read_record(&mut file, request_position)?;
        let payload = valid_record(&record, call, false, &position.tool)?;
        let arguments = payload.get("arguments").or_else(|| payload.get("input"));
        let mut selected: Option<(
            super::agent::Outgoing,
            Option<serde_json::Map<String, serde_json::Value>>,
        )> = None;
        if matches!(position.tool.as_str(), "exec" | "functions.exec") {
            let source = arguments.and_then(serde_json::Value::as_str)?;
            let invocations = lam_session_core::nested_invocations(source);
            let mut descriptor_index = 0;
            for (invocation_index, invocation) in invocations.calls.iter().enumerate() {
                let args = invocation
                    .literal
                    .as_ref()
                    .and_then(serde_json::Value::as_object);
                if let Some(descriptor) = super::agent::Outgoing::new(
                    invocation.tool,
                    args,
                    None,
                    true,
                    descriptor_index,
                    invocation_index,
                ) {
                    if descriptor_index == index {
                        selected = Some((descriptor, args.cloned()));
                        break;
                    }
                    descriptor_index += 1;
                }
            }
        } else {
            let args = match arguments {
                Some(serde_json::Value::Object(value)) => Some(value.clone()),
                Some(serde_json::Value::String(value)) => {
                    serde_json::from_str::<serde_json::Value>(value)
                        .ok()
                        .and_then(|value| value.as_object().cloned())
                }
                _ => None,
            };
            if index == 0 {
                if let Some(descriptor) =
                    super::agent::Outgoing::new(&position.tool, args.as_ref(), None, false, 0, 0)
                {
                    selected = Some((descriptor, args));
                }
            }
        }
        let (descriptor, args) = selected?;
        let context_recorded = args
            .as_ref()
            .is_some_and(|args| args.contains_key("context"));
        let request = args.map(|args| {
            let selected: serde_json::Map<String, serde_json::Value> = [
                "message",
                "prompt",
                "context",
                "fork_turns",
                "target",
                "recipient",
                "task_name",
                "model",
                "reasoning_effort",
            ]
            .into_iter()
            .filter_map(|key| args.get(key).cloned().map(|value| (key.to_owned(), value)))
            .collect();
            redact_value(serde_json::Value::Object(selected), masked, 0)
        });
        let response = if let Some(position) = &position.response {
            let record = read_record(&mut file, position)?;
            let payload = valid_record(&record, call, true, "")?;
            payload
                .get("output")
                .cloned()
                .map(|value| redact_value(value, masked, 0))
                .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };
        let after = file.metadata().ok()?;
        let path = fs::symlink_metadata(&self.path).ok()?;
        if super::redirected(&path)
            || super::file_identity(&path) != identity
            || super::file_identity(&after) != identity
            || after.len() != initial.len()
            || after.modified().ok() != initial.modified().ok()
            || !self.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        if !self.permitted() {
            return None;
        }
        Some(
            json!({"metadata":descriptor.metadata(),"request":request,"context_state":if context_recorded {"recorded"} else {"not_recorded"},"masked":masked,"response":response,"response_scope":"containing_tool_call","text":context,"content_status":"available","truncated":false}),
        )
    }
    pub(crate) fn context(&self, call: Option<&str>, masked: bool) -> Option<serde_json::Value> {
        if !self.permitted() {
            return None;
        }
        let selected = self;
        let position = if let Some(call) = call {
            match selected.calls.get(call)?.request.as_ref() {
                Some(position) => Some(position),
                None => {
                    return Some(
                        json!({"text":[],"scope":"preceding_visible_messages","truncated":false,"context_state":"pending","masked":masked}),
                    )
                }
            }
        } else {
            None
        };
        let root = fs::symlink_metadata(&selected.root).ok()?;
        if !root.is_dir() || super::redirected(&root) {
            return None;
        }
        let before = fs::symlink_metadata(&selected.path).ok()?;
        if !before.is_file() || super::redirected(&before) {
            return None;
        }
        let mut file = super::open_source(&selected.root, &selected.path).ok()?;
        let meta = file.metadata().ok()?;
        let identity = super::file_identity(&meta);
        if !meta.is_file()
            || super::redirected(&meta)
            || super::file_identity(&before) != identity
            || !selected.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        let end = if let Some(position) = position {
            if position.offset >= meta.len() {
                return None;
            }
            let value = read_record(&mut file, position)?;
            let call = call?;
            valid_record(&value, call, false, &selected.calls.get(call)?.tool)?;
            position.offset
        } else {
            meta.len()
        };
        let start = end.saturating_sub(CONTEXT_LIMIT);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        (&mut file).take(end - start).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 != end - start {
            return None;
        }
        let mut lines = if start > 0 {
            lam_session_core::prefix_width(&bytes).min(bytes.len())..bytes.len()
        } else {
            0..bytes.len()
        };
        let mut messages = Vec::new();
        while let Some((offset, line)) =
            lam_session_core::next_line(&bytes, &mut lines, CONTEXT_LIMIT as usize, false)
        {
            let line = line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line);
            if let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) {
                if let Some(message) = visible(&record, start + offset as u64, masked) {
                    messages.push(message);
                }
            }
        }
        let after = file.metadata().ok()?;
        let path = fs::symlink_metadata(&selected.path).ok()?;
        if super::redirected(&path)
            || super::file_identity(&path) != identity
            || super::file_identity(&after) != identity
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
            || !selected.anchor.verify(&mut file, identity).ok()?
        {
            return None;
        }
        if !self.permitted() {
            return None;
        }
        let truncated = start > 0 || messages.len() > 32;
        if messages.len() > 32 {
            messages.drain(..messages.len() - 32);
        }
        Some(
            json!({"text":messages,"scope":if call.is_some(){"preceding_visible_messages"}else{"latest_visible_messages"},"truncated":truncated,"context_state":if messages.is_empty(){"not_recorded"}else{"recorded"},"masked":masked}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_structured_and_prefixed_credentials_without_persisting_them() {
        for input in [
            r#"client_secret = "PRIVATE VALUE""#,
            r#"{"nested":{"apiToken":"PRIVATE_JSON"}}"#,
            r#"<token>PRIVATE_XML</token>"#,
            "token,value\nPRIVATE_CSV,1",
            "headers: {Authorization: PRIVATE_NESTED}",
        ] {
            assert!(!text_value(input, true, 0).to_string().contains("PRIVATE"));
        }
        assert!(text_value("client_secret = PRIVATE", false, 0)
            .to_string()
            .contains("PRIVATE"));
    }
    #[test]
    fn rejects_changed_call_even_when_sampled_file_edges_match() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("sessions");
        fs::create_dir(&root).unwrap();
        let path = root.join("rollout.jsonl");
        let prefix = format!(
            "{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"x".repeat(600)}]}})
        );
        let request=json!({"type":"response_item","payload":{"type":"function_call","name":"collaboration.send_message","call_id":"send1","arguments":"{}"}}).to_string();
        let suffix = format!("\n{}\n", json!({"type":"padding","value":"z".repeat(600)}));
        fs::write(&path, format!("{prefix}{request}{suffix}")).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        let meta = file.metadata().unwrap();
        let mut checkpoints =
            super::super::checkpoint::Checkpoints::new(home.path().join("monitoring"), true);
        let anchor = checkpoints
            .sample(&mut file, super::super::file_identity(&meta), meta.len())
            .unwrap();
        let selected = DetailFile {
            root,
            path: path.clone(),
            anchor,
            guard: None,
            calls: BTreeMap::from([(
                "send1".into(),
                CallPosition {
                    tool: "collaboration.send_message".into(),
                    request: Some(Position::new(prefix.len() as u64, request.as_bytes())),
                    response: None,
                },
            )]),
        };
        assert!(selected.context(Some("send1"), true).is_some());
        let altered = request.replace("send1", "send2");
        assert_eq!(request.len(), altered.len());
        fs::write(path, format!("{prefix}{altered}{suffix}")).unwrap();
        assert!(selected.context(Some("send1"), true).is_none());
    }

    #[test]
    fn rejects_changed_response_even_when_sampled_file_edges_match() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("sessions");
        fs::create_dir(&root).unwrap();
        let path = root.join("rollout.jsonl");
        let prefix = format!(
            "{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"x".repeat(600)}]}})
        );
        let request=json!({"type":"response_item","payload":{"type":"function_call","name":"collaboration.send_message","call_id":"send1","arguments":"{}"}}).to_string();
        let response=json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"send1","output":"value1"}}).to_string();
        let suffix = format!("\n{}\n", json!({"type":"padding","value":"z".repeat(600)}));
        fs::write(&path, format!("{prefix}{request}\n{response}{suffix}")).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        let meta = file.metadata().unwrap();
        let mut checkpoints =
            super::super::checkpoint::Checkpoints::new(home.path().join("monitoring"), true);
        let anchor = checkpoints
            .sample(&mut file, super::super::file_identity(&meta), meta.len())
            .unwrap();
        let selected = DetailFile {
            root,
            path: path.clone(),
            anchor,
            guard: None,
            calls: BTreeMap::from([(
                "send1".into(),
                CallPosition {
                    tool: "collaboration.send_message".into(),
                    request: Some(Position::new(prefix.len() as u64, request.as_bytes())),
                    response: Some(Position::new(
                        (prefix.len() + request.len() + 1) as u64,
                        response.as_bytes(),
                    )),
                },
            )]),
        };
        assert!(selected.agent_message("send1", 0, true).is_some());
        assert!(selected.tool("send1", true, false).is_some());
        let altered = response.replace("value1", "value2");
        assert_eq!(response.len(), altered.len());
        fs::write(path, format!("{prefix}{request}\n{altered}{suffix}")).unwrap();
        assert!(selected.agent_message("send1", 0, true).is_none());
        assert!(selected.tool("send1", true, false).is_none());
    }

    #[test]
    fn error_detail_rejects_changed_record_even_when_sampled_edges_match() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("sessions");
        fs::create_dir(&root).unwrap();
        let path = root.join("rollout.jsonl");
        let prefix = format!("{}\n", json!({"type":"padding","data":"x".repeat(600)}));
        let record =
            json!({"type":"event_msg","payload":{"type":"turn_failed","message":"value1"}})
                .to_string();
        let suffix = format!("\n{}\n", json!({"type":"padding","data":"z".repeat(600)}));
        fs::write(&path, format!("{prefix}{record}{suffix}")).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        let meta = file.metadata().unwrap();
        let mut checkpoints =
            super::super::checkpoint::Checkpoints::new(home.path().join("monitoring"), true);
        let anchor = checkpoints
            .sample(&mut file, super::super::file_identity(&meta), meta.len())
            .unwrap();
        let selected = DetailFile {
            root,
            path: path.clone(),
            anchor,
            guard: None,
            calls: BTreeMap::new(),
        };
        let event = json!({"source":"session","reason":"turn_failed","timestamp":null,"record_offset":prefix.len(),"record_hash":format!("{:x}", Sha256::digest(record.as_bytes()))});
        assert!(selected.error(&event, true, false).is_some());
        let altered = record.replace("value1", "value2");
        fs::write(path, format!("{prefix}{altered}{suffix}")).unwrap();
        assert!(selected.error(&event, true, false).is_none());
    }

    #[test]
    fn diagnostic_text_preserves_text_format_and_masks_like_python() {
        let plain = diagnostic_text("notice password=PRIVATE, next=visible", true, false);
        assert_eq!(plain["text"], "notice [已隱藏], next=visible");
        let yaml = diagnostic_text("alpha: beta", true, false);
        assert_eq!(yaml["text"], "alpha: beta");
        let json = diagnostic_text(r#"{"password":"PRIVATE","message":"visible"}"#, true, false);
        assert_eq!(
            json["text"],
            r#"{"password": "[已隱藏]", "message": "visible"}"#
        );
    }

    #[test]
    fn sql_content_masks_secret_columns_and_preserves_other_values() {
        let sql = "INSERT INTO users(name,password) VALUES ('ok','PRIVATE'),('two','OTHER');";
        let masked = sql_content(Some(sql), true, false);
        let text = masked["sql"].as_str().unwrap();
        assert!(text.contains("'ok'"));
        assert!(text.contains("'[已隱藏]'"));
        assert!(!text.contains("PRIVATE") && !text.contains("OTHER"));
        assert_eq!(sql_content(Some(sql), false, false)["sql"], sql);
        let malformed = sql_content(
            Some("INSERT INTO users(name,password) VALUES ('x','PRIVATE','extra')"),
            true,
            false,
        );
        assert!(!malformed.to_string().contains("PRIVATE"));
    }

    #[test]
    fn detail_record_uses_id_when_call_id_is_missing_or_empty() {
        for call_id in [serde_json::Value::Null, json!(""), json!("send1")] {
            let record = json!({"type":"response_item","payload":{
                "type":"function_call","name":"fixture","call_id":call_id,"id":"send1"
            }});
            assert!(valid_record(&record, "send1", false, "fixture").is_some());
        }
        let record = json!({"type":"response_item","payload":{
            "type":"function_call","name":"fixture","call_id":"other","id":"send1"
        }});
        assert!(valid_record(&record, "send1", false, "fixture").is_none());
    }
}
