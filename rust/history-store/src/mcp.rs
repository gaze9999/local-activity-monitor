use crate::legacy::{identifier, text, Object};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::Ipv4Addr;
use std::sync::LazyLock;
use url::{Host, Url};

const OPERATIONS: &[&str] = &[
    "search_query",
    "open",
    "click",
    "find",
    "screenshot",
    "image_query",
    "finance",
    "weather",
    "sports",
    "time",
];
const RESOURCE_KEYS: &[&str] = &[
    "page_id",
    "pageId",
    "threadId",
    "runId",
    "operationId",
    "deploymentId",
    "projectId",
    "file_key",
];
const STATUSES: &[&str] = &[
    "ok",
    "success",
    "completed",
    "ready",
    "installed",
    "extracted",
    "written",
    "preview",
    "unchanged",
    "partial",
    "passed",
    "failed",
    "error",
    "cancelled",
    "canceled",
    "timeout",
    "running",
    "pending",
    "queued",
    "fallback",
    "skipped",
    "dry_run",
    "used",
    "not_needed",
    "disabled",
    "missing_dependencies",
    "building",
    "deployed",
];
const UNITS: &[&str] = &[
    "count", "ms", "seconds", "bytes", "tokens", "credits", "percent", "ratio",
];
const NUMBER_LIMIT: u64 = 1 << 53;
const OUTPUT_CHARACTER_LIMIT: usize = 1024 * 1024;

fn within_output_limit(value: &str) -> bool {
    value.len() <= OUTPUT_CHARACTER_LIMIT || value.chars().nth(OUTPUT_CHARACTER_LIMIT).is_none()
}

static METRIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"^[a-z][a-z0-9_]{0,63}(?:_count|_items|_ms|_seconds|_bytes|_tokens|_credits|_attempts|_retries|_hits|_misses|_percent|_ratio|_nodes|_pages|_errors|_passed|_failed|_skipped)$"
).expect("metric pattern")
});
static NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]{0,79}$").expect("metric name"));
static FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]{0,63}_enabled$").expect("metric flag"));
static STATUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z_]{0,39}$").expect("status pattern"));
static DECIMAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{1,16}(?:\.[0-9]{1,16})?$").expect("credit decimal"));
static BLOCKED: &[&str] = &[
    "secret",
    "password",
    "credential",
    "api_key",
    "access_token",
    "refresh_token",
    "user_id",
    "account_id",
];

pub fn number(value: &Value, maximum: u64) -> bool {
    let Some(value) = value.as_number() else {
        return false;
    };
    value.as_u64().is_some_and(|value| value <= maximum)
        || value.is_f64()
            && value
                .as_f64()
                .is_some_and(|value| (0.0..=maximum as f64).contains(&value))
}

pub fn project(server: &str, data: &Object) -> Object {
    let metadata = data.get("metadata").and_then(Value::as_object);
    let original = data.get("result").and_then(Value::as_object);
    let (metadata, mut result) = if server == "web" {
        let operations = list(metadata, "operations")
            .take(20)
            .filter_map(Value::as_str)
            .filter(|value| OPERATIONS.contains(value))
            .collect::<Vec<_>>();
        (
            json!({"references": references(metadata), "operations": operations}),
            Object::from_iter([("references".into(), json!(references(original)))]),
        )
    } else {
        let resources = metadata
            .and_then(|value| value.get("resources"))
            .and_then(Value::as_object);
        (
            json!({"resources": resource_ids(resources)}),
            response(server, original),
        )
    };
    if let Some(status) = original
        .and_then(|data| text(data, "status"))
        .filter(|value| identifier(value, 160, b"_./:-"))
    {
        result.insert("status".into(), status.into());
    }
    Object::from_iter([
        ("metadata".into(), metadata),
        ("result".into(), Value::Object(result)),
    ])
}

