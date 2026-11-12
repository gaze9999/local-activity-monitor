use chrono::{DateTime, Datelike, Duration, Utc};
use lam_session_core::project_diagnostic;
use regex::Regex;
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension, Row};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration as StdDuration, SystemTime};

use super::{file_identity, model_api, project::stamp, redirected};

const PAGE: usize = 2000;
const LOG_LIMIT: usize = 1000;
const SQL_LIMIT: usize = 500;
const HOME_LIMIT: usize = 10_000;
const SIDECARS: [&str; 2] = ["-wal", "-journal"];

static LOG_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^logs_.*\.sqlite$").expect("log filename"));
static NONPRINTABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{C}\p{Z}]$").expect("nonprintable character"));
static LEVEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z]{1,16}$").expect("log level"));
static MODULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_./:-]{1,160}$").expect("log module"));
static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,160}$").expect("diagnostic identifier"));
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$").expect("thread id")
});
static PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b([a-zA-Z_][a-zA-Z0-9_]*)[=:]\s*("(?:\\.|[^"\\])*"|\S+)"#)
        .expect("diagnostic pair")
});
static DESKTOP_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b([a-zA-Z_][a-zA-Z0-9_]*)=("(?:\\.|[^"\\])*"|[^\s]+)"#)
        .expect("desktop diagnostic pair")
});
static DESKTOP_IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9_./:-]{1,160}$").expect("desktop diagnostic identifier")
});
static DESKTOP_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s[a-zA-Z_][a-zA-Z0-9_]*=").expect("desktop diagnostic heading"));
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s[a-zA-Z_][a-zA-Z0-9_]*[=:]").expect("diagnostic heading"));
static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(?:\\.|[^"\\])*""#).expect("quoted text"));
static HTTP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bHTTP(?:[/ ][0-9]\.[0-9])?[ :]+([0-9]{3})\b").expect("HTTP status")
});
static CAUSES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("rate_limit", r"rate.?limit|too many requests"),
        ("quota_exceeded", r"quota.{0,12}exceeded|insufficient_quota"),
        (
            "authentication_failed",
            r"unauthorized|authentication.{0,12}fail|invalid.{0,8}api.?key",
        ),
        ("permission_denied", r"permission denied|forbidden"),
        ("connection_refused", r"connection refused|ECONNREFUSED"),
        ("timeout", r"timed? out|timeout|ETIMEDOUT"),
        (
            "stream_interrupted",
            r"stream disconnected|stream interrupted",
        ),
        (
            "invalid_response",
            r"invalid response|invalid JSON|parse error",
        ),
    ]
    .into_iter()
    .map(|(name, pattern)| {
        (
            name,
            Regex::new(&format!("(?i){pattern}")).expect("diagnostic cause"),
        )
    })
    .collect()
});

#[derive(Clone, PartialEq, Eq)]
pub struct Source {
    path: PathBuf,
    identity: (u64, u64),
}

pub fn related_event(home: &Path, path: &Path) -> bool {
    if path.parent() != Some(home) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    LOG_NAME.is_match(name)
        || SIDECARS.iter().any(|suffix| {
            name.strip_suffix(suffix)
                .is_some_and(|name| LOG_NAME.is_match(name))
        })
}

pub fn discover(home: &Path) -> io::Result<Option<Source>> {
    let mut newest = None::<(Option<SystemTime>, Source)>;
    let mut entries = fs::read_dir(home)?;
    let mut unsafe_path = false;
    for entry in entries.by_ref().take(HOME_LIMIT) {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| LOG_NAME.is_match(name))
        {
            continue;
        }
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            result => result?,
        };
        if redirected(&meta) || !meta.is_file() {
            unsafe_path = true;
            continue;
        }
        let source = Source {
            path,
            identity: file_identity(&meta),
        };
        let modified = meta.modified().ok();
        if newest
            .as_ref()
            .is_none_or(|(previous, file)| (modified, &source.path) > (*previous, &file.path))
        {
            newest = Some((modified, source));
        }
    }
    if entries.next().is_some() || newest.is_none() && unsafe_path {
        return Err(io::Error::other("Core log discovery unavailable"));
    }
    Ok(newest.map(|(_, source)| source))
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

impl Source {
    pub fn watches(&self) -> BTreeMap<PathBuf, (u64, u64)> {
        std::iter::once(self.path.clone())
            .chain(SIDECARS.iter().map(|suffix| sidecar(&self.path, suffix)))
            .filter_map(|path| {
                let meta = fs::symlink_metadata(&path).ok()?;
                (meta.is_file() && !redirected(&meta)).then(|| (path, file_identity(&meta)))
            })
            .collect()
    }

