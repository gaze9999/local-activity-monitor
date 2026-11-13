//! Read bounded Jev telemetry from the locally configured database.
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc, RwLock,
};
use std::time::Duration as StdDuration;

pub(super) struct SourceWatcher {
    watcher: RecommendedWatcher,
    monitoring: PathBuf,
    config: PathBuf,
    database: Arc<RwLock<Option<PathBuf>>>,
    watched_dir: Arc<RwLock<Option<PathBuf>>>,
    failed: Arc<AtomicBool>,
    healthy: bool,
    directory: Option<PathBuf>,
    files: Vec<PathBuf>,
}

impl SourceWatcher {
    pub fn new(home: &Path, wake: SyncSender<()>, dirty: Arc<AtomicBool>) -> notify::Result<Self> {
        let monitoring = home
            .join("monitoring")
            .canonicalize()
            .unwrap_or_else(|_| home.join("monitoring"));
        let config = monitoring.join("jev-monitor.json");
        let database = Arc::new(RwLock::new(None::<PathBuf>));
        let watched_dir = Arc::new(RwLock::new(None::<PathBuf>));
        let failed = Arc::new(AtomicBool::new(false));
        let watched_db = Arc::clone(&database);
        let watched_parent = Arc::clone(&watched_dir);
        let callback_failed = Arc::clone(&failed);
        let monitored_dir = monitoring.clone();
        let watched_config = config.clone();
        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                let related = match result {
                    Ok(event) if !event.kind.is_access() => {
                        let current = watched_db.read().unwrap_or_else(|error| error.into_inner());
                        event.need_rescan()
                            || event.paths.iter().any(|path| {
                                path == &watched_config
                                    || (cfg!(target_os = "macos") && path == &monitored_dir)
                                    || current.as_ref().is_some_and(|database| {
                                        path == database
                                            || path == &wal_path(database)
                                            || database.starts_with(path)
                                            || (cfg!(target_os = "macos")
                                                && watched_parent
                                                    .read()
                                                    .unwrap_or_else(|error| error.into_inner())
                                                    .as_ref()
                                                    == Some(path))
                                    })
                            })
                    }
                    Err(_) => {
                        callback_failed.store(true, Ordering::Release);
                        true
                    }
                    _ => false,
                };
                if related {
                    dirty.store(true, Ordering::Release);
                    let _ = wake.try_send(());
                }
            },
            Config::default().with_follow_symlinks(false),
        )?;
        watcher.watch(&monitoring, RecursiveMode::NonRecursive)?;
        let mut result = Self {
            watcher,
            monitoring,
            config,
            database,
            watched_dir,
            failed,
            healthy: true,
            directory: None,
            files: Vec::new(),
        };
        result.refresh(home, true);
        Ok(result)
    }

    pub fn healthy(&self) -> bool {
        self.healthy && !self.failed.load(Ordering::Acquire)
    }

    pub fn refresh(&mut self, home: &Path, enabled: bool) {
        let metadata = snapshot(home, "all", super::project::now(), false);
        let database = metadata["reader"]["locations"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .filter(|_| enabled && metadata["enabled"] == true)
            .map(PathBuf::from)
            .map(|path| {
                path.parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .and_then(|parent| path.file_name().map(|name| parent.join(name)))
                    .unwrap_or(path)
            });
        let mut directory = database
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf);
        while directory.as_ref().is_some_and(|path| !path.is_dir()) {
            directory = directory.and_then(|path| {
                path.parent()
                    .filter(|parent| *parent != path)
                    .map(Path::to_path_buf)
            });
        }
        self.healthy = database.is_none() || directory.is_some();
        if directory != self.directory {
            if let Some(old) = self.directory.take().filter(|old| old != &self.monitoring) {
                let _ = self.watcher.unwatch(&old);
            }
            if let Some(new) = directory.as_ref().filter(|new| *new != &self.monitoring) {
                if self.watcher.watch(new, RecursiveMode::NonRecursive).is_ok() {
                    self.directory = Some(new.clone());
                } else {
                    self.healthy = false;
                }
            } else {
                self.directory = directory;
            }
        }
        *self
            .watched_dir
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.directory.clone();
        let mut wanted = vec![self.config.clone()];
        if let Some(path) = &database {
            wanted.push(path.clone());
            wanted.push(wal_path(path));
        }
        wanted.retain(|path| path.is_file());
        for old in self.files.iter().filter(|old| !wanted.contains(old)) {
            let _ = self.watcher.unwatch(old);
        }
        self.files.retain(|old| wanted.contains(old));
        for new in wanted {
            if !self.files.contains(&new) {
                if self
                    .watcher
                    .watch(&new, RecursiveMode::NonRecursive)
                    .is_ok()
                {
                    self.files.push(new);
                } else {
                    self.healthy = false;
                }
            }
        }
        *self
            .database
            .write()
            .unwrap_or_else(|error| error.into_inner()) = database;
    }
}

