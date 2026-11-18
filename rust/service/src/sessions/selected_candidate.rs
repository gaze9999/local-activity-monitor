//! Selected active/checkpoint candidate orchestration. Checkpoint source order
//! must already be verified by its caller. Full snapshot assembly stays private
//! until publication guards pass.
use super::{candidate_inputs::SelectedInputs, checkpoint, commands, details, mcp, project, sqlite,
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
    detail_pending: BTreeSet<String>,
    // Checkpoint pages, calls, payload bytes, losing calls, losing payload bytes.
    // Encoded payload bytes exclude cursor/keys and are not disk I/O or RSS.
    #[cfg(test)]
    read_work: [usize; 5],
    winners: BTreeMap<CallKey, usize>,
    slots: BTreeMap<CallKey, u64>,
    next_slots: BTreeMap<String, u64>,
    thread_slots: BTreeMap<String, usize>,
    source_threads: Vec<Option<String>>,
    allowance: Option<Value>,
    detail_indexes: BTreeMap<usize, details::Index>,
    detail_stage_bytes: usize,
    detail_stage_limit: usize,
    session_errors: Vec<Vec<Value>>,
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
    CheckpointRead, CheckpointChanged, InvalidProjection, StatisticsCapacity, DetailCapacity, State,
}

pub(super) struct CompletedCandidate {
    pub files: [Value; 4],
    pub series: [Value; 4],
    pub statistics: [Value; 4],
    pub commands: [Value; 4],
    pub skills: [Value; 4],
    pub rows: [Value; 4],
    pub observations: [Value; 4],
    pub failures: Vec<(Option<chrono::DateTime<chrono::Utc>>, Value)>,
    pub session_errors: Vec<Value>,
    // Source-selected usage, before finalizer health / usage-switch filtering.
    pub allowance: Option<Value>,
    pub detail_index: details::Index,
    pub metadata: BTreeMap<String, (Projection, bool)>,
    pub overflow_sources: usize,
    pub overflow_details_pending: BTreeSet<String>,
    pub time: SnapshotTime,
    inputs: Option<SelectedInputs>,
    physical: BTreeMap<std::path::PathBuf, String>,
    #[cfg(test)]
    pub read_work: [usize; 5],
}