    fn open(&self) -> Result<Connection, ReadError> {
        for path in std::iter::once(self.path.clone()).chain(
            ["-wal", "-journal", "-shm"]
                .iter()
                .map(|suffix| sidecar(&self.path, suffix)),
        ) {
            match fs::symlink_metadata(path) {
                Ok(meta) if !redirected(&meta) && meta.is_file() => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(ReadError::Io),
            }
        }
        let before = fs::symlink_metadata(&self.path).map_err(|_| ReadError::Io)?;
        if redirected(&before) || file_identity(&before) != self.identity {
            return Err(ReadError::Io);
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = Connection::open_with_flags(&self.path, flags)?;
        db.busy_timeout(StdDuration::from_millis(80))?;
        db.pragma_update(None, "query_only", true)?;
        db.pragma_update(None, "trusted_schema", false)?;
        let after = fs::symlink_metadata(&self.path).map_err(|_| ReadError::Io)?;
        if redirected(&after) || file_identity(&after) != self.identity {
            return Err(ReadError::Io);
        }
        Ok(db)
    }
}

enum ReadError {
    Io,
    Sql,
    Unsupported,
}

impl From<rusqlite::Error> for ReadError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sql
    }
}

pub struct Core {
    source: Option<Source>,
    cursor: Option<i64>,
    history: Option<i64>,
    backfill: bool,
    health: &'static str,
    error_type: Option<&'static str>,
    checked_at: Option<String>,
    read_lines: u64,
    parsed: u64,
    unsupported: u64,
    pub trimmed: u64,
    pub logs: VecDeque<Value>,
    pub errors: VecDeque<Value>,
    pub api_events: VecDeque<Value>,
    pub sql: VecDeque<Value>,
    pub pending_sql: Vec<Value>,
    pub pending_errors: Vec<Value>,
    pub storage_health: &'static str,
    pub storage_error: Option<&'static str>,
}

impl Core {
    pub fn new(enabled: bool) -> Self {
        Self {
            source: None,
            cursor: None,
            history: None,
            backfill: false,
            health: if enabled { "waiting" } else { "disabled" },
            error_type: None,
            checked_at: None,
            read_lines: 0,
            parsed: 0,
            unsupported: 0,
            trimmed: 0,
            logs: VecDeque::new(),
            errors: VecDeque::new(),
            api_events: VecDeque::new(),
            sql: VecDeque::new(),
            pending_sql: Vec::new(),
            pending_errors: Vec::new(),
            storage_health: if enabled { "waiting" } else { "disabled" },
            storage_error: None,
        }
    }

    pub fn backfill(&self) -> bool {
        self.health == "ok" && self.backfill && !self.has_pending()
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_sql.is_empty() || !self.pending_errors.is_empty()
    }

    pub fn reset_log_scan(&mut self) -> bool {
        if self.has_pending() {
            return false;
        }
        self.source = None;
        self.cursor = None;
        self.history = None;
        self.backfill = false;
        true
    }

