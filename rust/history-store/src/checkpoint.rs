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

#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub cursor: Cursor,
    pub calls: Vec<StoredCall>,
}

pub struct ExpectedCursor<'a> {
    pub source_key: &'a str,
    pub signature: Option<&'a str>,
}

impl Store {
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
            .any(|row| !keys.contains(row.source_key.as_str()))
        {
            return Err(StoreError::InvalidQuery);
        }
        self.commit_checkpoints(cursors, calls, retired, reset, expected, |tx, conflicts| {
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