fn list<'a>(value: Option<&'a Object>, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .and_then(|data| data.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn references(data: Option<&Object>) -> Vec<String> {
    let mut references = Vec::new();
    for url in list(data, "references")
        .take(30)
        .filter_map(Value::as_str)
        .filter_map(reference)
    {
        if !references.contains(&url) {
            references.push(url);
        }
    }
    references
}

fn reference(value: &str) -> Option<String> {
    if value.chars().count() > 2048 || value.chars().any(char::is_control) {
        return None;
    }
    let value = value.trim_start();
    let (scheme, rest) = value.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains(['\\', '@']) {
        return None;
    }
    let parsed = Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    match parsed.host()? {
        Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.');
            if !domain.contains('.')
                || domain.ends_with(".local")
                || domain.ends_with(".internal")
                || !domain
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
            {
                return None;
            }
        }
        Host::Ipv4(ip) if public_ipv4(ip) => {}
        _ => return None,
    }
    Some(format!(
        "{}://{}",
        scheme.to_ascii_lowercase(),
        rest.split(['?', '#']).next()?
    ))
}

fn public_ipv4(value: Ipv4Addr) -> bool {
    let [first, second, third, last] = value.octets();
    !matches!(first, 0 | 10 | 127 | 240..=255)
        && !matches!(
            (first, second),
            (169, 254) | (172, 16..=31) | (192, 168) | (100, 64..=127) | (198, 18..=19)
        )
        && !matches!(
            (first, second, third),
            (192, 0, 2) | (198, 51, 100) | (203, 0, 113) | (192, 88, 99)
        )
        && ((first, second, third) != (192, 0, 0) || matches!(last, 9 | 10))
}

fn resource_ids(data: Option<&Object>) -> Object {
    let Some(data) = data else {
        return Object::new();
    };
    RESOURCE_KEYS
        .iter()
        .filter_map(|&key| {
            let value = text(data, key).filter(|value| {
                identifier(value, 160, b"_.:-")
                    && !value.starts_with("sk-")
                    && !value.starts_with("sk_")
            })?;
            Some((key.into(), value.into()))
        })
        .collect()
}

pub fn sanitize_mcp_resources(event: &mut Value) {
    let Some(event) = event.as_object_mut() else {
        return;
    };
    if let Some(resources) = event
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .and_then(|metadata| metadata.get_mut("resources"))
        .and_then(Value::as_object_mut)
    {
        resources.retain(|_, value| {
            value
                .as_str()
                .is_none_or(|text| !text.starts_with("sk-") && !text.starts_with("sk_"))
        });
    }
    if let Some(result) = event.get_mut("result").and_then(Value::as_object_mut) {
        result.retain(|key, value| {
            !RESOURCE_KEYS.contains(&key.as_str())
                || value
                    .as_str()
                    .is_none_or(|text| !text.starts_with("sk-") && !text.starts_with("sk_"))
        });
    }
}

fn decoded(input: &Object) -> Object {
    let wrapper_error = input.get("isError").and_then(Value::as_bool) == Some(true);
    if let Some(value) = input.get("structuredContent").and_then(Value::as_object) {
        let mut value = value.clone();
        value.insert("isError".into(), wrapper_error.into());
        return value;
    }
    if let Some(blocks) = input.get("content").and_then(Value::as_array) {
        if blocks.len() <= 100 {
            let mut results = blocks
                .iter()
                .filter_map(Value::as_object)
                .filter(|block| matches!(text(block, "type"), Some("input_text" | "text")))
                .filter_map(|block| {
                    let value = block.get("text")?;
                    let value = match value {
                        Value::String(value) if within_output_limit(value) => {
                            serde_json::from_str::<Object>(value).ok()?
                        }
                        Value::Object(value) => value.clone(),
                        _ => return None,
                    };
                    text(&value, "status").is_some().then_some(value)
                });
            if let Some(mut value) = results.next() {
                if results.next().is_none() {
                    let error = wrapper_error
                        || value.get("isError").and_then(Value::as_bool) == Some(true);
                    value.insert("isError".into(), error.into());
                    return value;
                }
            }
        }
        if let [Value::Object(block)] = blocks.as_slice() {
            if let Some(value) = text(block, "text").filter(|value| within_output_limit(value)) {
                if let Ok(mut value) = serde_json::from_str::<Object>(value) {
                    value.insert("isError".into(), wrapper_error.into());
                    return value;
                }
            }
        }
    }
    input.clone()
}

fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_none_or(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn response(server: &str, input: Option<&Object>) -> Object {
    let Some(input) = input else {
        return Object::new();
    };
    let value = decoded(input);
    let mut result = Object::new();
    let status = if value.get("isError").and_then(Value::as_bool) == Some(true)
        || value.get("error").is_some_and(truth)
    {
        Some("error".to_owned())
    } else {
        value
            .get("status")
            .or_else(|| value.get("state"))
            .and_then(Value::as_str)
            .map(str::to_lowercase)
    };
    if let Some(status) = status.filter(|status| STATUS.is_match(status)) {
        result.insert("status".into(), status.into());
    }
    result.extend(resource_ids(Some(&value)));
    copy_numbers(
        &value,
        &[
            "latency_ms",
            "duration_ms",
            "http_attempts",
            "retries",
            "input_tokens",
            "output_tokens",
        ],
        "",
        &mut result,
    );
    metrics(&value, &mut result);
    credits(&value, &mut result);
    for key in [
        "written",
        "truncated",
        "cache_hit",
        "recording_enabled",
        "telemetry_enabled",
    ] {
        if let Some(flag) = value.get(key).and_then(Value::as_bool) {
            result.insert(key.into(), flag.into());
        }
    }
    if server == "local_documents" {
        copy_numbers(&value, &["content_chars"], "", &mut result);
        if let Some(ocr) = value.get("ocr").and_then(Value::as_object) {
            if let Some(status) = text(ocr, "status").filter(|status| STATUSES.contains(status)) {
                result.insert("ocr_status".into(), status.into());
            }
            copy_numbers(
                ocr,
                &["eligible_items", "processed_items", "omitted_items"],
                "ocr_",
                &mut result,
            );
            if let Some(errors) = ocr.get("errors").and_then(Value::as_array) {
                result.insert("ocr_errors".into(), errors.len().into());
            }
        }
    }
    if server == "workspace_inspection" {
        for key in [
            "changed",
            "only_in_source",
            "only_in_target",
            "missing",
            "added",
            "removed",
        ] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                result.insert(format!("{key}_files"), items.len().into());
            }
        }
        if let Some(runs) = value.get("runs").and_then(Value::as_array) {
            result.insert("evidence_runs".into(), runs.len().into());
            let mut counts = [0_usize; 3];
            for run in runs.iter().take(100).filter_map(Value::as_object) {
                for item in list(Some(run), "results")
                    .take(1000)
                    .filter_map(Value::as_object)
                {
                    if let Some(index) = ["passed", "failed", "skipped"]
                        .iter()
                        .position(|status| text(item, "status") == Some(status))
                    {
                        counts[index] += 1;
                    }
                }
            }
            for (key, count) in ["checks_passed", "checks_failed", "checks_skipped"]
                .into_iter()
                .zip(counts)
            {
                result.insert(key.into(), count.into());
            }
        }
    }
    if let Some(findings) = value.get("findings").and_then(Value::as_array) {
        result.insert("findings".into(), findings.len().into());
        let mut counts = [0_usize; 4];
        let labels = ["critical", "high", "medium", "low"];
        for item in findings.iter().take(1000).filter_map(Value::as_object) {
            if let Some(severity) = text(item, "severity") {
                if let Some(index) = labels
                    .iter()
                    .position(|label| severity.eq_ignore_ascii_case(label))
                {
                    counts[index] += 1;
                }
            }
        }
        for (label, count) in labels.into_iter().zip(counts) {
            if count > 0 {
                result.insert(format!("findings_{label}"), count.into());
            }
        }
    }
    result
}

fn copy_numbers(value: &Object, keys: &[&str], prefix: &str, result: &mut Object) {
    for &key in keys {
        if let Some(value) = value.get(key).filter(|item| number(item, NUMBER_LIMIT)) {
            result.insert(format!("{prefix}{key}"), value.clone());
        }
    }
}

