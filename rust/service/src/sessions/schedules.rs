use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Map, Value};

const FIELDS: &[&str] = &[
    "id",
    "name",
    "status",
    "kind",
    "next_run_at",
    "last_run_at",
    "rrule",
    "model",
    "reasoning_effort",
    "project_id",
    "target_thread_id",
    "execution_environment",
    "created_at",
    "updated_at",
];
const SCHEDULE_LIMIT: usize = 500;
const RUN_LIMIT: usize = 1000;

pub(super) fn columns(db: &Connection, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let result = statement.query_map([], |row| row.get(1))?.collect();
    result
}

pub(super) fn string(value: ValueRef<'_>) -> Option<&str> {
    match value {
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok(),
        _ => None,
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

pub(super) fn seconds_timestamp(seconds: f64) -> Value {
    let millis = (seconds * 1000.0).floor();
    if !millis.is_finite() || millis < i64::MIN as f64 || millis >= i64::MAX as f64 {
        return Value::Null;
    }
    DateTime::<Utc>::from_timestamp_millis(millis as i64)
        .map(|date| Value::String(date.to_rfc3339_opts(SecondsFormat::Millis, true)))
        .unwrap_or(Value::Null)
}

fn timestamp(value: ValueRef<'_>) -> Value {
    let number = match value {
        ValueRef::Integer(n) => n as f64,
        ValueRef::Real(n) => n,
        _ => return Value::Null,
    };
    seconds_timestamp(if number > 100_000_000_000.0 {
        number / 1000.0
    } else {
        number
    })
}

fn catalog_entries(home: &Path) -> (HashMap<String, Value>, Value) {
    let mut entries = HashMap::new();
    let path = home.join("sqlite/codex-dev.db");
    let checked = DateTime::<Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(SecondsFormat::Secs, false);
    let mut report = json!({"name":"codex-dev.db","location":path.to_string_lossy(),"health":if path.is_file(){"unavailable"}else{"missing"},"checked_at":checked,"fields":[]});
    let Ok(db) = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return (entries, report);
    };
    if db.busy_timeout(Duration::from_millis(80)).is_err()
        || db.pragma_update(None, "query_only", true).is_err()
    {
        return (entries, report);
    }
    let Ok(available) = columns(&db, "local_thread_catalog") else {
        return (entries, report);
    };
    let required = [
        "host_id",
        "thread_id",
        "display_title",
        "source_created_at",
        "source_updated_at",
        "source_kind",
        "thread_source",
        "project_id",
    ];
    if !required.iter().all(|key| available.contains(*key)) {
        report["health"] = json!("unsupported");
        report["queries"] = json!([
            {"name":"automation_runs","health":"ok","fields":["automation_runs.thread_id","automations.kind"],"rows_read":0,"selected_threads":0,"row_limit":0},
            {"name":"automations","health":"ok","fields":["target_thread_id","status"],"rows_read":0,"selected_threads":0,"row_limit":0}
        ]);
        return (entries, report);
    }
    let mut wanted = required.to_vec();
    wanted.extend(
        [
            "model",
            "reasoning_effort",
            "model_provider",
            "chatgpt_async_status",
            "archived",
        ]
        .into_iter()
        .filter(|key| available.contains(*key)),
    );
    let mut rows_read = 0usize;
    let status = if available.contains("chatgpt_async_status") {
        "chatgpt_async_status"
    } else {
        "NULL"
    };
    let optional = |name| {
        if available.contains(name) {
            name
        } else {
            "NULL"
        }
    };
    let sql = format!("SELECT host_id,thread_id,display_title,source_updated_at,thread_source,{status},source_created_at,source_kind,project_id,{},{},{},{} FROM local_thread_catalog ORDER BY source_updated_at DESC LIMIT 2000", optional("model"),optional("reasoning_effort"),optional("model_provider"),optional("archived"));
    {
        let Ok(mut statement) = db.prepare(&sql) else {
            return (entries, report);
        };
        let Ok(mut rows) = statement.query([]) else {
            return (entries, report);
        };
        loop {
            let row = match rows.next() {
                Ok(Some(row)) => row,
                Ok(None) => break,
                Err(_) => return (entries, report),
            };
            rows_read += 1;
            let Ok(id_value) = row.get_ref(1) else {
                continue;
            };
            let Some(id) = string(id_value).filter(|id| id.chars().count() <= 160) else {
                continue;
            };
            let host = row.get_ref(0).ok().and_then(string);
            let environment = match host {
                Some("local") => "local",
                Some("durable") => "cloud",
                Some(host) if host.starts_with("chatgpt:") => "cloud",
                Some(host) if host.starts_with("remote-control:") => "remote",
                _ => "unknown",
            };
            let updated = match row.get_ref(3) {
                Ok(ValueRef::Integer(n)) => seconds_timestamp(n as f64),
                Ok(ValueRef::Real(n)) => seconds_timestamp(n),
                _ => Value::Null,
            };
            if entries.get(id).is_some_and(|previous: &Value| {
                previous["last_run_at"].as_str().unwrap_or("") >= updated.as_str().unwrap_or("")
            }) {
                continue;
            }
            let title = row
                .get_ref(2)
                .ok()
                .and_then(string)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(512).collect::<String>());
            let kind = row
                .get_ref(4)
                .ok()
                .and_then(string)
                .filter(|s| {
                    matches!(
                        *s,
                        "dot"
                            | "orbit"
                            | "automation"
                            | "schedule"
                            | "heartbeat"
                            | "subagent"
                            | "guardian_review"
                            | "user"
                    )
                })
                .unwrap_or("unknown");
            let state = row.get_ref(5).ok().and_then(string).filter(|s| {
                !s.is_empty()
                    && s.len() <= 40
                    && s.as_bytes()[0].is_ascii_lowercase()
                    && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            });
            let created = match row.get_ref(6) {
                Ok(ValueRef::Integer(n)) => seconds_timestamp(n as f64),
                Ok(ValueRef::Real(n)) => seconds_timestamp(n),
                _ => Value::Null,
            };
            let activity_type = match row.get_ref(7).ok().and_then(string) {
                Some("chatgpt") => "chat",
                Some("vscode" | "cli" | "exec") => "codex",
                _ => "unknown",
            };
            let project_id = row
                .get_ref(8)
                .ok()
                .and_then(string)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(160).collect::<String>());
            let model = row
                .get_ref(9)
                .ok()
                .and_then(string)
                .filter(|s| identifier(s));
            let effort = row
                .get_ref(10)
                .ok()
                .and_then(string)
                .filter(|s| identifier(s));
            let provider = row
                .get_ref(11)
                .ok()
                .and_then(string)
                .filter(|s| identifier(s));
            let archived = match row.get_ref(12) {
                Ok(ValueRef::Integer(0)) => Some(false),
                Ok(ValueRef::Integer(1)) => Some(true),
                _ => None,
            };
            let execution = provider
                .map(|provider| json!({"model_provider":provider}))
                .unwrap_or_else(|| json!({}));
            entries.insert(id.to_owned(), json!({"thread_id":id,"name":title,"host_id":host,"environment":environment,"kind":kind,"last_run_at":updated,"status":state,"created_at":created,"activity_type":activity_type,"project_id":project_id,"project_scope":if project_id.is_some(){"project"}else{"none"},"model":model,"reasoning_effort":effort,"archived":archived,"execution":execution,"has_schedule":false}));
        }
    }
    let mut ids: Vec<_> = entries.keys().cloned().collect();
    ids.sort();
    let mut queries = [
        json!({"name":"automation_runs","health":"ok","fields":["automation_runs.thread_id","automations.kind"],"rows_read":0,"selected_threads":ids.len(),"row_limit":ids.len()}),
        json!({"name":"automations","health":"ok","fields":["target_thread_id","status"],"rows_read":0,"selected_threads":ids.len(),"row_limit":ids.len()}),
    ];
    for batch in ids.chunks(400) {
        let placeholders = vec!["?"; batch.len()].join(",");
        let sql = format!("SELECT r.thread_id,MAX(CASE WHEN a.kind='heartbeat' THEN 1 ELSE 0 END) FROM automation_runs r JOIN automations a ON a.id=r.automation_id WHERE r.thread_id IN ({placeholders}) GROUP BY r.thread_id");
        if let Ok(mut statement) = db.prepare(&sql) {
            if let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter())) {
                loop {
                    let row = match rows.next() {
                        Ok(Some(row)) => row,
                        Ok(None) => break,
                        Err(_) => {
                            queries[0]["health"] = json!("unsupported");
                            break;
                        }
                    };
                    queries[0]["rows_read"] =
                        json!(queries[0]["rows_read"].as_u64().unwrap_or(0) + 1);
                    let Ok(id) = row.get::<_, String>(0) else {
                        continue;
                    };
                    if let Some(entry) = entries.get_mut(&id) {
                        entry["kind"] = json!(if row.get::<_, i64>(1).unwrap_or(0) != 0 {
                            "heartbeat"
                        } else {
                            "schedule"
                        });
                    }
                }
            } else {
                queries[0]["health"] = json!("unsupported");
            }
        } else {
            queries[0]["health"] = json!("unsupported");
        }
        let sql = format!("SELECT target_thread_id FROM automations WHERE status='ACTIVE' AND target_thread_id IN ({placeholders}) GROUP BY target_thread_id");
        if let Ok(mut statement) = db.prepare(&sql) {
            if let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter())) {
                loop {
                    let row = match rows.next() {
                        Ok(Some(row)) => row,
                        Ok(None) => break,
                        Err(_) => {
                            queries[1]["health"] = json!("unsupported");
                            break;
                        }
                    };
                    queries[1]["rows_read"] =
                        json!(queries[1]["rows_read"].as_u64().unwrap_or(0) + 1);
                    if let Ok(id) = row.get::<_, String>(0) {
                        if let Some(entry) = entries.get_mut(&id) {
                            entry["has_schedule"] = json!(true);
                        }
                    }
                }
            } else {
                queries[1]["health"] = json!("unsupported");
            }
        } else {
            queries[1]["health"] = json!("unsupported");
        }
    }
    report["health"] = json!("ok");
    report["fields"] = json!(wanted);
    report["rows_read"] = json!(rows_read);
    report["row_limit"] = json!(2000);
    report["queries"] = json!(queries);
    (entries, report)
}

