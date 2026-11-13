use chrono::{DateTime, Utc};
use lam_history_store::{mcp, parse_utc_stamp};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

pub(super) const EVENT_LIMIT: usize = 1000;
pub(super) const TOOL_LIMIT: usize = 30;
type RecordingState<'a> = (Option<&'a str>, Value);
const ERROR_STATES: [&str; 6] = [
    "error",
    "failed",
    "timeout",
    "cancelled",
    "canceled",
    "missing_dependencies",
];

fn field<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn last(event: &Value) -> Option<&str> {
    field(event, "completed_at").or_else(|| field(event, "timestamp"))
}

pub fn summary(events: &[Value], cutoff: Option<DateTime<Utc>>, health: &str) -> Value {
    let visible: Vec<_> = events
        .iter()
        .filter(|event| {
            cutoff.is_none_or(|cutoff| {
                field(event, "timestamp")
                    .and_then(parse_utc_stamp)
                    .is_some_and(|time| time >= cutoff)
            })
        })
        .collect();
    let mut grouped = BTreeMap::<&str, Vec<&Value>>::new();
    for event in &visible {
        if let Some(server) = field(event, "server") {
            grouped.entry(server).or_default().push(event);
        }
    }
    let mut servers = Vec::new();
    let mut recording = Vec::new();
    for (server, events) in grouped {
        let (row, state) = server_summary(server, &events);
        servers.push(row);
        if let Some((time, state)) = state {
            recording.push((time, server, state));
        }
    }
    recording.sort_by_key(|(time, server, _)| (*time, *server));
    let recording: Map<_, _> = recording
        .into_iter()
        .map(|(_, server, state)| (server.into(), state))
        .collect();
    json!({
        "servers":servers, "events":visible.iter().take(EVENT_LIMIT).collect::<Vec<_>>(),
        "total_events":visible.len(), "history_health":health, "recording_status":recording,
        "categories":{
            "documents":"文件 / OCR", "workspace":"環境 / 驗證", "runtime":"Runtime / 瀏覽器",
            "workflow":"Codex 工作流程", "review":"Review / Security", "deployment":"部署",
            "apps":"App / 成果", "jev":"Jev", "web":"網路參考", "other":"其他 MCP"
        }
    })
}

pub fn disabled_server(server: &str) -> Value {
    let mut row = server_summary(server, &[]).0;
    row["enabled"] = false.into();
    row["connection"]["state"] = json!("paused");
    row
}

/// Read only configured source identities and bounded descriptions.
pub fn discover(home: &Path) -> BTreeMap<String, Option<super::source_introduction::Description>> {
    let mut found = BTreeMap::new();
    let path = home.join("config.toml");
    let Ok(mut file) = std::fs::File::open(path) else {
        return found;
    };
    if file
        .metadata()
        .ok()
        .is_none_or(|meta| !meta.is_file() || meta.len() > 1024 * 1024)
    {
        return found;
    }
    let mut bytes = Vec::new();
    if (&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 1024 * 1024
    {
        return found;
    }
    let Ok(content) = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
    else {
        return found;
    };
    let Ok(config) = content.parse::<toml::Value>() else {
        return found;
    };
    if let Some(servers) = config.get("mcp_servers").and_then(toml::Value::as_table) {
        for (index, (name, value)) in servers.iter().enumerate() {
            let Some(table) = value.as_table() else {
                continue;
            };
            if !(1..=80).contains(&name.len())
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                || table
                    .get("enabled")
                    .is_some_and(|enabled| enabled.as_bool() != Some(true))
            {
                continue;
            }
            let description = (index < 64)
                .then(|| table.get("description").and_then(toml::Value::as_str))
                .flatten()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| super::source_introduction::Description {
                    text: super::details::redact_description(
                        &text.chars().take(400).collect::<String>(),
                    ),
                    source: "config".into(),
                    path: None,
                })
                .or_else(|| {
                    (index < 64)
                        .then(|| super::source_introduction::introduction(table))
                        .flatten()
                });
            found.insert(name.clone(), description);
        }
    }
    if let Some(plugins) = config.get("plugins").and_then(toml::Value::as_table) {
        for (name, value) in plugins {
            let Some(table) = value.as_table() else {
                continue;
            };
            if table
                .get("enabled")
                .is_some_and(|enabled| enabled.as_bool() != Some(true))
            {
                continue;
            }
            let source = match name.split('@').next().unwrap_or("") {
                "codex-app-tools" => "codex_app",
                "code-review" => "code_review",
                "codex-security" => "codex_security",
                "unified-computer-use" => "cua_repl",
                _ => continue,
            };
            found.entry(source.into()).or_insert(None);
        }
    }
    found
}

