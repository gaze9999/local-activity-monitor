use crate::mcp;
use lam_session_core::database_path;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::history::write_history;
use crate::migration::{encode_identity, projected_key};
use crate::{
    check_path, normalize_utc_stamp, parse_utc_stamp, Access, HistoryWrite, Namespace, Result,
    Section, Store, StoreError,
};

pub(crate) type Object = Map<String, Value>;
type Time = DateTime<Utc>;
const DEFAULT_DAYS: i64 = 90;

struct Row {
    section: Section,
    key: String,
    time: String,
    payload: String,
}

struct Import {
    days: Option<i64>,
    metadata: Value,
    rows: Vec<Row>,
}

impl Store {
    /// Import one legacy JSON namespace if neither metadata nor rows exist for it.
    pub fn import_legacy(directory: &Path, namespace: Namespace, now: Time) -> Result<bool> {
        let (file, limit) = match namespace {
            Namespace::Activity => ("activity-history.json", 1024 * 1024),
            Namespace::Errors => ("error-history.json", 512 * 1024),
            Namespace::Threads => ("thread-state.json", 512 * 1024),
        };
        match Self::open(directory, Access::ReadOnly) {
            Ok(Some(store)) if namespace_present(&store.db, namespace)? => return Ok(false),
            Ok(_)
            | Err(StoreError::UnsupportedSchema {
                application: 0,
                version: 0,
            }) => {}
            Err(error) => return Err(error),
        }
        check_path(directory)?;
        let path = directory.join(file);
        check_path(&path)?;
        if !path.try_exists()? {
            return Ok(false);
        }
        let mut file = open_source(directory, file)?;
        let before = file.metadata()?;
        if !before.is_file() {
            return Err(StoreError::UnsafePath);
        }
        if before.len() > limit {
            return Err(StoreError::InvalidLegacy);
        }
        let mut raw = Vec::new();
        (&mut file).take(limit + 1).read_to_end(&mut raw)?;
        let after = file.metadata()?;
        if raw.len() as u64 > limit
            || before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
        {
            return Err(StoreError::InvalidLegacy);
        }
        let text = decode(&raw).ok_or(StoreError::InvalidLegacy)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| StoreError::InvalidLegacy)?;
        let data = value.as_object().ok_or(StoreError::InvalidLegacy)?;
        let import = match namespace {
            Namespace::Activity => activity(data, now)?,
            Namespace::Errors => errors(data, now)?,
            Namespace::Threads => threads(data)?,
        };
        let mut store =
            Self::open(directory, Access::ReadWrite)?.ok_or(StoreError::InvalidLegacy)?;
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if namespace_present(&tx, namespace)? {
            return Ok(false);
        }
        let days = match import.days {
            Some(days) => days,
            None => retention(&tx)?,
        };
        let boundary = (days != 0).then(|| date(now - Duration::days(days)));
        let rows = history_rows(&import.rows);
        let updated = now.timestamp_nanos_opt().ok_or(StoreError::InvalidLegacy)?;
        write_history(
            &tx,
            namespace,
            &import.metadata.to_string(),
            updated,
            boundary.as_deref(),
            &rows,
        )?;
        tx.commit()?;
        Ok(true)
    }
}

fn namespace_present(db: &rusqlite::Connection, namespace: Namespace) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM history_state WHERE namespace=?1)
             OR EXISTS(SELECT 1 FROM history_items WHERE namespace=?1)",
        [namespace.name()],
        |row| row.get(0),
    )
}

pub(crate) fn retention(db: &rusqlite::Connection) -> Result<i64> {
    let stored = db
        .query_row(
            "SELECT metadata FROM history_state WHERE namespace='activity'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match stored {
        Some(value) => {
            let data: Value =
                serde_json::from_str(&value).map_err(|_| StoreError::InvalidLegacy)?;
            days(data.as_object().ok_or(StoreError::InvalidLegacy)?)
        }
        None => Ok(DEFAULT_DAYS),
    }
}

fn days(data: &Object) -> Result<i64> {
    data.get("retention_days")
        .map_or(Ok(DEFAULT_DAYS), |value| {
            value
                .as_i64()
                .filter(|days| (0..=3650).contains(days))
                .ok_or(StoreError::InvalidLegacy)
        })
}

#[cfg(unix)]
fn open_source(directory: &Path, name: &str) -> io::Result<File> {
    use rustix::fs::{open, openat, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let parent = open(directory, flags | OFlags::DIRECTORY, Mode::empty())?;
    Ok(openat(&parent, name, flags, Mode::empty())?.into())
}

#[cfg(windows)]
fn open_source(directory: &Path, name: &str) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x00200000)
        .open(directory.join(name))?;
    if file.metadata()?.file_attributes() & 0x400 != 0 {
        return Err(io::Error::other("Unsafe legacy history"));
    }
    Ok(file)
}