pub(super) fn state_filename(name: &str) -> bool {
    let Some(name) = name.strip_prefix("state_") else {
        return false;
    };
    let name = name.strip_suffix("-wal").unwrap_or(name);
    name.strip_suffix(".sqlite")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn state_files(home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (state_filename(name) && !name.ends_with("-wal")).then(|| entry.path())
        })
        .collect();
    paths.sort_by(|a, b| {
        let number = |path: &Path| {
            path.file_name()
                .and_then(|v| v.to_str())
                .and_then(|s| s.strip_prefix("state_"))
                .and_then(|s| s.strip_suffix(".sqlite"))
                .unwrap_or("")
                .trim_start_matches('0')
                .to_owned()
        };
        let left = number(a);
        let right = number(b);
        right
            .len()
            .cmp(&left.len())
            .then_with(|| right.cmp(&left))
            .then_with(|| b.cmp(a))
    });
    paths
}

fn merge_state_database(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    observed: &BTreeSet<String>,
    include_workdirs: bool,
    select_recent: bool,
) -> Vec<Value> {
    let mut reports = Vec::new();
    let mut selected: BTreeSet<_> = entries
        .keys()
        .cloned()
        .chain(observed.iter().cloned())
        .collect();
    for path in state_files(home) {
        let checked = DateTime::<Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(SecondsFormat::Secs, false);
        let mut report = json!({"name":path.file_name().unwrap_or_default().to_string_lossy(),"location":path.to_string_lossy(),"health":if path.is_file(){"unavailable"}else{"missing"},"checked_at":checked,"fields":[]});
        let Ok(db) = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            reports.push(report);
            continue;
        };
        if db.busy_timeout(Duration::from_millis(80)).is_err()
            || db.pragma_update(None, "query_only", true).is_err()
        {
            reports.push(report);
            continue;
        }
        let Ok(fields) = columns(&db, "threads") else {
            reports.push(report);
            continue;
        };
        if !fields.contains("id") || !fields.contains("title") {
            report["health"] = json!("unsupported");
            reports.push(report);
            continue;
        }
        if select_recent {
            let order = ["updated_at_ms", "updated_at", "created_at_ms", "created_at"]
                .into_iter()
                .find(|field| fields.contains(*field))
                .unwrap_or("id");
            let remaining = 2000usize.saturating_sub(selected.len());
            if remaining > 0 {
                let query =
                    format!("SELECT id FROM threads ORDER BY {order} DESC LIMIT {remaining}");
                let result = db.prepare(&query).and_then(|mut statement| {
                    statement
                        .query_map([], |row| {
                            Ok(row.get_ref(0).ok().and_then(string).map(str::to_owned))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()
                });
                match result {
                    Ok(ids) => selected.extend(ids.into_iter().flatten().filter(|id| {
                        id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                    })),
                    Err(_) => {
                        reports.push(report);
                        continue;
                    }
                }
            }
        }
        let mut edges = std::collections::BTreeMap::new();
        let mut failed = false;
        let has_edges = columns(&db, "thread_spawn_edges").is_ok_and(|fields| {
            fields.contains("parent_thread_id") && fields.contains("child_thread_id")
        });
        if has_edges {
            let mut pending = selected.clone();
            let mut visited = BTreeSet::new();
            let edge_limit = if select_recent {
                500.min(2000usize.saturating_sub(selected.len()))
            } else {
                500
            };
            while !pending.is_empty() && edges.len() < edge_limit {
                let batch: Vec<_> = pending.difference(&visited).take(400).cloned().collect();
                if batch.is_empty() {
                    break;
                }
                for id in &batch {
                    pending.remove(id);
                    visited.insert(id.clone());
                }
                let placeholders = vec!["?"; batch.len()].join(",");
                let sql = format!("SELECT parent_thread_id,child_thread_id FROM thread_spawn_edges WHERE parent_thread_id IN ({placeholders}) ORDER BY child_thread_id LIMIT {}", edge_limit - edges.len());
                let Ok(mut statement) = db.prepare(&sql) else {
                    failed = true;
                    break;
                };
                let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter())) else {
                    failed = true;
                    break;
                };
                loop {
                    let row = match rows.next() {
                        Ok(Some(row)) => row,
                        Ok(None) => break,
                        Err(_) => {
                            failed = true;
                            break;
                        }
                    };
                    let (Ok(parent), Ok(child)) =
                        (row.get::<_, String>(0), row.get::<_, String>(1))
                    else {
                        continue;
                    };
                    let valid = |id: &str| {
                        id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                    };
                    if !valid(&parent) || !valid(&child) {
                        continue;
                    }
                    edges.insert(child.clone(), parent);
                    if !visited.contains(&child) {
                        pending.insert(child);
                    }
                }
            }
            selected.extend(edges.keys().cloned());
        }
        if failed {
            reports.push(report);
            continue;
        }
        let source = if fields.contains("thread_source") {
            "thread_source"
        } else {
            "NULL"
        };
        let updated = if fields.contains("updated_at_ms") {
            "updated_at_ms"
        } else {
            "NULL"
        };
        let seconds = if fields.contains("updated_at") {
            "updated_at"
        } else {
            "NULL"
        };
        let extra_fields = [
            "model",
            "reasoning_effort",
            "originator",
            "model_provider",
            "cli_version",
            "approval_mode",
            "sandbox_policy",
            "git_branch",
            "git_sha",
            "agent_nickname",
            "agent_role",
            "agent_path",
            "source",
            "history_mode",
            "archived",
            "created_at",
            "created_at_ms",
            "cwd",
        ];
        let extra = extra_fields
            .iter()
            .map(|field| {
                if fields.contains(*field) && (*field != "cwd" || include_workdirs) {
                    *field
                } else {
                    "NULL"
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut rows_read = 0usize;
        let selected_rows: Vec<_> = selected.iter().cloned().collect();
        for batch in selected_rows.chunks(400) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let sql = format!("SELECT id,title,{source},{updated},{seconds},{extra} FROM threads WHERE id IN ({placeholders})");
            let Ok(mut statement) = db.prepare(&sql) else {
                failed = true;
                break;
            };
            let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter())) else {
                failed = true;
                break;
            };
            loop {
                let row = match rows.next() {
                    Ok(Some(row)) => row,
                    Ok(None) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                rows_read += 1;
                let Ok(id) = row.get::<_, String>(0) else {
                    continue;
                };
                let entry = entries.entry(id.clone()).or_insert_with(|| json!({"thread_id":id,"name":null,"host_id":null,"environment":"unknown","kind":"unknown","last_run_at":null,"status":null}));
                let mut values = serde_json::Map::new();
                for (offset, key) in extra_fields.iter().enumerate() {
                    match row.get_ref(offset + 5) {
                        Ok(ValueRef::Text(bytes)) => {
                            if let Ok(value) = std::str::from_utf8(bytes) {
                                if *key == "model" {
                                    if let Some(model) =
                                        value.trim().chars().next().map(|_| {
                                            value.trim().chars().take(160).collect::<String>()
                                        })
                                    {
                                        values.insert((*key).into(), json!(model));
                                    }
                                } else if (*key == "cwd" && value.chars().count() <= 4096)
                                    || (*key != "cwd" && value.len() <= 4096)
                                {
                                    values.insert((*key).into(), json!(value));
                                }
                            }
                        }
                        Ok(ValueRef::Integer(value)) => {
                            values.insert((*key).into(), json!(value));
                        }
                        Ok(ValueRef::Real(value)) if value.is_finite() => {
                            values.insert((*key).into(), json!(value));
                        }
                        _ => {}
                    }
                }
                let values = Value::Object(values);
                if let Some(cwd) = values["cwd"].as_str().filter(|value| {
                    !value.is_empty()
                        && value.chars().count() <= 4096
                        && !value.chars().any(char::is_control)
                }) {
                    entry["_cwd"] = json!(cwd);
                }
                if let Some(model) = values["model"].as_str().filter(|model| !model.is_empty()) {
                    entry["model"] = json!(model);
                }
                if let Some(effort) = values["reasoning_effort"]
                    .as_str()
                    .filter(|value| value.len() <= 80 && identifier(value))
                {
                    entry["reasoning_effort"] = json!(effort);
                }
                if let Some(archived) = values["archived"]
                    .as_i64()
                    .filter(|value| *value == 0 || *value == 1)
                {
                    entry["archived"] = json!(archived == 1);
                }
                entry["execution"] = super::metadata::execution(&values);
                if let Some(kind) = values["originator"]
                    .as_str()
                    .and_then(|origin| match origin {
                        "codex_work_desktop" => Some("work"),
                        "Codex Desktop" => Some("codex"),
                        _ => None,
                    })
                {
                    entry["activity_type"] = json!(kind);
                }
                if entry["created_at"].is_null() {
                    let millis = values["created_at_ms"].as_f64();
                    let seconds = values["created_at"].as_f64();
                    entry["created_at"] = millis
                        .map(|n| seconds_timestamp(n / 1000.0))
                        .or_else(|| seconds.map(seconds_timestamp))
                        .unwrap_or(Value::Null);
                }
                if entry["name"].is_null() {
                    if let Some(name) = row
                        .get_ref(1)
                        .ok()
                        .and_then(string)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        entry["name"] = json!(name.chars().take(512).collect::<String>());
                    }
                }
                if let Some(parent) = edges.get(&id) {
                    entry["kind"] = json!("subagent");
                    if !entry["execution"].is_object() {
                        entry["execution"] = json!({});
                    }
                    entry["execution"]["parent_thread_id"] = json!(parent);
                }
                if let Some(source) = row
                    .get_ref(2)
                    .ok()
                    .and_then(string)
                    .filter(|s| matches!(*s, "user" | "subagent" | "guardian_review"))
                {
                    entry["kind"] = json!(source);
                }
                if entry["last_run_at"].is_null() {
                    let millis = row.get_ref(3).ok().and_then(|v| match v {
                        ValueRef::Integer(n) => Some(n as f64),
                        ValueRef::Real(n) => Some(n),
                        _ => None,
                    });
                    let fallback = row.get_ref(4).ok().and_then(|v| match v {
                        ValueRef::Integer(n) => Some(n as f64),
                        ValueRef::Real(n) => Some(n),
                        _ => None,
                    });
                    entry["last_run_at"] = millis
                        .map(|n| seconds_timestamp(n / 1000.0))
                        .or_else(|| fallback.map(seconds_timestamp))
                        .unwrap_or(Value::Null);
                }
            }
            if failed {
                break;
            }
        }
        if !failed {
            let mut wanted: Vec<_> = [
                "id",
                "title",
                "model",
                "reasoning_effort",
                "originator",
                "thread_source",
                "model_provider",
                "cli_version",
                "approval_mode",
                "sandbox_policy",
                "git_branch",
                "git_sha",
                "agent_nickname",
                "agent_role",
                "agent_path",
                "source",
                "history_mode",
                "archived",
                "cwd",
                "created_at",
                "updated_at",
                "created_at_ms",
                "updated_at_ms",
            ]
            .into_iter()
            .filter(|field| fields.contains(*field) && (*field != "cwd" || include_workdirs))
            .collect();
            if has_edges {
                wanted.extend([
                    "thread_spawn_edges.parent_thread_id",
                    "thread_spawn_edges.child_thread_id",
                ]);
            }
            report["health"] = json!("ok");
            report["fields"] = json!(wanted);
            report["rows_read"] = json!(rows_read);
            report["selected_threads"] = json!(selected.len());
            report["subagent_rows"] = json!(edges.len());
            report["subagent_limit"] = json!(500);
            reports.push(report);
            break;
        }
        reports.push(report);
    }
    reports
}