#[allow(dead_code)] // Not activated by the production snapshot worker.
impl SelectedCandidate {
    pub(super) fn new(collector: &Collector, selection: &CallSelection, time: SnapshotTime,
        sources: Vec<Source>, ordered: bool,
        statistics_units: usize, statistics_key_bytes: usize, enabled: bool, stopped: &AtomicBool) -> Option<Self>
    {
        // Never silently publish a checkpoint-only view of mutable active calls.
        if !collector.overflow_files.is_empty()
            || !collector.source_membership_complete || (sources.is_empty() && collector.files.is_empty()) || sources.len() > WALK_LIMIT
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
        let root = collector.session_root.as_deref()?;
        // Selected calls may belong exclusively to guarded active sources.
        // Only materialized overflow sources require checkpoint signatures.
        for path in collector.files.keys() { keys.insert(checkpoint::key(root, path)?); }
        if selection.calls.keys().any(|key| !keys.contains(key)) { return None; }
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
            metadata:BTreeMap::new(), partial:BTreeMap::new(), detail_pending:BTreeSet::new(),
            #[cfg(test)] read_work:[0; 5],
            winners:BTreeMap::new(), slots:BTreeMap::new(),
            next_slots:BTreeMap::new(), thread_slots:BTreeMap::new(), source_threads:Vec::new(), allowance:None, detail_indexes:BTreeMap::new(), detail_stage_bytes:1024,
            detail_stage_limit:details::STAGING_LIMIT, session_errors:Vec::new(), names:HashMap::new(), ordered,
            statistics_units, statistics_key_bytes, reducers:None })
    }

    #[cfg(test)]
    pub(super) fn with_detail_limit(mut self, limit: usize) -> Self { self.detail_stage_limit = limit; self }

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
        if self.detail_stage_bytes > self.detail_stage_limit { return Err(Failure::DetailCapacity); }
        let budget = budget.min(16);
        match self.phase {
            Phase::Headers => {
                for _ in 0..budget {
                    Self::controls(enabled, stopped)?;
                    if self.next == self.active.len() + self.sources.len() { break; }
                    let path = if self.next < self.active.len() { self.active[self.next].clone() }
                        else { self.source_paths[self.next - self.active.len()].clone() };
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    let (projection, partial, ids, errors, allowance) = if self.next < self.active.len() {
                        let path = &self.active[self.next];
                        let state = collector.files.get(path).ok_or(Failure::InputsRejected)?;
                        let key = checkpoint::key(collector.session_root.as_deref().ok_or(Failure::InputsRejected)?, path).ok_or(Failure::InputsRejected)?;
                        let selected = selection.calls.get(&key);
                        let ids: Vec<_> = state.projection.ordered_calls().into_iter()
                            .filter(|(id, _)| selected.is_some_and(|ids| ids.contains(*id)))
                            .map(|(id, _)| id.clone()).collect();
                        let mut metadata = Projection::for_id(state.projection.id.clone());
                        metadata.merge_snapshot(&state.projection, Some(&BTreeSet::new()), &self.inputs.as_ref().ok_or(Failure::State)?.time.cutoffs);
                        let errors = if state.projection.id.is_some() {
                            super::Session::projection_error_events(&state.projection, &path, &state.projection.errors).collect()
                        } else { Vec::new() };
                        (metadata, state.pending() || state.discard, ids, errors, state.projection.allowance.clone())
                    } else {
                        let source = &self.sources[self.next - self.active.len()];
                        let mut store = collector.checkpoints.snapshot_reader().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointRead)?;
                        let header = store.selected_checkpoint(&lam_history_store::ExpectedCursor {
                            source_key:&source.key, signature:Some(&source.signature) }, &[]).map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?;
                        let projection = checkpoint::snapshot_projection(header, false).ok_or(Failure::InvalidProjection)?;
                        if !projection.projection.calls.is_empty() { return Err(Failure::InvalidProjection); }
                        if projection.source_details_pending {
                            if let Some(id) = &projection.projection.id { self.detail_pending.insert(id.clone()); }
                        }
                        let ids = selection.calls.get(&source.key).into_iter().flatten().cloned().collect();
                        let errors = super::Session::projection_error_events(&projection.projection, &path, &projection.projection.errors).collect();
                        let allowance = projection.projection.allowance.clone();
                        (projection.projection, projection.partial, ids, errors, allowance)
                    };
                    // Match snapshots: complete discovery keeps the first equal-time
                    // allowance, canonical overflow fallback keeps the last.
                    if let Some(value) = allowance {
                        let updated = value["updated_at"].as_str().unwrap_or_default();
                        if self.allowance.as_ref().is_none_or(|previous| updated > previous["updated_at"].as_str().unwrap_or_default()
                            || (!collector.has_complete_source_order() && updated == previous["updated_at"].as_str().unwrap_or_default())) {
                            self.allowance = Some(value);
                        }
                    }
                    if let Some(thread) = projection.id.clone() {
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
                    }
                    if !self.valid_path(collector, &path) { return Err(Failure::PhysicalSource); }
                    self.source_threads.push(projection.id.clone());
                    self.session_errors.push(errors);
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
                    let detail_source;
                    let (mut projection, complete) = if source_index < self.active.len() {
                        let state = collector.files.get(&self.active[source_index]).ok_or(Failure::InputsRejected)?;
                        let mut projection = Projection::for_id(state.projection.id.clone());
                        let key = checkpoint::key(collector.session_root.as_deref().ok_or(Failure::InputsRejected)?, &path).ok_or(Failure::InputsRejected)?;
                        let selected = selection.calls.get(&key);
                        // Detail lookup keeps first-source positions, independently
                        // of later-source summary winners. Read all selected calls.
                        let ids: Vec<_> = state.projection.calls.keys().filter(|id| selected.is_some_and(|ids| ids.contains(*id)))
                            .skip(self.active_offset).take(512).collect();
                        detail_source = state.anchor.clone().map(|anchor| (anchor, None));
                        for id in &ids { projection.calls.insert((*id).clone(), state.projection.calls.get(*id).ok_or(Failure::InputsRejected)?.clone()); }
                        self.active_offset += ids.len();
                        let complete = ids.len() < 512;
                        (projection, complete)
                    } else {
                        let source = &self.sources[source_index - self.active.len()];
                        if self.reader.is_none() {
                            // Read every selected checkpoint call: even a losing call's
                            // locator contributes to the source detail-pending marker.
                            let ids = selection.calls.get(&source.key).into_iter().flatten().cloned().collect();
                            self.reader = Some(collector.checkpoints.snapshot_reader().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointRead)?
                                .into_selected_checkpoint_reader(&lam_history_store::ExpectedCursor {
                                    source_key:&source.key, signature:Some(&source.signature) }, ids).map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?);
                        }
                        let reader = self.reader.as_mut().ok_or(Failure::State)?;
                        let page = reader.next_page().map_err(|_| Failure::CheckpointRead)?.ok_or(Failure::CheckpointChanged)?;
                        #[cfg(test)] {
                            self.read_work[0] += 1;
                            self.read_work[1] += page.calls.len();
                            for call in &page.calls {
                                self.read_work[2] += call.payload.len();
                                if self.source_threads[source_index].as_ref().and_then(|thread| self.winners.get(&(thread.clone(), call.call_id.clone()))) != Some(&source_index) {
                                    self.read_work[3] += 1;
                                    self.read_work[4] += call.payload.len();
                                }
                            }
                        }
                        let projection = checkpoint::snapshot_projection(page, false).ok_or(Failure::InvalidProjection)?;
                        if projection.source_details_pending {
                            if let Some(id) = &projection.projection.id { self.detail_pending.insert(id.clone()); }
                        }
                        detail_source = Some((projection.anchor.clone(), Some(details::CheckpointGuard {
                            directory:collector.checkpoints.directory().to_path_buf(), source_key:source.key.clone(), signature:source.signature.clone(),
                        })));
                        (projection.projection, reader.is_complete())
                    };
                    if projection.id != self.source_threads[source_index] { return Err(Failure::InvalidProjection); }
                    if projection.id.is_some() {
                        if let Some((anchor, guard)) = detail_source {
                            let root = collector.session_root.as_deref().ok_or(Failure::InputsRejected)?;
                            let reserve = details::Index::projection_bytes(root, &path, &projection, None)
                                .saturating_add(guard.as_ref().map_or(0, |guard| guard.directory.as_os_str().len()
                                    .saturating_add(guard.source_key.len()).saturating_add(guard.signature.len()).saturating_add(512)));
                            if reserve > self.detail_stage_limit.saturating_sub(self.detail_stage_bytes) { return Err(Failure::DetailCapacity); }
                            self.detail_stage_bytes += reserve;
                            if !self.detail_indexes.entry(source_index).or_default().add_projection(root, &path, &anchor, &projection, guard) {
                                return Err(Failure::InvalidProjection);
                            }
                        }
                    }
                    // No-ID sources still contribute metadata and undergo every
                    // source validation, but baseline excludes their calls from rows.
                    if let Some(thread) = projection.id.as_deref() {
                        if projection.calls.keys().any(|id| !self.winners.contains_key(&(thread.to_owned(), id.clone()))) {
                            return Err(Failure::InvalidProjection);
                        }
                        projection.calls.retain(|id, _| self.winners.get(&(thread.to_owned(), id.clone())) == Some(&source_index));
                        for (id, call) in &mut projection.calls {
                            let key = (thread.to_owned(), id.clone());
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
                    let mut completed = self.finish(collector)?;
                    self.validate_inputs(collector, selection, enabled, stopped)?;
                    completed.inputs = self.inputs.take();
                    completed.physical = std::mem::take(&mut self.physical);
                    return Ok(Advance::Complete(completed));
                }
            }
        }
        self.validate_inputs(collector, selection, enabled, stopped)?;
        Ok(Advance::Pending(self))
    }

    fn finish(&mut self, collector: &Collector) -> Result<CompletedCandidate, Failure> {
        let mut reducers = self.reducers.take().ok_or(Failure::State)?;
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
                if row["thread_id"].as_str().is_some_and(|id| self.detail_pending.contains(id)
                    && !collector.overflow_details_ready.contains(id)) {
                    row["source_details_pending"] = json!(true);
                }
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
            let mut projects: BTreeMap<String, Value> = collector.project_details.iter().map(|(id, detail)| {
                let mut value = json!({"id":id,"name":detail["name"],"icon":detail["icon"],"kind":detail["kind"],"thread_count":0});
                for key in ["origin", "created_at", "updated_at"] {
                    if let Some(field) = detail.get(key) { value[key] = field.clone(); }
                }
                (id.clone(), value)
            }).collect();
            for row in &rows {
                if let Some(id) = row["project_id"].as_str().filter(|id| !id.is_empty()) {
                    let project = projects.entry(id.to_owned()).or_insert_with(|| json!({"id":id,"name":row["project_name"],"icon":null,"kind":row["project_kind"],"thread_count":0}));
                    project["thread_count"] = json!(project["thread_count"].as_u64().unwrap_or(0) + 1);
                }
            }
            json!({"rows":rows,"totals":totals,"nested_totals":nested_totals,"projects":projects.into_values().collect::<Vec<_>>()})
        }).collect::<Vec<_>>().try_into().map_err(|_| Failure::State)?;
        reducers.observations.extend(collector.core.sql.iter().chain(&collector.desktop.sql).cloned());
        let failures = reducers.failures.finish();
        let (mcp_events,sql_events): (Vec<_>,Vec<_>) = reducers.observations.finish().into_iter().partition(|event| event.get("server").is_some());
        let settings = self.inputs.as_ref().ok_or(Failure::State)?.source_settings();
        let (mcp_events, mut all_mcp) = collector.mcp_snapshot_summary(mcp_events, &settings.sources, &settings.customizations, settings.switches);
        if self.inputs.as_ref().ok_or(Failure::State)?.errors_enabled() {
            // MCP ties use canonical thread/call order even when row previews use
            // presentation slots. Preserve tool, Core, Desktop source precedence.
            let mut diagnostics: Vec<_> = failures.iter().map(|(_, event)| event).collect();
            diagnostics.sort_by(|left, right| (left["thread_id"].as_str(), left["call_id"].as_str())
                .cmp(&(right["thread_id"].as_str(), right["call_id"].as_str())));
            mcp::apply_diagnostics(&mut all_mcp, diagnostics.into_iter().chain(collector.core.errors.iter()).chain(collector.desktop.errors.iter()));
        }
        all_mcp["configuration"] = json!({"location":collector.catalog_home.join("config.toml").to_string_lossy(),"configured_sources":collector.mcp_catalog.len()});
        let sql_events = sqlite::identified(sql_events);
        let (mcp_enabled, sqlite_enabled) = self.inputs.as_ref().ok_or(Failure::State)?.observation_switches();
        let observations = std::array::from_fn(|index| {
            let cutoff = cutoffs[index];
            let mut mcp = cutoff.map_or_else(|| all_mcp.clone(), |cutoff| mcp::scoped_summary(&mcp_events, cutoff, collector.activity_history_health, &all_mcp));
            let mut sql = sqlite::window(if sqlite_enabled { &sql_events } else { &[] },cutoff,collector.activity_history_health);
            for event in mcp["events"].as_array_mut().into_iter().flatten()
                .chain(sql["events"].as_array_mut().into_iter().flatten()) {
                if let Some(name) = event["thread_id"].as_str().and_then(|id| self.names.get(id)) { event["thread_name"] = name.clone(); }
            }
            if let Some(view) = &collector.activity_view {
                let empty = lam_history_store::Aggregate::default();
                mcp["history_aggregate"] = super::history_view::render(if mcp_enabled { &view.mcp[index] } else { &empty }, super::WINDOWS[index]);
                sql["aggregate"] = super::history_view::render(if sqlite_enabled { &view.sql[index] } else { &empty }, super::WINDOWS[index]);
            }
            json!({"mcp":mcp,"sql":sql})
        });
        // Baseline groups only active sources when discovery order is complete,
        // then appends checkpoint source errors. Do not use winner metadata or
        // caller-selected tool presentation order for this separate precedence.
        let mut active_order: Vec<_> = (0..self.active.len()).filter(|index| self.source_threads[*index].is_some()).collect();
        if collector.has_complete_source_order() {
            let mut first = BTreeMap::new();
            for index in &active_order {
                let next = first.len();
                first.entry(&self.source_threads[*index]).or_insert(next);
            }
            active_order.sort_by_key(|index| first[&self.source_threads[*index]]);
        }
        let session_errors = active_order.into_iter().chain(self.active.len()..self.session_errors.len())
            .flat_map(|index| std::mem::take(&mut self.session_errors[index])).collect();
        // Payload pages arrive in reverse source order. Detail lookup must
        // retain the original first-permitted source, not summary winner order.
        let mut detail_index = details::Index::default();
        for index in std::mem::take(&mut self.detail_indexes).into_values() {
            for (thread, files) in index.files { detail_index.files.entry(thread).or_default().extend(files); }
        }
        let metadata = std::mem::take(&mut self.metadata).into_iter().map(|(thread, projection)| {
            let partial = self.partial[&thread]; (thread, (projection, partial))
        }).collect();
        let overflow_details_pending = self.detail_pending.iter().filter(|id| !collector.overflow_details_ready.contains(*id)).cloned().collect();
        let time = SnapshotTime { reference, cutoffs };
        Ok(CompletedCandidate { files,series,statistics,commands,skills,rows,observations,failures,session_errors,allowance:self.allowance.take(),detail_index,
            metadata,overflow_sources:self.sources.len(),overflow_details_pending,time,inputs:None,physical:BTreeMap::new(),
            #[cfg(test)] read_work:self.read_work })
    }
}

