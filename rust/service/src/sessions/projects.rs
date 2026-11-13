use super::schedules::{columns, seconds_timestamp, state_files, string};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RootStatus {
    pub complete: bool,
    pub limited: bool,
}
impl Default for RootStatus {
    fn default() -> Self {
        Self {
            complete: true,
            limited: false,
        }
    }
}

pub(super) fn preserve_roots(
    current: &mut BTreeMap<String, Value>,
    previous: &BTreeMap<String, Value>,
) {
    for (id, old) in previous {
        let Some(old_folders) = old["_folders"]
            .as_array()
            .filter(|folders| !folders.is_empty())
        else {
            continue;
        };
        let target = current.entry(id.clone()).or_insert_with(|| old.clone());
        let new_folders = target["_folders"].as_array().cloned().unwrap_or_default();
        let mut merged = old_folders.clone();
        for folder in new_folders {
            if merged.len() >= 32 {
                break;
            }
            if !merged.contains(&folder) {
                merged.push(folder);
            }
        }
        target["_folders"] = json!(merged);
    }
}

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
    include_roots: bool,
    root_budget: Duration,
    reports: Option<&mut Vec<Value>>,
) -> RootStatus {
    let paths = state_files(home);
    let mut failed = false;
    let mut status = RootStatus::default();
    let mut reports = reports;
    for path in paths {
        let Ok(mut db) = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            failed = true;
            continue;
        };
        if db.busy_timeout(Duration::from_millis(80)).is_err()
            || db.pragma_update(None, "query_only", true).is_err()
        {
            failed = true;
            continue;
        }
        let Ok(thread_fields) = columns(&db, "threads") else {
            failed = true;
            continue;
        };
        if !thread_fields.contains("id") || !thread_fields.contains("title") {
            failed = true;
            continue;
        }
        let mut project_rows = 0usize;
        let mut root_rows = 0usize;
        let mut project_error = false;
        let mut reported_fields = Vec::new();
        if thread_fields.contains("project_id") {
            reported_fields.push("project_id");
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
                        loop {
                            let row = match rows.next() {
                                Ok(Some(row)) => row,
                                Ok(None) => break,
                                Err(_) => {
                                    project_error = true;
                                    if include_roots {
                                        status.complete = false;
                                    }
                                    break;
                                }
                            };
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
                    } else {
                        project_error = true;
                        if include_roots {
                            status.complete = false;
                        }
                    }
                } else {
                    project_error = true;
                    if include_roots {
                        status.complete = false;
                    }
                }
            }
        }
        let project_fields = match columns(&db, "projects") {
            Ok(fields) => Some(fields),
            Err(_) => {
                project_error = true;
                if include_roots {
                    status.complete = false;
                }
                None
            }
        };
        if let Some(fields) = project_fields {
            if fields.contains("id") && fields.contains("name") {
                reported_fields.extend(["projects.id", "projects.name"]);
                if fields.contains("created_at_ms") {
                    reported_fields.push("projects.created_at_ms");
                }
                if fields.contains("updated_at_ms") {
                    reported_fields.push("projects.updated_at_ms");
                }
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
                        loop {
                            let row = match rows.next() {
                                Ok(Some(row)) => row,
                                Ok(None) => break,
                                Err(_) => {
                                    project_error = true;
                                    if include_roots {
                                        status.complete = false;
                                    }
                                    break;
                                }
                            };
                            project_rows += 1;
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
                    } else {
                        project_error = true;
                        if include_roots {
                            status.complete = false;
                        }
                    }
                } else {
                    project_error = true;
                    if include_roots {
                        status.complete = false;
                    }
                }
            }
        }
        let root_fields = if include_roots {
            match columns(&db, "project_roots") {
                Ok(fields) => Some(fields),
                Err(_) => {
                    project_error = true;
                    status.complete = false;
                    None
                }
            }
        } else {
            None
        };
        if root_fields
            .is_some_and(|fields| fields.contains("project_id") && fields.contains("path"))
        {
            reported_fields.extend(["project_roots.path", "project_roots.project_id"]);
            let deadline = Instant::now() + root_budget;
            if db
                .progress_handler(1000, Some(move || Instant::now() >= deadline))
                .is_err()
            {
                status.complete = false;
                if let Some(report) = reports.as_deref_mut().and_then(|reports| {
                    reports.iter_mut().find(|report| {
                        report["location"].as_str() == path.to_str() && report["health"] == "ok"
                    })
                }) {
                    report["health"] = json!("unavailable");
                    report["fields"] = json!([]);
                }
                break;
            }
            let mut ids: std::collections::BTreeSet<_> = projects.keys().cloned().collect();
            ids.extend(
                entries
                    .values()
                    .filter_map(|entry| entry["project_id"].as_str().map(str::to_owned)),
            );
            let ids: Vec<_> = ids.into_iter().collect();
            let mut total = 0usize;
            for batch in ids.chunks(400) {
                if Instant::now() >= deadline {
                    status.complete = false;
                    break;
                }
                if total >= 5000 {
                    status.complete = false;
                    status.limited = true;
                    break;
                }
                let placeholders = vec!["?"; batch.len()].join(",");
                let query = format!("SELECT project_id,path FROM project_roots WHERE project_id IN ({placeholders}) LIMIT {}", 5001-total);
                if let Ok(mut statement) = db.prepare(&query) {
                    if let Ok(mut rows) = statement.query(rusqlite::params_from_iter(batch.iter()))
                    {
                        loop {
                            let row = match rows.next() {
                                Ok(Some(row)) => row,
                                Ok(None) => break,
                                Err(_) => {
                                    project_error = true;
                                    status.complete = false;
                                    break;
                                }
                            };
                            total += 1;
                            root_rows = total.min(5000);
                            if total > 5000 {
                                status.complete = false;
                                status.limited = true;
                                break;
                            }
                            let Some(id) = row
                                .get_ref(0)
                                .ok()
                                .and_then(string)
                                .and_then(|value| text(Some(value), 160))
                            else {
                                continue;
                            };
                            let Some(folder) = row
                                .get_ref(1)
                                .ok()
                                .and_then(string)
                                .and_then(|value| text(Some(value), 4096))
                            else {
                                continue;
                            };
                            let project = projects.entry(id.clone()).or_insert_with(
                                || json!({"id":id,"name":null,"source":"project_roots"}),
                            );
                            let folders = project
                                .as_object_mut()
                                .expect("project object")
                                .entry("_folders".to_owned())
                                .or_insert_with(|| json!([]))
                                .as_array_mut()
                                .expect("folder array");
                            if folders.len() < 32 {
                                folders.push(json!(folder));
                            }
                        }
                    } else {
                        project_error = true;
                        status.complete = false;
                    }
                } else {
                    project_error = true;
                    status.complete = false;
                }
                if !status.complete {
                    break;
                }
            }
            let _ = db.progress_handler(0, None::<fn() -> bool>);
        }
        if let Some(report) = reports.as_deref_mut().and_then(|reports| {
            reports.iter_mut().find(|report| {
                report["location"].as_str() == path.to_str() && report["health"] == "ok"
            })
        }) {
            if project_error {
                report["health"] = json!("unavailable");
                report["fields"] = json!([]);
                continue;
            }
            if let Some(fields) = report["fields"].as_array_mut() {
                for field in reported_fields {
                    let field = json!(field);
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
            }
            report["project_rows"] = json!(project_rows);
            report["project_root_rows"] = json!(root_rows);
            report["project_limit"] = json!(2000);
            report["project_root_limit"] = json!(5000);
        }
        break;
    }
    if include_roots && failed {
        status.complete = false;
    }
    status
}

