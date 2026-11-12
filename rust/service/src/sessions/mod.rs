mod checkpoint;
mod commands;
mod core_logs;
mod desktop;
mod history_view;
pub(crate) mod instructions;
mod mcp;
mod metadata;
mod model_api;
mod project;
pub(crate) mod projects;
pub(crate) mod schedules;
mod sqlite;

use bytes::Bytes;
use checkpoint::{Anchor, Checkpoints};
use chrono::{Duration, Utc};
use lam_history_store::{ObservationView, StoreError, ERROR_PREVIEW_HOURS, ERROR_PREVIEW_LIMIT};
use lam_session_core::LINE_LIMIT;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use project::{now, stamp, Projection, SessionError, FILE_CALL_LIMIT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::SystemTime;
use tokio::sync::watch;
use walkdir::WalkDir;

const FILE_LIMIT: usize = 5000;
const WALK_LIMIT: usize = 40_000;
const ROUND_LIMIT: usize = 8 * 1024 * 1024;
const BUFFER_LIMIT: usize = 8 * 1024 * 1024;
const CALL_LIMIT: usize = 50_000;
const ERROR_LIMIT: usize = 1000;
const CHUNK_LIMIT: usize = 1024 * 1024;
const WINDOWS: [&str; 4] = ["1h", "24h", "7d", "all"];
const HOURS: [Option<i64>; 4] = [Some(1), Some(24), Some(168), None];

pub struct Snapshots {
    pub windows: [Bytes; 4],
    pub logs: [Bytes; 4],
    pub activity: Bytes,
    pub metadata_enabled: bool,
    pub logs_enabled: bool,
    pub debug_enabled: bool,
    version: u64,
    events: [OnceLock<Bytes>; 4],
}

impl Snapshots {
    pub fn window(query: &str) -> Option<usize> {
        Self::window_or(query, 1)
    }

    pub fn window_or(query: &str, default: usize) -> Option<usize> {
        if query.len() > 128 {
            return None;
        }
        let mut window = None;
        for (name, value) in form_urlencoded::parse(query.as_bytes()) {
            if name != "window" || window.is_some() {
                return None;
            }
            window = Some(WINDOWS.iter().position(|item| *item == value)?);
        }
        Some(window.unwrap_or(default))
    }

    pub fn event(&self, window: usize) -> Bytes {
        self.events[window].get_or_init(|| {
            let version = self.version;
            let name = WINDOWS[window];
            let mut frame = format!(
                "id: {version}\nevent: snapshot\ndata: {{\"version\":{version},\"window\":\"{name}\",\"snapshot\":"
            ).into_bytes();
            frame.extend_from_slice(&self.windows[window]);
            frame.extend_from_slice(b",\"logs\":");
            frame.extend_from_slice(&self.logs[window]);
            frame.extend_from_slice(b"}\n\n");
            frame.into()
        }).clone()
    }
}

pub struct Monitor {
    worker: Option<Worker>,
    metadata_enabled: Arc<AtomicBool>,
    journal: Arc<Mutex<crate::journal::Journal>>,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
}

struct Worker {
    manual: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    wake: SyncSender<()>,
    handle: JoinHandle<()>,
}

impl Monitor {
    pub fn debug(&self) -> Arc<Mutex<crate::performance_debug::PerformanceDebug>> {
        Arc::clone(&self.debug)
    }
    pub fn journal(&self) -> Arc<Mutex<crate::journal::Journal>> {
        Arc::clone(&self.journal)
    }
    pub fn metadata_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.metadata_enabled)
    }
    pub fn refresh_sender(&self) -> Option<(SyncSender<()>, Arc<AtomicBool>)> {
        self.worker
            .as_ref()
            .map(|worker| (worker.wake.clone(), Arc::clone(&worker.manual)))
    }

    pub fn start(
        home: &Path,
        enabled: bool,
        frontend_revision: Option<String>,
    ) -> io::Result<(Self, watch::Receiver<Snapshots>)> {
        let root = home.join("sessions");
        let mut collector = Collector::new(home.join("monitoring"), enabled, frontend_revision);
        let journal = Arc::clone(&collector.journal);
        let debug = Arc::clone(&collector.debug);
        let initial = if enabled {
            ("waiting", "starting")
        } else {
            ("disabled", "disabled")
        };
        let (publish, snapshots) = watch::channel(collector.snapshots(initial.0, initial.1, 0));
        let (wake, signal) = mpsc::sync_channel(1);
        let manual_refresh = Arc::new(AtomicBool::new(false));
        let manual_signal = Arc::clone(&manual_refresh);
        let metadata_enabled = Arc::new(AtomicBool::new(true));
        let metadata_control = Arc::clone(&metadata_enabled);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let home = home.to_path_buf();
        let events = wake.clone();
        let desktop_roots = if enabled {
            let user = crate::platform_home().unwrap_or_else(|| home.clone());
            let configured =
                crate::default_codex_home(std::env::var_os("CODEX_HOME"), Some(user.clone()))
                    .unwrap_or_else(|| home.clone());
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
            let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
            desktop::roots(
                &home,
                &configured,
                &user,
                local.as_deref(),
                state.as_deref(),
                config.as_deref(),
            )
        } else {
            Vec::new()
        };
        let desktop_wake = wake.clone();
        let schedule_wake = wake.clone();
        let handle = thread::Builder::new()
            .name("session-collector".into())
            .spawn(move || {
                if !enabled {
                    let mut version = 0;
                    while signal.recv().is_ok() && !stop.load(Ordering::Acquire) {
                        let began = std::time::Instant::now();
                        let timing_enabled = collector
                            .debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .enabled();
                        let cpu_started = timing_enabled
                            .then(crate::performance_debug::process_cpu_ms)
                            .flatten();
                        collector.metadata_enabled = metadata_control.load(Ordering::Acquire);
                        collector.logs_enabled = collector
                            .journal
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .enabled();
                        let reference = now();
                        let history_timer =
                            crate::performance_debug::PhaseTimer::start(timing_enabled);
                        collector.refresh_activity(reference);
                        collector.refresh_error_history(reference);
                        collector.updated = Some(reference);
                        let mut metrics =
                            json!({"refresh_ms":began.elapsed().as_secs_f64()*1000.0});
                        if let Some(cpu) = cpu_started.and_then(|before| {
                            crate::performance_debug::process_cpu_ms()
                                .map(|after| (after - before).max(0.0))
                        }) {
                            metrics["cpu_ms"] = json!(cpu);
                        }
                        if let Some(phase) = history_timer.map(|timer| timer.finish()) {
                            metrics["phase_metrics"] = json!({"history":phase});
                        }
                        collector
                            .debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .record("collection", metrics);
                        version += 1;
                        publish.send_replace(collector.snapshots("disabled", "disabled", version));
                    }
                    return;
                }
                let watch_failed = Arc::new(AtomicBool::new(false));
                let reenroll = Arc::new(AtomicBool::new(false));
                let schedule_dirty = Arc::new(AtomicBool::new(true));
                let mut watcher = start_watch(
                    &home,
                    &root,
                    events,
                    Arc::clone(&watch_failed),
                    Arc::clone(&reenroll),
                    Arc::clone(&schedule_dirty),
                )
                .ok();
                let mut desktop_failed = Arc::new(AtomicBool::new(false));
                let mut desktop_watcher = desktop::DesktopWatcher::new(
                    &desktop_roots,
                    desktop_wake.clone(),
                    Arc::clone(&desktop_failed),
                )
                .ok();
                let schedule_roots = [desktop::DesktopRoot {
                    base: home.clone(),
                    path: home.join("sqlite"),
                }];
                let mut schedule_failed = Arc::new(AtomicBool::new(false));
                let mut schedule_watcher = desktop::DesktopWatcher::new_with_signal(
                    &schedule_roots,
                    schedule_wake.clone(),
                    Arc::clone(&schedule_failed),
                    Some(Arc::clone(&schedule_dirty)),
                )
                .ok();
                let mut enrolled = None;
                let mut diagnostic_watches = BTreeMap::<PathBuf, (u64, u64)>::new();
                let mut state_watches = BTreeMap::<PathBuf, (u64, u64)>::new();
                let mut schedule_threads = BTreeSet::<String>::new();
                let mut rescan_core_logs = false;
                let mut rescan_desktop_logs = false;
                let global_state = home.join(".codex-global-state.json");
                let mut global_identity = None;
                let mut previous = initial;
                let mut version = 0;
                while !stop.load(Ordering::Acquire) {
                    let requested = manual_refresh.swap(false, Ordering::AcqRel);
                    if requested { schedule_dirty.store(true, Ordering::Release); }
                    let collection_began = std::time::Instant::now();
                    let timing_enabled = collector
                        .debug
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .enabled();
                    let cpu_started = timing_enabled
                        .then(crate::performance_debug::process_cpu_ms)
                        .flatten();
                    let desired_state: BTreeMap<_, _> = fs::read_dir(&home)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .filter_map(|entry| {
                            let name = entry.file_name();
                            if name != "session_index.jsonl"
                                && !schedules::state_filename(name.to_str()?)
                            {
                                return None;
                            }
                            let path = entry.path();
                            let metadata = fs::symlink_metadata(&path).ok()?;
                            (metadata.is_file() && !redirected(&metadata))
                                .then(|| (path, file_identity(&metadata)))
                        })
                        .collect();
                    if desired_state != state_watches {
                        if let Some(watcher) = watcher.as_mut() {
                            state_watches.retain(|path, identity| {
                                let keep = desired_state.get(path) == Some(identity);
                                if !keep {
                                    let _ = watcher.unwatch(path);
                                }
                                keep
                            });
                            for (path, identity) in desired_state {
                                if let std::collections::btree_map::Entry::Vacant(entry) =
                                    state_watches.entry(path)
                                {
                                    if watcher
                                        .watch(entry.key(), RecursiveMode::NonRecursive)
                                        .is_ok()
                                    {
                                        entry.insert(identity);
                                    } else {
                                        watch_failed.store(true, Ordering::Release);
                                    }
                                }
                            }
                        }
                        schedule_dirty.store(true, Ordering::Release);
                    }
                    let global_current = fs::symlink_metadata(&global_state)
                        .ok()
                        .filter(|meta| meta.is_file() && !redirected(meta))
                        .map(|meta| file_identity(&meta));
                    if global_current != global_identity {
                        if let Some(watcher) = watcher.as_mut() {
                            if global_identity.is_some() {
                                let _ = watcher.unwatch(&global_state);
                            }
                            if global_current.is_some()
                                && watcher
                                    .watch(&global_state, RecursiveMode::NonRecursive)
                                    .is_err()
                            {
                                watch_failed.store(true, Ordering::Release);
                            }
                        }
                        global_identity = global_current;
                        schedule_dirty.store(true, Ordering::Release);
                    }
                    let current = fs::symlink_metadata(&root)
                        .ok()
                        .filter(|meta| meta.is_dir() && !redirected(meta))
                        .map(|meta| file_identity(&meta));
                    if current != enrolled {
                        if let Some(watcher) = watcher.as_mut() {
                            if enrolled.is_some() {
                                let _ = watcher.unwatch(&root);
                            }
                            if current.is_some()
                                && watcher.watch(&root, RecursiveMode::Recursive).is_err()
                            {
                                watch_failed.store(true, Ordering::Release);
                            }
                        }
                        enrolled = current;
                    }
                    if cfg!(target_os = "macos")
                        && reenroll.swap(false, Ordering::AcqRel)
                        && current.is_some()
                    {
                        if let Some(watcher) = watcher.as_mut() {
                            if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
                                watch_failed.store(true, Ordering::Release);
                            }
                        }
                    }
                    let previous_checkpoint = collector.checkpoints.report;
                    let session_timer = crate::performance_debug::PhaseTimer::start(timing_enabled);
                    let (health, pending, changed) = collector.refresh(&root, &stop);
                    let session_phase = session_timer.map(|timer| timer.finish());
                    if changed {
                        let selected: BTreeSet<_> = collector
                            .files
                            .values()
                            .filter_map(|state| state.projection.id.clone())
                            .collect();
                        if schedule_threads != selected {
                            schedule_threads = selected;
                            schedule_dirty.store(true, Ordering::Release);
                        }
                    }
                    let source = core_logs::discover(&home);
                    if let Some(watcher) = watcher.as_mut() {
                        let desired = source
                            .as_ref()
                            .ok()
                            .and_then(Option::as_ref)
                            .map_or_else(BTreeMap::new, core_logs::Source::watches);
                        diagnostic_watches.retain(|path, identity| {
                            let keep = desired.get(path) == Some(identity);
                            if !keep {
                                let _ = watcher.unwatch(path);
                            }
                            keep
                        });
                        for (path, identity) in desired {
                            if let std::collections::btree_map::Entry::Vacant(entry) =
                                diagnostic_watches.entry(path)
                            {
                                if watcher
                                    .watch(entry.key(), RecursiveMode::NonRecursive)
                                    .is_ok()
                                {
                                    entry.insert(identity);
                                } else {
                                    watch_failed.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                    let core_timer = crate::performance_debug::PhaseTimer::start(timing_enabled);
                    let diagnostic_changed = collector.refresh_core(source, now());
                    let core_phase = core_timer.map(|timer| timer.finish());
                    let needs_schedule_watch = schedule_failed.load(Ordering::Acquire)
                        || schedule_watcher
                            .as_ref()
                            .is_none_or(|watcher| watcher.needs_reenroll(&schedule_roots));
                    let schedule_watch_ok = if needs_schedule_watch {
                        let next_failed = Arc::new(AtomicBool::new(false));
                        match desktop::DesktopWatcher::new_with_signal(
                            &schedule_roots,
                            schedule_wake.clone(),
                            Arc::clone(&next_failed),
                            Some(Arc::clone(&schedule_dirty)),
                        ) {
                            Ok(next) => {
                                let complete = !next.partial_failure;
                                schedule_watcher = Some(next);
                                schedule_failed = next_failed;
                                schedule_dirty.store(true, Ordering::Release);
                                complete
                            }
                            Err(_) => false,
                        }
                    } else {
                        true
                    };
                    let metadata = metadata_control.load(Ordering::Acquire);
                    let metadata_changed = metadata != collector.metadata_enabled;
                    collector.metadata_enabled = metadata;
                    let schedule_changed = if !metadata {
                        let changed = collector.schedules["health"] != "disabled"
                            || collector.schedule_report.is_some();
                        collector.schedules = json!({"items":[],"health":"disabled"});
                        collector.schedule_report = None;
                        collector.thread_metadata.clear();
                        collector.project_details.clear();
                        collector.dots = std::array::from_fn(|index| json!({"window":WINDOWS[index],"activity_items_scope":"latest_loaded_snapshot","activity":[]}));
                        changed
                    } else if metadata_changed || schedule_dirty.swap(false, Ordering::AcqRel) {
                        let reference = now();
                        let mut metadata = schedules::background_metadata(&home, &schedule_threads);
                        let projects = projects::read(&home, &mut metadata);
                        let dots = crate::dots::background(&home, &metadata, reference);
                        schedules::merge_remote_state(&home, &mut metadata);
                        let (schedules, report) = schedules::read(&home, reference, &metadata);
                        let changed = schedules != collector.schedules
                            || dots != collector.dots
                            || metadata != collector.thread_metadata
                            || projects != collector.project_details
                            || collector.schedule_report.as_ref().is_none_or(|previous| {
                                let mut previous = previous.clone();
                                let mut current = report.clone();
                                previous.as_object_mut().map(|row| row.remove("checked_at"));
                                current.as_object_mut().map(|row| row.remove("checked_at"));
                                previous != current
                            });
                        collector.schedules = schedules;
                        collector.dots = dots;
                        collector.thread_metadata = metadata;
                        collector.project_details = projects;
                        collector.schedule_report = Some(report);
                        changed
                    } else {
                        false
                    };
                    // Register before scanning so changes during the scan remain observable.
                    let needs_desktop_watch = desktop_failed.load(Ordering::Acquire)
                        || desktop_watcher
                            .as_ref()
                            .is_none_or(|watcher| watcher.needs_reenroll(&desktop_roots));
                    let desktop_watch_ok = if needs_desktop_watch {
                        let next_failed = Arc::new(AtomicBool::new(false));
                        match desktop::DesktopWatcher::new(
                            &desktop_roots,
                            desktop_wake.clone(),
                            Arc::clone(&next_failed),
                        ) {
                            Ok(next) => {
                                let complete = !next.partial_failure;
                                desktop_watcher = Some(next);
                                desktop_failed = next_failed;
                                complete
                            }
                            Err(_) => false,
                        }
                    } else {
                        true
                    };
                    let mut before = collector.desktop.source.clone();
                    if let Some(source) = before.as_object_mut() {
                        source.remove("checked_at");
                    }
                    let desktop_timer = crate::performance_debug::PhaseTimer::start(timing_enabled);
                    let batch = collector.desktop.refresh(&desktop_roots, now());
                    let desktop_pending = batch.live_pending || batch.backfill_pending;
                    let desktop_commit =
                        collector.desktop.commit(&mut collector.checkpoints, now());
                    let desktop_phase = desktop_timer.map(|timer| timer.finish());
                    let logs_enabled = collector
                        .journal
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .enabled();
                    let logs_changed = collector.logs_enabled != logs_enabled;
                    collector.logs_enabled = logs_enabled;
                    if logs_changed {
                        rescan_core_logs = logs_enabled;
                        rescan_desktop_logs = logs_enabled;
                    }
                    if !logs_enabled {
                        collector.core.logs.clear();
                        collector.desktop.logs.clear();
                    }
                    let mut rescan_ready = false;
                    if rescan_core_logs && collector.core.reset_log_scan() {
                        rescan_core_logs = false;
                        rescan_ready = true;
                    }
                    if rescan_desktop_logs && collector.desktop.reset_log_scan() {
                        rescan_desktop_logs = false;
                        rescan_ready = true;
                    }
                    let mut after = collector.desktop.source.clone();
                    if let Some(source) = after.as_object_mut() {
                        source.remove("checked_at");
                    }
                    let desktop_changed = before != after || matches!(desktop_commit, Ok(Some(_)));
                    let desktop_retry =
                        desktop_commit.is_err() || (batch.unavailable && batch.bytes_read == 0);
                    let watch_health = if watcher.is_none()
                        || watch_failed.load(Ordering::Acquire)
                        || !desktop_watch_ok
                        || !schedule_watch_ok
                        || schedule_failed.load(Ordering::Acquire)
                        || desktop_failed.load(Ordering::Acquire)
                    {
                        "unavailable"
                    } else {
                        "ok"
                    };
                    if changed
                        || diagnostic_changed
                        || schedule_changed
                        || metadata_changed
                        || logs_changed
                        || desktop_changed
                        || previous != (health, watch_health)
                        || previous_checkpoint != collector.checkpoints.report
                        || requested
                    {
                        previous = (health, watch_health);
                        let reference = now();
                        let history_timer =
                            crate::performance_debug::PhaseTimer::start(timing_enabled);
                        collector.refresh_activity(reference);
                        collector.refresh_error_history(reference);
                        let history_phase = history_timer.map(|timer| timer.finish());
                        collector.updated = Some(reference);
                        let mut metrics =
                            json!({"refresh_ms":collection_began.elapsed().as_secs_f64()*1000.0});
                        if let Some(cpu) = cpu_started.and_then(|before| {
                            crate::performance_debug::process_cpu_ms()
                                .map(|after| (after - before).max(0.0))
                        }) {
                            metrics["cpu_ms"] = json!(cpu);
                        }
                        if timing_enabled {
                            let mut phases = serde_json::Map::new();
                            if let Some(phase) = session_phase {
                                phases.insert("sessions".into(), phase);
                            }
                            if let Some(phase) =
                                crate::performance_debug::combine_phases(core_phase, desktop_phase)
                            {
                                phases.insert("diagnostics".into(), phase);
                            }
                            if let Some(phase) = history_phase {
                                phases.insert("history".into(), phase);
                            }
                            metrics["phase_metrics"] = Value::Object(phases);
                        }
                        collector
                            .debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .record("collection", metrics);
                        version += 1;
                        publish.send_replace(collector.snapshots(health, watch_health, version));
                    }
                    if !rescan_ready
                        && !pending
                        && !collector.core.backfill()
                        && (!desktop_pending || desktop_retry)
                        && signal.recv().is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok((
            Self {
                metadata_enabled,
                journal,
                debug,
                worker: Some(Worker {
                    manual: manual_signal,
                    stopped,
                    wake,
                    handle,
                }),
            },
            snapshots,
        ))
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stopped.store(true, Ordering::Release);
            let _ = worker.wake.try_send(());
            let _ = worker.handle.join();
        }
    }
}

fn start_watch(
    home: &Path,
    root: &Path,
    events: SyncSender<()>,
    failed: Arc<AtomicBool>,
    reenroll: Arc<AtomicBool>,
    schedule_dirty: Arc<AtomicBool>,
) -> notify::Result<RecommendedWatcher> {
    let root = root.to_path_buf();
    let watched_home = home.to_path_buf();
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            let notify = match result {
                Ok(event) => {
                    let global = event.need_rescan()
                        || (!event.kind.is_access()
                            && event.paths.iter().any(|path| {
                                path == &watched_home.join(".codex-global-state.json")
                                    || path == &watched_home.join("session_index.jsonl")
                                    || (path.parent() == Some(watched_home.as_path())
                                        && path
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .is_some_and(schedules::state_filename))
                                    || (cfg!(target_os = "macos") && path == &watched_home)
                            }));
                    if global {
                        schedule_dirty.store(true, Ordering::Release);
                    }
                    if cfg!(target_os = "macos")
                        && event.kind.is_create()
                        && event.paths.iter().any(|path| path.starts_with(&root))
                    {
                        reenroll.store(true, Ordering::Release);
                    }
                    global
                        || event.need_rescan()
                        || !event.kind.is_access()
                            && event.paths.iter().any(|path| {
                                path.starts_with(&root)
                                    || core_logs::related_event(&watched_home, path)
                                    // kqueue can report the directory or just one new child.
                                    || (cfg!(target_os = "macos")
                                        && (path == &watched_home
                                            || (event.kind.is_create()
                                                && path.parent() == Some(watched_home.as_path()))))
                            })
                }
                Err(_) => {
                    failed.store(true, Ordering::Release);
                    true
                }
            };
            if notify {
                let _ = events.try_send(());
            }
        },
        Config::default().with_follow_symlinks(false),
    )?;
    watcher.watch(home, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

#[derive(Default)]
struct Session {
    projection: Projection,
    offset: u64,
    size: u64,
    partial: Vec<u8>,
    discard: bool,
    identity: Option<(u64, u64)>,
    modified: Option<SystemTime>,
    anchor: Option<Anchor>,
    checkpoint_dirty: bool,
    tool_errors_unanchored_saved: bool,
    checkpoint_reset: bool,
    checkpoint_signature: Option<String>,
}

impl Session {
    fn error_events<'a>(
        &'a self,
        path: &'a Path,
        errors: &'a [SessionError],
    ) -> impl Iterator<Item = Value> + 'a {
        let file = project::identifier(path.file_name().and_then(|name| name.to_str()));
        errors.iter().map(move |error| {
            let timestamp = error.time.map(stamp);
            let source = if error.failure.is_some() { "tool_result" } else { "session" };
            let reason = error.failure.is_none().then_some(error.reason.as_str());
            let content_id = error.record_hash.as_deref().map(|hash| {
                let identity = [
                    Some(source),
                    self.projection.id.as_deref(),
                    error.call_id.as_deref(),
                    timestamp.as_deref(),
                    reason,
                    Some(&error.code),
                    Some(hash),
                ]
                .map(|field| serde_json::to_string(&field).expect("projected identifier"))
                .join(", ");
                format!("{:x}", Sha256::digest(format!("[{identity}]")))
            });
            let mut event = error.failure.as_ref().and_then(|failure| serde_json::to_value(failure).ok())
                .unwrap_or_else(|| json!({"code":error.code,"reason":reason,"severity":error.severity,"file":file}));
            let fields = event.as_object_mut().expect("projected error");
            fields.insert("timestamp".into(), json!(timestamp));
            fields.insert("thread_id".into(), json!(self.projection.id));
            fields.insert("source".into(), source.into());
            fields.insert("category".into(), if error.failure.is_some() { "tool" } else { "conversation" }.into());
            fields.insert("severity".into(), json!(error.severity));
            if error.failure.is_some() {
                fields.insert("thread_name".into(), Value::Null);
                fields.insert("call_id".into(), json!(error.call_id));
            }
            fields.insert("record_offset".into(), json!(error.record_offset));
            fields.insert("record_hash".into(), json!(error.record_hash));
            fields.insert("content_id".into(), json!(content_id));
            event
        })
    }

    fn pending(&self) -> bool {
        self.identity.is_none() || self.offset < self.size
    }

    fn unsaved_tool_errors(&self) -> bool {
        self.projection.id.is_some()
            && (self.anchor.is_some() || !self.tool_errors_unanchored_saved)
            && (self.projection.dirty_calls.iter().any(|id| {
                self.projection
                    .calls
                    .get(id)
                    .is_some_and(|call| call.has_failure())
            }) || self
                .projection
                .retired_observations
                .iter()
                .any(|(_, call)| call.has_failure()))
    }

    fn new(path: &Path) -> Self {
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(project::filename_identity);
        Self {
            projection: Projection::for_id(id),
            checkpoint_reset: true,
            ..Self::default()
        }
    }

    fn read(
        &mut self,
        root: &Path,
        path: &Path,
        budget: usize,
        call_limit: u32,
        checkpoints: &mut Checkpoints,
    ) -> io::Result<(usize, u64, bool)> {
        let meta = fs::symlink_metadata(path)?;
        if redirected(&meta) || !meta.is_file() {
            return Err(io::Error::other("Unsafe session file"));
        }
        let identity = file_identity(&meta);
        let modified = meta.modified().ok();
        if self.identity == Some(identity)
            && self.size == meta.len()
            && self.offset == meta.len()
            && self.modified == modified
            && (self.anchor.is_some() || !self.checkpoint_dirty)
        {
            return Ok((0, 0, false));
        }
        let mut stream = open_source(root, path)?;
        let meta = stream.metadata()?;
        if redirected(&meta) || !meta.is_file() {
            return Err(io::Error::other("Unsafe session file"));
        }
        let identity = file_identity(&meta);
        let modified = meta.modified().ok();
        let size = meta.len();
        let restored = if self.identity.is_none() {
            let loaded = checkpoints.restore(root, path, &mut stream, identity, size, call_limit);
            self.checkpoint_signature = loaded.signature;
            loaded.state
        } else {
            None
        };
        let mut changed = restored.is_some();
        if let Some(restored) = restored {
            self.offset = restored.anchor.offset;
            self.anchor = Some(restored.anchor);
            self.projection = restored.projection;
            self.discard = restored.discard;
            self.identity = Some(identity);
            self.size = size;
            self.modified = modified;
            self.checkpoint_dirty = !self.projection.pending_errors.is_empty();
            self.checkpoint_reset = false;
        }
        let mut replaced_prefix = false;
        if self.identity == Some(identity) && size > self.size {
            if let Some(anchor) = &self.anchor {
                replaced_prefix =
                    checkpoints.sample(&mut stream, identity, anchor.offset)? != *anchor;
            }
            replaced_prefix |=
                !checkpoints.partial_matches(&mut stream, self.offset, &self.partial)?;
        }
        let reset = replaced_prefix
            || self.identity != Some(identity)
            || size < self.size
            || size == self.size && self.modified != modified;
        if reset {
            if self.identity.is_some()
                && (self.anchor.is_some() || self.checkpoint_signature.is_some())
            {
                checkpoints.invalidate();
            }
            let signature = self.checkpoint_signature.take();
            *self = Self::new(path);
            self.checkpoint_signature = signature;
        }
        changed |= reset;
        let amount = size.saturating_sub(self.offset).min(budget as u64);
        if amount == 0 {
            self.identity = Some(identity);
            self.modified = modified;
            self.size = size;
            if reset || self.checkpoint_dirty && self.anchor.is_none() {
                self.capture_anchor(&mut stream, &meta, checkpoints);
            }
            return Ok((0, 0, changed));
        }
        stream.seek(SeekFrom::Start(self.offset))?;
        let mut raw = Vec::with_capacity(amount as usize);
        (&mut stream).take(amount).read_to_end(&mut raw)?;
        if !raw.is_empty() {
            self.tool_errors_unanchored_saved = false;
        }
        let base = self.complete_offset();
        self.identity = Some(identity);
        self.modified = modified;
        self.size = size;
        self.offset += raw.len() as u64;
        let mut data = std::mem::take(&mut self.partial);
        data.extend_from_slice(&raw);
        let chunk = lam_session_core::forward(&data, self.discard, LINE_LIMIT);
        self.partial = data[chunk.tail].to_vec();
        self.discard = chunk.discard;
        let mut lines = chunk.lines;
        let mut malformed = 0;
        while let Some((position, line)) =
            lam_session_core::next_line(&data, &mut lines, LINE_LIMIT, false)
        {
            malformed += u64::from(!self.projection.consume(line, base + position as u64));
        }
        if self
            .anchor
            .as_ref()
            .is_none_or(|anchor| anchor.offset != self.complete_offset())
        {
            self.capture_anchor(&mut stream, &meta, checkpoints);
        }
        Ok((raw.len(), malformed, changed || !raw.is_empty()))
    }

    fn complete_offset(&self) -> u64 {
        self.offset - self.partial.len() as u64
    }

    fn capture_anchor(
        &mut self,
        file: &mut File,
        before: &Metadata,
        checkpoints: &mut Checkpoints,
    ) {
        self.anchor = None;
        self.checkpoint_dirty = true;
        let result = checkpoints
            .sample(file, file_identity(before), self.complete_offset())
            .and_then(|anchor| file.metadata().map(|after| (anchor, after)));
        match result {
            Ok((anchor, after))
                if before.len() == after.len()
                    && before.modified().ok() == after.modified().ok() =>
            {
                self.anchor = Some(anchor);
            }
            Ok(_) => {}
            Err(_) => checkpoints.write_failed(),
        }
    }
}

fn redirected(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.is_symlink()
}

fn file_identity(metadata: &Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        (0, metadata.creation_time())
    }
}

#[cfg(unix)]
fn open_source(root: &Path, path: &Path) -> io::Result<File> {
    use rustix::fs::{open, openat, Mode, OFlags};
    let relative = path.strip_prefix(root).map_err(io::Error::other)?;
    let mut components = relative.components().peekable();
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let mut parent = open(root, flags | OFlags::DIRECTORY, Mode::empty())?;
    while let Some(std::path::Component::Normal(name)) = components.next() {
        if components.peek().is_none() {
            return Ok(openat(&parent, name, flags, Mode::empty())?.into());
        }
        parent = openat(&parent, name, flags | OFlags::DIRECTORY, Mode::empty())?;
    }
    Err(io::Error::other("Invalid session path"))
}

#[cfg(windows)]
fn open_source(root: &Path, path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    for directory in path.ancestors().skip(1).take_while(|item| *item != root) {
        if redirected(&fs::symlink_metadata(directory)?) {
            return Err(io::Error::other("Unsafe session directory"));
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x00200000)
        .open(path)
}

fn visible_logs<'a>(
    records: impl Iterator<Item = &'a Value>,
    boundary: Option<chrono::DateTime<Utc>>,
) -> Vec<&'a Value> {
    let mut positions = BTreeMap::<String, usize>::new();
    let mut visible = Vec::new();
    for event in records {
        let timestamp = event["timestamp"]
            .as_str()
            .and_then(lam_history_store::parse_utc_stamp)
            .map(|time| json!(["utc_millis", time.timestamp_millis()]))
            .unwrap_or_else(|| json!(["raw", event["timestamp"]]));
        let keys = [
            "source",
            "category",
            "severity",
            "code",
            "module",
            "thread_id",
            "call_id",
            "attempt",
            "index",
            "record_id",
        ];
        let identity = serde_json::to_string(&(
            timestamp,
            keys.iter().map(|key| &event[*key]).collect::<Vec<_>>(),
        ))
        .expect("projected identity");
        if let Some(position) = positions.get(&identity) {
            visible[*position] = event;
        } else {
            positions.insert(identity, visible.len());
            visible.push(event);
        }
    }
    visible.retain(|event| {
        !boundary.is_some_and(|cutoff| {
            event["timestamp"]
                .as_str()
                .and_then(lam_history_store::parse_utc_stamp)
                .is_none_or(|time| time < cutoff)
        })
    });
    visible.sort_by(|left, right| right["timestamp"].as_str().cmp(&left["timestamp"].as_str()));
    visible
}

