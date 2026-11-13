use sha2::{Digest, Sha256};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::{core_logs, file_identity, open_source, redirected};

const READ_LIMIT: usize = 1024 * 1024;
pub(super) const FILE_LIMIT: usize = 8;
const INITIAL_TAIL: u64 = 256 * 1024;

#[derive(Clone)]
pub(crate) struct ObservedSource {
    root: PathBuf,
    path: PathBuf,
    identity: (u64, u64),
}

impl ObservedSource {
    pub(crate) fn diagnostic_body(&self, event: &serde_json::Value) -> Option<String> {
        if event["source"] != "codex_desktop"
            || self.path.file_name()?.to_str()? != event["file"].as_str()?
        {
            return None;
        }
        let offset = event["record_offset"].as_u64()?;
        let hash = event["record_hash"].as_str().filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })?;
        let mut file = open_source(&self.root, &self.path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || redirected(&metadata) || file_identity(&metadata) != self.identity
        {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut raw = Vec::new();
        BufReader::new((&mut file).take(65_537))
            .read_until(b'\n', &mut raw)
            .ok()?;
        while raw.last() == Some(&b'\n') {
            raw.pop();
        }
        let line = String::from_utf8_lossy(&raw);
        if format!("{:x}", Sha256::digest(line.as_bytes())) != hash {
            return None;
        }
        let parsed = lam_session_core::desktop_line(&line)?;
        if parsed.module != event["module"].as_str()? {
            return None;
        }
        let timestamp =
            lam_history_store::parse_utc_stamp(parsed.timestamp).map(super::project::stamp)?;
        if event["timestamp"].as_str()? != timestamp {
            return None;
        }
        let after = file.metadata().ok()?;
        let current = std::fs::symlink_metadata(&self.path).ok()?;
        if redirected(&current)
            || file_identity(&current) != self.identity
            || file_identity(&after) != self.identity
        {
            return None;
        }
        Some(parsed.body.to_owned())
    }
    pub(crate) fn error_detail(
        &self,
        event: &serde_json::Value,
        masked: bool,
        full: bool,
    ) -> Option<serde_json::Value> {
        self.diagnostic_body(event)
            .map(|body| super::details::diagnostic_text(&body, masked, full))
    }
}

/// Resolve Desktop diagnostic roots for the explicitly selected Codex home.
#[allow(dead_code)]
pub(super) fn roots(
    selected_home: &Path,
    configured_home: &Path,
    user_home: &Path,
    local_app_data: Option<&Path>,
    xdg_state: Option<&Path>,
    xdg_config: Option<&Path>,
) -> Vec<DesktopRoot> {
    let configured = configured_home
        .strip_prefix("~")
        .map(|suffix| user_home.join(suffix))
        .unwrap_or_else(|_| configured_home.to_path_buf());
    let same_home = selected_home
        .canonicalize()
        .ok()
        .zip(configured.canonicalize().ok())
        .is_some_and(|(selected, configured)| selected == configured);
    if !same_home {
        return vec![DesktopRoot {
            base: selected_home.to_path_buf(),
            path: selected_home.join("desktop-logs"),
        }];
    }
    if cfg!(target_os = "windows") {
        let mut local = local_app_data
            .map(Path::to_path_buf)
            .unwrap_or_else(|| user_home.join("AppData/Local"));
        let redirected = local
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("ac"))
            && local
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| {
                    name.to_string_lossy()
                        .to_ascii_lowercase()
                        .starts_with("sandbox.")
                })
            && local
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("packages"));
        if redirected {
            if let Some(parent) = local.ancestors().nth(3) {
                local = parent.to_path_buf();
            }
        }
        vec![DesktopRoot {
            path: local.join("Codex/Logs"),
            base: if local_app_data.is_some() {
                local
            } else {
                user_home.to_path_buf()
            },
        }]
    } else if cfg!(target_os = "macos") {
        vec![DesktopRoot {
            base: user_home.to_path_buf(),
            path: user_home.join("Library/Logs/Codex"),
        }]
    } else {
        let state = xdg_state
            .map(Path::to_path_buf)
            .unwrap_or_else(|| user_home.join(".local/state"));
        let config = xdg_config
            .map(Path::to_path_buf)
            .unwrap_or_else(|| user_home.join(".config"));
        vec![
            DesktopRoot {
                path: state.join("Codex/Logs"),
                base: if xdg_state.is_some() {
                    state
                } else {
                    user_home.to_path_buf()
                },
            },
            DesktopRoot {
                path: config.join("Codex/logs"),
                base: if xdg_config.is_some() {
                    config
                } else {
                    user_home.to_path_buf()
                },
            },
        ]
    }
}

