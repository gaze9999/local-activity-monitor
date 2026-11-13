pub(crate) mod account;
mod agent;
pub(crate) mod allowance;
mod checkpoint;
pub(crate) mod cloud;
mod commands;
mod connection;
pub(crate) mod core_logs;
mod csv_dialect;
pub(crate) mod desktop;
pub(crate) mod details;
pub(crate) mod file_metadata;
mod history_view;
pub(crate) mod instructions;
pub(crate) mod jev;
mod mcp;
pub(crate) mod mcp_documents;
mod metadata;
mod model_api;
pub(crate) mod platform;
mod plugins;
mod project;
pub(crate) mod projects;
pub(crate) mod public_api;
pub(crate) mod schedules;
pub(crate) mod skill_detail;
mod source_introduction;
mod source_registry;
mod sqlite;
mod structured;
mod worktree_watch;
mod worktrees;

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
use std::sync::{Arc, Mutex, RwLock};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSwitches {
    pub git: bool,
    pub checks: bool,
    pub jev_calls: bool,
    pub skills: bool,
    pub jev: bool,
    pub worktrees: bool,
    pub usage: bool,
}
impl Default for CommandSwitches {
    fn default() -> Self {
        Self {
            git: true,
            checks: true,
            jev_calls: true,
            skills: true,
            jev: true,
            worktrees: true,
            usage: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpSwitches {
    pub mcp: bool,
    pub web: bool,
}
impl Default for McpSwitches {
    fn default() -> Self {
        Self {
            mcp: true,
            web: true,
        }
    }
}
impl McpSwitches {
    pub fn allows(&self, server: &str) -> bool {
        if server == "web" {
            self.web
        } else {
            self.mcp
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackingSettings {
    pub max_files: u32,
    pub track_all: bool,
}
impl Default for TrackingSettings {
    fn default() -> Self {
        Self {
            max_files: 20,
            track_all: false,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct McpCustomizations {
    pub mcp_categories: BTreeMap<String, String>,
    pub mcp_descriptions: BTreeMap<String, String>,
    pub mcp_tags: BTreeMap<String, Vec<String>>,
    pub tool_descriptions: BTreeMap<String, String>,
}

pub struct Snapshots {
    pub windows: [Bytes; 4],
    pub logs: [Bytes; 4],
    pub activity: Bytes,
    pub error_details: BTreeMap<String, Value>,
    pub codex_enabled: bool,
    pub metadata_enabled: bool,
    pub tool_events_enabled: bool,
    pub files_enabled: bool,
    pub errors_enabled: bool,
    pub sqlite_enabled: bool,
    pub model_api_enabled: bool,
    pub command_switches: CommandSwitches,
    pub account_settings: account::Settings,
    pub cloud_settings: account::Settings,
    pub public_settings: [account::Settings; 2],
    pub platform_settings: [account::Settings; 4],
    pub worktree_details: BTreeMap<String, Value>,
    pub mcp_sources: BTreeMap<String, bool>,
    pub mcp_switches: McpSwitches,
    pub customizations: McpCustomizations,
    pub retention_days: Option<i64>,
    pub tracking: TrackingSettings,
    pub logs_enabled: bool,
    pub debug_enabled: bool,
    version: u64,
    started_instant: std::time::Instant,
    traffic: Arc<Mutex<crate::server::Traffic>>,
    memory: Arc<Mutex<crate::runtime_info::Memory>>,
    collection: Arc<Mutex<crate::performance_debug::CollectionStatus>>,
    process_usage: Arc<Mutex<crate::process_usage::Sampler>>,
    rendered: Mutex<[Option<(u64, u64, u64, Bytes, Bytes)>; 4]>,
}

impl Snapshots {
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn window(query: &str) -> Option<usize> {
        Self::window_or(query, 1)
    }

    pub fn snapshot_window(query: &str) -> Option<usize> {
        if query.len() > 128 {
            return None;
        }
        let mut selected = None;
        for (name, value) in form_urlencoded::parse(query.as_bytes()).filter(|(_, value)| !value.is_empty()) {
            if name != "window" {
                return None;
            }
            if selected.is_none() {
                selected = Some(WINDOWS.iter().position(|item| *item == value)?);
            }
        }
        Some(selected.unwrap_or(1))
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

    fn render(&self, window: usize) -> (Bytes, Bytes) {
        let seconds = self.started_instant.elapsed().as_secs_f64().round() as u64;
        let traffic = *self
            .traffic
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (collection_revision, collection) = {
            let state = self
                .collection
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (state.revision(), state.snapshot())
        };
        let mut cache = self
            .rendered
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((cached, requests, revision, snapshot, event)) = &cache[window] {
            if *cached == seconds
                && *requests == traffic.revision
                && *revision == collection_revision
            {
                return (snapshot.clone(), event.clone());
            }
        }
        let original = &self.windows[window];
        let (memory, cpu_checked_at) = {
            let mut memory = self
                .memory
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let value = memory.snapshot();
            (value, memory.last_sample().map(stamp))
        };
        let process_usage = self
            .process_usage
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sample();
        let snapshot = serde_json::from_slice::<Value>(original)
            .ok()
            .and_then(|mut snapshot| {
                let monitor = snapshot.get_mut("monitor")?.as_object_mut()?;
                monitor.insert("uptime_seconds".into(), json!(seconds));
                monitor.insert("requests".into(), json!(traffic.requests));
                monitor.insert("http_errors".into(), json!(traffic.http_errors));
                monitor.insert("snapshot_bytes".into(), json!(traffic.snapshot_bytes));
                monitor.insert("transfer_bytes".into(), json!(traffic.transfer_bytes));
                monitor.insert("process_usage".into(), process_usage);
                monitor.insert("collection".into(), collection);
                if let Some(values) = memory.as_object() {
                    monitor.extend(values.clone());
                }
                if let Some(current) = monitor.get("device_checked_at").and_then(Value::as_str) {
                    if let Some(sample) =
                        cpu_checked_at.as_deref().filter(|sample| *sample > current)
                    {
                        monitor.insert("device_checked_at".into(), json!(sample));
                    }
                }
                serde_json::to_vec(&snapshot).ok().map(Bytes::from)
            })
            .unwrap_or_else(|| original.clone());
        let version = self.version;
        let name = WINDOWS[window];
        let mut frame = format!("id: {version}\nevent: snapshot\ndata: {{\"version\":{version},\"window\":\"{name}\",\"snapshot\":").into_bytes();
        frame.extend_from_slice(&snapshot);
        frame.extend_from_slice(b",\"logs\":");
        frame.extend_from_slice(&self.logs[window]);
        frame.extend_from_slice(b"}\n\n");
        let event = Bytes::from(frame);
        cache[window] = Some((
            seconds,
            traffic.revision,
            collection_revision,
            snapshot.clone(),
            event.clone(),
        ));
        (snapshot, event)
    }
    pub fn window_bytes(&self, window: usize) -> Bytes {
        self.render(window).0
    }
    pub fn event(&self, window: usize) -> Bytes {
        self.render(window).1
    }
}

pub struct Monitor {
    worker: Option<Worker>,
    device_completion: Arc<Mutex<crate::runtime_info::DeviceCompletion>>,
    fonts: Arc<RwLock<Option<Vec<String>>>>,
    gpus: Arc<RwLock<Option<Vec<Value>>>>,
    traffic: Arc<Mutex<crate::server::Traffic>>,
    codex_enabled: Arc<AtomicBool>,
    metadata_enabled: Arc<AtomicBool>,
    tool_events_enabled: Arc<AtomicBool>,
    files_enabled: Arc<AtomicBool>,
    errors_enabled: Arc<AtomicBool>,
    sqlite_enabled: Arc<AtomicBool>,
    model_api_enabled: Arc<AtomicBool>,
    account: Arc<account::Shared>,
    cloud: Arc<account::Shared>,
    public: [Arc<account::Shared>; 2],
    platform: [Arc<account::Shared>; 4],
    command_switches: Arc<RwLock<CommandSwitches>>,
    mcp_sources: Arc<RwLock<BTreeMap<String, bool>>>,
    mcp_switches: Arc<RwLock<McpSwitches>>,
    customizations: Arc<RwLock<McpCustomizations>>,
    tracking: Arc<RwLock<TrackingSettings>>,
    journal: Arc<Mutex<crate::journal::Journal>>,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
    details: Arc<RwLock<details::Index>>,
    core_source: Arc<RwLock<Option<core_logs::Source>>>,
    desktop_sources: Arc<RwLock<Vec<desktop::ObservedSource>>>,
}

struct Worker {
    manual: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    wake: SyncSender<()>,
    handle: JoinHandle<()>,
}

impl Monitor {
    pub fn device_completion(&self) -> Arc<Mutex<crate::runtime_info::DeviceCompletion>> {
        Arc::clone(&self.device_completion)
    }
    pub fn gpus(&self) -> Arc<RwLock<Option<Vec<Value>>>> {
        Arc::clone(&self.gpus)
    }
    pub fn fonts(&self) -> Arc<RwLock<Option<Vec<String>>>> {
        Arc::clone(&self.fonts)
    }
    pub fn detail_index(&self) -> Arc<RwLock<details::Index>> {
        Arc::clone(&self.details)
    }
    pub fn desktop_sources(&self) -> Arc<RwLock<Vec<desktop::ObservedSource>>> {
        Arc::clone(&self.desktop_sources)
    }
    pub fn core_source(&self) -> Arc<RwLock<Option<core_logs::Source>>> {
        Arc::clone(&self.core_source)
    }
    pub fn debug(&self) -> Arc<Mutex<crate::performance_debug::PerformanceDebug>> {
        Arc::clone(&self.debug)
    }
    pub fn journal(&self) -> Arc<Mutex<crate::journal::Journal>> {
        Arc::clone(&self.journal)
    }
    pub fn traffic(&self) -> Arc<Mutex<crate::server::Traffic>> {
        Arc::clone(&self.traffic)
    }
    pub fn codex_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.codex_enabled)
    }
    pub fn metadata_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.metadata_enabled)
    }
    pub fn tool_events_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.tool_events_enabled)
    }
    pub fn files_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.files_enabled)
    }
    pub fn errors_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.errors_enabled)
    }
    pub fn sqlite_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.sqlite_enabled)
    }
    pub fn model_api_setting(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.model_api_enabled)
    }
    pub fn account_shared(&self) -> Arc<account::Shared> {
        Arc::clone(&self.account)
    }
    pub fn cloud_shared(&self) -> Arc<account::Shared> {
        Arc::clone(&self.cloud)
    }
    pub fn public_shared(&self) -> [Arc<account::Shared>; 2] {
        self.public.clone()
    }
    pub fn platform_shared(&self) -> [Arc<account::Shared>; 4] {
        self.platform.clone()
    }
    pub fn command_switches_setting(&self) -> Arc<RwLock<CommandSwitches>> {
        Arc::clone(&self.command_switches)
    }
    pub fn mcp_sources_setting(&self) -> Arc<RwLock<BTreeMap<String, bool>>> {
        Arc::clone(&self.mcp_sources)
    }
    pub fn mcp_switches_setting(&self) -> Arc<RwLock<McpSwitches>> {
        Arc::clone(&self.mcp_switches)
    }
    pub fn customizations_setting(&self) -> Arc<RwLock<McpCustomizations>> {
        Arc::clone(&self.customizations)
    }
    pub fn tracking_setting(&self) -> Arc<RwLock<TrackingSettings>> {
        Arc::clone(&self.tracking)
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
        max_files: u32,
    ) -> io::Result<(Self, watch::Receiver<Snapshots>)> {
        let root = home.join("sessions");
        let mut collector = Collector::new(home.join("monitoring"), enabled, frontend_revision);
        collector
            .tracking
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .max_files = max_files;
        let traffic = Arc::clone(&collector.traffic);
        let fonts = Arc::clone(&collector.fonts);
        let device_completion = Arc::clone(&collector.device_completion);
        let gpus = Arc::clone(&collector.gpus);
        let tool_events_enabled = Arc::clone(&collector.tool_events_enabled);
        let files_enabled = Arc::clone(&collector.files_enabled);
        let errors_enabled = Arc::clone(&collector.errors_enabled);
        let sqlite_enabled = Arc::clone(&collector.sqlite_enabled);
        let model_api_enabled = Arc::clone(&collector.model_api_enabled);
        let account = Arc::clone(&collector.account);
        let cloud = Arc::clone(&collector.cloud);
        let public = collector.public.clone();
        let platform = collector.platform.clone();
        let command_switches = Arc::clone(&collector.command_switches);
        let mcp_sources = Arc::clone(&collector.mcp_sources);
        let mcp_switches = Arc::clone(&collector.mcp_switches);
        let customizations = Arc::clone(&collector.customizations);
        let tracking = Arc::clone(&collector.tracking);
        let journal = Arc::clone(&collector.journal);
        let debug = Arc::clone(&collector.debug);
        let details = Arc::new(RwLock::new(details::Index::default()));
        let detail_writer = Arc::clone(&details);
        let core_source = Arc::new(RwLock::new(None));
        let core_source_writer = Arc::clone(&core_source);
        let desktop_sources = Arc::new(RwLock::new(Vec::new()));
        let desktop_source_writer = Arc::clone(&desktop_sources);
        let initial = if enabled {
            ("waiting", "starting")
        } else {
            ("disabled", "disabled")
        };
        if enabled {
            let include_workdirs = collector
                .command_switches
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .worktrees;
            let (mut metadata, mut reports) =
                schedules::background_metadata(home, &BTreeSet::new(), include_workdirs, true);
            let (projects, roots_status) =
                projects::read(home, &mut metadata, include_workdirs, &mut reports);
            let reference = now();
            let dots = crate::dots::background(home, &metadata, reference);
            schedules::merge_remote_state(home, &mut metadata);
            reports.push(collector.related_lifecycle(&root, &mut metadata));
            let (schedules, schedule_report) = schedules::read(home, reference, &metadata);
            let (plugins, plugin_report, _) = plugins::scan(home);
            collector.thread_metadata = metadata;
            collector.metadata_reports = reports;
            collector.project_details = projects;
            collector.project_roots_status = roots_status;
            collector.dots = dots;
            collector.schedules = schedules;
            collector.schedule_report = Some(schedule_report);
            collector.plugins = plugins;
            collector.plugin_report = Some(plugin_report);
        }
        let (publish, snapshots) = watch::channel(collector.snapshots(initial.0, initial.1, 0));
        let (wake, signal) = mpsc::sync_channel(1);
        account.set_wake(wake.clone());
        cloud.set_wake(wake.clone());
        for source in &platform {
            source.set_wake(wake.clone());
        }
        for source in &public {
            source.set_wake(wake.clone());
        }
        let manual_refresh = Arc::new(AtomicBool::new(false));
        let manual_signal = Arc::clone(&manual_refresh);
        let codex_enabled = Arc::new(AtomicBool::new(enabled));
        let codex_control = Arc::clone(&codex_enabled);
        let metadata_enabled = Arc::new(AtomicBool::new(true));
        let metadata_control = Arc::clone(&metadata_enabled);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let home = home.to_path_buf();
        let events = wake.clone();
        let desktop_wake = wake.clone();
        let schedule_wake = wake.clone();
        let handle = thread::Builder::new()
            .name("session-collector".into())
            .spawn(move || {
                let jev_dirty = Arc::new(AtomicBool::new(true));
                let mut jev_watcher = jev::SourceWatcher::new(&home, events.clone(), Arc::clone(&jev_dirty)).ok();
                let mut version = 0;
                let mut was_active = false;
                while !stop.load(Ordering::Acquire) {
                if !codex_control.load(Ordering::Acquire) {
                    // Release a retained read transaction when collection is disabled.
                    *collector.selected_detail_scan.lock().unwrap_or_else(|error| error.into_inner()) = None;
                    let catalog_watcher = start_catalog_watch(&home, events.clone()).ok();
                    let _ = events.try_send(());
                    while !stop.load(Ordering::Acquire) && !codex_control.load(Ordering::Acquire) {
                        let (mut future_changed, next_future) = collector.checkpoints.pending_activity(now()).unwrap_or((false, Some(now() + Duration::seconds(5))));
                        let delay = next_future.map(|due| (due - now()).to_std().unwrap_or_default().max(std::time::Duration::from_millis(50)).min(std::time::Duration::from_secs(5))).unwrap_or(std::time::Duration::from_secs(5));
                        match signal.recv_timeout(if future_changed {std::time::Duration::ZERO} else {delay}) {
                            Ok(()) => {},
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                future_changed |= collector.checkpoints.pending_activity(now()).is_ok_and(|(changed,_)| changed);
                                if !future_changed && !jev_dirty.load(Ordering::Acquire) && mcp::discover(&home) == collector.mcp_catalog { continue; }
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                        if stop.load(Ordering::Acquire) || codex_control.load(Ordering::Acquire) { break; }
                        jev_dirty.store(false, Ordering::Release);
                        let requested = manual_refresh.swap(false, Ordering::AcqRel);
                        if requested && jev_watcher.as_ref().is_none_or(|watcher| !watcher.healthy()) { jev_watcher = jev::SourceWatcher::new(&home, events.clone(), Arc::clone(&jev_dirty)).ok(); }
                        let jev_enabled = collector.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev;
                        if let Some(watcher) = jev_watcher.as_mut() { watcher.refresh(&home, jev_enabled); }
                        collector.jev_watch_health = Some(jev_watcher.as_ref().is_some_and(|watcher| watcher.healthy()));
                        let began = std::time::Instant::now();
                        let cpu_started = crate::performance_debug::process_cpu_ms();
                        let before_bytes = collector.read_bytes.saturating_add(collector.desktop.bytes_read());
                        collector.metadata_enabled = metadata_control.load(Ordering::Acquire);
                        collector.applied_errors_enabled = collector.errors_enabled.load(Ordering::Acquire);
                        collector.applied_sqlite_enabled = collector.sqlite_enabled.load(Ordering::Acquire);
                        collector.applied_model_api_enabled = collector.model_api_enabled.load(Ordering::Acquire);
                        collector.applied_command_switches = *collector.command_switches.read().unwrap_or_else(|error| error.into_inner());
                        collector.logs_enabled = collector
                            .journal
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .enabled();
                        let reference = now();
                        collector.set_phase("history");
                        let history_timer =
                            crate::performance_debug::PhaseTimer::start(true);
                        collector.refresh_activity(reference);
                        collector.refresh_jev_errors(reference);
                        collector.refresh_error_history(reference);
                        collector.refresh_skill_history();
                        collector.mcp_catalog = mcp::discover(&home);
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
                        metrics["read_bytes"] = json!(collector.read_bytes.saturating_add(collector.desktop.bytes_read()).saturating_sub(before_bytes));
                        collector.record_monitor_sample(&mut metrics);
                        collector
                            .debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .record("collection", metrics);
                        version += 1;
                        let watch_health = if catalog_watcher.is_some() && collector.jev_watch_health == Some(true) { "ok" } else { "unavailable" };
                        collector.set_phase("projections");
                        let next = collector.snapshots("disabled", watch_health, version);
                        collector.set_phase("idle");
                        publish.send_replace(next);
                    }
                    continue;
                }
                let desktop_roots = {
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
        };
                collector.force_replay = was_active;
                let reactivated = was_active;
                was_active = true;
                let watch_failed = Arc::new(AtomicBool::new(false));
                let worktree_dirty = Arc::new(AtomicBool::new(true));
                let mut worktree_watcher = worktree_watch::SourceWatcher::new(events.clone(), Arc::clone(&worktree_dirty)).ok();
                let reenroll = Arc::new(AtomicBool::new(false));
                let schedule_dirty = Arc::new(AtomicBool::new(true));
                let plugin_dirty = Arc::new(AtomicBool::new(true));
                let mut plugin_watcher = plugins::Watcher::new(&home, events.clone(), Arc::clone(&plugin_dirty)).ok();
                let mut plugin_watch_ok = plugin_watcher.as_mut().is_some_and(|watcher| watcher.sync(&plugins::watch_roots(&home)).0);

                let mut watcher = start_watch(
                    &home,
                    &root,
                    events.clone(),
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
                let mut rescan_core_logs = reactivated;
                let mut rescan_desktop_logs = reactivated;
                let mut clear_core_for_errors = reactivated;
                let mut clear_desktop_for_errors = reactivated;
                let global_state = home.join(".codex-global-state.json");
                let mut global_identity = None;
                let mut previous = initial;
                let mut account_version = collector.account.version();
                let mut cloud_version = collector.cloud.version();
                let mut public_versions = collector.public.each_ref().map(|source| source.version());
                let mut platform_versions = collector.platform.each_ref().map(|source| source.version());
                while !stop.load(Ordering::Acquire) && codex_control.load(Ordering::Acquire) {
                    let jev_changed = jev_dirty.swap(false, Ordering::AcqRel);
                    let requested = manual_refresh.swap(false, Ordering::AcqRel);
                    let next_account_version = collector.account.version();
                    let account_changed = next_account_version != account_version;
                    account_version = next_account_version;
                    let next_cloud_version = collector.cloud.version();
                    let cloud_changed = next_cloud_version != cloud_version;
                    cloud_version = next_cloud_version;
                    let next_public_versions = collector.public.each_ref().map(|source| source.version());
                    let public_changed = next_public_versions != public_versions;
                    public_versions = next_public_versions;
                    let next_platform_versions = collector.platform.each_ref().map(|source|source.version());
                    let platform_changed = next_platform_versions != platform_versions;
                    platform_versions = next_platform_versions;
                    if requested && jev_watcher.as_ref().is_none_or(|watcher| !watcher.healthy()) { jev_watcher = jev::SourceWatcher::new(&home, events.clone(), Arc::clone(&jev_dirty)).ok(); }
                    let jev_enabled = collector.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev;
                    if let Some(watcher) = jev_watcher.as_mut() { watcher.refresh(&home, jev_enabled); }
                    collector.jev_watch_health = Some(jev_watcher.as_ref().is_some_and(|watcher| watcher.healthy()));
                    let worktree_signal = worktree_dirty.swap(false, Ordering::AcqRel);
                    let worktrees_enabled = collector.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees;
                    if requested && worktree_watcher.as_ref().is_none_or(|watcher| !watcher.healthy()) { worktree_watcher=worktree_watch::SourceWatcher::new(events.clone(), Arc::clone(&worktree_dirty)).ok(); }
                    if let Some(watcher) = worktree_watcher.as_mut() { watcher.refresh(&home, collector.worktrees.notification_roots(), worktrees_enabled); }
                    if requested { schedule_dirty.store(true, Ordering::Release); }
                    let collection_began = std::time::Instant::now();
                    let cpu_started = crate::performance_debug::process_cpu_ms();
                    let before_bytes = collector.read_bytes.saturating_add(collector.desktop.bytes_read());
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
                    let errors_enabled = collector.errors_enabled.load(Ordering::Acquire);
                    let errors_reenabled = errors_enabled && !collector.previous_errors_enabled;
                    let errors_changed = errors_enabled != collector.applied_errors_enabled;
                    let sqlite_enabled = collector.sqlite_enabled.load(Ordering::Acquire);
                    let sqlite_reenabled = sqlite_enabled && !collector.previous_sqlite_enabled;
                    let sqlite_changed = sqlite_enabled != collector.applied_sqlite_enabled;
                    let model_api_enabled = collector.model_api_enabled.load(Ordering::Acquire);
                    let model_api_reenabled = model_api_enabled && !collector.applied_model_api_enabled;
                    let model_api_changed = model_api_enabled != collector.applied_model_api_enabled;
                    let command_switches = *collector.command_switches.read().unwrap_or_else(|error| error.into_inner());
                    let commands_changed = command_switches != collector.applied_command_switches;
                    collector.core.capture_errors = errors_enabled;
                    collector.desktop.set_capture_errors(errors_enabled);
                    collector.core.capture_sql = sqlite_enabled;
                    collector.desktop.set_capture_sql(sqlite_enabled);
                    collector.core.capture_api = model_api_enabled;
                    collector.desktop.set_capture_api(model_api_enabled);
                    if errors_reenabled || sqlite_reenabled || model_api_reenabled {
                        rescan_core_logs = true; rescan_desktop_logs = true;
                        clear_core_for_errors = true; clear_desktop_for_errors = true;
                    }
                    collector.set_phase("sessions");
                    let session_timer = crate::performance_debug::PhaseTimer::start(true);
                    let (health, pending, changed) = collector.refresh(&root, &stop, errors_enabled, sqlite_enabled, command_switches);
                    if changed { collector.overflow_workdir_scan = None; }
                    let session_phase = session_timer.map(|timer| timer.finish());
                    if changed {
                        schedule_dirty.store(true, Ordering::Release);
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
                    collector.set_phase("diagnostics");
                    let core_timer = crate::performance_debug::PhaseTimer::start(true);
                    let diagnostic_changed = collector.refresh_core(source, now());
                    *core_source_writer.write().unwrap_or_else(|error| error.into_inner()) = collector.core.observed_source();
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
                    collector.set_phase("projections");
                    let metadata = metadata_control.load(Ordering::Acquire);
                    let metadata_changed = metadata != collector.metadata_enabled;
                    collector.metadata_enabled = metadata;
                    if !metadata { plugin_watcher = None; plugin_watch_ok = true; }
                    else if plugin_watcher.is_none() || (requested && plugin_watcher.as_ref().is_some_and(|watcher|!watcher.healthy())) {
                        plugin_watcher = plugins::Watcher::new(&home,events.clone(),Arc::clone(&plugin_dirty)).ok();
                        plugin_watch_ok = plugin_watcher.as_mut().is_some_and(|watcher|watcher.sync(&plugins::watch_roots(&home)).0);
                        plugin_dirty.store(true,Ordering::Release);
                    }
                    if metadata && plugin_watcher.as_ref().is_none_or(|watcher|!watcher.healthy()) {plugin_watch_ok=false;}
                    let plugin_changed = metadata && plugin_dirty.swap(false,Ordering::AcqRel);
                    let schedule_changed = if !metadata {
                        let mut changed = collector.schedules["health"] != "disabled"
                            || collector.schedule_report.is_some() || collector.plugins["health"] != "disabled";
                        collector.schedules = json!({"items":[],"health":"disabled"});
                        collector.plugins = json!({"items":[],"health":"disabled"});
                        collector.plugin_report = None;
                        collector.schedule_report = None;
                        collector.metadata_reports.clear();
                        collector.thread_metadata.clear();
                        let previous_workdirs = std::mem::take(&mut collector.overflow_thread_workdirs);
                        if command_switches.worktrees {
                            let mut scratch = HashMap::new();
                            if collector.populate_overflow_workdirs(&root, &mut scratch) { schedule_dirty.store(true, Ordering::Release); let _ = events.try_send(()); }
                            collector.overflow_thread_workdirs = scratch.into_iter().filter_map(|(id,entry)| entry["_cwd"].as_str().map(|cwd| (id,cwd.to_owned()))).collect();
                        } else { collector.overflow_workdirs.clear(); collector.overflow_workdir_scan = None; }
                        changed |= previous_workdirs != collector.overflow_thread_workdirs;
                        collector.project_details.clear();
                        collector.project_roots_status = projects::RootStatus::default();
                        collector.dots = std::array::from_fn(|index| json!({"window":WINDOWS[index],"activity_items_scope":"latest_loaded_snapshot","activity":[]}));
                        changed
                    } else if metadata_changed || plugin_changed || schedule_dirty.swap(false, Ordering::AcqRel) || command_switches.worktrees != collector.applied_command_switches.worktrees {
                        collector.overflow_thread_workdirs.clear();
                        let reference = now();
                        let metadata_identities = collector.metadata_identities(&schedule_threads);
                        let (mut metadata, mut metadata_reports) = schedules::background_metadata(&home, &metadata_identities, command_switches.worktrees, false);
                        let (mut projects, roots_status) = projects::read(&home, &mut metadata, command_switches.worktrees, &mut metadata_reports);
                        if command_switches.worktrees {
                            if collector.populate_overflow_workdirs(&root, &mut metadata) { schedule_dirty.store(true, Ordering::Release); let _ = events.try_send(()); }
                        } else { collector.overflow_workdirs.clear(); collector.overflow_workdir_scan = None; }
                        if !roots_status.complete { projects::preserve_roots(&mut projects, &collector.project_details); }
                        let dots = crate::dots::background(&home, &metadata, reference);
                        schedules::merge_remote_state(&home, &mut metadata);
                        let related_report = collector.related_lifecycle(&root, &mut metadata);
                        if related_report["pending_files"].as_u64().unwrap_or(0) > 0 { schedule_dirty.store(true, Ordering::Release); let _ = events.try_send(()); }
                        metadata_reports.push(related_report);
                        let (schedules, report) = schedules::read(&home, reference, &metadata);
                        let (plugins, mut plugin_report, watch_paths) = plugins::scan(&home);
                        let (watch_ok, watch_changed) = plugin_watcher.as_mut().map(|watcher|watcher.sync(&watch_paths)).unwrap_or((false,false));
                        plugin_watch_ok = watch_ok && plugin_report["watch_truncated"] != true;
                        plugin_report["watch_health"] = json!(if plugin_watch_ok {"ok"} else {"unavailable"});
                        if watch_changed {schedule_dirty.store(true,Ordering::Release);let _=events.try_send(());}

                        let changed = !same_metadata_reports(&metadata_reports, &collector.metadata_reports) || schedules != collector.schedules
                            || plugins != collector.plugins || collector.plugin_report.as_ref() != Some(&plugin_report)
                            || dots != collector.dots
                            || metadata != collector.thread_metadata
                            || projects != collector.project_details
                            || roots_status != collector.project_roots_status
                            || collector.schedule_report.as_ref().is_none_or(|previous| {
                                let mut previous = previous.clone();
                                let mut current = report.clone();
                                previous.as_object_mut().map(|row| row.remove("checked_at"));
                                current.as_object_mut().map(|row| row.remove("checked_at"));
                                previous != current
                            });
                        collector.schedules = schedules;
                        collector.plugins = plugins;
                        collector.plugin_report = Some(plugin_report);
                        collector.dots = dots;
                        collector.thread_metadata = metadata;
                        collector.project_details = projects;
                        collector.project_roots_status = roots_status;
                        collector.schedule_report = Some(report);
                        collector.metadata_reports = metadata_reports;
                        changed
                    } else {
                        false
                    };
                    collector.set_phase("worktrees");
                    let worktree_timer = crate::performance_debug::PhaseTimer::start(true);
                    let mut workdirs: BTreeMap<String, String> = collector.thread_metadata.iter().filter_map(|(id, value)| value["_cwd"].as_str().map(|cwd| (id.clone(), cwd.to_owned()))).collect();
                    for (id,cwd) in &collector.overflow_thread_workdirs { workdirs.entry(id.clone()).or_insert_with(|| cwd.clone()); }
                    let mut sessions: Vec<_> = collector.files.values().filter(|session| session.projection.id.is_some() && session.projection.workdir.is_some()).collect();
                    sessions.sort_by_key(|session| std::cmp::Reverse(session.projection.updated()));
                    for session in sessions {
                        if let (Some(id), Some(cwd)) = (&session.projection.id, &session.projection.workdir) { workdirs.entry(id.clone()).or_insert_with(|| cwd.clone()); }
                    }
                    let worktree_changed = collector.worktrees.refresh(&collector.project_details, &workdirs, collector.project_roots_status, command_switches.worktrees, schedule_changed || requested || worktree_signal || command_switches.worktrees != collector.applied_command_switches.worktrees);
                    let worktree_phase = worktree_timer.map(|timer| timer.finish());
                    if let Some(watcher) = worktree_watcher.as_mut() {
                        if watcher.refresh(&home, collector.worktrees.notification_roots(), command_switches.worktrees) {
                            worktree_dirty.store(true, Ordering::Release);
                            let _ = events.try_send(());
                        }
                    }
                    // Register before scanning so changes during the scan remain observable.
                    collector.set_phase("diagnostics");
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
                    let desktop_timer = crate::performance_debug::PhaseTimer::start(true);
                    let batch = collector.desktop.refresh(&desktop_roots, now());
                    *desktop_source_writer.write().unwrap_or_else(|error| error.into_inner()) = collector.desktop.observed_sources();
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
                    if logs_changed && logs_enabled {
                        rescan_core_logs = true;
                        rescan_desktop_logs = true;
                    }
                    if !logs_enabled {
                        collector.core.logs.clear();
                        collector.desktop.logs.clear();
                    }
                    if !model_api_enabled {
                        collector.core.api_events.clear();
                        collector.desktop.api_events.clear();
                    }
                    let mut rescan_ready = false;
                    if rescan_core_logs && collector.core.reset_log_scan() {
                        rescan_core_logs = false;
                        *core_source_writer.write().unwrap_or_else(|error| error.into_inner()) = None;
                        if clear_core_for_errors {
                            collector.core.logs.clear(); collector.core.errors.clear();
                            collector.core.api_events.clear(); collector.core.sql.clear();
                            clear_core_for_errors = false;
                        }
                        rescan_ready = true;
                    }
                    if rescan_desktop_logs && collector.desktop.reset_log_scan() {
                        rescan_desktop_logs = false;
                        desktop_source_writer.write().unwrap_or_else(|error| error.into_inner()).clear();
                        if clear_desktop_for_errors {
                            collector.desktop.logs.clear(); collector.desktop.errors.clear(); collector.desktop.api_events.clear(); collector.desktop.sql.clear();
                            clear_desktop_for_errors = false;
                        }
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
                        || (metadata && !plugin_watch_ok)
                        || schedule_failed.load(Ordering::Acquire)
                        || desktop_failed.load(Ordering::Acquire)
                        || collector.jev_watch_health != Some(true)
                        || (command_switches.worktrees && worktree_watcher.as_ref().is_none_or(|watcher| !watcher.healthy()))
                    {
                        "unavailable"
                    } else {
                        "ok"
                    };
                    collector.applied_errors_enabled = errors_enabled;
                    collector.applied_sqlite_enabled = sqlite_enabled;
                    collector.applied_model_api_enabled = model_api_enabled;
                    collector.applied_command_switches = command_switches;
                    let catalog = mcp::discover(&home);
                    let catalog_changed = catalog != collector.mcp_catalog;
                    collector.mcp_catalog = catalog;
                    let (future_changed, next_future) = collector.checkpoints.pending_activity(now()).unwrap_or((false, Some(now() + Duration::seconds(5))));
                    let selection_pending = collector.recent_call_refresh_pending();
                    if changed
                        || selection_pending
                        || future_changed
                        || catalog_changed
                        || errors_changed
                        || sqlite_changed
                        || model_api_changed
                        || commands_changed
                        || account_changed
                        || cloud_changed
                        || public_changed
                        || platform_changed
                        || jev_changed
                        || diagnostic_changed
                        || schedule_changed
                        || worktree_changed
                        || metadata_changed
                        || logs_changed
                        || desktop_changed
                        || previous != (health, watch_health)
                        || previous_checkpoint != collector.checkpoints.report
                        || requested
                    {
                        previous = (health, watch_health);
                        let reference = now();
                        collector.set_phase("history");
                        let history_timer =
                            crate::performance_debug::PhaseTimer::start(true);
                        collector.refresh_activity(reference);
                        collector.refresh_jev_errors(reference);
                        collector.refresh_error_history(reference);
                        collector.refresh_skill_history();
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
                        {
                            let mut phases = serde_json::Map::new();
                            if let Some(phase) = session_phase {
                                phases.insert("sessions".into(), phase);
                            }
                            if let Some(phase) = worktree_phase {
                                phases.insert("worktrees".into(), phase);
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
                        metrics["read_bytes"] = json!(collector.read_bytes.saturating_add(collector.desktop.bytes_read()).saturating_sub(before_bytes));
                        collector.record_monitor_sample(&mut metrics);
                        collector
                            .debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .record("collection", metrics);
                        if changed || selection_pending || schedule_changed || metadata_changed || previous_checkpoint != collector.checkpoints.report {
                            let (index, ready) = collector.detail_index(&root);
                            collector.overflow_details_ready = ready;
                            *detail_writer.write().unwrap_or_else(|error| error.into_inner()) = index;
                            collector.recent_call_details_dirty.store(false, Ordering::Release);
                        }
                        version += 1;
                        collector.set_phase("projections");
                        let next = collector.snapshots(health, watch_health, version);
                        collector.set_phase("idle");
                        publish.send_replace(next);
                    }
                    collector.set_phase("idle");
                    if !rescan_ready
                        && !pending
                        && !collector.recent_call_refresh_pending()
                        && !collector.worktrees.pending()
                        && !collector.core.backfill()
                        && (!desktop_pending || desktop_retry)
                    {
                        if let Some(due) = next_future {
                            let delay = (due - now()).to_std().unwrap_or_default().max(std::time::Duration::from_millis(50)).min(std::time::Duration::from_secs(5));
                            if matches!(signal.recv_timeout(delay), Err(mpsc::RecvTimeoutError::Disconnected)) { break; }
                        } else if signal.recv().is_err() { break; }
                    }
                }
                }
            })?;
        Ok((
            Self {
                fonts,
                gpus,
                device_completion,
                traffic,
                codex_enabled,
                metadata_enabled,
                tool_events_enabled,
                files_enabled,
                errors_enabled,
                sqlite_enabled,
                model_api_enabled,
                account,
                cloud,
                public,
                platform,
                command_switches,
                mcp_sources,
                mcp_switches,
                customizations,
                tracking,
                journal,
                debug,
                details,
                core_source,
                desktop_sources,
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

fn start_catalog_watch(home: &Path, wake: SyncSender<()>) -> notify::Result<RecommendedWatcher> {
    let watched_home = home.to_path_buf();
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            let relevant = match result {
                Ok(event) => {
                    event.need_rescan()
                        || (!event.kind.is_access()
                            && event.paths.iter().any(|path| {
                                path == &watched_home.join("config.toml")
                                    || (cfg!(target_os = "macos") && path == &watched_home)
                            }))
                }
                Err(_) => true,
            };
            if relevant {
                let _ = wake.try_send(());
            }
        },
        Config::default().with_follow_symlinks(false),
    )?;
    watcher.watch(home, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn session_metadata_event(event: &Event, root: &Path) -> bool {
    !event.kind.is_access() && event.paths.iter().any(|path| path.starts_with(root))
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
                                path == &watched_home.join("config.toml")
                                    || path == &watched_home.join(".codex-global-state.json")
                                    || path == &watched_home.join("session_index.jsonl")
                                    || (path.parent() == Some(watched_home.as_path())
                                        && path
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .is_some_and(schedules::state_filename))
                                    || (cfg!(target_os = "macos") && path == &watched_home)
                            }));
                    let session_change = session_metadata_event(&event, &root);
                    if global || session_change {
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
    skip_restore: bool,
    legacy_lifecycle_checked: bool,
}

impl Session {
    fn error_events<'a>(
        &'a self,
        path: &'a Path,
        errors: &'a [SessionError],
    ) -> impl Iterator<Item = Value> + 'a {
        Self::projection_error_events(&self.projection, path, errors)
    }

    fn projection_error_events<'a>(
        projection: &'a Projection,
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
                    projection.id.as_deref(),
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
            fields.insert("thread_id".into(), json!(projection.id));
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
        tool_events_enabled: bool,
        files_enabled: bool,
        mcp_sources: &BTreeMap<String, bool>,
        mcp_switches: McpSwitches,
        errors_enabled: bool,
        sqlite_enabled: bool,
        command_switches: CommandSwitches,
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
        let restored = if self.identity.is_none() && !self.skip_restore {
            let loaded = checkpoints.restore(root, path, &mut stream, identity, size, call_limit);
            self.checkpoint_signature = loaded.signature;
            loaded.state
        } else {
            None
        };
        self.skip_restore = false;
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
            malformed += u64::from(!self.projection.consume(
                line,
                base + position as u64,
                tool_events_enabled,
                files_enabled,
                mcp_sources,
                mcp_switches,
                errors_enabled,
                sqlite_enabled,
                command_switches,
            ));
        }
        if !self.legacy_lifecycle_checked && self.projection.id.is_some() {
            self.legacy_lifecycle_checked = true;
            if let Some(cached) = checkpoints.legacy_lifecycle(path) {
                if cached["offset"].as_u64().is_some_and(|offset| {
                    offset <= size && self.complete_offset().min(65_536) <= offset
                }) {
                    self.projection.restore_cached_lifecycle(&cached);
                }
            }
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

fn same_metadata_reports(left: &[Value], right: &[Value]) -> bool {
    fn without_time(values: &[Value]) -> Vec<Value> {
        values
            .iter()
            .cloned()
            .map(|mut value| {
                if let Some(object) = value.as_object_mut() {
                    object.remove("checked_at");
                }
                value
            })
            .collect()
    }
    without_time(left) == without_time(right)
}

fn apply_thread_metadata(row: &mut Value, metadata: Option<&Value>, metadata_only: bool) {
    row["metadata_only"] = json!(metadata_only);
    let Some(metadata) = metadata else { return };
    row["thread_name"] = metadata["name"].clone();
    let thread_name = metadata["name"].clone();
    if let Some(messages) = row["agent_messages"].as_array_mut() {
        for message in messages {
            if message["direction"] == "outgoing" {
                message["thread_name"] = thread_name.clone();
            }
        }
    }
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
                row["status_updated_at"] = metadata
                    .get("status_updated_at")
                    .cloned()
                    .unwrap_or_else(|| metadata["last_run_at"].clone());
                row["status_source"] = metadata
                    .get("status_source")
                    .cloned()
                    .unwrap_or_else(|| json!("catalog_status"));
            }
        }
    }
}

struct RelatedLifecycle {
    path: PathBuf,
    identity: (u64, u64),
    size: u64,
    modified: Option<SystemTime>,
    status: Value,
}

struct CachedOverflowWorkdir {
    checkpoint_signature: String,
    source_signature: ((u64, u64), u64, Option<SystemTime>),
    cwd: String,
}

#[derive(Default)]
struct OverflowWorkdirScan {
    revision: i64,
    after: String,
    retained: BTreeMap<String, String>,
    validating: bool,
    validated: String,
    winners: BTreeMap<String, Option<chrono::DateTime<Utc>>>,
    metadata: HashMap<String, Value>,
    bytes: usize,
    incomplete: bool,
}

struct SourceMembership {
    identity: u64,
    db: rusqlite::Connection,
    generation: i64,
    revision: i64,
}

struct MembershipRecord {
    key: String,
    fingerprint: Option<String>,
}

struct VerifiedMembership {
    key: String,
    fingerprint: String,
    checkpoint_signature: String,
    materialized: bool,
    call_headers: Option<Vec<lam_history_store::CallHeader>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceCallCursor {
    event_time: Option<String>,
    source_key: String,
    call_id: String,
}

#[derive(Debug, PartialEq, Eq)]
struct SourceCallCandidate {
    position: SourceCallCursor,
    checkpoint_signature: String,
}

#[derive(Clone)]
struct CallSelection {
    calls: BTreeMap<String, BTreeSet<String>>,
    complete: bool,
}

struct RecentCallScan {
    identity: u64,
    epoch: i64,
    limit: usize,
    scan_limit: usize,
    input_hash: [u8; 32],
    after: Option<SourceCallCursor>,
    heap: std::collections::BinaryHeap<std::cmp::Reverse<(Option<chrono::DateTime<Utc>>, Arc<str>, String, String)>>,
    valid: bool,
    finished: bool,
}

struct SelectedDetailScan {
    input_hash: [u8; 32],
    after: String,
    reader: Option<lam_history_store::SelectedCheckpointReader>,
    index: details::Index,
    readiness: BTreeMap<String, bool>,
    signatures: BTreeMap<String, String>,
    staged_bytes: usize,
    validating: bool,
    finished: bool,
    valid: bool,
}

fn source_fingerprint(
    identity: (u64, u64),
    size: u64,
    modified: Option<SystemTime>,
) -> Option<String> {
    let modified = modified?;
    let value = format!("{}:{}:{}:{modified:?}", identity.0, identity.1, size);
    Some(format!("{:x}", Sha256::digest(value.as_bytes())))
}

impl SourceMembership {
    fn new() -> rusqlite::Result<Self> {
        let db = rusqlite::Connection::open("")?;
        db.execute_batch("PRAGMA cache_size=-1024; PRAGMA foreign_keys=ON; CREATE TABLE sources(source_key TEXT PRIMARY KEY, fingerprint TEXT, checkpoint_signature TEXT, materialized INTEGER NOT NULL DEFAULT 0, verified INTEGER NOT NULL DEFAULT 0, generation INTEGER NOT NULL, revision INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID; CREATE TABLE working(source_key TEXT PRIMARY KEY) WITHOUT ROWID; CREATE TABLE source_calls(source_key TEXT NOT NULL,call_id TEXT NOT NULL,event_time TEXT,PRIMARY KEY(source_key,call_id),FOREIGN KEY(source_key) REFERENCES sources(source_key) ON DELETE CASCADE) WITHOUT ROWID; CREATE INDEX source_calls_recent ON source_calls(event_time DESC,source_key,call_id);")?;
        db.execute_batch("CREATE TABLE selection_epoch(value INTEGER NOT NULL); INSERT INTO selection_epoch VALUES(0);
            CREATE TRIGGER selection_source_insert AFTER INSERT ON sources BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_source_delete AFTER DELETE ON sources BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_source_update AFTER UPDATE ON sources WHEN OLD.fingerprint IS NOT NEW.fingerprint OR OLD.checkpoint_signature IS NOT NEW.checkpoint_signature OR OLD.materialized IS NOT NEW.materialized OR OLD.verified IS NOT NEW.verified OR OLD.revision IS NOT NEW.revision BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_call_insert AFTER INSERT ON source_calls BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_call_delete AFTER DELETE ON source_calls BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_working_insert AFTER INSERT ON working BEGIN UPDATE selection_epoch SET value=value+1; END;
            CREATE TRIGGER selection_working_delete AFTER DELETE ON working BEGIN UPDATE selection_epoch SET value=value+1; END;")?;
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Ok(Self {
            identity: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            db,
            generation: 1,
            revision: 0,
        })
    }

    fn next_generation(&mut self) -> rusqlite::Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        Ok(())
    }

    fn invalidate(&mut self) -> rusqlite::Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        Ok(())
    }

    fn insert(&mut self, keys: &[MembershipRecord]) -> rusqlite::Result<()> {
        let tx = self.db.transaction()?;
        {
            let mut statement = tx.prepare("INSERT INTO sources(source_key,fingerprint,generation) VALUES(?,?,?) ON CONFLICT(source_key) DO UPDATE SET materialized=CASE WHEN excluded.fingerprint IS NOT NULL AND sources.fingerprint=excluded.fingerprint THEN sources.materialized ELSE 0 END,checkpoint_signature=CASE WHEN excluded.fingerprint IS NOT NULL AND sources.fingerprint=excluded.fingerprint THEN sources.checkpoint_signature ELSE NULL END,verified=CASE WHEN excluded.fingerprint IS NOT NULL AND sources.fingerprint=excluded.fingerprint THEN sources.verified ELSE 0 END,fingerprint=excluded.fingerprint,generation=excluded.generation")?;
            for entry in keys {
                statement.execute(rusqlite::params![
                    entry.key,
                    entry.fingerprint,
                    self.generation
                ])?;
            }
        }
        tx.commit()
    }

    fn remove_stale(&mut self) -> rusqlite::Result<usize> {
        self.db.execute("DELETE FROM sources WHERE source_key IN (SELECT source_key FROM sources WHERE generation!=? LIMIT 512)", [self.generation])
    }

    fn set_working(&mut self, keys: &[String]) -> rusqlite::Result<()> {
        if keys.len() > FILE_LIMIT {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let existing: BTreeSet<String> = self.db.prepare("SELECT source_key FROM working")?
            .query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        if existing == keys.iter().cloned().collect() {
            self.db.execute("UPDATE sources SET verified=0 WHERE verified=1 AND materialized=0 AND source_key NOT IN (SELECT source_key FROM working)", [])?;
            return Ok(());
        }
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM working", [])?;
        {
            let mut statement =
                tx.prepare("INSERT OR IGNORE INTO working(source_key) VALUES(?)")?;
            for key in keys {
                statement.execute([key])?;
            }
        }
        tx.execute("UPDATE sources SET verified=0 WHERE verified=1 AND materialized=0 AND source_key NOT IN (SELECT source_key FROM working)", [])?;
        tx.commit()
    }

    #[cfg(test)]
    fn page(&self, after: &str, limit: usize) -> rusqlite::Result<Vec<String>> {
        if limit == 0 || limit > 64 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.db.prepare(
            "SELECT source_key FROM sources WHERE source_key>? ORDER BY source_key LIMIT ?",
        )?;
        let values = statement
            .query_map(rusqlite::params![after, limit as u32], |row| row.get(0))?
            .collect();
        values
    }

    fn mark_verified(&mut self, keys: &[VerifiedMembership]) -> rusqlite::Result<()> {
        if keys.len() > 512 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.db.transaction()?;
        {
            let mut statement = tx.prepare("UPDATE sources SET verified=1,revision=?,checkpoint_signature=?,materialized=? WHERE source_key=? AND fingerprint=? AND generation=?")?;
            let mut clear = tx.prepare("DELETE FROM source_calls WHERE source_key=?")?;
            let mut insert = tx.prepare("INSERT INTO source_calls(source_key,call_id,event_time) VALUES(?,?,?)")?;
            for key in keys {
                if key.checkpoint_signature.len() > 512
                    || key.materialized != key.call_headers.is_some()
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                if key.call_headers.as_ref().is_some_and(|headers| headers.len() > FILE_CALL_LIMIT || headers.iter().any(|header| header.call_id.is_empty() || header.call_id.len() > 160 || header.event_time.as_ref().is_some_and(|time| time.len() > 40))) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let updated = statement.execute(rusqlite::params![
                    self.revision,
                    key.checkpoint_signature,
                    key.materialized,
                    key.key,
                    key.fingerprint,
                    self.generation
                ])?;
                if updated != 0 {
                    clear.execute([&key.key])?;
                    if let Some(headers) = &key.call_headers {
                        for header in headers {
                            insert.execute(rusqlite::params![key.key, header.call_id, header.event_time])?;
                        }
                    }
                }
            }
        }
        tx.commit()
    }

    // A raw page advances over stale entries as well, keeping each read bounded.
    fn recent_call_page(&self, after: Option<&SourceCallCursor>, limit: usize)
        -> rusqlite::Result<(Vec<SourceCallCandidate>, Option<SourceCallCursor>)>
    {
        if !(1..=512).contains(&limit) || after.is_some_and(|cursor| {
            cursor.source_key.is_empty() || cursor.source_key.len() > 256 || cursor.call_id.is_empty()
                || cursor.call_id.len() > 160 || cursor.event_time.as_ref().is_some_and(|time| time.len() > 40)
        }) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut candidates = Vec::new();
        let mut last = None;
        let mut remaining = limit;
        {
            let mut scan = |condition: &str, parameters: Vec<rusqlite::types::Value>| -> rusqlite::Result<()> {
                if remaining == 0 { return Ok(()); }
                let sql = format!("SELECT c.event_time,c.source_key,c.call_id, CASE WHEN s.verified=1 AND s.revision=? AND s.generation=? AND NOT EXISTS(SELECT 1 FROM working w WHERE w.source_key=c.source_key) THEN s.checkpoint_signature END FROM source_calls c JOIN sources s ON s.source_key=c.source_key {condition} ORDER BY c.event_time DESC,c.source_key,c.call_id LIMIT ?");
                let mut args = vec![self.revision.into(), self.generation.into()];
                args.extend(parameters);
                args.push((remaining as i64).into());
                let mut query = self.db.prepare(&sql)?;
                let mut rows = query.query(rusqlite::params_from_iter(args))?;
                while let Some(row) = rows.next()? {
                    let position = SourceCallCursor { event_time:row.get(0)?, source_key:row.get(1)?, call_id:row.get(2)? };
                    let signature: Option<String> = row.get(3)?;
                    remaining -= 1;
                    if let Some(checkpoint_signature) = signature {
                        candidates.push(SourceCallCandidate { position:position.clone(), checkpoint_signature });
                    }
                    last = Some(position);
                }
                Ok(())
            };
            match after {
                None => scan("", vec![])?,
                Some(cursor) if cursor.event_time.is_none() => scan(
                    "WHERE c.event_time IS NULL AND (c.source_key,c.call_id)>(?,?)",
                    vec![cursor.source_key.clone().into(), cursor.call_id.clone().into()],
                )?,
                Some(cursor) => {
                    let time = cursor.event_time.as_ref().expect("checked event time");
                    scan("WHERE c.event_time=? AND (c.source_key,c.call_id)>(?,?)",
                        vec![time.clone().into(), cursor.source_key.clone().into(), cursor.call_id.clone().into()])?;
                    scan("WHERE c.event_time<?", vec![time.clone().into()])?;
                    scan("WHERE c.event_time IS NULL", vec![])?;
                }
            }
        }
        Ok((candidates, if remaining == 0 { last } else { None }))
    }

    fn unverified_count(&self) -> rusqlite::Result<usize> {
        self.db.query_row(
            "SELECT count(*) FROM sources WHERE verified=0 OR revision!=?",
            [self.revision],
            |row| {
                let count: i64 = row.get(0)?;
                usize::try_from(count).map_err(|_| rusqlite::Error::InvalidQuery)
            },
        )
    }

    fn is_verified(&self, key: &str) -> rusqlite::Result<bool> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE source_key=? AND generation=? AND verified=1 AND revision=? AND checkpoint_signature IS NOT NULL)", rusqlite::params![key, self.generation, self.revision], |row| row.get(0))
    }

    fn matches_verified(
        &self,
        key: &str,
        fingerprint: &str,
        signature: &str,
    ) -> rusqlite::Result<bool> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE source_key=? AND fingerprint=? AND checkpoint_signature=? AND generation=? AND revision=? AND verified=1)", rusqlite::params![key,fingerprint,signature,self.generation,self.revision], |row| row.get(0))
    }

    fn verified_page(&self, after: &str, limit: usize) -> rusqlite::Result<Vec<(String, String)>> {
        if limit == 0 || limit > 64 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.db.prepare("SELECT source_key,checkpoint_signature FROM sources WHERE verified=1 AND revision=? AND generation=? AND checkpoint_signature IS NOT NULL AND source_key>? ORDER BY source_key LIMIT ?")?;
        let rows = statement
            .query_map(
                rusqlite::params![self.revision, self.generation, after, limit as u32],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect();
        rows
    }

    fn materialized_page(
        &self,
        after: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<(String, String)>> {
        if limit == 0 || limit > 64 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.db.prepare("SELECT source_key,checkpoint_signature FROM sources WHERE verified=1 AND revision=? AND generation=? AND checkpoint_signature IS NOT NULL AND materialized=1 AND NOT EXISTS(SELECT 1 FROM working WHERE working.source_key=sources.source_key) AND source_key>? ORDER BY source_key LIMIT ?")?;
        let rows = statement
            .query_map(
                rusqlite::params![self.revision, self.generation, after, limit as u32],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect();
        rows
    }

    fn materialized_count(&self) -> rusqlite::Result<usize> {
        self.db.query_row("SELECT count(*) FROM sources WHERE verified=1 AND revision=? AND generation=? AND checkpoint_signature IS NOT NULL AND materialized=1", rusqlite::params![self.revision,self.generation], |row| {
            let count: i64 = row.get(0)?;
            usize::try_from(count).map_err(|_| rusqlite::Error::InvalidQuery)
        })
    }

    fn unverified_page(&self, after: &str, limit: usize) -> rusqlite::Result<Vec<String>> {
        if limit == 0 || limit > 64 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.db.prepare("SELECT source_key FROM sources WHERE (verified=0 OR revision!=?) AND source_key>? ORDER BY source_key LIMIT ?")?;
        let values = statement
            .query_map(
                rusqlite::params![self.revision, after, limit as u32],
                |row| row.get(0),
            )?
            .collect();
        values
    }
}

struct SessionDiscovery {
    root: PathBuf,
    root_identity: (u64, u64),
    entries: walkdir::IntoIter,
    candidates: BTreeSet<(Option<SystemTime>, PathBuf)>,
    indexed: BTreeMap<String, (Option<SystemTime>, PathBuf)>,
    duplicate_sources: BTreeMap<String, PathBuf>,
    selected: BTreeSet<String>,
    overflow_targets: BTreeSet<String>,
    overflow_paths: BTreeMap<String, PathBuf>,
    overflow_exhausted: bool,
    file_count: usize,
    incomplete: bool,
    membership: Option<SourceMembership>,
    membership_failed: bool,
    membership_pending: Vec<MembershipRecord>,
    walk_done: bool,
}

impl SessionDiscovery {
    fn new(
        root: &Path,
        root_identity: (u64, u64),
        selected: BTreeSet<String>,
        mut membership: Option<SourceMembership>,
        overflow_targets: BTreeSet<String>,
        overflow_exhausted: bool,
    ) -> Self {
        let membership_failed = membership
            .as_mut()
            .is_some_and(|index| index.next_generation().is_err());
        if membership_failed {
            membership = None;
        }
        Self {
            root: root.to_path_buf(),
            root_identity,
            entries: WalkDir::new(root)
                .follow_root_links(false)
                .max_open(10)
                .into_iter(),
            candidates: BTreeSet::new(),
            indexed: BTreeMap::new(),
            duplicate_sources: BTreeMap::new(),
            selected,
            overflow_targets,
            overflow_paths: BTreeMap::new(),
            overflow_exhausted,
            file_count: 0,
            incomplete: membership_failed,
            membership,
            membership_failed,
            membership_pending: Vec::new(),
            walk_done: false,
        }
    }

    fn flush_membership(&mut self) {
        if self.membership_pending.is_empty() {
            return;
        }
        if self
            .membership
            .as_mut()
            .is_none_or(|membership| membership.insert(&self.membership_pending).is_err())
        {
            self.membership = None;
            self.membership_failed = true;
            self.incomplete = true;
        }
        self.membership_pending.clear();
    }

    fn advance(&mut self, limit: usize, stopped: &AtomicBool) -> bool {
        for _ in 0..limit {
            if self.walk_done {
                break;
            }
            if stopped.load(Ordering::Acquire) {
                return false;
            }
            let Some(entry) = self.entries.next() else {
                self.flush_membership();
                self.walk_done = true;
                break;
            };
            let Ok(entry) = entry else {
                self.incomplete = true;
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_str().unwrap_or_default();
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                self.incomplete = true;
                continue;
            };
            if redirected(&meta) {
                self.incomplete = true;
                continue;
            }
            self.file_count = self.file_count.saturating_add(1);
            let fingerprint =
                source_fingerprint(file_identity(&meta), meta.len(), meta.modified().ok());
            let item = (meta.modified().ok(), entry.into_path());
            if !self.overflow_targets.is_empty() {
                if let Some(key) = checkpoint::key(&self.root, &item.1)
                    .filter(|key| self.overflow_targets.contains(key))
                {
                    self.overflow_paths.insert(key, item.1.clone());
                }
            }
            if self.file_count == FILE_LIMIT + 1
                && !self.membership_failed
                && self.membership.is_none()
            {
                match SourceMembership::new() {
                    Ok(membership) => {
                        self.membership = Some(membership);
                        let keys: Vec<_> = self
                            .candidates
                            .iter()
                            .filter_map(|(_, path)| {
                                checkpoint::key(&self.root, path).map(|key| MembershipRecord {
                                    key,
                                    fingerprint: fs::symlink_metadata(path)
                                        .ok()
                                        .filter(|metadata| {
                                            metadata.is_file() && !redirected(metadata)
                                        })
                                        .and_then(|metadata| {
                                            source_fingerprint(
                                                file_identity(&metadata),
                                                metadata.len(),
                                                metadata.modified().ok(),
                                            )
                                        }),
                                })
                            })
                            .collect();
                        if keys.len() != self.candidates.len() {
                            self.incomplete = true;
                        }
                        self.membership_pending.extend(keys);
                    }
                    Err(_) => {
                        self.membership_failed = true;
                        self.incomplete = true;
                    }
                }
            }
            if self.membership.is_some() {
                match checkpoint::key(&self.root, &item.1) {
                    Some(key) => self
                        .membership_pending
                        .push(MembershipRecord { key, fingerprint }),
                    None => {
                        self.incomplete = true;
                    }
                }
                if self.membership_pending.len() >= 512 {
                    self.flush_membership();
                }
            }
            if let Some(id) = item
                .1
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(project::filename_identity)
            {
                if let Some(previous) = self.indexed.get_mut(&id) {
                    for path in [&previous.1, &item.1] {
                        if self.duplicate_sources.len() < WALK_LIMIT {
                            if let Some(key) = checkpoint::key(&self.root, path) {
                                self.duplicate_sources.insert(key, path.clone());
                            }
                        }
                    }
                    if item > *previous {
                        *previous = item.clone();
                    }
                } else if self.indexed.len() < WALK_LIMIT || self.selected.contains(&id) {
                    self.indexed.insert(id, item.clone());
                }
            }
            self.candidates.insert(item);
            if self.candidates.len() > FILE_LIMIT {
                self.candidates.pop_first();
            }
        }
        self.flush_membership();
        if !self.walk_done {
            return false;
        }
        if !self.incomplete {
            if let Some(membership) = self.membership.as_mut() {
                match membership.remove_stale() {
                    Ok(removed) if removed > 0 => return false,
                    Ok(_) => {}
                    Err(_) => {
                        self.membership = None;
                        self.incomplete = true;
                    }
                }
            }
        }
        true
    }
}

struct Collector {
    runtime: Value,
    collection: Arc<Mutex<crate::performance_debug::CollectionStatus>>,
    device_completion: Arc<Mutex<crate::runtime_info::DeviceCompletion>>,
    fonts: Arc<RwLock<Option<Vec<String>>>>,
    gpus: Arc<RwLock<Option<Vec<Value>>>>,
    memory: Arc<Mutex<crate::runtime_info::Memory>>,
    process_usage: Arc<Mutex<crate::process_usage::Sampler>>,
    traffic: Arc<Mutex<crate::server::Traffic>>,
    initial_codex: bool,
    force_replay: bool,
    backend_revision: String,
    frontend_revision: Option<String>,
    files: BTreeMap<PathBuf, Session>,
    discovery: Option<SessionDiscovery>,
    discovered_files: Option<usize>,
    source_membership: Option<SourceMembership>,
    source_membership_root: Option<(u64, u64)>,
    session_root: Option<PathBuf>,
    source_membership_complete: bool,
    recent_call_scan: Mutex<Option<RecentCallScan>>,
    recent_call_details_dirty: AtomicBool,
    selected_detail_scan: Mutex<Option<SelectedDetailScan>>,
    overflow_files: BTreeMap<PathBuf, Session>,
    overflow_cursor: String,
    session_paths: BTreeMap<String, PathBuf>,
    duplicate_session_paths: BTreeMap<String, PathBuf>,
    related_lifecycle: BTreeMap<String, RelatedLifecycle>,
    related_budget: usize,
    overflow_details_ready: BTreeSet<String>,
    overflow_workdirs: BTreeMap<String, CachedOverflowWorkdir>,
    overflow_workdir_scan: Option<OverflowWorkdirScan>,
    overflow_thread_workdirs: BTreeMap<String, String>,
    cursor: usize,
    read_bytes: u64,
    monitor_history: std::collections::VecDeque<Value>,
    trimmed_calls: u64,
    refreshes: u64,
    started_instant: std::time::Instant,
    last_heartbeat: std::time::Instant,
    malformed: u64,
    started: chrono::DateTime<Utc>,
    updated: Option<chrono::DateTime<Utc>>,
    checkpoints: Checkpoints,
    activity_history: Arc<Vec<Value>>,
    activity_view: Option<ObservationView>,
    known_retention_days: Option<i64>,
    activity_history_health: &'static str,
    error_history: Vec<Value>,
    jev_errors: Vec<Value>,
    jev_error_write_error: Option<&'static str>,
    skill_history: Vec<Value>,
    jev_watch_health: Option<bool>,
    error_history_read_error: Option<&'static str>,
    core: core_logs::Core,
    desktop: desktop::DesktopCollector,
    schedules: Value,
    plugins: Value,
    plugin_report: Option<Value>,
    dots: [Value; 4],
    thread_metadata: HashMap<String, Value>,
    project_details: BTreeMap<String, Value>,
    project_roots_status: projects::RootStatus,
    worktrees: worktrees::Collector,
    schedule_report: Option<Value>,
    metadata_reports: Vec<Value>,
    metadata_enabled: bool,
    tool_events_enabled: Arc<AtomicBool>,
    previous_tool_events_enabled: bool,
    files_enabled: Arc<AtomicBool>,
    previous_files_enabled: bool,
    errors_enabled: Arc<AtomicBool>,
    previous_errors_enabled: bool,
    applied_errors_enabled: bool,
    sqlite_enabled: Arc<AtomicBool>,
    previous_sqlite_enabled: bool,
    applied_sqlite_enabled: bool,
    model_api_enabled: Arc<AtomicBool>,
    account: Arc<account::Shared>,
    cloud: Arc<account::Shared>,
    public: [Arc<account::Shared>; 2],
    platform: [Arc<account::Shared>; 4],
    applied_model_api_enabled: bool,
    command_switches: Arc<RwLock<CommandSwitches>>,
    previous_command_switches: CommandSwitches,
    applied_command_switches: CommandSwitches,
    mcp_sources: Arc<RwLock<BTreeMap<String, bool>>>,
    previous_mcp_sources: BTreeMap<String, bool>,
    mcp_view_sources: BTreeMap<String, bool>,
    mcp_catalog: BTreeMap<String, Option<source_introduction::Description>>,
    catalog_home: PathBuf,
    mcp_switches: Arc<RwLock<McpSwitches>>,
    previous_mcp_switches: McpSwitches,
    customizations: Arc<RwLock<McpCustomizations>>,
    tracking: Arc<RwLock<TrackingSettings>>,
    logs_enabled: bool,
    journal: Arc<Mutex<crate::journal::Journal>>,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
}

impl Collector {
    fn discovered_file_count(&self) -> usize {
        self.discovered_files.unwrap_or(self.files.len())
    }
    fn related_lifecycle(&mut self, root: &Path, metadata: &mut HashMap<String, Value>) -> Value {
        const TAIL: usize = 65_536;
        let selected: BTreeSet<_> = metadata
            .iter()
            .filter_map(|(id, value)| {
                value["execution"]["parent_thread_id"]
                    .as_str()
                    .map(|_| id.clone())
            })
            .collect();
        self.related_lifecycle.retain(|id, _| selected.contains(id));
        let loaded: BTreeSet<_> = self
            .files
            .values()
            .filter_map(|state| state.projection.id.as_deref())
            .collect();
        let mut report = json!({"name":"subagent_lifecycle","location":root.to_string_lossy(),"health":"ok","checked_at":stamp(now()),"fields":["thread_id","task_started","task_complete","timestamp"],"file_limit":FILE_LIMIT,"tail_bytes":TAIL,"rows_read":0,"bytes_read":0,"missing_files":0,"pending_files":0});
        for id in selected {
            if loaded.contains(id.as_str()) {
                continue;
            }
            let Some(path) = self.session_paths.get(&id) else {
                report["missing_files"] = json!(report["missing_files"].as_u64().unwrap_or(0) + 1);
                continue;
            };
            let result = (|| -> io::Result<Option<(RelatedLifecycle, usize, usize)>> {
                let before = fs::symlink_metadata(path)?;
                if redirected(&before) || !before.is_file() {
                    return Err(io::Error::other("Unsafe session file"));
                }
                let identity = file_identity(&before);
                let modified = before.modified().ok();
                if let Some(cached) = self.related_lifecycle.get(&id).filter(|cached| {
                    cached.path == *path
                        && cached.identity == identity
                        && cached.size == before.len()
                        && cached.modified == modified
                }) {
                    return Ok(Some((
                        RelatedLifecycle {
                            path: path.clone(),
                            identity,
                            size: before.len(),
                            modified,
                            status: cached.status.clone(),
                        },
                        0,
                        0,
                    )));
                }
                let size = before.len().min(TAIL as u64) as usize;
                if self.related_budget < size {
                    return Ok(None);
                }
                let mut file = open_source(root, path)?;
                let opened = file.metadata()?;
                if redirected(&opened) || !opened.is_file() || file_identity(&opened) != identity {
                    return Err(io::Error::other("Changed session file"));
                }
                let offset = before.len().saturating_sub(size as u64);
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = Vec::new();
                file.take(size as u64).read_to_end(&mut bytes)?;
                let after = fs::symlink_metadata(path)?;
                if redirected(&after)
                    || file_identity(&after) != identity
                    || before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                {
                    return Err(io::Error::other("Changed session file"));
                }
                let content = if offset > 0 {
                    bytes
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map_or(&[][..], |index| &bytes[index + 1..])
                } else {
                    &bytes[..]
                };
                let mut status = json!({});
                let mut rows = 0;
                for line in content.split(|byte| *byte == b'\n') {
                    let Ok(value) = serde_json::from_slice::<Value>(line) else {
                        continue;
                    };
                    if value["type"] != "event_msg" {
                        continue;
                    }
                    let kind = value["payload"]["type"].as_str();
                    if !matches!(kind, Some("task_started" | "task_complete")) {
                        continue;
                    }
                    let Some(time) = value["timestamp"]
                        .as_str()
                        .filter(|time| time.chars().count() <= 40)
                        .and_then(lam_history_store::normalize_utc_stamp)
                    else {
                        continue;
                    };
                    if status["status_updated_at"].as_str().unwrap_or("") <= time.as_str() {
                        status = json!({"status":if kind == Some("task_started") {"running"} else {"completed"},"status_updated_at":time,"status_source":"session_lifecycle"});
                        rows += 1;
                    }
                }
                Ok(Some((
                    RelatedLifecycle {
                        path: path.clone(),
                        identity,
                        size: before.len(),
                        modified,
                        status,
                    },
                    bytes.len(),
                    rows,
                )))
            })();
            match result {
                Ok(Some((cached, bytes, rows))) => {
                    self.related_budget -= bytes;
                    self.read_bytes += bytes as u64;
                    report["bytes_read"] =
                        json!(report["bytes_read"].as_u64().unwrap_or(0) + bytes as u64);
                    report["rows_read"] =
                        json!(report["rows_read"].as_u64().unwrap_or(0) + rows as u64);
                    if let (Some(entry), Some(status)) = (
                        metadata.get_mut(&id).and_then(Value::as_object_mut),
                        cached.status.as_object(),
                    ) {
                        entry.extend(status.clone());
                    }
                    self.related_lifecycle.insert(id, cached);
                }
                Ok(None) => {
                    report["pending_files"] =
                        json!(report["pending_files"].as_u64().unwrap_or(0) + 1)
                }
                Err(_) => {
                    self.related_lifecycle.remove(&id);
                    report["health"] = json!("partly_unavailable");
                }
            }
        }
        report
    }
    fn set_phase(&self, phase: &'static str) {
        self.collection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set(phase);
    }
    fn record_monitor_sample(&mut self, metrics: &mut Value) {
        self.refreshes = self.refreshes.saturating_add(1);
        if self.last_heartbeat.elapsed() >= std::time::Duration::from_secs(60) {
            self.journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("heartbeat");
            self.last_heartbeat = std::time::Instant::now();
        }
        let traffic = *self
            .traffic
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        metrics["snapshot_bytes"] = json!(traffic.snapshot_bytes);
        metrics["transfer_bytes"] = json!(traffic.transfer_bytes);
        let mut sql = 0;
        let mut web = 0;
        let mut mcp = 0;
        for event in self.activity_history.iter() {
            match event.get("server").and_then(Value::as_str) {
                Some("web") => web += 1,
                Some(_) => mcp += 1,
                None => sql += 1,
            }
        }
        if let Some(phases) = metrics.get("phase_metrics").and_then(Value::as_object) {
            if let Some(maximum) = phases
                .values()
                .filter_map(|phase| phase.get("cpu_ms").and_then(Value::as_f64))
                .reduce(f64::max)
            {
                metrics["max_phase_cpu_ms"] = json!(maximum);
            }
        }
        metrics["sql_records"] = json!(sql);
        metrics["web_records"] = json!(web);
        metrics["mcp_records"] = json!(mcp);
        metrics["retained_calls"] = json!(self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum::<usize>());
        metrics["buffer_bytes"] = json!(self
            .files
            .values()
            .map(|state| state.partial.len())
            .sum::<usize>());
        metrics["trimmed_calls"] = json!(self.trimmed_calls);
        metrics["call_limit"] = json!(CALL_LIMIT);
        metrics["file_limit"] = json!(FILE_LIMIT);
        metrics["buffer_limit"] = json!(BUFFER_LIMIT);
        let mut sample = metrics.clone();
        if let Some(fields) = sample.as_object_mut() {
            fields.insert("time".into(), json!(stamp(now())));
        }
        if self.monitor_history.len() == 360 {
            self.monitor_history.pop_front();
        }
        self.monitor_history.push_back(sample);
    }
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
        let catalog_home = directory.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mcp_catalog = mcp::discover(&catalog_home);
        let mut collector = Self {
            runtime: crate::runtime_info::snapshot(),
            collection: Arc::new(Mutex::new(crate::performance_debug::CollectionStatus::new())),
            device_completion: Arc::new(Mutex::new(
                crate::runtime_info::DeviceCompletion::default(),
            )),
            fonts: Arc::new(RwLock::new(None)),
            gpus: Arc::new(RwLock::new(None)),
            memory: Arc::new(Mutex::new(crate::runtime_info::Memory::new())),
            process_usage: Arc::new(Mutex::new(crate::process_usage::Sampler::new(
                directory.clone(),
            ))),
            traffic: Arc::new(Mutex::new(crate::server::Traffic::default())),
            initial_codex: enabled,
            force_replay: false,
            catalog_home: catalog_home.clone(),
            mcp_catalog,
            files: BTreeMap::new(),
            discovery: None,
            discovered_files: None,
            source_membership: None,
            source_membership_root: None,
            session_root: None,
            source_membership_complete: false,
            recent_call_scan: Mutex::new(None),
            recent_call_details_dirty: AtomicBool::new(false),
            selected_detail_scan: Mutex::new(None),
            overflow_files: BTreeMap::new(),
            overflow_cursor: String::new(),
            session_paths: BTreeMap::new(),
            duplicate_session_paths: BTreeMap::new(),
            related_lifecycle: BTreeMap::new(),
            related_budget: 0,
            overflow_details_ready: BTreeSet::new(),
            overflow_workdirs: BTreeMap::new(),
            overflow_workdir_scan: None,
            overflow_thread_workdirs: BTreeMap::new(),
            cursor: 0,
            read_bytes: 0,
            monitor_history: std::collections::VecDeque::with_capacity(360),
            trimmed_calls: 0,
            refreshes: 0,
            started_instant: std::time::Instant::now(),
            last_heartbeat: std::time::Instant::now(),
            malformed: 0,
            started,
            backend_revision,
            frontend_revision,
            updated: None,
            checkpoints: Checkpoints::new(directory, enabled),
            activity_history: Arc::new(Vec::new()),
            activity_view: None,
            known_retention_days: None,
            activity_history_health: "waiting",
            error_history: Vec::new(),
            jev_errors: Vec::new(),
            jev_error_write_error: None,
            skill_history: Vec::new(),
            jev_watch_health: None,
            error_history_read_error: None,
            core: core_logs::Core::new(enabled),
            desktop: desktop::DesktopCollector::default(),
            schedules: json!({"items":[],"health":if enabled {"waiting"} else {"disabled"}}),
            plugins: json!({"items":[],"health":if enabled {"waiting"} else {"disabled"}}),
            plugin_report: None,
            dots: std::array::from_fn(
                |index| json!({"window":WINDOWS[index],"activity_items_scope":"latest_loaded_snapshot","activity":[]}),
            ),
            schedule_report: None,
            metadata_reports: Vec::new(),
            thread_metadata: HashMap::new(),
            project_details: BTreeMap::new(),
            project_roots_status: projects::RootStatus::default(),
            worktrees: worktrees::Collector::new(catalog_home.clone()),
            metadata_enabled: true,
            tool_events_enabled: Arc::new(AtomicBool::new(true)),
            previous_tool_events_enabled: true,
            files_enabled: Arc::new(AtomicBool::new(true)),
            previous_files_enabled: true,
            errors_enabled: Arc::new(AtomicBool::new(true)),
            previous_errors_enabled: true,
            applied_errors_enabled: true,
            sqlite_enabled: Arc::new(AtomicBool::new(true)),
            previous_sqlite_enabled: true,
            applied_sqlite_enabled: true,
            model_api_enabled: Arc::new(AtomicBool::new(true)),
            account: Arc::new(account::Shared::new()),
            cloud: Arc::new(account::Shared::new()),
            public: std::array::from_fn(|_| Arc::new(account::Shared::new())),
            platform: std::array::from_fn(|_| Arc::new(account::Shared::new())),
            applied_model_api_enabled: true,
            command_switches: Arc::new(RwLock::new(CommandSwitches::default())),
            previous_command_switches: CommandSwitches::default(),
            applied_command_switches: CommandSwitches::default(),
            mcp_sources: Arc::new(RwLock::new(BTreeMap::new())),
            previous_mcp_sources: BTreeMap::new(),
            mcp_view_sources: BTreeMap::new(),
            mcp_switches: Arc::new(RwLock::new(McpSwitches::default())),
            previous_mcp_switches: McpSwitches::default(),
            customizations: Arc::new(RwLock::new(McpCustomizations::default())),
            tracking: Arc::new(RwLock::new(TrackingSettings::default())),
            logs_enabled: true,
            journal: Arc::new(Mutex::new(journal)),
            debug,
        };
        collector.desktop.source =
            json!({"source":"codex_desktop","health":if enabled {"waiting"} else {"disabled"}});
        collector.known_retention_days = collector.checkpoints.activity_retention().ok();
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
        let sources = self
            .mcp_sources
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let excluded: Vec<&str> = sources
            .iter()
            .filter_map(|(name, enabled)| (!enabled).then_some(name.as_str()))
            .collect();
        match self
            .checkpoints
            .observed_view(reference, &cutoffs, &excluded)
        {
            Ok(view) => {
                self.activity_history = Arc::clone(&view.events);
                self.known_retention_days = Some(view.retention_days);
                self.activity_view = Some(view);
                self.activity_history_health = "ok";
            }
            Err(_) => {
                self.activity_view = None;
                self.known_retention_days = None;
                self.activity_history_health = "unavailable";
            }
        }
        self.mcp_view_sources = sources.clone();
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

    fn refresh_jev_errors(&mut self, reference: chrono::DateTime<Utc>) {
        let sources = self
            .mcp_sources
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let switches = *self
            .mcp_switches
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let enabled = self.applied_errors_enabled
            && self.applied_command_switches.jev
            && switches.mcp
            && sources.get("jev") != Some(&false);
        let errors = if enabled {
            jev::error_events(&jev::snapshot(&self.catalog_home, "all", reference, true))
        } else {
            Vec::new()
        };
        drop(sources);
        if !errors.is_empty() {
            match self.checkpoints.save_diagnostics(&[], &errors, reference) {
                Ok(()) => self.jev_error_write_error = None,
                Err(error) => self.jev_error_write_error = Some(storage_error_type(&error)),
            }
        }
        self.jev_errors = errors;
    }

    fn refresh_skill_history(&mut self) {
        if let Ok(events) = self.checkpoints.skill_preview() {
            self.skill_history = events;
        }
    }

    fn refresh(
        &mut self,
        root: &Path,
        stopped: &AtomicBool,
        errors_enabled: bool,
        sqlite_enabled: bool,
        command_switches: CommandSwitches,
    ) -> (&'static str, bool, bool) {
        self.related_budget = 0;
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
        let enabled = self.tool_events_enabled.load(Ordering::Acquire);
        let files_enabled = self.files_enabled.load(Ordering::Acquire);
        let sources_lock = Arc::clone(&self.mcp_sources);
        let sources = sources_lock
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let switches_lock = Arc::clone(&self.mcp_switches);
        let switches = switches_lock
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let source_reenabled = (!self.previous_mcp_switches.mcp && switches.mcp)
            || (!self.previous_mcp_switches.web && switches.web)
            || self
                .previous_mcp_sources
                .iter()
                .any(|(name, enabled)| !enabled && sources.get(name) != Some(&false));
        if self.force_replay
            || source_reenabled
            || (enabled && !self.previous_tool_events_enabled)
            || (files_enabled && !self.previous_files_enabled)
            || (errors_enabled && !self.previous_errors_enabled)
            || (sqlite_enabled && !self.previous_sqlite_enabled)
            || (command_switches.git && !self.previous_command_switches.git)
            || (command_switches.checks && !self.previous_command_switches.checks)
            || (command_switches.jev_calls && !self.previous_command_switches.jev_calls)
            || (command_switches.skills && !self.previous_command_switches.skills)
            || (command_switches.worktrees && !self.previous_command_switches.worktrees)
            || (command_switches.usage && !self.previous_command_switches.usage)
        {
            let invalidate_failed = self
                .source_membership
                .as_mut()
                .is_some_and(|membership| membership.invalidate().is_err())
                || self
                    .discovery
                    .as_mut()
                    .and_then(|scan| scan.membership.as_mut())
                    .is_some_and(|membership| membership.invalidate().is_err());
            if invalidate_failed {
                self.source_membership_complete = false;
            }
            self.overflow_cursor.clear();
            for (path, state) in &mut self.files {
                let signature = state.checkpoint_signature.take();
                *state = Session::new(path);
                state.checkpoint_signature = signature;
                state.skip_restore = true;
            }
            for (path, state) in &mut self.overflow_files {
                let signature = state.checkpoint_signature.take();
                *state = Session::new(path);
                state.checkpoint_signature = signature;
                state.skip_restore = true;
            }
        }
        self.force_replay = false;
        self.previous_tool_events_enabled = enabled;
        self.previous_files_enabled = files_enabled;
        self.previous_errors_enabled = errors_enabled;
        self.previous_sqlite_enabled = sqlite_enabled;
        self.previous_command_switches = command_switches;
        self.previous_mcp_sources = sources.clone();
        self.previous_mcp_switches = *switches;
        let root_identity = match fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() && !redirected(&meta) => file_identity(&meta),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let changed = !self.files.is_empty();
                self.files.clear();
                self.session_paths.clear();
                self.duplicate_session_paths.clear();
                self.discovery = None;
                self.discovered_files = Some(0);
                self.source_membership = None;
                self.source_membership_root = None;
                self.source_membership_complete = false;
                self.overflow_files.clear();
                self.overflow_cursor.clear();
                return ("missing", false, changed);
            }
            _ => {
                let changed = !self.files.is_empty();
                self.files.clear();
                self.session_paths.clear();
                self.duplicate_session_paths.clear();
                self.discovery = None;
                self.discovered_files = None;
                self.source_membership = None;
                self.source_membership_root = None;
                self.source_membership_complete = false;
                self.overflow_files.clear();
                self.overflow_cursor.clear();
                return ("unavailable", false, changed);
            }
        };
        self.session_root = Some(root.to_path_buf());
        if self
            .discovery
            .as_ref()
            .is_none_or(|scan| scan.root_identity != root_identity)
        {
            let prior = if self.source_membership_root == Some(root_identity) {
                self.source_membership.take()
            } else {
                self.source_membership = None;
                None
            };
            let targets: BTreeSet<_> =
                if self.overflow_files.is_empty() && self.source_membership_complete {
                    prior
                        .as_ref()
                        .and_then(|index| index.unverified_page(&self.overflow_cursor, 64).ok())
                        .unwrap_or_default()
                        .into_iter()
                        .collect()
                } else {
                    BTreeSet::new()
                };
            let overflow_exhausted = targets.is_empty()
                && self.overflow_files.is_empty()
                && !self.overflow_cursor.is_empty();
            if targets.is_empty() && self.overflow_files.is_empty() {
                self.overflow_cursor.clear();
            }
            self.discovery = Some(SessionDiscovery::new(
                root,
                root_identity,
                self.thread_metadata.keys().cloned().collect(),
                prior,
                targets,
                overflow_exhausted,
            ));
        }
        if !self
            .discovery
            .as_mut()
            .expect("session discovery")
            .advance(WALK_LIMIT, stopped)
        {
            return ("waiting", true, saved);
        }
        let discovery = self.discovery.take().expect("completed session discovery");
        let overflow_exhausted = discovery.overflow_exhausted;
        let overflow_target_count = discovery.overflow_targets.len();
        let mut overflow_paths: BTreeMap<_, _> = discovery
            .overflow_targets
            .into_iter()
            .map(|key| (key, None))
            .collect();
        for (key, path) in discovery.overflow_paths {
            overflow_paths.insert(key, Some(path));
        }
        let discovery_changed = self.discovered_files != Some(discovery.file_count);
        self.discovered_files = Some(discovery.file_count);
        self.source_membership_complete = !discovery.incomplete && discovery.membership.is_some();
        self.source_membership = discovery.membership;
        self.source_membership_root = self.source_membership.as_ref().map(|_| root_identity);
        let mut incomplete = discovery.incomplete || discovery.file_count > FILE_LIMIT;
        self.session_paths = discovery
            .indexed
            .into_iter()
            .map(|(id, (_, path))| (id, path))
            .collect();
        self.duplicate_session_paths = discovery.duplicate_sources;
        let paths: Vec<_> = discovery.candidates.into_iter().rev().collect();
        let previous_files = self.files.len();
        let mut files = BTreeMap::new();
        let ordered: Vec<_> = paths.into_iter().map(|(_, path)| path).collect();
        if let Some(membership) = self.source_membership.as_mut() {
            let keys: Vec<_> = ordered
                .iter()
                .filter_map(|path| checkpoint::key(root, path))
                .collect();
            if keys.len() != ordered.len() || membership.set_working(&keys).is_err() {
                self.source_membership_complete = false;
                incomplete = true;
            }
        }
        for path in &ordered {
            let state = self
                .files
                .remove(path)
                .or_else(|| self.overflow_files.remove(path))
                .unwrap_or_else(|| Session::new(path));
            files.insert(path.clone(), state);
        }
        let mut changed =
            saved || discovery_changed || !self.files.is_empty() || files.len() != previous_files;
        self.files = files;
        self.overflow_files
            .retain(|path, _| !self.files.contains_key(path));
        for path in self.overflow_files.keys() {
            if let Some(key) = checkpoint::key(root, path) {
                overflow_paths.insert(key, Some(path.clone()));
            }
        }
        let reserve = if overflow_paths.is_empty() {
            0
        } else {
            CHUNK_LIMIT
        };
        let mut budget = ROUND_LIMIT - reserve;
        let length = ordered.len();
        let start = self.cursor % length.max(1);
        let mut pending = false;
        let mut retained: usize = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        retained += self
            .overflow_files
            .values()
            .map(|state| state.projection.calls.len())
            .sum::<usize>();
        for step in 0..length {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            let index = (start + step) % length;
            if budget == 0 {
                pending = true;
                break;
            }
            if step % 64 == 0 {
                let paths: Vec<_> = (step..(step + 64).min(length))
                    .map(|step| &ordered[(start + step) % length])
                    .filter(|path| {
                        self.files
                            .get(*path)
                            .is_some_and(|state| state.identity.is_none() && !state.skip_restore)
                    })
                    .cloned()
                    .collect();
                self.checkpoints.prefetch(root, &paths);
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
                enabled,
                files_enabled,
                &sources,
                *switches,
                errors_enabled,
                sqlite_enabled,
                command_switches,
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
            self.trimmed_calls = self
                .trimmed_calls
                .saturating_add(std::mem::take(&mut state.projection.newly_trimmed_calls));
            retained = retained - before + state.projection.calls.len();
        }
        self.bound_state(&ordered);
        for state in self.files.values_mut() {
            self.trimmed_calls = self
                .trimmed_calls
                .saturating_add(std::mem::take(&mut state.projection.newly_trimmed_calls));
        }
        self.bound_errors();
        changed |= self.checkpoints.save(root, &mut self.files);
        changed |= self.checkpoints.save_unanchored_errors(&mut self.files);
        if let Some(membership) = self.source_membership.as_mut() {
            let verified: Vec<_> = self
                .files
                .iter()
                .filter(|(_, state)| {
                    !state.pending()
                        && state.anchor.is_some()
                        && !state.checkpoint_dirty
                        && state.checkpoint_signature.is_some()
                })
                .filter_map(|(path, state)| {
                    Some(VerifiedMembership {
                        key: checkpoint::key(root, path)?,
                        fingerprint: source_fingerprint(
                            state.identity?,
                            state.size,
                            state.modified,
                        )?,
                        checkpoint_signature: state.checkpoint_signature.clone()?,
                        materialized: false,
                        call_headers: None,
                    })
                })
                .collect();
            if verified
                .chunks(512)
                .any(|batch| membership.mark_verified(batch).is_err())
            {
                self.source_membership_complete = false;
                incomplete = true;
            }
        }
        let mut overflow_budget = budget + reserve;
        let mut advanced = false;
        if !overflow_paths.is_empty() {
            self.checkpoints.begin_round();
        }
        let active_buffer: usize = self.files.values().map(|state| state.partial.len()).sum();
        for (key, path) in overflow_paths {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            let Some(path) = path else {
                self.overflow_cursor = key;
                advanced = true;
                continue;
            };
            if self.files.contains_key(&path) {
                self.overflow_cursor = key;
                advanced = true;
                continue;
            }
            if !self.overflow_files.is_empty() && !self.overflow_files.contains_key(&path) {
                break;
            }
            if overflow_budget == 0 {
                pending = true;
                break;
            }
            let state = self
                .overflow_files
                .entry(path.clone())
                .or_insert_with(|| Session::new(&path));
            // One overflow source is loaded at a time and bounded by the per-file limit.
            // Its calls must survive checkpointing even when the working set is full.
            let available = FILE_CALL_LIMIT;
            let read = match state.read(
                root,
                &path,
                overflow_budget.min(CHUNK_LIMIT),
                available as u32,
                enabled,
                files_enabled,
                &sources,
                *switches,
                errors_enabled,
                sqlite_enabled,
                command_switches,
                &mut self.checkpoints,
            ) {
                Ok((bytes, malformed, modified)) => {
                    self.read_bytes += bytes as u64;
                    self.malformed += malformed;
                    overflow_budget -= bytes;
                    changed |= modified;
                    bytes
                }
                Err(_) => {
                    if state.checkpoint_dirty
                        || !state.projection.pending_errors.is_empty()
                        || state.unsaved_tool_errors()
                    {
                        incomplete = true;
                        break;
                    }
                    self.overflow_files.remove(&path);
                    self.overflow_cursor = key;
                    advanced = true;
                    incomplete = true;
                    continue;
                }
            };
            let state = self
                .overflow_files
                .get_mut(&path)
                .expect("overflow session");
            if active_buffer + state.partial.len() > BUFFER_LIMIT {
                state.partial.clear();
                state.discard = true;
                state.checkpoint_dirty = true;
            }
            if state.projection.calls.len() > available {
                let mut calls: Vec<_> = state
                    .projection
                    .calls
                    .iter()
                    .map(|(id, call)| (call.time, id.clone()))
                    .collect();
                calls.sort_unstable();
                let extra = calls.len() - available;
                for (_, id) in calls.into_iter().take(extra) {
                    state.projection.retire_call(&id);
                }
                state.projection.prune_order();
            }
            self.trimmed_calls = self
                .trimmed_calls
                .saturating_add(std::mem::take(&mut state.projection.newly_trimmed_calls));
            changed |= self.checkpoints.save(root, &mut self.overflow_files);
            changed |= self
                .checkpoints
                .save_unanchored_errors(&mut self.overflow_files);
            let state = self.overflow_files.get(&path).expect("overflow session");
            if state.pending() {
                if read > 0 {
                    pending = true;
                } else {
                    incomplete = true;
                }
                break;
            }
            if state.checkpoint_dirty
                || state.checkpoint_signature.is_none()
                || state.anchor.is_none()
            {
                incomplete = true;
                break;
            }
            let fingerprint = state
                .identity
                .and_then(|identity| source_fingerprint(identity, state.size, state.modified));
            let Some(fingerprint) = fingerprint else {
                incomplete = true;
                break;
            };
            let record = VerifiedMembership {
                key: key.clone(),
                fingerprint,
                checkpoint_signature: state
                    .checkpoint_signature
                    .clone()
                    .expect("checked signature"),
                materialized: true,
                call_headers: Some(state.projection.calls.iter().map(|(id, call)| lam_history_store::CallHeader {
                    call_id: id.clone(), event_time: call.time.map(stamp),
                }).collect()),
            };
            let verified = self.source_membership.as_mut().map(|membership| {
                membership.mark_verified(&[record])?;
                membership.is_verified(&key)
            });
            match verified {
                Some(Ok(true)) => {}
                Some(Ok(false)) => {
                    incomplete = true;
                    pending = true;
                    break;
                }
                _ => {
                    self.source_membership_complete = false;
                    incomplete = true;
                    break;
                }
            }
            self.overflow_files.remove(&path);
            self.overflow_cursor = key;
            advanced = true;
        }
        self.related_budget = overflow_budget;
        if !overflow_exhausted && self.overflow_files.is_empty() && self.source_membership_complete
        {
            if overflow_target_count == 0 && self.overflow_cursor.is_empty() {
                pending |= self
                    .source_membership
                    .as_ref()
                    .and_then(|membership| membership.unverified_count().ok())
                    .is_some_and(|count| count > 0);
            } else if advanced {
                pending = true;
            }
        }
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

    fn error_windows(&self, health: &str, reference: chrono::DateTime<Utc>, threads: &BTreeMap<&str, (Projection, bool)>, session: &[Value]) -> [(Value, Bytes); 4] {
        let sources = self
            .mcp_sources
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let journal = self
            .journal
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor_entries = journal.entries();
        let monitor_source = journal.source();
        let monitor_errors: Vec<Value> = journal
            .events()
            .into_iter()
            .filter_map(|event| {
                let kind = event["kind"].as_str()?;
                if !matches!(kind, "refresh_failed" | "http_response_error" | "frontend_error") {
                    return None;
                }
                Some(json!({"timestamp":event["timestamp"],"category":"monitor","source":"monitor",
                    "severity":"error","code":event["error_type"].as_str().filter(|value| !value.is_empty()).unwrap_or("http_error"),
                    "http_status":event["http_status"],"reason":kind}))
            })
            .collect();
        drop(journal);
        let tool_errors: Vec<_> = threads
            .iter()
            .flat_map(|(thread, (projection, _))| {
                projection.calls.iter().filter_map(move |(id, call)| {
                    call.failure_event(thread, id).map(|event| (call.time, event))
                })
            })
            .collect();
        let session_source = json!({"source":"session","health":health,"files":[],"file_count":self.discovered_file_count(),
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
            .or(self.jev_error_write_error)
            .or(self.checkpoints.error_write_error)
            .or_else(|| self.desktop.source["storage_error"].as_str())
            .or_else(|| {
                self.core
                    .storage_error
                    .filter(|_| !self.core.pending_errors.is_empty())
            });
        let history_health = if health == "disabled" && self.jev_errors.is_empty() {
            "disabled"
        } else if error_type.is_some() {
            "unavailable"
        } else if self.updated.is_some() {
            "ok"
        } else {
            "waiting"
        };
        let history = json!({"health":history_health,"error_type":error_type,
                            "retained":if self.applied_errors_enabled { self.error_history.len() } else { 0 },"limit":ERROR_PREVIEW_LIMIT,
                            "hours":ERROR_PREVIEW_HOURS});
        std::array::from_fn(|index| {
            let boundary = HOURS[index].map(|hours| reference - Duration::hours(hours));
            let mut errors = visible_logs(
                session
                    .iter()
                    .filter(|_| health != "disabled")
                    .filter(|event| event["record_hash"].is_string())
                    .chain(
                        tool_errors
                            .iter()
                            .filter(|_| health != "disabled")
                            .filter(|(start, _)| {
                                boundary
                                    .is_none_or(|cutoff| start.is_some_and(|time| time >= cutoff))
                            })
                            .map(|(_, event)| event),
                    )
                    .chain(self.core.errors.iter().filter(|_| health != "disabled"))
                    .chain(self.desktop.errors.iter().filter(|_| health != "disabled"))
                    .chain(&self.jev_errors)
                    .chain(&self.error_history)
                    .chain(
                        session
                            .iter()
                            .filter(|_| health != "disabled")
                            .filter(|event| event["record_hash"].is_null()),
                    )
                    .filter(|event| {
                        event["source"] != "tool_result"
                            || event["server"]
                                .as_str()
                                .is_none_or(|server| sources.get(server) != Some(&false))
                    })
                    .filter(|_| self.applied_errors_enabled)
                    .chain(&monitor_errors),
                boundary,
            );
            let errors_enabled = self.applied_errors_enabled;
            let mut logs = if self.logs_enabled {
                visible_logs(
                    self.core
                        .logs
                        .iter()
                        .filter(|_| health != "disabled")
                        .chain(self.desktop.logs.iter().filter(|_| health != "disabled"))
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
            let enabled = errors_enabled;
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
            let diagnostic_health = if errors_enabled {
                json!({"core":self.core.health(),"desktop":desktop_source["health"]})
            } else {
                json!({"core":"disabled","desktop":"disabled"})
            };
            let summary = json!({"enabled":enabled,"total":total,"events":errors,
                                 "categories":categories,"severities":severities,"limit":ERROR_LIMIT,
                                 "history":history,
                                 "diagnostics":{"health":diagnostic_health}});
            (summary, logs)
        })
    }

    fn metadata_identities(&self, observed: &BTreeSet<String>) -> BTreeSet<String> {
        let active_calls = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        let (overflow, _) = self.overflow_projections(active_calls);
        let mut identities = observed.clone();
        identities.extend(overflow.into_iter().filter_map(|item| item.projection.id));
        identities
    }

    fn recent_call_refresh_pending(&self) -> bool {
        if self.discovery.is_none() && self.discovered_file_count() <= self.files.len() {
            // Selection is no longer called by snapshot/detail in this state.
            // Drop both continuation and publication work so the service can idle.
            *self.recent_call_scan.lock().unwrap_or_else(|error| error.into_inner()) = None;
            self.recent_call_details_dirty.store(false, Ordering::Release);
            *self.selected_detail_scan.lock().unwrap_or_else(|error| error.into_inner()) = None;
            return false;
        }
        self.discovery.is_none() && self.source_membership_complete && self.source_membership.is_some()
            && (self.recent_call_details_dirty.load(Ordering::Acquire) || self.recent_call_scan_pending()
                || self.selected_detail_scan.lock().unwrap_or_else(|error| error.into_inner()).as_ref().is_some_and(|scan| !scan.finished))
    }

    fn recent_call_scan_pending(&self) -> bool {
        self.discovery.is_none() && self.source_membership_complete && self.source_membership.is_some()
            && self.recent_call_scan.lock().unwrap_or_else(|error| error.into_inner()).as_ref().is_some_and(|scan| !scan.finished)
    }

    // Select ordering keys only; payloads remain in checkpoint storage. Retain a
    // bounded top-K heap and keyset cursor between rounds, never an entire scan.
    fn select_recent_calls(&self, limit: usize, scan_limit: usize) -> Option<CallSelection> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        if limit == 0 || limit > CALL_LIMIT || scan_limit == 0 { return None; }
        let root = self.session_root.as_deref()?;
        let index = self.source_membership.as_ref()
            .filter(|_| self.discovery.is_none() && self.source_membership_complete)?;
        let epoch: i64 = index.db.query_row("SELECT value FROM selection_epoch", [], |row| row.get(0)).ok()?;
        let mut valid = index.unverified_count().ok()? == 0;
        let mut paths = BTreeMap::<String, Arc<str>>::new();
        for path in self.files.keys().chain(self.session_paths.values()).chain(self.duplicate_session_paths.values()) {
            if let (Some(key), Some(text)) = (checkpoint::key(root, path), path.to_str()) {
                paths.insert(key, Arc::from(text));
            } else { valid = false; }
        }
        // Active calls and path mappings are not owned by the SQLite index.
        // Hash their bounded current inputs to reject stale continuation state.
        let mut input = Sha256::new();
        let hash_text = |hash: &mut Sha256, value: &str| {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        };
        input.update(index.revision.to_le_bytes());
        input.update((paths.len() as u64).to_le_bytes());
        for (key, path) in &paths {
            hash_text(&mut input, key); hash_text(&mut input, path);
        }
        input.update((self.files.len() as u64).to_le_bytes());
        for (path, state) in &self.files {
            hash_text(&mut input, path.to_str().unwrap_or_default());
            input.update((state.projection.calls.len() as u64).to_le_bytes());
            for (id, call) in &state.projection.calls {
                hash_text(&mut input, id);
                hash_text(&mut input, &call.time.map(stamp).unwrap_or_default());
            }
        }
        let input_hash: [u8; 32] = input.finalize().into();
        let mut cached = self.recent_call_scan.lock().unwrap_or_else(|error| error.into_inner());
        let reset = cached.as_ref().is_none_or(|scan| scan.identity != index.identity || scan.epoch != epoch
            || scan.limit != limit || scan.scan_limit != scan_limit || scan.input_hash != input_hash);
        if reset {
            let mut heap = BinaryHeap::new();
            for (path, state) in &self.files {
                let Some(key) = checkpoint::key(root, path) else { valid = false; continue };
                let Some(path_text) = paths.get(&key) else { valid = false; continue };
                for (id, call) in &state.projection.calls {
                    heap.push(Reverse((call.time, Arc::clone(path_text), id.clone(), key.clone())));
                    if heap.len() > limit { heap.pop(); }
                }
            }
            *cached = Some(RecentCallScan { identity: index.identity, epoch, limit, scan_limit, input_hash,
                after: None, heap, valid, finished: false });
        }
        let scan = cached.as_mut()?;
        let was_finished = scan.finished;
        let mut scanned = 0;
        while !scan.finished && scanned < scan_limit {
            let page_limit = 512.min(scan_limit - scanned);
            let (page, next) = match index.recent_call_page(scan.after.as_ref(), page_limit) {
                Ok(page) => page,
                Err(_) => { scan.valid = false; scan.finished = true; break; }
            };
            scanned += page_limit;
            for row in page {
                let Some(path) = paths.get(&row.position.source_key) else { scan.valid = false; continue };
                let time = match row.position.event_time.as_deref() {
                    Some(value) => match lam_history_store::parse_utc_stamp(value) {
                        Some(time) => Some(time),
                        None => { scan.valid = false; continue; }
                    },
                    None => None,
                };
                scan.heap.push(Reverse((time, Arc::clone(path), row.position.call_id, row.position.source_key)));
                if scan.heap.len() > limit { scan.heap.pop(); }
            }
            let Some(cursor) = next else { scan.finished = true; break };
            if scan.heap.len() == limit {
                let cursor_time = match cursor.event_time.as_deref() {
                    Some(value) => match lam_history_store::parse_utc_stamp(value) {
                        Some(time) => Some(Some(time)),
                        None => { scan.valid = false; None }
                    },
                    None => Some(None),
                };
                if cursor_time.is_some_and(|cursor_time| scan.heap.peek().is_some_and(|Reverse((time, _, _, _))| *time > cursor_time)) {
                    scan.finished = true;
                }
            }
            scan.after = Some(cursor);
        }
        let mut calls = BTreeMap::<String, BTreeSet<String>>::new();
        for Reverse((_, _, id, source)) in &scan.heap {
            calls.entry(source.clone()).or_default().insert(id.clone());
        }
        if !was_finished && scan.finished && scan.valid {
            self.recent_call_details_dirty.store(true, Ordering::Release);
        }
        Some(CallSelection { calls, complete: scan.valid && scan.finished })
    }

    fn overflow_path<'a>(&'a self, root: &Path, item: &checkpoint::SnapshotProjection) -> Option<&'a Path> {
        self.duplicate_session_paths.get(&item.source_key)
            .or_else(|| item.projection.id.as_ref().and_then(|id| self.session_paths.get(id)))
            .filter(|path| checkpoint::key(root, path).as_deref() == Some(item.source_key.as_str()))
            .map(PathBuf::as_path)
    }

    fn populate_overflow_workdirs(
        &mut self,
        root: &Path,
        metadata: &mut HashMap<String, Value>,
    ) -> bool {
        const CACHE_LIMIT: usize = 16 * 1024 * 1024;
        self.populate_overflow_workdirs_with_limit(root, metadata, CACHE_LIMIT)
    }

    fn populate_overflow_workdirs_with_limit(
        &mut self,
        root: &Path,
        metadata: &mut HashMap<String, Value>,
        cache_limit: usize,
    ) -> bool {
        if self.discovery.is_some() {
            return self.overflow_workdir_scan.is_some();
        }
        if !self.source_membership_complete || self.source_membership.is_none() {
            self.overflow_workdir_scan = None;
            return false;
        }
        let Some(membership) = self.source_membership.as_ref() else { return false };
        let Ok(Some(mut store)) = self.checkpoints.snapshot_reader() else { return false };
        const STAGING_LIMIT: usize = 16 * 1024 * 1024;
        const CURSOR_ROUND_LIMIT: usize = 16 * 1024 * 1024;
        const SOURCE_ROUND_LIMIT: usize = 64;
        let mut scan = self.overflow_workdir_scan.take()
            .filter(|scan| scan.revision == membership.revision)
            .unwrap_or_else(|| OverflowWorkdirScan { revision: membership.revision, ..Default::default() });
        scan.incomplete |= membership.unverified_count().map_or(true, |count| count != 0);
        let mut cursor_bytes = 0usize;
        let mut sources = 0usize;
        let mut cached_bytes: usize = self
            .overflow_workdirs
            .iter()
            .map(|(key, entry)| key.len() + entry.checkpoint_signature.len() + entry.cwd.len() + 256)
            .sum();
        let mut budget = ROUND_LIMIT;
        let mut pending = false;
        if !scan.validating {
            'pages: loop {
                let page = match membership.materialized_page(&scan.after, 16) {
                    Ok(page) => page,
                    Err(_) => { scan.incomplete = true; break; }
                };
                if page.is_empty() { break; }
                for (key, signature) in &page {
                    // One bounded cursor lookahead may exceed the round threshold,
                    // with a 16 MiB cursor plus bounded key/signature overhead.
                    if sources >= SOURCE_ROUND_LIMIT || cursor_bytes >= CURSOR_ROUND_LIMIT
                        || budget < LINE_LIMIT + 2048 {
                        pending = true;
                        break 'pages;
                    }
                    sources += 1;
                    // Workdir discovery needs cursor metadata, never call payloads.
                    // Decode one source at a time so total cursor bytes can exceed
                    // the per-read budget without retaining all projections.
                    let expected = lam_history_store::ExpectedCursor { source_key: key, signature: Some(signature) };
                    let item = match store.selected_checkpoint(&expected, &[]) {
                        Ok(Some(checkpoint)) => {
                            cursor_bytes += checkpoint.cursor.payload.len() + checkpoint.cursor.signature.len() + checkpoint.cursor.source_key.len();
                            checkpoint::snapshot_projection(checkpoint, false)
                        },
                        _ => { cursor_bytes += CURSOR_ROUND_LIMIT; None },
                    };
                    let Some(item) = item else { scan.incomplete = true; scan.after = key.clone(); continue };
                    let retained_cost = item.source_key.len() + item.checkpoint_signature.len() + 256;
                    // Reserve bounded bookkeeping and the maximum workdir result
                    // before mutating the staged state. Never publish a truncated scan.
                    if scan.bytes.saturating_add(retained_cost + 320 + 4 * 4096 + 256) > STAGING_LIMIT {
                        scan.incomplete = true;
                        break 'pages;
                    }
                    scan.bytes += retained_cost;
                    scan.retained.insert(item.source_key.clone(), item.checkpoint_signature.clone());
                    scan.after = key.clone();
                    let (Some(id), Some(position)) = (
                        item.projection.id.as_ref(),
                        item.projection.workdir_position.as_ref(),
                    ) else {
                        continue;
                    };
                    let updated = item.projection.updated();
                    if scan.winners.get(id).is_some_and(|previous| *previous >= updated) {
                        continue;
                    }
                    if metadata
                        .get(id)
                        .and_then(|entry| entry.get("_cwd"))
                        .and_then(Value::as_str)
                        .is_some_and(|cwd| !cwd.is_empty())
                    {
                        continue;
                    }
                    let active = self
                        .files
                        .values()
                        .filter(|state| {
                            state.projection.id.as_deref() == Some(id.as_str())
                                && state.projection.workdir.is_some()
                        })
                        .map(|state| state.projection.updated())
                        .max();
                    if active.is_some_and(|updated| updated >= item.projection.updated()) {
                        continue;
                    }
                    let cached = self
                        .overflow_workdirs
                        .get(&item.source_key)
                        .filter(|entry| {
                            let fingerprint = source_fingerprint(
                                entry.source_signature.0,
                                entry.source_signature.1,
                                entry.source_signature.2,
                            );
                            entry.checkpoint_signature == item.checkpoint_signature
                                && fingerprint.as_deref().is_some_and(|fingerprint| {
                                    self.source_membership.as_ref().is_some_and(|index| {
                                        index
                                            .matches_verified(
                                                &item.source_key,
                                                fingerprint,
                                                &item.checkpoint_signature,
                                            )
                                            .unwrap_or(false)
                                    })
                                })
                        })
                        .map(|entry| entry.cwd.clone());
                    if let Some(cwd) = cached {
                        if !scan.winners.contains_key(id) {
                            scan.bytes += 2 * id.len() + cwd.len() + 256;
                        } else {
                            scan.bytes = scan.bytes.saturating_sub(scan.metadata.get(id).and_then(|entry| entry["_cwd"].as_str()).map_or(0, str::len)) + cwd.len();
                        }
                        scan.winners.insert(id.clone(), updated);
                        scan.metadata
                            .entry(id.clone())
                            .or_insert_with(|| json!({"thread_id":id}))["_cwd"] = json!(cwd);
                        continue;
                    }
                    if let Some(previous) = self.overflow_workdirs.remove(&item.source_key) {
                        cached_bytes = cached_bytes.saturating_sub(item.source_key.len() + previous.checkpoint_signature.len() + previous.cwd.len() + 256);
                    }
                    let Some(path) = self.overflow_path(root, &item) else {
                        continue;
                    };
                    budget -= 2048;
                    let mut index = details::Index::default();
                    let guard = details::CheckpointGuard {
                        directory: self.checkpoints.directory().to_path_buf(),
                        source_key: item.source_key.clone(),
                        signature: item.checkpoint_signature.clone(),
                    };
                    if !index.add_projection(root, path, &item.anchor, &item.projection, Some(guard)) {
                        continue;
                    }
                    let Some(file) = index.select(id) else {
                        self.overflow_workdirs.remove(&item.source_key);
                        continue;
                    };
                    let Some(signature) = file.source_signature() else {
                        self.overflow_workdirs.remove(&item.source_key);
                        continue;
                    };
                    budget -= LINE_LIMIT;
                    let Some(cwd) = file.workdir(position) else {
                        continue;
                    };
                    if !scan.winners.contains_key(id) {
                            scan.bytes += 2 * id.len() + cwd.len() + 256;
                        } else {
                            scan.bytes = scan.bytes.saturating_sub(scan.metadata.get(id).and_then(|entry| entry["_cwd"].as_str()).map_or(0, str::len)) + cwd.len();
                        }
                    scan.winners.insert(id.clone(), updated);
                    let cache_cost = item.source_key.len() + item.checkpoint_signature.len() + cwd.len() + 256;
                    if cached_bytes.saturating_add(cache_cost) > cache_limit {
                        scan.metadata
                            .entry(id.clone())
                            .or_insert_with(|| json!({"thread_id":id}))["_cwd"] = json!(cwd);
                        continue;
                    }
                    cached_bytes += cache_cost;
                    self.overflow_workdirs.insert(
                        item.source_key.clone(),
                        CachedOverflowWorkdir {
                            checkpoint_signature: item.checkpoint_signature.clone(),
                            source_signature: signature,
                            cwd: cwd.clone(),
                        },
                    );
                    scan.metadata
                        .entry(id.clone())
                        .or_insert_with(|| json!({"thread_id":id}))["_cwd"] = json!(cwd);
                }

            }
            if !pending && !scan.incomplete { scan.validating = true; }
        }
        if scan.validating && !scan.incomplete {
            use std::ops::Bound::{Excluded, Unbounded};
            for (key, signature) in scan.retained.range((Excluded(scan.validated.clone()), Unbounded)).take(SOURCE_ROUND_LIMIT.saturating_sub(sources)) {
                if store.checkpoint_signature(key).ok().flatten().as_deref() != Some(signature.as_str())
                    || !membership.is_verified(key).unwrap_or(false) {
                    scan.incomplete = true;
                    break;
                }
                scan.validated = key.clone();
            }
            pending = !scan.incomplete && scan.retained.last_key_value().is_some_and(|(key, _)| key != &scan.validated);
        }
        if pending {
            self.overflow_workdir_scan = Some(scan);
            return true;
        }
        if !scan.incomplete {
            self.overflow_workdirs.retain(|key, _| scan.retained.contains_key(key));
            for (id, entry) in scan.metadata {
                if !metadata.get(&id).and_then(|value| value.get("_cwd")).and_then(Value::as_str).is_some_and(|cwd| !cwd.is_empty()) {
                    metadata.entry(id.clone()).or_insert_with(|| json!({"thread_id":id}))["_cwd"] = entry["_cwd"].clone();
                }
            }
        }
        false
    }

    fn selected_detail_index(&self, root: &Path, selection: &CallSelection) -> Option<(details::Index, BTreeSet<String>)> {
        self.selected_detail_index_with_limit(root, selection, 16 * 1024 * 1024)
    }

    fn selected_detail_index_with_limit(&self, root: &Path, selection: &CallSelection, stage_limit: usize) -> Option<(details::Index, BTreeSet<String>)> {
        if !selection.complete || self.discovery.is_some() || !self.source_membership_complete { return None; }
        let membership = self.source_membership.as_ref()?;
        let epoch: i64 = membership.db.query_row("SELECT value FROM selection_epoch", [], |row| row.get(0)).ok()?;
        let mut hash = Sha256::new();
        hash.update(membership.identity.to_le_bytes());
        hash.update(epoch.to_le_bytes());
        hash.update(stage_limit.to_le_bytes());
        hash.update(membership.revision.to_le_bytes());
        hash.update(serde_json::to_vec(&selection.calls).ok()?);
        for (path, state) in &self.files {
            hash.update(serde_json::to_vec(&(path, &state.anchor, &state.checkpoint_signature)).ok()?);
            for (id, call) in &state.projection.calls {
                hash.update(serde_json::to_vec(&(id, call.tool_name(), &call.request_position, &call.response_position)).ok()?);
            }
        }
        // Workdir readiness can change independently of selected call positions.
        hash.update(serde_json::to_vec(&self.overflow_thread_workdirs).ok()?);
        hash.update([self.applied_command_switches.worktrees as u8]);
        let mut metadata_workdirs: Vec<_> = self.thread_metadata.iter().filter_map(|(id, entry)| entry["_cwd"].as_str().map(|cwd| (id, cwd))).collect();
        metadata_workdirs.sort_unstable();
        hash.update(serde_json::to_vec(&metadata_workdirs).ok()?);
        let input_hash = hash.finalize().into();
        let mut cached = self.selected_detail_scan.lock().unwrap_or_else(|error| error.into_inner());
        if cached.as_ref().is_none_or(|scan| scan.input_hash != input_hash || !scan.valid) {
            // Drop the old reader before any fallible staging work. A new
            // oversized active stage must not leave an obsolete pending scan.
            *cached = None;
            let mut index = details::Index::default();
            let mut staged_bytes = 0usize;
            for (path, state) in &self.files {
                let Some(anchor) = state.anchor.as_ref() else { continue };
                let selected = checkpoint::key(root, path).and_then(|key| selection.calls.get(&key));
                let empty = BTreeSet::new();
                let selected = Some(selected.unwrap_or(&empty));
                staged_bytes = staged_bytes.saturating_add(details::Index::projection_bytes(root, path, &state.projection, selected));
                if staged_bytes > stage_limit { return None; }
                index.add_selected_projection(root, path, anchor, &state.projection, None, selected);
            }
            *cached = Some(SelectedDetailScan { input_hash, after: String::new(), reader: None, index,
                readiness: BTreeMap::new(), signatures: BTreeMap::new(), staged_bytes, validating: false, finished: false, valid: true });
        }
        let scan = cached.as_mut()?;
        let mut bytes = 0usize;
        let mut pages = 0usize;
        while !scan.finished && !scan.validating && pages < 16 && bytes < 16 * 1024 * 1024 {
            if scan.reader.is_none() {
                let page = match membership.materialized_page(&scan.after, 1) {
                    Ok(page) => page,
                    Err(_) => { scan.valid = false; scan.finished = true; break; }
                };
                let Some((key, signature)) = page.first() else { scan.validating = true; scan.after.clear(); break };
                // Reserve source bookkeeping before retaining it; empty selections
                // still carry cursor metadata but cannot grow the stage without bound.
                let reserve = key.len().saturating_add(signature.len()).saturating_add(512);
                if scan.signatures.len() >= WALK_LIMIT || reserve > stage_limit.saturating_sub(scan.staged_bytes) {
                    scan.valid = false; scan.finished = true; break;
                }
                scan.staged_bytes += reserve;
                let ids = selection.calls.get(key).into_iter().flatten().cloned().collect();
                let reader = self.checkpoints.snapshot_reader().ok().flatten()
                    .and_then(|store| store.into_selected_checkpoint_reader(&lam_history_store::ExpectedCursor { source_key: key, signature: Some(signature) }, ids).ok().flatten());
                let Some(reader) = reader else { scan.valid = false; scan.finished = true; break };
                scan.signatures.insert(key.clone(), signature.clone());
                scan.reader = Some(reader);
            }
            let reader = scan.reader.as_mut().expect("current source");
            let page = match reader.next_page() {
                Ok(Some(page)) => page,
                _ => { scan.valid = false; scan.finished = true; scan.reader = None; break; }
            };
            pages += 1;
            bytes += page.cursor.source_key.len() + page.cursor.signature.len() + page.cursor.payload.len()
                + page.calls.iter().map(|call| call.source_key.len() + call.call_id.len() + call.payload.len() + call.event_time.as_ref().map_or(0, String::len)).sum::<usize>();
            let complete = reader.is_complete();
            let Some(item) = checkpoint::snapshot_projection(page, false) else {
                scan.valid = false; scan.finished = true; scan.reader = None; break;
            };
            let Some(id) = item.projection.id.as_ref() else {
                if complete { scan.after = item.source_key; scan.reader = None; }
                continue;
            };
            let Some(path) = self.overflow_path(root, &item) else {
                scan.valid = false; scan.finished = true; scan.reader = None; break;
            };
            // Deliberately overcount repeated source fields on later pages. This
            // bounds both the retained index and the final publication clone.
            let reserve = details::Index::projection_bytes(root, path, &item.projection, None)
                .saturating_add(self.checkpoints.directory().as_os_str().len())
                .saturating_add(item.source_key.len()).saturating_add(item.checkpoint_signature.len());
            if reserve > stage_limit.saturating_sub(scan.staged_bytes) {
                scan.valid = false; scan.finished = true; scan.reader = None; break;
            }
            scan.staged_bytes += reserve;
            let guard = details::CheckpointGuard { directory: self.checkpoints.directory().to_path_buf(),
                source_key: item.source_key.clone(), signature: item.checkpoint_signature.clone() };
            let added = self.overflow_path(root, &item).is_some_and(|path| {
                scan.index.add_projection(root, path, &item.anchor, &item.projection, Some(guard))
            });
            if !added { scan.valid = false; scan.finished = true; scan.reader = None; break; }
            if item.source_details_pending {
                let ready = !item.workdir_pending || !self.applied_command_switches.worktrees
                    || self.overflow_thread_workdirs.contains_key(id)
                    || self.thread_metadata.get(id).and_then(|entry| entry["_cwd"].as_str()).is_some_and(|cwd| !cwd.is_empty());
                scan.readiness.entry(id.clone()).and_modify(|current| *current &= ready).or_insert(ready);
            }
            if complete {
                scan.after = item.source_key;
                scan.reader = None;
            }
        }
        // Earlier source snapshots may have finished several rounds ago. Recheck
        // signatures in bounded batches before publishing their compact positions.
        if scan.validating && !scan.finished {
            let Some(store) = self.checkpoints.snapshot_reader().ok().flatten() else {
                scan.valid = false; scan.finished = true; return None;
            };
            let batch: Vec<_> = scan.signatures.range((std::ops::Bound::Excluded(scan.after.clone()), std::ops::Bound::Unbounded))
                .take(64).map(|(key, signature)| (key.clone(), signature.clone())).collect();
            for (key, signature) in &batch {
                if store.checkpoint_signature(key).ok().flatten().as_ref() != Some(signature) {
                    scan.valid = false; scan.finished = true; return None;
                }
                scan.after = key.clone();
            }
            if batch.len() < 64 { scan.finished = true; }
        }
        if !scan.finished || !scan.valid { return None; }
        Some((scan.index.clone(), scan.readiness.iter().filter_map(|(id, ready)| ready.then_some(id.clone())).collect()))
    }

    fn detail_index(&self, root: &Path) -> (details::Index, BTreeSet<String>) {
        let active_calls = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        let selection = (self.discovered_file_count() > self.files.len())
            .then(|| self.select_recent_calls(CALL_LIMIT, 100_000)).flatten()
            .filter(|selection| selection.complete);
        if let Some(selection) = &selection {
            if let Some(index) = self.selected_detail_index(root, selection) { return index; }
            // Do not restart the legacy payload scan while the resumable stage
            // is pending or failed. Overflow details remain explicitly unready.
            return (details::Index::build(root, &self.files), BTreeSet::new());
        } else {
            *self.selected_detail_scan.lock().unwrap_or_else(|error| error.into_inner()) = None;
        }
        let mut index = details::Index::build(root, &self.files);
        let overflow = self.overflow_projections(active_calls).0;
        let mut readiness = BTreeMap::<String, bool>::new();
        for item in &overflow {
            let Some(id) = item.projection.id.as_ref() else {
                continue;
            };
            let path = self.overflow_path(root, item);
            let guard = details::CheckpointGuard {
                directory: self.checkpoints.directory().to_path_buf(),
                source_key: item.source_key.clone(),
                signature: item.checkpoint_signature.clone(),
            };
            let added = path.is_some_and(|path| {
                index.add_projection(root, path, &item.anchor, &item.projection, Some(guard))
            });
            if item.source_details_pending {
                let workdir_ready = !item.workdir_pending
                    || !self.applied_command_switches.worktrees
                    || self.overflow_thread_workdirs.contains_key(id)
                    || self
                        .thread_metadata
                        .get(id)
                        .and_then(|entry| entry.get("_cwd"))
                        .and_then(Value::as_str)
                        .is_some_and(|cwd| !cwd.is_empty());
                let ready = added && workdir_ready && !item.calls_incomplete;
                readiness
                    .entry(id.clone())
                    .and_modify(|current| *current &= ready)
                    .or_insert(ready);
            }
        }
        (
            index,
            readiness
                .into_iter()
                .filter_map(|(id, ready)| ready.then_some(id))
                .collect(),
        )
    }

    fn overflow_projections(
        &self,
        active_calls: usize,
    ) -> (Vec<checkpoint::SnapshotProjection>, bool) {
        const INPUT_LIMIT: usize = 16 * 1024 * 1024;
        self.overflow_projections_with_limit(active_calls, INPUT_LIMIT)
    }

    fn selected_overflow_projections(&self, selection: &CallSelection) -> Option<Vec<checkpoint::SnapshotProjection>> {
        const INPUT_LIMIT: usize = 16 * 1024 * 1024;
        let index = self.source_membership.as_ref()?;
        let mut store = self.checkpoints.snapshot_reader().ok()??;
        let mut after = String::new();
        let mut bytes = 0usize;
        let mut result = Vec::new();
        loop {
            let page = index.materialized_page(&after, 16).ok()?;
            if page.is_empty() { break; }
            for (key, signature) in &page {
                let ids: Vec<_> = selection.calls.get(key).into_iter().flatten().cloned().collect();
                let expected = lam_history_store::ExpectedCursor { source_key:key, signature:Some(signature) };
                let checkpoint = store.selected_checkpoint(&expected, &ids).ok()??;
                let size = checkpoint.cursor.payload.len().saturating_add(checkpoint.cursor.signature.len())
                    .saturating_add(checkpoint.calls.iter().map(|call| call.payload.len().saturating_add(call.call_id.len())).sum::<usize>());
                bytes = bytes.checked_add(size)?;
                if bytes > INPUT_LIMIT { return None; }
                result.push(checkpoint::snapshot_projection(checkpoint, false)?);
            }
            after = page.last()?.0.clone();
        }
        Some(result)
    }

    fn overflow_projections_with_limit(
        &self,
        active_calls: usize,
        input_limit: usize,
    ) -> (Vec<checkpoint::SnapshotProjection>, bool) {
        let outside = self
            .discovered_file_count()
            .saturating_sub(self.files.len());
        if outside == 0 {
            return (Vec::new(), false);
        }
        let Some(index) = self
            .source_membership
            .as_ref()
            .filter(|_| self.discovery.is_none() && self.source_membership_complete)
        else {
            return (Vec::new(), true);
        };
        let Ok(Some(mut store)) = self.checkpoints.snapshot_reader() else {
            return (Vec::new(), true);
        };
        let mut incomplete = index.unverified_count().map_or(true, |count| count > 0);
        let mut remaining = CALL_LIMIT.saturating_sub(active_calls);
        let mut bytes = 0usize;
        let mut after = String::new();
        let mut limit = 16usize;
        let mut result = Vec::new();
        loop {
            let page = match index.materialized_page(&after, limit) {
                Ok(page) => page,
                Err(_) => {
                    incomplete = true;
                    break;
                }
            };
            if page.is_empty() {
                break;
            }
            let expected: Vec<_> = page
                .iter()
                .map(|(key, signature)| lam_history_store::ExpectedCursor {
                    source_key: key,
                    signature: Some(signature),
                })
                .collect();
            let (checkpoints, excluded) =
                match store.matching_checkpoints_with_partials(&expected, remaining as u32) {
                    Ok(batch) => batch,
                    Err(_) if limit > 1 => {
                        limit = 1;
                        continue;
                    }
                    Err(_) => {
                        incomplete = true;
                        break;
                    }
                };
            incomplete |= !excluded.is_empty();
            for (mut checkpoint, mut calls_incomplete) in checkpoints {
                incomplete |= calls_incomplete;
                let header_size = checkpoint
                    .cursor
                    .payload
                    .len()
                    .saturating_add(checkpoint.cursor.signature.len());
                let mut size = header_size.saturating_add(
                        checkpoint
                            .calls
                            .iter()
                            .map(|call| call.payload.len().saturating_add(call.call_id.len()))
                            .sum::<usize>(),
                    );
                if bytes.saturating_add(size) > input_limit {
                    incomplete = true;
                    if bytes.saturating_add(header_size) > input_limit {
                        continue;
                    }
                    checkpoint.calls.clear();
                    calls_incomplete = true;
                    size = header_size;
                }
                bytes += size;
                remaining = remaining.saturating_sub(checkpoint.calls.len());
                if let Some(projection) = checkpoint::snapshot_projection(checkpoint, calls_incomplete) {
                    result.push(projection);
                } else {
                    incomplete = true;
                }
            }
            after = page.last().expect("nonempty page").0.clone();
        }
        if result.len() < outside {
            incomplete = true;
        }
        (result, incomplete)
    }

    fn snapshots(&self, health: &str, watch_health: &str, version: u64) -> Snapshots {
        let monitor_events = self
            .journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events();
        let debug = self
            .debug
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot();
        let active_calls = self
            .files
            .values()
            .map(|state| state.projection.calls.len())
            .sum();
        let selection = (health != "disabled" && self.discovered_file_count() > self.files.len())
            .then(|| self.select_recent_calls(CALL_LIMIT, 100_000)).flatten()
            .filter(|selection| selection.complete);
        let selected_overflow = selection.as_ref().and_then(|selection| self.selected_overflow_projections(selection));
        let selection = selection.filter(|_| selected_overflow.is_some());
        let (overflow, overflow_incomplete) = if health == "disabled" {
            (Vec::new(), false)
        } else if let Some(overflow) = selected_overflow {
            (overflow, false)
        } else {
            self.overflow_projections(active_calls)
        };
        // Source-backed details and complete metadata for overflow projections are not yet assembled.
        let overflow_incomplete = overflow_incomplete || !overflow.is_empty();
        let overflow_details_pending: BTreeSet<_> = overflow
            .iter()
            .filter(|item| item.source_details_pending)
            .filter_map(|item| item.projection.id.as_deref())
            .filter(|id| !self.overflow_details_ready.contains(*id))
            .collect();
        let overflow_calls_incomplete: BTreeSet<_> = overflow.iter()
            .filter(|item| item.calls_incomplete)
            .filter_map(|item| item.projection.id.as_deref())
            .collect();
        let reference = self.updated.unwrap_or(self.started);
        let cutoffs = HOURS.map(|hours| hours.map(|hours| reference - Duration::hours(hours)));
        let mut threads = BTreeMap::<&str, (Projection, bool)>::new();
        for (path, state) in self.files.iter().filter(|_| health != "disabled") {
            if let Some(id) = state.projection.id.as_deref() {
                let (projection, partial) = threads
                    .entry(id)
                    .or_insert_with(|| (Projection::for_id(Some(id.to_owned())), false));
                if let Some(selection) = &selection {
                    let key = self.session_root.as_deref().and_then(|root| checkpoint::key(root, path));
                    let selected = key.as_ref().and_then(|key| selection.calls.get(key));
                    let empty = BTreeSet::new();
                    projection.merge_snapshot(&state.projection, Some(selected.unwrap_or(&empty)), &cutoffs);
                } else {
                    projection.merge_snapshot(&state.projection, None, &cutoffs);
                }
                *partial |= state.pending() || state.discard;
            }
        }
        for item in &overflow {
            if let Some(id) = item.projection.id.as_deref() {
                let (projection, partial) = threads
                    .entry(id)
                    .or_insert_with(|| (Projection::for_id(Some(id.to_owned())), false));
                projection.merge_snapshot(&item.projection, None, &cutoffs);
                *partial |= item.partial;
            }
        }
        let pending_status: BTreeSet<_> = self
            .files
            .values()
            .filter(|state| health != "disabled" && state.pending())
            .filter_map(|state| state.projection.id.as_deref())
            .collect();
        let allowance = self
            .files
            .values()
            .filter_map(|state| state.projection.allowance.as_ref())
            .chain(overflow.iter().filter_map(|item| item.projection.allowance.as_ref()))
            .max_by_key(|value| value["updated_at"].as_str().unwrap_or_default())
            .cloned()
            .filter(|_| self.applied_command_switches.usage && health != "disabled");
        let source_index_ready = self.discovery.is_none()
            && self.source_membership_complete
            && self
                .source_membership
                .as_ref()
                .is_some_and(|index| index.verified_page("", 1).is_ok());
        let source_index_unverified = if source_index_ready {
            self.source_membership
                .as_ref()
                .and_then(|index| index.unverified_count().ok())
        } else {
            None
        };
        let source_index_materialized = if source_index_ready {
            self.source_membership
                .as_ref()
                .and_then(|index| index.materialized_count().ok())
        } else {
            None
        };
        let command_calls = commands::Calls::new(&threads);
        let recent = threads.iter().flat_map(|(thread, (row, _))| {
            row.calls
                .iter()
                .flat_map(move |(id, call)| call.activities(thread, id))
        });
        let observed_activity = lam_history_store::combine_observations(
            &self.activity_history,
            recent
                .chain(self.core.sql.iter().cloned())
                .chain(self.desktop.sql.iter().cloned()),
        );
        let tracking = *self
            .tracking
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let (sources, customizations) = {
            let sources = self
                .mcp_sources
                .read()
                .unwrap_or_else(|error| error.into_inner());
            let customizations = self
                .customizations
                .read()
                .unwrap_or_else(|error| error.into_inner());
            (sources.clone(), customizations.clone())
        };
        let switches = *self
            .mcp_switches
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let (mut mcp_events, sql_events): (Vec<_>, Vec<_>) = observed_activity
            .into_iter()
            .partition(|event| event.get("server").is_some());
        let observed_servers: BTreeSet<String> = mcp_events
            .iter()
            .filter_map(|event| event["server"].as_str().map(str::to_owned))
            .collect();
        mcp_events.retain(|event| {
            event["server"]
                .as_str()
                .is_none_or(|server| switches.allows(server) && sources.get(server) != Some(&false))
        });
        for event in &mut mcp_events {
            if let Some(category) = event["server"]
                .as_str()
                .and_then(|server| customizations.mcp_categories.get(server))
            {
                event["category"] = json!(category);
            }
        }
        let sql_events = sqlite::identified(sql_events);
        let api_events = model_api::merge(&self.core.api_events, &self.desktop.api_events);
        let mut all_mcp = mcp::summary(&mcp_events, None, self.activity_history_health);
        if let Some(rows) = all_mcp["servers"].as_array_mut() {
            for server in &observed_servers {
                if !switches.allows(server) || sources.get(server) == Some(&false) {
                    rows.push(mcp::disabled_server(server));
                }
            }
        }
        if let Some(rows) = all_mcp["servers"].as_array_mut() {
            for row in rows.iter_mut() {
                if let Some(name) = row["server"].as_str().map(str::to_owned) {
                    let enabled = sources.get(&name) != Some(&false);
                    row["enabled"] = json!(enabled);
                    if !enabled {
                        row["connection"]["state"] = json!("paused");
                    } else if row["connection"]["state"] == "paused" {
                        row["connection"]["state"] = json!("unconfirmed");
                    }
                    if self.mcp_catalog.contains_key(&name) {
                        row["origin"] = json!("configured");
                    }
                }
            }
            for (name, description) in &self.mcp_catalog {
                if !rows.iter().any(|row| row["server"] == *name) {
                    rows.push(mcp::configured_server(
                        name,
                        sources.get(name) != Some(&false),
                    ));
                }
                if let Some(row) = rows.iter_mut().find(|row| row["server"] == *name) {
                    if let Some(description) = description {
                        row["description"] = json!(description.text);
                        row["description_source"] = json!(description.source);
                        if let Some(path) = &description.path {
                            row["description_file"] = json!(path);
                        }
                    }
                    row["default_description"] = json!(description
                        .as_ref()
                        .map(|value| value.text.as_str())
                        .unwrap_or(""));
                }
            }
            for row in rows.iter_mut() {
                let Some(name) = row["server"].as_str().map(str::to_owned) else {
                    continue;
                };
                let default_description = row["description"].as_str().unwrap_or("").to_owned();
                row["default_description"] = json!(default_description);
                let tool_names = row["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                row["default_category"] = json!(lam_history_store::mcp::category(
                    &name,
                    &format!("{tool_names} {default_description}")
                ));
                row["category"] = customizations.mcp_categories.get(&name).map_or_else(
                    || row["default_category"].clone(),
                    |category| json!(category),
                );
                if let Some(description) = customizations.mcp_descriptions.get(&name) {
                    row["description"] = json!(description);
                    row["description_source"] = json!("custom");
                }
                if let Some(tags) = customizations.mcp_tags.get(&name) {
                    row["tags"] = json!(tags);
                }
            }
            rows.sort_by(|a, b| a["server"].as_str().cmp(&b["server"].as_str()));
        }
        if self.applied_errors_enabled && health != "disabled" {
            let diagnostics = threads.iter().flat_map(|(thread, (projection, _))| {
                projection.calls.iter()
                    .filter_map(move |(id, call)| call.failure_event(thread, id))
                    .map(std::borrow::Cow::Owned)
            }).chain(self.core.errors.iter().chain(&self.desktop.errors)
                .map(std::borrow::Cow::Borrowed));
            mcp::apply_diagnostics(&mut all_mcp, diagnostics);
        }
        all_mcp["configuration"] = json!({"location":self.catalog_home.join("config.toml").to_string_lossy(),"configured_sources":self.mcp_catalog.len()});
        let mut session_errors: Vec<_> = self.files.iter()
            .filter(|(_, state)| state.projection.id.is_some())
            .flat_map(|(path, state)| state.error_events(path, &state.projection.errors))
            .collect();
        if let Some(root) = &self.session_root {
            for item in &overflow {
                let Some(path) = self.overflow_path(root, item) else { continue };
                session_errors.extend(Session::projection_error_events(&item.projection, path, &item.projection.errors));
            }
        }
        let errors = self.error_windows(health, reference, &threads, &session_errors);
        let activity = json!({
            "paused":false, "health":health, "last_activity_at":threads.values().filter_map(|(row,_)| row.updated()).max().map(stamp),
            "update_mode":"source_events","watch_health":watch_health,"code_revision":self.backend_revision
        });
        let tool_events_enabled = self.tool_events_enabled.load(Ordering::Acquire);
        let files_enabled = self.files_enabled.load(Ordering::Acquire);
        let errors_enabled = self.applied_errors_enabled;
        let connection = connection::status(
            threads
                .values()
                .filter_map(|(row, _)| row.token_updated_at()),
            session_errors
                .iter().filter(|_| errors_enabled)
                .chain(self.core.errors.iter().filter(|_| errors_enabled))
                .chain(self.desktop.errors.iter().filter(|_| errors_enabled)),
            health != "disabled",
            reference,
        );
        let sqlite_enabled = self.applied_sqlite_enabled;
        let model_api_enabled = self.applied_model_api_enabled && health != "disabled";
        let command_switches = self.applied_command_switches;
        let account_settings = self.account.settings();
        let account = self.account.result();
        let cloud_settings = self.cloud.settings();
        let cloud = self.cloud.result();
        let public_settings = self.public.each_ref().map(|source| source.settings());
        let public = self.public.each_ref().map(|source| source.result());
        let platform_settings = self.platform.each_ref().map(|source| source.settings());
        let platform = self.platform.each_ref().map(|source| source.result());
        let diagnostic_locations: Vec<_> = self.desktop.source_locations()
            .chain(self.core.source_location())
            .map(Path::to_path_buf)
            .collect();
        let journal_source = self.journal.lock().unwrap_or_else(|error| error.into_inner()).source();
        let windows: [Bytes; 4] = std::array::from_fn(|index| {
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
                        tool_events_enabled,
                        files_enabled,
                        command_switches.jev_calls,
                    );
                    apply_thread_metadata(
                        &mut value,
                        row.id
                            .as_deref()
                            .and_then(|id| self.thread_metadata.get(id)),
                        false,
                    );
                    let thread_name = value["thread_name"].clone();
                    for key in ["file_changes", "file_reads"] {
                        if let Some(events) = value[key].as_array_mut() {
                            for event in events {
                                event["thread_name"] = thread_name.clone();
                            }
                        }
                    }
                    value
                })
                .collect::<Vec<Value>>();
            for (id, metadata) in &self.thread_metadata {
                if threads.contains_key(id.as_str()) {
                    continue;
                }
                let projection = Projection::for_id(Some(id.clone()));
                let mut value = projection.row(
                    true,
                    boundary,
                    reference,
                    &mut totals,
                    &mut nested_totals,
                    tool_events_enabled,
                    files_enabled,
                    command_switches.jev_calls,
                );
                apply_thread_metadata(&mut value, Some(metadata), true);
                rows.push(value);
            }
            for row in &mut rows {
                if row["thread_id"].as_str().is_some_and(|id| overflow_calls_incomplete.contains(id)) {
                    row["tool_calls"] = Value::Null;
                }
                row["status_backfill_pending"] = json!(row["thread_id"]
                    .as_str()
                    .is_some_and(|id| pending_status.contains(id)));
                if row["thread_id"]
                    .as_str()
                    .is_some_and(|id| overflow_details_pending.contains(id))
                {
                    row["source_details_pending"] = json!(true);
                }
            }
            rows.sort_by(|left, right| {
                right["updated_at"]
                    .as_str()
                    .cmp(&left["updated_at"].as_str())
            });
            let mut file_summary = project::FileSummary::new(1000);
            for (thread, _) in threads.values() {
                file_summary.observe(thread, boundary, files_enabled, None);
            }
            let (mut file_events, file_total, file_counts) = file_summary.finish();
            let thread_names: HashMap<_, _> = rows
                .iter()
                .filter_map(|row| {
                    Some((
                        row["thread_id"].as_str()?.to_owned(),
                        row["thread_name"].clone(),
                    ))
                })
                .collect();
            for event in &mut file_events {
                if let Some(name) = event["thread_id"]
                    .as_str()
                    .and_then(|id| thread_names.get(id))
                {
                    event["thread_name"] = name.clone();
                }
            }
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
            let (mut git, mut checks) = command_calls.window(boundary);
            if !command_switches.git {
                git = json!({"events":[],"operations":{},"total":0});
            }
            if !command_switches.checks {
                checks.clear();
            }
            let (skills, skill_total, skill_counts) = project::skill_summary(
                &threads, &self.skill_history, &thread_names, boundary, command_switches.skills,
            );
            let outside_working_set = self
                .discovered_file_count()
                .saturating_sub(self.files.len());
            let pending = if outside_working_set == 0 {
                Some(self.files.values().filter(|state| state.pending()).count())
            } else {
                None
            };
            let initial_pending = (outside_working_set == 0).then(|| {
                self.files
                    .values()
                    .filter(|state| state.identity.is_none())
                    .count()
            });
            let mut mcp = boundary.map_or_else(
                || all_mcp.clone(),
                |cutoff| {
                    mcp::scoped_summary(&mcp_events, cutoff, self.activity_history_health, &all_mcp)
                },
            );
            let mut sqlite = sqlite::window(
                if sqlite_enabled { &sql_events } else { &[] },
                boundary,
                self.activity_history_health,
            );
            if let Some(view) = &self.activity_view {
                if switches.mcp {
                    mcp["history_aggregate"] =
                        history_view::render(&view.mcp[index], WINDOWS[index]);
                } else {
                    mcp["history_aggregate"] = history_view::render(
                        &lam_history_store::Aggregate::default(),
                        WINDOWS[index],
                    );
                }
                sqlite["aggregate"] = if sqlite_enabled {
                    history_view::render(&view.sql[index], WINDOWS[index])
                } else {
                    history_view::render(&lam_history_store::Aggregate::default(), WINDOWS[index])
                };
            }
            let mut core_source = self.core.source();
            if !model_api_enabled {
                core_source["health"] = json!("disabled");
            }
            let mut model_api =
                model_api::summary(&api_events, boundary, model_api_enabled, core_source);
            if let Some(sources) = model_api["sources"].as_array_mut() {
                let mut desktop_source = self.desktop.source.clone();
                if !model_api_enabled {
                    desktop_source["health"] = json!("disabled");
                }
                sources.insert(0, desktop_source);
            }
            let settings = json!({"debug_mode":debug["enabled"],"observations":{"metadata":self.metadata_enabled,"logs":self.logs_enabled,"tool_events":tool_events_enabled,"files":files_enabled}});
            let mut snapshot = json!({
                "version":1,"label":"本機觀察統計","started_at":stamp(self.started),"updated_at":self.updated.map(stamp),
                "activity":activity,
                "backend_revision":self.backend_revision,
                "frontend_revision":self.frontend_revision,
                "monitor":{"version":env!("CARGO_PKG_VERSION"),"pid":std::process::id(),"health":if self.refreshes == 0 {"starting"} else {"ok"},"refreshes":self.refreshes,"uptime_seconds":self.started_instant.elapsed().as_secs_f64().round() as u64},
                "model_api":model_api,
                "errors":errors[index].0,
                "mcp":mcp,
                "codex":{
                    "source":"codex","health":health,"started_at":stamp(self.started),"files":self.discovered_file_count(),
                    "bytes_read":self.read_bytes,"malformed_lines":self.malformed,
                    "threads":rows,"tools":totals,"nested_tools":nested_totals,"observed_tool_calls":observed,
                    "git":git,"checks":checks,
                    "schedules":self.schedules,
                    "metadata_sources":self.schedule_report.iter().collect::<Vec<_>>(),
                    "sqlite":sqlite,
                    "activity_scope":{"window":WINDOWS[index],"timestamp":"call_started_at","unknown_timestamp":"all_only",
                                      "latest_state":["tokens","model","status"]},
                    "read_state":{"file_limit":null,"file_count":self.discovered_file_count(),"pending_files":pending,
                                  "read_mode":"incremental_all_sources","read_limit":ROUND_LIMIT,"tail_bytes":CHUNK_LIMIT,
                                  "call_limit":CALL_LIMIT,"buffer_limit":BUFFER_LIMIT,"file_call_limit":FILE_CALL_LIMIT,
                                  "git_event_limit":commands::GIT_EVENT_LIMIT,"check_event_limit":commands::CHECK_EVENT_LIMIT,
                                  "checkpoint":self.checkpoints.report}
                }
            });
            if let Some(read_state) = snapshot["codex"]["read_state"].as_object_mut() {
                read_state.extend(json!({"initial_pending_files":initial_pending,"history_pending_files":0,"history_pending_bytes":0,"error_pending_files":0,"error_pending_bytes":0,"working_file_limit":FILE_LIMIT,"working_files":self.files.len(),"outside_working_set":outside_working_set,"discovery_pending":self.discovery.is_some(),"source_index_ready":source_index_ready,"source_index_unverified":source_index_unverified,"source_index_materialized":source_index_materialized,"overflow_snapshot_sources":overflow.len(),"overflow_snapshot_incomplete":overflow_incomplete,"overflow_details_pending":overflow_details_pending.len()}).as_object().expect("read state fields").clone());
            }
            if health == "waiting" && version == 0 {
                snapshot["initial_sections"] = json!(["catalog", "runtime"]);
                snapshot["health"] = json!("starting");
            }
            snapshot["codex"]["plugins"] = self.plugins.clone();
            if let Some(sources) = snapshot["codex"]["metadata_sources"].as_array_mut() {
                sources.extend(self.metadata_reports.iter().cloned());
            }
            if let Some(report) = &self.plugin_report {
                if let Some(sources) = snapshot["codex"]["metadata_sources"].as_array_mut() {
                    sources.push(report.clone());
                }
            }
            if let (Some(monitor), Some(runtime)) = (
                snapshot["monitor"].as_object_mut(),
                self.runtime.as_object(),
            ) {
                monitor.extend(runtime.clone());
            }
            let fonts = self.fonts.read().unwrap_or_else(|error| error.into_inner());
            let gpus = self.gpus.read().unwrap_or_else(|error| error.into_inner());
            if let Some(fonts) = fonts.as_ref() {
                snapshot["monitor"]["fonts"] = json!(fonts);
            }
            if let Some(gpus) = gpus.as_ref() {
                snapshot["monitor"]["gpus"] = json!(gpus);
            }
            let ready_at = self
                .device_completion
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .ready_at();
            snapshot["monitor"]["device_ready"] = json!(ready_at.is_some());
            snapshot["monitor"]["device_checked_at"] = json!(ready_at.map(stamp));
            snapshot["settings"] = settings;
            snapshot["settings"]["observations"]["codex"] = json!(health != "disabled");
            snapshot["default_settings"] = json!({
                "debug_mode":false,"activity_retention_days":90,"max_files":20,"track_all":false,
                "observations":{"usage":true,"codex_account":false,"codex_cloud":false,"codex":self.initial_codex,
                    "jev":true,"metadata":true,"git":true,"worktrees":true,"jev_calls":true,"skills":true,
                    "checks":true,"tool_events":true,"mcp":true,"web":true,"files":true,"errors":true,
                    "logs":true,"sqlite":true,"model_api":true,"public_openai":false,"public_codex":false,
                    "platform_models":false,"platform_usage":false,"platform_decisions":false,"platform_costs":false},
                "api_intervals":{"codex_account":300,"codex_cloud":300,"public_openai":300,"public_codex":300,
                    "platform_models":300,"platform_usage":300,"platform_decisions":300,"platform_costs":300},
                "mcp_sources":{},"mcp_categories":{},"tool_descriptions":{},"mcp_descriptions":{},"mcp_tags":{}
            });
            for (key, result) in ["public_openai", "public_codex"]
                .into_iter()
                .zip(public.iter())
            {
                if let Some(value) = result {
                    snapshot["api_queries"][key] = value.clone();
                }
            }
            snapshot["platform_credentials"] = platform::credentials();
            for (key, result) in [
                "platform_models",
                "platform_usage",
                "platform_decisions",
                "platform_costs",
            ]
            .into_iter()
            .zip(platform.iter())
            {
                if let Some(value) = result {
                    snapshot["api_queries"][key] = value.clone();
                }
            }
            if let Some(cloud) = &cloud {
                snapshot["api_queries"]["codex_cloud"] = cloud.clone();
            }
            snapshot["codex"]["usage"] = allowance.clone().unwrap_or(Value::Null);
            snapshot["codex"]["account"] =
                account.clone().unwrap_or_else(|| account::empty("waiting"));
            if let Some(account) = &account {
                snapshot["api_queries"]["codex_account"] = account.clone();
                if account["methods"]["account/rateLimits/read"]["health"] == "ok" {
                    if let Some(limits) = account["limits"].as_object() {
                        if let Some(bucket) = allowance
                            .as_ref()
                            .and_then(|local| local["limit_id"].as_str())
                            .and_then(|id| limits.get(id))
                            .or_else(|| limits.get("codex"))
                            .or_else(|| limits.values().next())
                        {
                            let windows: Vec<_> = ["primary", "secondary"]
                                .into_iter()
                                .filter_map(|key| {
                                    bucket.get(key).filter(|value| !value.is_null()).cloned()
                                })
                                .collect();
                            let plan = bucket
                                .get("plan_type")
                                .and_then(Value::as_str)
                                .filter(|value| !value.is_empty())
                                .or_else(|| {
                                    account["plan_type"]
                                        .as_str()
                                        .filter(|value| !value.is_empty())
                                })
                                .or_else(|| {
                                    allowance
                                        .as_ref()
                                        .and_then(|local| local["plan_type"].as_str())
                                });
                            snapshot["codex"]["usage"] = json!({"source":"codex_app_server","updated_at":account["updated_at"],"limit_id":bucket["limit_id"],"plan_type":plan,"credits":bucket.get("credits").filter(|value|!value.is_null()).unwrap_or(&account["credits"]),"limits":windows});
                        }
                    }
                }
            }
            let jev_enabled =
                command_switches.jev && switches.mcp && sources.get("jev") != Some(&false);
            snapshot["jev"] =
                jev::snapshot(&self.catalog_home, WINDOWS[index], reference, jev_enabled);
            let jev_available = self.mcp_catalog.contains_key("jev")
                || snapshot["jev"]["reader"]["locations"]
                    .as_array()
                    .is_some_and(|locations| !locations.is_empty())
                || snapshot["mcp"]["servers"]
                    .as_array()
                    .is_some_and(|servers| servers.iter().any(|row| row["server"] == "jev"));
            snapshot["availability"]["jev"] = json!(jev_available);
            snapshot["mcp"]["telemetry"] = if jev_available {
                let data = &snapshot["jev"];
                json!({"jev":{"enabled":data["enabled"],"enabled_at":data["enabled_at"],"health":data["health"],"scope":data["scope"],"summary":data["summary"],"recent":data["recent"],"series":data["series"],"window":data["window"],"reader":{"locations":data["reader"]["locations"],"record_limit":80,"series_limit":168,"checked_at":data["reader"]["checked_at"],"error_type":data["reader"]["error_type"]}}})
            } else {
                json!({})
            };
            if jev_available {
                let data = &snapshot["jev"];
                let fields: Vec<_> = data["summary"]
                    .as_object()
                    .map(|summary| summary.keys().cloned().collect())
                    .unwrap_or_default();
                let source = json!({"name":"jev telemetry","features":["mcp","errors","logs"],"scope":data["scope"],"locations":data["reader"]["locations"],"mode":"read_only","fields":fields,"limits":{"loaded_records":data["recent"].as_array().map_or(0,Vec::len),"series_points":data["series"].as_array().map_or(0,Vec::len),"record_limit":80,"series_limit":168},"readers":[],"health":data["health"],"window":data["window"],"checked_at":data["reader"]["checked_at"],"error_type":data["reader"]["error_type"],"display_window":WINDOWS[index],"watch_health":match self.jev_watch_health {Some(true)=>"ok",Some(false)=>"unavailable",None=>"starting"}});
                snapshot["sources"]["telemetry:jev"] = source;
                let data = &snapshot["jev"];
                if data["reader"]["locations"]
                    .as_array()
                    .is_some_and(|locations| !locations.is_empty())
                {
                    let flag = json!({"enabled":data["enabled"],"observed_at":data["enabled_at"],"health":null,"source":"monitor_config"});
                    let mut state = snapshot["mcp"]["recording_status"]["jev"]
                        .as_object()
                        .cloned()
                        .unwrap_or_default();
                    let flags = state
                        .entry("flags".to_string())
                        .or_insert_with(|| json!({}));
                    flags["config.recording_enabled"] = flag.clone();
                    if state.get("enabled").is_none_or(Value::is_null) {
                        state.insert("enabled".into(), flag["enabled"].clone());
                        state.insert("observed_at".into(), flag["observed_at"].clone());
                        state.insert("health".into(), Value::Null);
                        state.insert("source".into(), json!("monitor_config"));
                        state.insert("key".into(), json!("config.recording_enabled"));
                    }
                    state.insert("collector_health".into(), snapshot["jev"]["health"].clone());
                    snapshot["mcp"]["recording_status"]["jev"] = Value::Object(state);
                }
            }
            snapshot["codex"]["skills"] =
                json!({"events":skills,"counts":skill_counts,"total":skill_total});
            snapshot["codex"]["worktrees"] = if health == "disabled" {
                json!({"health":"disabled","items":[],"checked_at":null})
            } else {
                self.worktrees.snapshot()
            };
            snapshot["settings"]["max_files"] = json!(tracking.max_files);
            snapshot["settings"]["track_all"] = json!(tracking.track_all);
            if let Some(days) = self.known_retention_days {
                snapshot["settings"]["activity_retention_days"] = json!(days);
            }
            if health != "disabled" {
                snapshot["codex"]["max_files"] = json!(tracking.max_files);
                snapshot["codex"]["track_all"] = json!(tracking.track_all);
                snapshot["codex"]["scope"] = json!("incremental_all_sources");
            }
            snapshot["settings"]["mcp_sources"] = json!(sources);
            snapshot["settings"]["mcp_categories"] = json!(customizations.mcp_categories);
            snapshot["settings"]["mcp_descriptions"] = json!(customizations.mcp_descriptions);
            snapshot["settings"]["mcp_tags"] = json!(customizations.mcp_tags);
            snapshot["settings"]["tool_descriptions"] = json!(customizations.tool_descriptions);
            snapshot["settings"]["observations"]["mcp"] = json!(switches.mcp);
            snapshot["settings"]["observations"]["web"] = json!(switches.web);
            snapshot["settings"]["observations"]["errors"] = json!(errors_enabled);
            snapshot["settings"]["observations"]["sqlite"] = json!(sqlite_enabled);
            snapshot["settings"]["observations"]["model_api"] =
                json!(self.applied_model_api_enabled);
            snapshot["settings"]["observations"]["git"] = json!(command_switches.git);
            snapshot["settings"]["observations"]["checks"] = json!(command_switches.checks);
            snapshot["settings"]["observations"]["jev_calls"] = json!(command_switches.jev_calls);
            snapshot["settings"]["observations"]["jev"] = json!(command_switches.jev);
            snapshot["settings"]["observations"]["skills"] = json!(command_switches.skills);
            snapshot["settings"]["observations"]["worktrees"] = json!(command_switches.worktrees);
            snapshot["settings"]["observations"]["usage"] = json!(command_switches.usage);
            snapshot["settings"]["observations"]["codex_account"] = json!(account_settings.enabled);
            snapshot["settings"]["observations"]["codex_cloud"] = json!(cloud_settings.enabled);
            for (key, setting) in [
                "platform_models",
                "platform_usage",
                "platform_decisions",
                "platform_costs",
            ]
            .into_iter()
            .zip(platform_settings)
            {
                snapshot["settings"]["observations"][key] = json!(setting.enabled);
                snapshot["settings"]["api_intervals"][key] = json!(setting.interval);
            }
            for (key, setting) in ["public_openai", "public_codex"]
                .into_iter()
                .zip(public_settings)
            {
                snapshot["settings"]["observations"][key] = json!(setting.enabled);
                snapshot["settings"]["api_intervals"][key] = json!(setting.interval);
            }
            snapshot["settings"]["api_intervals"]["codex_cloud"] = json!(cloud_settings.interval);
            snapshot["settings"]["api_intervals"]["codex_account"] =
                json!(account_settings.interval);
            snapshot["codex"]["file_activity"] =
                json!({"events":file_events,"total":file_total,"operations":file_counts});
            snapshot["codex"]["dots"] = self.dots[index].clone();
            snapshot["codex"]["projects"] = json!(projects.into_values().collect::<Vec<_>>());
            snapshot["monitor"]["debug"] = debug.clone();
            let events: Vec<_> = monitor_events
                .iter()
                .filter(|event| {
                    boundary.is_none_or(|cutoff| {
                        event["timestamp"]
                            .as_str()
                            .and_then(lam_history_store::parse_utc_stamp)
                            .is_some_and(|timestamp| timestamp >= cutoff)
                    })
                })
                .collect();
            snapshot["monitor"]["events"] = json!(events);
            snapshot["monitor"]["event_limit"] = json!(200);
            snapshot["monitor"]["log_path"] = json!(self
                .catalog_home
                .join("monitoring/local-activity-monitor.jsonl")
                .to_string_lossy());
            snapshot["monitor"]["history_limit"] = json!(360);
            let history: Vec<_> = self.monitor_history.iter().filter(|sample| {
                boundary.is_none_or(|cutoff| {
                    sample["time"].as_str()
                        .and_then(lam_history_store::parse_utc_stamp)
                        .is_some_and(|time| time >= cutoff)
                })
            }).collect();
            snapshot["monitor"]["history"] = json!(history);
            snapshot["monitor"]["window"] = json!(WINDOWS[index]);
            snapshot["monitor"]["sample_scope"] = json!("retained_samples_in_selected_window");
            snapshot["monitor"]["unknown_timestamp"] = json!("all_only");
            if let Some(last) = self.monitor_history.back().and_then(Value::as_object) {
                for (key, value) in last {
                    if key != "time" {
                        snapshot["monitor"][key] = value.clone();
                    }
                }
            }
            if health == "disabled" {
                let account = snapshot["codex"]["account"].clone();
                let usage = snapshot["codex"]["usage"].clone();
                snapshot["codex"] = json!({"source":"codex","health":"disabled","threads":[],"tools":{},"account":account,"usage":usage});
            }
            snapshot["codex"]["connection"] = connection.clone();
            source_registry::augment(
                &mut snapshot,
                &self.catalog_home,
                &self.core.source(),
                &self.desktop.source,
                &diagnostic_locations,
                &self.runtime,
                &journal_source,
            );
            snapshot.to_string().into()
        });
        let logs = std::array::from_fn(|index| {
            let mut logs =
                serde_json::from_slice::<Value>(&errors[index].1).unwrap_or_else(|_| json!({}));
            if let Some(sources) = logs["sources"].as_array_mut() {
                let jev = serde_json::from_slice::<Value>(&windows[index])
                    .ok()
                    .map(|value| value["jev"].clone())
                    .unwrap_or(Value::Null);
                sources.push(json!({"source":"jev_telemetry","health":jev["health"],"files":[],"checked_at":jev["reader"]["checked_at"]}));
            }
            logs.to_string().into()
        });
        let mut error_details = BTreeMap::new();
        let call_errors = threads.iter().flat_map(|(thread, (projection, _))| {
            projection.calls.iter().filter_map(move |(id, call)| call.failure_event(thread, id))
        });
        for event in self.core.errors.iter().chain(self.desktop.errors.iter())
            .chain(self.core.logs.iter()).chain(self.desktop.logs.iter())
            .chain(self.error_history.iter()).chain(session_errors.iter()).chain(self.jev_errors.iter())
            .cloned().chain(call_errors)
        {
            if let Some(id) = event["content_id"].as_str().map(str::to_owned) {
                error_details.entry(id).or_insert(event);
            }
        }
        Snapshots {
            windows,
            logs,
            activity: activity.to_string().into(),
            error_details,
            traffic: Arc::clone(&self.traffic),
            memory: Arc::clone(&self.memory),
            collection: Arc::clone(&self.collection),
            process_usage: Arc::clone(&self.process_usage),
            codex_enabled: health != "disabled",
            metadata_enabled: self.metadata_enabled,
            tool_events_enabled,
            files_enabled,
            errors_enabled,
            sqlite_enabled,
            model_api_enabled: self.applied_model_api_enabled,
            command_switches,
            account_settings,
            cloud_settings,
            public_settings,
            platform_settings,
            worktree_details: self.worktrees.details(),
            mcp_sources: self.mcp_view_sources.clone(),
            mcp_switches: switches,
            customizations,
            retention_days: self.known_retention_days,
            tracking,
            logs_enabled: self.logs_enabled,
            debug_enabled: debug["enabled"].as_bool().unwrap_or(false),
            version,
            started_instant: self.started_instant,
            rendered: Mutex::new(Default::default()),
        }
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.set_phase("stopped");
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
    fn selected_details_bound_source_and_signature_work_across_rounds() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for n in 0..65 {
            let id = format!("00000000-0000-4000-8000-{n:012}");
            std::fs::write(root.join(format!("rollout-{id}.jsonl")), format!("{}\n",
                json!({"type":"session_meta","payload":{"id":id}})
            )).unwrap();
        }
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        let stopped = AtomicBool::new(false);
        for _ in 0..4 {
            if !collector.refresh(&root, &stopped, true, false, CommandSwitches::default()).1 { break; }
        }
        let store = lam_history_store::Store::open(&root.join("monitoring"), lam_history_store::Access::ReadOnly).unwrap().unwrap();
        let mut membership = SourceMembership::new().unwrap();
        for path in collector.files.keys() {
            let key = checkpoint::key(&root, path).unwrap();
            membership.insert(&[MembershipRecord {key:key.clone(),fingerprint:Some("fixture".into())}]).unwrap();
            membership.mark_verified(&[VerifiedMembership {checkpoint_signature:store.checkpoint_signature(&key).unwrap().unwrap(),
                key,fingerprint:"fixture".into(),materialized:true,call_headers:Some(Vec::new())}]).unwrap();
        }
        collector.source_membership = Some(membership);
        collector.source_membership_complete = true;
        collector.discovered_files = Some(65);
        collector.discovery = None;
        collector.files.clear();
        let selection = CallSelection {calls:BTreeMap::new(),complete:true};
        let mut previous = 0;
        for round in 1..=6 {
            let result = collector.selected_detail_index(&root, &selection);
            let scan = collector.selected_detail_scan.lock().unwrap();
            let scan = scan.as_ref().unwrap();
            assert!(scan.signatures.len() - previous <= 16);
            previous = scan.signatures.len();
            if round < 6 { assert!(result.is_none()); }
            else {
                let (index, _) = result.expect("source and validation cursors must reach the end");
                assert_eq!(index.files.len(), 65);
                assert!(scan.finished);
            }
        }
        // A deliberately small compact-stage budget rejects many-source
        // metadata without publishing a prefix or scheduling an endless retry.
        assert!(collector.selected_detail_index_with_limit(&root, &selection, 4096).is_none());
        {
            let scan = collector.selected_detail_scan.lock().unwrap();
            let scan = scan.as_ref().unwrap();
            assert!(scan.staged_bytes <= 4096);
            assert!(scan.finished && !scan.valid);
            assert!(scan.signatures.len() < 65);
            assert!(scan.reader.is_none());
        }
        assert!(!collector.recent_call_refresh_pending());
        // Cancelling overflow also drops its compact staging and read cursor.
        collector.discovered_files = Some(0);
        assert!(!collector.recent_call_refresh_pending());
        assert!(collector.selected_detail_scan.lock().unwrap().is_none());
    }

    #[test]
    fn selected_details_resume_beyond_payload_budget_without_retaining_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = root.join(format!("rollout-{id}.jsonl"));
        std::fs::write(&path, format!("{}\n{}\n",
            json!({"timestamp":"2026-11-13T01:00:00Z","type":"session_meta","payload":{"id":id}}),
            json!({"timestamp":"2026-11-13T01:00:01Z","type":"response_item","payload":{"type":"function_call","name":"fixture_tool","call_id":"call00","arguments":"{}"}})
        )).unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        let stopped = AtomicBool::new(false);
        for _ in 0..4 {
            let (_, pending, _) = collector.refresh(&root, &stopped, true, false, CommandSwitches::default());
            if !pending { break; }
        }
        let key = checkpoint::key(&root, &path).unwrap();
        let mut store = lam_history_store::Store::open(&root.join("monitoring"), lam_history_store::Access::ReadWrite).unwrap().unwrap();
        let original = store.checkpoint(&key, 10).unwrap().unwrap();
        assert_eq!(original.calls.len(), 1);
        let calls: Vec<_> = (0..40).map(|n| {
            let mut row = original.calls[0].clone();
            row.call_id = format!("call{n:02}");
            // Valid JSON padding tests the serialized payload budget independently
            // of the compact position index retained by the service.
            row.payload.push_str(&" ".repeat(1024 * 1024));
            row
        }).collect();
        store.save_checkpoints(std::slice::from_ref(&original.cursor), &calls, &[]).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let fingerprint = source_fingerprint(file_identity(&metadata), metadata.len(), metadata.modified().ok()).unwrap();
        let mut membership = SourceMembership::new().unwrap();
        membership.insert(&[MembershipRecord { key:key.clone(), fingerprint:Some(fingerprint.clone()) }]).unwrap();
        membership.mark_verified(&[VerifiedMembership { key:key.clone(), fingerprint, checkpoint_signature:original.cursor.signature.clone(),
            materialized:true, call_headers:Some(calls.iter().map(|row| lam_history_store::CallHeader {call_id:row.call_id.clone(),event_time:row.event_time.clone()}).collect()) }]).unwrap();
        collector.source_membership = Some(membership);
        collector.source_membership_complete = true;
        collector.discovered_files = Some(1);
        collector.discovery = None;
        collector.files.clear();
        let selection = CallSelection { calls: [(key.clone(), calls.iter().map(|row| row.call_id.clone()).collect())].into(), complete:true };
        assert!(collector.selected_overflow_projections(&selection).is_none());
        assert!(collector.selected_detail_index(&root, &selection).is_none());
        assert!(collector.recent_call_refresh_pending());
        {
            let scan = collector.selected_detail_scan.lock().unwrap();
            let scan = scan.as_ref().unwrap();
            assert!(scan.reader.is_some());
            assert!(scan.index.files[id][0].calls.len() < 40);
        }
        let (index, _) = collector.selected_detail_index(&root, &selection).expect("second round completes remaining calls and validation");
        assert_eq!(index.files[id].len(), 1, "pages coalesce into one source");
        assert_eq!(index.files[id][0].calls.len(), 40);
        assert!(index.files[id][0].source_signature().is_some());
        // A changed source signature rejects both cached detail reads and a
        // staged result whose earlier pages were already consumed.
        *collector.selected_detail_scan.lock().unwrap() = None;
        assert!(collector.selected_detail_index(&root, &selection).is_none());
        let mut changed = original.cursor.clone();
        changed.signature = "changed-after-first-page".into();
        store.save_checkpoints(&[changed], &[], &[]).unwrap();
        assert!(index.files[id][0].source_signature().is_none());
        assert!(collector.selected_detail_index(&root, &selection).is_none());
        assert!(!collector.selected_detail_scan.lock().unwrap().as_ref().unwrap().valid);
        // Start another pending scan, then invalidate its inputs with an active
        // projection too large for the replacement stage budget.
        store.save_checkpoints(std::slice::from_ref(&original.cursor), &calls, &[]).unwrap();
        assert!(collector.selected_detail_index(&root, &selection).is_none());
        assert!(collector.selected_detail_scan.lock().unwrap().as_ref().unwrap().reader.is_some());
        let restored = checkpoint::snapshot_projection(store.checkpoint(&key, 1).unwrap().unwrap(), false).unwrap();
        let mut active = Session::default();
        active.anchor = Some(restored.anchor);
        active.projection = restored.projection;
        collector.files.insert(path.clone(), active);
        collector.discovered_files = Some(2);
        assert!(collector.selected_detail_index_with_limit(&root, &selection, 1).is_none());
        assert!(collector.selected_detail_scan.lock().unwrap().is_none());
        assert!(!collector.recent_call_refresh_pending());
    }

    #[test]
    fn overflow_workdirs_stream_cursor_metadata_beyond_snapshot_budget() {
        for (count, padding) in [(3, 9 * 1024 * 1024), (65, 0)] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let ids: Vec<_> = (1..=count).map(|i| format!("00000000-0000-4000-8000-{i:012}")).collect();
            for (i, id) in ids.iter().enumerate() {
                std::fs::write(root.join(format!("rollout-{id}.jsonl")), format!("{}\n", json!({
                    "timestamp":"2026-11-13T01:00:00Z", "type":"session_meta",
                    "payload":{"id":id,"cwd":format!("/fixture/workdir-{i}")}
                }))).unwrap();
            }
            let mut collector = Collector::new(root.join("monitoring"), true, None);
            let stopped = AtomicBool::new(false);
            for _ in 0..4 {
                let (_, pending, _) = collector.refresh(&root, &stopped, true, false, CommandSwitches::default());
                if !pending { break; }
            }
            assert_eq!(collector.files.len(), count);
            let mut store = lam_history_store::Store::open(&root.join("monitoring"), lam_history_store::Access::ReadWrite).unwrap().unwrap();
            let mut verified = Vec::new();
            for path in collector.files.keys() {
                let key = checkpoint::key(&root, path).unwrap();
                let mut checkpoint = store.checkpoint(&key, 0).unwrap().unwrap();
                // Valid JSON whitespace exercises raw cursor byte limits without
                // introducing unsupported fields or real source data.
                checkpoint.cursor.payload.push_str(&" ".repeat(padding));
                store.save_checkpoints(std::slice::from_ref(&checkpoint.cursor), &[], &[]).unwrap();
                let metadata = std::fs::metadata(path).unwrap();
                verified.push(VerifiedMembership { key, fingerprint: source_fingerprint(file_identity(&metadata), metadata.len(), metadata.modified().ok()).unwrap(),
                    checkpoint_signature: checkpoint.cursor.signature, materialized: true, call_headers: Some(Vec::new()) });
            }
            let mut membership = SourceMembership::new().unwrap();
            membership.insert(&verified.iter().map(|row| MembershipRecord { key: row.key.clone(), fingerprint: Some(row.fingerprint.clone()) }).collect::<Vec<_>>()).unwrap();
            membership.mark_verified(&verified).unwrap();
            collector.source_membership = Some(membership);
            collector.source_membership_complete = true;
            collector.discovered_files = Some(count);
            collector.discovery = None;
            collector.files.clear();
            if padding > 0 {
                assert!(collector.overflow_projections(0).1);
                assert_eq!(collector.overflow_projections(0).0.len(), 1);
            }
            let mut metadata = HashMap::new();
            assert!(collector.populate_overflow_workdirs_with_limit(&root, &mut metadata, 0));
            assert!(metadata.is_empty(), "do not publish a partial scan");
            assert!(collector.overflow_workdir_scan.as_ref().unwrap().retained.len() <= 64);
            assert!(collector.overflow_workdir_scan.as_ref().unwrap().bytes <= 16 * 1024 * 1024);
            let mut resumed_after = collector.overflow_workdir_scan.as_ref().unwrap().after.clone();
            assert!(!resumed_after.is_empty());
            let mut rounds = 1;
            while collector.populate_overflow_workdirs_with_limit(&root, &mut metadata, 0) {
                rounds += 1;
                assert!(rounds <= count + 1, "must not repeatedly scan the prefix");
                assert!(metadata.is_empty());
                let after = collector.overflow_workdir_scan.as_ref().unwrap().after.clone();
                let scan = collector.overflow_workdir_scan.as_ref().unwrap();
                assert!(after > resumed_after || scan.validating);
                resumed_after = after;
            }
            for (i, id) in ids.iter().enumerate() {
                assert_eq!(metadata[id]["_cwd"], format!("/fixture/workdir-{i}"));
            }
            assert!(collector.overflow_workdirs.is_empty());
            assert!(collector.overflow_workdir_scan.is_none());
            // A writer changing an already staged source invalidates publication.
            let mut stale_metadata = HashMap::new();
            assert!(collector.populate_overflow_workdirs_with_limit(&root, &mut stale_metadata, 0));
            let key = collector.overflow_workdir_scan.as_ref().unwrap().retained.first_key_value().unwrap().0.clone();
            let mut changed = store.checkpoint(&key, 0).unwrap().unwrap().cursor;
            changed.signature = "changed-between-rounds".into();
            store.save_checkpoints(&[changed], &[], &[]).unwrap();
            let mut rounds = 0;
            while collector.populate_overflow_workdirs_with_limit(&root, &mut stale_metadata, 0) {
                rounds += 1;
                assert!(rounds <= count + 1);
            }
            assert!(stale_metadata.is_empty());
        }
    }

    #[test]
    fn selects_latest_calls_across_working_and_verified_sources_with_path_ties() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector.session_root = Some(root.clone());
        let working = root.join("m.jsonl");
        let mut session = Session::default();
        let call: project::Call = serde_json::from_value(json!({"tool":"test","time":"2026-11-12T10:00:00Z","completed":null,"sequence":1})).unwrap();
        session.projection.calls.insert("active".into(), call);
        collector.files.insert(working.clone(), session);
        let mut membership = SourceMembership::new().unwrap();
        for name in ["a-z.jsonl", "a/z.jsonl"] {
            let path = root.join(name);
            let key = checkpoint::key(&root, &path).unwrap();
            membership.insert(&[MembershipRecord { key:key.clone(), fingerprint:Some("fingerprint".into()) }]).unwrap();
            membership.mark_verified(&[VerifiedMembership { key:key.clone(), fingerprint:"fingerprint".into(), checkpoint_signature:"signature".into(), materialized:true, call_headers:Some(vec![lam_history_store::CallHeader { call_id:"outside".into(), event_time:Some("2026-11-12T10:00:00.000Z".into()) }]) }]).unwrap();
            collector.duplicate_session_paths.insert(key, path);
        }
        collector.source_membership = Some(membership);
        collector.source_membership_complete = true;
        let selected = collector.select_recent_calls(2, 100).unwrap();
        assert!(selected.complete);
        assert!(selected.calls[&checkpoint::key(&root, &working).unwrap()].contains("active"));
        assert!(selected.calls[&checkpoint::key(&root, &root.join("a/z.jsonl")).unwrap()].contains("outside"));
        assert!(!selected.calls.contains_key(&checkpoint::key(&root, &root.join("a-z.jsonl")).unwrap()));
        assert!(!collector.select_recent_calls(2, 1).unwrap().complete);
    }

    #[test]
    fn recent_call_selection_clears_state_when_overflow_ends() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector.session_root = Some(root.clone());
        let path = root.join("outside.jsonl");
        let key = checkpoint::key(&root, &path).unwrap();
        let mut membership = SourceMembership::new().unwrap();
        membership.insert(&[MembershipRecord { key: key.clone(), fingerprint: Some("same".into()) }]).unwrap();
        membership.mark_verified(&[VerifiedMembership { key: key.clone(), fingerprint: "same".into(),
            checkpoint_signature: "anchor".into(), materialized: true,
            call_headers: Some(["a", "b"].map(|id| lam_history_store::CallHeader { call_id: id.into(), event_time: None }).into()),
        }]).unwrap();
        collector.duplicate_session_paths.insert(key, path.clone());
        collector.source_membership = Some(membership);
        collector.source_membership_complete = true;
        collector.discovered_files = Some(1);
        assert!(!collector.select_recent_calls(1, 1).unwrap().complete);
        assert!(collector.recent_call_refresh_pending());
        collector.recent_call_details_dirty.store(true, Ordering::Release);
        // The remaining source enters the working set; overflow selection is no
        // longer invoked by either snapshot or detail assembly.
        collector.files.insert(path, Session::default());
        assert!(!collector.recent_call_refresh_pending());
        assert!(collector.recent_call_scan.lock().unwrap().is_none());
        assert!(!collector.recent_call_details_dirty.load(Ordering::Acquire));
        assert!(!collector.recent_call_refresh_pending());
    }

    #[test]
    fn recent_call_selection_resumes_past_100000_tied_headers() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector.session_root = Some(root.clone());
        let mut membership = SourceMembership::new().unwrap();
        let mut records = Vec::new();
        for source in 0..14 {
            let path = root.join(format!("source-{source:02}.jsonl"));
            let key = checkpoint::key(&root, &path).unwrap();
            membership.insert(&[MembershipRecord { key: key.clone(), fingerprint: Some("same".into()) }]).unwrap();
            membership.mark_verified(&[VerifiedMembership { key: key.clone(), fingerprint: "same".into(),
                checkpoint_signature: "anchor".into(), materialized: true,
                call_headers: Some((0..7500).map(|i| lam_history_store::CallHeader {
                    call_id: format!("{i:05}"), event_time: None,
                }).collect()),
            }]).unwrap();
            records.push(MembershipRecord { key: key.clone(), fingerprint: Some("same".into()) });
            collector.duplicate_session_paths.insert(key, path);
        }
        collector.source_membership = Some(membership);
        collector.source_membership_complete = true;
        collector.discovered_files = Some(14);
        let first = collector.select_recent_calls(2, 100_000).unwrap();
        assert!(!first.complete);
        assert!(collector.recent_call_scan_pending());
        let cursor = collector.recent_call_scan.lock().unwrap().as_ref().unwrap().after.clone();
        // A generation-only discovery pass and unchanged working set must not
        // discard progress and repeatedly starve the last 5,000 headers.
        let index = collector.source_membership.as_mut().unwrap();
        index.next_generation().unwrap();
        index.insert(&records).unwrap();
        index.set_working(&[]).unwrap();
        let second = collector.select_recent_calls(2, 100_000).unwrap();
        assert!(second.complete);
        assert!(!collector.recent_call_scan_pending());
        assert!(collector.recent_call_refresh_pending(), "completed selection still needs detail publication");
        collector.recent_call_details_dirty.store(false, Ordering::Release);
        assert!(!collector.recent_call_refresh_pending());
        assert_ne!(collector.recent_call_scan.lock().unwrap().as_ref().unwrap().after, cursor);
        let latest = checkpoint::key(&root, &root.join("source-13.jsonl")).unwrap();
        assert_eq!(second.calls.len(), 1);
        assert_eq!(second.calls[&latest], ["07498".to_owned(), "07499".to_owned()].into_iter().collect());
        // A semantic source-index change must invalidate the completed result.
        collector.source_membership.as_mut().unwrap().set_working(std::slice::from_ref(&latest)).unwrap();
        assert!(!collector.select_recent_calls(2, 100_000).unwrap().complete);
        let updated = collector.select_recent_calls(2, 100_000).unwrap();
        assert!(updated.complete);
        assert!(!updated.calls.contains_key(&latest));
        let next = checkpoint::key(&root, &root.join("source-12.jsonl")).unwrap();
        assert_eq!(updated.calls[&next], ["07498".to_owned(), "07499".to_owned()].into_iter().collect());
        // Active in-memory calls are outside the index epoch and must also
        // invalidate an otherwise completed scan.
        let active_path = root.join("active.jsonl");
        let mut active = Session::default();
        active.projection.calls.insert("new-active".into(), serde_json::from_value(json!({
            "tool":"fixture", "time":"2026-11-13T10:00:00Z", "completed":null, "sequence":1
        })).unwrap());
        collector.files.insert(active_path.clone(), active);
        assert!(!collector.select_recent_calls(2, 100_000).unwrap().complete);
        let with_active = collector.select_recent_calls(2, 100_000).unwrap();
        assert!(with_active.complete);
        assert!(with_active.calls[&checkpoint::key(&root, &active_path).unwrap()].contains("new-active"));
    }

    #[test]
    fn recent_call_pages_advance_past_unverified_and_working_sources() {
        let mut membership = SourceMembership::new().unwrap();
        membership.insert(&["a", "b", "c"].map(|key| MembershipRecord { key:key.into(), fingerprint:Some("original".into()) })).unwrap();
        let row = |key: &str, calls: Vec<lam_history_store::CallHeader>| VerifiedMembership {
            key:key.into(), fingerprint:"original".into(), checkpoint_signature:format!("signature-{key}"),
            materialized:true, call_headers:Some(calls),
        };
        let header = |id: &str, time: Option<&str>| lam_history_store::CallHeader { call_id:id.into(), event_time:time.map(str::to_owned) };
        membership.mark_verified(&[
            row("a", vec![header("first", Some("2026-11-12T10:00:00.000Z"))]),
            row("b", vec![header("second", Some("2026-11-12T10:00:00.000Z")), header("unknown", None)]),
            row("c", vec![header("old", Some("2026-11-11T10:00:00.000Z"))]),
        ]).unwrap();
        membership.set_working(&["b".into()]).unwrap();
        membership.insert(&[MembershipRecord { key:"c".into(), fingerprint:Some("changed".into()) }]).unwrap();
        let mut after = None;
        let mut results = Vec::new();
        let mut pages = 0;
        loop {
            let (items, next) = membership.recent_call_page(after.as_ref(), 1).unwrap();
            results.extend(items);
            pages += 1;
            assert!(pages <= 5);
            let Some(cursor) = next else { break };
            after = Some(cursor);
        }
        assert_eq!(pages, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].position.source_key, "a");
        assert_eq!(results[0].position.call_id, "first");
        assert_eq!(results[0].checkpoint_signature, "signature-a");
        assert!(membership.recent_call_page(None, 513).is_err());
    }

    #[test]
    fn source_call_index_tracks_verified_revisions_and_removes_stale_sources() {
        let mut membership = SourceMembership::new().unwrap();
        membership.insert(&[MembershipRecord { key:"a".into(), fingerprint:Some("one".into()) }]).unwrap();
        let record = VerifiedMembership {
            key:"a".into(), fingerprint:"one".into(), checkpoint_signature:"signature".into(),
            materialized:true,
            call_headers:Some(vec![lam_history_store::CallHeader { call_id:"call".into(), event_time:Some("2026-11-12T10:00:00.000Z".into()) }]),
        };
        membership.mark_verified(&[record]).unwrap();
        let visible = |membership: &SourceMembership| -> i64 {
            membership.db.query_row("SELECT count(*) FROM source_calls c JOIN sources s ON s.source_key=c.source_key WHERE s.verified=1 AND s.revision=? AND s.generation=?", rusqlite::params![membership.revision, membership.generation], |row| row.get(0)).unwrap()
        };
        assert_eq!(visible(&membership), 1);
        let duplicate = lam_history_store::CallHeader { call_id:"duplicate".into(), event_time:None };
        let invalid = VerifiedMembership { key:"a".into(), fingerprint:"one".into(), checkpoint_signature:"new-signature".into(), materialized:true, call_headers:Some(vec![duplicate.clone(), duplicate]) };
        assert!(membership.mark_verified(&[invalid]).is_err());
        assert_eq!(visible(&membership), 1);
        assert!(membership.is_verified("a").unwrap());
        assert!(membership.matches_verified("a", "one", "signature").unwrap());
        membership.next_generation().unwrap();
        membership.insert(&[MembershipRecord { key:"a".into(), fingerprint:Some("changed".into()) }]).unwrap();
        assert_eq!(visible(&membership), 0);
        membership.next_generation().unwrap();
        membership.remove_stale().unwrap();
        let rows: i64 = membership.db.query_row("SELECT count(*) FROM source_calls", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn sources_leaving_working_set_require_materialized_snapshot() {
        let mut membership = SourceMembership::new().unwrap();
        let row = |key: &str, fingerprint: &str| MembershipRecord {
            key: key.into(),
            fingerprint: Some(fingerprint.into()),
        };
        let verified = |key: &str, fingerprint: &str, snapshot: Option<&str>| VerifiedMembership {
            key: key.into(),
            fingerprint: fingerprint.into(),
            checkpoint_signature: format!("signature-{key}"),
            materialized: snapshot.is_some(),
            call_headers: snapshot.map(|_| Vec::new()),
        };
        membership
            .insert(&[row("a", "one"), row("b", "two")])
            .unwrap();
        membership
            .mark_verified(&[verified("a", "one", None), verified("b", "two", None)])
            .unwrap();
        membership.set_working(&["a".into()]).unwrap();
        assert_eq!(membership.unverified_page("", 64).unwrap(), ["b"]);
        membership
            .mark_verified(&[verified("b", "two", Some("{}"))])
            .unwrap();
        membership.set_working(&[]).unwrap();
        assert_eq!(membership.unverified_page("", 64).unwrap(), ["a"]);
        assert_eq!(membership.materialized_count().unwrap(), 1);
        membership.insert(&[row("b", "changed")]).unwrap();
        assert_eq!(membership.materialized_count().unwrap(), 0);
    }

    #[test]
    fn source_membership_revalidates_changed_files_and_removes_stale_entries() {
        let mut membership = SourceMembership::new().unwrap();
        let row = |key: &str, fingerprint: &str| MembershipRecord {
            key: key.into(),
            fingerprint: Some(fingerprint.into()),
        };
        let verified = |key: &str, fingerprint: &str| VerifiedMembership {
            key: key.into(),
            fingerprint: fingerprint.into(),
            checkpoint_signature: format!("signature-{key}"),
            materialized: false,
            call_headers: None,
        };
        membership
            .insert(&[row("a", "one"), row("b", "two"), row("removed", "three")])
            .unwrap();
        membership
            .mark_verified(&[
                verified("a", "one"),
                verified("b", "two"),
                verified("removed", "three"),
            ])
            .unwrap();
        assert_eq!(membership.unverified_count().unwrap(), 0);
        membership.next_generation().unwrap();
        membership
            .insert(&[row("a", "one"), row("b", "changed")])
            .unwrap();
        assert_eq!(membership.unverified_page("", 64).unwrap(), ["b"]);
        membership.mark_verified(&[verified("b", "two")]).unwrap();
        assert_eq!(membership.unverified_page("", 64).unwrap(), ["b"]);
        assert_eq!(
            membership.verified_page("", 64).unwrap(),
            [("a".to_owned(), "signature-a".to_owned())]
        );
        assert_eq!(membership.remove_stale().unwrap(), 1);
        assert_eq!(membership.remove_stale().unwrap(), 0);
        assert_eq!(membership.page("", 64).unwrap(), ["a", "b"]);
        membership
            .mark_verified(&[verified("b", "changed")])
            .unwrap();
        assert_eq!(membership.unverified_count().unwrap(), 0);
        membership.invalidate().unwrap();
        assert_eq!(membership.unverified_page("", 64).unwrap(), ["a", "b"]);
    }

    #[test]
    fn snapshot_reports_discovered_files_outside_working_set_without_guessing_pending() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), true, None);
        let path = directory
            .path()
            .join("rollout-00000000-0000-4000-8000-000000000001.jsonl");
        collector.files.insert(path.clone(), Session::new(&path));
        collector.discovered_files = Some(5001);
        let snapshot = collector.snapshots("partly_unavailable", "ok", 1);
        let value: Value = serde_json::from_slice(&snapshot.window_bytes(3)).unwrap();
        let state = &value["codex"]["read_state"];
        assert_eq!(value["codex"]["files"], 5001);
        assert_eq!(state["file_count"], 5001);
        assert_eq!(state["working_files"], 1);
        assert_eq!(state["outside_working_set"], 5000);
        assert_eq!(state["source_index_ready"], false);
        assert!(state["pending_files"].is_null());
        assert!(state["initial_pending_files"].is_null());
    }

    #[test]
    fn session_discovery_resumes_after_each_bounded_batch() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for index in 0..7 {
            let id = format!("00000000-0000-4000-8000-{index:012}");
            std::fs::write(root.join(format!("rollout-{id}.jsonl")), "").unwrap();
        }
        let metadata = std::fs::metadata(&root).unwrap();
        let mut scan = SessionDiscovery::new(
            &root,
            file_identity(&metadata),
            BTreeSet::new(),
            None,
            BTreeSet::new(),
            false,
        );
        let stopped = AtomicBool::new(false);
        let mut batches = 0;
        while !scan.advance(2, &stopped) {
            batches += 1;
            assert!(batches < 10);
        }
        assert!(batches >= 3);
        assert_eq!(scan.file_count, 7);
        assert_eq!(scan.candidates.len(), 7);
        assert_eq!(scan.indexed.len(), 7);
        assert!(!scan.incomplete);
    }

    #[test]
    fn discovery_keeps_distinct_paths_for_duplicate_thread_ids() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let first = root.join(format!("rollout-{id}.jsonl"));
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let second = nested.join(format!("rollout-{id}.jsonl"));
        std::fs::write(&first, "").unwrap();
        std::fs::write(&second, "").unwrap();
        let mut scan = SessionDiscovery::new(&root, file_identity(&std::fs::metadata(&root).unwrap()), BTreeSet::new(), None, BTreeSet::new(), false);
        let stopped = AtomicBool::new(false);
        while !scan.advance(100, &stopped) {}
        assert_eq!(scan.file_count, 2);
        assert_eq!(scan.indexed.len(), 1);
        assert_eq!(scan.duplicate_sources.get(&checkpoint::key(&root, &first).unwrap()), Some(&first));
        assert_eq!(scan.duplicate_sources.get(&checkpoint::key(&root, &second).unwrap()), Some(&second));
    }

    #[test]
    fn session_discovery_counts_files_beyond_working_set() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for index in 0..=FILE_LIMIT {
            let id = format!("00000000-0000-4000-8000-{index:012}");
            std::fs::write(root.join(format!("rollout-{id}.jsonl")), "").unwrap();
        }
        let mut scan = SessionDiscovery::new(
            &root,
            file_identity(&std::fs::metadata(&root).unwrap()),
            BTreeSet::new(),
            None,
            BTreeSet::new(),
            false,
        );
        let stopped = AtomicBool::new(false);
        while !scan.advance(1000, &stopped) {}
        assert_eq!(scan.file_count, FILE_LIMIT + 1);
        assert_eq!(scan.candidates.len(), FILE_LIMIT);
        assert_eq!(scan.indexed.len(), FILE_LIMIT + 1);
        let membership = scan.membership.as_ref().unwrap();
        let mut after = String::new();
        let mut total = 0;
        loop {
            let page = membership.page(&after, 64).unwrap();
            if page.is_empty() {
                break;
            }
            assert!(page.iter().all(|key| key.starts_with("rust:v1:")
                && key.len() == 72
                && !key.contains("rollout")));
            total += page.len();
            after = page.last().unwrap().clone();
        }
        assert_eq!(total, FILE_LIMIT + 1);
        let membership = scan.membership.as_mut().unwrap();
        let first = membership.unverified_page("", 2).unwrap();
        assert_eq!(membership.unverified_count().unwrap(), FILE_LIMIT + 1);
        let records: Vec<_> = first
            .iter()
            .map(|key| VerifiedMembership {
                key: key.clone(),
                fingerprint: membership
                    .db
                    .query_row(
                        "SELECT fingerprint FROM sources WHERE source_key=?",
                        [key],
                        |row| row.get(0),
                    )
                    .unwrap(),
                checkpoint_signature: "fixture-signature".into(),
                materialized: false,
                call_headers: None,
            })
            .collect();
        membership.mark_verified(&records).unwrap();
        assert_eq!(membership.unverified_count().unwrap(), FILE_LIMIT - 1);
        assert!(membership
            .unverified_page("", 2)
            .unwrap()
            .iter()
            .all(|key| !first.contains(key)));
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector
            .tool_events_enabled
            .store(false, Ordering::Release);
        collector.files_enabled.store(false, Ordering::Release);
        collector.mcp_switches.write().unwrap().mcp = false;
        let (health, pending, _) =
            collector.refresh(&root, &stopped, true, false, CommandSwitches::default());
        assert_eq!(health, "partly_unavailable");
        assert!(pending);
        assert_eq!(
            collector
                .source_membership
                .as_ref()
                .unwrap()
                .unverified_count()
                .unwrap(),
            1
        );
        let outside = collector
            .source_membership
            .as_ref()
            .unwrap()
            .unverified_page("", 1)
            .unwrap()
            .remove(0);
        let path = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| checkpoint::key(&root, path).as_deref() == Some(outside.as_str()))
            .unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let id = project::filename_identity(path.file_name().unwrap().to_str().unwrap()).unwrap();
        let event_time = stamp(now());
        let mut body = format!(
            "{}\n",
            json!({"timestamp":event_time,"type":"session_meta","payload":{"id":id,"cwd":"/fixture/private-workdir"}})
        );
        let padding = format!(
            "{}\n",
            json!({"type":"event_msg","payload":{"type":"unrelated","value":"x".repeat(200_000)}})
        );
        for _ in 0..6 {
            body.push_str(&padding);
        }
        body.push_str(&format!("{}\n", json!({"timestamp":event_time,"type":"response_item","payload":{"type":"function_call","name":"mcp__sqlite__query","call_id":"outside-call","arguments":"{\"query\":\"SELECT 1\",\"api_key\":\"PRIVATE_MARKER\"}"}})));
        body.push_str(&format!("{}\n", json!({"timestamp":event_time,"type":"response_item","payload":{"type":"function_call_output","call_id":"outside-call","output":{"isError":true}}})));
        body.push_str(&format!("{}\n", json!({"timestamp":event_time,"type":"event_msg","payload":{"type":"turn_failed","error_code":"connection_refused","message":"PRIVATE_SESSION_ERROR"}})));
        body.push_str(&format!("{}\n", json!({"timestamp":event_time,"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":25}}}})));
        body.push_str(&format!(
            "{}\n",
            json!({"timestamp":event_time,"type":"event_msg","payload":{"type":"task_complete"}})
        ));
        std::fs::write(&path, body).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let (_, more, _) =
            collector.refresh(&root, &stopped, true, false, CommandSwitches::default());
        assert!(more);
        assert_eq!(
            collector
                .source_membership
                .as_ref()
                .unwrap()
                .unverified_count()
                .unwrap(),
            1
        );
        collector.refresh(&root, &stopped, true, false, CommandSwitches::default());
        assert_eq!(
            collector
                .source_membership
                .as_ref()
                .unwrap()
                .unverified_count()
                .unwrap(),
            0
        );
        assert_eq!(
            collector
                .source_membership
                .as_ref()
                .unwrap()
                .materialized_count()
                .unwrap(),
            1
        );
        let materialized: bool = collector
            .source_membership
            .as_ref()
            .unwrap()
            .db
            .query_row(
                "SELECT materialized FROM sources WHERE source_key=?",
                [&outside],
                |row| row.get(0),
            )
            .unwrap();
        assert!(materialized);
        let columns: Vec<String> = collector.source_membership.as_ref().unwrap().db
            .prepare("PRAGMA table_info(sources)").unwrap().query_map([], |row| row.get(1)).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        assert!(!columns.iter().any(|column| column == "snapshot_payload"));
        let indexed: Vec<String> = collector.source_membership.as_ref().unwrap().db
            .prepare("SELECT call_id FROM source_calls WHERE source_key=?").unwrap()
            .query_map([&outside], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(indexed, ["outside-call"]);
        let mut store = lam_history_store::Store::open(
            &root.join("monitoring"),
            lam_history_store::Access::ReadOnly,
        )
        .unwrap()
        .unwrap();
        let saved = store.checkpoint(&outside, 10).unwrap().unwrap();
        let payload: Value = serde_json::from_str(&saved.cursor.payload).unwrap();
        assert_eq!(payload["projection"]["id"], id);
        assert_eq!(payload["projection"]["status"], "completed");
        assert_eq!(saved.calls.len(), 1);
        assert_eq!(saved.calls[0].call_id, "outside-call");
        assert!(!saved.cursor.payload.contains("/fixture/private-workdir"));
        assert!(!saved.calls[0].payload.contains("PRIVATE_MARKER"));
        assert_eq!(payload["projection"]["sqlite_incomplete"], true);
        let disabled_call: Value = serde_json::from_str(&saved.calls[0].payload).unwrap();
        assert_eq!(disabled_call["files_incomplete"], true);
        assert_eq!(disabled_call["mcp_sources_incomplete"], true);
        let header_size = saved.cursor.payload.len() + saved.cursor.signature.len();
        drop(store);
        let (bounded, incomplete) = collector.overflow_projections_with_limit(0, header_size);
        assert!(incomplete);
        let bounded = bounded.iter().find(|item| item.projection.id.as_deref() == Some(id.as_str())).unwrap();
        assert!(bounded.calls_incomplete);
        assert!(bounded.projection.calls.is_empty());
        let (headers, incomplete) = collector.overflow_projections(CALL_LIMIT);
        assert!(incomplete);
        let header = headers.iter().find(|item| item.projection.id.as_deref() == Some(id.as_str())).unwrap();
        assert!(header.calls_incomplete);
        assert!(header.projection.calls.is_empty());
        assert!(header.source_details_pending);
        collector.applied_sqlite_enabled = false;
        collector.files.values_mut().next().unwrap().projection.allowance = allowance::project(&json!({"primary":{"used_percent":80}}), now() - Duration::days(1));
        let disabled = collector.snapshots("partly_unavailable", "ok", 1);
        let disabled: Value = serde_json::from_slice(&disabled.window_bytes(3)).unwrap();
        assert_eq!(disabled["codex"]["usage"]["limits"][0]["used_percent"], 25);
        let outside_row = disabled["codex"]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["thread_id"] == id)
            .unwrap();
        assert_eq!(outside_row["status"], "completed");
        assert_eq!(outside_row["tool_calls"], 1);
        assert_eq!(
            disabled["codex"]["sqlite"]["events"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        collector.tool_events_enabled.store(true, Ordering::Release);
        collector.files_enabled.store(true, Ordering::Release);
        collector.mcp_switches.write().unwrap().mcp = true;
        for _ in 0..4 {
            collector.refresh(&root, &stopped, true, true, CommandSwitches::default());
            if collector
                .source_membership
                .as_ref()
                .unwrap()
                .unverified_count()
                .unwrap()
                == 0
            {
                break;
            }
        }
        assert_eq!(
            collector
                .source_membership
                .as_ref()
                .unwrap()
                .unverified_count()
                .unwrap(),
            0
        );
        collector.applied_sqlite_enabled = true;
        let mut store = lam_history_store::Store::open(
            &root.join("monitoring"),
            lam_history_store::Access::ReadOnly,
        )
        .unwrap()
        .unwrap();
        let replayed = store.checkpoint(&outside, 10).unwrap().unwrap();
        let payload: Value = serde_json::from_str(&replayed.cursor.payload).unwrap();
        assert_ne!(payload["projection"]["sqlite_incomplete"], true);
        let call: Value = serde_json::from_str(&replayed.calls[0].payload).unwrap();
        assert_ne!(call["files_incomplete"], true);
        assert_ne!(call["mcp_sources_incomplete"], true);
        let history = store
            .page(lam_history_store::PageQuery {
                section: lam_history_store::Section::Sql,
                limit: 10,
                offset: 0,
                since: None,
                after: None,
                excluded_sources: &[],
            })
            .unwrap();
        assert_eq!(history.total, 1);
        std::fs::write(
            root.join("session_index.jsonl"),
            format!("{}\n", json!({"id":id,"thread_name":"Overflow title"})),
        )
        .unwrap();
        let state_db = rusqlite::Connection::open(root.join("state_1.sqlite")).unwrap();
        state_db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT); CREATE TABLE projects(id TEXT,name TEXT); INSERT INTO projects VALUES('p','Overflow project');").unwrap();
        state_db
            .execute("INSERT INTO threads VALUES(?1,NULL,'p')", [&id])
            .unwrap();
        let identities = collector.metadata_identities(&BTreeSet::new());
        assert!(identities.contains(&id));
        let (mut metadata, mut reports) =
            schedules::background_metadata(&root, &identities, false, false);
        let (projects, _) = projects::read(&root, &mut metadata, false, &mut reports);
        let mut uncached_metadata = HashMap::new();
        assert!(!collector.populate_overflow_workdirs_with_limit(&root, &mut uncached_metadata, 0));
        assert_eq!(uncached_metadata[&id]["_cwd"], "/fixture/private-workdir");
        assert!(collector.overflow_workdirs.is_empty());
        assert!(!collector.populate_overflow_workdirs(&root, &mut metadata));
        assert_eq!(metadata[&id]["_cwd"], "/fixture/private-workdir");
        let mut cached_metadata = HashMap::new();
        assert!(!collector.populate_overflow_workdirs(&root, &mut cached_metadata));
        assert_eq!(cached_metadata[&id]["_cwd"], "/fixture/private-workdir");
        collector.thread_metadata = metadata;
        collector.project_details = projects;
        collector.files.values_mut().next().unwrap().projection.allowance = allowance::project(&json!({"primary":{"used_percent":80}}), now() - Duration::days(1));
        let duplicate_dir = root.join("duplicate");
        std::fs::create_dir(&duplicate_dir).unwrap();
        let duplicate = duplicate_dir.join(path.file_name().unwrap());
        std::fs::write(&duplicate, "").unwrap();
        collector.session_paths.insert(id.clone(), duplicate);
        collector.duplicate_session_paths.insert(outside.clone(), path.clone());
        let (detail_index, ready) = collector.detail_index(&root);
        assert!(ready.contains(&id));
        let selected = detail_index.select_call(&id, "outside-call").unwrap();
        let detail = selected.tool("outside-call", true, false).unwrap();
        assert!(!detail.to_string().contains("PRIVATE_MARKER"));
        collector.overflow_details_ready = ready;
        collector.updated = Some(now());
        let snapshot = collector.snapshots("partly_unavailable", "ok", 1);
        let value: Value = serde_json::from_slice(&snapshot.window_bytes(3)).unwrap();
        assert!(!value.to_string().contains("/fixture/private-workdir"));
        let row = value["codex"]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["thread_id"] == id)
            .unwrap();
        assert_eq!(row["status"], "completed");
        assert_eq!(row["thread_name"], "Overflow title");
        assert_eq!(row["project_name"], "Overflow project");
        assert_eq!(row["tool_calls"], 1);
        assert_eq!(value["codex"]["tools"]["mcp__sqlite__query"], 1);
        assert!(value["errors"]["events"].as_array().unwrap().iter().any(|event| event["thread_id"] == id && event["call_id"] == "outside-call"));
        assert!(value["errors"]["events"].as_array().unwrap().iter().any(|event| event["thread_id"] == id && event["reason"] == "turn_failed" && event["code"] == "connection_refused"));
        assert_eq!(value["codex"]["connection"]["state"], "error");
        assert!(!value.to_string().contains("PRIVATE_SESSION_ERROR"));
        assert!(row["source_details_pending"].is_null());
        assert_eq!(value["codex"]["read_state"]["overflow_snapshot_sources"], 1);
        assert!(value["codex"]["read_state"]["file_limit"].is_null());
        assert_eq!(value["codex"]["read_state"]["working_file_limit"], FILE_LIMIT);
        assert_eq!(value["codex"]["read_state"]["outside_working_set"], 1);
        assert_eq!(
            value["codex"]["read_state"]["overflow_snapshot_incomplete"],
            true
        );
        drop(store);
        let mut writer = lam_history_store::Store::open(
            &root.join("monitoring"),
            lam_history_store::Access::ReadWrite,
        )
        .unwrap()
        .unwrap();
        let mut changed_cursor = replayed.cursor.clone();
        changed_cursor.signature = "changed-by-another-writer".into();
        writer
            .save_checkpoints(&[changed_cursor], &[], &[])
            .unwrap();
        drop(writer);
        assert!(detail_index.select_call(&id, "outside-call").is_none());
        let mut stale_metadata = HashMap::new();
        collector.populate_overflow_workdirs(&root, &mut stale_metadata);
        assert!(stale_metadata.get(&id).is_none());
        let stale = collector.snapshots("partly_unavailable", "ok", 2);
        let stale: Value = serde_json::from_slice(&stale.window_bytes(3)).unwrap();
        assert_eq!(stale["codex"]["read_state"]["overflow_snapshot_sources"], 0);
        let stale_row = stale["codex"]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["thread_id"] == id)
            .unwrap();
        assert_eq!(stale_row["metadata_only"], true);
        assert_eq!(stale_row["tool_calls"], 0);
        assert_eq!(stale["codex"]["usage"]["limits"][0]["used_percent"], 80);
        let mut writer = lam_history_store::Store::open(&root.join("monitoring"), lam_history_store::Access::ReadWrite).unwrap().unwrap();
        writer.save_checkpoints(std::slice::from_ref(&replayed.cursor), &replayed.calls, &[]).unwrap();
        drop(writer);
        let template: project::Call = serde_json::from_str(&replayed.calls[0].payload).unwrap();
        let mut active_paths: Vec<_> = collector.files.iter().map(|(path, state)| (state.modified, path.clone())).collect();
        active_paths.sort_by(|left, right| right.cmp(left));
        let mut remaining = CALL_LIMIT;
        for (_, active_path) in active_paths.into_iter().take(7) {
            let state = collector.files.get_mut(&active_path).unwrap();
            let count = remaining.min(FILE_CALL_LIMIT);
            for index in 0..count {
                state.projection.calls.insert(format!("fixture-{index}"), template.clone());
            }
            remaining -= count;
        }
        assert_eq!(remaining, 0);
        assert_eq!(collector.files.values().map(|state| state.projection.calls.len()).sum::<usize>(), CALL_LIMIT);
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", json!({"timestamp":stamp(now()),"type":"response_item","payload":{"type":"function_call","name":"fixture_tool","call_id":"new-overflow","arguments":{}}})).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        drop(file);
        let mut preserved = false;
        for _ in 0..8 {
            collector.refresh(&root, &stopped, true, true, CommandSwitches::default());
            let mut reader = lam_history_store::Store::open(&root.join("monitoring"), lam_history_store::Access::ReadOnly).unwrap().unwrap();
            let checkpoint = reader.checkpoint(&outside, 10).unwrap();
            preserved = checkpoint.is_some_and(|checkpoint| checkpoint.calls.iter().any(|call| call.call_id == "new-overflow"));
            if preserved && collector.source_membership.as_ref().unwrap().unverified_count().unwrap() == 0 { break; }
        }
        assert!(preserved, "overflow call must survive a full working set");
        assert_eq!(collector.source_membership.as_ref().unwrap().unverified_count().unwrap(), 0);
        collector.updated = Some(now());
        let ranked = collector.snapshots("partly_unavailable", "ok", 3);
        let ranked: Value = serde_json::from_slice(&ranked.window_bytes(3)).unwrap();
        let row = ranked["codex"]["threads"].as_array().unwrap().iter().find(|row| row["thread_id"] == id).unwrap();
        assert!(row["tool_events"].as_array().unwrap().iter().any(|event| event["call_id"] == "new-overflow"));
        let (ranked_details, _) = collector.detail_index(&root);
        assert!(ranked_details.select_call(&id, "new-overflow").is_some());
    }

    #[test]
    fn thread_status_reports_pending_source_until_read_completes() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), true, None);
        let id = "00000000-0000-4000-8000-000000000001";
        let path = directory.path().join(format!("rollout-{id}.jsonl"));
        let mut session = Session::new(&path);
        session.size = 10;
        collector.files.insert(path.clone(), session);
        let snapshot = collector.snapshots("ok", "ok", 1);
        let value: Value = serde_json::from_slice(&snapshot.window_bytes(3)).unwrap();
        assert_eq!(
            value["codex"]["threads"][0]["status_backfill_pending"],
            true
        );
        assert_eq!(value["codex"]["read_state"]["initial_pending_files"], 1);
        assert_eq!(value["codex"]["read_state"]["history_pending_bytes"], 0);
        assert_eq!(value["codex"]["read_state"]["error_pending_bytes"], 0);
        collector.files.get_mut(&path).unwrap().identity = Some((1, 1));
        collector.files.get_mut(&path).unwrap().offset = 10;
        let snapshot = collector.snapshots("ok", "ok", 2);
        let value: Value = serde_json::from_slice(&snapshot.window_bytes(3)).unwrap();
        assert_eq!(
            value["codex"]["threads"][0]["status_backfill_pending"],
            false
        );
    }

    #[test]
    fn related_session_changes_invalidate_metadata_without_including_access_events() {
        let root = Path::new("/fixture/sessions");
        let changed = Event::new(notify::EventKind::Any).add_path(root.join("rollout-child.jsonl"));
        assert!(session_metadata_event(&changed, root));
        let unrelated =
            Event::new(notify::EventKind::Any).add_path(PathBuf::from("/fixture/other"));
        assert!(!session_metadata_event(&unrelated, root));
        let access = Event::new(notify::EventKind::Access(notify::event::AccessKind::Any))
            .add_path(root.join("rollout-child.jsonl"));
        assert!(!session_metadata_event(&access, root));
    }

    #[test]
    fn session_filename_selects_last_uuid_with_extra_suffix() {
        let first = "00000000-0000-4000-8000-000000000001";
        let last = "00000000-0000-4000-8000-000000000002";
        let filename = format!("rollout-前-{first}-{last}-suffix.jsonl");
        assert_eq!(project::filename_identity(&filename).as_deref(), Some(last));
        assert!(project::filename_identity("rollout-no-id.jsonl").is_none());
    }

    #[test]
    fn related_lifecycle_reads_only_bounded_selected_tail_and_reuses_cache() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let id = "00000000-0000-4000-8000-000000000002";
        let path = root.join(format!("rollout-{id}.jsonl"));
        let body = b"{\"timestamp\":\"2026-11-12T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n{\"timestamp\":\"2026-11-12T10:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n";
        std::fs::write(&path, body).unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector.session_paths.insert(id.to_owned(), path);
        let original =
            json!({"execution":{"parent_thread_id":"00000000-0000-4000-8000-000000000001"}});
        let mut metadata = HashMap::from([(id.to_owned(), original.clone())]);
        let pending = collector.related_lifecycle(&root, &mut metadata);
        assert_eq!(pending["pending_files"], 1);
        assert!(metadata[id].get("status").is_none());
        collector.related_budget = body.len();
        let report = collector.related_lifecycle(&root, &mut metadata);
        assert_eq!(report["bytes_read"], body.len());
        assert_eq!(report["rows_read"], 2);
        assert_eq!(metadata[id]["status"], "completed");
        assert_eq!(metadata[id]["status_source"], "session_lifecycle");
        let mut next = HashMap::from([(id.to_owned(), original)]);
        let cached = collector.related_lifecycle(&root, &mut next);
        assert_eq!(cached["bytes_read"], 0);
        assert_eq!(next[id]["status"], "completed");
    }

    #[cfg(unix)]
    #[test]
    fn related_lifecycle_rejects_redirected_source() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("private.jsonl");
        std::fs::write(&target, b"private").unwrap();
        let id = "00000000-0000-4000-8000-000000000002";
        let path = root.join(format!("rollout-{id}.jsonl"));
        symlink(target, &path).unwrap();
        let mut collector = Collector::new(root.join("monitoring"), true, None);
        collector.session_paths.insert(id.to_owned(), path);
        collector.related_budget = 65536;
        let mut metadata = HashMap::from([(
            id.to_owned(),
            json!({"execution":{"parent_thread_id":"00000000-0000-4000-8000-000000000001"}}),
        )]);
        let report = collector.related_lifecycle(&root, &mut metadata);
        assert_eq!(report["health"], "partly_unavailable");
        assert_eq!(report["bytes_read"], 0);
        assert!(metadata[id].get("status").is_none());
    }

    #[test]
    fn preload_snapshot_marks_initial_sections_only_before_collection() {
        let directory = tempfile::tempdir().unwrap();
        let collector = Collector::new(directory.path().to_path_buf(), true, None);
        let initial = collector.snapshots("waiting", "starting", 0);
        for window in 0..4 {
            let value: Value = serde_json::from_slice(&initial.window_bytes(window)).unwrap();
            assert_eq!(value["initial_sections"], json!(["catalog", "runtime"]));
            assert_eq!(value["health"], "starting");
        }
        let complete = collector.snapshots("ok", "ok", 1);
        let value: Value = serde_json::from_slice(&complete.window_bytes(3)).unwrap();
        assert!(value.get("initial_sections").is_none());
    }

    #[test]
    fn collection_phase_updates_existing_snapshot_without_new_collection() {
        let directory = tempfile::tempdir().unwrap();
        let collector = Collector::new(directory.path().to_path_buf(), false, None);
        let snapshots = collector.snapshots("disabled", "disabled", 0);
        let read = || serde_json::from_slice::<Value>(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(read()["monitor"]["collection"]["phase"], "waiting");
        collector.set_phase("sessions");
        let active = read();
        assert_eq!(active["monitor"]["collection"]["phase"], "sessions");
        assert!(active["monitor"]["collection"]["phase_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
        collector.set_phase("idle");
        let idle = read();
        assert_eq!(idle["monitor"]["collection"]["phase"], "idle");
        assert_eq!(idle["monitor"]["collection"]["phase_ms"], 0);
    }

    #[test]
    fn error_details_include_observed_events_beyond_summary_limit() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), true, None);
        let timestamp = stamp(now());
        for index in 0..1001 {
            let event = json!({"timestamp":timestamp,"source":if index < 500 {"codex_core"} else {"codex_desktop"},
                "category":"runtime","severity":"error","code":"fixture","record_id":index,
                "content_id":format!("{index:064x}")});
            if index < 500 { collector.core.errors.push_back(event); } else { collector.desktop.errors.push(event); }
        }
        collector.updated = Some(now());
        let snapshots = collector.snapshots("ok", "ok", 1);
        let value: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(value["errors"]["events"].as_array().unwrap().len(), 1000);
        assert_eq!(snapshots.error_details.len(), 1001);
        assert!(snapshots.error_details.contains_key(&format!("{:064x}", 1000)));
    }

    #[test]
    fn monitor_errors_remain_visible_when_log_and_error_observation_are_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        collector.applied_errors_enabled = false;
        collector.logs_enabled = false;
        let mut journal = collector.journal.lock().unwrap();
        journal.set_enabled(false);
        assert!(journal.http_error(503));
        assert_eq!(
            journal.frontend_failed(json!({"phase":"script","error_type":"TypeError","frontend_revision":null,"frames":[]})),
            Ok(true)
        );
        drop(journal);
        let windows = collector.error_windows("disabled", now(), &BTreeMap::new(), &[]);
        let summary = &windows[3].0;
        assert_eq!(summary["enabled"], false);
        assert_eq!(summary["total"], 2);
        let events = summary["events"].as_array().unwrap();
        assert!(events.iter().any(|event| event["reason"] == "http_response_error"
            && event["http_status"] == 503 && event["code"] == "http_error"));
        assert!(events.iter().any(|event| event["reason"] == "frontend_error"
            && event["code"] == "TypeError"));
    }

    #[test]
    fn collection_heartbeat_is_emitted_only_after_interval() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        collector.journal.lock().unwrap().set_enabled(false);
        collector.record_monitor_sample(&mut json!({}));
        assert!(!collector
            .journal
            .lock()
            .unwrap()
            .events()
            .iter()
            .any(|event| event["kind"] == "heartbeat"));
        collector.last_heartbeat = std::time::Instant::now() - std::time::Duration::from_secs(61);
        collector.record_monitor_sample(&mut json!({}));
        collector.record_monitor_sample(&mut json!({}));
        let events = collector.journal.lock().unwrap().events();
        assert_eq!(
            events
                .iter()
                .filter(|event| event["kind"] == "heartbeat")
                .count(),
            1
        );
        assert_eq!(collector.monitor_history.len(), 3);
    }

    #[test]
    fn initial_snapshot_reports_persisted_retention_without_an_activity_view() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = lam_history_store::Store::open(directory.path(), lam_history_store::Access::ReadWrite).unwrap().unwrap();
        store.save_history(lam_history_store::Namespace::Activity, r#"{"version":3,"retention_days":7}"#, 0, None, &[]).unwrap();
        drop(store);
        for enabled in [false, true] {
            let collector = Collector::new(directory.path().to_path_buf(), enabled, None);
            assert!(collector.activity_view.is_none());
            let snapshots = collector.snapshots(if enabled { "waiting" } else { "disabled" }, "starting", 0);
            for window in 0..4 {
                let value: Value = serde_json::from_slice(&snapshots.window_bytes(window)).unwrap();
                assert_eq!(value["settings"]["activity_retention_days"], 7);
            }
            assert_eq!(snapshots.retention_days, Some(7));
        }
    }

    #[cfg(unix)]
    #[test]
    fn initial_snapshot_omits_retention_when_storage_is_unsafe() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join(lam_history_store::FILE_NAME)).unwrap();
        let collector = Collector::new(directory.path().to_path_buf(), true, None);
        let snapshots = collector.snapshots("waiting", "starting", 0);
        let value: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert!(value["settings"].get("activity_retention_days").is_none());
        assert_eq!(snapshots.retention_days, None);
    }

    #[test]
    fn monitor_history_respects_selected_time_window() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        let reference = now();
        collector.updated = Some(reference);
        collector.monitor_history.extend([
            json!({"time":stamp(reference - Duration::hours(2)),"refresh_ms":10}),
            json!({"time":stamp(reference - Duration::minutes(30)),"refresh_ms":20}),
            json!({"time":null,"refresh_ms":30}),
        ]);
        let snapshots = collector.snapshots("disabled", "disabled", 1);
        let hour: Value = serde_json::from_slice(&snapshots.window_bytes(0)).unwrap();
        let day: Value = serde_json::from_slice(&snapshots.window_bytes(1)).unwrap();
        let all: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(hour["monitor"]["history"].as_array().unwrap().len(), 1);
        assert_eq!(day["monitor"]["history"].as_array().unwrap().len(), 2);
        assert_eq!(all["monitor"]["history"].as_array().unwrap().len(), 3);
        assert_eq!(hour["monitor"]["refresh_ms"], 30);
        assert_eq!(hour["monitor"]["window"], "1h");
        assert_eq!(hour["monitor"]["sample_scope"], "retained_samples_in_selected_window");
        assert_eq!(hour["monitor"]["unknown_timestamp"], "all_only");
    }

    #[test]
    fn live_uptime_updates_only_the_monitor_field() {
        let directory = tempfile::tempdir().unwrap();
        let collector = Collector::new(directory.path().to_path_buf(), false, None);
        let mut snapshots = collector.snapshots("disabled", "disabled", 0);
        let mut value: Value = serde_json::from_slice(&snapshots.windows[3]).unwrap();
        value["codex"]["uptime_seconds"] = json!(42);
        snapshots.windows[3] = value.to_string().into();
        snapshots.started_instant = std::time::Instant::now() - std::time::Duration::from_secs(3);
        let actual: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(actual["codex"]["uptime_seconds"], 42);
        assert!(actual["monitor"]["uptime_seconds"].as_u64().unwrap() >= 3);
    }

    #[test]
    fn initial_pending_files_only_counts_unopened_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        let path = directory.path().join("rollout-pending.jsonl");
        collector.files.insert(path.clone(), Session::new(&path));
        let snapshots = collector.snapshots("waiting", "waiting", 0);
        let value: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(value["codex"]["read_state"]["initial_pending_files"], 1);
        collector.files.get_mut(&path).unwrap().identity = Some((1, 1));
        let snapshots = collector.snapshots("waiting", "waiting", 1);
        let value: Value = serde_json::from_slice(&snapshots.window_bytes(3)).unwrap();
        assert_eq!(value["codex"]["read_state"]["initial_pending_files"], 0);
    }

    #[test]
    fn monitor_history_counts_loaded_activity_by_source() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        collector.activity_history = Arc::new(vec![
            json!({"engine":"sqlite","statement":"SELECT"}),
            json!({"server":"web","tool":"search"}),
            json!({"server":"fixture","tool":"query"}),
            json!({"server":"fixture","tool":"query"}),
        ]);
        let mut metrics = json!({});
        collector.record_monitor_sample(&mut metrics);
        assert_eq!(metrics["sql_records"], 1);
        assert_eq!(metrics["web_records"], 1);
        assert_eq!(metrics["mcp_records"], 2);
        assert_eq!(collector.monitor_history[0]["mcp_records"], 2);
    }

    #[test]
    fn retired_call_count_is_per_process_and_enters_monitor_sample() {
        let directory = tempfile::tempdir().unwrap();
        let mut collector = Collector::new(directory.path().to_path_buf(), false, None);
        let mut projection = Projection::for_id(None);
        for index in 0..=FILE_CALL_LIMIT {
            let line = json!({"type":"response_item","payload":{"type":"function_call","name":"fixture","call_id":format!("call{index}")}}).to_string();
            assert!(projection.consume(
                line.as_bytes(),
                index as u64,
                true,
                true,
                &BTreeMap::new(),
                McpSwitches::default(),
                true,
                true,
                crate::sessions::CommandSwitches::default()
            ));
        }
        assert_eq!(projection.calls.len(), FILE_CALL_LIMIT);
        assert_eq!(projection.newly_trimmed_calls, 1);
        projection.retire_call("unknown");
        assert_eq!(projection.newly_trimmed_calls, 1);
        collector.trimmed_calls += std::mem::take(&mut projection.newly_trimmed_calls);
        let mut metrics = json!({});
        collector.record_monitor_sample(&mut metrics);
        assert_eq!(metrics["trimmed_calls"], 1);
        assert_eq!(collector.monitor_history[0]["trimmed_calls"], 1);
        assert!(!serde_json::to_string(&projection)
            .unwrap()
            .contains("newly_trimmed_calls"));
    }

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
