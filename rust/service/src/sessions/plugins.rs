//! Bounded plugin configuration and cache metadata, excluding runtime arguments.
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::LazyLock,
};
const ROWS: usize = 256;
const MANIFEST: usize = 65536;
const BYTES: usize = 2 * 1024 * 1024;
const DIRECTORIES: usize = 4096;
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]{1,160}$").unwrap());
static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]+)*(?:[-+][A-Za-z0-9_.-]+)?$").unwrap());
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*\[plugins\.\"([A-Za-z0-9_.-]+)@([A-Za-z0-9_.-]+)\"\]\s*(?:#.*)?$"#).unwrap()
});
static ENABLED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*enabled\s*=\s*(true|false)\s*(?:#.*)?$").unwrap());
#[derive(Default)]
struct Budget {
    bytes: usize,
    dirs: usize,
    rows: usize,
    watch: BTreeSet<PathBuf>,
    watch_overflow: bool,
}
impl Budget {
    fn watch_dir(&mut self, path: &Path) {
        self.watch_path(path, true);
    }
    fn watch_file(&mut self, path: &Path) {
        self.watch_path(path, false);
    }
    fn watch_path(&mut self, path: &Path, directory: bool) {
        if safe(path).is_ok()
            && fs::symlink_metadata(path).is_ok_and(|metadata| {
                if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                }
            })
        {
            if self.watch.contains(path) {
                return;
            }
            if self.watch.len() < DIRECTORIES {
                self.watch.insert(path.to_path_buf());
            } else {
                self.watch_overflow = true;
            }
        }
    }
}
fn safe(path: &Path) -> io::Result<()> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) if super::redirected(&meta) => {
                return Err(io::Error::other("redirected path"))
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn dirs(path: &Path, budget: &mut Budget, limit: usize) -> (Vec<PathBuf>, &'static str) {
    if let Err(error) = safe(path) {
        return (
            Vec::new(),
            if error.kind() == io::ErrorKind::NotFound {
                "missing"
            } else {
                "unavailable"
            },
        );
    }
    let Ok(read) = fs::read_dir(path) else {
        return (Vec::new(), "unavailable");
    };
    budget.watch_dir(path);
    let remaining = DIRECTORIES.saturating_sub(budget.dirs);
    let mut list = Vec::new();
    for row in read.take((limit + 1).min(remaining)) {
        match row {
            Ok(row) => list.push(row.path()),
            Err(_) => return (Vec::new(), "unavailable"),
        }
    }
    budget.dirs += list.len();
    let state = if list.len() > limit || remaining <= list.len() {
        "truncated"
    } else {
        "available"
    };
    list.truncate(limit);
    list.retain(|entry| {
        entry
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| NAME.is_match(s))
            && safe(entry).is_ok()
            && fs::symlink_metadata(entry).is_ok_and(|m| m.is_dir())
    });
    for entry in &list {
        budget.watch_dir(entry);
    }
    (list, state)
}
fn read(home: &Path, path: &Path, budget: &mut Budget) -> (Option<Value>, &'static str) {
    if let Some(parent) = path.parent() {
        budget.watch_dir(parent);
    }
    budget.watch_file(path);
    if let Err(error) = safe(path) {
        return (
            None,
            if error.kind() == io::ErrorKind::NotFound {
                "missing"
            } else {
                "unavailable"
            },
        );
    }
    let remaining = BYTES.saturating_sub(budget.bytes);
    if remaining == 0 {
        return (None, "truncated");
    }
    let Ok(file) = super::open_source(home, path) else {
        return (None, "unavailable");
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return (None, "unavailable");
    }
    let mut bytes = Vec::new();
    if file
        .take((MANIFEST + 1).min(remaining) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return (None, "unavailable");
    }
    budget.bytes += bytes.len();
    if bytes.len() > MANIFEST || bytes.len() == remaining {
        return (None, "truncated");
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) if value.is_object() => (Some(value), "available"),
        _ => (None, "unavailable"),
    }
}
fn text(value: &Value, max: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then(|| value.chars().take(max).collect())
}
fn version_key(value: &str) -> Vec<&str> {
    value
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|v| {
            let trimmed = v.trim_start_matches('0');
            if trimmed.is_empty() {
                "0"
            } else {
                trimmed
            }
        })
        .collect()
}
fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let a = version_key(a);
    let b = version_key(b);
    for (a, b) in a.iter().zip(&b) {
        let cmp = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
    }
    a.len().cmp(&b.len())
}
fn metadata(home: &Path, directory: &Path, budget: &mut Budget) -> Value {
    let (mut manifest, mut status) =
        read(home, &directory.join(".codex-plugin/plugin.json"), budget);
    let mut source = ".codex-plugin/plugin.json";
    if status == "missing" {
        (manifest, status) = read(home, &directory.join("plugin.json"), budget);
        source = "plugin.json";
    }
    let mut result = json!({"version":directory.file_name().and_then(|s|s.to_str()).filter(|s|VERSION.is_match(s)),"metadata_status":status,"manifest_source":if manifest.is_some(){Some(source)}else{None},"skills_count":null,"mcp_count":null,"skills":null,"mcp_servers":null,"components":[],"skills_status":"not_read","mcp_status":"not_read"});
    let Some(manifest) = manifest else {
        return result;
    };
    if let Some(version) = text(&manifest["version"], 80) {
        result["version"] = json!(version)
    }
    let interface = manifest
        .get("interface")
        .filter(|v| v.is_object())
        .or_else(|| {
            manifest
                .pointer("/extensions/com.openai/interface")
                .filter(|v| v.is_object())
        });
    result["display_name"] = json!(interface.and_then(|v| text(&v["displayName"], 160)));
    result["description"] = json!(text(&manifest["description"], 512));
    let mut components: Vec<String> = [
        "skills",
        "mcpServers",
        "apps",
        "hooks",
        "commands",
        "agents",
    ]
    .into_iter()
    .filter(|key| manifest.get(*key).is_some())
    .map(str::to_owned)
    .collect();
    let allowed_skills = manifest.get("skills").is_none_or(|v| {
        v.as_str()
            .is_some_and(|s| ["./skills/", "./skills", "skills", "skills/"].contains(&s))
    });
    if allowed_skills {
        let (entries, state) = dirs(&directory.join("skills"), budget, ROWS);
        for entry in &entries {
            budget.watch_file(&entry.join("SKILL.md"));
        }
        result["skills_status"] = json!(state);
        if ["available", "missing", "truncated"].contains(&state) {
            let mut names: Vec<String> = entries
                .into_iter()
                .filter(|entry| {
                    let path = entry.join("SKILL.md");
                    safe(&path).is_ok() && fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
                })
                .filter_map(|entry| entry.file_name()?.to_str().map(str::to_owned))
                .collect();
            names.sort();
            let missing = state == "missing" && manifest.get("skills").is_some();
            if !missing {
                result["skills"] = json!(names);
                if state != "truncated" {
                    result["skills_count"] = json!(names.len());
                }
            }
            if !names.is_empty() && !components.iter().any(|v| v == "skills") {
                components.push("skills".into());
            }
        }
    } else {
        result["skills_status"] = json!("unsupported");
    }
    let mut servers = None;
    let mut server_state = "missing";
    match manifest.get("mcpServers") {
        Some(Value::Object(map)) => {
            servers = Some(map.clone());
            server_state = "available"
        }
        None => servers = Some(Default::default()),
        Some(Value::String(path)) if ["./.mcp.json", ".mcp.json"].contains(&path.as_str()) => {
            let (value, state) = read(home, &directory.join(".mcp.json"), budget);
            server_state = state;
            servers = value.and_then(|v| v.get("mcpServers").and_then(Value::as_object).cloned());
            if state == "available" && servers.is_none() {
                server_state = "unavailable"
            }
        }
        _ => server_state = "unsupported",
    }
    if let Some(servers) = servers {
        let mut names: Vec<_> = servers
            .keys()
            .filter(|name| NAME.is_match(name))
            .cloned()
            .collect();
        names.sort();
        let count = names.len();
        if count > ROWS {
            server_state = "truncated"
        } else if count != servers.len() {
            server_state = "unavailable"
        }
        names.truncate(ROWS);
        result["mcp_servers"] = json!(names);
        if server_state != "truncated" && server_state != "unavailable" {
            result["mcp_count"] = json!(count);
        }
    }
    result["mcp_status"] = json!(server_state);
    result["components"] = json!(components);
    result
}
fn configured(home: &Path, budget: &mut Budget) -> Vec<((String, String), Option<bool>)> {
    let mut result: Vec<((String, String), Option<bool>)> = Vec::new();
    let path = home.join("config.toml");
    budget.watch_file(&path);
    if safe(&path).is_err() {
        return result;
    }
    let Ok(file) = super::open_source(home, &path) else {
        return result;
    };
    let mut data = Vec::new();
    if file
        .take((MANIFEST + 1) as u64)
        .read_to_end(&mut data)
        .is_err()
    {
        return result;
    }
    budget.bytes += data.len();
    if data.len() > MANIFEST {
        return result;
    }
    let Ok(data) = std::str::from_utf8(&data) else {
        return result;
    };
    let mut current = None;
    for line in data.lines() {
        if line.trim_start().starts_with('[') {
            current = HEADER
                .captures(line)
                .map(|c| (c[2].to_owned(), c[1].to_owned()));
            if let Some(key) = current.as_ref() {
                if result.len() < ROWS {
                    if !result.iter().any(|(existing, _)| existing == key) {
                        result.push((key.clone(), None));
                    }
                }
            }
        }
        if let Some(key) = current.as_ref() {
            if let (Some(value), Some(capture)) = (
                result
                    .iter_mut()
                    .find(|(existing, _)| existing == key)
                    .map(|(_, enabled)| enabled),
                ENABLED.captures(line),
            ) {
                *value = Some(&capture[1] == "true");
            }
        }
    }
    result
}
pub(super) fn scan(home: &Path) -> (Value, Value, Vec<PathBuf>) {
    let root = home.join("plugins/cache");
    let mut budget = Budget::default();
    budget.watch_dir(home);
    budget.watch_dir(&home.join("plugins"));
    let configured = configured(home, &mut budget);
    let mut items = Vec::<((String, String), Value)>::new();
    let (mut providers, state) = dirs(&root, &mut budget, ROWS);
    providers.sort();
    let mut health = match state {
        "available" => "ok",
        "truncated" => "partial",
        other => other,
    };
    for provider in providers {
        let (mut plugins, state) = dirs(&provider, &mut budget, ROWS);
        plugins.sort();
        if state != "available" {
            health = if state == "truncated" {
                "partial"
            } else {
                "partly_unavailable"
            };
        }
        for plugin in plugins {
            if items.len() >= ROWS || budget.bytes >= BYTES {
                health = "partial";
                break;
            }
            let (versions, state) = dirs(&plugin, &mut budget, 64);
            if state != "available" {
                health = if state == "truncated" {
                    "partial"
                } else {
                    "partly_unavailable"
                };
            }
            let latest = versions
                .iter()
                .filter_map(|path| {
                    path.file_name()?
                        .to_str()
                        .filter(|name| VERSION.is_match(name))
                        .map(|name| (name, path))
                })
                .max_by(|(a, _), (b, _)| cmp_version(a, b))
                .map(|(_, path)| path)
                .or_else(|| {
                    versions
                        .iter()
                        .find(|path| path.file_name().is_some_and(|name| name == "latest"))
                });
            let Some(directory) = latest else { continue };
            let Some(provider_name) = provider.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(plugin_name) = plugin.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let key = (provider_name.to_owned(), plugin_name.to_owned());
            let mut item = metadata(home, directory, &mut budget);
            item["id"] = json!(format!("{plugin_name}@{provider_name}"));
            item["name"] = json!(plugin_name);
            item["provider"] = json!(provider_name);
            let config = configured.iter().find(|(existing, _)| existing == &key);
            item["enabled"] = json!(config.and_then(|(_, enabled)| *enabled));
            item["config_present"] = json!(config.is_some());
            item["state"] = json!(if config.is_some() {
                "configured"
            } else {
                "cached"
            });
            let missing = ["skills", "mcp"].iter().any(|field| {
                item[format!("{field}_status")] == "missing"
                    && item[format!("{field}_count")].is_null()
            });
            if item["metadata_status"] == "missing"
                || missing
                || ["metadata_status", "skills_status", "mcp_status"]
                    .iter()
                    .any(|field| {
                        matches!(item[*field].as_str(), Some("unavailable" | "unsupported"))
                    })
            {
                health = "partly_unavailable"
            } else if ["metadata_status", "skills_status", "mcp_status"]
                .iter()
                .any(|field| item[*field] == "truncated")
            {
                health = "partial"
            }
            items.push((key, item));
            budget.rows += 1;
        }
    }
    for ((provider, name), enabled) in configured {
        if !items.iter().any(|((existing_provider, existing_name), _)| {
            existing_provider == &provider && existing_name == &name
        }) {
            let key = (provider.clone(), name.clone());
            let value = json!({"id":format!("{name}@{provider}"),"name":name,"provider":provider,"version":null,"enabled":enabled,"config_present":true,"state":"configured","metadata_status":"missing","manifest_source":null,"skills_count":null,"mcp_count":null,"skills":null,"mcp_servers":null,"components":[],"skills_status":"not_read","mcp_status":"not_read"});
            items.push((key, value));
        }
    }
    if budget.watch_overflow && health == "ok" {
        health = "partial";
    }
    let report = json!({"name":"plugins","location":root.to_string_lossy(),"health":health,"fields":["name","version","provider","enabled","skills_count","mcp_count","skills","mcp_servers","components","metadata_status","manifest_source","skills_status","mcp_status"],"row_limit":ROWS,"manifest_bytes":MANIFEST,"byte_limit":BYTES,"directory_limit":DIRECTORIES,"directories_read":budget.dirs,"bytes_read":budget.bytes,"rows_read":budget.rows,"watch_limit":DIRECTORIES,"watch_count":budget.watch.len(),"watch_truncated":budget.watch_overflow});
    (
        json!({"items":items.into_iter().map(|(_,value)|value).take(ROWS).collect::<Vec<_>>(),"health":health,"row_limit":ROWS}),
        report,
        budget.watch.into_iter().collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_latest_plugin_metadata_without_runtime_arguments() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("config.toml"),"[plugins.\"fixture@provider\"]\nenabled = true\n[plugins.\"absent@provider\"]\nenabled = false\n").unwrap();
        let root = home.path().join("plugins/cache/provider/fixture");
        for version in ["1.2.0", "1.10.0"] {
            fs::create_dir_all(root.join(version).join("skills/demo")).unwrap();
            fs::write(
                root.join(version).join("skills/demo/SKILL.md"),
                "private body",
            )
            .unwrap();
        }
        fs::write(root.join("1.2.0/plugin.json"), r#"{"version":"1.2.0"}"#).unwrap();
        fs::write(root.join("1.10.0/plugin.json"),r#"{"version":"1.10.0","description":"Fixture","skills":"./skills","mcpServers":{"demo":{"command":"PRIVATE_COMMAND","env":{"TOKEN":"PRIVATE_TOKEN"}}}}"#).unwrap();
        let (result, report, _) = scan(&home.path().canonicalize().unwrap());
        let items = result["items"].as_array().unwrap();
        let found = items
            .iter()
            .find(|item| item["id"] == "fixture@provider")
            .unwrap();
        assert_eq!(found["version"], "1.10.0");
        assert_eq!(found["enabled"], true);
        assert_eq!(found["skills_count"], 1);
        assert_eq!(found["mcp_servers"], json!(["demo"]));
        assert!(items
            .iter()
            .any(|item| item["id"] == "absent@provider" && item["enabled"] == false));
        assert!(!result.to_string().contains("PRIVATE"));
        assert_eq!(report["rows_read"], 1);
    }
    #[cfg(unix)]
    #[test]
    fn linked_cache_entries_are_not_followed() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("plugins/cache/provider")).unwrap();
        fs::create_dir_all(outside.path().join("1.0.0")).unwrap();
        symlink(
            outside.path(),
            home.path().join("plugins/cache/provider/linked"),
        )
        .unwrap();
        let (result, report, _) = scan(&home.path().canonicalize().unwrap());
        assert_eq!(report["health"], "ok");
        assert!(result["items"].as_array().unwrap().is_empty());
    }

    #[test]
    fn oversized_manifest_is_reported_without_reading_its_content() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("plugins/cache/provider/fixture/1.0.0");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("plugin.json"), vec![b'x'; MANIFEST + 1]).unwrap();
        let (result, report, _) = scan(&home.path().canonicalize().unwrap());
        assert_eq!(result["items"][0]["metadata_status"], "truncated");
        assert_eq!(report["health"], "partial");
        assert_eq!(report["bytes_read"], MANIFEST + 1);
    }

    #[test]
    fn notification_scope_excludes_unrelated_plugin_files() {
        let home = Path::new("/fixture");
        assert!(relevant(
            home,
            &home.join("plugins/cache/vendor/plugin/1.0/plugin.json")
        ));
        assert!(relevant(
            home,
            &home.join("plugins/cache/vendor/plugin/1.0/.codex-plugin/plugin.json")
        ));
        assert!(relevant(
            home,
            &home.join("plugins/cache/vendor/plugin/1.0/skills/demo/SKILL.md")
        ));
        assert!(!relevant(
            home,
            &home.join("plugins/cache/vendor/plugin/1.0/skills/demo/private.txt")
        ));
        assert!(!relevant(
            home,
            &home.join("plugins/cache/vendor/plugin/1.0/other/file")
        ));
        assert!(!relevant(home, &home.join("plugins/other/file")));
        assert!(!relevant(home, &home.join("plugins/cache/../private")));
    }

    #[test]
    fn watcher_signals_manifest_changes_in_selected_directories() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        };
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let path = home.join("plugins/cache/vendor/plugin/1.0");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("plugin.json"), "{}").unwrap();
        let (_, _, paths) = scan(&home);
        let (wake, receive) = mpsc::sync_channel(1);
        let dirty = Arc::new(AtomicBool::new(false));
        let mut watcher = Watcher::new(&home, wake, Arc::clone(&dirty)).unwrap();
        assert!(watcher.sync(&paths).0);
        while receive.try_recv().is_ok() {}
        dirty.store(false, Ordering::Release);
        fs::write(path.join("plugin.json"), "{\"version\":\"2\"}").unwrap();
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(dirty.load(Ordering::Acquire));
        fs::remove_file(path.join("plugin.json")).unwrap();
        let (healthy, changed) = watcher.sync(&paths);
        assert!(healthy && changed);
        assert!(watcher.healthy());
    }
}