pub(crate) fn merge_remote_state(home: &Path, entries: &mut HashMap<String, Value>) {
    let Some(state) = crate::dots::read_state(home) else {
        return;
    };
    let Some(atoms) = state
        .get("electron-persisted-atom-state")
        .and_then(Value::as_object)
    else {
        return;
    };
    if let Some(outputs) = atoms.get("orbit-outputs-v1").and_then(Value::as_array) {
        for output in outputs.iter().take(2000) {
            if let Some(entry) = output
                .get("threadId")
                .and_then(Value::as_str)
                .and_then(|id| entries.get_mut(id))
            {
                entry["kind"] = json!("dot");
            }
        }
    }
    for (key, rows) in atoms {
        let Some(host) = key.strip_prefix("remote-thread-summaries-v3:") else {
            continue;
        };
        let valid_host = host == "local"
            || host.strip_prefix("remote-control:").is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix.len() <= 160
                    && suffix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
        let Some(rows) = rows.as_array().filter(|_| valid_host) else {
            continue;
        };
        for row in rows.iter().take(1000) {
            let Some(id) = row
                .get("conversationId")
                .and_then(Value::as_str)
                .filter(|id| {
                    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                })
            else {
                continue;
            };
            let updated = row
                .get("updatedAt")
                .and_then(Value::as_f64)
                .map(seconds_timestamp)
                .unwrap_or(Value::Null);
            if entries.get(id).is_some_and(|previous| {
                previous["last_run_at"].as_str().unwrap_or("") > updated.as_str().unwrap_or("")
            }) {
                continue;
            }
            let kind = row
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
            let status = row
                .get("threadRuntimeStatus")
                .and_then(|status| status.get("type"))
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "running" | "completed" | "idle" | "error"));
            let name = row
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(|title| title.chars().take(512).collect::<String>());
            let entry = entries
                .entry(id.to_owned())
                .or_insert_with(|| json!({"thread_id":id}));
            entry["name"] = json!(name);
            entry["host_id"] = json!(host);
            entry["environment"] = json!("remote");
            entry["kind"] = json!(kind);
            entry["last_run_at"] = updated;
            entry["created_at"] = row
                .get("createdAt")
                .and_then(Value::as_f64)
                .map(seconds_timestamp)
                .unwrap_or(Value::Null);
            entry["activity_type"] = json!("codex");
            entry["project_scope"] = json!("unknown");
            if let Some(status) = status {
                entry["status"] = json!(status);
            }
            if let Some(parent) = row
                .get("parentThreadId")
                .and_then(Value::as_str)
                .filter(|id| {
                    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                })
            {
                entry["execution"] = json!({"parent_thread_id":parent});
            }
        }
    }
}

