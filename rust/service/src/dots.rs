use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

const LIMIT: u64 = 16 * 1024 * 1024;
fn thread(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn host(id: &str) -> bool {
    id == "local" || suffix(id, "remote-control:")
}
fn cloud(id: &str) -> bool {
    id == "durable" || suffix(id, "chatgpt:")
}
fn suffix(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|s| {
        !s.is_empty()
            && s.len() <= 160
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn date(seconds: Option<f64>) -> Value {
    let Some(millis) = seconds
        .map(|n| (n * 1000.0).floor())
        .filter(|n| n.is_finite() && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
    else {
        return Value::Null;
    };
    DateTime::<Utc>::from_timestamp_millis(millis as i64)
        .map(|date| json!(date.to_rfc3339_opts(SecondsFormat::Millis, true)))
        .unwrap_or(Value::Null)
}
fn newest(slot: &mut Option<String>, time: &Value) {
    if let Some(value) = time.as_str().filter(|value| !value.is_empty()) {
        if slot.as_deref().unwrap_or("") < value {
            *slot = Some(value.to_owned());
        }
    }
}
pub(crate) fn read_state(home: &Path) -> Option<Value> {
    read_state_with_health(home).ok()
}

pub(crate) fn read_state_with_health(home: &Path) -> Result<Value, &'static str> {
    let path = home.join(".codex-global-state.json");
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "missing"
        } else {
            "unavailable"
        }
    })?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("unavailable");
        }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("unavailable");
    }
    if metadata.len() > LIMIT {
        return Err("oversized");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options.open(path).map_err(|_| "unavailable")?;
    if !file.metadata().map_err(|_| "unavailable")?.is_file() {
        return Err("unavailable");
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unavailable")?;
    if bytes.len() as u64 > LIMIT {
        return Err("oversized");
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "unsupported")?;
    if value.is_object() {
        Ok(value)
    } else {
        Err("unsupported")
    }
}

pub fn read(home: &Path, window: &str, now: DateTime<Utc>) -> Value {
    let Some(state) = read_state(home) else {
        return json!({"health":"error","checked_at":now.to_rfc3339_opts(SecondsFormat::Secs,false),"source":"desktop_cache","window":window});
    };
    project(&state, window, now, BTreeMap::new())
}

pub fn background(
    home: &Path,
    metadata: &std::collections::HashMap<String, Value>,
    now: DateTime<Utc>,
) -> [Value; 4] {
    let windows = ["1h", "24h", "7d", "all"];
    let Some(state) = read_state(home) else {
        return windows.map(|window| json!({"window":window,"activity_items_scope":"latest_loaded_snapshot","activity":[]}));
    };
    let entries: BTreeMap<_,_> = metadata.iter().map(|(id,value)| {
        (id.clone(), json!({"host_id":value["host_id"],"timestamp":value["last_run_at"],"trigger":value["kind"],"environment":value["environment"]}))
    }).collect();
    windows.map(|window| {
        let mut dots = project(&state, window, now, entries.clone());
        if let Some(object) = dots.as_object_mut() {
            object.remove("health");
            object.remove("source");
            object.remove("checked_at");
            object.insert(
                "activity_items_scope".into(),
                json!("latest_loaded_snapshot"),
            );
        }
        dots
    })
}

fn project(
    state: &Value,
    window: &str,
    now: DateTime<Utc>,
    mut entries: BTreeMap<String, Value>,
) -> Value {
    let mut result = json!({"health":"error","checked_at":now.to_rfc3339_opts(SecondsFormat::Secs,false),"source":"desktop_cache","window":window});
    let empty = serde_json::Map::new();
    let atom = state
        .get("electron-persisted-atom-state")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut machines: BTreeMap<String, (Option<String>, usize)> = BTreeMap::new();
    for entry in entries.values() {
        if let Some(host_id) = entry["host_id"].as_str().filter(|id| host(id)) {
            newest(
                &mut machines.entry(host_id.to_owned()).or_default().0,
                &entry["timestamp"],
            );
        }
    }
    let mut activity = Vec::new();
    let outputs = atom.get("orbit-outputs-v1").and_then(Value::as_array);
    let snapshots = atom
        .get("orbit-activity-snapshots-v1")
        .and_then(Value::as_array);
    let mut events = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(outputs) = outputs {
        for output in outputs.iter().take(2000) {
            if let Some(entry) = output
                .get("threadId")
                .and_then(Value::as_str)
                .and_then(|id| entries.get_mut(id))
            {
                entry["trigger"] = json!("dot");
            }
            let Some(id) = output
                .get("threadId")
                .and_then(Value::as_str)
                .filter(|id| thread(id))
            else {
                continue;
            };
            let Some(artifact) = output
                .get("artifact")
                .filter(|artifact| artifact.is_object())
            else {
                continue;
            };
            let timestamp = date(
                artifact
                    .get("producedAtMs")
                    .and_then(Value::as_f64)
                    .map(|n| n / 1000.0),
            );
            let kind = artifact
                .get("type")
                .and_then(Value::as_str)
                .filter(|s| identifier(s));
            let turn = output.get("turnId").and_then(Value::as_str);
            let identity = serde_json::to_string(&(id, turn, &timestamp, kind)).unwrap_or_default();
            if !seen.insert(identity) {
                continue;
            }
            let host = output
                .get("hostId")
                .and_then(Value::as_str)
                .filter(|id| host(id));
            events.push(
                json!({"thread_id":id,"timestamp":timestamp,"artifact_type":kind,"host_id":host}),
            );
        }
    }
    for (key, rows) in atom {
        let Some(host_id) = key
            .strip_prefix("remote-thread-summaries-v3:")
            .filter(|id| host(id))
        else {
            continue;
        };
        let Some(rows) = rows.as_array() else {
            continue;
        };
        for row in rows.iter().take(1000) {
            let Some(id) = row
                .get("conversationId")
                .and_then(Value::as_str)
                .filter(|id| thread(id))
            else {
                continue;
            };
            let updated = date(row.get("updatedAt").and_then(Value::as_f64));
            if entries.get(id).is_some_and(|old| {
                old["timestamp"].as_str().unwrap_or("") > updated.as_str().unwrap_or("")
            }) {
                continue;
            }
            let trigger = row
                .get("threadSource")
                .and_then(Value::as_str)
                .filter(|s| {
                    matches!(
                        *s,
                        "user"
                            | "dot"
                            | "orbit"
                            | "automation"
                            | "heartbeat"
                            | "schedule"
                            | "subagent"
                    )
                })
                .unwrap_or("unknown");
            entries.insert(id.to_owned(),json!({"host_id":host_id,"timestamp":updated,"trigger":trigger,"environment":"remote"}));
            newest(&mut machines.entry(host_id.into()).or_default().0, &updated);
        }
    }
    if let Some(snapshots) = snapshots {
        for snapshot in snapshots.iter().take(200) {
            if let Some(rows) = snapshot.get("data").and_then(Value::as_array) {
                for row in rows.iter().take(1000) {
                    let Some(host_id) = row
                        .get("hostId")
                        .and_then(Value::as_str)
                        .filter(|id| host(id) || cloud(id))
                    else {
                        continue;
                    };
                    let Some(id) = row
                        .get("threadId")
                        .or_else(|| row.get("conversationId"))
                        .and_then(Value::as_str)
                        .filter(|id| thread(id))
                    else {
                        continue;
                    };
                    let when = row
                        .get("updatedAtMs")
                        .or_else(|| row.get("timestampMs"))
                        .and_then(Value::as_f64)
                        .map(|n| n / 1000.0)
                        .or_else(|| row.get("updatedAt").and_then(Value::as_f64));
                    let timestamp = date(when);
                    let environment = if cloud(host_id) {
                        "cloud"
                    } else if host_id == "local" {
                        "local"
                    } else {
                        "remote"
                    };
                    activity.push(json!({"thread_id":id,"host_id":host_id,"timestamp":timestamp,"activity_type":"dot","environment":environment}));
                    if host(host_id) {
                        newest(
                            &mut machines.entry(host_id.into()).or_default().0,
                            &timestamp,
                        );
                    }
                }
            }
        }
    }
    for event in &events {
        let id = event["thread_id"].as_str().unwrap_or("");
        let host_id = event["host_id"]
            .as_str()
            .or_else(|| entries.get(id).and_then(|entry| entry["host_id"].as_str()));
        if let Some(host_id) = host_id.filter(|id| host(id)) {
            activity.push(json!({"thread_id":id,"host_id":host_id,"timestamp":event["timestamp"],"activity_type":"output"}));
            newest(
                &mut machines.entry(host_id.into()).or_default().0,
                &event["timestamp"],
            );
        }
    }
    for (id, entry) in &entries {
        if let Some(host_id) = entry["host_id"].as_str().filter(|id| host(id)) {
            machines.entry(host_id.into()).or_default().1 += 1;
        }
        if matches!(entry["trigger"].as_str(), Some("dot" | "orbit")) {
            activity.push(json!({"thread_id":id,"host_id":entry["host_id"],"timestamp":entry["timestamp"],"activity_type":"dot","environment":entry["environment"]}));
        }
    }
    let mut computers: Vec<_> = machines.into_iter().map(|(host,(last,count))|json!({"host_id":host,"last_activity_at":last,"thread_count":count,"connection_status":"unknown"})).collect();
    computers.sort_by(|a, b| {
        b["last_activity_at"]
            .as_str()
            .unwrap_or("")
            .cmp(a["last_activity_at"].as_str().unwrap_or(""))
    });
    computers.truncate(100);
    activity.sort_by(|a, b| {
        b["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(a["timestamp"].as_str().unwrap_or(""))
    });
    activity.truncate(500);
    let cutoff = match window {
        "1h" => Some(now - Duration::hours(1)),
        "24h" => Some(now - Duration::hours(24)),
        "7d" => Some(now - Duration::hours(168)),
        _ => None,
    };
    let contains = |event: &Value| {
        cutoff.is_none_or(|boundary| {
            event["timestamp"]
                .as_str()
                .and_then(lam_history_store::parse_utc_stamp)
                .is_some_and(|time| time >= boundary)
        })
    };
    activity.retain(&contains);
    events.retain(&contains);
    events.sort_by(|a, b| {
        b["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(a["timestamp"].as_str().unwrap_or(""))
    });
    let total = events.len();
    events.truncate(500);
    result["health"] = json!("ok");
    result["computers"] = json!(computers);
    result["activity"] = json!(activity);
    result["events"] = if outputs.is_some() {
        json!(events)
    } else {
        Value::Null
    };
    if outputs.is_some() {
        result["total"] = json!(total);
    }
    result["activity_items"] = snapshots
        .map(|rows| {
            json!(rows
                .iter()
                .take(2000)
                .filter_map(|row| row.get("data").and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>())
        })
        .unwrap_or(Value::Null);
    result
}