fn storage_error_type(error: &StoreError) -> &'static str {
    match error {
        StoreError::Sql(_) => "SQLiteError",
        StoreError::Io(_) | StoreError::UnsafePath => "OSError",
        _ => "ValueError",
    }
}

fn apply_thread_metadata(row: &mut Value, metadata: Option<&Value>, metadata_only: bool) {
    row["metadata_only"] = json!(metadata_only);
    let Some(metadata) = metadata else { return };
    row["thread_name"] = metadata["name"].clone();
    row["host_id"] = metadata["host_id"].clone();
    row["archived"] = metadata["archived"].clone();
    row["project_id"] = metadata["project_id"].clone();
    row["project_name"] = metadata["project_name"].clone();
    row["project_kind"] = metadata["project_kind"].clone();
    row["project_scope"] = metadata
        .get("project_scope")
        .cloned()
        .unwrap_or_else(|| json!("unknown"));
    row["has_schedule"] = metadata
        .get("has_schedule")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .into();
    let observed_execution = row["execution"].as_object().cloned().unwrap_or_default();
    row["execution"] = metadata
        .get("execution")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(execution) = row["execution"].as_object_mut() {
        execution.extend(observed_execution);
    }
    let environment = metadata["environment"].as_str().unwrap_or("unknown");
    row["environment"] = json!(if environment == "unknown" && !metadata_only {
        "local"
    } else {
        environment
    });
    for (target, source) in [
        ("created_at", "created_at"),
        ("updated_at", "last_run_at"),
        ("model", "model"),
        ("reasoning_effort", "reasoning_effort"),
        ("activity_type", "activity_type"),
        ("status", "status"),
        ("trigger", "kind"),
    ] {
        let incoming = &metadata[source];
        if incoming.is_null() {
            continue;
        }
        let previous = &row[target];
        let replace = metadata_only
            || previous.is_null()
            || matches!(previous.as_str(), Some("unknown" | "observed"))
            || (target == "trigger"
                && matches!(incoming.as_str(), Some("dot" | "schedule" | "heartbeat")));
        if replace {
            row[target] = incoming.clone();
            if target == "status" && row["status_updated_at"].is_null() {
                row["status_updated_at"] = metadata["last_run_at"].clone();
                row["status_source"] = json!("catalog_status");
            }
        }
    }
}