pub(super) fn read(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    include_roots: bool,
    reports: &mut Vec<Value>,
) -> (BTreeMap<String, Value>, RootStatus) {
    read_internal(
        home,
        entries,
        include_roots,
        Duration::from_secs(5),
        Some(reports),
    )
}

#[cfg(test)]
fn read_with_budget(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    include_roots: bool,
    root_budget: Duration,
) -> (BTreeMap<String, Value>, RootStatus) {
    read_internal(home, entries, include_roots, root_budget, None)
}

fn read_internal(
    home: &Path,
    entries: &mut HashMap<String, Value>,
    include_roots: bool,
    root_budget: Duration,
    mut reports: Option<&mut Vec<Value>>,
) -> (BTreeMap<String, Value>, RootStatus) {
    let mut projects = BTreeMap::new();
    let status = database(
        home,
        entries,
        &mut projects,
        include_roots,
        root_budget,
        reports.as_deref_mut(),
    );
    for entry in entries.values_mut() {
        if let Some(name) = entry["project_id"]
            .as_str()
            .and_then(|id| projects.get(id))
            .and_then(|project| project["name"].as_str())
        {
            entry["project_name"] = json!(name);
        }
    }
    let state_result = crate::dots::read_state_with_health(home);
    if let Some(reports) = reports {
        let path = home.join(".codex-global-state.json");
        let checked = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let health = state_result
            .as_ref()
            .map(|_| "ok")
            .unwrap_or_else(|health| *health);
        let mut report = json!({"name":".codex-global-state.json","location":path.to_string_lossy(),"health":health,"checked_at":checked,"fields":[]});
        if health == "ok" {
            report["fields"] = json!([
                "project_name",
                "project_membership",
                "project_kind",
                "project_origin",
                "project_root_metadata",
                "legacy_project_id_mapping",
                "dot_thread_id",
                "artifact_type",
                "produced_at",
                "host_id",
                "remote_thread_summary",
                "dot_activity"
            ]);
            report["byte_limit"] = json!(16 * 1024 * 1024);
            report["project_limit"] = json!(2000);
            report["loaded_projects"] = json!(state_result
                .as_ref()
                .ok()
                .and_then(|state| state.get("local-projects"))
                .and_then(Value::as_object)
                .map_or(0, |locals| locals
                    .iter()
                    .take(2000)
                    .filter(|(id, value)| text(Some(id), 160).is_some() && value.is_object())
                    .count()));
        } else if matches!(health, "oversized" | "unsupported") {
            report["byte_limit"] = json!(16 * 1024 * 1024);
        }
        reports.push(report);
    }
    let Ok(state) = state_result else {
        return (projects, status);
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
            if include_roots && entry["_folders"].as_array().is_none_or(Vec::is_empty) {
                if let Some(paths) = project["rootPaths"].as_array() {
                    entry["_folders"] = json!(paths
                        .iter()
                        .take(32)
                        .filter_map(|value| text(value.as_str(), 4096))
                        .collect::<Vec<_>>());
                }
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
                if include_roots && project["_folders"].as_array().is_none_or(Vec::is_empty) {
                    if let Some(paths) = local["rootPaths"].as_array() {
                        project["_folders"] = json!(paths
                            .iter()
                            .take(32)
                            .filter_map(|value| text(value.as_str(), 4096))
                            .collect::<Vec<_>>());
                    }
                }
            }
        }
    }
    for entry in entries.values_mut() {
        if let Some(project) = entry["project_id"].as_str().and_then(|id| projects.get(id)) {
            entry["project_kind"] = project["kind"].clone();
        }
    }
    (projects, status)
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