    pub fn health(&self) -> &'static str {
        self.health
    }

    pub fn source(&self) -> Value {
        let files: Vec<_> = self
            .source
            .iter()
            .filter_map(|source| {
                Some(json!({"name":source.path.file_name()?.to_str()?,"record_id":self.cursor}))
            })
            .collect();
        json!({"source":"codex_core","health":self.health,"checked_at":self.checked_at,
               "read_bytes":null,"read_lines":self.read_lines,"parsed_lines":self.parsed,
               "unsupported_lines":self.unsupported,"oversized_lines":0,"error_type":self.error_type,
               "backfill_pending":self.backfill,"files":files,"read_limit":PAGE*2,"read_unit":"rows",
               "history_hours":24,"storage_health":self.storage_health})
    }

    pub fn refresh(&mut self, source: io::Result<Option<Source>>, now: DateTime<Utc>) -> bool {
        let source = match source {
            Ok(Some(source)) => source,
            Ok(None) => {
                let changed = self.source.take().is_some();
                self.cursor = None;
                self.history = None;
                self.backfill = false;
                return self.set_health("missing", None, now) || changed;
            }
            Err(_) => return self.set_health("unavailable", Some("OSError"), now),
        };
        let replaced = self.source.as_ref() != Some(&source);
        if replaced {
            self.source = Some(source.clone());
            self.cursor = None;
            self.history = None;
            self.backfill = false;
        }
        let batch = match self.read(&source, now) {
            Ok(batch) => batch,
            Err(ReadError::Unsupported) => {
                self.cursor = None;
                self.history = None;
                self.backfill = false;
                return self.set_health("unsupported", None, now) || replaced;
            }
            Err(ReadError::Io) => {
                return self.set_health("unavailable", Some("OSError"), now) || replaced
            }
            Err(ReadError::Sql) => {
                return self.set_health("unavailable", Some("SQLiteError"), now) || replaced
            }
        };
        let changed = replaced
            || batch.read != 0
            || self.cursor != batch.cursor
            || self.backfill != batch.backfill;
        self.cursor = batch.cursor;
        self.history = batch.history;
        self.backfill = batch.backfill;
        self.read_lines += batch.read;
        self.parsed += batch.parsed;
        self.unsupported += batch.unsupported;
        for value in batch.logs {
            if self.logs.len() == LOG_LIMIT {
                self.trimmed += 1;
            }
            push(&mut self.logs, value, LOG_LIMIT);
        }
        for value in batch.errors {
            self.pending_errors.push(value.clone());
            push(&mut self.errors, value, LOG_LIMIT);
        }
        for value in batch.api_events {
            if !self
                .api_events
                .iter()
                .any(|event| event["observation_id"] == value["observation_id"])
            {
                self.api_events.push_back(value);
                if self.api_events.len() > LOG_LIMIT {
                    if let Some((index, _)) =
                        self.api_events.iter().enumerate().min_by_key(|(_, event)| {
                            event["timestamp"].as_str().unwrap_or_default()
                        })
                    {
                        self.api_events.remove(index);
                    }
                }
            }
        }
        for value in batch.sql {
            push(&mut self.sql, value.clone(), SQL_LIMIT);
            self.pending_sql.push(value);
        }
        if changed {
            self.checked_at = Some(stamp(now));
        }
        self.set_health("ok", None, now) || changed
    }

    fn set_health(
        &mut self,
        health: &'static str,
        error: Option<&'static str>,
        now: DateTime<Utc>,
    ) -> bool {
        if self.health == health && self.error_type == error {
            return false;
        }
        self.health = health;
        self.error_type = error;
        self.checked_at = Some(stamp(now));
        true
    }

    fn read(&self, source: &Source, now: DateTime<Utc>) -> Result<Batch, ReadError> {
        let mut db = source.open()?;
        let tx = db.transaction()?;
        let mut statement = tx.prepare("PRAGMA table_list('logs')")?;
        let ordinary = statement
            .query_row([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(2)?))
            })
            .optional()?
            .is_some_and(|(schema, kind)| schema == "main" && kind == "table");
        if !ordinary {
            return Err(ReadError::Unsupported);
        }
        let mut statement = tx.prepare("PRAGMA table_info(logs)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        if !["id", "ts", "level", "target"]
            .iter()
            .all(|name| columns.contains(*name))
        {
            return Err(ReadError::Unsupported);
        }
        let high = tx.query_row("SELECT max(id) FROM logs", [], |row| {
            row.get_ref(0).map(|value| match value {
                ValueRef::Null => Ok(None),
                ValueRef::Integer(value) => Ok(Some(value)),
                _ => Err(ReadError::Unsupported),
            })
        })??;
        let Some(high) = high else {
            return Ok(Batch::default());
        };
        let (cursor, history) = if self.cursor.is_some_and(|cursor| high < cursor) {
            (None, None)
        } else {
            (self.cursor, self.history)
        };
        let body = match (
            columns.contains("feedback_log_body"),
            columns.contains("message"),
        ) {
            (true, true) => "coalesce(nullif(feedback_log_body,''),nullif(message,''))",
            (true, false) => "feedback_log_body",
            (false, true) => "message",
            _ => "NULL",
        };
        let thread = if columns.contains("thread_id") {
            "thread_id"
        } else {
            "NULL"
        };
        let fields = format!("id,ts,level,target,{thread},CASE WHEN upper(level) IN ('ERROR','FATAL','CRITICAL','WARN','WARNING')
            OR lower(target) LIKE '%sql%' OR lower(target) LIKE '%database%' OR lower(target) LIKE '%state_db%'
            OR target LIKE 'codex_api::%' OR target='codex_otel.log_only' OR target='codex_core::responses_retry'
            THEN substr({body},1,8192) END");
        let mut batch = Batch {
            cursor,
            history,
            ..Batch::default()
        };
        let file = source
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ReadError::Io)?;
        let first = cursor.is_none();
        let order = if first { "DESC" } else { "ASC" };
        let mut forward = tx.prepare(&format!(
            "SELECT {fields} FROM logs WHERE id>? ORDER BY id {order} LIMIT {PAGE}"
        ))?;
        let mut rows = forward.query([cursor.unwrap_or(0)])?;
        let mut recent = Vec::new();
        let mut edge = None;
        while let Some(row) = rows.next()? {
            let id = row.get::<_, i64>(0)?;
            batch.cursor = Some(batch.cursor.map_or(id, |cursor| cursor.max(id)));
            edge = Some(edge.map_or(id, |edge: i64| edge.min(id)));
            recent.push(batch.project(row, file, None)?);
        }
        if first {
            if recent.len() == PAGE {
                batch.history = edge;
            }
            recent.reverse();
        }
        for value in recent {
            if let RowProjection::Event(value) = value {
                batch.append(*value, false);
            }
        }
        if let Some(history) = batch.history {
            let mut query = tx.prepare(&format!(
                "SELECT {fields} FROM logs WHERE id<? ORDER BY id DESC LIMIT {PAGE}"
            ))?;
            let mut rows = query.query([history])?;
            let mut count = 0;
            let mut edge = None;
            let mut expired = false;
            while let Some(row) = rows.next()? {
                count += 1;
                edge = Some(row.get::<_, i64>(0)?);
                let cutoff = now - Duration::hours(24);
                match batch.project(row, file, Some(cutoff))? {
                    RowProjection::Event(value) => batch.append(*value, true),
                    RowProjection::Ignored => {}
                    RowProjection::Expired => {
                        expired = true;
                        break;
                    }
                }
            }
            batch.history = if count == PAGE && !expired {
                edge
            } else {
                None
            };
        }
        batch.backfill =
            batch.history.is_some() || batch.cursor.is_some_and(|cursor| cursor < high);
        Ok(batch)
    }
}