// Worker admission keeps exact output or falls back; these logical budgets
// are not allocator/RSS bounds. Three units per selected primary call cover
// group, distinct-thread entry and duration; nested excess is rejected whole.
const WORKER_STATISTICS_UNITS: usize = CALL_LIMIT * 3;
const WORKER_STATISTICS_KEY_BYTES: usize = details::STAGING_LIMIT;

enum WorkerPhase {
    Sources { after: String, sources: Vec<Source> },
    Reducing(SelectedCandidate),
    Publishing(PendingPublication),
}

pub(super) struct WorkerCandidate {
    selection: CallSelection,
    opening: Option<SelectedInputs>,
    phase: WorkerPhase,
    statistics_units: usize,
    statistics_key_bytes: usize,
}

pub(super) enum WorkerAdvance { Pending(WorkerCandidate), Published }

impl WorkerCandidate {
    pub(super) fn start(collector: &Collector, stopped: &AtomicBool) -> Option<Self> {
        let membership = collector.source_membership.as_ref()?;
        if !collector.source_membership_complete || !collector.overflow_files.is_empty()
            || membership.materialized_count().ok()? == 0 { return None; }
        let selection = collector.select_recent_calls(CALL_LIMIT, 100_000)?;
        let opening = SelectedInputs::capture(collector, &selection, collector.capture_snapshot_time(), true, stopped)?;
        Some(Self { selection, opening:Some(opening), phase:WorkerPhase::Sources {after:String::new(),sources:Vec::new()},
            statistics_units:WORKER_STATISTICS_UNITS,statistics_key_bytes:WORKER_STATISTICS_KEY_BYTES })
    }

