use crate::timestamp::normalize;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};
use serde_json::{value::RawValue, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::{
    check_path, identity, Result, StoreError, APPLICATION_ID, FILE_NAME, SCHEMA_VERSION, SIDECARS,
};

const TO_V2: &str = "
CREATE INDEX history_recent ON history_items(namespace, section, event_time DESC, item_key DESC);
CREATE INDEX history_cleanup ON history_items(event_time);
";
const TO_V3: &str = "
CREATE TABLE session_cursors (source_key TEXT PRIMARY KEY, offset INTEGER NOT NULL, signature TEXT NOT NULL, payload TEXT NOT NULL);
CREATE TABLE session_calls (source_key TEXT NOT NULL, call_id TEXT NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(source_key,call_id));
CREATE INDEX session_calls_recent ON session_calls(source_key,event_time DESC,call_id);
";
const TO_V4: &str = "
ALTER TABLE history_items ADD COLUMN server;
ALTER TABLE history_items ADD COLUMN operation;
ALTER TABLE history_items ADD COLUMN statement;
ALTER TABLE history_items ADD COLUMN nested;
UPDATE history_items SET
    server=CASE WHEN json_valid(payload) THEN json_extract(payload,'$.server') END,
    operation=CASE WHEN json_valid(payload) THEN json_extract(payload,'$.operation') END,
    statement=CASE WHEN json_valid(payload) THEN json_extract(payload,'$.statement') END,
    nested=CASE WHEN json_valid(payload) THEN CASE WHEN json_extract(payload,'$.nested')=1 THEN 1 ELSE 0 END END;
CREATE INDEX history_aggregate ON history_items(namespace, section, event_time, server, operation, statement, nested);
";

pub fn legacy((application, version): (i64, i64)) -> bool {
    application == APPLICATION_ID && (1..SCHEMA_VERSION).contains(&version)
}

pub fn connection(path: &Path, mode: OpenFlags) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_NOFOLLOW | OpenFlags::SQLITE_OPEN_NO_MUTEX | mode;
    let db = Connection::open_with_flags(path, flags)?;
    db.busy_timeout(Duration::from_millis(200))?;
    Ok(db)
}

pub fn upgrade(db: &mut Connection, path: &Path) -> Result<bool> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let header = identity(&tx)?;
    if header == (APPLICATION_ID, SCHEMA_VERSION) {
        return Ok(false);
    }
    if !legacy(header) {
        return Err(StoreError::UnsupportedSchema {
            application: header.0,
            version: header.1,
        });
    }
    let version = header.1;
    backup(path, version)?;
    if version < 2 {
        tx.execute_batch(TO_V2)?;
        rekey(&tx)?;
    }
    if version < 3 {
        tx.execute_batch(TO_V3)?;
    }
    tx.execute_batch(TO_V4)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(true)
}

fn backup(path: &Path, version: i64) -> Result<()> {
    let directory = path.parent().ok_or(StoreError::UnsafePath)?;
    let name = format!("{FILE_NAME}.schema-v{version}.bak");
    let destination = directory.join(&name);
    check_path(&destination)?;
    for suffix in SIDECARS {
        check_path(&directory.join(format!("{name}{suffix}")))?;
    }
    if !destination.try_exists()? {
        let temp = tempfile::Builder::new()
            .prefix(".lam-history-backup-")
            .tempfile_in(directory)?
            .into_temp_path();
        // The migration transaction reserves the writer lock. A second, read-only
        // connection copies the pre-migration database including committed WAL pages.
        let source = connection(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut target = connection(temp.as_ref(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        {
            let copy = Backup::new(&source, &mut target)?;
            if !matches!(copy.step(-1)?, StepResult::Done) {
                return Err(StoreError::InvalidMigration);
            }
        }
        target.pragma_update(None, "journal_mode", "DELETE")?;
        verify_backup(&target, version)?;
        target.close().map_err(|(_, error)| error)?;
        drop(source);
        OpenOptions::new().write(true).open(&temp)?.sync_all()?;
        if let Err(error) = temp.persist_noclobber(&destination) {
            if error.error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.error.into());
            }
        }
    }
    let stored = connection(&destination, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    verify_backup(&stored, version)?;
    #[cfg(unix)]
    std::fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn verify_backup(db: &Connection, version: i64) -> Result<()> {
    if identity(db)? != (APPLICATION_ID, version)
        || db.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))? != "ok"
    {
        return Err(StoreError::InvalidMigration);
    }
    Ok(())
}

fn rekey(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TEMP TABLE lam_rekey (
            namespace TEXT NOT NULL, section TEXT NOT NULL, item_key TEXT NOT NULL,
            ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL,
            PRIMARY KEY(namespace, section, item_key)
        )",
    )?;
    {
        let mut select = tx.prepare(
            "SELECT namespace, section, item_key, ordinal, event_time, payload
             FROM history_items ORDER BY rowid",
        )?;
        let mut insert = tx.prepare("INSERT OR REPLACE INTO lam_rekey VALUES (?,?,?,?,?,?)")?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let namespace: String = row.get(0)?;
            let section: String = row.get(1)?;
            let key: String = row.get(2)?;
            let ordinal: i64 = row.get(3)?;
            let event_time: Option<String> = row.get(4)?;
            let payload = row.get_ref(5)?.as_str().map_err(rusqlite::Error::from)?;
            if payload.len() > 16 * 1024 * 1024 {
                return Err(StoreError::InvalidMigration);
            }
            let stable = stable_key(&namespace, &section, key, payload)?;
            let event_time = event_time
                .as_deref()
                .map(|value| normalize(value, true).ok_or(StoreError::InvalidMigration))
                .transpose()?;
            insert.execute(params![
                namespace, section, stable, ordinal, event_time, payload
            ])?;
        }
    }
    tx.execute_batch(
        "DELETE FROM history_items;
         INSERT INTO history_items SELECT * FROM lam_rekey ORDER BY rowid;
         DROP TABLE lam_rekey;",
    )?;
    Ok(())
}

