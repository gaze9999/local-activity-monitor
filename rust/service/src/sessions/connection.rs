//! Connection evidence derived only from already collected metadata.
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

pub(super) fn status<'a>(
    responses: impl IntoIterator<Item = DateTime<Utc>>,
    errors: impl IntoIterator<Item = &'a Value>,
    enabled: bool,
    now: DateTime<Utc>,
) -> Value {
    let mut result = json!({"state":if enabled {"unknown"} else {"paused"},"observed_at":null,"last_response_at":null,"evidence":null,"recent_seconds":300});
    if !enabled {
        return result;
    }
    let mut latest: Option<(DateTime<Utc>, bool, String)> = None;
    let mut last_response: Option<DateTime<Utc>> = None;
    for time in responses {
        if time > now {
            continue;
        }
        if last_response.is_none_or(|old| time > old) {
            last_response = Some(time)
        }
        if latest
            .as_ref()
            .is_none_or(|(old, is_error, _)| time > *old || (time == *old && !is_error))
        {
            latest = Some((time, false, super::project::stamp(time)));
        }
    }
    for event in errors {
        if !matches!(event["category"].as_str(), Some("conversation" | "codex"))
            || !matches!(
                event["source"].as_str(),
                Some("codex_core" | "codex_desktop" | "session")
            )
        {
            continue;
        }
        if !(event["reason"] == "stream_interrupted"
            || matches!(
                event["code"].as_str(),
                Some(
                    "stream_interrupted"
                        | "stream_disconnected"
                        | "connection_refused"
                        | "network_unavailable"
                )
            )
            || matches!(
                event["cause"].as_str(),
                Some("connection_refused" | "stream_interrupted")
            )
            || (event["reason"] == "message_submit_failed" && event["cause"] == "timeout"))
        {
            continue;
        }
        let Some(raw) = event["timestamp"].as_str() else {
            continue;
        };
        let Some(time) = lam_history_store::parse_utc_stamp(raw).filter(|time| *time <= now) else {
            continue;
        };
        if latest.as_ref().is_none_or(|(old, _, _)| time >= *old) {
            latest = Some((time, true, raw.to_owned()));
        }
    }
    result["last_response_at"] = json!(last_response.map(super::project::stamp));
    if let Some((time, error, raw)) = latest {
        result["observed_at"] = json!(raw);
        result["evidence"] = json!(if error {
            "connection_error"
        } else {
            "model_usage"
        });
        if now.signed_duration_since(time) <= chrono::Duration::seconds(300) {
            result["state"] = json!(if error { "error" } else { "response" });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn time(value: &str) -> DateTime<Utc> {
        lam_history_store::parse_utc_stamp(value).unwrap()
    }
    #[test]
    fn selects_recent_evidence_and_prefers_error_on_ties() {
        let now = time("2026-10-05T01:10:00Z");
        let response = time("2026-10-05T01:09:00Z");
        let event = json!({"timestamp":"2026-10-05T01:09:00Z","category":"conversation","source":"session","code":"stream_disconnected"});
        let status = status([response], [&event], true, now);
        assert_eq!(status["state"], "error");
        assert_eq!(status["last_response_at"], "2026-10-05T01:09:00.000Z");
        assert_eq!(status["evidence"], "connection_error");
    }
    #[test]
    fn ignores_unrelated_invalid_future_and_keeps_stale_evidence() {
        let now = time("2026-10-05T01:10:00Z");
        let old = time("2026-10-05T01:00:00Z");
        let future = time("2026-10-05T01:11:00Z");
        let unrelated = json!({"timestamp":"2026-10-05T01:09:00Z","category":"tool","source":"session","code":"stream_interrupted"});
        let value = status([old, future], [&unrelated], true, now);
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["evidence"], "model_usage");
        assert_eq!(status([old], [&unrelated], false, now)["state"], "paused");
    }
    #[test]
    fn recent_boundary_is_exact() {
        let now = time("2026-10-05T01:10:00Z");
        let edge = time("2026-10-05T01:05:00Z");
        let stale = time("2026-10-05T01:04:59.999999Z");
        assert_eq!(
            status([edge], std::iter::empty(), true, now)["state"],
            "response"
        );
        assert_eq!(
            status([stale], std::iter::empty(), true, now)["state"],
            "unknown"
        );
    }
}