fn merge_session_index(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    observed: &BTreeSet<String>,
) -> Value {
    const LIMIT: u64 = 1024 * 1024;
    let path = home.join("session_index.jsonl");
    let checked = DateTime::<Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(SecondsFormat::Secs, false);
    let report = |health: &str, matched: Option<usize>| {
        let mut value = json!({"name":"session_index.jsonl","location":path.to_string_lossy(),"health":health,"checked_at":checked,"fields":if health=="ok" {vec!["id","thread_name"]} else {Vec::<&str>::new()}});
        if let Some(count) = matched {
            value["tail_bytes"] = json!(LIMIT);
            value["matched_threads"] = json!(count);
        }
        value
    };
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !super::redirected(&metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return report("missing", None)
        }
        _ => return report("unavailable", None),
    };
    let Ok(mut file) = super::open_source(home, &path) else {
        return report("unavailable", None);
    };
    if !file.metadata().is_ok_and(|current| {
        current.is_file() && super::file_identity(&current) == super::file_identity(&metadata)
    }) {
        return report("unavailable", None);
    }
    let Ok(size) = file.seek(SeekFrom::End(0)) else {
        return report("unavailable", None);
    };
    let offset = size.saturating_sub(LIMIT);
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return report("unavailable", None);
    }
    let mut bytes = Vec::new();
    if file.take(LIMIT).read_to_end(&mut bytes).is_err() {
        return report("unavailable", None);
    }
    let mut lines = bytes.split(|byte| *byte == b'\n');
    if offset > 0 {
        lines.next();
    }
    let mut titles = HashMap::new();
    for line in lines {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let Some(id) = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| observed.contains(*id))
        else {
            continue;
        };
        let Some(title) = value
            .get("thread_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            continue;
        };
        titles.insert(id.to_owned(), title.chars().take(512).collect::<String>());
    }
    let matched = titles.len();
    for (id, title) in titles {
        let entry=entries.entry(id.clone()).or_insert_with(||json!({"thread_id":id,"name":null,"host_id":null,"environment":"unknown","kind":"unknown","last_run_at":null,"status":null}));
        if entry["name"].is_null() {
            entry["name"] = json!(title);
        }
    }
    report("ok", Some(matched))
}

