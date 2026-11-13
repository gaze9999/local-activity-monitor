use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::aggregate::aggregate;
use crate::error_history;
use crate::history::{prune_batch, write_history, Filter};
use crate::legacy::{activity_row, merge_event, retention, text, Object};
use crate::migration::{encode_identity, projected_key};
use crate::{
    check_paths, normalize_utc_stamp, parse_utc_stamp, Access, Aggregate, Cursor, ExpectedCursor,
    HistoryWrite, Namespace, Result, Section, Store, StoreError, StoredCall,
};
use sha2::{Digest, Sha256};

pub struct ObservationWrite {
    pub source_key: String,
    pub event: Value,
}

pub struct ObservationBatch<'a> {
    pub rows: &'a [ObservationWrite],
    pub errors: &'a [ObservationWrite],
    pub skills: &'a [ObservationWrite],
    pub lifecycle: &'a [ObservationWrite],
    pub now: DateTime<Utc>,
}

fn section(row: &Object) -> Option<Section> {
    match text(row, "server") {
        Some("web") => Some(Section::Web),
        Some(_) => Some(Section::Mcp),
        None if text(row, "engine").is_some() && text(row, "statement").is_some() => {
            Some(Section::Sql)
        }
        None => None,
    }
}

pub fn sql_event_id(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let fields = [
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
    ];
    let mut key = String::from("[");
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            key.push_str(", ");
        }
        encode_identity(object.get(*field).unwrap_or(&Value::Null), &mut key).ok()?;
    }
    key.push(']');
    Some(format!("{:x}", Sha256::digest(key.as_bytes())))
}