struct Collector {
    backend_revision: String,
    frontend_revision: Option<String>,
    files: BTreeMap<PathBuf, Session>,
    cursor: usize,
    read_bytes: u64,
    malformed: u64,
    started: chrono::DateTime<Utc>,
    updated: Option<chrono::DateTime<Utc>>,
    checkpoints: Checkpoints,
    activity_history: Arc<Vec<Value>>,
    activity_view: Option<ObservationView>,
    activity_history_health: &'static str,
    error_history: Vec<Value>,
    error_history_read_error: Option<&'static str>,
    core: core_logs::Core,
    desktop: desktop::DesktopCollector,
    schedules: Value,
    dots: [Value; 4],
    thread_metadata: HashMap<String, Value>,
    project_details: BTreeMap<String, Value>,
    schedule_report: Option<Value>,
    metadata_enabled: bool,
    logs_enabled: bool,
    journal: Arc<Mutex<crate::journal::Journal>>,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
}

impl Collector {
    fn new(directory: PathBuf, enabled: bool, frontend_revision: Option<String>) -> Self {
        let started = now();
        let instance = format!(
            "{}:{}:{}",
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            started.timestamp_nanos_opt().unwrap_or_default()
        );
        let backend_revision = format!("{:x}", Sha256::digest(instance))[..16].to_owned();
        let debug = Arc::new(Mutex::new(crate::performance_debug::PerformanceDebug::new(
            &directory,
            backend_revision.clone(),
        )));
        let mut journal = crate::journal::Journal::new(&directory);
        journal.event("started");
        let mut collector = Self {
            files: BTreeMap::new(),
            cursor: 0,
            read_bytes: 0,
            malformed: 0,
            started,
            backend_revision,
            frontend_revision,
            updated: None,
            checkpoints: Checkpoints::new(directory, enabled),
            activity_history: Arc::new(Vec::new()),
            activity_view: None,
            activity_history_health: "waiting",
            error_history: Vec::new(),
            error_history_read_error: None,
            core: core_logs::Core::new(enabled),
            desktop: desktop::DesktopCollector::default(),
            schedules: json!({"items":[],"health":if enabled {"waiting"} else {"disabled"}}),
            dots: std::array::from_fn(
                |index| json!({"window":WINDOWS[index],"activity_items_scope":"latest_loaded_snapshot","activity":[]}),
            ),
            schedule_report: None,
            thread_metadata: HashMap::new(),
            project_details: BTreeMap::new(),
            metadata_enabled: true,
            logs_enabled: true,
            journal: Arc::new(Mutex::new(journal)),
            debug,
        };
        collector.desktop.source =
            json!({"source":"codex_desktop","health":if enabled {"waiting"} else {"disabled"}});
        if !enabled {
            match collector.checkpoints.observed_history() {
                Ok(events) => {
                    collector.activity_history = Arc::new(events);
                    collector.activity_history_health = "ok";
                }
                Err(_) => collector.activity_history_health = "unavailable",
            }
            collector.updated = Some(now());
        }
        collector
    }

