use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::LazyLock;

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([\w.]{1,64})=("(?:\\.|[^"\\])*(?:"|$)|[^\s{},:"]+)|"(?:\\.|[^"\\])*(?:"|$)"#)
        .expect("diagnostic tokens")
});
static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,160}$").unwrap());
static ENDPOINT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/[a-zA-Z0-9_/-]{1,80}$").unwrap());
static UUID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$").unwrap());
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\p{Nd}{1,16}(?:\.\p{Nd}{1,6})?$").unwrap());
static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Nd}$").unwrap());
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[1-5]\p{Nd}{2}$").unwrap());

fn decimal_ascii(value: &str) -> String {
    value
        .chars()
        .map(|digit| {
            if digit.is_ascii() {
                return digit;
            }
            let code = digit as u32;
            let mut first = code;
            while let Some(previous) = first.checked_sub(1).and_then(char::from_u32) {
                if !DIGIT.is_match(previous.encode_utf8(&mut [0; 4])) {
                    break;
                }
                first -= 1;
            }
            char::from(b'0' + ((code - first) % 10) as u8)
        })
        .collect()
}

pub fn project(module: &str, body: &str) -> Option<Map<String, Value>> {
    if ![
        "codex_api::",
        "codex_otel.log_only",
        "codex_core::responses_retry",
    ]
    .iter()
    .any(|prefix| module.starts_with(prefix))
    {
        return None;
    }
    let end = body
        .char_indices()
        .nth(8192)
        .map_or(body.len(), |(index, _)| index);
    let body = &body[..end];
    let mut fields = BTreeMap::<String, String>::new();
    let mut heading = String::new();
    let mut previous = 0;
    for capture in TOKEN.captures_iter(body) {
        let whole = capture.get(0)?;
        if capture.get(1).is_some()
            && body[..whole.start()]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
        {
            if let Some(value) = capture
                .get(2)
                .filter(|value| value.as_str().starts_with('"'))
            {
                heading.push_str(&body[previous..value.start()]);
                previous = value.end();
            }
            continue;
        }
        heading.push_str(&body[previous..whole.start()]);
        previous = whole.end();
        if let (Some(key), Some(value)) = (capture.get(1), capture.get(2)) {
            let raw = value.as_str();
            let parsed = if raw.starts_with('"') {
                serde_json::from_str::<String>(raw).ok()
            } else {
                Some(raw.to_owned())
            };
            if let Some(value) = parsed {
                fields.insert(key.as_str().to_owned(), value);
            }
        }
    }
    heading.push_str(&body[previous..]);
    let event = fields.get("event.name").map(String::as_str);
    let (operation, kind, mut status) = match event {
        Some(name @ ("codex.api_request" | "codex.websocket.request")) => {
            let status = match fields.get("success").map(String::as_str) {
                Some("true") if name == "codex.websocket.request" => "sent",
                Some("true") => "returned",
                Some("false") => "failed",
                _ => "unknown",
            };
            (name, "request", status)
        }
        _ if module == "codex_core::responses_retry" && fields.contains_key("retries") => {
            ("stream_retry", "retry", "retrying")
        }
        _ if module == "codex_api::endpoint::responses_websocket" => {
            let status = if heading.contains("successfully connected to websocket") {
                "connected"
            } else if heading.contains("failed to connect to websocket") {
                "failed"
            } else if heading.contains("connecting to websocket") {
                "connecting"
            } else {
                return None;
            };
            ("websocket_connection", "connection", status)
        }
        _ => return None,
    };
    if kind == "request"
        && fields
            .get("endpoint")
            .or_else(|| fields.get("api.path"))
            .is_some_and(|value| {
                ![
                    "/responses",
                    "responses",
                    "/v1/responses",
                    "/chat/completions",
                    "/v1/chat/completions",
                ]
                .contains(&value.as_str())
            })
    {
        return None;
    }
    let mut result = Map::new();
    for (key, aliases) in [
        ("model", &["model"][..]),
        ("provider", &["provider"][..]),
        ("request_id", &["request_id", "request.id"][..]),
        ("trace_id", &["trace_id", "trace.id"][..]),
        ("turn_id", &["turn_id", "turn.id"][..]),
        (
            "thread_id",
            &["thread_id", "thread.id", "conversation.id"][..],
        ),
        ("transport", &["transport"][..]),
        ("endpoint", &["endpoint", "api.path"][..]),
    ] {
        if let Some(value) = aliases.iter().find_map(|alias| fields.get(*alias)) {
            let valid = if key == "endpoint" {
                ENDPOINT.is_match(value)
            } else {
                IDENTIFIER.is_match(value)
            };
            if valid
                && !value.starts_with("sk-")
                && !value.starts_with("sk_")
                && (key != "thread_id" || UUID.is_match(value))
            {
                result.insert(key.into(), value.clone().into());
            }
        }
    }
    for key in [
        "duration_ms",
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "retries",
        "attempt",
    ] {
        if let Some(value) = fields.get(key).filter(|value| NUMBER.is_match(value)) {
            if kind != "request"
                && matches!(
                    key,
                    "duration_ms" | "input_tokens" | "output_tokens" | "total_tokens"
                )
            {
                continue;
            }
            let value = decimal_ascii(value);
            if value.contains('.') {
                if key == "duration_ms" {
                    if let Ok(number) = value.parse::<f64>() {
                        if number <= 9_007_199_254_740_991.0 {
                            result.insert(key.into(), json!(number));
                        }
                    }
                }
            } else if let Ok(number) = value.parse::<u64>() {
                if number <= 9_007_199_254_740_991 {
                    result.insert(key.into(), number.into());
                }
            }
        }
    }
    for key in ["http_status", "status_code", "http.status_code"] {
        if let Some(code) = fields
            .get(key)
            .filter(|value| STATUS.is_match(value))
            .and_then(|value| decimal_ascii(value).parse::<u16>().ok())
            .filter(|code| (100..=599).contains(code))
        {
            result.insert("http_status".into(), code.into());
            if kind == "request" {
                status = if code >= 400 {
                    "failed"
                } else if event == Some("codex.websocket.request") {
                    "sent"
                } else {
                    "returned"
                };
            }
            break;
        }
    }
    result.insert("event_kind".into(), kind.into());
    result.insert("operation".into(), operation.into());
    result.insert("status".into(), status.into());
    Some(result)
}