fn metrics(value: &Object, result: &mut Object) {
    let mut candidates = value
        .iter()
        .take(100)
        .map(|(key, item)| (key.clone(), item))
        .collect::<Vec<_>>();
    for group in ["metrics", "statistics", "usage", "performance"] {
        let Some(items) = value.get(group).and_then(Value::as_object) else {
            continue;
        };
        candidates.extend(
            items
                .iter()
                .take(64)
                .map(|(key, item)| (format!("{group}_{key}"), item)),
        );
        for (key, item) in items
            .iter()
            .take(40)
            .filter_map(|(key, item)| item.as_object().map(|item| (key, item)))
        {
            if let (Some(unit), Some(value)) = (
                text(item, "unit").filter(|unit| UNITS.contains(unit)),
                item.get("value"),
            ) {
                candidates.push((format!("{group}_{key}_{unit}"), value));
            }
        }
    }
    for (key, value) in candidates {
        if result.len() >= 40 {
            break;
        }
        if BLOCKED.iter().any(|blocked| key.contains(blocked)) {
            continue;
        }
        let has_unit = METRIC.is_match(&key)
            || NAME.is_match(&key)
                && key
                    .split('_')
                    .any(|part| matches!(part, "tokens" | "credits"));
        if has_unit && number(value, NUMBER_LIMIT) || FLAG.is_match(&key) && value.is_boolean() {
            result.entry(key).or_insert_with(|| value.clone());
        }
    }
}

fn credits(value: &Object, result: &mut Object) {
    for (prefix, group) in [
        ("", Some(value)),
        ("usage_", value.get("usage").and_then(Value::as_object)),
    ] {
        let Some(group) = group else { continue };
        for key in [
            "credits",
            "credits_used",
            "credits_remaining",
            "credits_balance",
        ] {
            if result.len() < 40 {
                copy_numbers(group, &[key], prefix, result);
            }
        }
        if let Some(credits) = group.get("credits").and_then(Value::as_object) {
            for key in ["balance", "used", "remaining", "limit", "total"] {
                if result.len() >= 40 {
                    break;
                }
                let Some(item) = credits.get(key) else {
                    continue;
                };
                let decimal = item
                    .as_str()
                    .filter(|text| DECIMAL.is_match(text))
                    .and_then(|text| text.parse::<f64>().ok())
                    .map(|value| json!(value));
                let value = decimal.as_ref().unwrap_or(item);
                if number(value, NUMBER_LIMIT) {
                    result.insert(format!("{prefix}credits_{key}"), value.clone());
                }
            }
            for key in ["has_credits", "unlimited"] {
                if result.len() < 40 {
                    if let Some(flag) = credits.get(key).and_then(Value::as_bool) {
                        result.insert(format!("{prefix}credits_{key}"), flag.into());
                    }
                }
            }
        }
        if result.len() >= 40 {
            break;
        }
    }
}

static LINKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"\]\)]+"#).expect("reference pattern"));

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observed {
    pub server: String,
    pub tool: String,
    pub category: Category,
    pub action: Action,
    pub metadata: Object,
    pub result: Object,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Documents,
    Workspace,
    Runtime,
    Workflow,
    Review,
    Deployment,
    Apps,
    Web,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Deployment,
    Change,
    Write,
    Read,
    Operation,
}

pub fn identity(tool: &str) -> Option<(&str, &str)> {
    if matches!(
        tool,
        "web__run" | "web.run" | "web_run" | "functions.web__run"
    ) {
        return Some(("web", "run"));
    }
    let qualified = tool.strip_prefix("mcp__")?;
    qualified.rmatch_indices("__").find_map(|(position, _)| {
        let server = &qualified[..position];
        let operation = &qualified[position + 2..];
        (identifier(server, 80, b"_.-") && identifier(operation, 160, b"_.:-"))
            .then_some((server, operation))
    })
}

