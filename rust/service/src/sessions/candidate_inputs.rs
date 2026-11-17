//! Internal validation prerequisite, not a production aggregation/publication switch.
//! Other metadata/diagnostics
//! and final publication guards remain separate requirements.
use super::{account, Anchor, Collector, CommandSwitches, Session, SessionContentStamp, SnapshotSourceSettings};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::SystemTime;

#[derive(PartialEq, Eq)]
struct ReadProgress {
    offset: u64,
    size: u64,
    partial_bytes: usize,
    discard: bool,
    identity: Option<(u64, u64)>,
    modified: Option<SystemTime>,
    anchor: Option<Anchor>,
    checkpoint_signature: Option<String>,
}
impl ReadProgress {
    fn capture(session: &Session) -> Self {
        Self { offset:session.offset, size:session.size, partial_bytes:session.partial.len(),
            discard:session.discard, identity:session.identity, modified:session.modified,
            anchor:session.anchor.clone(), checkpoint_signature:session.checkpoint_signature.clone() }
    }
}

#[derive(PartialEq, Eq)]
struct ObservationFlags {
    tool_events: bool,
    files: bool,
    requested_errors: bool,
    applied_errors: bool,
    requested_sqlite: bool,
    applied_sqlite: bool,
    requested_model_api: bool,
    applied_model_api: bool,
    requested_commands: CommandSwitches,
    applied_commands: CommandSwitches,
    metadata: bool,
    logs: bool,
}
impl ObservationFlags {
    fn capture(collector: &Collector) -> Self {
        Self {
            tool_events:collector.tool_events_enabled.load(Ordering::Acquire),
            files:collector.files_enabled.load(Ordering::Acquire),
            requested_errors:collector.errors_enabled.load(Ordering::Acquire),
            applied_errors:collector.applied_errors_enabled,
            requested_sqlite:collector.sqlite_enabled.load(Ordering::Acquire),
            applied_sqlite:collector.applied_sqlite_enabled,
            requested_model_api:collector.model_api_enabled.load(Ordering::Acquire),
            applied_model_api:collector.applied_model_api_enabled,
            requested_commands:*collector.command_switches.read().unwrap_or_else(|error| error.into_inner()),
            applied_commands:collector.applied_command_switches,
            metadata:collector.metadata_enabled, logs:collector.logs_enabled,
        }
    }
}

// Selection inputs outside the Session payload owners. A generation-only rescan
// is deliberately not an input; meaningful membership changes own the SQL epoch.
#[derive(PartialEq, Eq)]
struct SourceTopology {
    membership: Option<(u64, i64, i64)>,
    root_identity: Option<(u64, u64)>,
    root: Option<PathBuf>,
    complete: bool,
    discovered_files: Option<usize>,
    order: BTreeMap<PathBuf, usize>,
    paths: BTreeMap<String, PathBuf>,
    duplicates: BTreeMap<String, PathBuf>,
}
impl SourceTopology {
    fn capture(collector: &Collector) -> Option<Self> {
        // Discovery may temporarily own and mutate the membership database.
        if collector.discovery.is_some() { return None; }
        let membership = match &collector.source_membership {
            Some(source) => Some((source.identity, source.revision,
                source.db.query_row("SELECT value FROM selection_epoch", [],
                    |row| row.get::<_, i64>(0)).ok()?)),
            None => None,
        };
        Some(Self { membership, root_identity:collector.source_membership_root,
            root:collector.session_root.clone(), complete:collector.source_membership_complete,
            discovered_files:collector.discovered_files, order:collector.snapshot_source_order.clone(),
            paths:collector.session_paths.clone(), duplicates:collector.duplicate_session_paths.clone() })
    }
}

type Sessions = BTreeMap<PathBuf, (SessionContentStamp, ReadProgress)>;

pub(super) struct CandidateInputs {
    topology: SourceTopology,
    active: Sessions,
    overflow: Sessions,
    history_revision: u64,
    thread_metadata_revision: u64,
    project_details_revision: u64,
    mcp_catalog_revision: u64,
    diagnostic_observations: (u64, u64),
    api_events_revision: u64,
    overflow_details_revision: u64,
    queries: [(Arc<account::Shared>, u64); 8],
    sources: SnapshotSourceSettings,
    flags: ObservationFlags,
}