fn push(values: &mut VecDeque<Value>, value: Value, limit: usize) {
    if values.len() == limit {
        values.pop_front();
    }
    values.push_back(value);
}

#[derive(Default)]
struct Batch {
    cursor: Option<i64>,
    history: Option<i64>,
    backfill: bool,
    read: u64,
    parsed: u64,
    unsupported: u64,
    logs: Vec<Value>,
    errors: Vec<Value>,
    api_events: Vec<Value>,
    sql: Vec<Value>,
}

#[derive(Clone)]
pub(super) struct Projected {
    pub(super) log: Value,
    pub(super) severity: Option<&'static str>,
    pub(super) sql: Option<Value>,
    pub(super) api: Option<Value>,
}

enum RowProjection {
    Ignored,
    Expired,
    Event(Box<Projected>),
}

impl Batch {
    fn project(
        &mut self,
        row: &Row<'_>,
        file: &str,
        cutoff: Option<DateTime<Utc>>,
    ) -> Result<RowProjection, ReadError> {
        self.read += 1;
        let id = row.get::<_, i64>(0)?;
        let level = row
            .get_ref(2)?
            .as_str()
            .ok()
            .filter(|level| LEVEL.is_match(level));
        let module = row
            .get_ref(3)?
            .as_str()
            .ok()
            .filter(|module| MODULE.is_match(module));
        let Some((level, module, date)) = level
            .zip(module)
            .zip(timestamp(row.get_ref(1)?))
            .map(|((level, module), date)| (level, module, date))
        else {
            self.unsupported += 1;
            return Ok(RowProjection::Ignored);
        };
        if cutoff.is_some_and(|cutoff| date < cutoff) {
            return Ok(RowProjection::Expired);
        }
        self.parsed += 1;
        let when = stamp(date);
        let thread = row
            .get_ref(4)?
            .as_str()
            .ok()
            .filter(|thread| UUID.is_match(thread));
        let body = row.get_ref(5)?.as_str().unwrap_or_default();
        let level = level.to_ascii_lowercase();
        let severity = match level.as_str() {
            "warn" | "warning" => Some("warning"),
            "fatal" | "critical" | "error" => Some("error"),
            _ => None,
        };
        let (category, code) = classify(module, body);
        let mut log = json!({
            "timestamp":when,"category":category,"code":code,"reason":code,"source":"codex_core",
            "module":module,"thread_id":thread,"severity":if level=="warn" {"warning"} else {&level},
            "file":file,"record_id":id,"content_id":diagnostic_id(&when,module,file,id),
        });
        log.as_object_mut()
            .expect("projected log")
            .extend(details(body));
        let api = model_api::project(module, body).map(|projected| {
            let mut event = Map::new();
            for key in [
                "timestamp",
                "source",
                "module",
                "file",
                "record_id",
                "thread_id",
            ] {
                event.insert(key.into(), log[key].clone());
            }
            event.extend(projected);
            event.insert("observation_id".into(), log["content_id"].clone());
            Value::Object(event)
        });
        let sql = project_diagnostic(module, body).map(|projected| {
            let mut sql = json!(projected);
            sql.as_object_mut()
                .expect("projected SQL")
                .extend(Map::from_iter([
                    ("timestamp".into(), when.into()),
                    ("source".into(), "codex_core".into()),
                    ("module".into(), module.into()),
                    ("thread_id".into(), json!(thread)),
                    ("file".into(), file.into()),
                    ("record_id".into(), id.into()),
                    ("recognition".into(), "diagnostic_log".into()),
                    (
                        "result".into(),
                        if severity == Some("error") {
                            "log_error"
                        } else {
                            "log_recorded"
                        }
                        .into(),
                    ),
                ]));
            sql
        });
        Ok(RowProjection::Event(Box::new(Projected {
            log,
            severity,
            sql,
            api,
        })))
    }