pub fn category(server: &str, tool: &str) -> Category {
    if server == "web" {
        return Category::Web;
    }
    let label = format!("{server} {tool}").to_ascii_lowercase();
    for (words, category) in [
        (
            &["deploy", "publish", "runtime_logs"][..],
            Category::Deployment,
        ),
        (
            &["security", "review", "pull_requests_checks"][..],
            Category::Review,
        ),
        (
            &["artifact", "presentation", "spreadsheet", "design"][..],
            Category::Apps,
        ),
        (
            &["browser", "navigate", "screenshot", "repl"][..],
            Category::Runtime,
        ),
        (&["document", "ocr", "pdf"][..], Category::Documents),
        (
            &["workspace", "environment", "validate", "verify"][..],
            Category::Workspace,
        ),
        (&["thread", "worktree", "sidebar"][..], Category::Workflow),
    ] {
        if words.iter().any(|word| label.contains(word)) {
            return category;
        }
    }
    Category::Other
}

fn action(tool: &str) -> Action {
    let tool = tool.to_ascii_lowercase();
    for (words, action) in [
        (&["deploy", "deployment", "publish"][..], Action::Deployment),
        (
            &["delete", "remove", "archive", "cancel", "reset"][..],
            Action::Change,
        ),
        (
            &[
                "create", "update", "write", "edit", "send", "attach", "move", "restore",
                "handoff", "fork",
            ][..],
            Action::Write,
        ),
        (
            &[
                "get", "read", "list", "search", "find", "status", "inspect", "compare", "extract",
                "checks",
            ][..],
            Action::Read,
        ),
    ] {
        if tool
            .split(|ch: char| !ch.is_ascii_lowercase())
            .any(|part| words.contains(&part))
        {
            return action;
        }
    }
    Action::Operation
}

fn request(server: &str, args: Option<&Object>) -> Object {
    let mut metadata = Object::from_iter([("resources".into(), json!(resource_ids(args)))]);
    let Some(args) = args else {
        if server == "web" {
            metadata.insert("references".into(), json!([]));
            metadata.insert("operations".into(), json!([]));
        }
        return metadata;
    };
    if server == "local_documents" {
        if let Some(value) = text(args, "source")
            .filter(|value| !value.is_empty())
            .or_else(|| text(args, "target"))
        {
            let extension = value
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if [
                "pdf", "docx", "pptx", "xlsx", "csv", "txt", "md", "png", "jpg", "jpeg", "webp",
            ]
            .contains(&extension.as_str())
            {
                metadata.insert("format".into(), extension.into());
            }
        }
        if let Some(mode @ ("auto" | "force" | "off")) = text(args, "ocr") {
            metadata.insert("ocr_mode".into(), mode.into());
        }
        for key in ["write", "write_output"] {
            if let Some(flag) = args.get(key).and_then(Value::as_bool) {
                metadata.insert(key.into(), flag.into());
            }
        }
    } else if server == "workspace_inspection" {
        if let Some(limit) = args
            .get("max_files")
            .and_then(Value::as_u64)
            .filter(|limit| *limit <= NUMBER_LIMIT)
        {
            metadata.insert("max_files".into(), limit.into());
        }
    } else if server == "web" {
        let urls = ["open", "click", "find", "screenshot"]
            .into_iter()
            .flat_map(|key| list(Some(args), key).take(30))
            .filter_map(|entry| entry.get("ref_id").and_then(Value::as_str));
        metadata.insert("references".into(), json!(clean_references(urls)));
        let operations: Vec<_> = OPERATIONS
            .iter()
            .filter(|key| {
                args.get(**key)
                    .and_then(Value::as_array)
                    .is_some_and(|items| !items.is_empty())
            })
            .copied()
            .collect();
        metadata.insert("operations".into(), json!(operations));
    }
    metadata
}

fn clean_references<'a>(urls: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut result = Vec::new();
    for url in urls.filter_map(reference) {
        if !result.contains(&url) {
            result.push(url);
            if result.len() == 30 {
                break;
            }
        }
    }
    result
}

impl Observed {
    pub fn new(tool: &str, args: Option<&Object>) -> Option<Self> {
        let (server, tool) = identity(tool)?;
        Some(Self {
            server: server.into(),
            tool: tool.into(),
            category: category(server, tool),
            action: action(tool),
            metadata: request(server, args),
            result: Object::new(),
        })
    }

