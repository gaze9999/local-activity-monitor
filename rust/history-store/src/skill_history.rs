use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction};
use serde_json::Value;

use crate::history::write_history;
use crate::legacy::skill_entry;
use crate::migration::{encode_identity, projected_key};
use crate::{HistoryWrite, Namespace, Result, Section, Store, StoreError};

pub(crate) fn persist<'a>(
    tx: &Transaction<'_>,
    events: impl Iterator<Item = &'a Value>,
    now: DateTime<Utc>,
) -> Result<()> {
    let mut stored = Vec::new();
    for event in events.filter_map(Value::as_object) {
        let Some(row) = skill_entry(event) else {
            continue;
        };
        let key = projected_key(Section::Skills, &row)?;
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
    let metadata: String = tx
        .query_row(
            "SELECT metadata FROM history_state WHERE namespace='threads'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_else(|| r#"{"version":1}"#.to_owned());
    let rows: Vec<_> = stored
        .iter()
        .enumerate()
        .map(|(ordinal, (key, time, payload))| HistoryWrite {
            section: Section::Skills,
            key,
            ordinal: ordinal as i64,
            event_time: Some(time),
            payload,
        })
        .collect();
    write_history(
        tx,
        Namespace::Threads,
        &metadata,
        now.timestamp_nanos_opt().ok_or(StoreError::InvalidQuery)?,
        None,
        &rows,
    )?;
    tx.execute("DELETE FROM history_items WHERE rowid IN (SELECT rowid FROM history_items WHERE namespace='threads' AND section='skills' ORDER BY event_time DESC, item_key DESC LIMIT -1 OFFSET 500)", [])?;
    Ok(())
}

impl Store {
    pub fn skill_preview(&self) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare("SELECT CASE WHEN length(CAST(payload AS BLOB))<=4096 THEN payload END FROM history_items WHERE namespace='threads' AND section='skills' ORDER BY event_time DESC, item_key DESC LIMIT 500")?;
        let mut rows = statement.query([])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let Some(raw) = row.get::<_, Option<String>>(0)? else {
                continue;
            };
            let value: Value = serde_json::from_str(&raw).map_err(|_| StoreError::InvalidLegacy)?;
            if let Some(skill) = value.as_object().and_then(skill_entry) {
                result.push(Value::Object(skill));
            }
        }
        Ok(result)
    }
}