fn decode(raw: &[u8]) -> Option<String> {
    // Legacy builds write UTF-8. Recognize the Unicode encodings of the previous JSON reader.
    let decode32 = |bytes: &[u8], little: bool| {
        let (units, remainder) = bytes.as_chunks::<4>();
        if !remainder.is_empty() {
            return None;
        }
        units
            .iter()
            .map(|&unit| {
                char::from_u32(if little {
                    u32::from_le_bytes(unit)
                } else {
                    u32::from_be_bytes(unit)
                })
            })
            .collect::<Option<String>>()
    };
    let decode16 = |bytes: &[u8], little: bool| {
        let (units, remainder) = bytes.as_chunks::<2>();
        if !remainder.is_empty() {
            return None;
        }
        let values = units
            .iter()
            .map(|&unit| {
                if little {
                    u16::from_le_bytes(unit)
                } else {
                    u16::from_be_bytes(unit)
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&values).ok()
    };
    if let Some(raw) = raw.strip_prefix(b"\xff\xfe\0\0") {
        return decode32(raw, true);
    }
    if let Some(raw) = raw.strip_prefix(b"\0\0\xfe\xff") {
        return decode32(raw, false);
    }
    for (bom, little) in [(b"\xff\xfe", true), (b"\xfe\xff", false)] {
        if let Some(raw) = raw.strip_prefix(bom) {
            return decode16(raw, little);
        }
    }
    match raw {
        [0, 0, _, _, ..] => decode32(raw, false),
        [_, 0, 0, 0, ..] => decode32(raw, true),
        [0, _, ..] => decode16(raw, false),
        [_, 0, ..] => decode16(raw, true),
        _ => std::str::from_utf8(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw))
            .ok()
            .map(str::to_owned),
    }
}

fn date(value: Time) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn version(data: &Object, supported: &[u64]) -> Result<()> {
    if data
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|value| supported.contains(&value))
    {
        Ok(())
    } else {
        Err(StoreError::InvalidLegacy)
    }
}

fn array<'a>(data: &'a Object, key: &str) -> Result<&'a [Value]> {
    data.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(StoreError::InvalidLegacy)
}

pub(crate) fn identifier(value: &str, limit: usize, extra: &[u8]) -> bool {
    (1..=limit).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
}

pub(crate) fn text<'a>(data: &'a Object, key: &str) -> Option<&'a str> {
    data.get(key)?.as_str()
}

fn identifiers(data: &Object, fields: &[&str], result: &mut Object) {
    for &key in fields {
        if let Some(value) = text(data, key).filter(|value| identifier(value, 160, b"_./:-")) {
            result.insert(key.into(), value.into());
        }
    }
}

fn integers(data: &Object, fields: &[&str], minimum: i64, maximum: i64, result: &mut Object) {
    for &key in fields {
        if let Some(value) = data
            .get(key)
            .and_then(Value::as_i64)
            .filter(|value| (minimum..=maximum).contains(value))
        {
            result.insert(key.into(), value.into());
        }
    }
}

fn within(data: &Object, boundary: Option<Time>, ceiling: Time) -> Option<&str> {
    let stamp = text(data, "timestamp")?;
    let time = parse_utc_stamp(stamp)?;
    (time <= ceiling && boundary.is_none_or(|cutoff| time >= cutoff)).then_some(stamp)
}

fn row(section: Section, key: String, data: Object) -> Result<Row> {
    let time = text(&data, "timestamp")
        .or_else(|| text(&data, "task_time"))
        .and_then(normalize_utc_stamp)
        .ok_or(StoreError::InvalidLegacy)?;
    let mut payload = String::new();
    encode_identity(&Value::Object(data), &mut payload)?;
    Ok(Row {
        section,
        key,
        time,
        payload,
    })
}

fn history_rows(rows: &[Row]) -> Vec<HistoryWrite<'_>> {
    let mut ordinals = [0; 6];
    rows.iter()
        .map(|row| {
            let index = row.section as usize;
            let ordinal = ordinals[index];
            ordinals[index] += 1;
            HistoryWrite {
                section: row.section,
                key: &row.key,
                ordinal,
                event_time: Some(&row.time),
                payload: &row.payload,
            }
        })
        .collect()
}

