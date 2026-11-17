use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::LazyLock;

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([\p{L}\p{N}_.]{1,64})=("(?:\\.|[^"\\])*(?:"|$)|[^\s\x1c-\x1f{},:"]+)|"(?:\\.|[^"\\])*(?:"|$)"#)
        .expect("diagnostic tokens")
});
static TOKEN_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_.]$").unwrap());
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
                .is_some_and(|ch| TOKEN_WORD.is_match(ch.encode_utf8(&mut [0; 4])))
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
        .filter(|_| enabled)
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

// One retained arrival sequence for both diagnostic sources, like Python observe_api.
// The caller commits Desktop observations before Core observations each round.
pub fn collect(retained: &mut VecDeque<Value>, core: &mut VecDeque<Value>, desktop: &mut Vec<Value>) -> bool {
    let mut changed = false;
    for event in desktop.drain(..).chain(core.drain(..)) {
        let Some(id) = event["observation_id"].as_str() else { continue };
        if retained.iter().any(|old| old["observation_id"].as_str() == Some(id)) {
            continue;
        }
        if retained.len() == 1000 {
            let (index, oldest) = retained.iter().enumerate()
                .min_by_key(|(_, value)| value["timestamp"].as_str().unwrap_or_default())
                .expect("full API event queue");
            if event["timestamp"].as_str().unwrap_or_default() < oldest["timestamp"].as_str().unwrap_or_default() {
                continue;
            }
            retained.remove(index);
        }
        retained.push_back(event);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_event(id: &str, timestamp: &str, kind: &str) -> Value {
        json!({"observation_id":id,"timestamp":timestamp,"event_kind":kind,"status":"returned"})
    }

    #[test]
    fn api_retention_preserves_source_and_cross_round_ties_without_replaying_duplicates() {
        let timestamp = "2026-11-12T00:00:00Z";
        let mut retained = VecDeque::new();
        let mut core = VecDeque::from([api_event("a", timestamp, "request")]);
        let mut desktop = vec![api_event("z", timestamp, "connection")];
        assert!(collect(&mut retained, &mut core, &mut desktop));
        assert!(core.is_empty() && desktop.is_empty());
        assert_eq!(retained.iter().map(|v| v["observation_id"].as_str().unwrap()).collect::<Vec<_>>(), ["z", "a"]);
        core.push_back(api_event("a", timestamp, "retry"));
        desktop.push(api_event("b", timestamp, "retry"));
        assert!(collect(&mut retained, &mut core, &mut desktop));
        let value = summary(&retained, None, true, json!({}));
        assert_eq!(value["events"].as_array().unwrap().iter().map(|v| v["observation_id"].as_str().unwrap()).collect::<Vec<_>>(), ["z", "a", "b"]);
        assert_eq!(value["requests"], 1);
        assert_eq!(value["connections"], 1);
        assert_eq!(value["retry_events"], 1);
        assert!(!collect(&mut retained, &mut core, &mut desktop));
        assert_eq!(summary(&retained, None, true, json!({})), value);
    }

    #[test]
    fn api_retention_applies_one_global_cap_in_arrival_order() {
        let timestamp = "2026-11-12T00:00:00Z";
        let mut retained = VecDeque::new();
        let mut core = (0..1000).map(|i| api_event(&format!("a{i:04}"), timestamp, "request")).collect();
        let mut desktop = Vec::new();
        assert!(collect(&mut retained, &mut core, &mut desktop));
        desktop.push(api_event("z", timestamp, "connection"));
        assert!(collect(&mut retained, &mut core, &mut desktop));
        assert_eq!(retained.len(), 1000);
        assert_eq!(retained.front().unwrap()["observation_id"], "a0001");
        assert_eq!(retained.back().unwrap()["observation_id"], "z");
        let value = summary(&retained, None, true, json!({}));
        assert_eq!(value["requests"], 999);
        assert_eq!(value["connections"], 1);
        desktop.push(api_event("old", "2026-11-11T00:00:00Z", "retry"));
        core.push_back(api_event("a0001", timestamp, "retry"));
        assert!(!collect(&mut retained, &mut core, &mut desktop));
        assert_eq!(summary(&retained, None, true, json!({})), value);
        retained.clear();
        core.push_back(api_event("a0001", timestamp, "request"));
        assert!(collect(&mut retained, &mut core, &mut desktop));
        assert_eq!(retained.len(), 1);
    }

    #[test]
    fn diagnostic_precedence_matches_python_structural_oracles() {
        // Fixed outputs from Python api_diagnostic, not generated by the Rust projector.
        let cases: Value = serde_json::from_str(r###"[
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request success=false http_status=200 status_code=500",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "returned",
      "http_status": 200
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.websocket.request success=false http_status=invalid status_code=201 http.status_code=500",
    "expected": {
      "event_kind": "request",
      "operation": "codex.websocket.request",
      "status": "sent",
      "http_status": 201
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request success=true http_status=404",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "failed",
      "http_status": 404
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request model=first model=last request_id=sk-private request.id=valid",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "unknown",
      "model": "last"
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request thread_id=invalid thread.id=11111111-1111-1111-1111-111111111111",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "unknown"
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request endpoint=/models api.path=/responses",
    "expected": null
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request api.path=/v1/responses duration_ms=1.5 input_tokens=2 output_tokens=3.0 total_tokens=9007199254740991 attempt=0 retries=2",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "unknown",
      "endpoint": "/v1/responses",
      "duration_ms": 1.5,
      "input_tokens": 2,
      "total_tokens": 9007199254740991,
      "retries": 2,
      "attempt": 0
    }
  },
  {
    "module": "codex_core::responses_retry",
    "body": "retries=2 duration_ms=1 input_tokens=2 output_tokens=3 total_tokens=5 attempt=3 status_code=429",
    "expected": {
      "event_kind": "retry",
      "operation": "stream_retry",
      "status": "retrying",
      "retries": 2,
      "attempt": 3,
      "http_status": 429
    }
  },
  {
    "module": "codex_api::endpoint::responses_websocket",
    "body": "successfully connected to websocket duration_ms=1 input_tokens=2 status_code=500",
    "expected": {
      "event_kind": "connection",
      "operation": "websocket_connection",
      "status": "connected",
      "http_status": 500
    }
  },
  {
    "module": "codex_otel.log_only",
    "body": "event.name=codex.api_request model=\"valid\" model=\"unterminated",
    "expected": {
      "event_kind": "request",
      "operation": "codex.api_request",
      "status": "unknown",
      "model": "valid"
    }
  }
]"###).unwrap();
        for case in cases.as_array().unwrap() {
            let actual = project(case["module"].as_str().unwrap(), case["body"].as_str().unwrap());
            assert_eq!(actual.map(Value::Object).unwrap_or(Value::Null), case["expected"], "{}", case["body"]);
        }
    }

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

    #[test]
    fn projects_metadata_separated_by_python_control_whitespace() {
        for separator in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let body = format!("event.name=codex.api_request{separator}input_tokens=7{separator}output_tokens=9{separator}status_code=200");
            let value = project("codex_otel.log_only", &body).unwrap();
            assert_eq!(value["input_tokens"], 7);
            assert_eq!(value["output_tokens"], 9);
            assert_eq!(value["status"], "returned");
            assert_eq!(value["http_status"], 200);
        }
    }
}

#[cfg(test)]
mod token_word_tests {
    use super::*;

    #[test]
    fn token_word_boundaries_follow_python_alphanumeric_keys() {
        for prefix in ["\u{0301}", "\u{203f}", "\u{200d}", "\u{0345}", "\u{05b0}", "\u{093e}"] {
            let body = format!("{prefix}event.name=codex.api_request success=true");
            let value = project("codex_api::probe", &body).unwrap();
            assert_eq!(value["status"], "returned");
            assert_eq!(value["event_kind"], "request");
        }
        for prefix in ["é", "中", "²", "_", "."] {
            let body = format!("{prefix}event.name=codex.api_request success=true");
            assert!(project("codex_api::probe", &body).is_none());
        }
    }
}