/// Resolve the nearest existing safe directory for source creation notifications.
#[allow(dead_code)]
pub(super) fn watch_anchor(base: &Path, root: &Path) -> io::Result<(PathBuf, bool)> {
    root.strip_prefix(base).map_err(io::Error::other)?;
    let mut existing = base;
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(meta) if meta.is_dir() && !redirected(&meta) => break,
            Ok(meta) if redirected(&meta) => {
                if !existing.canonicalize()?.is_dir() {
                    return Err(io::Error::other("Unsafe Desktop watch base"));
                }
                break;
            }
            Ok(_) => return Err(io::Error::other("Unsafe Desktop watch base")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                existing = existing
                    .parent()
                    .ok_or_else(|| io::Error::other("Desktop watch base unavailable"))?;
            }
            Err(error) => return Err(error),
        }
    }
    let relative = root.strip_prefix(existing).map_err(io::Error::other)?;
    let mut anchor = existing.canonicalize()?;
    let mut complete = true;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::other("Invalid Desktop watch path"));
        };
        let next = anchor.join(name);
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if !redirected(&meta) && meta.is_dir() => anchor = next,
            Ok(_) => return Err(io::Error::other("Unsafe Desktop watch path")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                complete = false;
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok((anchor, complete))
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DesktopRoot {
    pub base: PathBuf,
    pub path: PathBuf,
}

#[allow(dead_code)]
pub(super) struct DesktopWatchPlan {
    pub anchor: PathBuf,
    pub target: PathBuf,
    pub complete: bool,
    identity: (u64, u64),
}

#[allow(dead_code)]
impl DesktopWatchPlan {
    pub fn verify(&self, source: &DesktopRoot) -> io::Result<bool> {
        let current = watch_plan(source)?;
        Ok(current.anchor == self.anchor
            && current.target == self.target
            && current.complete == self.complete
            && current.identity == self.identity)
    }
}

#[allow(dead_code)]
pub(super) fn watch_plan(source: &DesktopRoot) -> io::Result<DesktopWatchPlan> {
    let (anchor, complete) = watch_anchor(&source.base, &source.path)?;
    let ancestor = source
        .path
        .ancestors()
        .find(|candidate| candidate.canonicalize().ok().as_deref() == Some(anchor.as_path()))
        .ok_or_else(|| io::Error::other("Desktop watch path changed"))?;
    let suffix = source
        .path
        .strip_prefix(ancestor)
        .map_err(io::Error::other)?;
    let metadata = std::fs::symlink_metadata(&anchor)?;
    if redirected(&metadata) || !metadata.is_dir() {
        return Err(io::Error::other("Desktop watch path changed"));
    }
    Ok(DesktopWatchPlan {
        target: anchor.join(suffix),
        identity: file_identity(&metadata),
        anchor,
        complete,
    })
}

/// An independent watcher owns only its own registrations.
#[allow(dead_code)]
pub(super) struct DesktopWatcher {
    watcher: notify::RecommendedWatcher,
    pub plans: Vec<DesktopWatchPlan>,
    pub partial_failure: bool,
    changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[allow(dead_code)]
impl DesktopWatcher {
    pub fn new(
        sources: &[DesktopRoot],
        wake: std::sync::mpsc::SyncSender<()>,
        failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> io::Result<Self> {
        Self::new_with_signal(sources, wake, failed, None)
    }

    pub fn new_with_signal(
        sources: &[DesktopRoot],
        wake: std::sync::mpsc::SyncSender<()>,
        failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
        dirty: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> io::Result<Self> {
        use notify::{Config, RecursiveMode, Watcher};
        use std::sync::atomic::Ordering;
        use std::sync::Arc;

        let mut partial_failure = false;
        let mut plans: Vec<_> = sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| match watch_plan(source) {
                Ok(plan) => Some((index, plan)),
                Err(_) => {
                    partial_failure = true;
                    None
                }
            })
            .collect();
        let filters = Arc::new(
            plans
                .iter()
                .map(|(_, plan)| (plan.anchor.clone(), plan.target.clone(), plan.complete))
                .collect::<Vec<_>>(),
        );
        let callback_filters = Arc::clone(&filters);
        let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_changed = Arc::clone(&changed);
        let mut watcher = notify::RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                let relevant = match result {
                    Ok(event) => {
                        if event.need_rescan() || event.kind.is_create() || event.kind.is_remove() {
                            callback_changed.store(true, Ordering::Release);
                        }
                        event.need_rescan()
                            || (!event.kind.is_access()
                                && callback_filters.iter().any(|(anchor, target, complete)| {
                                    event
                                        .paths
                                        .iter()
                                        .any(|path| related_event(anchor, target, *complete, path))
                                }))
                    }
                    Err(_) => {
                        failed.store(true, Ordering::Release);
                        true
                    }
                };
                if relevant {
                    if let Some(dirty) = &dirty {
                        dirty.store(true, Ordering::Release);
                    }
                    let _ = wake.try_send(());
                }
            },
            Config::default().with_follow_symlinks(false),
        )
        .map_err(io::Error::other)?;
        let mut registrations = std::collections::BTreeMap::<PathBuf, bool>::new();
        for (_, plan) in &plans {
            registrations
                .entry(plan.anchor.clone())
                .and_modify(|recursive| *recursive |= plan.complete)
                .or_insert(plan.complete);
        }
        let mut failed_anchors = std::collections::BTreeSet::new();
        for (anchor, recursive) in registrations {
            if watcher
                .watch(
                    &anchor,
                    if recursive {
                        RecursiveMode::Recursive
                    } else {
                        RecursiveMode::NonRecursive
                    },
                )
                .is_err()
            {
                failed_anchors.insert(anchor);
            }
        }
        plans.retain(|(index, plan)| {
            let valid = !failed_anchors.contains(&plan.anchor)
                && plan.verify(&sources[*index]).unwrap_or(false);
            partial_failure |= !valid;
            valid
        });
        Ok(Self {
            watcher,
            plans: plans.into_iter().map(|(_, plan)| plan).collect(),
            partial_failure,
            changed,
        })
    }

    pub fn needs_reenroll(&self, sources: &[DesktopRoot]) -> bool {
        self.changed
            .swap(false, std::sync::atomic::Ordering::AcqRel)
            || self.partial_failure
            || self.plans.len() != sources.len()
            || self
                .plans
                .iter()
                .zip(sources)
                .any(|(plan, source)| !plan.verify(source).unwrap_or(false))
    }

    pub fn reenroll(
        &mut self,
        sources: &[DesktopRoot],
        wake: std::sync::mpsc::SyncSender<()>,
        failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> io::Result<()> {
        let replacement = Self::new(sources, wake, failed)?;
        *self = replacement;
        Ok(())
    }
}

/// Match only the watched source tree or its next missing path component.
#[allow(dead_code)]
pub(super) fn related_event(anchor: &Path, target: &Path, complete: bool, path: &Path) -> bool {
    if complete {
        return path.starts_with(anchor);
    }
    if path == anchor {
        return true;
    }
    let Ok(relative) = target.strip_prefix(anchor) else {
        return false;
    };
    relative
        .components()
        .next()
        .is_some_and(|component| anchor.join(component) == path)
}

fn io_error_type(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "PermissionError",
        io::ErrorKind::NotFound => "FileNotFoundError",
        _ => "OSError",
    }
}

/// Discover the eight most recently modified regular Desktop logs without following links.
#[allow(dead_code)]
pub(super) fn discover(roots: &[DesktopRoot]) -> (Vec<(PathBuf, PathBuf)>, Option<&'static str>) {
    let mut found = Vec::new();
    let mut error_type = None;
    let mut visited = 0usize;
    for source in roots {
        let root = &source.path;
        let anchor = match watch_anchor(&source.base, root) {
            Ok((anchor, true)) => anchor,
            Ok((_, false)) => continue,
            Err(error) => {
                error_type = Some(io_error_type(&error));
                continue;
            }
        };
        let base = match source.base.canonicalize() {
            Ok(base) if anchor.starts_with(&base) => base,
            Ok(_) => {
                error_type = Some("OSError");
                continue;
            }
            Err(error) => {
                error_type = Some(io_error_type(&error));
                continue;
            }
        };
        let root = &anchor;
        let root_meta = match std::fs::symlink_metadata(root) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                error_type = Some(io_error_type(&error));
                continue;
            }
        };
        if redirected(&root_meta) || !root_meta.is_dir() {
            error_type = Some("OSError");
            continue;
        }
        for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter() {
            visited += 1;
            if visited > 40_000 {
                error_type = Some("OSError");
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    error_type = Some(error.io_error().map_or("OSError", io_error_type));
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if !name.starts_with("codex-desktop-") || !name.ends_with(".log") {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(error) => {
                    error_type = Some(error.io_error().map_or("OSError", io_error_type));
                    continue;
                }
            };
            if redirected(&meta) {
                continue;
            }
            found.push((
                meta.modified().ok(),
                entry.path().to_path_buf(),
                base.clone(),
            ));
        }
    }
    found.sort_unstable_by(|a, b| b.cmp(a));
    found.truncate(FILE_LIMIT);
    (
        found
            .into_iter()
            .map(|(_, path, root)| (root, path))
            .collect(),
        error_type,
    )
}

/// Bounded live reader, owned by one selected Desktop log.
#[derive(Default)]
#[allow(dead_code)]
pub(super) struct DesktopFile {
    identity: Option<(u64, u64)>,
    stream: lam_session_core::DesktopStream,
    history_cursor: u64,
    unread_bytes: u64,
    read_lines: u64,
    parsed_lines: u64,
    unsupported_lines: u64,
    oversized_lines: u64,
}