    pub fn complete(&mut self, output: &Value) {
        self.result = response(&self.server, output_object(output).as_deref());
        if self.server == "web" {
            let encoded;
            let raw = if let Some(raw) = output.as_str() {
                raw
            } else {
                encoded = output.to_string();
                &encoded
            };
            let mut end = raw.len().min(1024 * 1024);
            while !raw.is_char_boundary(end) {
                end -= 1;
            }
            let raw = &raw[..end];
            self.result.insert(
                "references".into(),
                json!(clean_references(LINKS.find_iter(raw).map(|value| {
                    value.as_str().trim_end_matches(['.', ',', ';', '\\'])
                }))),
            );
        }
    }
}

fn output_object(output: &Value) -> Option<std::borrow::Cow<'_, Object>> {
    use std::borrow::Cow;
    fn list(items: Vec<Value>) -> Object {
        Object::from_iter([("content".into(), Value::Array(items))])
    }
    match output {
        Value::Object(value) => Some(Cow::Borrowed(value)),
        Value::Array(items) => Some(Cow::Owned(list(items.clone()))),
        Value::String(raw) if within_output_limit(raw) => {
            match serde_json::from_str::<Value>(raw).ok()? {
                Value::Object(value) => Some(Cow::Owned(value)),
                Value::Array(items) => Some(Cow::Owned(list(items))),
                _ => None,
            }
        }
        _ => None,
    }
}

static EXIT_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^Process exited with code (-?[0-9]{1,6})\s*$").expect("exit code pattern")
});

/// Whether the containing tool reported failure. No raw error text is retained.
pub fn output_failed(output: &Value) -> bool {
    output_error(output, |_| Object::new()).is_some()
}

