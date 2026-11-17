//! Selected active/checkpoint candidate orchestration. The production worker does not call
//! this path. Checkpoint source order must already be verified by its caller;
//! no-ID sources and final snapshot augmentation remain separate gates.
use super::{candidate_inputs::SelectedInputs, checkpoint, commands, mcp, project, sqlite,
    CallSelection, Collector, Projection, SnapshotTime, CALL_LIMIT, WALK_LIMIT};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

type CallKey = (String, String);
type WindowRows = BTreeMap<String, project::RowSummary>;

pub(super) struct Source {
    pub key: String,
    pub signature: String,
}

enum Phase { Headers, Payload, Physical, Validate }

struct Reducers {
    files: [project::FileSummary; 4],
    series: [project::ActivitySeries; 4],
    statistics: [project::OwnedStatistics; 4],
    commands: [commands::WindowSummary; 4],
    skills: [project::SkillSummary; 4],
    rows: [WindowRows; 4],
    totals: [BTreeMap<String, usize>; 4],
    nested: [BTreeMap<String, usize>; 4],
    observations: lam_history_store::ObservationMerge,
    failures: project::ToolErrorSummary,
}

pub(super) struct SelectedCandidate {
    inputs: Option<SelectedInputs>,
    sources: Vec<Source>,
    active: Vec<std::path::PathBuf>,
    active_offset: usize,
    source_paths: Vec<std::path::PathBuf>,
    physical: BTreeMap<std::path::PathBuf, String>,
    physical_after: Option<std::path::PathBuf>,
    phase: Phase,
    next: usize,
    reader: Option<lam_history_store::SelectedCheckpointReader>,
    metadata: BTreeMap<String, Projection>,
    partial: BTreeMap<String, bool>,
    winners: BTreeMap<CallKey, usize>,
    slots: BTreeMap<CallKey, u64>,
    next_slots: BTreeMap<String, u64>,
    thread_slots: BTreeMap<String, usize>,
    source_threads: Vec<String>,
    names: HashMap<String, Value>,
    ordered: bool,
    statistics_units: usize,
    statistics_key_bytes: usize,
    reducers: Option<Reducers>,
}

pub(super) enum Advance {
    Pending(SelectedCandidate),
    Complete(CompletedCandidate),
}

// Internal boundaries, not HTTP messages or retry/fallback policy. Physical and
// input validation can reject missing/unreadable state as well as changed state.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Cancelled, Disabled, InvalidBudget, InputsRejected, PhysicalSource,
    CheckpointRead, CheckpointChanged, InvalidProjection, StatisticsCapacity, State,
}

pub(super) struct CompletedCandidate {
    pub files: [Value; 4],
    pub series: [Value; 4],
    pub statistics: [Value; 4],
    pub commands: [Value; 4],
    pub skills: [Value; 4],
    pub rows: [Value; 4],
    pub observations: [Value; 4],
    pub failures: Value,
}