impl CandidateInputs {
    fn queries(collector: &Collector) -> [&Arc<account::Shared>; 8] {
        [&collector.account, &collector.cloud, &collector.public[0], &collector.public[1],
            &collector.platform[0], &collector.platform[1], &collector.platform[2], &collector.platform[3]]
    }
    fn sessions(files: &BTreeMap<PathBuf, Session>) -> Option<Sessions> {
        files.iter().map(|(path, session)| {
            session.projection.content_revision.value()?;
            Some((path.clone(), (session.content_stamp(), ReadProgress::capture(session))))
        }).collect()
    }
    fn sessions_match(expected: &Sessions, actual: &BTreeMap<PathBuf, Session>) -> bool {
        expected.len() == actual.len() && expected.iter().all(|(path, (content, progress))|
            actual.get(path).is_some_and(|session| content.matches(session) && *progress == ReadProgress::capture(session)))
    }
    #[allow(dead_code)] // Ready for bounded candidate integration, not used by snapshots.
    pub(super) fn capture(collector: &Collector, enabled: bool, stopped: &AtomicBool) -> Option<Self> {
        if !enabled || stopped.load(Ordering::Acquire) { return None; }
        let inputs = Self {
            topology:SourceTopology::capture(collector)?,
            active:Self::sessions(&collector.files)?, overflow:Self::sessions(&collector.overflow_files)?,
            history_revision:collector.history_revision.value()?,
            thread_metadata_revision:collector.thread_metadata_revision.value()?,
            project_details_revision:collector.project_details_revision.value()?,
            mcp_catalog_revision:collector.mcp_catalog_revision.value()?,
            diagnostic_observations:(collector.core.observation_revision.value()?, collector.desktop.observation_revision.value()?),
            api_events_revision:collector.api_events_revision.value()?,
            overflow_details_revision:collector.overflow_details_revision.value()?,
            queries:Self::queries(collector).into_iter().map(|source|
                Some((Arc::clone(source), source.content_revision()?)))
                .collect::<Option<Vec<_>>>()?.try_into().ok()?,
            sources:collector.capture_snapshot_source_settings(), flags:ObservationFlags::capture(collector),
        };
        // Recheck earlier independently locked inputs after finishing the capture.
        inputs.validate(collector, enabled, stopped)
    }
    #[allow(dead_code)]
    pub(super) fn validate(self, collector: &Collector, enabled: bool, stopped: &AtomicBool) -> Option<Self> {
        let valid = enabled && !stopped.load(Ordering::Acquire)
            && SourceTopology::capture(collector).as_ref() == Some(&self.topology)
            && collector.history_revision.value() == Some(self.history_revision)
            && collector.thread_metadata_revision.value() == Some(self.thread_metadata_revision)
            && collector.project_details_revision.value() == Some(self.project_details_revision)
            && collector.mcp_catalog_revision.value() == Some(self.mcp_catalog_revision)
            && collector.core.observation_revision.value() == Some(self.diagnostic_observations.0)
            && collector.desktop.observation_revision.value() == Some(self.diagnostic_observations.1)
            && collector.api_events_revision.value() == Some(self.api_events_revision)
            && collector.overflow_details_revision.value() == Some(self.overflow_details_revision)
            && Self::sessions_match(&self.active, &collector.files)
            && Self::sessions_match(&self.overflow, &collector.overflow_files)
            && self.queries.iter().zip(Self::queries(collector)).all(|((owner, revision), current)|
                Arc::ptr_eq(owner, current) && current.content_revision() == Some(*revision))
            && self.sources == collector.capture_snapshot_source_settings()
            && self.flags == ObservationFlags::capture(collector);
        // A rejected guard drops all retained owner handles and settings copies.
        valid.then_some(self)
    }
}

