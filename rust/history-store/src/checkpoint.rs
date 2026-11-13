use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{BTreeSet, HashSet};

use crate::{error_history, observations, ObservationBatch, Result, Store, StoreError};

const CHECKPOINT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub source_key: String,
    pub offset: i64,
    pub signature: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCall {
    pub source_key: String,
    pub call_id: String,
    pub event_time: Option<String>,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHeader {
    pub call_id: String,
    pub event_time: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub cursor: Cursor,
    pub calls: Vec<StoredCall>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckpointBatch {
    pub checkpoints: Vec<Checkpoint>,
    pub incomplete_sources: Vec<String>,
}

pub struct ExpectedCursor<'a> {
    pub source_key: &'a str,
    pub signature: Option<&'a str>,
}

impl Store {
    /// Read selected calls and their cursor from one committed checkpoint snapshot.
    pub fn selected_checkpoint(&mut self, expected: &ExpectedCursor<'_>, call_ids: &[String]) -> Result<Option<Checkpoint>> {
        let Some(signature) = expected.signature else { return Err(StoreError::InvalidQuery) };
        if expected.source_key.is_empty() || expected.source_key.len() > 256 || signature.len() > 512
            || call_ids.len() > 8192 || call_ids.iter().any(|id| id.is_empty() || id.len() > 160)
            || call_ids.iter().collect::<HashSet<_>>().len() != call_ids.len()
        {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self.db.transaction()?;
        let cursor = tx.query_row("SELECT offset,signature,payload FROM session_cursors WHERE source_key=?", [expected.source_key], |row| {
            let signature = row.get_ref(1)?.as_str()?;
            let payload = row.get_ref(2)?.as_str()?;
            if signature.len() > 512 || payload.len() > CHECKPOINT_LIMIT { return Err(rusqlite::Error::InvalidQuery); }
            Ok(Cursor { source_key:expected.source_key.to_owned(), offset:row.get(0)?, signature:signature.to_owned(), payload:payload.to_owned() })
        }).optional()?;
        let Some(cursor) = cursor.filter(|cursor| cursor.signature == signature) else {
            tx.commit()?;
            return Ok(None);
        };
        let mut bytes = cursor.payload.len() + cursor.signature.len() + cursor.source_key.len();
        let mut calls = Vec::with_capacity(call_ids.len());
        {
            let mut query = tx.prepare("SELECT event_time,payload FROM session_calls WHERE source_key=? AND call_id=?")?;
            for id in call_ids {
                let result = query.query_row(params![expected.source_key, id], |row| {
                    let time = match row.get_ref(0)? { rusqlite::types::ValueRef::Null => None, value => Some(value.as_str()?) };
                    let payload = row.get_ref(1)?.as_str()?;
                    if time.is_some_and(|value| value.len() > 40) { return Err(rusqlite::Error::InvalidQuery); }
                    bytes = bytes.checked_add(id.len() + payload.len() + expected.source_key.len() + time.map_or(0, str::len)).ok_or(rusqlite::Error::InvalidQuery)?;
                    if bytes > CHECKPOINT_LIMIT { return Err(rusqlite::Error::InvalidQuery); }
                    Ok(StoredCall { source_key:expected.source_key.to_owned(), call_id:id.clone(), event_time:time.map(str::to_owned), payload:payload.to_owned() })
                }).optional()?;
                let Some(call) = result else { return Ok(None) };
                calls.push(call);
            }
        }
        tx.commit()?;
        Ok(Some(Checkpoint { cursor, calls }))
    }

    /// Visit selected payloads in bounded batches from one SQLite read snapshot.
    ///
    /// Unlike `selected_checkpoint`, the total selection may exceed 16 MiB. Each
    /// cursor plus batch remains bounded by that limit, and a batch contains at
    /// most 512 calls. Selection order is preserved. Consumers must stage their
    /// result until this method succeeds: an I/O or callback error can occur
    /// after earlier batches have been visited. No callback runs for a missing
    /// cursor, stale signature, missing selected call, or oversized record.
    pub fn visit_selected_checkpoint(
        &mut self,
        expected: &ExpectedCursor<'_>,
        call_ids: &[String],
        mut visit: impl FnMut(&Cursor, &[StoredCall]) -> Result<()>,
    ) -> Result<bool> {
        let Some(signature) = expected.signature else { return Err(StoreError::InvalidQuery) };
        if expected.source_key.is_empty() || expected.source_key.len() > 256 || signature.len() > 512
            || call_ids.len() > 8192 || call_ids.iter().any(|id| id.is_empty() || id.len() > 160)
            || call_ids.iter().collect::<HashSet<_>>().len() != call_ids.len()
        {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self.db.transaction()?;
        let cursor = tx.query_row(
            "SELECT offset,signature,payload FROM session_cursors WHERE source_key=?",
            [expected.source_key], |row| {
                let signature = row.get_ref(1)?.as_str()?;
                let payload = row.get_ref(2)?.as_str()?;
                if signature.len() > 512 || payload.len() > CHECKPOINT_LIMIT {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(Cursor { source_key: expected.source_key.to_owned(), offset: row.get(0)?,
                    signature: signature.to_owned(), payload: payload.to_owned() })
            },
        ).optional()?;
        let Some(cursor) = cursor.filter(|cursor| cursor.signature == signature) else {
            return Ok(false);
        };
        let header_bytes = cursor.source_key.len() + cursor.signature.len() + cursor.payload.len();
        if header_bytes > CHECKPOINT_LIMIT { return Err(StoreError::InvalidQuery); }
        // Preflight metadata without copying payloads. The transaction keeps this
        // validation and all subsequent payload pages on the same snapshot.
        {
            let mut query = tx.prepare("SELECT event_time,length(CAST(payload AS BLOB)) FROM session_calls WHERE source_key=? AND call_id=?")?;
            for id in call_ids {
                let size = query.query_row(params![expected.source_key, id], |row| {
                    let time = match row.get_ref(0)? { rusqlite::types::ValueRef::Null => None, value => Some(value.as_str()?) };
                    let payload_bytes: i64 = row.get(1)?;
                    if time.is_some_and(|value| value.len() > 40) || payload_bytes < 0 {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok(payload_bytes as u64 + id.len() as u64 + expected.source_key.len() as u64 + time.map_or(0, str::len) as u64)
                }).optional()?;
                let Some(size) = size else { return Ok(false) };
                if size > (CHECKPOINT_LIMIT - header_bytes) as u64 { return Err(StoreError::InvalidQuery); }
            }
        }
        {
            let mut query = tx.prepare("SELECT event_time,payload FROM session_calls WHERE source_key=? AND call_id=?")?;
            let mut calls = Vec::new();
            let mut bytes = header_bytes;
            for id in call_ids {
                let mut rows = query.query(params![expected.source_key, id])?;
                let row = rows.next()?.ok_or(StoreError::InvalidQuery)?;
                let time = match row.get_ref(0)? { rusqlite::types::ValueRef::Null => None, value => Some(value.as_str().map_err(rusqlite::Error::from)?) };
                let payload = row.get_ref(1)?.as_str().map_err(rusqlite::Error::from)?;
                let size = id.len() + expected.source_key.len() + time.map_or(0, str::len) + payload.len();
                if calls.len() == 512 || bytes + size > CHECKPOINT_LIMIT {
                    visit(&cursor, &calls)?;
                    calls.clear();
                    bytes = header_bytes;
                }
                calls.push(StoredCall { source_key: expected.source_key.to_owned(), call_id: id.clone(),
                    event_time: time.map(str::to_owned), payload: payload.to_owned() });
                bytes += size;
            }
            if !calls.is_empty() || call_ids.is_empty() { visit(&cursor, &calls)?; }
        }
        tx.commit()?;
        Ok(true)
    }

    /// Read only ordering keys for one matching checkpoint, without loading call payloads.
    pub fn checkpoint_call_headers(
        &mut self,
        expected: &ExpectedCursor<'_>,
        after: Option<&CallHeader>,
        limit: u32,
    ) -> Result<Option<Vec<CallHeader>>> {
        let Some(signature) = expected.signature else { return Err(StoreError::InvalidQuery) };
        if expected.source_key.is_empty() || expected.source_key.len() > 256 || signature.len() > 512
            || !(1..=512).contains(&limit)
            || after.is_some_and(|cursor| cursor.call_id.is_empty() || cursor.call_id.len() > 160
                || cursor.event_time.as_ref().is_some_and(|time| time.len() > 40))
        {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self.db.transaction()?;
        let current: Option<String> = tx.query_row(
            "SELECT CASE WHEN length(CAST(signature AS BLOB))<=512 THEN signature END FROM session_cursors WHERE source_key=?",
            [expected.source_key], |row| row.get(0),
        ).optional()?.flatten();
        if current.as_deref() != Some(signature) {
            tx.commit()?;
            return Ok(None);
        }
        let sql = if after.is_some() {
            "SELECT call_id,event_time FROM session_calls WHERE source_key=?1 AND ((?2 IS NULL AND event_time IS NULL AND call_id>?3) OR (?2 IS NOT NULL AND (event_time IS NULL OR event_time<?2 OR (event_time=?2 AND call_id>?3)))) ORDER BY event_time DESC,call_id LIMIT ?4"
        } else {
            "SELECT call_id,event_time FROM session_calls WHERE source_key=?1 ORDER BY event_time DESC,call_id LIMIT ?2"
        };
        let mut headers = Vec::new();
        {
            let mut statement = tx.prepare(sql)?;
            let mut rows = if let Some(cursor) = after {
                statement.query(params![expected.source_key, cursor.event_time, cursor.call_id, limit])?
            } else {
                statement.query(params![expected.source_key, limit])?
            };
            while let Some(row) = rows.next()? {
                let call_id = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
                let event_time = match row.get_ref(1)? {
                    rusqlite::types::ValueRef::Null => None,
                    value => Some(value.as_str().map_err(rusqlite::Error::from)?),
                };
                if call_id.is_empty() || call_id.len() > 160 || event_time.is_some_and(|time| time.len() > 40) {
                    return Err(StoreError::InvalidQuery);
                }
                headers.push(CallHeader { call_id: call_id.to_owned(), event_time: event_time.map(str::to_owned) });
            }
        }
        tx.commit()?;
        Ok(Some(headers))
    }

    /// Retain matching cursor data when the selected calls exceed the budget.
    pub fn matching_checkpoints_with_partials(&mut self, expected: &[ExpectedCursor<'_>], call_budget: u32) -> Result<(Vec<(Checkpoint, bool)>, Vec<String>)> {
        if expected.iter().any(|entry| entry.signature.is_none()) { return Err(StoreError::InvalidQuery); }
        let keys: Vec<_> = expected.iter().map(|entry| entry.source_key.to_owned()).collect();
        let batch = self.checkpoints(&keys, call_budget)?;
        let incomplete: HashSet<_> = batch.incomplete_sources.into_iter().collect();
        let mut checkpoints: std::collections::BTreeMap<_, _> = batch.checkpoints.into_iter().map(|value| (value.cursor.source_key.clone(), value)).collect();
        let mut accepted = Vec::new();
        let mut excluded = Vec::new();
        for entry in expected {
            match checkpoints.remove(entry.source_key) {
                Some(mut checkpoint) if Some(checkpoint.cursor.signature.as_str()) == entry.signature => {
                    let partial = incomplete.contains(entry.source_key);
                    if partial { checkpoint.calls.clear(); }
                    accepted.push((checkpoint, partial));
                }
                _ => excluded.push(entry.source_key.to_owned()),
            }
        }
        Ok((accepted, excluded))
    }
    pub fn checkpoint_signature(&self, source_key: &str) -> Result<Option<String>> {
        if source_key.is_empty() || source_key.len() > 256 { return Err(StoreError::InvalidQuery); }
        Ok(self.db.query_row("SELECT CASE WHEN length(CAST(signature AS BLOB))<=512 THEN signature END FROM session_cursors WHERE source_key=?", [source_key], |row| row.get(0)).optional()?.flatten())
    }
    /// Exclude missing, changed, or call-truncated checkpoints from a selected batch.
    pub fn matching_checkpoints(&mut self, expected: &[ExpectedCursor<'_>], call_budget: u32) -> Result<(Vec<Checkpoint>, Vec<String>)> {
        if expected.iter().any(|entry| entry.signature.is_none()) { return Err(StoreError::InvalidQuery); }
        let keys: Vec<_> = expected.iter().map(|entry| entry.source_key.to_owned()).collect();
        let batch = self.checkpoints(&keys, call_budget)?;
        let mut accepted = Vec::new();
        let mut excluded = Vec::new();
        let mut checkpoints: std::collections::BTreeMap<_, _> = batch.checkpoints.into_iter().map(|value| (value.cursor.source_key.clone(), value)).collect();
        let incomplete: HashSet<_> = batch.incomplete_sources.into_iter().collect();
        for entry in expected {
            match checkpoints.remove(entry.source_key) {
                Some(checkpoint) if !incomplete.contains(entry.source_key) && Some(checkpoint.cursor.signature.as_str()) == entry.signature => accepted.push(checkpoint),
                _ => excluded.push(entry.source_key.to_owned()),
            }
        }
        Ok((accepted, excluded))
    }

    /// Read a bounded list of selected checkpoints from one committed snapshot.
    pub fn checkpoints(&mut self, source_keys: &[String], call_budget: u32) -> Result<CheckpointBatch> {
        if source_keys.len() > 64 || call_budget > 50_000 || source_keys.iter().any(|key| key.is_empty() || key.len() > 256) || source_keys.iter().collect::<HashSet<_>>().len() != source_keys.len() {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self.db.transaction()?;
        let mut result = CheckpointBatch { checkpoints: Vec::new(), incomplete_sources: Vec::new() };
        let mut remaining = call_budget as usize;
        let mut bytes = 0usize;
        {
            let mut cursor_query = tx.prepare("SELECT offset,signature,payload FROM session_cursors WHERE source_key=?")?;
            let mut calls_query = tx.prepare("SELECT call_id,event_time,payload FROM session_calls WHERE source_key=? ORDER BY event_time DESC,call_id LIMIT ?")?;
            for key in source_keys {
                let row = cursor_query.query_row([key], |row| {
                    let signature = row.get_ref(1)?.as_str()?;
                    let payload = row.get_ref(2)?.as_str()?;
                    if signature.len() > 512 || payload.len() > CHECKPOINT_LIMIT { return Err(rusqlite::Error::InvalidQuery); }
                    Ok(Cursor { source_key:key.clone(), offset:row.get(0)?, signature:signature.to_owned(), payload:payload.to_owned() })
                }).optional()?;
                let Some(cursor) = row else { continue };
                bytes = bytes.checked_add(cursor.payload.len() + cursor.signature.len() + cursor.source_key.len()).ok_or(StoreError::InvalidQuery)?;
                if bytes > CHECKPOINT_LIMIT { return Err(StoreError::InvalidQuery); }
                let limit = remaining.min(8192);
                let mut query = calls_query.query(params![key, (limit + 1) as u32])?;
                let mut calls = Vec::new();
                while let Some(row) = query.next()? {
                    if calls.len() == limit { result.incomplete_sources.push(key.clone()); break; }
                    let id = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
                    let payload = row.get_ref(2)?.as_str().map_err(rusqlite::Error::from)?;
                    let time = match row.get_ref(1)? {
                        rusqlite::types::ValueRef::Null => None,
                        value => Some(value.as_str().map_err(rusqlite::Error::from)?),
                    };
                    if id.len() > 160 || time.is_some_and(|value| value.len() > 40) { return Err(StoreError::InvalidQuery); }
                    let next_bytes = bytes.checked_add(id.len() + payload.len() + key.len() + time.map_or(0,str::len)).ok_or(StoreError::InvalidQuery)?;
                    if next_bytes > CHECKPOINT_LIMIT {
                        result.incomplete_sources.push(key.clone());
                        break;
                    }
                    bytes = next_bytes;
                    calls.push(StoredCall { source_key:key.clone(), call_id:id.to_owned(), event_time:time.map(str::to_owned), payload:payload.to_owned() });
                }
                remaining -= calls.len();
                result.checkpoints.push(Checkpoint { cursor, calls });
            }
        }
        tx.commit()?;
        Ok(result)
    }
    /// Read and revalidate the Python lifecycle entry for one observed session filename.
    pub fn thread_lifecycle(&self, filename: &str) -> Result<Option<serde_json::Value>> {
        let payload: Option<String> = self.db.query_row(
            "SELECT CASE WHEN length(CAST(payload AS BLOB))<=524288 THEN payload END FROM history_items WHERE namespace='threads' AND section='entries' AND item_key=?",
            [filename], |row| row.get(0),
        ).optional()?.flatten();
        let Some(payload) = payload else { return Ok(None) };
        let value: serde_json::Value = serde_json::from_str(&payload).map_err(|_| StoreError::InvalidLegacy)?;
        Ok(value.as_object().and_then(|object| crate::legacy::thread_entry(filename, object)).map(serde_json::Value::Object))
    }

    /// Read the cursor and calls from a single committed snapshot.
    pub fn checkpoint(&mut self, source_key: &str, call_limit: u32) -> Result<Option<Checkpoint>> {
        let tx = self.db.transaction()?;
        let cursor = tx
            .query_row(
                "SELECT offset, signature, payload FROM session_cursors WHERE source_key=?",
                [source_key],
                |row| {
                    let signature = row.get_ref(1)?.as_str()?;
                    let payload = row.get_ref(2)?.as_str()?;
                    if signature.len() > 512 || payload.len() > CHECKPOINT_LIMIT {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok(Cursor {
                        source_key: source_key.into(),
                        offset: row.get(0)?,
                        signature: signature.into(),
                        payload: payload.into(),
                    })
                },
            )
            .optional()?;
        let Some(cursor) = cursor else {
            return Ok(None);
        };
        let calls = {
            let mut statement = tx.prepare(
                "SELECT call_id, event_time, payload FROM session_calls
                 WHERE source_key=? ORDER BY event_time DESC, call_id LIMIT ?",
            )?;
            let mut rows = statement.query(params![source_key, call_limit.min(8192)])?;
            let mut calls = Vec::new();
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                let payload = row.get_ref(2)?.as_str().map_err(rusqlite::Error::from)?;
                bytes += payload.len();
                if bytes > CHECKPOINT_LIMIT {
                    return Err(StoreError::InvalidQuery);
                }
                calls.push(StoredCall {
                    source_key: source_key.into(),
                    call_id: row.get(0)?,
                    event_time: row.get(1)?,
                    payload: payload.into(),
                });
            }
            calls
        };
        tx.commit()?;
        Ok(Some(Checkpoint { cursor, calls }))
    }

    /// Persist only projected metadata. One transaction advances all cursors,
    /// upserts changed calls and removes retired calls.
    pub fn save_checkpoints(
        &mut self,
        cursors: &[Cursor],
        calls: &[StoredCall],
        retired: &[(String, String)],
    ) -> Result<()> {
        self.save_checkpoint_batch(cursors, calls, retired, &[], &[])
            .map(|_| ())
    }

    /// Replace invalidated source calls, leaving keys changed by another writer intact.
    pub fn save_checkpoint_batch(
        &mut self,
        cursors: &[Cursor],
        calls: &[StoredCall],
        retired: &[(String, String)],
        reset: &[String],
        expected: &[ExpectedCursor<'_>],
    ) -> Result<BTreeSet<String>> {
        self.commit_checkpoints(cursors, calls, retired, reset, expected, |_, _| Ok(()))
            .map(|(conflicts, ())| conflicts)
    }

    /// Commit selected activity with its checkpoint, excluding conflicting source keys.
    pub fn save_observation_batch(
        &mut self,
        cursors: &[Cursor],
        calls: &[StoredCall],
        retired: &[(String, String)],
        reset: &[String],
        expected: &[ExpectedCursor<'_>],
        activity: ObservationBatch<'_>,
    ) -> Result<(BTreeSet<String>, bool)> {
        let keys = cursors
            .iter()
            .map(|row| row.source_key.as_str())
            .collect::<HashSet<_>>();
        if activity
            .rows
            .iter()
            .chain(activity.errors)
            .chain(activity.skills)
            .chain(activity.lifecycle)
            .any(|row| !keys.contains(row.source_key.as_str()))
        {
            return Err(StoreError::InvalidQuery);
        }
        self.commit_checkpoints(cursors, calls, retired, reset, expected, |tx, conflicts| {
            persist_lifecycle(tx, activity.lifecycle.iter().filter(|row| !conflicts.contains(&row.source_key)).map(|row| &row.event), activity.now)?;
            crate::skill_history::persist(
                tx,
                activity
                    .skills
                    .iter()
                    .filter(|row| !conflicts.contains(&row.source_key))
                    .map(|row| &row.event),
                activity.now,
            )?;
            error_history::persist(
                tx,
                activity
                    .errors
                    .iter()
                    .filter(|row| !conflicts.contains(&row.source_key))
                    .map(|row| &row.event),
                activity.now,
            )?;
            observations::persist(
                tx,
                activity
                    .rows
                    .iter()
                    .filter(|row| !conflicts.contains(&row.source_key))
                    .map(|row| &row.event),
                activity.now,
            )
        })
    }

    fn commit_checkpoints<T: Default>(
        &mut self,
        cursors: &[Cursor],
        calls: &[StoredCall],
        retired: &[(String, String)],
        reset: &[String],
        expected: &[ExpectedCursor<'_>],
        after: impl FnOnce(&Transaction<'_>, &BTreeSet<String>) -> Result<T>,
    ) -> Result<(BTreeSet<String>, T)> {
        if cursors.iter().any(|cursor| cursor.offset < 0) {
            return Err(StoreError::InvalidQuery);
        }
        if cursors.is_empty() {
            return if calls.is_empty()
                && retired.is_empty()
                && reset.is_empty()
                && expected.is_empty()
            {
                Ok((BTreeSet::new(), T::default()))
            } else {
                Err(StoreError::InvalidQuery)
            };
        }
        if !reset.is_empty() || !expected.is_empty() {
            let keys = cursors
                .iter()
                .map(|cursor| cursor.source_key.as_str())
                .collect::<HashSet<_>>();
            if reset.iter().any(|source| !keys.contains(source.as_str()))
                || expected
                    .iter()
                    .any(|cursor| !keys.contains(cursor.source_key))
            {
                return Err(StoreError::InvalidQuery);
            }
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut conflicts = BTreeSet::new();
        {
            let mut inspect =
                tx.prepare("SELECT signature FROM session_cursors WHERE source_key=?")?;
            for cursor in expected {
                let same = inspect
                    .query_row([cursor.source_key], |row| {
                        Ok(row.get_ref(0)?.as_str().ok() == cursor.signature
                            && cursor.signature.is_some())
                    })
                    .optional()?
                    .unwrap_or_else(|| cursor.signature.is_none());
                if !same {
                    conflicts.insert(cursor.source_key.to_owned());
                }
            }
            let mut clear = tx.prepare("DELETE FROM session_calls WHERE source_key=?")?;
            for source in reset.iter().filter(|source| !conflicts.contains(*source)) {
                clear.execute([source])?;
            }
            let mut update = tx.prepare(
                "INSERT INTO session_cursors VALUES(?,?,?,?)
                 ON CONFLICT(source_key) DO UPDATE SET offset=excluded.offset,
                 signature=excluded.signature, payload=excluded.payload
                 WHERE session_cursors.offset!=excluded.offset OR session_cursors.signature!=excluded.signature
                 OR session_cursors.payload!=excluded.payload",
            )?;
            for row in cursors
                .iter()
                .filter(|row| !conflicts.contains(&row.source_key))
            {
                update.execute(params![
                    row.source_key,
                    row.offset,
                    row.signature,
                    row.payload
                ])?;
            }
            let mut update = tx.prepare(
                "INSERT INTO session_calls VALUES(?,?,?,?) ON CONFLICT(source_key,call_id)
                 DO UPDATE SET event_time=excluded.event_time,payload=excluded.payload
                 WHERE session_calls.payload!=excluded.payload",
            )?;
            for row in calls
                .iter()
                .filter(|row| !conflicts.contains(&row.source_key))
            {
                update.execute(params![
                    row.source_key,
                    row.call_id,
                    row.event_time,
                    row.payload
                ])?;
            }
            let mut remove =
                tx.prepare("DELETE FROM session_calls WHERE source_key=? AND call_id=?")?;
            for (source, call) in retired
                .iter()
                .filter(|(source, _)| !conflicts.contains(source))
            {
                remove.execute(params![source, call])?;
            }
        }
        let result = after(&tx, &conflicts)?;
        tx.commit()?;
        Ok((conflicts, result))
    }
}

fn persist_lifecycle<'a>(tx: &Transaction<'_>, events: impl Iterator<Item = &'a serde_json::Value>, now: chrono::DateTime<chrono::Utc>) -> Result<()> {
    use crate::history::write_history;
    use crate::{HistoryWrite, Namespace, Section};
    let mut stored = Vec::new();
    for value in events.filter_map(serde_json::Value::as_object) {
        let Some(filename) = value.get("source_file").and_then(serde_json::Value::as_str) else { continue };
        let Some(projected) = crate::legacy::thread_entry(filename, value) else { continue };
        let time = projected["task_time"].as_str().expect("validated time").to_owned();
        let time = crate::normalize_utc_stamp(&time).ok_or(StoreError::InvalidQuery)?;
        let existing: Option<(String, String)> = tx.query_row("SELECT event_time,payload FROM history_items WHERE namespace='threads' AND section='entries' AND item_key=?", [filename], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        if let Some((previous_time, previous_payload)) = existing {
            let previous: serde_json::Value = serde_json::from_str(&previous_payload).map_err(|_| StoreError::InvalidLegacy)?;
            if previous["thread_id"] == projected["thread_id"] && previous_time > time { continue; }
        }
        let mut payload = String::new();
        crate::migration::encode_identity(&serde_json::Value::Object(projected), &mut payload)?;
        stored.push((filename.to_owned(), time, payload));
    }
    if stored.is_empty() { return Ok(()); }
    let metadata: String = tx.query_row("SELECT metadata FROM history_state WHERE namespace='threads'", [], |row| row.get(0)).optional()?.unwrap_or_else(|| r#"{"version":1}"#.to_owned());
    let rows: Vec<_> = stored.iter().enumerate().map(|(ordinal,(key,time,payload))| HistoryWrite { section:Section::Threads,key,ordinal:ordinal as i64,event_time:Some(time),payload }).collect();
    write_history(tx, Namespace::Threads, &metadata, now.timestamp_nanos_opt().ok_or(StoreError::InvalidQuery)?, None, &rows)?;
    Ok(())
}