fn boundary(days: i64, now: DateTime<Utc>) -> Option<String> {
    (days > 0).then(|| (now - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub(crate) fn persist<'a>(
    tx: &Transaction<'_>,
    events: impl Iterator<Item = &'a Value>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let mut events = events.peekable();
    if events.peek().is_none() {
        return Ok(false);
    }
    let days = retention(tx)?;
    let cutoff = boundary(days, now);
    let ceiling = now + Duration::minutes(1);
    let mut previous = tx.prepare(
        "SELECT payload FROM history_items WHERE namespace='activity' AND section=? AND item_key=?",
    )?;
    let mut rows = Vec::new();
    let mut indexes = HashMap::new();
    for event in events {
        let Some(event) = event.as_object() else {
            continue;
        };
        let Some(section) = section(event) else {
            continue;
        };
        let Some(timestamp) = text(event, "timestamp") else {
            continue;
        };
        let Some(event_time) = parse_utc_stamp(timestamp) else { continue };
        let Some(stamp) = normalize_utc_stamp(timestamp) else {
            continue;
        };
        if cutoff.as_ref().is_some_and(|cutoff| stamp < *cutoff) {
            continue;
        }
        let Some(mut projected) = activity_row(section, event, timestamp) else {
            continue;
        };
        let key = projected_key(section, &projected)?;
        let (_, name) = section.names();
        let existing = tx.query_row("SELECT payload FROM history_items WHERE namespace='activity_pending' AND section=? AND item_key=?", params![name, key], |row| row.get::<_, String>(0)).optional()?;
        if event_time > ceiling {
            let mut combined = existing.map(|value| serde_json::from_str::<Object>(&value).map_err(|_| StoreError::InvalidLegacy)).transpose()?.unwrap_or_default();
            merge_event(&mut combined, projected);
            let mut payload = String::new();
            encode_identity(&Value::Object(combined), &mut payload)?;
            tx.execute("INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload) VALUES('activity_pending',?,?,0,?,?) ON CONFLICT(namespace,section,item_key) DO UPDATE SET event_time=excluded.event_time,payload=excluded.payload", params![name, key, stamp, payload])?;
            continue;
        }
        if let Some(existing) = existing {
            let mut previous = serde_json::from_str::<Object>(&existing).map_err(|_| StoreError::InvalidLegacy)?;
            merge_event(&mut previous, projected);
            projected = previous;
        }
        tx.execute("DELETE FROM history_items WHERE namespace='activity_pending' AND section=? AND item_key=?", params![name, key])?;
        if let Some(&index) = indexes.get(&key) {
            let (_, _, _, item): &mut (Section, String, String, Object) = &mut rows[index];
            merge_event(item, projected);
            continue;
        }
        let stored = previous
            .query_row(params![name, key], |row| row.get::<_, String>(0))
            .optional()?;
        let mut combined = match stored {
            Some(value) => {
                serde_json::from_str::<Object>(&value).map_err(|_| StoreError::InvalidLegacy)?
            }
            None => Object::new(),
        };
        merge_event(&mut combined, projected);
        indexes.insert(key.clone(), rows.len());
        rows.push((section, key, stamp, combined));
    }
    if rows.is_empty() {
        return Ok(false);
    }
    let metadata = tx
        .query_row(
            "SELECT metadata FROM history_state WHERE namespace='activity'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| format!(r#"{{"version":3,"retention_days":{days}}}"#));
    let mut stored = Vec::new();
    for (section, key, stamp, projected) in rows {
        let mut payload = String::new();
        encode_identity(&Value::Object(projected), &mut payload)?;
        stored.push((section, key, stamp, payload));
    }
    let rows: Vec<_> = stored
        .iter()
        .enumerate()
        .map(|(ordinal, (section, key, stamp, payload))| HistoryWrite {
            section: *section,
            key,
            ordinal: ordinal as i64,
            event_time: Some(stamp),
            payload,
        })
        .collect();
    let mut changed = write_history(
        tx,
        Namespace::Activity,
        &metadata,
        now.timestamp_nanos_opt().ok_or(StoreError::InvalidQuery)?,
        cutoff.as_deref(),
        &rows,
    )?;
    if let Some(cutoff) = cutoff {
        changed |= tx.execute(
            "DELETE FROM history_items WHERE rowid IN (
                 SELECT rowid FROM history_items WHERE namespace='activity' AND section IN ('mcp','web','sql')
                 AND event_time<? ORDER BY event_time LIMIT 500
             )",
            [cutoff],
        )? != 0;
    }
    Ok(changed)
}

impl Store {
    pub fn activity_retention(&self) -> Result<i64> {
        retention(&self.db)
    }

    /// Continue a pending cleanup only while its retention setting is current.
    pub fn continue_retention_cleanup(&mut self, days: i64, now: DateTime<Utc>) -> Result<usize> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = if retention(&tx)? == days {
            match boundary(days, now) {
                Some(cutoff) => prune_batch(&tx, &cutoff, 500)?,
                None => 0,
            }
        } else {
            0
        };
        tx.commit()?;
        Ok(removed)
    }

    /// Persist the shared retention setting and prune one bounded batch.
    pub fn set_activity_retention(&mut self, days: i64, now: DateTime<Utc>) -> Result<usize> {
        if !(0..=3650).contains(&days) {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        retention(&tx)?;
        let mut metadata: Object = tx
            .query_row(
                "SELECT metadata FROM history_state WHERE namespace='activity'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|raw| serde_json::from_str(&raw).map_err(|_| StoreError::InvalidLegacy))
            .transpose()?
            .unwrap_or_else(|| Object::from_iter([("version".into(), Value::from(3))]));
        metadata.insert("retention_days".into(), Value::from(days));
        write_history(
            &tx,
            Namespace::Activity,
            &Value::Object(metadata).to_string(),
            now.timestamp_nanos_opt().ok_or(StoreError::InvalidQuery)?,
            None,
            &[],
        )?;
        let removed = match boundary(days, now) {
            Some(cutoff) => prune_batch(&tx, &cutoff, 500)?,
            None => 0,
        };
        tx.commit()?;
        Ok(removed)
    }

    pub fn observed_history(&mut self, now: DateTime<Utc>) -> Result<Vec<Value>> {
        let tx = self.db.transaction()?;
        let cutoff = boundary(retention(&tx)?, now);
        let events = recent(&tx, cutoff.as_deref())?;
        tx.commit()?;
        Ok(events)
    }
}

fn recent(db: &Connection, cutoff: Option<&str>) -> Result<Vec<Value>> {
    let mut events = Vec::new();
    {
        let mut query = db.prepare(
            "SELECT payload FROM history_items WHERE namespace='activity' AND section=?
                 AND (? IS NULL OR event_time>=?) ORDER BY event_time DESC, item_key LIMIT ?",
        )?;
        for (section, limit) in [
            (Section::Mcp, 1000),
            (Section::Web, 1000),
            (Section::Sql, 500),
        ] {
            let (_, name) = section.names();
            let mut rows = query.query(params![name, cutoff, cutoff, limit])?;
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                let raw = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
                bytes += raw.len();
                if bytes > 1024 * 1024 {
                    break;
                }
                let item =
                    serde_json::from_str::<Object>(raw).map_err(|_| StoreError::InvalidLegacy)?;
                if let Some(projected) = text(&item, "timestamp")
                    .and_then(|timestamp| activity_row(section, &item, timestamp))
                {
                    events.push(Value::Object(projected));
                }
            }
        }
    }
    Ok(events)
}

pub fn combine_observations(history: &[Value], recent: impl Iterator<Item = Value>) -> Vec<Value> {
    let mut result = HashMap::<String, Object>::new();
    for event in history.iter().cloned().chain(recent) {
        let Value::Object(event) = event else {
            continue;
        };
        let Some(section) = section(&event) else {
            continue;
        };
        if let Ok(key) = projected_key(section, &event) {
            merge_event(result.entry(key).or_default(), event);
        }
    }
    let mut result: Vec<_> = result.into_iter().collect();
    result.sort_by(|(left_key, left), (right_key, right)| {
        text(right, "timestamp")
            .cmp(&text(left, "timestamp"))
            .then_with(|| left_key.cmp(right_key))
    });
    result
        .into_iter()
        .map(|(_, event)| Value::Object(event))
        .collect()
}

#[derive(Clone)]
pub struct ObservationView {
    pub retention_days: i64,
    pub events: Arc<Vec<Value>>,
    pub sql: [Arc<Aggregate>; 4],
    pub mcp: [Arc<Aggregate>; 4],
}

impl Default for ObservationView {
    fn default() -> Self {
        let empty = Arc::new(Aggregate::default());
        Self {
            retention_days: 90,
            events: Arc::new(Vec::new()),
            sql: std::array::from_fn(|_| Arc::clone(&empty)),
            mcp: std::array::from_fn(|_| Arc::clone(&empty)),
        }
    }
}

struct Cached<T> {
    cutoff: Option<String>,
    value: Arc<T>,
}

/// Read working events and complete aggregates from one SQLite snapshot.
pub struct ObservationStore {
    directory: PathBuf,
    store: Store,
    generation: Option<i64>,
    recent: Option<Cached<Vec<Value>>>,
    sql: [Option<Cached<Aggregate>>; 4],
    mcp: [Option<Cached<Aggregate>>; 4],
    excluded_sources: Vec<String>,
}

impl ObservationStore {
    pub fn next_pending_activity(&self) -> Result<Option<DateTime<Utc>>> {
        check_paths(&self.directory)?;
        let stamp: Option<String> = self.store.db.query_row("SELECT MIN(event_time) FROM history_items WHERE namespace='activity_pending'", [], |row| row.get(0))?;
        Ok(stamp.as_deref().and_then(parse_utc_stamp).map(|time| time - Duration::minutes(1)))
    }

    pub fn process_pending_activity(&mut self, now: DateTime<Utc>) -> Result<bool> {
        check_paths(&self.directory)?;
        let tx = self.store.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let ceiling = (now + Duration::minutes(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut query = tx.prepare("SELECT section,item_key,payload FROM history_items WHERE namespace='activity_pending' AND event_time<=? ORDER BY event_time LIMIT 500")?;
        let rows = query.query_map([ceiling], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(query);
        let mut events = Vec::with_capacity(rows.len());
        let mut ready = Vec::with_capacity(rows.len());
        for (section, key, payload) in &rows {
            let event = serde_json::from_str::<Value>(payload).map_err(|_| StoreError::InvalidLegacy)?;
            let timestamp = event.get("timestamp").and_then(Value::as_str).and_then(parse_utc_stamp).ok_or(StoreError::InvalidLegacy)?;
            if timestamp <= now + Duration::minutes(1) {
                events.push(event);
                ready.push((section, key));
            }
        }
        let changed = persist(&tx, events.iter(), now)?;
        for (section, key) in ready { tx.execute("DELETE FROM history_items WHERE namespace='activity_pending' AND section=? AND item_key=?", params![section,key])?; }
        tx.commit()?;
        if changed { self.generation = None; }
        Ok(changed)
    }

    pub fn skill_preview(&self) -> Result<Vec<Value>> {
        check_paths(&self.directory)?;
        self.store.skill_preview()
    }
    pub fn open(directory: &Path, access: Access) -> Result<Option<Self>> {
        Ok(Store::open(directory, access)?.map(|store| Self {
            directory: directory.to_path_buf(),
            store,
            generation: None,
            recent: None,
            sql: Default::default(),
            mcp: Default::default(),
            excluded_sources: Vec::new(),
        }))
    }

    pub fn save_observation_batch(
        &mut self,
        cursors: &[Cursor],
        calls: &[StoredCall],
        retired: &[(String, String)],
        reset: &[String],
        expected: &[ExpectedCursor<'_>],
        activity: ObservationBatch<'_>,
    ) -> Result<BTreeSet<String>> {
        check_paths(&self.directory)?;
        let (conflicts, changed) = self
            .store
            .save_observation_batch(cursors, calls, retired, reset, expected, activity)?;
        // data_version does not change for commits made on this same connection.
        if changed {
            self.generation = None;
        }
        Ok(conflicts)
    }

    /// Commit SQL and error metadata from the same diagnostic page.
    pub fn save_diagnostics(
        &mut self,
        sql: &[Value],
        errors: &[Value],
        now: DateTime<Utc>,
    ) -> Result<()> {
        check_paths(&self.directory)?;
        let tx = self
            .store
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = persist(&tx, sql.iter(), now)?;
        error_history::persist(&tx, errors.iter(), now)?;
        tx.commit()?;
        if changed {
            self.generation = None;
        }
        Ok(())
    }

    pub fn error_preview(&self, now: DateTime<Utc>) -> Result<Vec<Value>> {
        check_paths(&self.directory)?;
        error_history::preview(&self.store.db, now)
    }

    pub fn read(
        &mut self,
        now: DateTime<Utc>,
        cutoffs: &[Option<DateTime<Utc>>; 4],
    ) -> Result<ObservationView> {
        self.read_filtered(now, cutoffs, &[])
    }

    /// Keep full recent history while excluding selected sources from MCP aggregates.
    pub fn read_filtered(
        &mut self,
        now: DateTime<Utc>,
        cutoffs: &[Option<DateTime<Utc>>; 4],
        excluded_sources: &[&str],
    ) -> Result<ObservationView> {
        Filter::new(Section::Mcp, None, excluded_sources)?;
        let mut normalized: Vec<String> = excluded_sources
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        normalized.sort();
        normalized.dedup();
        if self.excluded_sources != normalized {
            self.mcp = Default::default();
            self.excluded_sources = normalized;
        }
        check_paths(&self.directory)?;
        let previous_generation = self.generation.take();
        let before = self
            .store
            .db
            .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))?;
        let tx = self.store.db.transaction()?;
        // Retention opens the read snapshot before comparing connection-local revisions.
        let retention_days = retention(&tx)?;
        let recent_boundary = boundary(retention_days, now);
        let generation = tx.pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))?;
        if previous_generation != Some(generation) || before != generation {
            self.recent = None;
            self.sql = Default::default();
            self.mcp = Default::default();
        }
        let recent = cached(&tx, &mut self.recent, recent_boundary, None, recent)?;
        let mut view = ObservationView {
            events: recent,
            retention_days,
            ..ObservationView::default()
        };
        for (index, cutoff) in cutoffs.iter().enumerate() {
            let cutoff = cutoff.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
            view.sql[index] = cached(
                &tx,
                &mut self.sql[index],
                cutoff.clone(),
                Some(Section::Sql),
                |db, cutoff| Ok(aggregate(db, &Filter::new(Section::Sql, cutoff, &[])?)?),
            )?;
            view.mcp[index] = cached(
                &tx,
                &mut self.mcp[index],
                cutoff,
                Some(Section::Mcp),
                |db, cutoff| {
                    Ok(aggregate(
                        db,
                        &Filter::new(Section::Mcp, cutoff, excluded_sources)?,
                    )?)
                },
            )?;
        }
        // An external commit during the read cannot invalidate the snapshot,
        // but it prevents these entries from being reused at the next read.
        tx.commit()?;
        let after = self
            .store
            .db
            .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))?;
        self.generation = (before == generation && generation == after).then_some(generation);
        Ok(view)
    }
}