pub fn configured_server(server: &str, enabled: bool) -> Value {
    let mut row = server_summary(server, &[]).0;
    row["origin"] = "configured".into();
    row["enabled"] = enabled.into();
    if !enabled {
        row["connection"]["state"] = json!("paused");
    }
    row
}

pub fn apply_diagnostics(summary: &mut Value, diagnostics: &[Value]) {
    let mut failures = BTreeMap::<&str, &Value>::new();
    for event in diagnostics {
        let (Some(server), Some(code)) = (field(event, "server"), field(event, "code")) else {
            continue;
        };
        if ![
            "network_unavailable",
            "transport_error",
            "connection_closed",
            "connection_refused",
            "authentication_failed",
        ]
        .contains(&code)
        {
            continue;
        }
        let timestamp = field(event, "timestamp").unwrap_or("");
        if failures
            .get(server)
            .is_none_or(|previous| timestamp > field(previous, "timestamp").unwrap_or(""))
        {
            failures.insert(server, event);
        }
    }
    for row in summary["servers"].as_array_mut().into_iter().flatten() {
        let Some(server) = row["server"].as_str() else {
            continue;
        };
        let Some(failure) = failures.get(server) else {
            continue;
        };
        let timestamp = field(failure, "timestamp").unwrap_or("");
        let prior = field(&row["connection"], "observed_at").unwrap_or("");
        if timestamp > prior {
            row["connection"]["observed_at"] = json!(timestamp);
            if row["enabled"] == true {
                row["connection"]["state"] = json!("error");
                row["connection"]["error_code"] = failure["code"].clone();
            }
        }
    }
}

fn server_summary<'a>(server: &str, events: &[&'a Value]) -> (Value, Option<RecordingState<'a>>) {
    let direct: Vec<_> = events
        .iter()
        .copied()
        .filter(|event| event["nested"].as_bool() != Some(true))
        .collect();
    let mut durations: Vec<_> = direct
        .iter()
        .filter_map(|event| event["duration_ms"].as_f64())
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .collect();
    durations.sort_by(f64::total_cmp);
    let statuses: Vec<_> = direct
        .iter()
        .filter_map(|event| field(&event["result"], "status"))
        .collect();
    let returned = direct
        .iter()
        .filter_map(|event| field(event, "completed_at"))
        .collect::<Vec<_>>();
    let pending = direct
        .iter()
        .filter(|event| field(event, "completed_at").is_none())
        .collect::<Vec<_>>();
    let pending_at = pending
        .iter()
        .filter_map(|event| field(event, "timestamp"))
        .max();
    let last_response = returned.iter().copied().max();
    let connection = if pending_at > last_response {
        "pending"
    } else if last_response.is_some() {
        "response"
    } else {
        "unconfirmed"
    };
    let tools: BTreeSet<_> = events
        .iter()
        .filter_map(|event| field(event, "tool"))
        .collect();
    let (metrics, recording) = measurements(events);
    let average = (!durations.is_empty())
        .then(|| (durations.iter().sum::<f64>() / durations.len() as f64).round_ties_even());
    let p95 = (!durations.is_empty())
        .then(|| durations[(durations.len() * 95 / 100).min(durations.len() - 1)]);
    let p99 = (!durations.is_empty()).then(|| {
        let position = (durations.len() - 1) as f64 * 0.99;
        let lower = position as usize;
        let next = (lower + 1).min(durations.len() - 1);
        let value =
            durations[lower] + (durations[next] - durations[lower]) * (position - lower as f64);
        (value * 100.0).round_ties_even() / 100.0
    });
    (
        json!({
            "server":server, "origin":"observed", "category":mcp::category(server, ""), "enabled":true,
            "calls":direct.len(), "recognized":events.len()-direct.len(), "returned":returned.len(),
            "known_status":statuses.len(), "errors":statuses.iter().filter(|status| ERROR_STATES.contains(status)).count(),
            "average_ms":average, "p95_ms":p95, "p99_ms":p99, "last_at":events.iter().filter_map(|event| last(event)).max(),
            "latest_metrics":metrics, "tools":tools.into_iter().take(TOOL_LIMIT).collect::<Vec<_>>(),
            "connection":{"state":connection,"observed_at":if connection == "pending" {pending_at} else {last_response},
                          "last_response_at":last_response,"pending_calls":pending.len(),"error_code":null,"method":"local_records"}
        }),
        recording,
    )
}