    fn append(&mut self, mut projected: Projected, past: bool) {
        if let Some(api) = projected.api {
            self.api_events.push(api);
        }
        if let Some(sql) = projected.sql {
            self.sql.push(sql);
        }
        if !past {
            self.logs.push(projected.log.clone());
        }
        if let Some(severity) = projected.severity {
            projected.log["severity"] = severity.into();
            self.errors.push(projected.log);
        }
    }
}

fn timestamp(value: ValueRef<'_>) -> Option<DateTime<Utc>> {
    let seconds = match value {
        ValueRef::Integer(value) => value as f64,
        ValueRef::Real(value) => value,
        _ => return None,
    };
    if !seconds.is_finite() {
        return None;
    }
    let date =
        DateTime::<Utc>::from_timestamp_micros((seconds * 1_000_000.0).round_ties_even() as i64)?;
    (1..=9999).contains(&date.year()).then_some(date)
}

fn classify(module: &str, body: &str) -> (&'static str, &'static str) {
    let lower = body.to_ascii_lowercase();
    let source = module.to_ascii_lowercase();
    if [
        "error submitting",
        "submit failed",
        "submission failed",
        "failed to send",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        ("conversation", "message_submit_failed")
    } else if lower.contains("stream disconnected") || module.contains("responses_retry") {
        ("conversation", "stream_interrupted")
    } else if source.contains("mcp") {
        ("mcp", "mcp_diagnostic")
    } else if source.contains("tools") || source.contains("browser-use") {
        ("tool", "tool_diagnostic")
    } else {
        ("codex", "codex_diagnostic")
    }
}

// Desktop source collection is integrated separately from this bounded projection.
#[allow(dead_code)]
pub(super) fn project_desktop_line(line: &str, file: &str, offset: u64) -> Option<Projected> {
    let parsed = lam_session_core::desktop_line(line)?;
    let date = lam_history_store::parse_utc_stamp(parsed.timestamp)?;
    let heading = DESKTOP_HEADING
        .find(parsed.body)
        .map_or(parsed.body, |found| &parsed.body[..found.start()]);
    let heading: String = heading.chars().take(512).collect();
    let (category, reason) = classify(parsed.module, &heading);
    let severity = match parsed.level.to_ascii_lowercase().as_str() {
        "warn" | "warning" => "warning".to_owned(),
        other => other.to_owned(),
    };
    let mut fields = BTreeMap::new();
    for capture in DESKTOP_PAIR.captures_iter(parsed.body) {
        let key = capture.get(1).expect("field name").as_str();
        let value = capture.get(2).expect("field value").as_str();
        if [
            "conversationId",
            "threadId",
            "thread_id",
            "errorCode",
            "errorName",
            "method",
        ]
        .contains(&key)
            && value != "null"
            && !value.starts_with('"')
            && !value.starts_with("sk-")
            && !value.starts_with("sk_")
            && DESKTOP_IDENTIFIER.is_match(value)
        {
            fields.insert(key, value);
        }
    }
    let thread = ["conversationId", "threadId", "thread_id"]
        .into_iter()
        .filter_map(|key| fields.get(key))
        .find(|value| UUID.is_match(value))
        .copied();
    let mut event = json!({
        "timestamp": stamp(date),
        "category": category,
        "code": fields.get("errorCode").copied().unwrap_or(reason),
        "reason": reason,
        "source": "codex_desktop",
        "module": parsed.module,
        "thread_id": thread,
        "error_type": fields.get("errorName"),
        "method": fields.get("method"),
        "severity": severity,
    });
    event.as_object_mut()?.extend(details(parsed.body));
    let record_hash = format!("{:x}", Sha256::digest(line.as_bytes()));
    let content_id = desktop_diagnostic_id(
        event["timestamp"].as_str()?,
        parsed.module,
        file,
        &record_hash,
    );
    let fields = event.as_object_mut()?;
    fields.insert("file".into(), file.into());
    fields.insert("record_offset".into(), offset.into());
    fields.insert("record_hash".into(), record_hash.into());
    fields.insert("content_id".into(), content_id.clone().into());
    let severity = match parsed.level.to_ascii_lowercase().as_str() {
        "warn" | "warning" => Some("warning"),
        "fatal" | "critical" | "error" => Some("error"),
        _ => None,
    };
    let api = model_api::project(parsed.module, parsed.body).map(|mut fields| {
        for key in [
            "timestamp",
            "source",
            "module",
            "thread_id",
            "file",
            "record_offset",
        ] {
            fields.insert(key.into(), event[key].clone());
        }
        fields.insert("observation_id".into(), content_id.into());
        Value::Object(fields)
    });
    let sql = project_diagnostic(parsed.module, parsed.body).map(|projected| {
        let mut value = json!(projected);
        let fields = value.as_object_mut().expect("projected SQL");
        for key in [
            "timestamp",
            "source",
            "module",
            "thread_id",
            "file",
            "record_offset",
            "record_hash",
        ] {
            fields.insert(key.into(), event[key].clone());
        }
        fields.insert("recognition".into(), "diagnostic_log".into());
        fields.insert(
            "result".into(),
            if severity == Some("error") {
                "log_error"
            } else {
                "log_recorded"
            }
            .into(),
        );
        value
    });
    Some(Projected {
        log: event,
        severity,
        sql,
        api,
    })
}