fn cached<T>(
    db: &Connection,
    cache: &mut Option<Cached<T>>,
    cutoff: Option<String>,
    section: Option<Section>,
    load: impl FnOnce(&Connection, Option<&str>) -> Result<T>,
) -> Result<Arc<T>> {
    if let Some(previous) = cache.as_mut() {
        if !boundary_changes(db, previous.cutoff.as_deref(), cutoff.as_deref(), section)? {
            previous.cutoff = cutoff;
            return Ok(Arc::clone(&previous.value));
        }
    }
    let value = Arc::new(load(db, cutoff.as_deref())?);
    *cache = Some(Cached {
        cutoff,
        value: Arc::clone(&value),
    });
    Ok(value)
}

fn boundary_changes(
    db: &Connection,
    previous: Option<&str>,
    current: Option<&str>,
    section: Option<Section>,
) -> rusqlite::Result<bool> {
    if previous == current {
        return Ok(false);
    }
    let (Some(previous), Some(current)) = (previous, current) else {
        return Ok(true);
    };
    let (first, last) = if previous < current {
        (previous, current)
    } else {
        (current, previous)
    };
    if let Some(section) = section {
        let (_, name) = section.names();
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM history_items WHERE namespace='activity' AND section=?
             AND event_time>=? AND event_time<?)",
            params![name, first, last],
            |row| row.get(0),
        )
    } else {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM history_items WHERE namespace='activity'
             AND section IN ('sql','mcp','web') AND event_time>=? AND event_time<?)",
            params![first, last],
            |row| row.get(0),
        )
    }
}
