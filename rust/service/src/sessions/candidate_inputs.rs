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
    pub(super) fn validate_sources(mut self, collector: &Collector, selection: &super::CallSelection,
        enabled: bool, stopped: &AtomicBool, limit: usize) -> Option<(Self, bool)>
    {
        use std::ops::Bound::{Excluded, Unbounded};
        if limit == 0 { return None; }
        self = self.validate(collector, selection, enabled, stopped)?;
        let store = collector.checkpoints.snapshot_reader().ok().flatten()?;
        if self.sweep_complete { self.validated_after = Some(String::new()); }
        let after = self.validated_after.clone().unwrap_or_default();
        for (key, signature) in self.signatures.range((Excluded(after), Unbounded)).take(limit.min(64)) {
            if stopped.load(Ordering::Acquire)
                || store.checkpoint_signature(key).ok().flatten().as_ref() != Some(signature) { return None; }
            self.validated_after = Some(key.clone());
        }
        let complete = self.signatures.last_key_value().is_none_or(|(key, _)| self.validated_after.as_ref() == Some(key));
        self.sweep_complete = complete;
        // The empty sweep is also closed to additional sources.
        if self.validated_after.is_none() { self.validated_after = Some(String::new()); }
        self = self.validate(collector, selection, enabled, stopped)?;
        Some((self, complete))
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
