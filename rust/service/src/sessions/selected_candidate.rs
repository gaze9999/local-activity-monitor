//! Checkpoint-only candidate orchestration. The production worker does not call
//! this path. Source order must already be verified by its caller; active Session
//! overlays, no-ID sources and final snapshot augmentation are separate gates.
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

enum Phase { Headers, Payload, Validate }

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
        sources: Vec<Source>, names: HashMap<String, Value>, ordered: bool,
        statistics_units: usize, statistics_key_bytes: usize, enabled: bool, stopped: &AtomicBool) -> Option<Self>
    {
        // Never silently publish a checkpoint-only view of mutable active calls.
        if !collector.files.is_empty() || !collector.overflow_files.is_empty()
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
        Some(Self { inputs:Some(inputs), sources, phase:Phase::Headers, next:0, reader:None,
            metadata:BTreeMap::new(), partial:BTreeMap::new(), winners:BTreeMap::new(), slots:BTreeMap::new(),
            next_slots:BTreeMap::new(), thread_slots:BTreeMap::new(), source_threads:Vec::new(), names, ordered,
            statistics_units, statistics_key_bytes, reducers:None })
    }

    #[cfg(test)]
    pub(super) fn awaiting_validation(&self) -> bool { matches!(self.phase, Phase::Validate) }

    fn initialize(&mut self, collector: &Collector) {
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

    // Budget bounds source-header/page operations, not elapsed time or RSS. Each
    // reader page retains its existing 512-call / 16 MiB validation limits.
    pub(super) fn advance(mut self, collector: &Collector, selection: &CallSelection,
        enabled: bool, stopped: &AtomicBool, budget: usize) -> Option<Advance>
    {
        if budget == 0 { return None; }
        self.inputs = Some(self.inputs.take()?.validate(collector, selection, enabled, stopped)?);
        let budget = budget.min(16);
        match self.phase {
            Phase::Headers => {
                for _ in 0..budget {
                    if stopped.load(Ordering::Acquire) { return None; }
                    if self.next == self.sources.len() { break; }
                    let source = &self.sources[self.next];
                    let mut store = collector.checkpoints.snapshot_reader().ok().flatten()?;
                    let header = store.selected_checkpoint(&lam_history_store::ExpectedCursor {
                        source_key:&source.key, signature:Some(&source.signature) }, &[]).ok().flatten()?;
                    let projection = checkpoint::snapshot_projection(header, false)?;
                    if !projection.projection.calls.is_empty() { return None; }
                    let thread = projection.projection.id.clone()?;
                    let next_thread = self.thread_slots.len();
                    self.thread_slots.entry(thread.clone()).or_insert(next_thread);
                    self.metadata.entry(thread.clone()).or_insert_with(|| Projection::for_id(Some(thread.clone())))
                        .merge_snapshot(&projection.projection, None, &self.inputs.as_ref()?.time.cutoffs);
                    *self.partial.entry(thread.clone()).or_default() |= projection.partial;
                    if let Some(ids) = selection.calls.get(&source.key) {
                        for id in ids {
                            let key = (thread.clone(), id.clone());
                            let next = self.next_slots.entry(thread.clone()).or_default();
                            self.slots.entry(key.clone()).or_insert_with(|| { let slot = *next; *next += 1; slot });
                            self.winners.insert(key, self.next);
                        }
                    }
                    self.source_threads.push(thread);
                    self.next += 1;
                }
                if self.next == self.sources.len() {
                    self.initialize(collector);
                    self.phase = Phase::Payload;
                    self.next = 0;
                }
            }
            Phase::Payload => {
                for _ in 0..budget {
                    if stopped.load(Ordering::Acquire) { return None; }
                    if self.next == self.sources.len() {
                        self.phase = Phase::Validate;
                        break;
                    }
                    // Reverse payload arrival is safe only after the source-order
                    // winner and first-insertion slot plan has been completed.
                    let source_index = self.sources.len() - self.next - 1;
                    let source = &self.sources[source_index];
                    if self.reader.is_none() {
                        let ids = self.winners.iter().filter(|(_, winner)| **winner == source_index)
                            .map(|((_, id), _)| id.clone()).collect();
                        self.reader = Some(collector.checkpoints.snapshot_reader().ok().flatten()?
                            .into_selected_checkpoint_reader(&lam_history_store::ExpectedCursor {
                                source_key:&source.key, signature:Some(&source.signature) }, ids).ok().flatten()?);
                    }
                    let reader = self.reader.as_mut()?;
                    let page = reader.next_page().ok().flatten()?;
                    let mut projection = checkpoint::snapshot_projection(page, false)?;
                    let thread = projection.projection.id.as_deref()?;
                    if thread != self.source_threads[source_index] { return None; }
                    for (id, call) in &mut projection.projection.calls {
                        let key = (thread.to_owned(), id.clone());
                        if self.winners.get(&key) != Some(&source_index) { return None; }
                        if self.ordered { call.presentation_sequence = *self.slots.get(&key)?; }
                    }
                    let reducers = self.reducers.as_mut()?;
                    let time = &self.inputs.as_ref()?.time;
                    for (index, cutoff) in time.cutoffs.iter().copied().enumerate() {
                        if self.ordered {
                            reducers.files[index].ordered_thread(self.thread_slots[thread]);
                            reducers.commands[index].ordered_thread(self.thread_slots[thread]);
                        }
                        reducers.files[index].observe(&projection.projection, cutoff, self.inputs.as_ref()?.reducer_switches().1, None);
                        reducers.series[index].observe(&projection.projection, cutoff);
                        for (id, call) in &projection.projection.calls {
                            reducers.statistics[index].observe(thread, call);
                            reducers.commands[index].observe(thread, id, call);
                            reducers.skills[index].observe(thread, Some(thread), id, call);
                            reducers.rows[index].get_mut(thread)?.observe(id, call, &mut reducers.totals[index], &mut reducers.nested[index]);
                        }
                    }
                    for (id, call) in &projection.projection.calls {
                        reducers.failures.observe(thread, id, call);
                        reducers.observations.extend(call.activities(thread, id));
                    }
                    if reader.is_complete() { self.reader = None; self.next += 1; }
                }
            }
            Phase::Validate => {
                let (inputs, complete) = self.inputs.take()?.validate_sources(collector, selection, enabled, stopped, budget)?;
                self.inputs = Some(inputs);
                if complete {
                    let completed = self.finish(collector)?;
                    self.inputs.take()?.validate(collector, selection, enabled, stopped)?;
                    return Some(Advance::Complete(completed));
                }
            }
        }
        self.inputs = Some(self.inputs.take()?.validate(collector, selection, enabled, stopped)?);
        Some(Advance::Pending(self))
    }

    fn finish(&mut self, collector: &Collector) -> Option<CompletedCandidate> {
        let reducers = self.reducers.take()?;
        let reference = self.inputs.as_ref()?.time.reference;
        let cutoffs = self.inputs.as_ref()?.time.cutoffs;
        let files = reducers.files.map(|summary| { let (events,total,operations) = summary.finish(); json!({"events":events,"total":total,"operations":operations}) });
        let series = reducers.series.map(|series| { let (activity,tools) = series.finish(); json!({"activity":activity,"tools":tools}) });
        let statistics = reducers.statistics.into_iter().map(|summary| summary.finish()).collect::<Option<Vec<_>>>()?.try_into().ok()?;
        let switches = self.inputs.as_ref()?.reducer_switches().2;
        let commands = reducers.commands.map(|summary| {
            let (mut git,mut checks) = summary.finish();
            if !switches.git { git = json!({"events":[],"operations":{},"total":0}); }
            if !switches.checks { checks.clear(); }
            json!({"git":git,"checks":checks})
        });
        let skills = reducers.skills.map(|summary| { let (events,total,counts) = summary.finish(&self.names); json!({"events":events,"total":total,"counts":counts}) });
        let rows = reducers.rows.into_iter().zip(reducers.totals).zip(reducers.nested).map(|((summaries,totals),nested_totals)| {
            let rows: BTreeMap<_,_> = summaries.into_iter().map(|(thread,summary)| {
                let row = self.metadata[&thread].row_with_summary(self.partial[&thread], reference, summary);
                (thread,row)
            }).collect();
            json!({"rows":rows,"totals":totals,"nested_totals":nested_totals})
        }).collect::<Vec<_>>().try_into().ok()?;
        let (mcp_events,sql_events): (Vec<_>,Vec<_>) = reducers.observations.finish().into_iter().partition(|event| event.get("server").is_some());
        let sql_events = sqlite::identified(sql_events);
        let observations = cutoffs.map(|cutoff| json!({"mcp":mcp::summary(&mcp_events,cutoff,collector.activity_history_health),
            "sql":sqlite::window(&sql_events,cutoff,collector.activity_history_health)}));
        let failures: Vec<_> = reducers.failures.finish().into_iter().map(|(start,event)| json!({"start":start.map(project::stamp),"event":event})).collect();
        Some(CompletedCandidate { files,series,statistics,commands,skills,rows,observations,failures:json!(failures) })
    }
}