    #[cfg(test)]
    pub(super) fn with_statistics_limits(mut self, units: usize, key_bytes: usize) -> Self {
        self.statistics_units = units; self.statistics_key_bytes = key_bytes; self
    }

    pub(super) fn advance(mut self, collector: &Collector, health: &str, watch_health: &str,
        version: u64, enabled: bool, stopped: &AtomicBool, sender: &tokio::sync::watch::Sender<super::Snapshots>)
        -> Result<WorkerAdvance, Failure>
    {
        SelectedCandidate::controls(enabled, stopped)?;
        self.phase = match self.phase {
            WorkerPhase::Sources { mut after, mut sources } => {
                self.opening = Some(self.opening.take().ok_or(Failure::State)?
                    .validate(collector, &self.selection, enabled, stopped).ok_or(Failure::InputsRejected)?);
                let page = collector.source_membership.as_ref().ok_or(Failure::InputsRejected)?
                    .materialized_page(&after, 64).map_err(|_| Failure::CheckpointRead)?;
                let complete = page.len() < 64;
                for (key, signature) in page {
                    if sources.len() >= WALK_LIMIT { return Err(Failure::InputsRejected); }
                    after = key.clone(); sources.push(Source {key,signature});
                }
                if complete {
                    let candidate = SelectedCandidate::new(collector, &self.selection, collector.capture_snapshot_time(),
                        sources, collector.has_complete_source_order(), self.statistics_units, self.statistics_key_bytes, enabled, stopped)
                        .ok_or(Failure::InputsRejected)?;
                    self.opening = None;
                    WorkerPhase::Reducing(candidate)
                } else { WorkerPhase::Sources {after,sources} }
            }
            WorkerPhase::Reducing(candidate) => {
                // Never combine several nonempty payload pages in one worker
                // round. Empty sources/header/guard work can use the existing 16.
                let budget = if matches!(candidate.phase, Phase::Payload) {
                    let root = collector.session_root.as_deref().ok_or(Failure::InputsRejected)?;
                    let total = candidate.active.len() + candidate.sources.len();
                    let nonempty = (candidate.next..total.min(candidate.next + 16)).any(|offset| {
                        let index = total - offset - 1;
                        let key = if index < candidate.active.len() { checkpoint::key(root, &candidate.active[index]) }
                            else { Some(candidate.sources[index - candidate.active.len()].key.clone()) };
                        key.as_ref().and_then(|key| self.selection.calls.get(key)).is_some_and(|ids| !ids.is_empty())
                    });
                    if nonempty { 1 } else { 16 }
                } else { 16 };
                match candidate.advance(collector, &self.selection, enabled, stopped, budget)? {
                    Advance::Pending(next) => WorkerPhase::Reducing(next),
                    Advance::Complete(mut result) => WorkerPhase::Publishing(result.prepare_publication(collector,
                        &self.selection, health, watch_health, version, stopped)?),
                }
            }
            WorkerPhase::Publishing(publication) => match publication.advance(collector, &self.selection, enabled, stopped, 16, sender)? {
                PublicationAdvance::Pending(next) => WorkerPhase::Publishing(next),
                PublicationAdvance::Published => return Ok(WorkerAdvance::Published),
            },
        };
        Ok(WorkerAdvance::Pending(self))
    }
}