#[allow(dead_code)]
impl DesktopFile {
    pub fn read(
        &mut self,
        root: &Path,
        path: &Path,
        budget: usize,
    ) -> io::Result<(usize, Vec<core_logs::Projected>)> {
        let mut file = open_source(root, path)?;
        let metadata = file.metadata()?;
        if redirected(&metadata) || !metadata.is_file() {
            return Err(io::Error::other("Unsafe Desktop log"));
        }
        let identity = file_identity(&metadata);
        let size = metadata.len();
        if self.identity != Some(identity) || size < self.stream.offset() {
            self.history_cursor = size.saturating_sub(INITIAL_TAIL);
            self.stream.reset(self.history_cursor);
            self.identity = Some(identity);
        }
        file.seek(SeekFrom::Start(self.stream.offset()))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(budget.min(READ_LIMIT) as u64)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        let current = std::fs::symlink_metadata(path)?;
        if redirected(&current)
            || file_identity(&current) != identity
            || file_identity(&after) != identity
            || after.len() < size
            || (after.len() == size && after.modified().ok() != metadata.modified().ok())
        {
            return Err(io::Error::other("Desktop log changed during read"));
        }
        let lines = self
            .stream
            .push(&bytes)
            .ok_or_else(|| io::Error::other("Invalid Desktop read"))?;
        if self.history_cursor != 0 && self.stream.skipped() > 0 {
            self.history_cursor += self.stream.skipped() as u64;
        }
        self.unread_bytes = after.len().saturating_sub(self.stream.offset());
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io::Error::other("Invalid Desktop filename"))?;
        self.read_lines = self.read_lines.saturating_add(lines.len() as u64);
        self.oversized_lines = self
            .oversized_lines
            .saturating_add(self.stream.oversized() as u64);
        let lines_count = lines.len();
        let projected: Vec<_> = lines
            .into_iter()
            .filter_map(|(offset, line)| core_logs::project_desktop_line(&line, name, offset))
            .collect();
        self.parsed_lines = self.parsed_lines.saturating_add(projected.len() as u64);
        self.unsupported_lines = self
            .unsupported_lines
            .saturating_add((lines_count - projected.len()) as u64);
        Ok((bytes.len(), projected))
    }
    pub fn backfill(
        &mut self,
        root: &Path,
        path: &Path,
        budget: usize,
        now: chrono::DateTime<chrono::Utc>,
    ) -> io::Result<(usize, Vec<core_logs::Projected>)> {
        let end = self.history_cursor;
        if end == 0 || budget == 0 {
            return Ok((0, Vec::new()));
        }
        let mut file = open_source(root, path)?;
        let metadata = file.metadata()?;
        if redirected(&metadata)
            || !metadata.is_file()
            || self.identity != Some(file_identity(&metadata))
            || metadata.len() < end
        {
            return Err(io::Error::other("Desktop log changed during backfill"));
        }
        let begin = end.saturating_sub(budget.min(READ_LIMIT).min(INITIAL_TAIL as usize) as u64);
        file.seek(SeekFrom::Start(begin))?;
        let mut bytes = Vec::new();
        (&mut file).take(end - begin).read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        let current = std::fs::symlink_metadata(path)?;
        if bytes.len() as u64 != end - begin
            || redirected(&current)
            || file_identity(&current) != file_identity(&metadata)
            || file_identity(&after) != file_identity(&metadata)
            || after.len() != metadata.len()
            || after.modified().ok() != metadata.modified().ok()
        {
            return Err(io::Error::other("Desktop log changed during backfill"));
        }
        let start = if begin > 0 {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |position| position + 1)
        } else {
            0
        };
        let mut range = start..bytes.len();
        let mut read_lines = 0u64;
        let mut parsed_lines = 0u64;
        let mut unsupported_lines = 0u64;
        let mut oversized_lines = 0u64;
        let mut projected = Vec::new();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io::Error::other("Invalid Desktop filename"))?;
        let mut expired = false;
        while range.start < range.end {
            let chunk = &bytes[range.clone()];
            let width = chunk
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1);
            let offset = range.start + width;
            range.end = range.start + width.saturating_sub(1);
            let line = &chunk[width..];
            if line.len() > 65_536 {
                oversized_lines += 1;
                continue;
            }
            read_lines += 1;
            let line = String::from_utf8_lossy(line);
            if let Some(event) = core_logs::project_desktop_line(&line, name, begin + offset as u64)
            {
                if event.log["timestamp"]
                    .as_str()
                    .and_then(lam_history_store::parse_utc_stamp)
                    .is_some_and(|timestamp| timestamp < now - chrono::Duration::hours(24))
                {
                    expired = true;
                    break;
                }
                parsed_lines += 1;
                if event.severity.is_some() || event.sql.is_some() || event.api.is_some() {
                    projected.push(event);
                }
            } else {
                unsupported_lines += 1;
            }
        }
        self.unsupported_lines = self.unsupported_lines.saturating_add(unsupported_lines);
        self.read_lines = self.read_lines.saturating_add(read_lines);
        self.parsed_lines = self.parsed_lines.saturating_add(parsed_lines);
        self.oversized_lines = self.oversized_lines.saturating_add(oversized_lines);
        self.history_cursor = if expired {
            0
        } else if begin > 0 && start < bytes.len() {
            begin + start as u64
        } else {
            begin
        };
        Ok((bytes.len(), projected))
    }
}

/// One bounded pass across the currently selected Desktop files.
#[allow(dead_code)]
#[derive(Default)]
pub(super) struct DesktopCollector {
    files: std::collections::BTreeMap<PathBuf, DesktopFile>,
    cursor: usize,
    pending: Option<DesktopBatch>,
    bytes_read: u64,
    observed_sources: Vec<ObservedSource>,
    capture_errors: Option<bool>,
    capture_sql: Option<bool>,
    capture_api: Option<bool>,
    read_lines: u64,
    parsed_lines: u64,
    unsupported_lines: u64,
    oversized_lines: u64,
    pub logs: Vec<serde_json::Value>,
    pub errors: Vec<serde_json::Value>,
    pub api_events: Vec<serde_json::Value>,
    pub sql: Vec<serde_json::Value>,
    pub source: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Clone, Default)]
pub(super) struct DesktopBatch {
    pub live: Vec<core_logs::Projected>,
    pub history: Vec<core_logs::Projected>,
    pub unavailable: bool,
    pub backfill_pending: bool,
    pub live_pending: bool,
    pub bytes_read: usize,
    pub source: serde_json::Value,
    capture_errors: bool,
    capture_sql: bool,
    capture_api: bool,
}

