use super::schedules::{columns, seconds_timestamp, state_files, string};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::Duration;

fn text(value: Option<&str>, limit: usize) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(limit).collect())
}
fn number(value: ValueRef<'_>) -> Option<f64> {
    match value {
        ValueRef::Integer(n) => Some(n as f64),
        ValueRef::Real(n) => Some(n),
        _ => None,
    }
}
fn allowed(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

fn database(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    projects: &mut BTreeMap<String, Value>,
) {
    for path in state_files(home) {
        let Ok(db) = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        if db.busy_timeout(Duration::from_millis(80)).is_err()
            || db.pragma_update(None, "query_only", true).is_err()
        {
            continue;
        }
        let Ok(thread_fields) = columns(&db, "threads") else {
            continue;
        };
        if !thread_fields.contains("id") || !thread_fields.contains("title") {
            continue;
        }
        if thread_fields.contains("project_id") {
            let mut ids: Vec<_> = entries.keys().cloned().collect();
            ids.sort();
            for batch in ids.chunks(400) {
                let placeholders = vec!["?"; batch.len()].join(",");
                let sql = format!("SELECT id,project_id FROM threads WHERE id IN ({placeholders})");
                if let Ok(mut statement) = db.prepare(&sql) {
                    if let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter()))
                    {
                        while let Ok(Some(row)) = rows.next() {
                            let Ok(id) = row.get::<_, String>(0) else {
                                continue;
                            };
                            let Some(project) = row
                                .get_ref(1)
                                .ok()
                                .and_then(string)
                                .and_then(|s| text(Some(s), 160))
                            else {
                                continue;
                            };
                            if let Some(entry) = entries.get_mut(&id) {
                                entry["project_id"] = json!(project);
                                entry["project_scope"] = json!("project");
                            }
                        }
                    }
                }
            }
        }
        if let Ok(fields) = columns(&db, "projects") {
            if fields.contains("id") && fields.contains("name") {
                let created = if fields.contains("created_at_ms") {
                    "created_at_ms"
                } else {
                    "NULL"
                };
                let updated = if fields.contains("updated_at_ms") {
                    "updated_at_ms"
                } else {
                    "NULL"
                };
                let order = if fields.contains("updated_at_ms") {
                    "updated_at_ms DESC"
                } else {
                    "id"
                };
                let sql = format!(
                    "SELECT id,name,{created},{updated} FROM projects ORDER BY {order} LIMIT 2000"
                );
                if let Ok(mut statement) = db.prepare(&sql) {
                    if let Ok(mut rows) = statement.query([]) {
                        while let Ok(Some(row)) = rows.next() {
                            let Some(id) = row
                                .get_ref(0)
                                .ok()
                                .and_then(string)
                                .and_then(|s| text(Some(s), 160))
                            else {
                                continue;
                            };
                            let name = row
                                .get_ref(1)
                                .ok()
                                .and_then(string)
                                .and_then(|s| text(Some(s), 512));
                            let mut project = json!({"id":id,"name":name,"source":"projects"});
                            for (index, key) in [(2, "created_at"), (3, "updated_at")] {
                                if let Some(value) = row.get_ref(index).ok().and_then(number) {
                                    project[key] = seconds_timestamp(value / 1000.0);
                                }
                            }
                            projects.insert(id, project);
                        }
                    }
                }
            }
        }
        break;
    }
}

pub(super) fn read(home: &Path, entries: &mut HashMap<String, Value>) -> BTreeMap<String, Value> {
    let mut projects = BTreeMap::new();
    database(home, entries, &mut projects);
    for entry in entries.values_mut() {
        if let Some(name) = entry["project_id"]
            .as_str()
            .and_then(|id| projects.get(id))
            .and_then(|project| project["name"].as_str())
        {
            entry["project_name"] = json!(name);
        }
    }
    let Some(state) = crate::dots::read_state(home) else {
        return projects;
    };
    let locals = state.get("local-projects").and_then(Value::as_object);
    let mappings = state
        .get("app-server-project-id-by-legacy-project-id-by-host")
        .and_then(Value::as_object);
    let map_id = |id: &str, host: &str, projects: &BTreeMap<String, Value>| -> String {
        mappings
            .and_then(|m| m.get(host))
            .and_then(|m| m.get(id))
            .and_then(Value::as_str)
            .filter(|target| {
                projects
                    .get(*target)
                    .is_some_and(|p| p["source"] == "projects")
            })
            .unwrap_or(id)
            .to_owned()
    };
    if let Some(locals) = locals {
        for (id, project) in locals.iter().take(2000) {
            let Some(id) = text(Some(id), 160) else {
                continue;
            };
            if !project.is_object() {
                continue;
            }
            let mapped = map_id(&id, "local", &projects);
            let entry = projects
                .entry(mapped.clone())
                .or_insert_with(|| json!({"id":mapped}));
            let name = text(project.get("name").and_then(Value::as_str), 512);
            if mapped == id && entry["source"] != "projects" {
                entry["name"] = json!(name);
                entry["source"] = json!("local-projects");
            } else if entry["name"].is_null() {
                entry["name"] = json!(name);
            }
        }
    }
    let assignments = state
        .get("thread-project-assignments")
        .and_then(Value::as_object);
    for (id, entry) in entries.iter_mut() {
        let assignment = assignments
            .and_then(|assignments| assignments.get(id))
            .filter(|value| value.is_object());
        let original = assignment
            .and_then(|assignment| text(assignment.get("projectId").and_then(Value::as_str), 160));
        if let Some(original) = &original {
            let host = entry["host_id"].as_str().unwrap_or("local");
            let mapped = map_id(original, host, &projects);
            entry["project_id"] = json!(mapped);
            entry["project_scope"] = json!("project");
            for (source, target) in [("projectKind", "kind"), ("projectOrigin", "origin")] {
                if let Some(value) = assignment
                    .and_then(|a| a.get(source))
                    .and_then(Value::as_str)
                    .filter(|value| allowed(value))
                {
                    projects
                        .entry(mapped.clone())
                        .or_insert_with(|| json!({"id":mapped,"name":null}))[target] = json!(value);
                }
            }
        }
        let local_id = original.as_deref().or_else(|| entry["project_id"].as_str());
        if let Some(local) = local_id
            .and_then(|id| locals.and_then(|locals| locals.get(id)))
            .filter(|value| value.is_object())
        {
            let project_id = entry["project_id"].as_str().unwrap_or("").to_owned();
            let known = projects
                .get(&project_id)
                .and_then(|p| p["name"].as_str())
                .map(str::to_owned);
            let name = known.or_else(|| text(local.get("name").and_then(Value::as_str), 512));
            entry["project_name"] = json!(name);
            if !project_id.is_empty() {
                let project = projects
                    .entry(project_id.clone())
                    .or_insert_with(|| json!({"id":project_id}));
                if project["source"] != "projects" {
                    project["name"] = json!(text(local.get("name").and_then(Value::as_str), 512));
                    project["source"] = json!("local-projects");
                }
            }
        }
    }
    for entry in entries.values_mut() {
        if let Some(project) = entry["project_id"].as_str().and_then(|id| projects.get(id)) {
            entry["project_kind"] = project["kind"].clone();
        }
    }
    projects
}

