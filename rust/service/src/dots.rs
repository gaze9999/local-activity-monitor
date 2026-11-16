use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
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
    let Some(seconds) = seconds.filter(|n| {
        n.is_finite() && *n * 1000.0 >= i64::MIN as f64 && *n * 1000.0 < i64::MAX as f64
    }) else {
        return Value::Null;
    };
    let micros = (seconds.fract() * 1_000_000.0).round_ties_even() as i64;
    let Some(millis) = (seconds.trunc() as i64)
        .checked_mul(1000)
        .and_then(|whole| whole.checked_add(micros.div_euclid(1000)))
    else {
        return Value::Null;
    };
    DateTime::<Utc>::from_timestamp_millis(millis)
        .filter(|date| (1..=9999).contains(&date.year()))
        .map(|date| json!(date.to_rfc3339_opts(SecondsFormat::Millis, true)))
        .unwrap_or(Value::Null)
}
type Machine = (Option<String>, usize, usize);

fn machine<'a>(machines: &'a mut BTreeMap<String, Machine>, host: &str) -> &'a mut Machine {
    let order = machines.len();
    machines.entry(host.to_owned()).or_insert((None, 0, order))
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
    project(&state, window, now, Map::new())
}

pub fn background(
    home: &Path,
    metadata: &Map<String, Value>,
    now: DateTime<Utc>,
) -> [Value; 4] {
    let windows = ["1h", "24h", "7d", "all"];
    let Some(state) = read_state(home) else {
        return windows.map(|window| json!({"window":window,"activity_items_scope":"latest_loaded_snapshot","activity":[]}));
    };
    let entries: Map<_,_> = metadata.iter().map(|(id,value)| {
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
    mut entries: Map<String, Value>,
) -> Value {
    let mut result = json!({"health":"error","checked_at":now.to_rfc3339_opts(SecondsFormat::Secs,false),"source":"desktop_cache","window":window});
    if state
        .get("electron-persisted-atom-state")
        .is_some_and(|atom| !atom.is_object())
    {
        result["health"] = json!("ok");
        result["events"] = Value::Null;
        result["activity_items"] = Value::Null;
        result["activity"] = json!([]);
        return result;
    }
    let empty = serde_json::Map::new();
    let atom = state
        .get("electron-persisted-atom-state")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut entry_order: Vec<String> = entries.keys().cloned().collect();
    let mut machines: BTreeMap<String, Machine> = BTreeMap::new();
    for entry in entries.values() {
        if let Some(host_id) = entry["host_id"].as_str().filter(|id| host(id)) {
            newest(
                &mut machine(&mut machines, host_id).0,
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
            if !entries.contains_key(id) {
                entry_order.push(id.to_owned());
            }
            entries.insert(id.to_owned(),json!({"host_id":host_id,"timestamp":updated,"trigger":trigger,"environment":"remote"}));
            newest(&mut machine(&mut machines, host_id).0, &updated);
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
                            &mut machine(&mut machines, host_id).0,
                            &timestamp,
                        );
                    }
                }
            }
        }
    }
    events.sort_by(|a, b| {
        b["timestamp"]
            .as_str()
            .unwrap_or("")
            .cmp(a["timestamp"].as_str().unwrap_or(""))
    });
    for event in events.iter().take(500) {
        let id = event["thread_id"].as_str().unwrap_or("");
        let host_id = event["host_id"]
            .as_str()
            .or_else(|| entries.get(id).and_then(|entry| entry["host_id"].as_str()));
        if let Some(host_id) = host_id.filter(|id| host(id)) {
            activity.push(json!({"thread_id":id,"host_id":host_id,"timestamp":event["timestamp"],"activity_type":"output"}));
            newest(
                &mut machine(&mut machines, host_id).0,
                &event["timestamp"],
            );
        }
    }
    for id in &entry_order {
        let entry = &entries[id];
        if let Some(host_id) = entry["host_id"].as_str().filter(|id| host(id)) {
            machine(&mut machines, host_id).1 += 1;
        }
        if matches!(entry["trigger"].as_str(), Some("dot" | "orbit")) {
            activity.push(json!({"thread_id":id,"host_id":entry["host_id"],"timestamp":entry["timestamp"],"activity_type":"dot","environment":entry["environment"]}));
        }
    }
    let mut computers: Vec<_> = machines.into_iter().collect();
    computers.sort_by(|(_, a), (_, b)| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    computers.truncate(100);
    let computers: Vec<_> = computers.into_iter().map(|(host, (last, count, _))|
        json!({"host_id":host,"last_activity_at":last,"thread_count":count,"connection_status":"unknown"})
    ).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_field_precedence_preserves_invalid_and_missing_values() {
        let id = "00000000-0000-4000-8000-000000000001";
        let cases = [
            (json!({}), Some(json!("1970-01-01T00:00:02.000Z"))),
            (json!({"threadId":null}), None),
            (json!({"threadId":"bad"}), None),
            (
                json!({"updatedAtMs":"bad","timestampMs":1000}),
                Some(json!("1970-01-01T00:00:02.000Z")),
            ),
            (
                json!({"updatedAtMs":null,"timestampMs":1000}),
                Some(json!("1970-01-01T00:00:02.000Z")),
            ),
            (
                json!({"timestampMs":1000}),
                Some(json!("1970-01-01T00:00:01.000Z")),
            ),
            (
                json!({"updatedAtMs":0}),
                Some(json!("1970-01-01T00:00:00.000Z")),
            ),
            (json!({"updatedAtMs":1e300}), Some(Value::Null)),
            (
                json!({"updatedAtMs":true}),
                Some(json!("1970-01-01T00:00:02.000Z")),
            ),
            (json!({"hostId":"https://private.invalid"}), None),
        ];
        for (patch, expected) in cases {
            let mut row = json!({"conversationId":id,"hostId":"local","updatedAt":2,"private":"PRIVATE_MARKER"});
            row.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            let state = json!({"electron-persisted-atom-state":{"orbit-activity-snapshots-v1":[{"data":[row]}]}});
            let value = project(
                &state,
                "all",
                DateTime::from_timestamp(3, 0).unwrap(),
                Map::new(),
            );
            assert_eq!(value["activity_items"], 1);
            if let Some(timestamp) = expected {
                assert_eq!(
                    value["activity"],
                    json!([{"thread_id":id,"host_id":"local","timestamp":timestamp,"activity_type":"dot","environment":"local"}]),
                    "{patch}"
                );
                assert_eq!(
                    value["computers"],
                    json!([{"host_id":"local","last_activity_at":timestamp,"thread_count":0,"connection_status":"unknown"}]),
                    "{patch}"
                );
            } else {
                assert_eq!(value["activity"], json!([]), "{patch}");
                assert_eq!(value["computers"], json!([]), "{patch}");
            }
            assert!(!value.to_string().contains("PRIVATE_MARKER"));
        }
    }

    #[test]
    fn invalid_atom_does_not_claim_an_empty_computer_catalog() {
        let now = DateTime::from_timestamp(0, 0).unwrap();
        for atom in [
            Value::Null,
            json!(false),
            json!(0),
            json!("invalid"),
            json!([]),
        ] {
            let value = project(
                &json!({"electron-persisted-atom-state":atom}),
                "all",
                now,
                Map::from_iter([("thread".into(), json!({"host_id":"local","trigger":"dot"}))]),
            );
            assert_eq!(value["health"], "ok");
            assert_eq!(value["activity"], json!([]));
            assert!(value["events"].is_null());
            assert!(value["activity_items"].is_null());
            assert!(value.get("computers").is_none());
        }
        assert_eq!(
            project(&json!({}), "all", now, Map::new())["computers"],
            json!([])
        );
    }

    #[test]
    fn desktop_dates_round_microseconds_before_rendering_milliseconds() {
        for (seconds, expected) in [
            (0.9999996, "1970-01-01T00:00:01.000Z"),
            (-0.0000004, "1970-01-01T00:00:00.000Z"),
            (1.2349996, "1970-01-01T00:00:01.235Z"),
            (0.0009994, "1970-01-01T00:00:00.000Z"),
            (0.0009995, "1970-01-01T00:00:00.001Z"),
            (-0.0009996, "1969-12-31T23:59:59.999Z"),
        ] {
            assert_eq!(date(Some(seconds)), expected, "seconds={seconds}");
        }
        for seconds in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
        ] {
            assert!(date(Some(seconds)).is_null());
        }
        assert!(date(None).is_null());
    }

    #[test]
    fn desktop_dates_match_python_calendar_bounds() {
        assert_eq!(date(Some(-62135596800.0)), "0001-01-01T00:00:00.000Z");
        assert_eq!(date(Some(253402300799.0)), "9999-12-31T23:59:59.000Z");
        for seconds in [-62135596801.0, 253402300800.0] {
            assert!(date(Some(seconds)).is_null());
        }
    }

    #[test]
    fn remote_summary_activity_keeps_first_thread_encounter_order() {
        let first = "ffffffff-ffff-4fff-8fff-ffffffffffff";
        let second = "00000000-0000-4000-8000-000000000001";
        let state = json!({"electron-persisted-atom-state":{
            "remote-thread-summaries-v3:local":[
                {"conversationId":first,"updatedAt":100,"threadSource":"dot"},
                {"conversationId":second,"updatedAt":100,"threadSource":"dot"},
                {"conversationId":first,"updatedAt":100,"threadSource":"orbit"}
            ]
        }});
        let value = project(
            &state,
            "all",
            DateTime::from_timestamp(101, 0).unwrap(),
            Map::new(),
        );
        let activity = value["activity"].as_array().unwrap();
        assert_eq!(activity.len(), 2);
        assert_eq!(activity[0]["thread_id"], first);
        assert_eq!(activity[1]["thread_id"], second);
        assert_eq!(value["computers"][0]["thread_count"], 2);
    }

    #[test]
    fn computer_preview_keeps_first_seen_hosts_at_undated_limit() {
        let rows: Vec<_> = (0..101).rev().map(|index| json!({
            "threadId":"00000000-0000-4000-8000-000000000001",
            "hostId":format!("remote-control:host-{index:03}")
        })).collect();
        let state = json!({"electron-persisted-atom-state":{"orbit-activity-snapshots-v1":[{"data":rows}]}});
        let value = project(&state, "all", DateTime::from_timestamp(2, 0).unwrap(), Map::new());
        let computers = value["computers"].as_array().unwrap();
        assert_eq!(computers.len(), 100);
        assert_eq!(computers[0]["host_id"], "remote-control:host-100");
        assert_eq!(computers[99]["host_id"], "remote-control:host-001");
        assert!(computers.iter().all(|row| row["last_activity_at"].is_null()));
    }

    #[test]
    fn output_preview_keeps_equal_time_insertion_order_and_all_event_totals() {
        let outputs: Vec<_> = (0..501).map(|index| json!({
            "threadId":"00000000-0000-4000-8000-000000000001", "turnId":index.to_string(),
            "hostId":if index == 500 { "remote-control:excluded" } else { "remote-control:shown" },
            "artifact":{"type":"text","producedAtMs":1000}
        })).collect();
        let state = json!({"electron-persisted-atom-state":{"orbit-outputs-v1":outputs}});
        let value = project(&state, "all", DateTime::from_timestamp(2, 0).unwrap(), Map::new());
        assert_eq!(value["total"], 501);
        assert_eq!(value["events"].as_array().unwrap().len(), 500);
        assert_eq!(value["activity"].as_array().unwrap().len(), 500);
        assert_eq!(value["computers"].as_array().unwrap().len(), 1);
        assert_eq!(value["computers"][0]["host_id"], "remote-control:shown");
    }
}
