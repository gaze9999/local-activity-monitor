use super::project::{now, stamp, Call, Projection};
use super::{file_identity, redirected, storage_error_type, Session};
use chrono::{DateTime, Utc};
use lam_history_store::{
    Access, Checkpoint, Cursor, ExpectedCursor, ObservationBatch, ObservationStore,
    ObservationView, ObservationWrite, Store, StoreError, StoredCall, FILE_NAME,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const BYTE_LIMIT: usize = 16 * 1024 * 1024;
const SAMPLE: u64 = 256;
// Version 17 includes plain execution modes, so older cursors must replay their source.
const STATE_VERSION: u32 = 17;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub offset: u64,
    identity: (u64, u64),
    digest: [u8; 32],
}

impl Anchor {
    pub(super) fn verify(&self, file: &mut File, identity: (u64, u64)) -> io::Result<bool> {
        if self.identity != identity || file.metadata()?.len() < self.offset {
            return Ok(false);
        }
        let size = self.offset.min(SAMPLE) as usize;
        let mut data = [0; 2 * SAMPLE as usize];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut data[..size])?;
        file.seek(SeekFrom::Start(self.offset - size as u64))?;
        file.read_exact(&mut data[size..size * 2])?;
        Ok(<[u8; 32]>::from(Sha256::digest(&data[..size * 2])) == self.digest)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VersionedAnchor {
    anchor: Anchor,
    revision: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State<P> {
    version: u32,
    discard: bool,
    projection: P,
}

pub struct Restored {
    pub anchor: Anchor,
    pub discard: bool,
    pub projection: Projection,
}

#[derive(Clone)]
pub struct SnapshotProjection {
    pub projection: Projection,
    pub calls_incomplete: bool,
    pub source_details_pending: bool,
    pub workdir_pending: bool,
    pub partial: bool,
    pub source_key: String,
    pub anchor: Anchor,
    pub checkpoint_signature: String,
}

// Call only for a checkpoint matched against the current source-membership revision.
// Incomplete optional features remain valid while those features are disabled; the
// membership revision is invalidated before an observation is re-enabled.
pub fn snapshot_projection(checkpoint: Checkpoint, calls_incomplete: bool) -> Option<SnapshotProjection> {
    let anchor = serde_json::from_str::<VersionedAnchor>(&checkpoint.cursor.signature)
        .map(|value| value.anchor)
        .or_else(|_| serde_json::from_str::<Anchor>(&checkpoint.cursor.signature))
        .ok()?;
    if i64::try_from(anchor.offset).ok()? != checkpoint.cursor.offset {
        return None;
    }
    let state: State<Projection> = serde_json::from_str(&checkpoint.cursor.payload).ok()?;
    if state.version != STATE_VERSION {
        return None;
    }
    let calls = checkpoint
        .calls
        .into_iter()
        .map(|row| {
            serde_json::from_str::<Call>(&row.payload)
                .ok()
                .map(|call| (row.call_id, call))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    if calls.values().any(|call| {
        [
            call.request_position.as_ref(),
            call.response_position.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|position| position.offset >= anchor.offset)
    }) {
        return None;
    }
    let workdir_pending = state.projection.workdir_position.is_some();
    let source_details_pending = calls_incomplete || workdir_pending
        || calls
            .values()
            .any(|call| call.request_position.is_some() || call.response_position.is_some())
        || state
            .projection
            .errors
            .iter()
            .any(|error| error.record_offset.is_some());
    if state
        .projection
        .workdir_position
        .as_ref()
        .is_some_and(|position| !position.valid() || position.offset >= anchor.offset)
        || state.projection.errors.iter().any(|error| {
            error
                .record_offset
                .is_some_and(|offset| offset >= anchor.offset)
        })
    {
        return None;
    }
    let mut projection = state.projection;
    if !projection.restore_calls(calls) {
        return None;
    }
    Some(SnapshotProjection {
        projection,
        calls_incomplete,
        source_details_pending,
        workdir_pending,
        partial: state.discard || calls_incomplete,
        source_key: checkpoint.cursor.source_key,
        anchor,
        checkpoint_signature: checkpoint.cursor.signature,
    })
}

#[derive(Default)]
pub struct Loaded {
    pub signature: Option<String>,
    pub state: Option<Restored>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Report {
    load_health: &'static str,
    write_health: &'static str,
    bytes_read: u64,
    restored: u64,
    invalidated: u64,
}

pub struct Checkpoints {
    directory: PathBuf,
    reader: Option<Store>,
    attempted: bool,
    prefetched: BTreeMap<String, Checkpoint>,
    prefetched_keys: BTreeSet<String>,
    observation_store: Option<((u64, u64), Access, ObservationStore)>,
    pub report: Report,
    pub error_write_error: Option<&'static str>,
}

pub(super) fn key(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.to_str()?;
    Some(format!("rust:v1:{:x}", Sha256::digest(relative.as_bytes())))
}

impl Checkpoints {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn snapshot_reader(&self) -> lam_history_store::Result<Option<Store>> {
        let Some(before) = self.storage_identity()? else {
            return Ok(None);
        };
        let store = Store::open(&self.directory, Access::ReadOnly)?;
        if self.storage_identity()? != Some(before) {
            return Err(StoreError::UnsafePath);
        }
        Ok(store)
    }
    pub fn legacy_lifecycle(&self, path: &Path) -> Option<Value> {
        let filename = path.file_name()?.to_str()?;
        self.reader
            .as_ref()?
            .thread_lifecycle(filename)
            .ok()
            .flatten()
    }
    pub fn new(directory: PathBuf, enabled: bool) -> Self {
        let health = if enabled { "missing" } else { "disabled" };
        Self {
            directory,
            reader: None,
            attempted: false,
            prefetched: BTreeMap::new(),
            prefetched_keys: BTreeSet::new(),
            observation_store: None,
            error_write_error: None,
            report: Report {
                load_health: health,
                write_health: health,
                bytes_read: 0,
                restored: 0,
                invalidated: 0,
            },
        }
    }

    pub fn begin_round(&mut self) {
        self.reader = None;
        self.attempted = false;
        self.prefetched.clear();
        self.prefetched_keys.clear();
    }

    pub fn prefetch(&mut self, root: &Path, paths: &[PathBuf]) {
        self.prefetched.clear();
        self.prefetched_keys.clear();
        let keys: Vec<_> = paths.iter().filter_map(|path| key(root, path)).collect();
        if keys.is_empty() {
            return;
        }
        if !self.attempted {
            self.attempted = true;
            match Store::open(&self.directory, Access::ReadOnly) {
                Ok(reader) => self.reader = reader,
                Err(_) => {
                    self.report.load_health = "unavailable";
                    return;
                }
            }
        }
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let Ok(batch) = reader.checkpoints(&keys, 50_000) else {
            return;
        };
        self.prefetched_keys.extend(keys);
        for incomplete in batch.incomplete_sources {
            self.prefetched_keys.remove(&incomplete);
        }
        for checkpoint in batch.checkpoints {
            if self.prefetched_keys.contains(&checkpoint.cursor.source_key) {
                self.prefetched
                    .insert(checkpoint.cursor.source_key.clone(), checkpoint);
            }
        }
    }

    pub fn sample(
        &mut self,
        file: &mut File,
        identity: (u64, u64),
        offset: u64,
    ) -> io::Result<Anchor> {
        let size = offset.min(SAMPLE) as usize;
        let mut data = [0; 2 * SAMPLE as usize];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut data[..size])?;
        self.report.bytes_read += size as u64;
        file.seek(SeekFrom::Start(offset - size as u64))?;
        file.read_exact(&mut data[size..size * 2])?;
        self.report.bytes_read += size as u64;
        Ok(Anchor {
            offset,
            identity,
            digest: Sha256::digest(&data[..size * 2]).into(),
        })
    }

    pub fn restore(
        &mut self,
        root: &Path,
        path: &Path,
        file: &mut File,
        identity: (u64, u64),
        size: u64,
        call_limit: u32,
    ) -> Loaded {
        let Some(key) = key(root, path) else {
            return Loaded::default();
        };
        if !self.attempted {
            self.attempted = true;
            match Store::open(&self.directory, Access::ReadOnly) {
                Ok(reader) => self.reader = reader,
                Err(_) => self.report.load_health = "unavailable",
            }
        }
        let Some(reader) = self.reader.as_mut() else {
            return Loaded::default();
        };
        let checkpoint = if self.prefetched_keys.remove(&key) {
            let Some(mut checkpoint) = self.prefetched.remove(&key) else {
                return Loaded::default();
            };
            checkpoint.calls.truncate(call_limit.min(8192) as usize);
            checkpoint
        } else {
            match reader.checkpoint(&key, call_limit) {
                Ok(Some(value)) => value,
                Ok(None) => return Loaded::default(),
                Err(_) => {
                    self.report.load_health = "unavailable";
                    return Loaded::default();
                }
            }
        };
        let restored = (|| {
            let anchor = serde_json::from_str::<VersionedAnchor>(&checkpoint.cursor.signature)
                .map(|value| value.anchor)
                .or_else(|_| serde_json::from_str::<Anchor>(&checkpoint.cursor.signature))
                .ok()?;
            if anchor.identity != identity
                || anchor.offset > size
                || i64::try_from(anchor.offset).ok() != Some(checkpoint.cursor.offset)
                || self.sample(file, identity, anchor.offset).ok()? != anchor
            {
                return None;
            }
            let state: State<Projection> = serde_json::from_str(&checkpoint.cursor.payload).ok()?;
            // Older checkpoints can omit unmatched tool failures, so replay their source.
            if state.version != STATE_VERSION
                || state.projection.errors_incomplete
                || state.projection.sqlite_incomplete
                || state.projection.commands_incomplete
            {
                return None;
            }
            let calls = checkpoint
                .calls
                .into_iter()
                .map(|row| {
                    serde_json::from_str::<Call>(&row.payload)
                        .ok()
                        .map(|value| (row.call_id, value))
                })
                .collect::<Option<BTreeMap<_, _>>>()?;
            if calls.values().any(|call| {
                call.needs_tool_replay()
                    || [
                        call.request_position.as_ref(),
                        call.response_position.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|position| position.offset >= anchor.offset)
            }) {
                return None;
            }
            let mut projection = state.projection;
            if projection
                .workdir_position
                .as_ref()
                .is_some_and(|position| !position.valid() || position.offset >= anchor.offset)
            {
                return None;
            }
            if let Some(position) = &projection.workdir_position {
                let record = super::details::read_record(file, position)?;
                if record["type"] != "session_meta" {
                    return None;
                }
                let cwd = record["payload"]["cwd"].as_str()?;
                if cwd.is_empty() || cwd.chars().count() > 4096 || cwd.chars().any(char::is_control)
                {
                    return None;
                }
                projection.set_workdir(Some(cwd));
            }
            if projection.errors.iter().any(|error| {
                error
                    .record_offset
                    .is_some_and(|offset| offset >= anchor.offset)
            }) {
                return None;
            }
            projection.restore_calls(calls).then_some(Restored {
                anchor,
                discard: state.discard,
                projection,
            })
        })();
        if restored.is_some() {
            self.report.restored += 1;
            self.report.load_health = "ok";
        } else {
            self.invalidate();
        }
        Loaded {
            signature: Some(checkpoint.cursor.signature),
            state: restored,
        }
    }

    pub fn invalidate(&mut self) {
        self.report.invalidated += 1;
        self.report.load_health = "stale";
    }

    pub fn write_failed(&mut self) {
        self.report.write_health = "unavailable";
    }

    pub fn partial_matches(
        &mut self,
        file: &mut File,
        end: u64,
        partial: &[u8],
    ) -> io::Result<bool> {
        if partial.is_empty() {
            return Ok(true);
        }
        let size = partial.len().min(SAMPLE as usize);
        let mut bytes = [0; SAMPLE as usize];
        file.seek(SeekFrom::Start(end - partial.len() as u64))?;
        file.read_exact(&mut bytes[..size])?;
        self.report.bytes_read += size as u64;
        if bytes[..size] != partial[..size] {
            return Ok(false);
        }
        if size == partial.len() {
            return Ok(true);
        }
        file.seek(SeekFrom::Start(end - size as u64))?;
        file.read_exact(&mut bytes[..size])?;
        self.report.bytes_read += size as u64;
        Ok(bytes[..size] == partial[partial.len() - size..])
    }

    pub fn observed_history(&self) -> lam_history_store::Result<Vec<Value>> {
        match Store::open(&self.directory, Access::ReadOnly)? {
            Some(mut store) => store.observed_history(now()),
            None => Ok(Vec::new()),
        }
    }

    pub fn activity_retention(&self) -> lam_history_store::Result<i64> {
        match Store::open(&self.directory, Access::ReadOnly)? {
            Some(store) => store.activity_retention(),
            None => Ok(90),
        }
    }

    pub fn save_diagnostics(
        &mut self,
        sql: &[Value],
        errors: &[Value],
        reference: DateTime<Utc>,
    ) -> lam_history_store::Result<()> {
        self.connect_observations(Access::ReadWrite)?
            .ok_or(StoreError::InvalidQuery)?
            .save_diagnostics(sql, errors, reference)
    }

    pub fn error_preview(
        &mut self,
        reference: DateTime<Utc>,
    ) -> lam_history_store::Result<Vec<Value>> {
        match self.connect_observations(Access::ReadOnly)? {
            Some(store) => store.error_preview(reference),
            None => Ok(Vec::new()),
        }
    }

    pub fn skill_preview(&mut self) -> lam_history_store::Result<Vec<Value>> {
        match self.connect_observations(Access::ReadOnly)? {
            Some(store) => store.skill_preview(),
            None => Ok(Vec::new()),
        }
    }

    pub fn save_unanchored_errors(&mut self, files: &mut BTreeMap<PathBuf, Session>) -> bool {
        let pending = |session: &Session| {
            session.anchor.is_none()
                && (!session.projection.pending_errors.is_empty() || session.unsaved_tool_errors())
        };
        let errors: Vec<_> = files
            .iter()
            .filter(|(_, session)| pending(session))
            .flat_map(|(path, session)| {
                let session_errors = session.error_events(path, &session.projection.pending_errors);
                let tool_errors = session
                    .projection
                    .id
                    .as_deref()
                    .into_iter()
                    .flat_map(|thread| {
                        session
                            .projection
                            .dirty_calls
                            .iter()
                            .filter_map(|id| {
                                session.projection.calls.get(id).map(|call| (id, call))
                            })
                            .chain(
                                session
                                    .projection
                                    .retired_observations
                                    .iter()
                                    .map(|(id, call)| (id, call)),
                            )
                            .filter_map(move |(id, call)| call.failure_event(thread, id))
                    });
                session_errors.chain(tool_errors)
            })
            .collect();
        if errors.is_empty() {
            return false;
        }
        // An actively changing file may prevent a stable checkpoint anchor.
        // These error keys can be replayed without advancing the cursor.
        if let Err(error) = self.save_diagnostics(&[], &errors, now()) {
            self.error_write_error = Some(storage_error_type(&error));
            self.report.write_health = "unavailable";
            return false;
        }
        for session in files.values_mut().filter(|session| pending(session)) {
            session.projection.pending_errors.clear();
            session.tool_errors_unanchored_saved = true;
        }
        if files.values().all(|session| {
            session.projection.pending_errors.is_empty() && !session.unsaved_tool_errors()
        }) {
            self.error_write_error = None;
        }
        true
    }

    fn storage_identity(&self) -> lam_history_store::Result<Option<(u64, u64)>> {
        match fs::symlink_metadata(self.directory.join(FILE_NAME)) {
            Ok(meta) if !redirected(&meta) && meta.is_file() => Ok(Some(file_identity(&meta))),
            Ok(_) => Err(StoreError::UnsafePath),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn connect_observations(
        &mut self,
        access: Access,
    ) -> lam_history_store::Result<Option<&mut ObservationStore>> {
        let mut identity = self.storage_identity()?;
        let reusable = self
            .observation_store
            .as_ref()
            .is_some_and(|(current, mode, _)| {
                Some(*current) == identity
                    && (*mode == Access::ReadWrite || access == Access::ReadOnly)
            });
        if !reusable {
            self.observation_store = None;
            if identity.is_none() {
                let initialized = Store::open(&self.directory, access)?;
                if initialized.is_none() {
                    return Ok(None);
                }
                drop(initialized);
                identity = self.storage_identity()?;
            }
            let expected = identity.ok_or(StoreError::UnsafePath)?;
            if let Some(store) = ObservationStore::open(&self.directory, access)? {
                if self.storage_identity()? != Some(expected) {
                    return Err(StoreError::UnsafePath);
                }
                self.observation_store = Some((expected, access, store));
            }
        }
        Ok(self.observation_store.as_mut().map(|(_, _, store)| store))
    }

    pub fn observed_view(
        &mut self,
        reference: DateTime<Utc>,
        cutoffs: &[Option<DateTime<Utc>>; 4],
        excluded_sources: &[&str],
    ) -> lam_history_store::Result<ObservationView> {
        let result = self
            .connect_observations(Access::ReadOnly)
            .and_then(|store| match store {
                Some(store) => store.read_filtered(reference, cutoffs, excluded_sources),
                None => Ok(ObservationView::default()),
            });
        if result.is_err() {
            self.observation_store = None;
        }
        result
    }

    pub fn pending_activity(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<(bool, Option<DateTime<Utc>>), StoreError> {
        let next = match self.connect_observations(Access::ReadOnly)? {
            Some(store) => store.next_pending_activity()?,
            None => None,
        };
        if next.is_some_and(|due| due <= now) {
            let Some(store) = self.connect_observations(Access::ReadWrite)? else {
                return Ok((false, next));
            };
            let changed = store.process_pending_activity(now)?;
            return Ok((changed, store.next_pending_activity()?));
        }
        Ok((false, next))
    }

    pub fn save(&mut self, root: &Path, files: &mut BTreeMap<PathBuf, Session>) -> bool {
        self.begin_round();
        let mut cursors = Vec::new();
        let mut calls = Vec::new();
        let mut retired = Vec::new();
        let mut activity = Vec::new();
        let mut errors = Vec::new();
        let mut skills = Vec::new();
        let mut lifecycle = Vec::new();
        let mut reset = Vec::new();
        let mut saved = Vec::new();
        let mut previous = Vec::new();
        let mut oversized = false;
        let mut encoding_failed = false;
        for (path, session) in files.iter() {
            if session.projection.observation_overflow {
                oversized = true;
                continue;
            }
            let Some(anchor) = session.anchor.as_ref().filter(|_| {
                session.checkpoint_dirty || !session.projection.retired_calls.is_empty()
            }) else {
                continue;
            };
            let Some(key) = key(root, path) else { continue };
            let Ok(offset) = i64::try_from(anchor.offset) else {
                continue;
            };
            let revision = session
                .checkpoint_signature
                .as_ref()
                .and_then(|value| serde_json::from_str::<VersionedAnchor>(value).ok())
                .map_or(0, |value| value.revision);
            let Some(revision) = revision.checked_add(1) else {
                encoding_failed = true;
                continue;
            };
            let Ok(signature) = serde_json::to_string(&VersionedAnchor {
                anchor: anchor.clone(),
                revision,
            }) else {
                encoding_failed = true;
                continue;
            };
            let Ok(payload) = serde_json::to_string(&State {
                version: STATE_VERSION,
                discard: session.discard,
                projection: &session.projection,
            }) else {
                encoding_failed = true;
                continue;
            };
            let stored = session
                .projection
                .dirty_calls
                .iter()
                .map(|id| {
                    let call = session.projection.calls.get(id)?;
                    Some(StoredCall {
                        source_key: key.clone(),
                        call_id: id.clone(),
                        event_time: call.time.map(stamp),
                        payload: serde_json::to_string(call).ok()?,
                    })
                })
                .collect::<Option<Vec<_>>>();
            let Some(stored) = stored else {
                encoding_failed = true;
                continue;
            };
            let bytes = payload.len() + stored.iter().map(|row| row.payload.len()).sum::<usize>();
            if bytes > BYTE_LIMIT {
                oversized = true;
                continue;
            }
            if let Some(thread) = session.projection.id.as_deref() {
                let current = session
                    .projection
                    .dirty_calls
                    .iter()
                    .filter_map(|id| session.projection.calls.get(id).map(|call| (id, call)));
                let retired = session
                    .projection
                    .retired_observations
                    .iter()
                    .map(|(id, call)| (id, call));
                skills.extend(retired.chain(current).flat_map(|(id, call)| {
                    call.skill_history(thread, id)
                        .map(|event| ObservationWrite {
                            source_key: key.clone(),
                            event,
                        })
                }));
                let current = session
                    .projection
                    .dirty_calls
                    .iter()
                    .filter_map(|id| session.projection.calls.get(id).map(|call| (id, call)));
                let retired = session
                    .projection
                    .retired_observations
                    .iter()
                    .map(|(id, call)| (id, call));
                activity.extend(retired.chain(current).flat_map(|(id, call)| {
                    call.activities(thread, id).map(|event| ObservationWrite {
                        source_key: key.clone(),
                        event,
                    })
                }));
                let current = session
                    .projection
                    .dirty_calls
                    .iter()
                    .filter_map(|id| session.projection.calls.get(id).map(|call| (id, call)));
                let retired = session
                    .projection
                    .retired_observations
                    .iter()
                    .map(|(id, call)| (id, call));
                errors.extend(retired.chain(current).filter_map(|(id, call)| {
                    call.failure_event(thread, id)
                        .map(|event| ObservationWrite {
                            source_key: key.clone(),
                            event,
                        })
                }));
            }
            errors.extend(
                session
                    .error_events(path, &session.projection.pending_errors)
                    .map(|event| ObservationWrite {
                        source_key: key.clone(),
                        event,
                    }),
            );
            if session.checkpoint_reset {
                reset.push(key.clone());
            }
            retired.extend(
                session
                    .projection
                    .retired_calls
                    .iter()
                    .map(|id| (key.clone(), id.clone())),
            );
            if let Some(event) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| session.projection.lifecycle_entry(name, anchor.offset))
            {
                lifecycle.push(ObservationWrite {
                    source_key: key.clone(),
                    event,
                });
            }
            cursors.push(Cursor {
                source_key: key.clone(),
                offset,
                signature: signature.clone(),
                payload,
            });
            previous.push((key.clone(), session.checkpoint_signature.clone()));
            calls.extend(stored);
            saved.push((path.clone(), key, signature));
        }
        if cursors.is_empty() {
            if encoding_failed {
                self.write_failed();
            } else if oversized {
                self.report.write_health = "oversized";
            }
            return false;
        }
        let expected: Vec<_> = previous
            .iter()
            .map(|(key, signature)| ExpectedCursor {
                source_key: key,
                signature: signature.as_deref(),
            })
            .collect();
        let result = self
            .connect_observations(Access::ReadWrite)
            .and_then(|store| {
                store
                    .ok_or(lam_history_store::StoreError::InvalidQuery)?
                    .save_observation_batch(
                        &cursors,
                        &calls,
                        &retired,
                        &reset,
                        &expected,
                        ObservationBatch {
                            rows: &activity,
                            errors: &errors,
                            skills: &skills,
                            lifecycle: &lifecycle,
                            now: now(),
                        },
                    )
            });
        let conflicts = match result {
            Ok(conflicts) => {
                if !errors.is_empty() {
                    self.error_write_error = None;
                }
                conflicts
            }
            Err(error) => {
                if !errors.is_empty() {
                    self.error_write_error = Some(storage_error_type(&error));
                }
                self.report.write_health = "unavailable";
                return false;
            }
        };
        for (path, key, signature) in saved {
            if conflicts.contains(&key) {
                continue;
            }
            if let Some(session) = files.get_mut(&path) {
                session.checkpoint_signature = Some(signature);
                session.checkpoint_dirty = false;
                session.checkpoint_reset = false;
                session.projection.dirty_calls.clear();
                session.projection.retired_calls.clear();
                session.projection.retired_observations.clear();
                session.projection.retired_activity_count = 0;
                session.projection.pending_errors.clear();
            }
        }
        self.report.write_health = if encoding_failed {
            "unavailable"
        } else if !conflicts.is_empty() {
            "conflict"
        } else if oversized {
            "oversized"
        } else {
            "ok"
        };
        !activity.is_empty() || !errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_processes_save_unanchored_errors_without_duplicates() {
        const CHILD: &str = "LAM_TEST_UNANCHORED_CHILD";
        const DIRECTORY: &str = "LAM_TEST_UNANCHORED_DIRECTORY";
        const TIMESTAMP: &str = "LAM_TEST_UNANCHORED_TIMESTAMP";
        if let Some(child) = std::env::var_os(CHILD) {
            let directory = PathBuf::from(std::env::var_os(DIRECTORY).unwrap());
            let timestamp = std::env::var(TIMESTAMP).unwrap();
            let child = child.to_str().unwrap();
            let path = directory.join("rollout-00000000-0000-4000-8000-000000000001.jsonl");
            let mut session = Session::new(&path);
            for id in ["same", child] {
                for payload in [
                    serde_json::json!({"type":"function_call","name":"mcp__fixture__read","call_id":id,"arguments":"{}"}),
                    serde_json::json!({"type":"function_call_output","call_id":id,"output":{"isError":true}}),
                ] {
                    let line = serde_json::json!({"type":"response_item","timestamp":timestamp,"payload":payload}).to_string();
                    assert!(session.projection.consume(
                        line.as_bytes(),
                        0,
                        true,
                        true,
                        &BTreeMap::new(),
                        crate::sessions::McpSwitches::default(),
                        true,
                        true,
                        crate::sessions::CommandSwitches::default()
                    ));
                }
            }
            std::fs::write(directory.join(format!("ready-{child}")), b"ready").unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while !directory.join("start").exists() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let mut files = BTreeMap::from([(path, session)]);
            let mut checkpoints = Checkpoints::new(directory, true);
            while !checkpoints.save_unanchored_errors(&mut files) {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        drop(Store::open(directory.path(), Access::ReadWrite).unwrap());
        let timestamp = stamp(now());
        let executable = std::env::current_exe().unwrap();
        let mut children = Vec::new();
        for child in ["first", "second"] {
            children.push(std::process::Command::new(&executable)
                .args(["--exact", "sessions::checkpoint::tests::concurrent_processes_save_unanchored_errors_without_duplicates"])
                .env(CHILD, child).env(DIRECTORY, directory.path()).env(TIMESTAMP, &timestamp)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).spawn().unwrap());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !(directory.path().join("ready-first").exists()
            && directory.path().join("ready-second").exists())
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::fs::write(directory.path().join("start"), b"go").unwrap();
        let outputs: Vec<_> = children
            .into_iter()
            .map(|child| child.wait_with_output().unwrap())
            .collect();
        for output in outputs {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut checkpoints = Checkpoints::new(directory.path().to_path_buf(), true);
        let events = checkpoints.error_preview(now()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["call_id"] == "same")
                .count(),
            1
        );
        let mut store = Store::open(directory.path(), Access::ReadOnly)
            .unwrap()
            .unwrap();
        let path = directory
            .path()
            .join("rollout-00000000-0000-4000-8000-000000000001.jsonl");
        assert!(store
            .checkpoint(&key(directory.path(), &path).unwrap(), 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn unanchored_tool_failure_retries_without_advancing_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("rollout-00000000-0000-4000-8000-000000000001.jsonl");
        let mut session = Session::new(&path);
        let time = now();
        for payload in [
            serde_json::json!({"type":"function_call","name":"mcp__fixture__read","call_id":"call","arguments":"{}"}),
            serde_json::json!({"type":"function_call_output","call_id":"call","output":{"isError":true}}),
        ] {
            let line = serde_json::json!({"type":"response_item","timestamp":stamp(time),"payload":payload}).to_string();
            assert!(session.projection.consume(
                line.as_bytes(),
                0,
                true,
                true,
                &BTreeMap::new(),
                crate::sessions::McpSwitches::default(),
                true,
                true,
                crate::sessions::CommandSwitches::default()
            ));
        }
        let mut files = BTreeMap::from([(path.clone(), session)]);
        let mut checkpoints = Checkpoints::new(directory.path().to_path_buf(), true);
        drop(Store::open(directory.path(), Access::ReadWrite).unwrap());
        let db = rusqlite::Connection::open(directory.path().join(FILE_NAME)).unwrap();
        db.execute_batch(
            "CREATE TRIGGER reject_errors BEFORE INSERT ON history_items
            WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
        )
        .unwrap();
        assert!(!checkpoints.save_unanchored_errors(&mut files));
        assert!(!files[&path].tool_errors_unanchored_saved);
        db.execute_batch("DROP TRIGGER reject_errors").unwrap();
        assert!(checkpoints.save_unanchored_errors(&mut files));
        assert!(files[&path].tool_errors_unanchored_saved);
        assert_eq!(checkpoints.error_preview(time).unwrap().len(), 1);
        assert!(!checkpoints.save_unanchored_errors(&mut files));
        let mut store = Store::open(directory.path(), Access::ReadOnly)
            .unwrap()
            .unwrap();
        assert!(store
            .checkpoint(&key(directory.path(), &path).unwrap(), 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn unanchored_errors_are_saved_without_advancing_the_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout-fixture.jsonl");
        let mut session = Session::new(&path);
        let time = now();
        let line = serde_json::json!({
            "type":"event_msg", "timestamp":stamp(time), "payload":{"type":"turn_aborted"}
        })
        .to_string();
        assert!(session.projection.consume(
            line.as_bytes(),
            0,
            true,
            true,
            &BTreeMap::new(),
            crate::sessions::McpSwitches::default(),
            true,
            true,
            crate::sessions::CommandSwitches::default()
        ));
        let mut files = BTreeMap::from([(path.clone(), session)]);
        let mut checkpoints = Checkpoints::new(directory.path().to_path_buf(), true);
        assert!(checkpoints.save_unanchored_errors(&mut files));
        assert_eq!(checkpoints.error_preview(time).unwrap().len(), 1);
        assert!(!checkpoints.save_unanchored_errors(&mut files));
        let mut store = Store::open(directory.path(), Access::ReadOnly)
            .unwrap()
            .unwrap();
        assert!(store
            .checkpoint(&key(directory.path(), &path).unwrap(), 1)
            .unwrap()
            .is_none());
    }
}