#[allow(dead_code)] // Not activated by the production snapshot worker.
impl SelectedCandidate {
    pub(super) fn new(collector: &Collector, selection: &CallSelection, time: SnapshotTime,
        sources: Vec<Source>, ordered: bool,
        statistics_units: usize, statistics_key_bytes: usize, enabled: bool, stopped: &AtomicBool) -> Option<Self>
    {
        // Never silently publish a checkpoint-only view of mutable active calls.
        if !collector.overflow_files.is_empty()
            || !collector.source_membership_complete || sources.is_empty() || sources.len() > WALK_LIMIT
            || selection.calls.values().try_fold(0usize, |total, ids| total.checked_add(ids.len()))? > CALL_LIMIT { return None; }
        let membership = collector.source_membership.as_ref()?;
        if sources.len() != membership.materialized_count().ok()? { return None; }
        let mut keys = BTreeSet::new();
        let mut inputs = SelectedInputs::capture(collector, selection, time, enabled, stopped)?;
        for source in &sources {
            if source.key.is_empty() || !keys.insert(source.key.clone())
                || !membership.db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE source_key=? AND checkpoint_signature=? AND generation=? AND revision=? AND verified=1 AND materialized=1)",
                    rusqlite::params![source.key, source.signature, membership.generation, membership.revision], |row| row.get::<_, bool>(0)).ok()? { return None; }
            inputs = inputs.source(source.key.clone(), source.signature.clone())?;
        }
        if selection.calls.keys().any(|key| !keys.contains(key)) { return None; }
        let root = collector.session_root.as_deref()?;
        let paths: BTreeMap<_,_> = collector.files.keys().chain(collector.session_paths.values())
            .chain(collector.duplicate_session_paths.values())
            .map(|path| Some((checkpoint::key(root, path)?, path.clone()))).collect::<Option<_>>()?;
        let mut physical = BTreeMap::new();
        let mut source_paths = Vec::new();
        for source in &sources {
            let path = paths.get(&source.key)?.clone();
            let fingerprint: String = membership.db.query_row("SELECT fingerprint FROM sources WHERE source_key=?",
                [&source.key], |row| row.get(0)).ok()?;
            physical.insert(path.clone(), fingerprint);
            source_paths.push(path);
        }
        let active = collector.ordered_working_sources().into_iter().map(|(path, state)| {
            let fingerprint = super::source_fingerprint(state.identity?, state.size, state.modified)?;
            if physical.get(path).is_some_and(|saved| saved != &fingerprint) { return None; }
            physical.insert(path.clone(), fingerprint);
            Some(path.clone())
        }).collect::<Option<Vec<_>>>()?;
        Some(Self { inputs:Some(inputs), sources, active, active_offset:0, source_paths, physical, physical_after:None, phase:Phase::Headers, next:0, reader:None,
            metadata:BTreeMap::new(), partial:BTreeMap::new(), winners:BTreeMap::new(), slots:BTreeMap::new(),
            next_slots:BTreeMap::new(), thread_slots:BTreeMap::new(), source_threads:Vec::new(), names:HashMap::new(), ordered,
            statistics_units, statistics_key_bytes, reducers:None })
    }

    #[cfg(test)]
    pub(super) fn awaiting_validation(&self) -> bool { matches!(self.phase, Phase::Validate) }

    #[cfg(test)]
    pub(super) fn active_payload_in_progress(&self) -> bool { self.active_offset != 0 }

    #[cfg(test)]
    pub(super) fn awaiting_physical_validation(&self) -> bool { matches!(self.phase, Phase::Physical) }

    fn valid_path(&self, collector: &Collector, path: &std::path::Path) -> bool {
        collector.session_root.as_deref().zip(collector.source_membership_root)
            .zip(self.physical.get(path)).is_some_and(|((root, identity), fingerprint)|
                physical_matches(root, identity, path, fingerprint))
    }

    fn initialize(&mut self, collector: &Collector) {
        // All windows use the same catalog name, including catalog-only rows.
        // Its actual owner revision is checked before and after each advance.
        self.names = self.metadata.keys().chain(collector.thread_metadata.keys()).map(|id|
            (id.clone(), collector.thread_metadata.get(id).map_or(Value::Null, |entry| entry["name"].clone()))).collect();
        let cutoffs = self.inputs.as_ref().unwrap().time.cutoffs;
        let (tools, files, switches) = self.inputs.as_ref().unwrap().reducer_switches();
        self.reducers = Some(Reducers {
            files:std::array::from_fn(|_| project::FileSummary::new(1000)),
            series:std::array::from_fn(|_| project::ActivitySeries::new()),
            statistics:cutoffs.map(|cutoff| project::OwnedStatistics::new(cutoff, tools, self.statistics_units, self.statistics_key_bytes)),
            commands:cutoffs.map(commands::WindowSummary::new),
            skills:cutoffs.map(|cutoff| project::SkillSummary::new(&collector.skill_history, &self.names, cutoff, switches.skills)),
            rows:cutoffs.map(|cutoff| self.metadata.iter().map(|(thread, metadata)|
                (thread.clone(), project::RowSummary::new(metadata, cutoff, tools, files, switches.jev_calls, self.ordered))).collect()),
            totals:Default::default(), nested:Default::default(),
            observations:lam_history_store::ObservationMerge::new(&collector.activity_history),
            failures:project::ToolErrorSummary::new(self.ordered),
        });
    }

    fn controls(enabled: bool, stopped: &AtomicBool) -> Result<(), Failure> {
        if stopped.load(Ordering::Acquire) { Err(Failure::Cancelled) }
        else if !enabled { Err(Failure::Disabled) } else { Ok(()) }
    }

    fn validate_inputs(&mut self, collector: &Collector, selection: &CallSelection,
        enabled: bool, stopped: &AtomicBool) -> Result<(), Failure> {
        Self::controls(enabled, stopped)?;
        let inputs = self.inputs.take().ok_or(Failure::State)?.validate(collector, selection, enabled, stopped);
        Self::controls(enabled, stopped)?;
        self.inputs = Some(inputs.ok_or(Failure::InputsRejected)?);
        Ok(())
    }

    // Budget bounds source-header/page operations, not elapsed time or RSS. Each
    // reader page retains its existing 512-call / 16 MiB validation limits.
    pub(super) fn advance(mut self, collector: &Collector, selection: &CallSelection,
        enabled: bool, stopped: &AtomicBool, budget: usize) -> Result<Advance, Failure>
    {
        Self::controls(enabled, stopped)?;
        if budget == 0 { return Err(Failure::InvalidBudget); }
        self.validate_inputs(collector, selection, enabled, stopped)?;
        let budget = budget.min(16);
        match self.phase {
            Phase::Headers => {
                for _ in 0..budget {
                    Self::controls(enabled, stopped)?;
                    if self.next == self.active.len() + self.sources.len() { break; }
                    let path = if self.next < self.active.len() { self.active[self.next].clone() }
                        else { self.source_paths[self.next - self.active.len()].clone() };
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    let (projection, partial, ids) = if self.next < self.active.len() {
                        let path = &self.active[self.next];
                        let state = collector.files.get(path).ok_or(Failure::InputsRejected)?;
                        let key = checkpoint::key(collector.session_root.as_deref().ok_or(Failure::InputsRejected)?, path).ok_or(Failure::InputsRejected)?;
                        let selected = selection.calls.get(&key);
                        let ids: Vec<_> = state.projection.ordered_calls().into_iter()
                            .filter(|(id, _)| selected.is_some_and(|ids| ids.contains(*id)))
                            .map(|(id, _)| id.clone()).collect();
                        let mut metadata = Projection::for_id(state.projection.id.clone());
                        metadata.merge_snapshot(&state.projection, Some(&BTreeSet::new()), &self.inputs.as_ref().ok_or(Failure::State)?.time.cutoffs);
                        (metadata, state.pending() || state.discard, ids)
                    } else {
                        let source = &self.sources[self.next - self.active.len()];
                        let mut store = collector.checkpoints.snapshot_reader().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointRead)?;
                        let header = store.selected_checkpoint(&lam_history_store::ExpectedCursor {
                            source_key:&source.key, signature:Some(&source.signature) }, &[]).map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?;
                        let projection = checkpoint::snapshot_projection(header, false).ok_or(Failure::InvalidProjection)?;
                        if !projection.projection.calls.is_empty() { return Err(Failure::InvalidProjection); }
                        let ids = selection.calls.get(&source.key).into_iter().flatten().cloned().collect();
                        (projection.projection, projection.partial, ids)
                    };
                    let thread = projection.id.clone().ok_or(Failure::InvalidProjection)?;
                    let next_thread = self.thread_slots.len();
                    self.thread_slots.entry(thread.clone()).or_insert(next_thread);
                    self.metadata.entry(thread.clone()).or_insert_with(|| Projection::for_id(Some(thread.clone())))
                        .merge_snapshot(&projection, None, &self.inputs.as_ref().ok_or(Failure::State)?.time.cutoffs);
                    *self.partial.entry(thread.clone()).or_default() |= partial;
                    for id in ids {
                        let key = (thread.clone(), id);
                        let next = self.next_slots.entry(thread.clone()).or_default();
                        self.slots.entry(key.clone()).or_insert_with(|| { let slot = *next; *next += 1; slot });
                        self.winners.insert(key, self.next);
                    }
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    self.source_threads.push(thread);
                    self.next += 1;
                }
                if self.next == self.active.len() + self.sources.len() {
                    self.initialize(collector);
                    self.phase = Phase::Payload;
                    self.next = 0;
                }
            }
            Phase::Payload => {
                for _ in 0..budget {
                    Self::controls(enabled, stopped)?;
                    if self.next == self.active.len() + self.sources.len() {
                        self.phase = Phase::Physical;
                        break;
                    }
                    // Reverse payload arrival is safe only after the source-order
                    // winner and first-insertion slot plan has been completed.
                    let source_index = self.active.len() + self.sources.len() - self.next - 1;
                    let path = if source_index < self.active.len() { self.active[source_index].clone() }
                        else { self.source_paths[source_index - self.active.len()].clone() };
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    let (mut projection, complete) = if source_index < self.active.len() {
                        let state = collector.files.get(&self.active[source_index]).ok_or(Failure::InputsRejected)?;
                        let mut projection = Projection::for_id(state.projection.id.clone());
                        let ids: Vec<_> = self.winners.iter().filter(|(_, winner)| **winner == source_index)
                            .map(|((_, id), _)| id).skip(self.active_offset).take(512).collect();
                        for id in &ids { projection.calls.insert((*id).clone(), state.projection.calls.get(*id).ok_or(Failure::InputsRejected)?.clone()); }
                        self.active_offset += ids.len();
                        let complete = ids.len() < 512;
                        (projection, complete)
                    } else {
                        let source = &self.sources[source_index - self.active.len()];
                        if self.reader.is_none() {
                            let ids = self.winners.iter().filter(|(_, winner)| **winner == source_index)
                                .map(|((_, id), _)| id.clone()).collect();
                            self.reader = Some(collector.checkpoints.snapshot_reader().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointRead)?
                                .into_selected_checkpoint_reader(&lam_history_store::ExpectedCursor {
                                    source_key:&source.key, signature:Some(&source.signature) }, ids).map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?);
                        }
                        let reader = self.reader.as_mut().ok_or(Failure::State)?;
                        let page = reader.next_page().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?;
                        (checkpoint::snapshot_projection(page, false).ok_or(Failure::InvalidProjection)?.projection, reader.is_complete())
                    };
                    let thread = projection.id.as_deref().ok_or(Failure::InvalidProjection)?;
                    if thread != self.source_threads[source_index] { return Err(Failure::InvalidProjection); }
                    for (id, call) in &mut projection.calls {
                        let key = (thread.to_owned(), id.clone());
                        if self.winners.get(&key) != Some(&source_index) { return Err(Failure::InvalidProjection); }
                        if self.ordered { call.presentation_sequence = *self.slots.get(&key).ok_or(Failure::State)?; }
                    }
                    let reducers = self.reducers.as_mut().ok_or(Failure::State)?;
                    let time = &self.inputs.as_ref().ok_or(Failure::State)?.time;
                    for (index, cutoff) in time.cutoffs.iter().copied().enumerate() {
                        if self.ordered {
                            reducers.files[index].ordered_thread(self.thread_slots[thread]);
                            reducers.commands[index].ordered_thread(self.thread_slots[thread]);
                        }
                        reducers.files[index].observe(&projection, cutoff, self.inputs.as_ref().ok_or(Failure::State)?.reducer_switches().1, None);
                        reducers.series[index].observe(&projection, cutoff);
                        for (id, call) in &projection.calls {
                            reducers.statistics[index].observe(thread, call);
                            reducers.commands[index].observe(thread, id, call);
                            reducers.skills[index].observe(thread, Some(thread), id, call);
                            reducers.rows[index].get_mut(thread).ok_or(Failure::State)?.observe(id, call, &mut reducers.totals[index], &mut reducers.nested[index]);
                        }
                    }
                    for (id, call) in &projection.calls {
                        reducers.failures.observe(thread, id, call);
                        reducers.observations.extend(call.activities(thread, id));
                    }
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    if complete { self.reader = None; self.active_offset = 0; self.next += 1; }
                }
            }
            Phase::Physical => {
                use std::ops::Bound::{Excluded, Unbounded};
                let paths: Vec<_> = match &self.physical_after {
                    Some(after) => self.physical.range((Excluded(after.clone()), Unbounded)).map(|(path, _)| path).take(budget).cloned().collect(),
                    None => self.physical.keys().take(budget).cloned().collect(),
                };
                for path in paths {
                    Self::controls(enabled, stopped)?;
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    self.physical_after = Some(path);
                }
                if self.physical.last_key_value().is_none_or(|(path, _)| self.physical_after.as_ref() == Some(path)) {
                    self.phase = Phase::Validate;
                }
            }
            Phase::Validate => {
                let (inputs, complete) = self.inputs.take().ok_or(Failure::State)?.try_validate_sources(collector, selection, enabled, stopped, budget).map_err(|error| {
                    use super::candidate_inputs::SourceValidationError as Error;
                    match error {
                        Error::Inputs => if !enabled { Failure::Disabled } else { Failure::InputsRejected },
                        Error::Read => Failure::CheckpointRead, Error::Changed => Failure::CheckpointChanged,
                        Error::InvalidBudget => Failure::InvalidBudget, Error::Cancelled => Failure::Cancelled,
                    }
                })?;
                self.inputs = Some(inputs);
                if complete {
                    let completed = self.finish(collector)?;
                    self.validate_inputs(collector, selection, enabled, stopped)?;
                    return Ok(Advance::Complete(completed));
                }
            }
        }
        self.validate_inputs(collector, selection, enabled, stopped)?;
        Ok(Advance::Pending(self))
    }

    fn finish(&mut self, collector: &Collector) -> Result<CompletedCandidate, Failure> {
        let reducers = self.reducers.take().ok_or(Failure::State)?;
        let reference = self.inputs.as_ref().ok_or(Failure::State)?.time.reference;
        let cutoffs = self.inputs.as_ref().ok_or(Failure::State)?.time.cutoffs;
        let files = reducers.files.map(|summary| {
            let (mut events,total,operations) = summary.finish();
            for event in &mut events {
                if let Some(name) = event["thread_id"].as_str().and_then(|id| self.names.get(id)) { event["thread_name"] = name.clone(); }
            }
            json!({"events":events,"total":total,"operations":operations})
        });
        let series = reducers.series.map(|series| { let (activity,tools) = series.finish(); json!({"activity":activity,"tools":tools}) });
        let statistics = reducers.statistics.into_iter().map(|summary| summary.finish()).collect::<Option<Vec<_>>>().ok_or(Failure::StatisticsCapacity)?.try_into().map_err(|_| Failure::State)?;
        let switches = self.inputs.as_ref().ok_or(Failure::State)?.reducer_switches().2;
        let commands = reducers.commands.map(|summary| {
            let (mut git,mut checks) = summary.finish();
            for event in git["events"].as_array_mut().into_iter().flatten().chain(checks.iter_mut()) {
                if let Some(name) = event["thread_id"].as_str().and_then(|id| self.names.get(id)) { event["thread_name"] = name.clone(); }
            }
            if !switches.git { git = json!({"events":[],"operations":{},"total":0}); }
            if !switches.checks { checks.clear(); }
            json!({"git":git,"checks":checks})
        });
        let skills = reducers.skills.map(|summary| { let (events,total,counts) = summary.finish(&self.names); json!({"events":events,"total":total,"counts":counts}) });
        let (tools, files_enabled, _) = self.inputs.as_ref().ok_or(Failure::State)?.reducer_switches();
        let rows = reducers.rows.into_iter().zip(reducers.totals).zip(reducers.nested).enumerate().map(|(index,((summaries,mut totals),mut nested_totals))| {
            let mut rows: Vec<_> = summaries.into_iter().map(|(thread,summary)| {
                let mut row = self.metadata[&thread].row_with_summary(self.partial[&thread], reference, summary);
                super::apply_thread_metadata(&mut row, collector.thread_metadata.get(&thread), false);
                let name = row["thread_name"].clone();
                for key in ["file_changes", "file_reads"] {
                    for event in row[key].as_array_mut().into_iter().flatten() { event["thread_name"] = name.clone(); }
                }
                row
            }).collect();
            for (thread, metadata) in &collector.thread_metadata {
                if self.metadata.contains_key(thread) { continue; }
                let mut row = Projection::for_id(Some(thread.clone())).row(true, cutoffs[index], reference,
                    &mut totals, &mut nested_totals, tools, files_enabled, switches.jev_calls, false);
                super::apply_thread_metadata(&mut row, Some(metadata), true);
                rows.push(row);
            }
            let pending: BTreeSet<_> = collector.files.values().filter(|state| state.pending())
                .filter_map(|state| state.projection.id.as_deref()).collect();
            for row in &mut rows {
                row["status_backfill_pending"] = json!(row["thread_id"].as_str().is_some_and(|id| pending.contains(id)));
            }
            // Stable canonical ties retain BTreeMap source rows followed by catalog
            // insertion order. Complete-source ties use first-observed thread slots.
            rows.sort_by(|left, right| right["updated_at"].as_str().cmp(&left["updated_at"].as_str())
                .then_with(|| {
                    if !self.ordered { return std::cmp::Ordering::Equal; }
                    let rank = |row: &Value| row["thread_id"].as_str()
                        .and_then(|id| self.thread_slots.get(id)).copied().unwrap_or(usize::MAX);
                    rank(left).cmp(&rank(right))
                }));
            json!({"rows":rows,"totals":totals,"nested_totals":nested_totals})
        }).collect::<Vec<_>>().try_into().map_err(|_| Failure::State)?;
        let (mcp_events,sql_events): (Vec<_>,Vec<_>) = reducers.observations.finish().into_iter().partition(|event| event.get("server").is_some());
        let sql_events = sqlite::identified(sql_events);
        let observations = cutoffs.map(|cutoff| {
            let mut mcp = mcp::summary(&mcp_events,cutoff,collector.activity_history_health);
            let mut sql = sqlite::window(&sql_events,cutoff,collector.activity_history_health);
            for event in mcp["events"].as_array_mut().into_iter().flatten()
                .chain(sql["events"].as_array_mut().into_iter().flatten()) {
                if let Some(name) = event["thread_id"].as_str().and_then(|id| self.names.get(id)) { event["thread_name"] = name.clone(); }
            }
            json!({"mcp":mcp,"sql":sql})
        });
        let failures: Vec<_> = reducers.failures.finish().into_iter().map(|(start,event)| json!({"start":start.map(project::stamp),"event":event})).collect();
        Ok(CompletedCandidate { files,series,statistics,commands,skills,rows,observations,failures:json!(failures) })
    }
}