pub fn summary(
    events: &VecDeque<Value>,
    boundary: Option<DateTime<Utc>>,
    enabled: bool,
    source: Value,
) -> Value {
    let mut selected: Vec<_> = events
        .iter()
        .filter(|event| {
            boundary.is_none_or(|cutoff| {
                event["timestamp"]
                    .as_str()
                    .and_then(lam_history_store::parse_utc_stamp)
                    .is_some_and(|time| time >= cutoff)
            })
        })
        .collect();
    selected.sort_by(|a, b| b["timestamp"].as_str().cmp(&a["timestamp"].as_str()));
    let count = |kind| {
        selected
            .iter()
            .filter(|event| event["event_kind"] == kind)
            .count()
    };
    let failed = selected
        .iter()
        .filter(|event| event["event_kind"] == "request" && event["status"] == "failed")
        .count();
    json!({"enabled":enabled,"events":selected,"requests":count("request"),"connections":count("connection"),"retry_events":count("retry"),"failed_requests":failed,"limit":1000,"sources":[source]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_only_valid_metadata_and_ignores_quoted_spoofs() {
        let oversized_key = format!("{}=\"connecting to websocket\"", "a".repeat(65));
        assert!(project("codex_api::endpoint::responses_websocket", &oversized_key).is_none());
        let truncated = format!("{}=\"connecting to websocket", "a".repeat(65));
        assert!(project("codex_api::endpoint::responses_websocket", &truncated).is_none());
        let observed = format!("connecting to websocket {oversized_key}");
        assert_eq!(
            project("codex_api::endpoint::responses_websocket", &observed).unwrap()["status"],
            "connecting"
        );
        assert!(project(
            "codex_otel.log_only",
            r#"message="event.name=codex.api_request""#
        )
        .is_none());
        assert!(project(
            "codex_otel.log_only",
            r#"event.name="codex.api_request" endpoint="/models""#
        )
        .is_none());
        assert!(project(
            "codex_api::endpoint::responses_websocket",
            r#"errorMessage="connecting to websocket""#
        )
        .is_none());
        let value = project("codex_otel.log_only", r#"event.name="codex.api_request" success="false" duration_ms=-1 input_tokens=NaN output_tokens=1.5 total_tokens=9999999999999999 status_code=429 request_id=sk-PRIVATE thread_id=PRIVATE"#).unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["http_status"], 429);
        for key in [
            "duration_ms",
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "request_id",
            "thread_id",
        ] {
            assert!(!value.contains_key(key));
        }
    }

    #[test]
    fn projects_unicode_decimal_digits_like_python() {
        let value = project(
            "codex_otel.log_only",
            "event.name=codex.api_request input_tokens=１２ duration_ms=١.٥ status_code=4٢٩",
        )
        .unwrap();
        assert_eq!(value["input_tokens"], 12);
        assert_eq!(value["duration_ms"], 1.5);
        assert_eq!(value["http_status"], 429);
        assert_eq!(value["status"], "failed");
    }
}