// A rendered batch remains private until its retained input, checkpoint and
// physical-source guards pass again. Failure drops only the unpublished batch.
#[allow(dead_code)]
pub(super) struct PendingPublication {
    snapshots: super::Snapshots,
    inputs: Option<SelectedInputs>,
    root: std::path::PathBuf,
    root_identity: (u64, u64),
    physical: BTreeMap<std::path::PathBuf, String>,
    physical_after: Option<std::path::PathBuf>,
    physical_complete: bool,
}

pub(super) enum PublicationAdvance { Pending(PendingPublication), Published }

#[allow(dead_code)]
impl CompletedCandidate {
    pub(super) fn prepare_publication(&mut self, collector: &Collector, selection: &CallSelection,
        health: &str, watch_health: &str, version: u64, stopped: &AtomicBool) -> Result<PendingPublication, Failure>
    {
        if health == "disabled" { return Err(Failure::Disabled); }
        SelectedCandidate::controls(true, stopped)?;
        let inputs = self.inputs.take().ok_or(Failure::State)?
            .validate(collector, selection, true, stopped).ok_or(Failure::InputsRejected)?;
        let snapshots = collector.assemble_snapshots(health, watch_health, version, Some(self));
        let inputs = inputs.validate(collector, selection, true, stopped).ok_or(Failure::InputsRejected)?;
        Ok(PendingPublication { snapshots, inputs:Some(inputs),
            root:collector.session_root.clone().ok_or(Failure::InputsRejected)?,
            root_identity:collector.source_membership_root.ok_or(Failure::InputsRejected)?,
            physical:std::mem::take(&mut self.physical),physical_after:None,physical_complete:false })
    }
}