// Own the exact selection and clock alongside mutable-owner validation. This is
// reusable candidate state, but is not called by the production snapshot worker.
pub(super) struct SelectedInputs {
    inputs: CandidateInputs,
    selection: super::CallSelection,
    pub(super) time: super::SnapshotTime,
    signatures: BTreeMap<String, String>,
    validated_after: Option<String>,
    sweep_complete: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SourceValidationError { Inputs, Read, Changed, InvalidBudget, Cancelled }

#[allow(dead_code)]
impl SelectedInputs {
    pub(super) fn capture(collector: &Collector, selection: &super::CallSelection,
        time: super::SnapshotTime, enabled: bool, stopped: &AtomicBool) -> Option<Self>
    {
        if !selection.complete { return None; }
        Some(Self { inputs:CandidateInputs::capture(collector, enabled, stopped)?,
            selection:selection.clone(), time, signatures:BTreeMap::new(), validated_after:None, sweep_complete:false })
    }
    pub(super) fn reducer_switches(&self) -> (bool, bool, super::CommandSwitches) {
        (self.inputs.flags.tool_events, self.inputs.flags.files, self.inputs.flags.applied_commands)
    }
    pub(super) fn source_settings(&self) -> &SnapshotSourceSettings { &self.inputs.sources }
    pub(super) fn errors_enabled(&self) -> bool { self.inputs.flags.applied_errors }
    pub(super) fn observation_switches(&self) -> (bool, bool) {
        (self.inputs.sources.switches.mcp, self.inputs.flags.applied_sqlite)
    }
    pub(super) fn validate(mut self, collector: &Collector, selection: &super::CallSelection,
        enabled: bool, stopped: &AtomicBool) -> Option<Self>
    {
        if self.selection != *selection { return None; }
        self.inputs = self.inputs.validate(collector, enabled, stopped)?;
        Some(self)
    }
    // Include metadata-only sources too; their cursor can affect the batch even
    // when no selected call belongs to that source. Never silently replace a stamp.
    pub(super) fn source(mut self, key: String, signature: String) -> Option<Self> {
        if key.is_empty() || self.validated_after.is_some() { return None; }
        if let Some(existing) = self.signatures.get(&key) {
            if existing != &signature { return None; }
        } else {
            if self.signatures.len() >= super::WALK_LIMIT { return None; }
            self.signatures.insert(key, signature);
        }
        Some(self)
    }
    // Each call opens a fresh Store and retains no SQLite transaction across
    // rounds. Completion means a sequential sweep, not a database-wide snapshot.
    pub(super) fn validate_sources(self, collector: &Collector, selection: &super::CallSelection,
        enabled: bool, stopped: &AtomicBool, limit: usize) -> Option<(Self, bool)>
    {
        self.try_validate_sources(collector, selection, enabled, stopped, limit).ok()
    }
    pub(super) fn try_validate_sources(mut self, collector: &Collector, selection: &super::CallSelection,
        enabled: bool, stopped: &AtomicBool, limit: usize) -> Result<(Self, bool), SourceValidationError>
    {
        use std::ops::Bound::{Excluded, Unbounded};
        if limit == 0 { return Err(SourceValidationError::InvalidBudget); }
        self = self.validate(collector, selection, enabled, stopped).ok_or_else(|| if stopped.load(Ordering::Acquire) { SourceValidationError::Cancelled } else { SourceValidationError::Inputs })?;
        let store = collector.checkpoints.snapshot_reader().map_err(|_| SourceValidationError::Read)?.ok_or(SourceValidationError::Read)?;
        if self.sweep_complete { self.validated_after = Some(String::new()); }
        let after = self.validated_after.clone().unwrap_or_default();
        for (key, signature) in self.signatures.range((Excluded(after), Unbounded)).take(limit.min(64)) {
            if stopped.load(Ordering::Acquire) { return Err(SourceValidationError::Cancelled); }
            if store.checkpoint_signature(key).map_err(|_| SourceValidationError::Read)?.as_ref() != Some(signature) {
                return Err(SourceValidationError::Changed);
            }
            self.validated_after = Some(key.clone());
        }
        let complete = self.signatures.last_key_value().is_none_or(|(key, _)| self.validated_after.as_ref() == Some(key));
        self.sweep_complete = complete;
        // The empty sweep is also closed to additional sources.
        if self.validated_after.is_none() { self.validated_after = Some(String::new()); }
        self = self.validate(collector, selection, enabled, stopped).ok_or_else(|| if stopped.load(Ordering::Acquire) { SourceValidationError::Cancelled } else { SourceValidationError::Inputs })?;
        Ok((self, complete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{McpSwitches, now};
    use serde_json::json;
    use std::io::Write;

    fn fixture() -> (tempfile::TempDir, Collector, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let path = root.join("rollout-00000000-0000-4000-8000-000000000001.jsonl");
        std::fs::write(&path, format!("{}\n", json!({"type":"response_item","timestamp":"2026-11-13T01:00:00Z",
            "payload":{"type":"function_call","name":"fixture","call_id":"a","arguments":"{}"}}))).unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        for _ in 0..4 {
            let (_, pending, _) = collector.refresh(&root, &AtomicBool::new(false), true, true, CommandSwitches::default());
            if !pending { break; }
        }
        assert_eq!(collector.files[&path].projection.calls.len(), 1);
        (temporary, collector, path)
    }

    #[test]
    fn candidate_inputs_track_retained_api_adoption_not_pending_queues() {
        let (_temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let event = json!({"observation_id":"fixture","timestamp":"2026-11-13T01:00:00Z","event_kind":"request","status":"returned"});
        let guard = capture(&collector);
        collector.desktop.api_events.push(event.clone());
        collector.core.api_events.push_back(event.clone());
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.collect_api_events();
        assert_eq!(collector.api_events.len(), 1);
        assert!(collector.core.api_events.is_empty() && collector.desktop.api_events.is_empty());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.core.api_events.push_back(event);
        collector.desktop.api_events.push(json!({"missing_id":true}));
        collector.collect_api_events();
        assert_eq!(collector.api_events.len(), 1);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.clear_api_events();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.clear_api_events();
        collector.collect_api_events();
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_log_only_acceptance_and_clear() {
        let (temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let reference = now();
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let db = rusqlite::Connection::open(temporary.path().join("logs_candidate.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT)").unwrap();
        db.execute("INSERT INTO logs VALUES(1,?,'INFO','codex_core')", [reference.timestamp()]).unwrap();
        let guard = capture(&collector);
        collector.refresh_core(super::super::core_logs::discover(temporary.path()), reference);
        assert_eq!(collector.core.logs.len(), 1);
        assert!(collector.core.errors.is_empty() && collector.core.sql.is_empty());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.refresh_core(super::super::core_logs::discover(temporary.path()), reference + chrono::Duration::seconds(1));
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.core.clear_logs();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.core.clear_logs();
        assert!(guard.validate(&collector, true, &stopped).is_some());

        let logs = temporary.path().join("desktop-logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::write(logs.join("codex-desktop-candidate.log"),
            format!("{} INFO [codex] fixture\n", reference.to_rfc3339())).unwrap();
        let roots = [super::super::desktop::DesktopRoot {base:temporary.path().to_path_buf(),path:logs}];
        let guard = capture(&collector);
        assert_eq!(collector.desktop.refresh(&roots, reference).live.len(), 1);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.desktop.commit(&mut collector.checkpoints, reference).unwrap();
        assert_eq!(collector.desktop.logs.len(), 1);
        assert!(collector.desktop.errors.is_empty() && collector.desktop.sql.is_empty());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        let previous_checked_at = collector.desktop.source["checked_at"].clone();
        collector.desktop.refresh(&roots, reference + chrono::Duration::seconds(1));
        collector.desktop.commit(&mut collector.checkpoints, reference + chrono::Duration::seconds(1)).unwrap();
        assert_ne!(collector.desktop.source["checked_at"], previous_checked_at);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.desktop.clear_logs();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.desktop.clear_logs();
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_core_observation_refresh_and_clear() {
        let (temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let reference = now();
        let db = rusqlite::Connection::open(temporary.path().join("logs_candidate.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT)").unwrap();
        db.execute("INSERT INTO logs VALUES(1,?,'WARN','codex_core')", [reference.timestamp()]).unwrap();
        let guard = CandidateInputs::capture(&collector, true, &stopped).unwrap();
        collector.refresh_core(super::super::core_logs::discover(temporary.path()), reference);
        assert!(!collector.core.errors.is_empty());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = CandidateInputs::capture(&collector, true, &stopped).unwrap();
        collector.refresh_core(super::super::core_logs::discover(temporary.path()), reference);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = CandidateInputs::capture(&collector, true, &stopped).unwrap();
        collector.core.clear_observations();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = CandidateInputs::capture(&collector, true, &stopped).unwrap();
        collector.core.clear_observations();
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_mcp_catalog_replacement_and_clear() {
        let (_temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let guard = capture(&collector);
        assert!(collector.replace_mcp_catalog([("fixture".into(), None)].into_iter().collect()));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        assert!(!collector.replace_mcp_catalog(collector.mcp_catalog.clone()));
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        assert!(collector.replace_mcp_catalog([("fixture".into(), Some(super::super::source_introduction::Description {
            text:"Changed".into(),source:"fixture".into(),path:None,
        }))].into_iter().collect()));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        assert!(collector.replace_mcp_catalog(Default::default()));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        assert!(!collector.replace_mcp_catalog(Default::default()));
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_project_catalog_replacement_and_clear() {
        let (_temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let projects = [("project".to_owned(), json!({"name":"First","root":"fixture/root"}))].into_iter().collect();
        let guard = capture(&collector);
        assert!(collector.replace_project_details(projects));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let revision = collector.project_details_revision.value();
        let guard = capture(&collector);
        assert!(!collector.replace_project_details(collector.project_details.clone()));
        assert_eq!(collector.project_details_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        let mut projects = collector.project_details.clone();
        projects.get_mut("project").unwrap()["name"] = json!("Updated");
        assert!(collector.replace_project_details(projects));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        assert!(collector.replace_project_details(Default::default()));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        assert!(!collector.replace_project_details(Default::default()));
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_detail_readiness_replacement_and_clear() {
        let (_temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let ready = ["first".to_owned()].into_iter().collect();
        let guard = capture(&collector);
        collector.replace_overflow_details_ready(ready);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let revision = collector.overflow_details_revision.value();
        let guard = capture(&collector);
        collector.replace_overflow_details_ready(collector.overflow_details_ready.clone());
        assert_eq!(collector.overflow_details_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.replace_overflow_details_ready(Default::default());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let revision = collector.overflow_details_revision.value();
        let guard = capture(&collector);
        collector.replace_overflow_details_ready(Default::default());
        assert_eq!(collector.overflow_details_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_track_thread_metadata_replacement_order_and_clear() {
        let (_temporary, mut collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let mut metadata = serde_json::Map::new();
        metadata.insert("first".into(), json!({"name":"first title","_cwd":"fixture/project"}));
        metadata.insert("second".into(), json!({"name":"second title"}));
        let guard = capture(&collector);
        collector.replace_thread_metadata(metadata.clone());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let revision = collector.thread_metadata_revision.value();
        let guard = capture(&collector);
        collector.replace_thread_metadata(metadata.clone());
        assert_eq!(collector.thread_metadata_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let mut reordered = serde_json::Map::new();
        reordered.insert("second".into(), metadata["second"].clone());
        reordered.insert("first".into(), metadata["first"].clone());
        assert!(!metadata.iter().eq(reordered.iter()), "actual preserve_order feature is required by this oracle");
        let guard = capture(&collector);
        collector.replace_thread_metadata(reordered);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let mut metadata = collector.thread_metadata.clone();
        metadata["first"]["name"] = json!("updated title");
        let guard = capture(&collector);
        collector.replace_thread_metadata(metadata);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.replace_thread_metadata(serde_json::Map::new());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let revision = collector.thread_metadata_revision.value();
        let guard = capture(&collector);
        collector.replace_thread_metadata(serde_json::Map::new());
        assert_eq!(collector.thread_metadata_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn source_validation_reports_read_failure_and_missing_signature_separately() {
        let (temporary, collector, _) = fixture();
        let stopped = AtomicBool::new(false);
        let selection = super::super::CallSelection { calls:BTreeMap::new(), complete:true };
        let make = || SelectedInputs::capture(&collector, &selection, collector.capture_snapshot_time(), true, &stopped)
            .unwrap().source("missing-source".into(), "expected-signature".into()).unwrap();
        assert_eq!(make().try_validate_sources(&collector, &selection, true, &stopped, 1).err(), Some(SourceValidationError::Changed));
        let directory = temporary.path().join("monitoring");
        let database = directory.join(lam_history_store::FILE_NAME);
        let saved = directory.join("saved.sqlite3");
        std::fs::rename(&database, &saved).unwrap();
        std::fs::create_dir(&database).unwrap();
        assert_eq!(make().try_validate_sources(&collector, &selection, true, &stopped, 1).err(), Some(SourceValidationError::Read));
        std::fs::remove_dir(&database).unwrap();
        std::fs::rename(&saved, &database).unwrap();
    }

    #[test]
    fn candidate_inputs_reject_actual_payload_and_framing_changes_and_release_owners() {
        let (temporary, mut collector, path) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let guard = capture(&collector);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        let revision = collector.files[&path].projection.content_revision.value();
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{").unwrap();
        collector.files.get_mut(&path).unwrap().read(temporary.path(), &path, 1024, 8192,
            true, true, &BTreeMap::new(), McpSwitches::default(), true, true,
            CommandSwitches::default(), &mut collector.checkpoints).unwrap();
        assert_eq!(collector.files[&path].projection.content_revision.value(), revision);
        assert!(guard.validate(&collector, true, &stopped).is_none(), "read progress is independent of payload revision");
        let guard = capture(&collector);
        let output = json!({"type":"response_item","timestamp":"2026-11-13T01:00:00Z",
            "payload":{"type":"function_call_output","call_id":"a","output":{"exit_code":2}}}).to_string();
        assert!(collector.files.get_mut(&path).unwrap().projection.consume(output.as_bytes(), 100,
            true, true, &BTreeMap::new(), McpSwitches::default(), true, true, CommandSwitches::default()));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        let session = collector.files.remove(&path).unwrap();
        collector.overflow_files.insert(path.clone(), session);
        assert!(guard.validate(&collector, true, &stopped).is_none(), "working-set membership changed");
        let guard = capture(&collector);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let owner = collector.overflow_files[&path].content_owner.clone();
        let baseline = Arc::strong_count(&owner);
        let guard = capture(&collector);
        assert_eq!(Arc::strong_count(&owner), baseline + 1);
        stopped.store(true, Ordering::Release);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        assert_eq!(Arc::strong_count(&owner), baseline);
        assert!(CandidateInputs::capture(&collector, true, &stopped).is_none());
        stopped.store(false, Ordering::Release);
        assert!(capture(&collector).validate(&collector, false, &stopped).is_none());
        let guard = capture(&collector);
        collector.overflow_files.insert(path.clone(), Session::new(&path));
        assert!(guard.validate(&collector, true, &stopped).is_none(), "same path with a new owner");
    }

    #[test]
    fn candidate_inputs_validate_real_query_history_and_settings_owners() {
        let (_temporary, mut collector, _path) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        let guard = capture(&collector);
        collector.account.publish(json!({"fixture":1}));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        let event = collector.account.version();
        collector.account.publish(json!({"fixture":1}));
        assert!(collector.account.version() > event);
        assert!(guard.validate(&collector, true, &stopped).is_some(), "cached completion is not a content mutation");
        for source in 0..8 {
            let guard = capture(&collector);
            let queries = CandidateInputs::queries(&collector);
            queries[source].configure(account::Settings { enabled:true, interval:600 });
            assert!(guard.validate(&collector, true, &stopped).is_none(), "source {source}");
        }
        let guard = capture(&collector);
        collector.account = Arc::new(account::Shared::new());
        assert!(guard.validate(&collector, true, &stopped).is_none(), "query owner replacement");
        let guard = capture(&collector);
        let reference = now();
        collector.checkpoints.save_diagnostics(&[], &[json!({"timestamp":super::super::stamp(reference),
            "source":"codex_core","category":"runtime","severity":"error","record_id":1})], reference).unwrap();
        collector.refresh_error_history(reference);
        assert_eq!(collector.error_history.len(), 1);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.refresh_error_history(reference);
        assert!(guard.validate(&collector, true, &stopped).is_some());
        let guard = capture(&collector);
        collector.tracking.write().unwrap().max_files = 37;
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.errors_enabled.store(false, Ordering::Release);
        assert!(guard.validate(&collector, true, &stopped).is_none(), "requested disable before worker applies it");
        let guard = capture(&collector);
        collector.applied_errors_enabled = false;
        assert!(guard.validate(&collector, true, &stopped).is_none(), "worker applied state is also an input");
        assert!(capture(&collector).validate(&collector, true, &stopped).is_some());
    }

    #[test]
    fn candidate_inputs_validate_membership_epoch_and_source_order_without_rescan_churn() {
        use super::super::{MembershipRecord, SourceMembership};
        let (_temporary, mut collector, path) = fixture();
        let stopped = AtomicBool::new(false);
        let capture = |collector: &Collector| CandidateInputs::capture(collector, true, &stopped).unwrap();
        // Use the real membership producers, not a synthetic revision counter.
        collector.source_membership = Some(SourceMembership::new().unwrap());
        let record = || MembershipRecord { key:"fixture".into(), fingerprint:Some("unchanged".into()) };
        collector.source_membership.as_mut().unwrap().insert(&[record()]).unwrap();
        let guard = capture(&collector);
        let source = collector.source_membership.as_mut().unwrap();
        source.next_generation().unwrap();
        source.insert(&[record()]).unwrap();
        assert!(guard.validate(&collector, true, &stopped).is_some(), "unchanged rescan generation");
        let guard = capture(&collector);
        collector.source_membership.as_mut().unwrap().set_working(&["fixture".into()]).unwrap();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.source_membership.as_mut().unwrap().invalidate().unwrap();
        assert!(guard.validate(&collector, true, &stopped).is_none(), "invalidation precedes row updates");
        let guard = capture(&collector);
        collector.source_membership.as_mut().unwrap().insert(&[MembershipRecord {
            key:"fixture".into(), fingerprint:Some("changed".into()) }]).unwrap();
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.snapshot_source_order.insert(path.clone(), 17);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.session_paths.insert("fixture".into(), path.clone());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.duplicate_session_paths.insert("fixture".into(), path.clone());
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.source_membership_complete = !collector.source_membership_complete;
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.source_membership_root = Some((91, 92));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.session_root = Some(path.parent().unwrap().join("other"));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.discovered_files = Some(37);
        assert!(guard.validate(&collector, true, &stopped).is_none());
        let guard = capture(&collector);
        collector.discovery = Some(super::super::SessionDiscovery::new(path.parent().unwrap(),
            (91, 92), Default::default(), collector.source_membership.take(), Default::default(), false));
        assert!(guard.validate(&collector, true, &stopped).is_none());
        assert!(CandidateInputs::capture(&collector, true, &stopped).is_none(), "discovery owns mutable membership");
        collector.source_membership = collector.discovery.take().unwrap().membership;
        let guard = capture(&collector);
        collector.source_membership = Some(SourceMembership::new().unwrap());
        assert!(guard.validate(&collector, true, &stopped).is_none(), "same empty epoch is not same owner");
        let guard = capture(&collector);
        collector.source_membership.as_ref().unwrap().db.execute("UPDATE selection_epoch SET value='invalid'", []).unwrap();
        assert!(guard.validate(&collector, true, &stopped).is_none(), "read failure drops candidate");
        assert!(CandidateInputs::capture(&collector, true, &stopped).is_none());
    }

}