pub(crate) fn background_metadata(
    home: &Path,
    observed: &BTreeSet<String>,
    include_workdirs: bool,
    select_recent: bool,
) -> (HashMap<String, Value>, Vec<Value>) {
    let (mut entries, catalog_report) = catalog_entries(home);
    let state_reports = merge_state_database(
        home,
        &mut entries,
        observed,
        include_workdirs,
        select_recent,
    );
    let report = merge_session_index(home, &mut entries, observed);
    let mut reports = vec![catalog_report];
    reports.extend(state_reports);
    reports.push(report);
    (entries, reports)
}

fn project_remote_activities(entries: &HashMap<String, Value>) -> Vec<Value> {
    let mut result: Vec<_> = entries
        .values()
        .filter(|item| {
            matches!(item["environment"].as_str(), Some("remote" | "cloud"))
                && matches!(
                    item["kind"].as_str(),
                    Some("automation" | "schedule" | "heartbeat")
                )
        })
        .map(|item| json!({"thread_id":item["thread_id"],"name":item["name"],"host_id":item["host_id"],"environment":item["environment"],"kind":item["kind"],"last_run_at":item["last_run_at"],"status":item["status"]}))
        .collect();
    result.sort_by(|a, b| {
        b["last_run_at"]
            .as_str()
            .unwrap_or("")
            .cmp(a["last_run_at"].as_str().unwrap_or(""))
    });
    result.truncate(SCHEDULE_LIMIT);
    result
}