/// Resolve folders only for a project already selected from a published snapshot.
pub(crate) fn detail_folders(
    home: &Path,
    project_id: &str,
    threads: &[Value],
) -> (Vec<String>, Option<&'static str>) {
    let mut folders = Vec::new();
    let mut source = None;
    let mut database_ids = std::collections::BTreeSet::new();
    for path in state_files(home) {
        let Ok(db) = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        if db.busy_timeout(Duration::from_millis(80)).is_err()
            || db.pragma_update(None, "query_only", true).is_err()
        {
            continue;
        }
        let Ok(fields) = columns(&db, "threads") else {
            continue;
        };
        if !fields.contains("id") || !fields.contains("title") {
            continue;
        }
        if let Ok(fields) = columns(&db, "projects") {
            if fields.contains("id") && fields.contains("name") {
                let order = if fields.contains("updated_at_ms") {
                    "updated_at_ms DESC"
                } else {
                    "id"
                };
                if let Ok(mut statement) = db.prepare(&format!(
                    "SELECT id FROM projects ORDER BY {order} LIMIT 2000"
                )) {
                    if let Ok(mut rows) = statement.query([]) {
                        while let Ok(Some(row)) = rows.next() {
                            if let Some(id) = row
                                .get_ref(0)
                                .ok()
                                .and_then(string)
                                .and_then(|value| text(Some(value), 160))
                            {
                                database_ids.insert(id);
                            }
                        }
                    }
                }
            }
        }
        if database_ids.contains(project_id) {
            source = Some("projects");
        }
        if let Ok(fields) = columns(&db, "project_roots") {
            if fields.contains("project_id") && fields.contains("path") {
                if let Ok(mut statement) =
                    db.prepare("SELECT path FROM project_roots WHERE project_id=?1 LIMIT 5000")
                {
                    if let Ok(mut rows) = statement.query([project_id]) {
                        while let Ok(Some(row)) = rows.next() {
                            if source.is_none() {
                                source = Some("project_roots");
                            }
                            if folders.len() < 32 {
                                if let Some(folder) = row
                                    .get_ref(0)
                                    .ok()
                                    .and_then(string)
                                    .and_then(|value| text(Some(value), 4096))
                                {
                                    folders.push(folder);
                                }
                            }
                        }
                    }
                }
            }
        }
        break;
    }
    if let Some(state) = crate::dots::read_state(home) {
        let locals = state.get("local-projects").and_then(Value::as_object);
        let mappings = state
            .get("app-server-project-id-by-legacy-project-id-by-host")
            .and_then(Value::as_object);
        let map_id = |id: &str, host: &str| -> String {
            mappings
                .and_then(|maps| maps.get(host))
                .and_then(|map| map.get(id))
                .and_then(Value::as_str)
                .filter(|target| database_ids.contains(*target))
                .unwrap_or(id)
                .to_owned()
        };
        let assignments = state
            .get("thread-project-assignments")
            .and_then(Value::as_object);
        let mut candidates = std::collections::BTreeSet::new();
        if let Some(locals) = locals {
            for (id, value) in locals.iter().take(2000) {
                let Some(id) = text(Some(id), 160) else {
                    continue;
                };
                if value.is_object() && map_id(&id, "local") == project_id {
                    candidates.insert(id);
                }
            }
        }
        for thread in threads {
            let Some(id) = thread["thread_id"].as_str() else {
                continue;
            };
            let host = thread["host_id"].as_str().unwrap_or("local");
            let Some(id) = assignments
                .and_then(|a| a.get(id))
                .and_then(|value| text(value.get("projectId").and_then(Value::as_str), 160))
            else {
                continue;
            };
            if map_id(&id, host) == project_id {
                candidates.insert(id);
            }
        }
        for candidate in candidates {
            let Some(local) = locals
                .and_then(|locals| locals.get(&candidate))
                .filter(|value| value.is_object())
            else {
                continue;
            };
            if source != Some("projects") {
                source = Some("local-projects");
            }
            if folders.is_empty() {
                if let Some(paths) = local.get("rootPaths").and_then(Value::as_array) {
                    folders = paths
                        .iter()
                        .take(32)
                        .filter_map(|path| text(path.as_str(), 4096))
                        .collect();
                }
            }
        }
    }
    (folders, source)
}