#[cfg(test)]
mod tests {
    #[test]
    fn failed_project_schema_does_not_report_state_as_healthy() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("state_1.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT); CREATE VIEW projects AS SELECT id,name FROM missing_project_table;").unwrap();
        let mut reports =
            vec![json!({"location":path.to_string_lossy(),"health":"ok","fields":["id","title"]})];
        read(home.path(), &mut HashMap::new(), false, &mut reports);
        assert_eq!(reports[0]["health"], "unavailable");
    }
    #[test]
    fn project_counts_attach_to_matching_state_reader() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("state_1.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT); CREATE TABLE projects(id TEXT,name TEXT); INSERT INTO projects VALUES('p','Project'); CREATE TABLE project_roots(project_id TEXT,path TEXT); INSERT INTO project_roots VALUES('p','/fixture/root');").unwrap();
        let mut reports =
            vec![json!({"location":path.to_string_lossy(),"health":"ok","fields":["id","title"]})];
        read(home.path(), &mut HashMap::new(), true, &mut reports);
        assert_eq!(reports[0]["project_rows"], 1);
        assert_eq!(reports[0]["project_root_rows"], 1);
        assert!(reports[0]["fields"]
            .as_array()
            .unwrap()
            .contains(&json!("projects.name")));
    }
    #[test]
    fn app_state_report_distinguishes_missing_invalid_and_loaded() {
        let home = tempfile::tempdir().unwrap();
        let mut reports = Vec::new();
        read(home.path(), &mut HashMap::new(), false, &mut reports);
        assert_eq!(reports[0]["health"], "missing");
        let path = home.path().join(".codex-global-state.json");
        std::fs::write(&path, "[]").unwrap();
        reports.clear();
        read(home.path(), &mut HashMap::new(), false, &mut reports);
        assert_eq!(reports[0]["health"], "unsupported");
        std::fs::write(
            path,
            r#"{"local-projects":{"valid":{"name":"A"},"invalid":null}}"#,
        )
        .unwrap();
        reports.clear();
        read(home.path(), &mut HashMap::new(), false, &mut reports);
        assert_eq!(reports[0]["health"], "ok");
        assert_eq!(reports[0]["loaded_projects"], 1);
    }
    use super::*;

    #[test]
    fn batched_roots_match_detail_priority_and_observation_switch() {
        let home = tempfile::tempdir().unwrap();
        let db = Connection::open(home.path().join("state_1.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT); CREATE TABLE projects(id TEXT,name TEXT); INSERT INTO projects VALUES('a','Alpha'),('b','Beta'); CREATE TABLE project_roots(project_id TEXT,path TEXT); INSERT INTO project_roots VALUES('a','/database/alpha');").unwrap();
        let state = json!({"local-projects":{"old-a":{"name":"Old Alpha","rootPaths":["/local/alpha"]},"old-b":{"name":"Old Beta","rootPaths":["/local/beta"]}},"app-server-project-id-by-legacy-project-id-by-host":{"local":{"old-a":"a","old-b":"b"}}});
        std::fs::write(
            home.path().join(".codex-global-state.json"),
            state.to_string(),
        )
        .unwrap();
        let mut entries = HashMap::new();
        let (projects, status) = read(home.path(), &mut entries, true, &mut Vec::new());
        assert!(status.complete);
        for id in ["a", "b"] {
            let (expected, _) = detail_folders(home.path(), id, &[]);
            assert_eq!(projects[id]["_folders"], json!(expected));
        }
        assert_eq!(projects["a"]["_folders"], json!(["/database/alpha"]));
        assert_eq!(projects["b"]["_folders"], json!(["/local/beta"]));
        let (disabled, _) = read(home.path(), &mut HashMap::new(), false, &mut Vec::new());
        assert!(disabled
            .values()
            .all(|project| project.get("_folders").is_none()));
    }

    #[test]
    fn exhausted_root_budget_marks_incomplete_and_keeps_previous_roots() {
        let home = tempfile::tempdir().unwrap();
        let db = Connection::open(home.path().join("state_1.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT); CREATE TABLE projects(id TEXT,name TEXT); INSERT INTO projects VALUES('a','Alpha'); CREATE TABLE project_roots(project_id TEXT,path TEXT); INSERT INTO project_roots VALUES('a','/fixture/root');").unwrap();
        let (previous, status) = read_with_budget(
            home.path(),
            &mut HashMap::new(),
            true,
            Duration::from_secs(1),
        );
        assert!(status.complete);
        let (mut current, status) =
            read_with_budget(home.path(), &mut HashMap::new(), true, Duration::ZERO);
        assert!(!status.complete);
        assert!(current["a"].get("_folders").is_none());
        preserve_roots(&mut current, &previous);
        assert_eq!(current["a"]["_folders"], json!(["/fixture/root"]));
    }

    #[test]
    fn root_row_limit_marks_partial_instead_of_complete() {
        let home = tempfile::tempdir().unwrap();
        let db = Connection::open(home.path().join("state_1.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT); CREATE TABLE projects(id TEXT,name TEXT); INSERT INTO projects VALUES('a','Alpha'); CREATE TABLE project_roots(project_id TEXT,path TEXT); WITH RECURSIVE items(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM items WHERE n<5001) INSERT INTO project_roots SELECT 'a','/fixture/'||n FROM items;").unwrap();
        let (projects, status) = read_with_budget(
            home.path(),
            &mut HashMap::new(),
            true,
            Duration::from_secs(2),
        );
        assert!(!status.complete && status.limited);
        assert_eq!(projects["a"]["_folders"].as_array().unwrap().len(), 32);
    }
}