#[cfg(test)]
fn remote_activities(home: &Path, observed: &BTreeSet<String>) -> Vec<Value> {
    let (mut entries, _) = background_metadata(home, observed, true, false);
    merge_remote_state(home, &mut entries);
    project_remote_activities(&entries)
}

fn read_database(
    path: &Path,
    items: &mut Vec<Value>,
    fields: &mut Vec<String>,
    rows_read: &mut usize,
    runs_read: &mut usize,
) -> rusqlite::Result<bool> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_millis(80))?;
    db.pragma_update(None, "query_only", true)?;
    let available = columns(&db, "automations")?;
    if !available.contains("id") || !available.contains("status") {
        return Ok(false);
    }
    let wanted: Vec<_> = FIELDS
        .iter()
        .copied()
        .filter(|field| available.contains(*field))
        .collect();
    fields.extend(wanted.iter().map(|field| format!("automations.{field}")));
    let order = if available.contains("next_run_at") {
        "next_run_at IS NULL,next_run_at"
    } else {
        "id"
    };
    let mut statement = db.prepare(&format!(
        "SELECT {} FROM automations ORDER BY {order} LIMIT {SCHEDULE_LIMIT}",
        wanted.join(",")
    ))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        *rows_read += 1;
        let id = row.get_ref(wanted.iter().position(|field| *field == "id").expect("id"))?;
        let Some(id) = string(id).filter(|id| identifier(id)) else {
            continue;
        };
        let mut item = Map::new();
        item.insert("id".into(), json!(id));
        item.insert("name".into(), Value::Null);
        for key in ["next_run_at", "last_run_at", "created_at", "updated_at"] {
            item.insert(key.into(), Value::Null);
        }
        for (index, field) in wanted.iter().enumerate() {
            let value = row.get_ref(index)?;
            match *field {
                "name" => {
                    item.insert(
                        "name".into(),
                        string(value)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(|s| Value::String(s.chars().take(256).collect()))
                            .unwrap_or(Value::Null),
                    );
                }
                "next_run_at" | "last_run_at" | "created_at" | "updated_at" => {
                    item.insert((*field).into(), timestamp(value));
                }
                "rrule" => {
                    if let Some(s) = string(value).filter(|s| {
                        s.chars().count() <= 2048
                            && !s.is_empty()
                            && s.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || b"=;,:+_./-\r\n ".contains(&b)
                            })
                    }) {
                        item.insert("rrule".into(), json!(s));
                    }
                }
                "id" => {}
                _ => {
                    if let Some(s) = string(value).filter(|s| identifier(s)) {
                        item.insert((*field).into(), json!(s));
                    }
                }
            }
        }
        items.push(Value::Object(item));
    }
    let available = columns(&db, "automation_runs")?;
    if ["automation_id", "thread_id", "created_at"]
        .iter()
        .all(|field| available.contains(*field))
    {
        let wanted: Vec<_> = ["automation_id", "thread_id", "created_at", "status"]
            .into_iter()
            .filter(|field| available.contains(*field))
            .collect();
        fields.extend(
            wanted
                .iter()
                .map(|field| format!("automation_runs.{field}")),
        );
        let lookup: HashMap<_, _> = items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| Some((item["id"].as_str()?.to_owned(), i)))
            .collect();
        let mut statement = db.prepare(&format!(
            "SELECT {} FROM automation_runs ORDER BY created_at DESC LIMIT {RUN_LIMIT}",
            wanted.join(",")
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            *runs_read += 1;
            let Some(index) = string(row.get_ref(0)?)
                .and_then(|id| lookup.get(id))
                .copied()
            else {
                continue;
            };
            let Some(thread) = string(row.get_ref(1)?).filter(|s| {
                !s.is_empty()
                    && s.len() <= 160
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }) else {
                continue;
            };
            let item = items[index].as_object_mut().expect("schedule object");
            if item.contains_key("last_thread_id") {
                continue;
            }
            item.insert("last_thread_id".into(), json!(thread));
            if item["last_run_at"].is_null() {
                item.insert("last_run_at".into(), timestamp(row.get_ref(2)?));
            }
            if wanted.len() == 4 {
                if let Some(status) = string(row.get_ref(3)?).filter(|s| {
                    !s.is_empty()
                        && s.len() <= 40
                        && s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_')
                }) {
                    item.insert("last_run_status".into(), json!(status));
                }
            }
        }
    }
    Ok(true)
}