// Metadata-only physical guard, using the collector's no-follow source opening.
// A sequential observation is not an atomic filesystem snapshot or content hash.
fn physical_matches(root: &std::path::Path, root_identity: (u64, u64), path: &std::path::Path, expected: &str) -> bool {
    let check = || -> Option<bool> {
        let root_before = std::fs::symlink_metadata(root).ok()?;
        if !root_before.is_dir() || super::redirected(&root_before) || super::file_identity(&root_before) != root_identity { return Some(false); }
        let file = super::open_source(root, path).ok()?;
        let opened = file.metadata().ok()?;
        let current = std::fs::symlink_metadata(path).ok()?;
        let root_after = std::fs::symlink_metadata(root).ok()?;
        let stamp = |metadata: &std::fs::Metadata| super::source_fingerprint(super::file_identity(metadata), metadata.len(), metadata.modified().ok());
        Some(opened.is_file() && current.is_file() && !super::redirected(&current)
            && root_after.is_dir() && !super::redirected(&root_after) && super::file_identity(&root_after) == root_identity
            && stamp(&opened).as_deref() == Some(expected) && stamp(&current).as_deref() == Some(expected))
    };
    check() == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_guard_rejects_replacement_removal_and_redirection() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let root_identity = super::super::file_identity(&std::fs::metadata(&root).unwrap());
        let directory = root.join("sessions");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.jsonl");
        std::fs::write(&path, b"fixture\n").unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let expected = super::super::source_fingerprint(super::super::file_identity(&metadata), metadata.len(), metadata.modified().ok()).unwrap();
        assert!(physical_matches(&root, root_identity, &path, &expected));
        let saved = directory.join("saved.jsonl");
        std::fs::rename(&path, &saved).unwrap();
        assert!(!physical_matches(&root, root_identity, &path, &expected));
        std::fs::write(&path, b"fixture\n").unwrap();
        assert!(!physical_matches(&root, root_identity, &path, &expected), "same bytes are not the same source owner");
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&saved, &path).unwrap();
        assert!(physical_matches(&root, root_identity, &path, &expected));
        #[cfg(unix)] {
            let renamed = root.join("renamed");
            std::fs::rename(&directory, &renamed).unwrap();
            std::os::unix::fs::symlink(&renamed, &directory).unwrap();
            assert!(!physical_matches(&root, root_identity, &path, &expected), "same file through a redirected parent is rejected");
        }
    }
}