/// Project failure metadata without retaining the tool response.
pub fn output_error(output: &Value, details: impl FnOnce(&str) -> Object) -> Option<Object> {
    let value = output_object(output)
        .map(|value| decoded(&value))
        .unwrap_or_default();
    let exit = if value.is_empty() {
        output
            .as_str()
            .and_then(|raw| {
                let end = raw
                    .char_indices()
                    .nth(OUTPUT_CHARACTER_LIMIT)
                    .map_or(raw.len(), |(index, _)| index);
                EXIT_CODE.captures(&raw[..end])
            })
            .and_then(|captures| captures[1].parse::<i32>().ok())
    } else {
        value
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok())
    };
    let status = value
        .get("status")
        .or_else(|| value.get("state"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let failed = value.get("isError").and_then(Value::as_bool) == Some(true)
        || value.get("error").is_some_and(truth)
        || exit.is_some_and(|code| code != 0)
        || matches!(
            status.as_deref(),
            Some(
                "error"
                    | "failed"
                    | "timeout"
                    | "missing_dependencies"
                    | "network_unavailable"
                    | "http_error"
                    | "invalid_response"
                    | "response_too_large"
            )
        );
    if !failed {
        return None;
    }
    let safe_identifier = |value: &str| {
        identifier(value, 160, b"_./:-") && !value.starts_with("sk-") && !value.starts_with("sk_")
    };
    let error = value.get("error").and_then(Value::as_object);
    let code = error
        .and_then(|error| text(error, "code"))
        .filter(|value| safe_identifier(value))
        .or_else(|| text(&value, "error_code").filter(|value| safe_identifier(value)))
        .or_else(|| (exit.is_some_and(|code| code != 0)).then_some("process_exit"))
        .or_else(|| status.as_deref().filter(|value| safe_identifier(value)))
        .unwrap_or("tool_error");
    let mut result = Object::from_iter([
        ("code".into(), code.into()),
        ("exit_code".into(), json!(exit)),
    ]);
    if let Some(error) = error {
        if let Some(message) = text(error, "message") {
            result.extend(details(message));
        }
        for key in ["request_id", "trace_id", "error_type"] {
            if let Some(value) = text(error, key).filter(|value| safe_identifier(value)) {
                result.insert(key.into(), value.into());
            }
        }
        if let Some(status) = error
            .get("http_status")
            .or_else(|| error.get("status_code"))
            .and_then(Value::as_i64)
            .filter(|status| (100..=599).contains(status))
        {
            result.insert("http_status".into(), status.into());
        }
    }
    Some(result)
}

fn safe_resources(data: &Value) -> bool {
    data.as_object().is_some_and(|items| {
        items.iter().all(|(key, value)| {
            RESOURCE_KEYS.contains(&key.as_str())
                && value.as_str().is_some_and(|text| {
                    identifier(text, 160, b"_.:-")
                        && !text.starts_with("sk-")
                        && !text.starts_with("sk_")
                })
        })
    })
}

fn safe_references(data: &Value) -> bool {
    data.as_array().is_some_and(|values| {
        values.len() <= 30
            && values.iter().all(|value| {
                value
                    .as_str()
                    .is_some_and(|text| reference(text).as_deref() == Some(text))
            })
    })
}

impl Observed {
    pub fn valid_for(&self, tool: &str) -> bool {
        identity(tool) == Some((self.server.as_str(), self.tool.as_str()))
            && self.category == category(&self.server, &self.tool)
            && self.action == action(&self.tool)
            && self.metadata.iter().all(|(key, value)| match key.as_str() {
                "resources" => safe_resources(value),
                "references" if self.server == "web" => safe_references(value),
                "operations" if self.server == "web" => value.as_array().is_some_and(|items| {
                    items.len() <= OPERATIONS.len()
                        && items.iter().all(|item| {
                            item.as_str().is_some_and(|item| OPERATIONS.contains(&item))
                        })
                }),
                "format" if self.server == "local_documents" => {
                    value.as_str().is_some_and(|format| {
                        [
                            "pdf", "docx", "pptx", "xlsx", "csv", "txt", "md", "png", "jpg",
                            "jpeg", "webp",
                        ]
                        .contains(&format)
                    })
                }
                "ocr_mode" if self.server == "local_documents" => {
                    matches!(value.as_str(), Some("auto" | "force" | "off"))
                }
                "write" | "write_output" if self.server == "local_documents" => value.is_boolean(),
                "max_files" if self.server == "workspace_inspection" => {
                    value.as_u64().is_some_and(|value| value <= NUMBER_LIMIT)
                }
                _ => false,
            })
            && self.result.len() <= 70
            && self
                .result
                .iter()
                .all(|(key, value)| valid_result(&self.server, key, value))
    }
}

fn valid_result(server: &str, key: &str, value: &Value) -> bool {
    if BLOCKED.iter().any(|blocked| key.contains(blocked)) {
        return false;
    }
    if RESOURCE_KEYS.contains(&key) {
        return value
            .as_str()
            .is_some_and(|text| identifier(text, 160, b"_.:-"));
    }
    match key {
        "references" => server == "web" && safe_references(value),
        "status" => value.as_str().is_some_and(|status| STATUS.is_match(status)),
        "ocr_status" => {
            server == "local_documents"
                && value
                    .as_str()
                    .is_some_and(|status| STATUSES.contains(&status))
        }
        "written"
        | "truncated"
        | "cache_hit"
        | "recording_enabled"
        | "telemetry_enabled"
        | "credits_has_credits"
        | "credits_unlimited"
        | "usage_credits_has_credits"
        | "usage_credits_unlimited" => value.is_boolean(),
        "latency_ms"
        | "duration_ms"
        | "http_attempts"
        | "retries"
        | "input_tokens"
        | "output_tokens"
        | "content_chars"
        | "ocr_eligible_items"
        | "ocr_processed_items"
        | "ocr_omitted_items"
        | "ocr_errors"
        | "evidence_runs"
        | "findings"
        | "findings_critical"
        | "findings_high"
        | "findings_medium"
        | "findings_low" => number(value, NUMBER_LIMIT),
        _ => {
            let has_unit = METRIC.is_match(key)
                || NAME.is_match(key)
                    && key
                        .split('_')
                        .any(|part| matches!(part, "tokens" | "credits"));
            has_unit && number(value, NUMBER_LIMIT) || FLAG.is_match(key) && value.is_boolean()
        }
    }
}