#[allow(dead_code)]
impl PendingPublication {
    pub(super) fn advance(mut self, collector: &Collector, selection: &CallSelection,
        enabled: bool, stopped: &AtomicBool, budget: usize, sender: &tokio::sync::watch::Sender<super::Snapshots>)
        -> Result<PublicationAdvance, Failure>
    {
        use std::ops::Bound::{Excluded, Unbounded};
        SelectedCandidate::controls(enabled, stopped)?;
        if budget == 0 { return Err(Failure::InvalidBudget); }
        self.inputs = Some(self.inputs.take().ok_or(Failure::State)?
            .validate(collector, selection, enabled, stopped).ok_or(Failure::InputsRejected)?);
        if !self.physical_complete {
            let paths: Vec<_> = match &self.physical_after {
                Some(after) => self.physical.range((Excluded(after.clone()), Unbounded)),
                None => self.physical.range::<std::path::PathBuf, _>(..),
            }.take(budget.min(64)).map(|(path, fingerprint)| (path.clone(), fingerprint.clone())).collect();
            for (path, fingerprint) in paths {
                SelectedCandidate::controls(enabled, stopped)?;
                if !physical_matches(&self.root, self.root_identity, &path, &fingerprint) { return Err(Failure::PhysicalSource); }
                self.physical_after = Some(path);
            }
            self.physical_complete = self.physical.last_key_value().is_none_or(|(path, _)| self.physical_after.as_ref() == Some(path));
            return Ok(PublicationAdvance::Pending(self));
        }
        let (inputs, complete) = self.inputs.take().ok_or(Failure::State)?
            .try_validate_sources(collector, selection, enabled, stopped, budget).map_err(|error| {
                use super::candidate_inputs::SourceValidationError as Error;
                match error {
                    Error::Inputs => Failure::InputsRejected, Error::Read => Failure::CheckpointRead,
                    Error::Changed => Failure::CheckpointChanged, Error::InvalidBudget => Failure::InvalidBudget,
                    Error::Cancelled => Failure::Cancelled,
                }
            })?;
        self.inputs = Some(inputs);
        if !complete { return Ok(PublicationAdvance::Pending(self)); }
        SelectedCandidate::controls(enabled, stopped)?;
        self.inputs.take().ok_or(Failure::State)?.validate(collector, selection, enabled, stopped).ok_or(Failure::InputsRejected)?;
        // Same operation publishes all windows, logs, error details and the
        // matching immutable position index. Never expose a partial batch.
        sender.send_replace(self.snapshots);
        Ok(PublicationAdvance::Published)
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
