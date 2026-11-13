//! Describe already loaded sources without reading additional files.
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn augment(snapshot: &mut Value, home: &Path, core: &Value, desktop: &Value, diagnostic_locations: &[PathBuf], runtime: &Value, journal: &Value) {
    let codex = snapshot["codex"].clone();
    let mcp = snapshot["mcp"].clone();
    let settings = snapshot["settings"].clone();
    let monitor = snapshot["monitor"].clone();
    let read = &codex["read_state"];
    let active = codex["health"] != "disabled";
    let updated = snapshot["updated_at"].clone();
    let window = codex["activity_scope"]["window"].clone();
    let account = &codex["account"];
    let methods: Vec<_> = account["methods"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, state)| json!({"name":name,"health":state["health"]}))
        .collect();
    snapshot["sources"]["account_api"] = json!({"name":"Codex account API","features":["usage"],"scope":"official_account_read_only","locations":[],"mode":"read_only","fields":["plan_type","limits","credits","account_usage"],"limits":{"cache_seconds":60,"timeout_seconds":10,"output_bytes":1048576},"readers":methods,"health":account["health"],"display_window":window});
    let location = home
        .join("monitoring")
        .join(lam_history_store::FILE_NAME)
        .to_string_lossy()
        .to_string();
    let enabled: Vec<_> = settings["observations"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| (value == true).then_some(key.clone()))
        .collect();
    let mut limits = read
        .as_object()
        .map(|values| {
            values
                .iter()
                .filter(|(_, value)| value.is_number() || value.is_boolean())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    limits.insert("read_bytes".into(), codex["bytes_read"].clone());
    limits.insert("unsupported_lines".into(), codex["malformed_lines"].clone());
    snapshot["sources"]["session"] = json!({"name":"Codex session","features":["codex","tools","git","workflow","skills","checks","files","sqlite","mcp","web","errors","logs","usage","dots"],"scope":"recent_tail_and_incremental_metadata","locations":[home.join("sessions").to_string_lossy()],"mode":"read_only","fields":[],"limits":limits,"readers":[],"health":codex["health"],"observations":enabled,"checked_at":updated,"display_window":window});
    let readers = codex["metadata_sources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let loaded: Vec<_> = readers
        .iter()
        .filter(|reader| reader["health"] == "ok")
        .collect();
    let health = if !active || settings["observations"]["metadata"] == false {
        "disabled"
    } else if loaded.is_empty() {
        if readers.is_empty() {
            "waiting"
        } else {
            "unavailable"
        }
    } else if loaded.len() == readers.len() {
        "ok"
    } else {
        "partly_unavailable"
    };
    let mut fields = BTreeSet::new();
    for reader in &loaded {
        for field in reader["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            fields.insert(field.to_owned());
        }
    }
    let locations: Vec<_> = readers
        .iter()
        .filter_map(|reader| reader["location"].as_str())
        .collect();
    snapshot["sources"]["catalog"] = json!({"name":"Codex / ChatGPT catalog","features":["codex","usage","dots","workflow","skills","plugins"],"scope":"titles_classification_and_local_thread_metadata","locations":locations,"mode":"read_only","fields":fields,"limits":{},"readers":readers,"health":health,"display_window":window});
    let checkpoint = &read["checkpoint"];
    let checkpoint_health = if active {
        checkpoint["write_health"]
            .as_str()
            .or_else(|| checkpoint["load_health"].as_str())
            .unwrap_or("waiting")
    } else {
        "disabled"
    };
    let checkpoint_limits = checkpoint
        .as_object()
        .map(|values| {
            values
                .iter()
                .filter(|(_, value)| value.is_number())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    snapshot["sources"]["thread_state"] = json!({"name":"Thread lifecycle / Skills","features":["codex","workflow","skills"],"scope":"confirmed_lifecycle_and_skill_checkpoints","locations":[location],"mode":"bounded_metadata_cache","fields":["thread_id","status","task_time","task_start","offset","skill","call_id"],"limits":checkpoint_limits,"readers":[{"name":"Checkpoint load","health":checkpoint["load_health"]},{"name":"Checkpoint save","health":checkpoint["write_health"]}],"health":checkpoint_health,"display_window":window});
    let history_health = if active {
        codex["sqlite"]["history_health"]
            .as_str()
            .unwrap_or("waiting")
    } else {
        "disabled"
    };
    snapshot["sources"]["activity_history"] = json!({"name":"Activity history","features":["sqlite","web","mcp"],"scope":"retained_activity_metadata","locations":[location],"mode":"bounded_metadata_cache","fields":["timestamp","thread_id","call_id","operation","status","duration_ms","reference_url"],"limits":{"retention_days":settings["activity_retention_days"],"sql_records":monitor["sql_records"],"web_records":monitor["web_records"],"mcp_records":monitor["mcp_records"]},"readers":[],"health":history_health,"display_window":window});
    let worktree_limits = codex["worktrees"].as_object().map(|fields| {
        fields.iter().filter(|(_, value)| value.is_i64() || value.is_u64() || value.is_boolean())
            .map(|(key, value)| (key.clone(), value.clone())).collect::<serde_json::Map<_, _>>()
    }).unwrap_or_default();
    snapshot["sources"]["worktrees"] = json!({"name":"Git worktrees","features":["projects","worktrees"],"scope":"loaded_project_roots_and_codex_managed_worktrees","locations":[home.join("worktrees").to_string_lossy()],"mode":"read_only","fields":["name","branch","commit","detached","locked","prunable","managed","project_ids","thread_ids","checked_at"],"limits":worktree_limits,"readers":[],"health":codex["worktrees"]["health"],"checked_at":codex["worktrees"]["checked_at"],"display_window":window});
    let reader_enabled = active && ["errors", "logs", "sqlite", "model_api"].iter().any(|key| settings["observations"][*key] == true);
    let mut readers = vec![desktop.clone(), core.clone()];
    if !reader_enabled {
        for reader in &mut readers {
            if reader.is_object() { reader["health"] = json!("disabled"); }
        }
    }
    let locations: Vec<_> = diagnostic_locations.iter().map(|path| path.to_string_lossy()).collect();
    snapshot["sources"]["diagnostics"] = json!({"name":"Codex diagnostics","features":["errors","logs","sqlite","model-api"],"scope":"recent_metadata_and_24h_error_backfill","locations":locations,"mode":"read_only","fields":["timestamp","severity","module","code","thread_id","call_id","request_id","trace_id","sql"],"limits":{"event_limit":super::core_logs::LOG_LIMIT,"file_limit":super::desktop::FILE_LIMIT},"readers":readers,"display_window":window});
    if !active || ["errors", "logs", "sqlite"].iter().all(|key| settings["observations"][*key] == false) {
        snapshot["sources"]["diagnostics"]["health"] = json!("disabled");
    }
    let configuration = &mcp["configuration"];
    if let Some(path) = configuration["location"].as_str() {
        snapshot["sources"]["mcp_config"] = json!({"name":"MCP configuration","features":["mcp","tools"],"scope":"configured_sources_and_purposes","locations":[path],"mode":"read_only","fields":["server","enabled","description","category","tags"],"limits":{"configured_sources":configuration["configured_sources"],"loaded_sources":mcp["servers"].as_array().map_or(0,Vec::len),"event_limit":super::mcp::EVENT_LIMIT,"tool_limit":super::mcp::TOOL_LIMIT},"readers":[],"display_window":window});
    }
    let mut runtime_fields: BTreeSet<String> = runtime.as_object().into_iter().flatten().map(|(key, _)| key.clone()).collect();
    for key in ["pid", "version", "fonts"] {
        if monitor.get(key).is_some() { runtime_fields.insert(key.to_owned()); }
    }
    snapshot["sources"]["monitor"] = json!({"name":"Local Activity Monitor","features":["monitor","errors","logs"],"scope":"runtime_samples_and_bounded_event_metadata","locations":[home.join("monitoring/local-activity-monitor.jsonl").to_string_lossy(),location],"mode":"bounded_metadata_cache","fields":runtime_fields,"limits":{"history_limit":monitor["history_limit"],"event_limit":monitor["event_limit"],"retained_samples":monitor["history"].as_array().map_or(0,Vec::len),"retained_events":monitor["events"].as_array().map_or(0,Vec::len),"byte_limit":journal["byte_limit"],"error_history_limit":lam_history_store::ERROR_PREVIEW_LIMIT},"readers":[],"health":monitor["health"],"checked_at":updated,"display_window":window});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_observed_worktree_limits_and_disabled_diagnostics() {
        let mut snapshot = json!({
            "codex":{"health":"ok","worktrees":{"health":"ok","root_limit":64,"count_complete":false,"managed_root":"private"}},
            "settings":{"observations":{"errors":false,"logs":false,"sqlite":false,"model_api":true}},
            "monitor":{"history_limit":360,"event_limit":200,"history":[{}],"events":[{},{}]}
        });
        let core = json!({"source":"codex_core","health":"ok"});
        let desktop = json!({"source":"codex_desktop","health":"missing"});
        let locations = [PathBuf::from("/fixture/desktop.log"), PathBuf::from("/fixture/logs.sqlite")];
        let runtime = json!({"platform":"fixture","architecture":"test"});
        let journal = json!({"byte_limit":131072});
        augment(&mut snapshot, Path::new("/fixture"), &core, &desktop, &locations, &runtime, &journal);
        let sources = &snapshot["sources"];
        assert_eq!(sources["worktrees"]["limits"]["root_limit"], 64);
        assert_eq!(sources["worktrees"]["limits"]["count_complete"], false);
        assert!(sources["worktrees"]["limits"]["managed_root"].is_null());
        assert_eq!(sources["diagnostics"]["health"], "disabled");
        assert_eq!(sources["diagnostics"]["readers"][0]["source"], "codex_desktop");
        assert_eq!(sources["diagnostics"]["readers"][0]["health"], "missing");
        assert_eq!(sources["diagnostics"]["limits"]["event_limit"], 1000);
        assert_eq!(sources["diagnostics"]["limits"]["file_limit"], 8);
        assert_eq!(sources["diagnostics"]["locations"], json!(["/fixture/desktop.log", "/fixture/logs.sqlite"]));
        assert_eq!(sources["monitor"]["limits"]["retained_events"], 2);
        assert_eq!(sources["monitor"]["limits"]["event_limit"], 200);
        assert_eq!(sources["monitor"]["limits"]["byte_limit"], 131072);
        assert_eq!(sources["monitor"]["limits"]["error_history_limit"], 1000);
        assert_eq!(sources["monitor"]["fields"], json!(["architecture", "platform"]));
        assert!(sources["thread_state"]["fields"].as_array().unwrap().contains(&json!("task_start")));
        snapshot["settings"]["observations"]["model_api"] = json!(false);
        augment(&mut snapshot, Path::new("/fixture"), &core, &desktop, &locations, &runtime, &journal);
        assert!(snapshot["sources"]["diagnostics"]["readers"].as_array().unwrap().iter().all(|reader| reader["health"] == "disabled"));
    }
}