    fn refresh_activity(&mut self, reference: chrono::DateTime<Utc>) {
        let cutoffs = HOURS.map(|hours| hours.map(|hours| reference - Duration::hours(hours)));
        match self.checkpoints.observed_view(reference, &cutoffs) {
            Ok(view) => {
                self.activity_history = Arc::clone(&view.events);
                self.activity_view = Some(view);
                self.activity_history_health = "ok";
            }
            Err(_) => {
                self.activity_view = None;
                self.activity_history_health = "unavailable";
            }
        }
    }

    fn refresh_core(
        &mut self,
        source: io::Result<Option<core_logs::Source>>,
        reference: chrono::DateTime<Utc>,
    ) -> bool {
        let (stored, ready) = self.flush_diagnostics(reference);
        if !ready {
            return stored;
        }
        let changed = self.core.refresh(source, reference);
        let (flushed, _) = self.flush_diagnostics(reference);
        stored || changed || flushed
    }

    fn flush_diagnostics(&mut self, reference: chrono::DateTime<Utc>) -> (bool, bool) {
        if !self.core.has_pending() {
            return (false, true);
        }
        if let Err(error) = self.checkpoints.save_diagnostics(
            &self.core.pending_sql,
            &self.core.pending_errors,
            reference,
        ) {
            let error_type = Some(storage_error_type(&error));
            let changed = self.core.storage_error != error_type;
            self.core.storage_health = "unavailable";
            self.core.storage_error = error_type;
            return (changed, false);
        }
        self.core.pending_sql.clear();
        self.core.pending_errors.clear();
        self.core.storage_health = "ok";
        self.core.storage_error = None;
        (true, true)
    }