fn relevant(home: &Path, path: &Path) -> bool {
    if path == home {
        return cfg!(target_os = "macos");
    }
    if path == home.join("config.toml") {
        return true;
    }
    let Ok(relative) = path.strip_prefix(home) else {
        return false;
    };
    let Some(parts): Option<Vec<_>> = relative
        .components()
        .map(|part| match part {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect()
    else {
        return false;
    };
    if parts.first() != Some(&"plugins") {
        return false;
    }
    if parts.len() == 1 {
        return true;
    }
    if parts[1] != "cache" {
        return false;
    }
    if parts.len() <= 5 {
        return true;
    }
    let tail = &parts[5..];
    matches!(
        tail,
        ["plugin.json"] | [".mcp.json"] | [".codex-plugin"] | [".codex-plugin", "plugin.json"]
    ) || (tail.first() == Some(&"skills")
        && (tail.len() <= 2 || tail.len() == 3 && tail[2] == "SKILL.md"))
}

pub(super) fn watch_roots(home: &Path) -> Vec<PathBuf> {
    [
        home.to_path_buf(),
        home.join("plugins"),
        home.join("plugins/cache"),
    ]
    .into_iter()
    .filter(|path| {
        safe(path).is_ok() && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
    })
    .collect()
}

pub(super) struct Watcher {
    watcher: notify::RecommendedWatcher,
    registered: BTreeMap<PathBuf, (u64, u64)>,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Watcher {
    pub fn new(
        home: &Path,
        wake: std::sync::mpsc::SyncSender<()>,
        dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> io::Result<Self> {
        use notify::{Config, Event, Watcher as _};
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let failed = Arc::new(AtomicBool::new(false));
        let errors = Arc::clone(&failed);
        let home = home.to_path_buf();
        let watcher = notify::RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                let notify = match result {
                    Ok(event) => {
                        event.need_rescan()
                            || (!event.kind.is_access()
                                && event.paths.iter().any(|path| relevant(&home, path)))
                    }
                    Err(_) => {
                        errors.store(true, Ordering::Release);
                        true
                    }
                };
                if notify {
                    dirty.store(true, Ordering::Release);
                    let _ = wake.try_send(());
                }
            },
            Config::default().with_follow_symlinks(false),
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            watcher,
            registered: BTreeMap::new(),
            failed,
        })
    }
    pub fn healthy(&self) -> bool {
        !self.failed.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn sync(&mut self, paths: &[PathBuf]) -> (bool, bool) {
        use notify::{RecursiveMode, Watcher as _};
        let mut desired = BTreeMap::new();
        let mut healthy = self.healthy();
        let mut changed = false;
        for path in paths.iter().take(DIRECTORIES) {
            match fs::symlink_metadata(path) {
                Ok(metadata)
                    if (metadata.is_dir() || metadata.is_file())
                        && !super::redirected(&metadata) =>
                {
                    match safe(path) {
                        Ok(()) => {
                            desired.insert(path.clone(), super::file_identity(&metadata));
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => changed = true,
                        Err(_) => healthy = false,
                    }
                }
                Ok(_) => changed = true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => changed = true,
                Err(_) => healthy = false,
            }
        }
        for (path, identity) in &desired {
            if self.registered.get(path) == Some(identity) {
                continue;
            }
            if self.registered.contains_key(path) {
                let _ = self.watcher.unwatch(path);
                self.registered.remove(path);
            }
            if self
                .watcher
                .watch(path, RecursiveMode::NonRecursive)
                .is_err()
            {
                if fs::symlink_metadata(path)
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
                {
                    changed = true;
                } else {
                    healthy = false;
                }
                continue;
            }
            if fs::symlink_metadata(path).is_ok_and(|metadata| {
                (metadata.is_dir() || metadata.is_file())
                    && !super::redirected(&metadata)
                    && safe(path).is_ok()
                    && super::file_identity(&metadata) == *identity
            }) {
                self.registered.insert(path.clone(), *identity);
                changed = true;
            } else {
                let _ = self.watcher.unwatch(path);
                changed = true;
            }
        }
        let stale: Vec<_> = self
            .registered
            .keys()
            .filter(|path| !desired.contains_key(*path))
            .cloned()
            .collect();
        for path in stale {
            let _ = self.watcher.unwatch(&path);
            self.registered.remove(&path);
            changed = true;
        }
        if !healthy {
            self.failed
                .store(true, std::sync::atomic::Ordering::Release);
        }
        (healthy, changed)
    }
}