pub(super) fn read(
    home: &Path,
    checked_at: DateTime<Utc>,
    metadata: &HashMap<String, Value>,
) -> (Value, Value) {
    let path = home.join("sqlite/codex-dev.db");
    let mut items = Vec::new();
    let mut fields = Vec::new();
    let mut rows_read = 0;
    let mut runs_read = 0;
    let health = match read_database(
        &path,
        &mut items,
        &mut fields,
        &mut rows_read,
        &mut runs_read,
    ) {
        Ok(true) => "ok",
        Ok(false) => "unsupported",
        Err(_) if path.is_file() => "unavailable",
        Err(_) => "missing",
    };
    let result = json!({"items":items,"health":health,"row_limit":SCHEDULE_LIMIT,"run_limit":RUN_LIMIT,"remote_health":"cache_only","remote_activities":project_remote_activities(metadata)});
    let report = json!({"name":"automations","location":path.to_string_lossy(),"health":health,"checked_at":checked_at.to_rfc3339_opts(SecondsFormat::Millis, true),"fields":fields,"row_limit":SCHEDULE_LIMIT,"run_limit":RUN_LIMIT,"rows_read":rows_read,"runs_read":runs_read});
    (result, report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn preload_selects_recent_state_threads_only_when_requested() {
        let home = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(home.path().join("state_1.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,updated_at INTEGER); INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000001','Old',1),('00000000-0000-4000-8000-000000000002','New',2),('invalid','Hidden',3);").unwrap();
        let (preload, _) = super::background_metadata(
            home.path(),
            &std::collections::BTreeSet::new(),
            false,
            true,
        );
        assert_eq!(preload.len(), 2);
        assert_eq!(
            preload["00000000-0000-4000-8000-000000000002"]["name"],
            "New"
        );
        let (normal, _) = super::background_metadata(
            home.path(),
            &std::collections::BTreeSet::new(),
            false,
            false,
        );
        assert!(normal.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn remote_state_rejects_redirected_global_state() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = outside.path().join("state.json");
        std::fs::write(
            &path,
            json!({"electron-persisted-atom-state":{"orbit-outputs-v1":[{"threadId":id}]}})
                .to_string(),
        )
        .unwrap();
        symlink(&path, home.path().join(".codex-global-state.json")).unwrap();
        let mut entries = HashMap::from([(id.to_owned(), json!({"kind":"user"}))]);
        merge_remote_state(home.path(), &mut entries);
        assert_eq!(entries[id]["kind"], "user");
    }
    #[test]
    fn state_reports_include_failed_candidate_and_selected_rows() {
        let home = tempfile::tempdir().unwrap();
        let newer = rusqlite::Connection::open(home.path().join("state_6.sqlite")).unwrap();
        newer.execute("CREATE TABLE threads(id TEXT)", []).unwrap();
        let older = rusqlite::Connection::open(home.path().join("state_5.sqlite")).unwrap();
        older.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,cwd TEXT); INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000001','Title','/private');").unwrap();
        let observed = ["00000000-0000-4000-8000-000000000001".to_owned()]
            .into_iter()
            .collect();
        let (_, reports) = super::background_metadata(home.path(), &observed, false, false);
        let state: Vec<_> = reports
            .iter()
            .filter(|r| {
                r["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("state_"))
            })
            .collect();
        assert_eq!(state.len(), 2);
        assert_eq!(state[0]["health"], "unsupported");
        assert_eq!(state[1]["health"], "ok");
        assert_eq!(state[1]["rows_read"], 1);
        assert_eq!(state[1]["selected_threads"], 1);
        assert!(!state[1]["fields"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("cwd")));
    }
    #[test]
    fn catalog_report_distinguishes_missing_and_unsupported() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(super::catalog_entries(home.path()).1["health"], "missing");
        std::fs::create_dir(home.path().join("sqlite")).unwrap();
        let db = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
        db.execute("CREATE TABLE local_thread_catalog(thread_id TEXT)", [])
            .unwrap();
        let report = super::catalog_entries(home.path()).1;
        assert_eq!(report["health"], "unsupported");
        assert_eq!(report["queries"].as_array().unwrap().len(), 2);
        assert!(report["queries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|query| query["health"] == "ok"
                && query["selected_threads"] == 0
                && query["rows_read"] == 0));
    }
    use super::*;

    #[test]
    fn session_index_supplies_only_observed_missing_titles() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("session_index.jsonl");
        std::fs::write(path, b"{\"id\":\"one\",\"thread_name\":\"First\"}\n{\"id\":\"one\",\"thread_name\":\" Latest \"}\n{\"id\":\"two\",\"thread_name\":\"Hidden\"}\n").unwrap();
        let mut entries = HashMap::from([
            ("one".to_owned(), json!({"name":null})),
            ("two".to_owned(), json!({"name":null})),
        ]);
        let observed = BTreeSet::from(["one".to_owned()]);
        let report = merge_session_index(
            &home.path().canonicalize().unwrap(),
            &mut entries,
            &observed,
        );
        assert_eq!(report["health"], "ok");
        assert_eq!(report["matched_threads"], 1);
        assert_eq!(report["fields"], json!(["id", "thread_name"]));
        assert_eq!(entries["one"]["name"], "Latest");
        assert!(entries["two"]["name"].is_null());
        entries.get_mut("one").unwrap()["name"] = json!("Catalog");
        merge_session_index(
            &home.path().canonicalize().unwrap(),
            &mut entries,
            &observed,
        );
        assert_eq!(entries["one"]["name"], "Catalog");
    }

    #[cfg(unix)]
    #[test]
    fn session_index_reports_missing_and_refuses_links() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let mut entries = HashMap::new();
        let observed = BTreeSet::new();
        assert_eq!(
            merge_session_index(&root, &mut entries, &observed)["health"],
            "missing"
        );
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.join("session_index.jsonl")).unwrap();
        assert_eq!(
            merge_session_index(&root, &mut entries, &observed)["health"],
            "unavailable"
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn observed_parent_classifies_catalog_child_as_subagent() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("sqlite")).unwrap();
        let catalog = Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
        catalog.execute_batch("CREATE TABLE local_thread_catalog(host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at INTEGER, source_updated_at INTEGER, source_kind TEXT, thread_source TEXT, project_id TEXT); INSERT INTO local_thread_catalog VALUES('durable','00000000-0000-4000-8000-000000000002','Child',0,1700000000,'chatgpt','schedule',NULL);").unwrap();
        let db = Connection::open(home.path().join("state_1.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT, title TEXT, thread_source TEXT); CREATE TABLE thread_spawn_edges(parent_thread_id TEXT, child_thread_id TEXT); INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000002','Child',NULL); INSERT INTO thread_spawn_edges VALUES('00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-000000000002');").unwrap();
        let observed = BTreeSet::from(["00000000-0000-4000-8000-000000000001".to_owned()]);
        assert!(remote_activities(home.path(), &observed).is_empty());
        db.execute("DELETE FROM thread_spawn_edges", []).unwrap();
        assert_eq!(
            remote_activities(home.path(), &observed)[0]["kind"],
            "schedule"
        );
    }

    #[test]
    fn remote_schedules_merge_catalog_runs_and_newer_cached_threads() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("sqlite")).unwrap();
        let db = Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
        db.execute_batch("CREATE TABLE local_thread_catalog(host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at INTEGER, source_updated_at INTEGER, source_kind TEXT, thread_source TEXT, project_id TEXT, chatgpt_async_status TEXT); CREATE TABLE automations(id TEXT, status TEXT, kind TEXT); CREATE TABLE automation_runs(automation_id TEXT, thread_id TEXT); INSERT INTO automations VALUES('a','ACTIVE','heartbeat'); INSERT INTO local_thread_catalog VALUES('durable','cloud-thread','Cloud',0,1700000000,'chatgpt','user',NULL,'running'); INSERT INTO automation_runs VALUES('a','cloud-thread'); INSERT INTO local_thread_catalog VALUES('remote-control:host','00000000-0000-4000-8000-000000000001','Catalog',0,1700000001,'cli','schedule',NULL,'idle');").unwrap();
        let state = json!({"electron-persisted-atom-state":{"remote-thread-summaries-v3:remote-control:host":[{"conversationId":"00000000-0000-4000-8000-000000000001","title":"Older cache","threadSource":"user","updatedAt":1700000000}]}});
        std::fs::write(
            home.path().join(".codex-global-state.json"),
            state.to_string(),
        )
        .unwrap();
        let rows = remote_activities(home.path(), &BTreeSet::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Catalog");
        assert_eq!(rows[1]["kind"], "heartbeat");
        assert_eq!(rows[1]["environment"], "cloud");
    }

    #[test]
    fn selects_bounded_schedule_metadata_and_latest_run() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("sqlite")).unwrap();
        let db = Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
        db.execute_batch("CREATE TABLE automations(id TEXT, status TEXT, name TEXT, next_run_at INTEGER, prompt TEXT); CREATE TABLE automation_runs(automation_id TEXT, thread_id TEXT, created_at INTEGER, status TEXT, message TEXT); INSERT INTO automations VALUES('a-1','ACTIVE','  Fixture  ',1700000000000,'secret'); INSERT INTO automations VALUES('bad id','ACTIVE','skip',NULL,'secret'); INSERT INTO automation_runs VALUES('a-1','thread-1',1700000010,'completed','secret');").unwrap();
        let (result, report) = read(
            home.path(),
            DateTime::from_timestamp(0, 0).unwrap(),
            &HashMap::new(),
        );
        assert_eq!(result["health"], "ok");
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert_eq!(result["items"][0]["name"], "Fixture");
        assert_eq!(
            result["items"][0]["next_run_at"],
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(result["items"][0]["last_thread_id"], "thread-1");
        assert_eq!(report["rows_read"], 2);
        assert_eq!(report["runs_read"], 1);
        assert!(!result.to_string().contains("secret"));
    }
}