#[allow(dead_code)]
impl DesktopCollector {
    pub(super) fn source_locations(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    pub fn set_capture_errors(&mut self, enabled: bool) {
        self.capture_errors = Some(enabled);
    }
    pub fn set_capture_sql(&mut self, enabled: bool) {
        self.capture_sql = Some(enabled);
    }
    pub fn set_capture_api(&mut self, enabled: bool) {
        self.capture_api = Some(enabled);
    }
    pub fn observed_sources(&self) -> Vec<ObservedSource> {
        self.observed_sources.clone()
    }
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }
    pub fn reset_log_scan(&mut self) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.files.clear();
        self.observed_sources.clear();
        self.cursor = 0;
        true
    }

    pub fn refresh(
        &mut self,
        roots: &[DesktopRoot],
        now: chrono::DateTime<chrono::Utc>,
    ) -> DesktopBatch {
        if let Some(batch) = &self.pending {
            return batch.clone();
        }
        let (selected, mut error_type) = discover(roots);
        let unavailable = error_type.is_some();
        if !unavailable {
            self.files
                .retain(|path, _| selected.iter().any(|(_, selected)| selected == path));
        }
        let mut batch = DesktopBatch {
            unavailable,
            capture_errors: self.capture_errors.unwrap_or(true),
            capture_sql: self.capture_sql.unwrap_or(true),
            capture_api: self.capture_api.unwrap_or(true),
            ..DesktopBatch::default()
        };
        let count = selected.len();
        let start = self.cursor % count.max(1);
        self.cursor = self.cursor.wrapping_add(1);
        let mut live_budget = READ_LIMIT;
        for step in 0..count {
            let (root, path) = &selected[(start + step) % count];
            if unavailable && !self.files.contains_key(path) && self.files.len() >= 8 {
                continue;
            }
            let file = self.files.entry(path.clone()).or_default();
            if live_budget == 0 {
                batch.live_pending = true;
                continue;
            }
            let before = (
                file.read_lines,
                file.parsed_lines,
                file.oversized_lines,
                file.unsupported_lines,
            );
            match file.read(root, path, live_budget) {
                Ok((read, events)) => {
                    live_budget -= read;
                    batch.bytes_read += read;
                    batch.live.extend(events);
                    self.read_lines += file.read_lines - before.0;
                    self.parsed_lines += file.parsed_lines - before.1;
                    self.oversized_lines += file.oversized_lines - before.2;
                    self.unsupported_lines += file.unsupported_lines - before.3;
                    batch.live_pending |= file.unread_bytes > 0;
                }
                Err(error) => {
                    batch.unavailable = true;
                    error_type = Some(io_error_type(&error));
                }
            }
        }
        let mut history_budget = READ_LIMIT;
        for step in 0..count {
            let (root, path) = &selected[(start + step) % count];
            let Some(file) = self.files.get_mut(path) else {
                continue;
            };
            if history_budget != 0 {
                let before = (
                    file.read_lines,
                    file.parsed_lines,
                    file.oversized_lines,
                    file.unsupported_lines,
                );
                match file.backfill(root, path, history_budget, now) {
                    Ok((read, events)) => {
                        history_budget -= read;
                        batch.bytes_read += read;
                        batch.history.extend(events);
                        self.read_lines += file.read_lines - before.0;
                        self.parsed_lines += file.parsed_lines - before.1;
                        self.oversized_lines += file.oversized_lines - before.2;
                        self.unsupported_lines += file.unsupported_lines - before.3;
                    }
                    Err(error) => {
                        batch.unavailable = true;
                        error_type = Some(io_error_type(&error));
                    }
                }
            }
            batch.backfill_pending |= file.history_cursor > 0;
        }
        self.bytes_read = self.bytes_read.saturating_add(batch.bytes_read as u64);
        self.observed_sources = selected
            .iter()
            .filter_map(|(root, path)| {
                let identity = self.files.get(path)?.identity?;
                Some(ObservedSource {
                    root: root.clone(),
                    path: path.clone(),
                    identity,
                })
            })
            .collect();
        let files: Vec<_> = selected
            .iter()
            .filter_map(|(_, path)| {
                let state = self.files.get(path)?;
                let name = path.file_name()?.to_str()?;
                Some(serde_json::json!({
                    "name": name,
                    "offset": state.identity.map(|_| state.stream.offset()),
                    "pending_bytes": state.stream.pending_bytes(),
                    "unread_bytes": state.identity.map(|_| state.unread_bytes),
                }))
            })
            .collect();
        let health = match (batch.unavailable, count > 0) {
            (true, true) => "partly_unavailable",
            (true, false) => "unavailable",
            (false, true) => "ok",
            (false, false) => "missing",
        };
        let backfill_bytes = self.files.values().fold(0u64, |total, file| {
            total.saturating_add(file.history_cursor)
        });
        batch.source = serde_json::json!({
            "source":"codex_desktop", "health":health, "checked_at":super::project::stamp(now),
            "error_type":error_type,
            "read_bytes":self.bytes_read, "read_lines":self.read_lines, "parsed_lines":self.parsed_lines,
            "unsupported_lines":self.unsupported_lines,
            "oversized_lines":self.oversized_lines, "backfill_pending":backfill_bytes,
            "live_pending":batch.live_pending,
            "files":files, "read_limit":READ_LIMIT * 2, "read_unit":"bytes", "history_hours":24,
            "storage_health":self.source.get("storage_health").and_then(|value| value.as_str()).unwrap_or("waiting"),
            "storage_error":self.source.get("storage_error"),
        });
        if !batch.live.is_empty() || !batch.history.is_empty() {
            self.pending = Some(batch.clone());
        } else {
            self.source = batch.source.clone();
        }
        batch
    }

    pub fn commit(
        &mut self,
        checkpoints: &mut super::checkpoint::Checkpoints,
        now: chrono::DateTime<chrono::Utc>,
    ) -> lam_history_store::Result<Option<DesktopBatch>> {
        let Some(batch) = &self.pending else {
            return Ok(None);
        };
        let mut sql = Vec::new();
        let mut errors = Vec::new();
        for event in batch.live.iter().chain(&batch.history) {
            if let Some(value) = event.sql.as_ref().filter(|_| batch.capture_sql) {
                sql.push(value.clone());
            }
            if let Some(severity) = event.severity.filter(|_| batch.capture_errors) {
                let mut value = event.log.clone();
                value["severity"] = severity.into();
                errors.push(value);
            }
        }
        if !sql.is_empty() || !errors.is_empty() {
            if let Err(error) = checkpoints.save_diagnostics(&sql, &errors, now) {
                self.source = batch.source.clone();
                self.source["storage_health"] = "unavailable".into();
                self.source["storage_error"] = super::storage_error_type(&error).into();
                return Err(error);
            }
        }
        let batch = self.pending.take().expect("pending batch");
        self.source = batch.source.clone();
        self.source["storage_health"] = "ok".into();
        self.source["storage_error"] = serde_json::Value::Null;
        for event in &batch.live {
            if !self
                .logs
                .iter()
                .any(|item| item["content_id"] == event.log["content_id"])
            {
                self.logs.push(event.log.clone());
            }
        }
        for event in batch.live.iter().chain(&batch.history) {
            if let Some(sql) = event.sql.as_ref().filter(|_| batch.capture_sql) {
                if let Some(id) = lam_history_store::sql_event_id(sql) {
                    if !self.sql.iter().any(|old| {
                        lam_history_store::sql_event_id(old).as_deref() == Some(id.as_str())
                    }) {
                        self.sql.push(sql.clone());
                    }
                }
            }
            if let Some(severity) = event.severity.filter(|_| batch.capture_errors) {
                if !self
                    .errors
                    .iter()
                    .any(|item| item["content_id"] == event.log["content_id"])
                {
                    let mut error = event.log.clone();
                    error["severity"] = severity.into();
                    self.errors.push(error);
                }
            }
            if let Some(api) = event.api.as_ref().filter(|_| batch.capture_api) {
                if !self
                    .api_events
                    .iter()
                    .any(|item| item["observation_id"] == api["observation_id"])
                {
                    self.api_events.push(api.clone());
                }
            }
        }
        for values in [&mut self.logs, &mut self.errors, &mut self.api_events] {
            values.sort_by(|left, right| {
                right["timestamp"].as_str().cmp(&left["timestamp"].as_str())
            });
            values.truncate(1000);
        }
        self.sql
            .sort_by(|a, b| b["timestamp"].as_str().cmp(&a["timestamp"].as_str()));
        self.sql.truncate(500);
        Ok(Some(batch))
    }

    #[cfg(test)]
    pub fn acknowledge(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopFile;
    use sha2::Digest;
    use std::io::Write;

    #[test]
    fn desktop_detail_rejects_changed_line_and_replaced_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = root.join("codex-desktop-fixture.log");
        let line = "2026-11-12T01:02:03.123Z ERROR [codex] value1";
        std::fs::write(&path, format!("{line}\n")).unwrap();
        let identity = super::file_identity(&std::fs::symlink_metadata(&path).unwrap());
        let source = super::ObservedSource {
            root: root.clone(),
            path: path.clone(),
            identity,
        };
        let event = serde_json::json!({"source":"codex_desktop","file":"codex-desktop-fixture.log","record_offset":0,"record_hash":format!("{:x}", sha2::Sha256::digest(line.as_bytes())),"module":"codex","timestamp":"2026-11-12T01:02:03.123Z"});
        assert_eq!(
            source.error_detail(&event, true, false).unwrap()["text"],
            "value1"
        );
        std::fs::write(&path, format!("{}\n", line.replace("value1", "value2"))).unwrap();
        assert!(source.error_detail(&event, true, false).is_none());
        std::fs::rename(&path, root.join("old.log")).unwrap();
        std::fs::write(&path, format!("{line}\n")).unwrap();
        assert!(source.error_detail(&event, true, false).is_none());
    }

    fn source(path: &std::path::Path) -> super::DesktopRoot {
        super::DesktopRoot {
            base: path.parent().unwrap().to_path_buf(),
            path: path.to_path_buf(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn discovery_limits_files_and_excludes_symlinks() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        for index in 0..10 {
            std::fs::write(
                folder.path().join(format!("codex-desktop-{index}.log")),
                b"",
            )
            .unwrap();
        }
        symlink(
            folder.path().join("codex-desktop-0.log"),
            folder.path().join("codex-desktop-link.log"),
        )
        .unwrap();
        let (selected, unavailable) = super::discover(&[source(folder.path())]);
        assert!(unavailable.is_none());
        assert_eq!(selected.len(), 8);
        assert!(selected
            .iter()
            .all(|(_, path)| !path.ends_with("codex-desktop-link.log")));
    }

    #[test]
    fn backfill_rejects_replacement_without_advancing_cursor() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("codex-desktop-fixture.log");
        std::fs::write(&path, vec![b'x'; 300_000]).unwrap();
        let mut reader = DesktopFile::default();
        reader.read(folder.path(), &path, 1).unwrap();
        let cursor = reader.history_cursor;
        let replacement = folder.path().join("replacement");
        std::fs::write(&replacement, vec![b'y'; 300_000]).unwrap();
        std::fs::rename(replacement, &path).unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        assert!(reader.backfill(folder.path(), &path, 1024, now).is_err());
        assert_eq!(reader.history_cursor, cursor);
    }

    #[test]
    fn backfill_recovers_line_crossing_initial_tail_boundary() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("codex-desktop-fixture.log");
        let crossing = "2026-11-12T00:00:00Z WARN [codex] crossing\n";
        let filler = "x".repeat(256 * 1024 - crossing.len() / 2);
        let padding = "2026-11-12T00:00:01Z INFO [codex] x\n".repeat(10);
        let data = format!("{padding}{crossing}{filler}");
        let boundary = data.len() - 256 * 1024;
        assert!(boundary > padding.len() && boundary < padding.len() + crossing.len());
        std::fs::write(&path, &data).unwrap();
        let mut reader = DesktopFile::default();
        let (_, live) = reader.read(folder.path(), &path, 1024 * 1024).unwrap();
        assert!(!live
            .iter()
            .any(|event| event.log["record_offset"] == padding.len() as u64));
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        let (_, history) = reader
            .backfill(folder.path(), &path, 1024 * 1024, now)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].log["record_offset"], padding.len() as u64);
    }

    #[test]
    fn backfill_preserves_info_sql_and_model_api_diagnostics() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("codex-desktop-fixture.log");
        let recent = concat!(
            "2026-11-12T00:00:00Z INFO [sqlx::query] summary=\"SELECT private\" elapsed=2ms\n",
            "2026-11-12T00:00:01Z INFO [codex_otel.log_only] event.name=\"codex.api_request\" status_code=200\n",
        );
        let filler = "2026-11-12T00:00:02Z INFO [codex] x\n".repeat(8_000);
        std::fs::write(&path, format!("{recent}{filler}")).unwrap();
        let mut reader = DesktopFile::default();
        reader.read(folder.path(), &path, 1024 * 1024).unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        let (_, history) = reader
            .backfill(folder.path(), &path, 1024 * 1024, now)
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            history.iter().filter(|event| event.sql.is_some()).count(),
            1
        );
        assert_eq!(
            history.iter().filter(|event| event.api.is_some()).count(),
            1
        );
        assert!(history.iter().all(|event| event.severity.is_none()));
    }

    #[test]
    fn disabled_model_api_batch_does_not_capture_events_after_toggle() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::write(logs.join("codex-desktop-fixture.log"), "2026-11-12T00:00:01Z ERROR [codex_otel.log_only] event.name=\"codex.api_request\" status_code=200\n").unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        collector.set_capture_api(false);
        collector.refresh(&[source(&logs)], now);
        collector.set_capture_api(true);
        let mut checkpoints =
            super::super::checkpoint::Checkpoints::new(folder.path().join("monitoring"), true);
        collector.commit(&mut checkpoints, now).unwrap().unwrap();
        assert!(collector.api_events.is_empty());
        assert_eq!(collector.errors.len(), 1);
    }

    #[test]
    fn committed_backfill_keeps_sql_and_model_api_without_live_logs() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        let recent = concat!(
            "2026-11-12T00:00:00Z INFO [sqlx::query] summary=\"SELECT PRIVATE\" elapsed=2ms\n",
            "2026-11-12T00:00:01Z INFO [codex_otel.log_only] event.name=\"codex.api_request\" status_code=200\n",
        );
        let filler = "2026-11-12T00:00:02Z INFO [codex] x\n".repeat(8_000);
        std::fs::write(
            logs.join("codex-desktop-fixture.log"),
            format!("{recent}{filler}"),
        )
        .unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        let monitoring = folder.path().join("monitoring");
        let mut collector = super::DesktopCollector::default();
        let batch = collector.refresh(&[source(&logs)], now);
        assert_eq!(batch.history.len(), 2);
        let mut checkpoints = super::super::checkpoint::Checkpoints::new(monitoring.clone(), true);
        collector.commit(&mut checkpoints, now).unwrap().unwrap();
        assert_eq!(collector.api_events.len(), 1);
        assert!(!collector
            .logs
            .iter()
            .any(|event| event["timestamp"] == "2026-11-12T00:00:00.000Z"));
        let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM history_items WHERE namespace='activity' AND section='sql'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn backfill_reads_recent_errors_and_stops_at_older_records() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("codex-desktop-fixture.log");
        let old = "2026-11-10T01:00:00Z ERROR [codex] old\n";
        let recent = "2026-11-12T00:00:00Z WARN [codex] recent\n";
        let padding = "2026-11-12T00:00:01Z INFO [codex] x\n".repeat(8_000);
        std::fs::write(&path, format!("{old}{recent}{padding}")).unwrap();
        let mut reader = DesktopFile::default();
        reader.read(folder.path(), &path, 1024 * 1024).unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T01:00:00Z").unwrap();
        let (_, events) = reader
            .backfill(folder.path(), &path, 1024 * 1024, now)
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log["timestamp"], "2026-11-12T00:00:00.000Z");
        assert_eq!(
            reader.read_lines - reader.parsed_lines - reader.unsupported_lines,
            1
        );
        assert_eq!(reader.history_cursor, 0);
    }

    #[test]
    fn source_error_types_preserve_recognized_io_errors() {
        assert_eq!(
            super::io_error_type(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            "PermissionError"
        );
        assert_eq!(
            super::io_error_type(&std::io::Error::from(std::io::ErrorKind::NotFound)),
            "FileNotFoundError"
        );
        assert_eq!(
            super::io_error_type(&std::io::Error::other("other")),
            "OSError"
        );
    }

    #[test]
    fn collector_processes_readable_root_when_another_root_is_invalid() {
        let folder = tempfile::tempdir().unwrap();
        let valid = folder.path().join("valid");
        std::fs::create_dir(&valid).unwrap();
        std::fs::write(
            valid.join("codex-desktop-fixture.log"),
            b"2026-11-12T01:02:03Z WARN [codex] failed\n",
        )
        .unwrap();
        let invalid = folder.path().join("not-a-directory");
        std::fs::write(&invalid, b"").unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let batch =
            super::DesktopCollector::default().refresh(&[source(&invalid), source(&valid)], now);
        assert!(batch.unavailable);
        assert_eq!(batch.source["health"], "partly_unavailable");
        assert_eq!(batch.source["error_type"], "OSError");
        assert_eq!(batch.source["files"].as_array().unwrap().len(), 1);
        assert!(!batch
            .source
            .to_string()
            .contains(folder.path().to_str().unwrap()));
        assert_eq!(batch.live.len(), 1);
    }

    #[test]
    fn source_counts_valid_invalid_empty_and_oversized_lines() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("logs");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("codex-desktop-fixture.log");
        let mut bytes = b"2026-11-12T01:02:03Z INFO [codex] valid\ninvalid\n\n".to_vec();
        bytes.extend(vec![b'x'; 65_537]);
        std::fs::write(&path, bytes).unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        let first = collector.refresh(&[source(&root)], now);
        assert_eq!(first.source["read_lines"], 3);
        assert_eq!(first.source["parsed_lines"], 1);
        assert_eq!(first.source["unsupported_lines"], 2);
        assert_eq!(first.source["oversized_lines"], 1);
        collector.acknowledge();
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"tail\n")
            .unwrap();
        let second = collector.refresh(&[source(&root)], now);
        assert_eq!(second.source["oversized_lines"], 1);
    }

    #[test]
    fn source_health_updates_without_projected_records() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("logs");
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        collector.refresh(&[source(&root)], now);
        assert_eq!(collector.source["health"], "missing");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("codex-desktop-fixture.log"), b"").unwrap();
        collector.refresh(&[source(&root)], now);
        assert_eq!(collector.source["health"], "ok");
        assert_eq!(collector.source["files"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn failed_commit_retains_unread_progress_for_next_pass() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        let line = b"2026-11-12T01:02:03Z ERROR [codex] failure\n";
        for index in 0..5 {
            let mut bytes = line.to_vec();
            bytes.resize(256 * 1024, b'x');
            std::fs::write(logs.join(format!("codex-desktop-{index}.log")), bytes).unwrap();
        }
        let monitoring = folder.path().join("monitoring");
        drop(
            lam_history_store::Store::open(&monitoring, lam_history_store::Access::ReadWrite)
                .unwrap(),
        );
        let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
        db.execute_batch("CREATE TRIGGER reject_errors BEFORE INSERT ON history_items WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        let first = collector.refresh(&[source(&logs)], now);
        assert!(first.live_pending);
        assert_eq!(first.live.len(), 4);
        let mut checkpoints = super::super::checkpoint::Checkpoints::new(monitoring, true);
        assert!(collector.commit(&mut checkpoints, now).is_err());
        let retry = collector.refresh(&[source(&logs)], now);
        assert!(retry.live_pending);
        assert_eq!(retry.live.len(), 4);
        db.execute_batch("DROP TRIGGER reject_errors").unwrap();
        let committed = collector.commit(&mut checkpoints, now).unwrap().unwrap();
        assert!(committed.live_pending);
        let remaining = collector.refresh(&[source(&logs)], now);
        assert_eq!(remaining.live.len(), 1);
        assert!(!remaining.live_pending);
        collector.commit(&mut checkpoints, now).unwrap();
        assert_eq!(collector.errors.len(), 5);
    }

    #[test]
    fn collector_reports_unread_files_when_live_budget_is_exhausted() {
        let folder = tempfile::tempdir().unwrap();
        for index in 0..5 {
            std::fs::write(
                folder.path().join(format!("codex-desktop-{index}.log")),
                vec![b'x'; 256 * 1024],
            )
            .unwrap();
        }
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        let first = collector.refresh(&[source(folder.path())], now);
        assert!(first.live_pending);
        assert!(first.source["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["offset"].is_null() && file["unread_bytes"].is_null()));
        assert_eq!(first.source["backfill_pending"], 0);
        let second = collector.refresh(&[source(folder.path())], now);
        assert!(!second.live_pending);
    }

    #[test]
    fn collector_reads_selected_files_once_and_tracks_offsets() {
        let folder = tempfile::tempdir().unwrap();
        for index in 0..2 {
            std::fs::write(
                folder.path().join(format!("codex-desktop-{index}.log")),
                b"2026-11-12T01:02:03Z WARN [codex] failed\n",
            )
            .unwrap();
        }
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        let first = collector.refresh(&[source(folder.path())], now);
        assert!(!first.unavailable);
        assert_eq!(first.live.len(), 2);
        assert!(first.source["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["unread_bytes"] == 0));
        let retry = collector.refresh(&[source(folder.path())], now);
        assert_eq!(retry.live.len(), 2);
        assert_eq!(
            retry.live[0].log["content_id"],
            first.live[0].log["content_id"]
        );
        collector.acknowledge();
        let second = collector.refresh(&[source(folder.path())], now);
        assert!(second.live.is_empty());
    }

    #[test]
    fn reprocessing_same_desktop_file_does_not_duplicate_history() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        let path = logs.join("codex-desktop-fixture.log");
        std::fs::write(&path, concat!(
            "2026-11-12T01:02:03Z ERROR [codex] failure\n",
            "2026-11-12T01:02:04Z INFO [sqlx::query] summary=\"SELECT PRIVATE\" elapsed=2ms rows_returned=3\n",
        )).unwrap();
        let monitoring = folder.path().join("monitoring");
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        for _ in 0..2 {
            let mut collector = super::DesktopCollector::default();
            let batch = collector.refresh(&[source(&logs)], now);
            assert_eq!(batch.live.len(), 2);
            let mut checkpoints =
                super::super::checkpoint::Checkpoints::new(monitoring.clone(), true);
            collector.commit(&mut checkpoints, now).unwrap().unwrap();
        }
        let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
        let errors: i64 = db
            .query_row(
                "SELECT count(*) FROM history_items WHERE namespace='errors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sql: i64 = db
            .query_row(
                "SELECT count(*) FROM history_items WHERE namespace='activity' AND section='sql'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((errors, sql), (1, 1));
    }

    #[test]
    fn failed_storage_keeps_pending_batch_for_retry() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::write(
            logs.join("codex-desktop-fixture.log"),
            b"2026-11-12T01:02:03Z ERROR [codex] failed\n",
        )
        .unwrap();
        let monitoring = folder.path().join("monitoring");
        drop(
            lam_history_store::Store::open(&monitoring, lam_history_store::Access::ReadWrite)
                .unwrap(),
        );
        let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
        db.execute_batch("CREATE TRIGGER reject_errors BEFORE INSERT ON history_items WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
        let now = lam_history_store::parse_utc_stamp("2026-11-12T02:00:00Z").unwrap();
        let mut collector = super::DesktopCollector::default();
        assert_eq!(collector.refresh(&[source(&logs)], now).live.len(), 1);
        let mut checkpoints = super::super::checkpoint::Checkpoints::new(monitoring, true);
        assert!(collector.commit(&mut checkpoints, now).is_err());
        assert_eq!(collector.source["health"], "ok");
        assert_eq!(collector.source["storage_health"], "unavailable");
        assert_eq!(collector.source["storage_error"], "SQLiteError");
        assert!(collector.errors.is_empty());
        assert!(collector.logs.is_empty());
        assert_eq!(collector.refresh(&[source(&logs)], now).live.len(), 1);
        db.execute_batch("DROP TRIGGER reject_errors").unwrap();
        let committed = collector.commit(&mut checkpoints, now).unwrap().unwrap();
        assert_eq!(committed.live.len(), 1);
        assert_eq!(collector.source["storage_health"], "ok");
        assert!(collector.source["storage_error"].is_null());
        assert_eq!(collector.errors.len(), 1);
        assert_eq!(collector.logs.len(), 1);
        assert!(collector.refresh(&[source(&logs)], now).live.is_empty());
        assert_eq!(collector.source["storage_health"], "ok");
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM history_items WHERE namespace='errors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn distinct_selected_home_uses_only_its_fixture_root() {
        let root = std::path::Path::new("/fixture");
        let configured = std::path::Path::new("/real-profile");
        let user = std::path::Path::new("/user");
        let paths = super::roots(root, configured, user, None, None, None);
        assert_eq!(paths[0].base, root);
        assert_eq!(paths[0].path, root.join("desktop-logs"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn configured_home_uses_macos_logs_directory() {
        let user = tempfile::tempdir().unwrap();
        let root = user.path().join(".codex");
        std::fs::create_dir(&root).unwrap();
        let paths = super::roots(
            &root,
            std::path::Path::new("~/.codex"),
            user.path(),
            None,
            None,
            None,
        );
        assert_eq!(paths[0].base, user.path());
        assert_eq!(paths[0].path, user.path().join("Library/Logs/Codex"));
    }

    #[cfg(unix)]
    #[test]
    fn configured_symlink_resolves_to_selected_home() {
        use std::os::unix::fs::symlink;
        let user = tempfile::tempdir().unwrap();
        let root = user.path().join("codex");
        std::fs::create_dir(&root).unwrap();
        let link = user.path().join("alias");
        symlink(&root, &link).unwrap();
        let paths = super::roots(&root, &link, user.path(), None, None, None);
        assert_ne!(paths[0].path, root.join("desktop-logs"));
    }

    #[test]
    fn unknown_configured_home_stays_in_selected_fixture() {
        let user = tempfile::tempdir().unwrap();
        let selected = user.path().join("selected");
        std::fs::create_dir(&selected).unwrap();
        let missing = user.path().join("missing");
        let paths = super::roots(&selected, &missing, user.path(), None, None, None);
        assert_eq!(paths[0].base, selected);
        assert_eq!(paths[0].path, selected.join("desktop-logs"));
    }

    #[test]
    fn configured_home_with_dot_components_matches_selected() {
        let user = tempfile::tempdir().unwrap();
        let root = user.path().join("codex");
        std::fs::create_dir(&root).unwrap();
        let configured = user.path().join(".").join("codex");
        let paths = super::roots(&root, &configured, user.path(), None, None, None);
        assert_ne!(paths[0].path, root.join("desktop-logs"));
    }

    #[cfg(unix)]
    #[test]
    fn missing_configured_base_under_symlink_tracks_its_target() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let alias = folder.path().join("alias");
        symlink(&target, &alias).unwrap();
        let base = alias.join("custom");
        let root = base.join("Codex/Logs");
        assert_eq!(
            super::watch_anchor(&base, &root).unwrap(),
            (target.canonicalize().unwrap(), false)
        );
        std::fs::create_dir_all(&root).unwrap();
        let source = super::DesktopRoot {
            base,
            path: root.clone(),
        };
        std::fs::write(root.join("codex-desktop-fixture.log"), b"").unwrap();
        let (found, unavailable) = super::discover(&[source]);
        assert!(unavailable.is_none());
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].0,
            root.parent()
                .unwrap()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
        );
    }

    #[test]
    fn watch_anchor_handles_missing_configured_base() {
        let folder = tempfile::tempdir().unwrap();
        let base = folder.path().join("custom");
        let root = base.join("Codex/Logs");
        assert_eq!(
            super::watch_anchor(&base, &root).unwrap(),
            (folder.path().canonicalize().unwrap(), false)
        );
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            super::watch_anchor(&base, &root).unwrap(),
            (root.canonicalize().unwrap(), true)
        );
    }

    #[cfg(unix)]
    #[test]
    fn watch_plan_maps_symlink_base_to_canonical_event_paths() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let alias = folder.path().join("alias");
        symlink(&target, &alias).unwrap();
        let source = super::DesktopRoot {
            base: alias.join("custom"),
            path: alias.join("custom/Codex/Logs"),
        };
        let plan = super::watch_plan(&source).unwrap();
        assert!(!plan.complete);
        assert_eq!(plan.anchor, target.canonicalize().unwrap());
        assert!(super::related_event(
            &plan.anchor,
            &plan.target,
            plan.complete,
            &plan.anchor.join("custom")
        ));
        assert!(!super::related_event(
            &plan.anchor,
            &plan.target,
            plan.complete,
            &plan.anchor.join("other")
        ));
    }

    #[test]
    fn watch_plan_detects_replaced_directory() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("logs");
        std::fs::create_dir(&root).unwrap();
        let source = super::DesktopRoot {
            base: folder.path().to_path_buf(),
            path: root.clone(),
        };
        let plan = super::watch_plan(&source).unwrap();
        assert!(plan.verify(&source).unwrap());
        std::fs::rename(&root, folder.path().join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(!plan.verify(&source).unwrap());
    }

    #[test]
    fn missing_source_event_filter_uses_only_the_next_component() {
        let anchor = std::path::Path::new("/fixture");
        let target = std::path::Path::new("/fixture/a/b");
        assert!(super::related_event(anchor, target, false, anchor));
        assert!(super::related_event(
            anchor,
            target,
            false,
            std::path::Path::new("/fixture/a")
        ));
        assert!(!super::related_event(
            anchor,
            target,
            false,
            std::path::Path::new("/fixture/other")
        ));
        assert!(super::related_event(
            target,
            target,
            true,
            std::path::Path::new("/fixture/a/b/codex-desktop.log")
        ));
        assert!(!super::related_event(
            target,
            target,
            true,
            std::path::Path::new("/fixture/a/elsewhere")
        ));
    }

    #[test]
    fn watcher_reenrollment_failure_preserves_existing_watch() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        use std::time::Duration;
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("desktop-logs");
        let source = super::DesktopRoot {
            base: folder.path().to_path_buf(),
            path: root.clone(),
        };
        let (send, receive) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let mut watcher =
            super::DesktopWatcher::new(std::slice::from_ref(&source), send.clone(), failed.clone())
                .unwrap();

        std::fs::write(&root, b"not a directory").unwrap();
        assert!(watcher
            .reenroll(std::slice::from_ref(&source), send.clone(), failed.clone())
            .is_err());
        while receive.try_recv().is_ok() {}
        std::fs::remove_file(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        watcher.reenroll(&[source], send, failed).unwrap();
        assert!(watcher.plans[0].complete);
    }

    #[test]
    fn watcher_shares_anchor_without_losing_source_notifications() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        use std::time::Duration;
        let folder = tempfile::tempdir().unwrap();
        let sources: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|name| super::DesktopRoot {
                base: folder.path().to_path_buf(),
                path: folder.path().join(name),
            })
            .collect();
        let (send, receive) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let watcher = super::DesktopWatcher::new(&sources, send, failed).unwrap();
        std::fs::create_dir(&sources[1].path).unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(watcher);
    }

    #[test]
    fn desktop_watcher_keeps_available_source_when_another_is_invalid() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        use std::time::Duration;

        let folder = tempfile::tempdir().unwrap();
        let valid = folder.path().join("valid");
        let invalid = folder.path().join("invalid");
        std::fs::create_dir(&valid).unwrap();
        std::fs::write(&invalid, b"not a directory").unwrap();
        let sources = [
            super::DesktopRoot {
                base: folder.path().to_path_buf(),
                path: valid.clone(),
            },
            super::DesktopRoot {
                base: folder.path().to_path_buf(),
                path: invalid.clone(),
            },
        ];
        let (send, receive) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let watcher = super::DesktopWatcher::new(&sources, send.clone(), failed.clone()).unwrap();
        assert!(watcher.partial_failure);
        std::fs::write(valid.join("codex-desktop-fixture.log"), b"record\n").unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        std::fs::remove_file(&invalid).unwrap();
        std::fs::create_dir(&invalid).unwrap();
        let recovered = super::DesktopWatcher::new(&sources, send, failed).unwrap();
        assert!(!recovered.partial_failure);
    }

    #[test]
    fn desktop_watcher_signals_source_creation() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        use std::time::Duration;
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("desktop-logs");
        let source = super::DesktopRoot {
            base: folder.path().to_path_buf(),
            path: root.clone(),
        };
        let (send, receive) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let mut watcher =
            super::DesktopWatcher::new(std::slice::from_ref(&source), send.clone(), failed.clone())
                .unwrap();
        std::fs::create_dir(&root).unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        watcher
            .reenroll(std::slice::from_ref(&source), send.clone(), failed.clone())
            .unwrap();
        assert!(watcher.plans[0].complete);
        while receive.try_recv().is_ok() {}
        let path = root.join("codex-desktop-fixture.log");
        std::fs::write(&path, b"2026-11-12T01:02:03Z INFO [codex] first\n").unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        watcher.reenroll(&[source], send, failed).unwrap();
        let mut reader = super::DesktopFile::default();
        assert_eq!(reader.read(&root, &path, 1024).unwrap().1.len(), 1);
        while receive.try_recv().is_ok() {}
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"2026-11-12T01:02:04Z WARN [codex] second\n")
            .unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(reader.read(&root, &path, 1024).unwrap().1.len(), 1);
        drop(watcher);
    }

    #[test]
    fn independent_watcher_observes_source_creation_and_append() {
        use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("desktop-logs");
        let source = super::DesktopRoot {
            base: folder.path().to_path_buf(),
            path: root.clone(),
        };
        let (send, receive) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |event| {
                let _ = send.send(event);
            },
            Config::default().with_follow_symlinks(false),
        )
        .unwrap();
        let initial = super::watch_plan(&source).unwrap();
        watcher
            .watch(&initial.anchor, RecursiveMode::NonRecursive)
            .unwrap();
        assert!(initial.verify(&source).unwrap());
        std::fs::create_dir(&root).unwrap();
        let event = receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(event.paths.iter().any(|path| super::related_event(
            &initial.anchor,
            &initial.target,
            initial.complete,
            path
        )));
        let plan = super::watch_plan(&source).unwrap();
        watcher
            .watch(&plan.anchor, RecursiveMode::Recursive)
            .unwrap();
        assert!(plan.verify(&source).unwrap());
        let path = plan.anchor.join("codex-desktop-fixture.log");
        std::fs::write(&path, b"first\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let event = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
                .unwrap();
            if event
                .paths
                .iter()
                .any(|path| path == &plan.anchor || path.starts_with(&plan.anchor))
            {
                break;
            }
        }
        // A synchronous recursive watch ensures the newly created file is enrolled.
        watcher
            .watch(&plan.anchor, RecursiveMode::Recursive)
            .unwrap();
        while receive.try_recv().is_ok() {}
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"second\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let event = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
                .unwrap();
            if event.paths.iter().any(|event_path| event_path == &path) {
                break;
            }
        }
    }

    #[test]
    fn watch_anchor_tracks_missing_directory_then_existing_root() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("desktop-logs");
        assert_eq!(
            super::watch_anchor(folder.path(), &root).unwrap(),
            (folder.path().canonicalize().unwrap(), false)
        );
        std::fs::create_dir(&root).unwrap();
        assert_eq!(
            super::watch_anchor(folder.path(), &root).unwrap(),
            (root.canonicalize().unwrap(), true)
        );
    }

    #[cfg(unix)]
    #[test]
    fn watch_anchor_rejects_redirected_root() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = folder.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(super::watch_anchor(folder.path(), &link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn watch_anchor_rejects_redirected_intermediate_directory() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = folder.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(super::watch_anchor(folder.path(), &link.join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn discovery_rejects_redirected_intermediate_directory() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("logs")).unwrap();
        std::fs::write(outside.path().join("logs/codex-desktop-fixture.log"), b"").unwrap();
        let link = folder.path().join("link");
        symlink(outside.path(), &link).unwrap();
        let source = super::DesktopRoot {
            base: folder.path().to_path_buf(),
            path: link.join("logs"),
        };
        let (selected, unavailable) = super::discover(&[source]);
        assert!(unavailable.is_some());
        assert!(selected.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn explicitly_configured_base_symlink_is_resolved_once() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target");
        let root = target.join("Codex/Logs");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("codex-desktop-fixture.log"), b"").unwrap();
        let alias = folder.path().join("alias");
        symlink(&target, &alias).unwrap();
        let source = super::DesktopRoot {
            base: alias.clone(),
            path: alias.join("Codex/Logs"),
        };
        let (selected, unavailable) = super::discover(&[source]);
        assert!(unavailable.is_none());
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].0, target.canonicalize().unwrap());
    }

    #[test]
    fn reads_appends_and_resets_after_replacement() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("codex-desktop-fixture.log");
        std::fs::write(&path, b"2026-11-12T01:02:03Z WARN [codex] one").unwrap();
        let mut reader = DesktopFile::default();
        assert!(reader
            .read(folder.path(), &path, 1024)
            .unwrap()
            .1
            .is_empty());
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(reader.read(folder.path(), &path, 1024).unwrap().1.len(), 1);
        let replacement = folder.path().join("replacement");
        std::fs::write(&replacement, b"2026-11-12T01:02:04Z ERROR [codex] two\n").unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert_eq!(reader.read(folder.path(), &path, 1024).unwrap().1.len(), 1);
    }
}