fn wal_path(database: &Path) -> PathBuf {
    let mut value = database.as_os_str().to_owned();
    value.push("-wal");
    PathBuf::from(value)
}

#[derive(Debug)]
pub(crate) enum ConfigureError {
    Invalid,
    Io,
}

fn default_database() -> Option<PathBuf> {
    let home = crate::platform_home()?;
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join("AppData/Local"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    Some(base.join("local-activity-monitor/state/jev.sqlite3"))
}

fn read_config(path: &Path) -> Result<Option<Vec<u8>>, ConfigureError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !super::redirected(&meta) && meta.len() <= 8192 => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) => return Err(ConfigureError::Invalid),
        Err(_) => return Err(ConfigureError::Io),
    };
    let mut file = fs::File::open(path).map_err(|_| ConfigureError::Io)?;
    let actual = file.metadata().map_err(|_| ConfigureError::Io)?;
    if super::file_identity(&metadata) != super::file_identity(&actual) {
        return Err(ConfigureError::Invalid);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigureError::Io)?;
    if bytes.len() > 8192 {
        return Err(ConfigureError::Invalid);
    }
    Ok(Some(bytes))
}

pub(crate) fn configure(
    monitoring: &Path,
    enabled: bool,
    override_database: Option<&Path>,
) -> Result<(bool, Option<String>), ConfigureError> {
    let path = monitoring.join("jev-monitor.json");
    let before = read_config(&path)?;
    let old: Option<Value> = before
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes).map_err(|_| ConfigureError::Invalid))
        .transpose()?;
    let (old_database, old_since) = if let Some(old) = &old {
        let raw = old["database"].as_str().ok_or(ConfigureError::Invalid)?;
        let database = if let Some(rest) = raw.strip_prefix("~/") {
            crate::platform_home()
                .ok_or(ConfigureError::Invalid)?
                .join(rest)
        } else {
            PathBuf::from(raw)
        };
        if !database.is_absolute() {
            return Err(ConfigureError::Invalid);
        }
        if old["version"] != 1 || !old["enabled"].is_boolean() {
            return Err(ConfigureError::Invalid);
        }
        (
            Some(database),
            old["enabled_at"]
                .as_str()
                .and_then(lam_history_store::parse_utc_stamp)
                .map(super::project::stamp),
        )
    } else {
        (None, None)
    };
    let requested = override_database.map(|path| {
        let raw = path.to_string_lossy();
        if let Some(rest) = raw.strip_prefix("~/") {
            crate::platform_home()
                .map(|home| home.join(rest))
                .unwrap_or_default()
        } else {
            path.to_path_buf()
        }
    });
    let database = requested
        .or_else(|| old_database.clone())
        .or_else(default_database)
        .ok_or(ConfigureError::Invalid)?;
    if !database.is_absolute() {
        return Err(ConfigureError::Invalid);
    }
    let since = if old_database.as_ref() == Some(&database) {
        old_since
    } else {
        None
    }
    .unwrap_or_else(|| super::project::stamp(super::project::now()));
    let value = json!({"version":1,"enabled":enabled,"database":database.to_string_lossy(),"enabled_at":since});
    if old.as_ref() == Some(&value) {
        return Ok((false, Some(since)));
    }
    if !monitoring.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(monitoring).map_err(|_| ConfigureError::Io)?;
    }
    let directory = fs::symlink_metadata(monitoring).map_err(|_| ConfigureError::Io)?;
    if !directory.is_dir() || super::redirected(&directory) {
        return Err(ConfigureError::Invalid);
    }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = format!(
        "{}-{}-{}",
        std::process::id(),
        super::project::now()
            .timestamp_nanos_opt()
            .unwrap_or_default(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    let open_new = |path: &Path| -> std::io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    };
    if let Some(bytes) = &before {
        let mut backup = open_new(&monitoring.join(format!("jev-monitor.json.{nonce}.bak")))
            .map_err(|_| ConfigureError::Io)?;
        backup
            .write_all(bytes)
            .and_then(|_| backup.sync_all())
            .map_err(|_| ConfigureError::Io)?;
    }
    let temporary = monitoring.join(format!(".monitor-{nonce}"));
    let result = (|| {
        let mut file = open_new(&temporary).map_err(|_| ConfigureError::Io)?;
        let mut output = serde_json::to_vec_pretty(&value).map_err(|_| ConfigureError::Invalid)?;
        output.push(b'\n');
        file.write_all(&output)
            .and_then(|_| file.sync_all())
            .map_err(|_| ConfigureError::Io)?;
        if read_config(&path)? != before {
            return Err(ConfigureError::Invalid);
        }
        fs::rename(&temporary, &path).map_err(|_| ConfigureError::Io)?;
        Ok((true, Some(since)))
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn name(value: Option<&Value>) -> Option<&str> {
    value?.as_str().filter(|text| {
        !text.is_empty()
            && text.len() <= 160
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    })
}
fn integer(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|value| *value >= 0)
}

pub(super) fn error_events(snapshot: &Value) -> Vec<Value> {
    let mut errors = Vec::new();
    for event in snapshot["recent"].as_array().into_iter().flatten() {
        for (index, attempt) in event["attempts"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(code) = attempt["status"].as_str().filter(|status| {
                [
                    "error",
                    "failed",
                    "timeout",
                    "missing_dependencies",
                    "network_unavailable",
                    "http_error",
                    "invalid_response",
                    "response_too_large",
                ]
                .contains(status)
            }) else {
                continue;
            };
            errors.push(json!({"timestamp":event["timestamp"],"category":"mcp","source":"jev_telemetry","severity":"error","server":"jev","tool":event["operation"],"code":code,"http_status":attempt["http_status"],"attempt":index+1}));
        }
    }
    errors
}

fn details(
    db: &Connection,
    cutoff: &str,
    summary: &mut Value,
) -> rusqlite::Result<(Vec<Value>, Vec<Value>)> {
    let mut statuses = serde_json::Map::new();
    let mut query = db.prepare(
        "SELECT status,count(*) FROM jev_events WHERE timestamp>=? GROUP BY status LIMIT 40",
    )?;
    for row in query.query_map([cutoff], |row| {
        Ok((
            row.get_ref(0)?.as_str().ok().map(str::to_owned),
            row.get::<_, i64>(1)?,
        ))
    })? {
        let (Some(status), count) = row? else {
            continue;
        };
        if name(Some(&json!(status))).is_some() {
            statuses.insert(status, json!(count));
        }
    }
    summary["statuses"] = json!(statuses);
    let mut query = db.prepare("SELECT substr(timestamp,1,13),count(*) FROM jev_events WHERE timestamp>=? GROUP BY 1 ORDER BY 1 DESC LIMIT 168")?;
    let mut series = Vec::new();
    for row in query.query_map([cutoff], |row| {
        Ok((
            row.get_ref(0)?.as_str().ok().map(str::to_owned),
            row.get::<_, i64>(1)?,
        ))
    })? {
        let (Some(time), calls) = row? else { continue };
        if time.len() == 13
            && chrono::NaiveDateTime::parse_from_str(&format!("{time}:00:00"), "%Y-%m-%dT%H:%M:%S")
                .is_ok()
        {
            series.push(json!({"time":format!("{time}:00:00Z"),"calls":calls}));
        }
    }
    series.reverse();
    let mut query = db.prepare("SELECT CASE WHEN length(CAST(metadata AS BLOB))<=65536 THEN metadata END FROM jev_events WHERE timestamp>=? ORDER BY timestamp DESC LIMIT 80")?;
    let mut recent = Vec::new();
    for row in query.query_map([cutoff], |row| row.get::<_, Option<String>>(0))? {
        let Some(raw) = row? else { continue };
        let Ok(event) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let (Some(status), Some(operation)) =
            (name(event.get("status")), name(event.get("operation")))
        else {
            continue;
        };
        let attempts = event["attempts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let shown: Vec<_> = attempts.iter().take(16).filter(|value| value.is_object()).map(|value| json!({"status":name(value.get("status")).unwrap_or("unknown"),"http_status":integer(value.get("http_status")),"latency_ms":integer(value.get("latency_ms"))})).collect();
        let mut item = json!({"timestamp":event["timestamp"].as_str().and_then(lam_history_store::parse_utc_stamp).map(super::project::stamp),"operation":operation,"status":status,"source":name(event.get("source")),"model":name(event.get("resolved_model")).or_else(||name(event.get("requested_model"))),"input_tokens":integer(event.get("input_tokens")),"output_tokens":integer(event.get("output_tokens")),"latency_ms":integer(event.get("latency_ms")),"request_bytes":integer(event.get("request_bytes")),"response_bytes":integer(event.get("response_bytes")),"http_attempts":attempts.len(),"attempts":shown});
        let projected = lam_history_store::mcp::project(
            "jev",
            &serde_json::Map::from_iter([("result".into(), event.clone())]),
        );
        if let Some(values) = projected.get("result").and_then(Value::as_object) {
            for (key, value) in values {
                if value.is_number() || value.is_boolean() {
                    item[key] = value.clone();
                }
            }
        }
        recent.push(item);
    }
    Ok((recent, series))
}

pub(crate) fn snapshot(home: &Path, window: &str, reference: DateTime<Utc>, active: bool) -> Value {
    let config_path = home.join("monitoring/jev-monitor.json");
    if !active {
        let config = fs::File::open(&config_path).ok().and_then(|file| {
            let mut data = Vec::new();
            file.take(8193).read_to_end(&mut data).ok()?;
            (data.len() <= 8192)
                .then(|| serde_json::from_slice::<Value>(&data).ok())
                .flatten()
        });
        let valid = config
            .as_ref()
            .filter(|value| value["version"] == 1 && value["enabled"].is_boolean())
            .and_then(|value| {
                let path = value["database"].as_str()?;
                let path = if let Some(rest) = path.strip_prefix("~/") {
                    PathBuf::from(std::env::var_os("HOME")?).join(rest)
                } else {
                    PathBuf::from(path)
                };
                path.is_absolute().then_some((value, path))
            });
        return json!({"source":"jev","enabled":valid.as_ref().and_then(|(value,_)|value["enabled"].as_bool()).unwrap_or(false),"enabled_at":valid.as_ref().and_then(|(value,_)|value["enabled_at"].as_str()).and_then(lam_history_store::parse_utc_stamp).map(super::project::stamp),"health":"paused","scope":"completed_operations_in_local_database","summary":{},"recent":[],"series":[],"reader":{"locations":valid.as_ref().map(|(_,path)|vec![path.to_string_lossy().to_string()]).unwrap_or_default()}});
    }
    let mut result = json!({"source":"jev","enabled":false,"enabled_at":null,"health":"disabled","scope":"completed_operations_in_local_database","window":window,"summary":{},"recent":[],"series":[],"reader":{"locations":[],"checked_at":super::project::stamp(reference),"config_location":config_path.to_string_lossy(),"record_limit":80,"series_limit":168}});
    let data = match fs::File::open(&config_path).and_then(|file| {
        let mut data = Vec::new();
        file.take(8193).read_to_end(&mut data)?;
        Ok(data)
    }) {
        Ok(data) if data.len() <= 8192 => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        Err(_) => {
            result["health"] = json!("unavailable");
            result["reader"]["error_type"] = json!("OSError");
            return result;
        }
        _ => {
            result["health"] = json!("invalid_config");
            return result;
        }
    };
    let config: Value = match serde_json::from_slice(&data) {
        Ok(value) => value,
        Err(_) => {
            result["health"] = json!("invalid_config");
            return result;
        }
    };
    let (Some(enabled), Some(database)) =
        (config["enabled"].as_bool(), config["database"].as_str())
    else {
        result["health"] = json!("invalid_config");
        return result;
    };
    let database = if let Some(rest) = database.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::new(),
        }
    } else {
        PathBuf::from(database)
    };
    if config["version"] != 1 || !database.is_absolute() {
        result["health"] = json!("invalid_config");
        return result;
    }
    result["enabled"] = json!(enabled);
    result["enabled_at"] = json!(config["enabled_at"]
        .as_str()
        .and_then(lam_history_store::parse_utc_stamp)
        .map(super::project::stamp));
    result["reader"]["locations"] = json!([database.to_string_lossy()]);
    if !enabled {
        return result;
    }
    result["health"] = json!("not_recorded");
    let before = match fs::symlink_metadata(&database) {
        Ok(meta) if meta.is_file() && !super::redirected(&meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        _ => {
            result["health"] = json!("unavailable");
            return result;
        }
    };
    let actual_path = match database.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            result["health"] = json!("unavailable");
            return result;
        }
    };
    let opened = Connection::open_with_flags(
        &actual_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|db| {
        db.busy_timeout(StdDuration::from_millis(80))?;
        db.pragma_update(None, "query_only", true)?;
        db.pragma_update(None, "trusted_schema", false)?;
        Ok(db)
    });
    let db = match opened {
        Ok(db) => db,
        Err(_) => {
            result["health"] = json!("unavailable");
            result["reader"]["error_type"] = json!("OperationalError");
            return result;
        }
    };
    if fs::symlink_metadata(&database).ok().is_none_or(|after| {
        super::redirected(&after) || super::file_identity(&after) != super::file_identity(&before)
    }) {
        result["health"] = json!("unavailable");
        return result;
    }
    let cutoff = match window {
        "1h" => Some(1),
        "24h" => Some(24),
        "7d" => Some(168),
        _ => None,
    }
    .map(|hours| (reference - Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Millis, true))
    .unwrap_or_default();
    let query = "SELECT count(*), min(timestamp), sum(http_attempts), sum(max(http_attempts-1,0)), sum(input_tokens), sum(output_tokens), count(input_tokens), count(output_tokens), sum(request_bytes), sum(response_bytes), sum(response_unknown_attempts), avg(latency_ms) FROM jev_events WHERE timestamp>=?";
    let summary = db.query_row(query, [&cutoff], |row| {
        let count:i64 = row.get(0)?; let since:Option<String> = row.get(1)?; let latency:Option<f64> = row.get(11)?;
        Ok(json!({"calls":count,"since":since.as_deref().and_then(lam_history_store::parse_utc_stamp).map(super::project::stamp),"http_attempts":row.get::<_,Option<i64>>(2)?.unwrap_or(0),"retries":row.get::<_,Option<i64>>(3)?.unwrap_or(0),"input_tokens":row.get::<_,Option<i64>>(4)?,"output_tokens":row.get::<_,Option<i64>>(5)?,"input_known_calls":row.get::<_,i64>(6)?,"output_known_calls":row.get::<_,i64>(7)?,"input_unknown_calls":count-row.get::<_,i64>(6)?,"output_unknown_calls":count-row.get::<_,i64>(7)?,"request_body_bytes":row.get::<_,Option<i64>>(8)?.unwrap_or(0),"known_response_bytes":row.get::<_,Option<i64>>(9)?.unwrap_or(0),"response_unknown_attempts":row.get::<_,Option<i64>>(10)?.unwrap_or(0),"average_latency_ms":latency.map(|value|value.round() as i64)}))
    });
    let read = summary.and_then(|mut summary| {
        details(&db, &cutoff, &mut summary).map(|(recent, series)| (summary, recent, series))
    });
    match read {
        Ok((summary, recent, series)) => {
            result["health"] = json!("ok");
            result["summary"] = summary;
            result["recent"] = json!(recent);
            result["series"] = json!(series);
        }
        Err(error) => {
            let message = error.to_string();
            result["health"] = json!(if message.starts_with("no such table:")
                || message.starts_with("no such column:")
            {
                "unsupported"
            } else {
                "unavailable"
            });
            result["reader"]["error_type"] = json!("OperationalError");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watcher_reports_creation_and_runtime_failure() {
        let home = tempfile::tempdir().unwrap();
        let (wake, _signal) = std::sync::mpsc::sync_channel(1);
        let dirty = Arc::new(AtomicBool::new(false));
        assert!(SourceWatcher::new(home.path(), wake.clone(), Arc::clone(&dirty)).is_err());
        fs::create_dir(home.path().join("monitoring")).unwrap();
        let watcher = SourceWatcher::new(home.path(), wake, dirty).unwrap();
        assert!(watcher.healthy());
        watcher.failed.store(true, Ordering::Release);
        assert!(!watcher.healthy());
    }

    #[test]
    fn readonly_summary_and_recent_follow_window_and_omit_private_fields() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("monitoring")).unwrap();
        let database = home.path().join("jev.sqlite");
        let db = Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
        let time = "2026-11-13T09:00:00.000Z";
        let metadata = json!({"timestamp":time,"status":"ok","operation":"rank","prompt":"PRIVATE","attempts":[{"status":"ok","http_status":200,"body":"PRIVATE"}],"input_tokens":12}).to_string();
        db.execute(
            "INSERT INTO jev_events VALUES(?,'ok',3,12,NULL,25,30,1,9,?)",
            rusqlite::params![time, metadata],
        )
        .unwrap();
        fs::write(
            home.path().join("monitoring/jev-monitor.json"),
            json!({"version":1,"enabled":true,"database":database}).to_string(),
        )
        .unwrap();
        let reference = lam_history_store::parse_utc_stamp("2026-11-13T11:00:00Z").unwrap();
        let all = snapshot(home.path(), "all", reference, true);
        assert_eq!(all["health"], "ok");
        assert_eq!(all["summary"]["calls"], 1);
        assert_eq!(all["summary"]["retries"], 2);
        assert_eq!(all["summary"]["output_unknown_calls"], 1);
        assert_eq!(all["recent"][0]["attempts"][0]["http_status"], 200);
        assert!(!all.to_string().contains("PRIVATE"));
        assert_eq!(
            snapshot(home.path(), "1h", reference, true)["summary"]["calls"],
            0
        );
    }

    #[test]
    fn invalid_disabled_missing_and_unsupported_sources_are_distinct() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("monitoring")).unwrap();
        let path = home.path().join("monitoring/jev-monitor.json");
        let now = lam_history_store::parse_utc_stamp("2026-11-13T11:00:00Z").unwrap();
        assert_eq!(
            snapshot(home.path(), "all", now, true)["health"],
            "disabled"
        );
        fs::write(&path, "invalid").unwrap();
        assert_eq!(
            snapshot(home.path(), "all", now, true)["health"],
            "invalid_config"
        );
        let db = home.path().join("missing.sqlite");
        fs::write(
            &path,
            json!({"version":1,"enabled":true,"database":db}).to_string(),
        )
        .unwrap();
        assert_eq!(
            snapshot(home.path(), "all", now, true)["health"],
            "not_recorded"
        );
        drop(Connection::open(&db).unwrap());
        assert_eq!(
            snapshot(home.path(), "all", now, true)["health"],
            "unsupported"
        );
        fs::write(
            &path,
            json!({"version":1,"enabled":false,"database":db}).to_string(),
        )
        .unwrap();
        assert_eq!(
            snapshot(home.path(), "all", now, true)["health"],
            "disabled"
        );
    }
}
