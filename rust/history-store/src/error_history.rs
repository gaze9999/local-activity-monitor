use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

use crate::history::write_history;
use crate::legacy::{error_row, retention};
use crate::migration::{encode_identity, projected_key};
use crate::{HistoryWrite, Namespace, Result, Section, StoreError};

pub const ERROR_PREVIEW_LIMIT: usize = 1000;
pub const ERROR_PREVIEW_HOURS: i64 = 24;
const BYTE_LIMIT: usize = 512 * 1024;

pub fn sanitize_error_identifiers(value: &mut Value) {
    if let Some(row) = value.as_object_mut() {
        row.retain(|key, item| {
            !matches!(
                key.as_str(),
                "code" | "request_id" | "trace_id" | "error_type" | "method"
            ) || item
                .as_str()
                .is_none_or(|text| !text.starts_with("sk-") && !text.starts_with("sk_"))
        });
    }
}

pub(crate) fn persist<'a>(
    tx: &Transaction<'_>,
    events: impl Iterator<Item = &'a Value>,
    now: DateTime<Utc>,
) -> Result<()> {
    let ceiling = now + Duration::minutes(1);
    let mut stored = Vec::new();
    for event in events.filter_map(Value::as_object) {
        let Some(row) = error_row(event, None, ceiling) else {
            continue;
        };
        let key = projected_key(Section::Errors, &row)?;
        if row["source"] == "session"
            && !row.contains_key("record_hash")
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM history_items
                 WHERE namespace='errors' AND section='events' AND item_key=?)",
                [&key],
                |row| row.get::<_, bool>(0),
            )?
        {
            // Older Rust checkpoints lack the original record location.
            continue;
        }
        let time = row["timestamp"]
            .as_str()
            .expect("projected timestamp")
            .to_owned();
        let mut payload = String::new();
        encode_identity(&Value::Object(row), &mut payload)?;
        stored.push((key, time, payload));
    }
    if stored.is_empty() {
        return Ok(());
    }
    let days = retention(tx)?;
    let cutoff = (days > 0)
        .then(|| (now - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Millis, true));
    let rows: Vec<_> = stored
        .iter()
        .enumerate()
        .map(|(ordinal, (key, time, payload))| HistoryWrite {
            section: Section::Errors,
            key,
            ordinal: ordinal as i64,
            event_time: Some(time),
            payload,
        })
        .collect();
    write_history(
        tx,
        Namespace::Errors,
        r#"{"version":1}"#,
        now.timestamp_nanos_opt().ok_or(StoreError::InvalidQuery)?,
        cutoff.as_deref(),
        &rows,
    )?;
    if let Some(cutoff) = cutoff {
        tx.execute(
            "DELETE FROM history_items WHERE rowid IN (
                SELECT rowid FROM history_items WHERE namespace='errors' AND section='events'
                AND event_time<? ORDER BY event_time LIMIT 500
             )",
            [cutoff],
        )?;
    }
    Ok(())
}

pub(crate) fn preview(db: &Connection, now: DateTime<Utc>) -> Result<Vec<Value>> {
    let cutoff = now - Duration::hours(ERROR_PREVIEW_HOURS);
    let ceiling = now + Duration::minutes(1);
    let mut query = db.prepare(
        "SELECT CASE WHEN length(CAST(payload AS BLOB))<=? THEN payload END FROM history_items
         WHERE namespace='errors' AND section='events' AND event_time>=? AND event_time<=?
         ORDER BY event_time DESC, item_key DESC LIMIT ?",
    )?;
    let mut rows = query.query(params![
        BYTE_LIMIT as i64,
        cutoff.to_rfc3339_opts(SecondsFormat::Millis, true),
        ceiling.to_rfc3339_opts(SecondsFormat::Millis, true),
        ERROR_PREVIEW_LIMIT as i64,
    ])?;
    let mut remaining = BYTE_LIMIT;
    let mut events = Vec::new();
    while let Some(row) = rows.next()? {
        let value = row.get_ref(0)?;
        let Ok(payload) = value.as_str() else { break };
        let Some(available) = remaining.checked_sub(payload.len()) else {
            break;
        };
        remaining = available;
        let event =
            serde_json::from_str::<Value>(payload).map_err(|_| StoreError::InvalidLegacy)?;
        if let Some(row) = event
            .as_object()
            .and_then(|event| error_row(event, Some(cutoff), ceiling))
        {
            events.push(Value::Object(row));
        }
    }
    Ok(events)
}