fn measurements<'a>(events: &[&'a Value]) -> (Map<String, Value>, Option<RecordingState<'a>>) {
    let mut ordered = events.to_vec();
    ordered.sort_by_key(|event| last(event));
    let mut metrics = Map::new();
    let mut flags = Map::new();
    let mut selected = None;
    let mut first = None;
    for event in ordered {
        let Some(result) = event["result"].as_object() else {
            continue;
        };
        let time = last(event);
        let mut observed = Vec::new();
        for (key, value) in result {
            if (value.is_boolean() || mcp::number(value, 1 << 53))
                && (metrics.len() < 32 || metrics.contains_key(key))
            {
                metrics.insert(key.clone(), json!({"value":value,"timestamp":time}));
            }
            if value.is_boolean()
                && (key.ends_with("recording_enabled") || key.ends_with("telemetry_enabled"))
            {
                observed.push(key);
                flags.insert(
                    key.clone(),
                    json!({"enabled":value,"health":result.get("status"),"observed_at":time}),
                );
            }
        }
        if let Some(key) = observed
            .into_iter()
            .min_by_key(|key| (!key.ends_with("recording_enabled"), *key))
        {
            first.get_or_insert(time);
            selected = Some(key);
        }
    }
    let recording = selected.and_then(|key| {
        let mut current = flags.get(key)?.as_object()?.clone();
        current.insert("flags".into(), Value::Object(flags));
        current.insert("key".into(), key.as_str().into());
        Some((first.flatten(), Value::Object(current)))
    });
    (metrics, recording)
}

/// Server identity and connection state are independent of the selected time window.
pub fn scoped_summary(events: &[Value], cutoff: DateTime<Utc>, health: &str, all: &Value) -> Value {
    let mut scoped = summary(events, Some(cutoff), health);
    let rows = scoped["servers"]
        .as_array_mut()
        .expect("server summary array");
    let indexes: BTreeMap<String, usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| Some((field(row, "server")?.to_owned(), index)))
        .collect();
    for server in all["servers"].as_array().into_iter().flatten() {
        let Some(name) = field(server, "server") else {
            continue;
        };
        if let Some(&index) = indexes.get(name) {
            rows[index]["connection"] = server["connection"].clone();
            for key in [
                "origin",
                "enabled",
                "description",
                "description_source",
                "default_description",
                "default_category",
                "tags",
            ] {
                if let Some(value) = server.get(key) {
                    rows[index][key] = value.clone();
                }
            }
        } else {
            let mut row = server.clone();
            for key in ["calls", "recognized", "returned", "known_status", "errors"] {
                row[key] = 0.into();
            }
            for key in ["average_ms", "p95_ms", "p99_ms", "last_at"] {
                row[key] = Value::Null;
            }
            row["latest_metrics"] = json!({});
            rows.push(row);
        }
    }
    scoped["recording_status"] = all["recording_status"].clone();
    if let Some(configuration) = all.get("configuration") {
        scoped["configuration"] = configuration.clone();
    }
    scoped
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    #[test]
    fn named_transport_failures_update_only_newer_server_evidence() {
        let mut value = json!({"servers":[
            {"server":"alpha","enabled":true,"connection":{"state":"response","observed_at":"2026-01-01T00:00:01Z","last_response_at":"2026-01-01T00:00:01Z","pending_calls":0,"error_code":null,"method":"local_records"}},
            {"server":"beta","enabled":false,"connection":{"state":"paused","observed_at":null,"last_response_at":null,"pending_calls":0,"error_code":null,"method":"local_records"}}
        ]});
        let errors = vec![
            json!({"server":"alpha","code":"connection_refused","timestamp":"2026-01-01T00:00:01Z"}),
            json!({"server":"alpha","code":"unrelated","timestamp":"2026-01-01T00:00:09Z"}),
            json!({"server":"beta","code":"transport_error","timestamp":"2026-01-01T00:00:02Z"}),
        ];
        apply_diagnostics(&mut value, &errors);
        assert_eq!(value["servers"][0]["connection"]["state"], "response");
        assert_eq!(value["servers"][1]["connection"]["state"], "paused");
        assert_eq!(
            value["servers"][1]["connection"]["observed_at"],
            "2026-01-01T00:00:02Z"
        );
        apply_diagnostics(
            &mut value,
            &[
                json!({"server":"alpha","code":"network_unavailable","timestamp":"2026-01-01T00:00:03Z"}),
            ],
        );
        assert_eq!(value["servers"][0]["connection"]["state"], "error");
        assert_eq!(
            value["servers"][0]["connection"]["error_code"],
            "network_unavailable"
        );
        assert_eq!(
            value["servers"][0]["connection"]["last_response_at"],
            "2026-01-01T00:00:01Z"
        );
    }
}