    fn refresh_error_history(&mut self, reference: chrono::DateTime<Utc>) {
        match self.checkpoints.error_preview(reference) {
            Ok(events) => {
                self.error_history = events;
                self.error_history_read_error = None;
            }
            Err(error) => self.error_history_read_error = Some(storage_error_type(&error)),
        }
    }

    fn refresh(&mut self, root: &Path, stopped: &AtomicBool) -> (&'static str, bool, bool) {
        self.checkpoints.begin_round();
        let has_errors = |state: &Session| {
            !state.projection.pending_errors.is_empty() || state.unsaved_tool_errors()
        };
        let saved = if self.files.values().any(has_errors) {
            let mut changed = self.checkpoints.save(root, &mut self.files);
            changed |= self.checkpoints.save_unanchored_errors(&mut self.files);
            if self.files.values().any(has_errors) {
                return ("partly_unavailable", false, changed);
            }
            changed
        } else {
            false
        };
        match fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() && !redirected(&meta) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let changed = !self.files.is_empty();
                self.files.clear();
                return ("missing", false, changed);
            }
            _ => {
                let changed = !self.files.is_empty();
                self.files.clear();
                return ("unavailable", false, changed);
            }
        }
        let mut paths = Vec::new();
        let mut incomplete = false;
        let mut entries = WalkDir::new(root)
            .follow_root_links(false)
            .max_depth(8)
            .into_iter();
        for entry in entries.by_ref().take(WALK_LIMIT) {
            if stopped.load(Ordering::Acquire) {
                return ("waiting", false, false);
            }
            let Ok(entry) = entry else {
                incomplete = true;
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_str().unwrap_or_default();
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            match entry.metadata() {
                Ok(meta) if !redirected(&meta) => {
                    paths.push((meta.modified().ok(), entry.into_path()))
                }
                _ => incomplete = true,
            }
        }
        incomplete |= entries.next().is_some() || paths.len() > FILE_LIMIT;
        paths.sort_unstable_by(|left, right| right.cmp(left));
        paths.truncate(FILE_LIMIT);
        let previous_files = self.files.len();
        let mut files = BTreeMap::new();
        let ordered: Vec<_> = paths.into_iter().map(|(_, path)| path).collect();
        for path in &ordered {
            let state = self
                .files
                .remove(path)
                .unwrap_or_else(|| Session::new(path));
            files.insert(path.clone(), state);
        }
        let mut changed = saved || !self.files.is_empty() || files.len() != previous_files;
        self.files = files;
        let mut budget = ROUND_LIMIT;
        let length = ordered.len();
        let start = self.cursor % length.max(1);
        let mut pending = false;
        let mut retained: usize = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        for step in 0..length {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            let index = (start + step) % length;
            if budget == 0 {
                pending = true;
                break;
            }
            self.cursor = index + 1;
            let path = &ordered[index];
            let state = self.files.get_mut(path).expect("discovered path");
            let before = state.projection.calls.len();
            let call_limit = CALL_LIMIT
                .saturating_sub(retained - before)
                .min(FILE_CALL_LIMIT) as u32;
            match state.read(
                root,
                path,
                budget.min(CHUNK_LIMIT),
                call_limit,
                &mut self.checkpoints,
            ) {
                Ok((read, malformed, modified)) => {
                    self.read_bytes += read as u64;
                    self.malformed += malformed;
                    budget -= read;
                    pending |= read > 0 && state.offset < state.size;
                    incomplete |= read == 0 && state.offset < state.size;
                    changed |= modified;
                }
                Err(_) => incomplete = true,
            }
            retained = retained - before + state.projection.calls.len();
        }
        self.bound_state(&ordered);
        self.bound_errors();
        changed |= self.checkpoints.save(root, &mut self.files);
        changed |= self.checkpoints.save_unanchored_errors(&mut self.files);
        (
            if incomplete {
                "partly_unavailable"
            } else {
                "ok"
            },
            pending,
            changed,
        )
    }

    fn bound_state(&mut self, ordered: &[PathBuf]) {
        let mut buffered = 0;
        for path in ordered {
            let state = self.files.get_mut(path).expect("discovered path");
            buffered += state.partial.len();
            if buffered > BUFFER_LIMIT {
                buffered -= state.partial.len();
                state.partial = Vec::new();
                state.discard = true;
                state.checkpoint_dirty = true;
            }
        }
        let retained: usize = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        if retained > CALL_LIMIT {
            let mut calls = self
                .files
                .iter()
                .flat_map(|(path, state)| {
                    state
                        .projection
                        .calls
                        .iter()
                        .map(move |(id, call)| (call.time, path, id))
                })
                .collect::<Vec<_>>();
            let overflow = retained - CALL_LIMIT;
            calls.select_nth_unstable(overflow);
            let retired: Vec<_> = calls
                .into_iter()
                .take(overflow)
                .map(|(_, path, id)| (path.clone(), id.clone()))
                .collect();
            for (path, id) in retired {
                self.files
                    .get_mut(&path)
                    .expect("retained path")
                    .projection
                    .retire_call(&id);
            }
            for state in self.files.values_mut() {
                state.projection.prune_order();
            }
        }
        let mut retained = 0;
        for path in ordered {
            let state = self.files.get_mut(path).expect("discovered path");
            let count = state.projection.retired_activity_count;
            if retained + count > CALL_LIMIT {
                state.projection.observation_overflow = true;
                state.projection.retired_observations.clear();
                state.projection.retired_activity_count = 0;
            } else {
                retained += count;
            }
        }
    }

    fn bound_errors(&mut self) {
        let retained: usize = self
            .files
            .values()
            .map(|state| state.projection.errors.len())
            .sum();
        if retained <= ERROR_LIMIT {
            return;
        }
        let mut errors = self
            .files
            .iter()
            .flat_map(|(path, state)| {
                state
                    .projection
                    .errors
                    .iter()
                    .enumerate()
                    .map(move |(index, error)| (std::cmp::Reverse(error.time), path, index))
            })
            .collect::<Vec<_>>();
        errors.select_nth_unstable(ERROR_LIMIT);
        let mut removed = BTreeMap::<PathBuf, BTreeSet<usize>>::new();
        for (_, path, index) in errors.into_iter().skip(ERROR_LIMIT) {
            removed.entry(path.clone()).or_default().insert(index);
        }
        for (path, indexes) in removed {
            let mut index = 0;
            let state = self.files.get_mut(&path).expect("retained path");
            state.checkpoint_dirty = true;
            state.projection.errors.retain(|_| {
                let keep = !indexes.contains(&index);
                index += 1;
                keep
            });
        }
    }

    fn error_windows(&self, health: &str, reference: chrono::DateTime<Utc>) -> [(Value, Bytes); 4] {
        let journal = self
            .journal
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor_entries = journal.entries();
        let monitor_source = journal.source();
        drop(journal);
        let session: Vec<_> = self
            .files
            .iter()
            .filter(|(_, state)| state.projection.id.is_some())
            .flat_map(|(path, state)| state.error_events(path, &state.projection.errors))
            .collect();
        let tool_errors: Vec<_> = self
            .files
            .values()
            .flat_map(|state| {
                state
                    .projection
                    .id
                    .as_deref()
                    .into_iter()
                    .flat_map(|thread| {
                        state.projection.calls.iter().filter_map(move |(id, call)| {
                            call.failure_event(thread, id)
                                .map(|event| (call.time, event))
                        })
                    })
            })
            .collect();
        let session_source = json!({"source":"session","health":health,"files":[],"file_count":self.files.len(),
                           "checked_at":self.updated.map(stamp),"read_bytes":self.read_bytes,
                           "unsupported_lines":self.malformed});
        let core_source = self.core.source();
        let desktop_source = if health == "disabled" {
            json!({"source":"codex_desktop","health":"disabled"})
        } else {
            self.desktop.source.clone()
        };
        let error_type = self
            .error_history_read_error
            .or(self.checkpoints.error_write_error)
            .or_else(|| self.desktop.source["storage_error"].as_str())
            .or_else(|| {
                self.core
                    .storage_error
                    .filter(|_| !self.core.pending_errors.is_empty())
            });
        let history_health = if health == "disabled" {
            "disabled"
        } else if error_type.is_some() {
            "unavailable"
        } else if self.updated.is_some() {
            "ok"
        } else {
            "waiting"
        };
        let history = json!({"health":history_health,"error_type":error_type,
                            "retained":self.error_history.len(),"limit":ERROR_PREVIEW_LIMIT,
                            "hours":ERROR_PREVIEW_HOURS});
        std::array::from_fn(|index| {
            let boundary = HOURS[index].map(|hours| reference - Duration::hours(hours));
            let mut errors = visible_logs(
                session
                    .iter()
                    .filter(|event| event["record_hash"].is_string())
                    .chain(
                        tool_errors
                            .iter()
                            .filter(|(start, _)| {
                                boundary
                                    .is_none_or(|cutoff| start.is_some_and(|time| time >= cutoff))
                            })
                            .map(|(_, event)| event),
                    )
                    .chain(&self.core.errors)
                    .chain(&self.desktop.errors)
                    .chain(&self.error_history)
                    .chain(
                        session
                            .iter()
                            .filter(|event| event["record_hash"].is_null()),
                    ),
                boundary,
            );
            let mut logs = if self.logs_enabled {
                visible_logs(
                    self.core
                        .logs
                        .iter()
                        .chain(&self.desktop.logs)
                        .chain(&monitor_entries)
                        .chain(errors.iter().copied()),
                    boundary,
                )
            } else {
                Vec::new()
            };
            let mut severities = BTreeMap::<&str, usize>::new();
            let mut categories = BTreeMap::<&str, usize>::new();
            for error in &errors {
                if let Some(severity) = error["severity"].as_str() {
                    *severities.entry(severity).or_default() += 1;
                }
                if let Some(category) = error["category"].as_str() {
                    *categories.entry(category).or_default() += 1;
                }
            }
            let total = errors.len();
            let log_total = logs.len();
            let enabled = health != "disabled";
            errors.truncate(ERROR_LIMIT);
            logs.truncate(2000);
            let mut log_core = core_source.clone();
            let mut log_desktop = desktop_source.clone();
            if !self.logs_enabled {
                log_core["health"] = json!("disabled");
                log_desktop["health"] = json!("disabled");
            }
            let logs = json!({"enabled":self.logs_enabled,"total":log_total,"entries":logs,
                              "limit":2000,"trimmed":self.core.trimmed,"sources":[log_core,log_desktop,session_source,monitor_source]})
                .to_string()
                .into();
            let summary = json!({"enabled":enabled,"total":total,"events":errors,
                                 "categories":categories,"severities":severities,"limit":ERROR_LIMIT,
                                 "history":history,
                                 "diagnostics":{"health":{"core":self.core.health(),"desktop":desktop_source["health"]}}});
            (summary, logs)
        })
    }

    fn snapshots(&self, health: &str, watch_health: &str, version: u64) -> Snapshots {
        let debug = self
            .debug
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot();
        let mut threads = BTreeMap::<&str, (Projection, bool)>::new();
        for state in self.files.values() {
            if let Some(id) = state.projection.id.as_deref() {
                let (projection, partial) = threads
                    .entry(id)
                    .or_insert_with(|| (Projection::for_id(Some(id.to_owned())), false));
                projection.merge(&state.projection);
                *partial |= state.pending() || state.discard;
            }
        }
        let reference = self.updated.unwrap_or(self.started);
        let command_calls = commands::Calls::new(&threads);
        let recent = threads.iter().flat_map(|(thread, (row, _))| {
            row.calls
                .iter()
                .flat_map(move |(id, call)| call.activities(thread, id))
        });
        let observed_activity = lam_history_store::combine_observations(
            &self.activity_history,
            recent.chain(self.core.sql.iter().cloned()),
        );
        let (mcp_events, sql_events): (Vec<_>, Vec<_>) = observed_activity
            .into_iter()
            .partition(|event| event.get("server").is_some());
        let sql_events = sqlite::identified(sql_events);
        let mut api_events = self.core.api_events.clone();
        api_events.extend(self.desktop.api_events.iter().cloned());
        api_events
            .make_contiguous()
            .sort_by(|left, right| right["timestamp"].as_str().cmp(&left["timestamp"].as_str()));
        api_events.truncate(1000);
        let all_mcp = mcp::summary(&mcp_events, None, self.activity_history_health);
        let errors = self.error_windows(health, reference);
        let activity = json!({
            "paused":false, "health":health, "last_activity_at":threads.values().filter_map(|(row,_)| row.updated()).max().map(stamp),
            "update_mode":"source_events","watch_health":watch_health,"code_revision":self.backend_revision
        });
        let windows = std::array::from_fn(|index| {
            let boundary = HOURS[index].map(|hours| reference - Duration::hours(hours));
            let mut totals = BTreeMap::new();
            let mut nested_totals = BTreeMap::new();
            let mut rows = threads
                .values()
                .map(|(row, partial)| {
                    let mut value = row.row(
                        *partial,
                        boundary,
                        reference,
                        &mut totals,
                        &mut nested_totals,
                    );
                    apply_thread_metadata(
                        &mut value,
                        row.id
                            .as_deref()
                            .and_then(|id| self.thread_metadata.get(id)),
                        false,
                    );
                    value
                })
                .collect::<Vec<Value>>();
            for (id, metadata) in &self.thread_metadata {
                if threads.contains_key(id.as_str()) {
                    continue;
                }
                let projection = Projection::for_id(Some(id.clone()));
                let mut value =
                    projection.row(true, boundary, reference, &mut totals, &mut nested_totals);
                apply_thread_metadata(&mut value, Some(metadata), true);
                rows.push(value);
            }
            rows.sort_by(|left, right| {
                right["updated_at"]
                    .as_str()
                    .cmp(&left["updated_at"].as_str())
            });
            let mut projects: BTreeMap<String, Value> = self.project_details.iter().map(|(id,detail)| {
                let mut value = json!({"id":id,"name":detail["name"],"icon":detail["icon"],"kind":detail["kind"],"thread_count":0});
                for key in ["origin","created_at","updated_at"] { if let Some(field)=detail.get(key) { value[key]=field.clone(); } }
                (id.clone(),value)
            }).collect();
            for row in &rows {
                if let Some(id) = row["project_id"].as_str().filter(|id| !id.is_empty()) {
                    let project=projects.entry(id.to_owned()).or_insert_with(||json!({"id":id,"name":row["project_name"],"icon":null,"kind":row["project_kind"],"thread_count":0}));
                    project["thread_count"] =
                        json!(project["thread_count"].as_u64().unwrap_or(0) + 1);
                }
            }
            let observed = totals.values().sum::<usize>();
            let (git, checks) = command_calls.window(boundary);
            let pending = self.files.values().filter(|state| state.pending()).count();
            let mut mcp = boundary.map_or_else(
                || all_mcp.clone(),
                |cutoff| {
                    mcp::scoped_summary(&mcp_events, cutoff, self.activity_history_health, &all_mcp)
                },
            );
            let mut sqlite = sqlite::window(&sql_events, boundary, self.activity_history_health);
            if let Some(view) = &self.activity_view {
                mcp["history_aggregate"] = history_view::render(&view.mcp[index], WINDOWS[index]);
                sqlite["aggregate"] = history_view::render(&view.sql[index], WINDOWS[index]);
            }
            let mut model_api = model_api::summary(
                &api_events,
                boundary,
                health != "disabled",
                self.core.source(),
            );
            if health != "disabled" {
                if let Some(sources) = model_api["sources"].as_array_mut() {
                    sources.push(self.desktop.source.clone());
                }
            }
            let settings = json!({"debug_mode":debug["enabled"],"observations":{"metadata":self.metadata_enabled,"logs":self.logs_enabled}});
            let mut snapshot = json!({
                "version":1,"label":"本機觀察統計","started_at":stamp(self.started),"updated_at":self.updated.map(stamp),
                "activity":activity,
                "backend_revision":self.backend_revision,
                "frontend_revision":self.frontend_revision,
                "monitor":{"version":env!("CARGO_PKG_VERSION")},
                "model_api":model_api,
                "errors":errors[index].0,
                "mcp":mcp,
                "codex":{
                    "source":"codex","health":health,"started_at":stamp(self.started),"files":self.files.len(),
                    "bytes_read":self.read_bytes,"malformed_lines":self.malformed,
                    "threads":rows,"tools":totals,"nested_tools":nested_totals,"observed_tool_calls":observed,
                    "git":git,"checks":checks,
                    "schedules":self.schedules,
                    "metadata_sources":self.schedule_report.iter().collect::<Vec<_>>(),
                    "sqlite":sqlite,
                    "activity_scope":{"window":WINDOWS[index],"timestamp":"call_started_at","unknown_timestamp":"all_only",
                                      "latest_state":["tokens","model","status"]},
                    "read_state":{"file_limit":FILE_LIMIT,"file_count":self.files.len(),"pending_files":pending,
                                  "read_mode":"incremental_all_sources","read_limit":ROUND_LIMIT,"tail_bytes":CHUNK_LIMIT,
                                  "call_limit":CALL_LIMIT,"buffer_limit":BUFFER_LIMIT,"file_call_limit":FILE_CALL_LIMIT,
                                  "git_event_limit":commands::GIT_EVENT_LIMIT,"check_event_limit":commands::CHECK_EVENT_LIMIT,
                                  "checkpoint":self.checkpoints.report}
                }
            });
            snapshot["settings"] = settings;
            snapshot["codex"]["dots"] = self.dots[index].clone();
            snapshot["codex"]["projects"] = json!(projects.into_values().collect::<Vec<_>>());
            snapshot["monitor"]["debug"] = debug.clone();
            snapshot.to_string().into()
        });
        Snapshots {
            windows,
            logs: errors.map(|(_, logs)| logs),
            activity: activity.to_string().into(),
            metadata_enabled: self.metadata_enabled,
            logs_enabled: self.logs_enabled,
            debug_enabled: debug["enabled"].as_bool().unwrap_or(false),
            version,
            events: Default::default(),
        }
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .event("stopped");
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn catalog_metadata_preserves_session_model_and_lifecycle() {
        let mut row = json!({"thread_id":"fixture","model":"session-model","status":"running","status_updated_at":"2026-01-01T00:00:00Z","trigger":"user","environment":"local","tokens":{"total_tokens":0},"tool_calls":1});
        let metadata = json!({"name":"title","model":"catalog-model","status":"idle","last_run_at":"2026-01-02T00:00:00Z","kind":"schedule","environment":"remote"});
        apply_thread_metadata(&mut row, Some(&metadata), false);
        assert_eq!(row["model"], "session-model");
        assert_eq!(row["status"], "running");
        assert_eq!(row["trigger"], "schedule");
        assert_eq!(row["tokens"]["total_tokens"], 0);
        assert_eq!(row["tool_calls"], 1);
    }

    #[test]
    fn equivalent_millisecond_identity_keeps_last_record_before_filtering() {
        let records = [
            json!({"timestamp":"2026-01-01T00:00:00.123100Z","source":"monitor","code":"http_response_error","http_status":404}),
            json!({"timestamp":"2026-01-01T08:00:00.123900+08:00","source":"monitor","code":"http_response_error","http_status":400}),
        ];
        let logs = visible_logs(records.iter(), None);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0]["http_status"], 400);
        let cutoff = lam_history_store::parse_utc_stamp("2026-01-01T00:00:00.123500Z").unwrap();
        assert_eq!(
            visible_logs(records.iter(), Some(cutoff))[0]["http_status"],
            400
        );
        assert!(visible_logs(records.iter().rev(), Some(cutoff)).is_empty());
    }
}