fn stable_key(namespace: &str, section: &str, key: String, payload: &str) -> Result<String> {
    if (namespace, section) == ("threads", "entries") {
        return Ok(key);
    }
    let keys = identity_fields(namespace, section)?;
    let object: BTreeMap<String, &RawValue> =
        serde_json::from_str(payload).map_err(|_| StoreError::InvalidMigration)?;
    let values = keys
        .iter()
        .map(|key| match object.get(*key) {
            Some(raw) => serde_json::from_str(raw.get()).map_err(|_| StoreError::InvalidMigration),
            None => Ok(Value::Null),
        })
        .collect::<Result<Vec<_>>>()?;
    encode_key(namespace, values)
}

fn identity_fields(namespace: &str, section: &str) -> Result<&'static [&'static str]> {
    const WEB: &[&str] = &["thread_id", "call_id", "index", "timestamp", "tool"];
    Ok(match (namespace, section) {
        ("threads", "skills") => &["thread_id", "call_id", "skill"],
        ("activity", "sql") => &[
            "source",
            "thread_id",
            "call_id",
            "index",
            "file",
            "record_id",
            "record_offset",
            "record_hash",
            "timestamp",
            "statement",
        ],
        ("activity", "web") => WEB,
        ("activity", "mcp") => &[
            "server",
            "thread_id",
            "call_id",
            "index",
            "timestamp",
            "tool",
        ],
        ("errors", "events") => &[
            "timestamp",
            "source",
            "category",
            "severity",
            "code",
            "module",
            "thread_id",
            "call_id",
            "attempt",
            "index",
            "record_id",
        ],
        _ => return Err(StoreError::InvalidMigration),
    })
}

pub(crate) fn projected_key(
    section: crate::Section,
    row: &serde_json::Map<String, Value>,
) -> Result<String> {
    let (namespace, section) = section.names();
    let values = identity_fields(namespace.name(), section)?
        .iter()
        .map(|key| row.get(*key).cloned().unwrap_or(Value::Null))
        .collect();
    encode_key(namespace.name(), values)
}

fn encode_key(namespace: &str, mut values: Vec<Value>) -> Result<String> {
    if namespace == "errors" {
        if let Some(stamp) = values[0].as_str().and_then(|value| normalize(value, false)) {
            values[0] = Value::String(stamp);
        }
    }
    let mut data = String::new();
    encode_identity(&Value::Array(values), &mut data)?;
    Ok(format!("{:x}", Sha256::digest(data.as_bytes())))
}

pub(crate) fn encode_identity(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::String(text) => encode_string(text, output),
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                encode_identity(item, output)?;
            }
            output.push(']');
        }
        Value::Object(items) => {
            output.push('{');
            for (index, (key, item)) in items.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                encode_string(key, output);
                output.push(':');
                encode_identity(item, output)?;
            }
            output.push('}');
        }
        Value::Number(number) => {
            let raw = number.as_str();
            if raw.contains(['.', 'e', 'E']) {
                let value = number.as_f64().ok_or(StoreError::InvalidMigration)?;
                let scientific = format!("{value:e}");
                let (mantissa, exponent) = scientific
                    .split_once('e')
                    .ok_or(StoreError::InvalidMigration)?;
                let exponent = exponent
                    .parse::<i32>()
                    .map_err(|_| StoreError::InvalidMigration)?;
                if (-4..16).contains(&exponent) {
                    let fixed = value.to_string();
                    output.push_str(&fixed);
                    if !fixed.contains('.') {
                        output.push_str(".0");
                    }
                } else {
                    write!(
                        output,
                        "{mantissa}e{}{:02}",
                        if exponent < 0 { '-' } else { '+' },
                        exponent.abs()
                    )
                    .expect("write to String");
                }
            } else {
                output.push_str(if raw == "-0" { "0" } else { raw });
            }
        }
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Null => output.push_str("null"),
    }
    Ok(())
}

fn encode_string(text: &str, output: &mut String) {
    let quoted = serde_json::to_string(text).expect("string JSON");
    for character in quoted.chars() {
        if character < '\u{7f}' {
            output.push(character);
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(output, "\\u{unit:04x}").expect("write to String");
            }
        }
    }
}