fn activity(data: &Object, now: Time) -> Result<Import> {
    version(data, &[1, 2, 3])?;
    let days = days(data)?;
    let boundary = (days > 0).then(|| now - Duration::days(days));
    let ceiling = now + Duration::minutes(1);
    let mut rows = Vec::new();
    for (section, values, limit) in [
        (Section::Sql, array(data, "sql")?, 500),
        (Section::Web, array(data, "web")?, 1000),
        (
            Section::Mcp,
            if data.contains_key("mcp") {
                array(data, "mcp")?
            } else {
                &[]
            },
            1000,
        ),
    ] {
        let mut keys = HashMap::new();
        let mut merged = Vec::new();
        for value in values.iter().take(limit).filter_map(Value::as_object) {
            let Some(stamp) = within(value, boundary, ceiling) else {
                continue;
            };
            let Some(projected) = activity_row(section, value, stamp) else {
                continue;
            };
            collect_row(section, projected, &mut keys, &mut merged, true)?;
        }
        rows.extend(sorted_rows(section, merged)?);
    }
    Ok(Import {
        days: Some(days),
        metadata: json!({"version":3,"retention_days":days}),
        rows,
    })
}

const STATEMENTS: &[&str] = &[
    "CONNECT",
    "CLOSE",
    "SELECT",
    "EXPLAIN",
    "INSERT",
    "UPDATE",
    "DELETE",
    "REPLACE",
    "CREATE",
    "ALTER",
    "DROP",
    "PRAGMA",
    "VACUUM",
    "ANALYZE",
    "REINDEX",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    "RELEASE",
    "ATTACH",
    "DETACH",
    "DATABASE_EVENT",
];

pub(crate) fn activity_row(section: Section, data: &Object, stamp: &str) -> Option<Object> {
    let mut output = Object::new();
    output.insert("timestamp".into(), stamp.into());
    let fields: &[&str] = if section == Section::Sql {
        &[
            "source",
            "operation",
            "engine",
            "recognition",
            "result",
            "tool",
            "thread_id",
            "call_id",
            "file",
            "record_hash",
        ]
    } else {
        &[
            "tool",
            "thread_id",
            "call_id",
            "action",
            "category",
            "server",
        ]
    };
    identifiers(data, fields, &mut output);
    integers(
        data,
        &[
            "index",
            "record_id",
            "record_offset",
            "rows_affected",
            "rows_returned",
        ],
        0,
        i64::MAX,
        &mut output,
    );
    for key in ["duration_ms", "container_duration_ms"] {
        if let Some(value) = data.get(key).filter(|value| mcp::number(value, 86_400_000)) {
            output.insert(key.into(), value.clone());
        }
    }
    if section == Section::Sql {
        let statement = text(data, "statement").filter(|value| STATEMENTS.contains(value))?;
        if !["operation", "engine", "recognition"]
            .iter()
            .all(|key| output.contains_key(*key))
        {
            return None;
        }
        output.insert("statement".into(), statement.into());
        for key in ["database", "workdir"] {
            if let Some(value) = text(data, key).and_then(database_path) {
                output.insert(key.into(), value.into());
            }
        }
    } else {
        let server = text(data, "server").filter(|server| {
            if section == Section::Web {
                *server == "web"
            } else {
                *server != "web" && identifier(server, 80, b"_.-")
            }
        })?;
        output.insert(
            "nested".into(),
            (data.get("nested").and_then(Value::as_bool) == Some(true)).into(),
        );
        if let Some(value) = text(data, "completed_at")
            .filter(|value| value.chars().count() <= 40 && parse_utc_stamp(value).is_some())
        {
            output.insert("completed_at".into(), value.into());
        }
        output.extend(mcp::project(server, data));
    }
    Some(output)
}

pub(crate) fn error_row(event: &Object, boundary: Option<Time>, ceiling: Time) -> Option<Object> {
    if !matches!(text(event, "severity"), Some("error" | "warning")) {
        return None;
    }
    let stamp = within(event, boundary, ceiling)?;
    let mut output = Object::new();
    output.insert("timestamp".into(), normalize_utc_stamp(stamp)?.into());
    identifiers(
        event,
        &[
            "source",
            "category",
            "severity",
            "code",
            "reason",
            "cause",
            "error_type",
            "module",
            "method",
            "server",
            "tool",
            "call_id",
            "thread_id",
            "file",
            "request_id",
            "trace_id",
            "record_hash",
            "content_id",
        ],
        &mut output,
    );
    if !["source", "category", "severity"]
        .iter()
        .all(|key| output.contains_key(*key))
    {
        return None;
    }
    integers(
        event,
        &[
            "exit_code",
            "http_status",
            "attempt",
            "index",
            "record_id",
            "record_offset",
        ],
        i32::MIN.into(),
        i32::MAX.into(),
        &mut output,
    );
    let mut value = Value::Object(output);
    crate::sanitize_error_identifiers(&mut value);
    value.as_object().cloned()
}

fn errors(data: &Object, now: Time) -> Result<Import> {
    version(data, &[1])?;
    let mut keys = HashMap::new();
    let mut merged = Vec::new();
    for event in array(data, "events")?
        .iter()
        .take(1000)
        .filter_map(Value::as_object)
    {
        let Some(output) = error_row(
            event,
            Some(now - Duration::hours(24)),
            now + Duration::minutes(1),
        ) else {
            continue;
        };
        collect_row(Section::Errors, output, &mut keys, &mut merged, false)?;
    }
    Ok(Import {
        days: None,
        metadata: json!({"version":1}),
        rows: sorted_rows(Section::Errors, merged)?,
    })
}