fn python_repr_file(file: &str) -> String {
    let quote = if file.contains('\'') && !file.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut quoted_file = String::new();
    quoted_file.push(quote);
    for character in file.chars() {
        match character {
            '\\' => quoted_file.push_str("\\\\"),
            '\n' => quoted_file.push_str("\\n"),
            '\r' => quoted_file.push_str("\\r"),
            '\t' => quoted_file.push_str("\\t"),
            value if value == quote => {
                quoted_file.push('\\');
                quoted_file.push(value);
            }
            value if value != ' ' && NONPRINTABLE.is_match(value.encode_utf8(&mut [0; 4])) => {
                use std::fmt::Write;
                let code = value as u32;
                if code <= 0xff {
                    write!(quoted_file, "\\x{code:02x}").unwrap();
                } else if code <= 0xffff {
                    write!(quoted_file, "\\u{code:04x}").unwrap();
                } else {
                    write!(quoted_file, "\\U{code:08x}").unwrap();
                }
            }
            value => quoted_file.push(value),
        }
    }
    quoted_file.push(quote);
    quoted_file
}

fn desktop_diagnostic_id(time: &str, module: &str, file: &str, hash: &str) -> String {
    let file = python_repr_file(file);
    let identity = format!("{{'source': 'codex_desktop', 'timestamp': '{time}', 'module': '{module}', 'file': {file}, 'record_id': None, 'record_hash': '{hash}'}}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn diagnostic_id(time: &str, module: &str, file: &str, id: i64) -> String {
    let quoted_file = python_repr_file(file);
    let identity = format!("{{'source': 'codex_core', 'timestamp': '{time}', 'module': '{module}', 'file': {quoted_file}, 'record_id': {id}, 'record_hash': None}}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

pub(super) fn details(body: &str) -> Map<String, Value> {
    // Match the Python diagnostic limit in Unicode characters.
    let bounded: String;
    let body = if body.len() > 8192 {
        bounded = body.chars().take(8192).collect();
        bounded.as_str()
    } else {
        body
    };
    let mut fields = BTreeMap::new();
    for capture in PAIR.captures_iter(body) {
        let key = capture.get(1).expect("named field").as_str();
        let value = capture.get(2).expect("field value").as_str();
        let value = if let Some(value) = value.strip_prefix('"') {
            value.strip_suffix('"').unwrap_or(value)
        } else {
            value.trim_end_matches([',', ';', '}'])
        };
        fields.insert(key, value);
    }
    let mut result = Map::new();
    for (key, aliases) in [
        ("request_id", &["request_id", "requestId"][..]),
        ("trace_id", &["trace_id", "traceId", "correlationId"][..]),
        ("error_type", &["errorName", "error_type"][..]),
        ("method", &["method"][..]),
    ] {
        if let Some(value) = aliases
            .iter()
            .filter_map(|alias| fields.get(alias))
            .find(|value| {
                IDENTIFIER.is_match(value) && !value.starts_with("sk-") && !value.starts_with("sk_")
            })
        {
            result.insert(key.into(), (*value).into());
        }
    }
    let heading = HEADING
        .find(body)
        .map_or(body, |part| &body[..part.start()]);
    let heading = QUOTED.replace_all(heading, "");
    let status = |value: &str| -> Option<u16> {
        (value.len() == 3)
            .then(|| value.parse::<u16>().ok())
            .flatten()
            .filter(|status| (100..=599).contains(status))
    };
    if let Some(status) = HTTP
        .captures(&heading)
        .and_then(|capture| status(&capture[1]))
    {
        result.insert("http_status".into(), status.into());
    }
    for key in ["http_status", "status_code", "status"] {
        if let Some(status) = fields.get(key).and_then(|value| status(value)) {
            result.insert("http_status".into(), status.into());
        }
    }
    if let Some((cause, _)) = CAUSES
        .iter()
        .find(|(_, pattern)| pattern.is_match(&heading))
    {
        result.insert("cause".into(), (*cause).into());
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_projection_excludes_raw_body_and_credentials() {
        let line = "2026-11-12T01:02:03.123Z WARN [codex.mcp] stream disconnected request_id=sk-PRIVATE trace_id=safe-id secret=PRIVATE";
        let event = super::project_desktop_line(line, "fixture.log", 7)
            .unwrap()
            .log;
        assert_eq!(event["source"], "codex_desktop");
        assert_eq!(event["reason"], "stream_interrupted");
        assert_eq!(event["severity"], "warning");
        assert_eq!(event["trace_id"], "safe-id");
        assert!(!event.to_string().contains("PRIVATE"));
        assert!(super::project_desktop_line(
            "2026-99-99T01:02:03Z WARN [codex] text",
            "fixture.log",
            7
        )
        .is_none());
        let basic =
            super::project_desktop_line("20261010T100000+0800 WARN [codex] text", "fixture.log", 7)
                .unwrap()
                .log;
        assert_eq!(basic["timestamp"], "2026-10-10T02:00:00.000Z");
        assert!(super::project_desktop_line(
            "2026-10-10T10:00:00 WARN [codex] text",
            "fixture.log",
            7
        )
        .is_none());
        let event = super::project_desktop_line("2026-11-12T01:02:03Z ERROR [codex] failed threadId=12345678-1234-1234-1234-123456789abc errorCode=SAFE errorName=sk_PRIVATE method=send", "fixture.log", 7).unwrap().log;
        assert_eq!(event["thread_id"], "12345678-1234-1234-1234-123456789abc");
        assert_eq!(event["code"], "SAFE");
        assert!(event["error_type"].is_null());
        assert_eq!(event["method"], "send");
        let event = super::project_desktop_line(
            "2026-11-12T01:02:03Z WARN [codex] notice error: stream disconnected",
            "fixture.log",
            0,
        )
        .unwrap();
        assert_eq!(event.log["reason"], "stream_interrupted");
    }

    #[test]
    fn desktop_diagnostic_projects_sql_and_api_metadata_without_body() {
        let sql = super::project_desktop_line("2026-11-12T01:02:03Z INFO [sqlx::query] summary=\"SELECT PRIVATE\" elapsed=2ms rows_returned=3", "fixture.log", 7).unwrap();
        let projected = sql.sql.unwrap();
        assert_eq!(projected["source"], "codex_desktop");
        assert_eq!(projected["statement"], "SELECT");
        assert!(!projected.to_string().contains("PRIVATE"));
        let api = super::project_desktop_line("2026-11-12T01:02:03Z INFO [codex_otel.log_only] event.name=\"codex.api_request\" status_code=200", "fixture.log", 7).unwrap();
        let api = api.api.unwrap();
        assert!(api.get("record_hash").is_none());
        assert_eq!(api["record_offset"], 7);
        assert!(api["observation_id"].is_string());
    }

    #[test]
    fn desktop_identity_matches_python_for_quoted_filename() {
        assert_eq!(
            super::desktop_diagnostic_id(
                "2026-11-12T01:02:03.123Z",
                "codex.mcp",
                "quote's.log",
                &"a".repeat(64)
            ),
            "f8121e5d7f26f89994f0dc5a3be68f1e5ed4bc485ab62f310ac12fa4d81126f6"
        );
    }

    #[test]
    fn desktop_stream_projects_complete_lines_once_with_byte_offsets() {
        let first = "2026-11-12T01:02:03Z WARN [codex.mcp] failed request_id=req-1";
        let second = "2026-11-12T01:02:04Z ERROR [codex] failed errorCode=SAFE";
        let mut stream = lam_session_core::DesktopStream::default();
        let input = format!("{first}\n{second}\n");
        let split = first.len() / 2;
        assert!(stream.push(&input.as_bytes()[..split]).unwrap().is_empty());
        let projected: Vec<_> = stream
            .push(&input.as_bytes()[split..])
            .unwrap()
            .into_iter()
            .filter_map(|(offset, line)| super::project_desktop_line(&line, "fixture.log", offset))
            .collect();
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].log["record_offset"], 0);
        assert_eq!(projected[1].log["record_offset"], (first.len() + 1) as u64);
        assert_ne!(
            projected[0].log["content_id"],
            projected[1].log["content_id"]
        );
        assert!(stream.push(b"").unwrap().is_empty());
    }

    #[test]
    fn diagnostic_details_limit_matches_unicode_character_boundary() {
        let accepted = format!("{} request_id=inside", "字".repeat(8170));
        assert_eq!(super::details(&accepted)["request_id"], "inside");
        let excluded = format!("{} request_id=outside", "字".repeat(8192));
        assert!(super::details(&excluded).get("request_id").is_none());
        let boundary = format!("{} request_id=abc", "x".repeat(8177));
        assert_eq!(super::details(&boundary)["request_id"], "abc");
        let truncated = format!("{} request_id=abcdef", "x".repeat(8177));
        assert_eq!(super::details(&truncated)["request_id"], "abc");
    }

    use super::*;

    #[test]
    fn core_diagnostic_identity_matches_python_for_quoted_filename() {
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_quote's.sqlite",
                1
            ),
            "582bb6e9e9a51d019e2775bb43e74211e37cc7abc400ff2a7fefd5de39d61fbc"
        );
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_\"and'quotes.sqlite",
                1
            ),
            "af89a023b15ca00526a4549ad200cce4fc1a975cb1dd8743ac5070039754755c"
        );
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_中文.sqlite",
                1
            ),
            "4d8275198a2d50c62ff890d585e9f44ce96821e769495d626db011f0d0bcee65"
        );
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_e\u{0301}.sqlite",
                1
            ),
            "f067358e23f58837129be3f231021d1bbd0bfd35f7cc0963fb87b0b69395046d"
        );
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_\0.sqlite",
                1
            ),
            "5ddcc9956153c1d4f76cd4a48230d48afb7e5761685aeb364e0870646e9c3c22"
        );
        assert_eq!(
            diagnostic_id(
                "2026-11-12T00:00:00.000Z",
                "codex_core",
                "logs_a\u{200d}b.sqlite",
                1
            ),
            "0841f47ba03f9a74eec6659f3a4e0298425908ca74951e63cc40933471ee0247"
        );
    }

    #[test]
    fn discovers_and_watches_non_numeric_core_log_names() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let path = home.join("logs_active.sqlite");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT)",
            )
            .unwrap();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        writer
            .execute(
                "INSERT INTO logs VALUES(1,?,'WARN','codex_core')",
                [now.timestamp()],
            )
            .unwrap();
        let mut core = Core::new(true);
        assert!(related_event(&home, &path));
        assert!(related_event(&home, &sidecar(&path, "-wal")));
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.health(), "ok");
        assert_eq!(core.errors.len(), 1);
    }

    #[test]
    fn forward_and_historical_reads_have_independent_page_limits() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let path = home.join("logs_1.sqlite");
        let mut writer = Connection::open(path).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT)",
            )
            .unwrap();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        fn insert(writer: &mut Connection, start: i64, end: i64, stamp: i64) {
            let tx = writer.transaction().unwrap();
            {
                let mut query = tx
                    .prepare("INSERT INTO logs VALUES(?,?,'INFO','codex_core')")
                    .unwrap();
                for id in start..=end {
                    query.execute([id, stamp]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        insert(&mut writer, 1, 4201, now.timestamp());
        let mut core = Core::new(true);
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.read_lines, 4000);
        assert_eq!(core.cursor, Some(4201));
        assert!(core.backfill());
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.read_lines, 4201);
        assert!(!core.backfill());
        assert_eq!(core.logs.len(), 1000);

        insert(&mut writer, 4202, 6301, now.timestamp());
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.cursor, Some(6201));
        assert_eq!(core.read_lines, 6201);
        assert!(core.backfill());
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.cursor, Some(6301));
        assert!(!core.backfill());
    }

    #[test]
    fn core_rejects_view_instead_of_scanning_it() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let writer = Connection::open(home.join("logs_9.sqlite")).unwrap();
        writer.execute_batch("CREATE VIEW logs AS SELECT 1 AS id, 1800000000 AS ts, 'INFO' AS level, 'sqlx::query' AS target").unwrap();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let mut core = Core::new(true);
        assert!(core.refresh(discover(&home), now));
        assert_eq!(core.health(), "unsupported");
        assert!(core.logs.is_empty());
        assert!(!core.backfill());
    }
}