fn collect_row(
    section: Section,
    row: Object,
    keys: &mut HashMap<String, usize>,
    rows: &mut Vec<(String, Object)>,
    merge: bool,
) -> Result<()> {
    let key = projected_key(section, &row)?;
    if let Some(&index) = keys.get(&key) {
        if merge {
            merge_event(&mut rows[index].1, row);
        } else {
            rows[index].1 = row;
        }
    } else {
        keys.insert(key.clone(), rows.len());
        rows.push((key, row));
    }
    Ok(())
}

pub(crate) fn merge_event(previous: &mut Object, incoming: Object) {
    for (key, value) in incoming {
        match value {
            Value::Null => {}
            Value::Object(items) => {
                let target = previous
                    .entry(key)
                    .or_insert_with(|| Value::Object(Object::new()));
                if !target.is_object() {
                    *target = Value::Object(Object::new());
                }
                if let Some(target) = target.as_object_mut() {
                    merge_event(target, items);
                }
            }
            Value::Array(items) => {
                if let Some(existing) = previous.get_mut(&key).and_then(Value::as_array_mut) {
                    for item in items {
                        if !existing.contains(&item) {
                            existing.push(item);
                        }
                    }
                    existing.truncate(30);
                } else {
                    previous.insert(key, Value::Array(items));
                }
            }
            value => {
                previous.insert(key, value);
            }
        }
    }
}

fn sorted_rows(section: Section, mut items: Vec<(String, Object)>) -> Result<Vec<Row>> {
    items.sort_by(|(_, left), (_, right)| text(right, "timestamp").cmp(&text(left, "timestamp")));
    items
        .into_iter()
        .map(|(key, item)| row(section, key, item))
        .collect()
}

fn threads(data: &Object) -> Result<Import> {
    version(data, &[1])?;
    let entries = data
        .get("entries")
        .and_then(Value::as_object)
        .ok_or(StoreError::InvalidLegacy)?;
    let mut rows = Vec::new();
    for (key, value) in entries.iter().take(1000) {
        if let Some(item) = value.as_object().and_then(|data| thread_entry(key, data)) {
            rows.push(row(Section::Threads, key.clone(), item)?);
        }
    }
    if let Some(skills) = data.get("skills").and_then(Value::as_array) {
        for value in skills.iter().take(500).filter_map(Value::as_object) {
            if let Some(item) = skill_entry(value) {
                rows.push(row(
                    Section::Skills,
                    projected_key(Section::Skills, &item)?,
                    item,
                )?);
            }
        }
    }
    Ok(Import {
        days: None,
        metadata: json!({"version":1}),
        rows,
    })
}

fn thread_identity(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn thread_stamp(value: &str) -> bool {
    value.chars().count() <= 40 && parse_utc_stamp(value).is_some()
}

fn thread_entry(key: &str, data: &Object) -> Option<Object> {
    let source = key.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    if !identifier(source, 180, b"_.-") {
        return None;
    }
    let thread = text(data, "thread_id").filter(|value| thread_identity(value))?;
    let status = text(data, "status").filter(|value| matches!(*value, "running" | "completed"))?;
    let offset = data.get("offset")?.as_i64().filter(|value| *value >= 0)?;
    let time = text(data, "task_time").filter(|value| thread_stamp(value))?;
    let start = match data.get("task_start") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_str().filter(|value| thread_stamp(value))?),
    };
    Some(Map::from_iter([
        ("thread_id".into(), thread.into()),
        ("status".into(), status.into()),
        ("offset".into(), offset.into()),
        ("task_time".into(), time.into()),
        ("task_start".into(), json!(start)),
    ]))
}

fn skill_entry(data: &Object) -> Option<Object> {
    let thread = text(data, "thread_id").filter(|value| thread_identity(value))?;
    let call = text(data, "call_id").filter(|value| identifier(value, 160, b"_.:-"))?;
    let skill = text(data, "skill").filter(|value| {
        (1..=160).contains(&value.chars().count())
            && value.chars().all(|ch| {
                ch.is_ascii_alphanumeric()
                    || "_. -".contains(ch)
                    || ('\u{3400}'..='\u{9fff}').contains(&ch)
            })
    })?;
    let stamp = text(data, "timestamp").filter(|value| thread_stamp(value))?;
    Some(Map::from_iter([
        ("thread_id".into(), thread.into()),
        ("call_id".into(), call.into()),
        ("skill".into(), skill.into()),
        ("timestamp".into(), stamp.into()),
        ("has_document".into(), false.into()),
    ]))
}
