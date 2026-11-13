//! Bounded, read-only Git worktree enumeration.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BYTE_LIMIT: usize = 1024 * 1024;
const ROOT_LIMIT: usize = 100;
const ITEM_LIMIT: usize = 500;

pub(super) struct Collector {
    home: PathBuf,
    items: BTreeMap<String, Value>,
    paths: BTreeMap<String, PathBuf>,
    root_items: BTreeMap<PathBuf, BTreeSet<String>>,
    signature: Vec<(PathBuf, BTreeSet<String>)>,
    cursor: usize,
    next_read: Option<Instant>,
    pending: bool,
    remaining: usize,
    cycle_failures: Vec<Value>,
    result: Value,
}

impl Collector {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            items: BTreeMap::new(),
            paths: BTreeMap::new(),
            root_items: BTreeMap::new(),
            signature: Vec::new(),
            cursor: 0,
            next_read: None,
            pending: false,
            remaining: 0,
            cycle_failures: Vec::new(),
            result: json!({"health":"waiting","items":[],"checked_at":null}),
        }
    }

    pub fn snapshot(&self) -> Value {
        let mut result = self.result.clone();
        result["items"] = json!(self.items.values().collect::<Vec<_>>());
        result
    }

    pub fn pending(&self) -> bool {
        self.pending
    }

    pub fn details(&self) -> BTreeMap<String, Value> {
        self.items
            .iter()
            .filter_map(|(id, item)| {
                self.paths.get(id).map(|path| {
                    let mut item = item.clone();
                    item["path"] = json!(path.to_string_lossy());
                    (id.clone(), item)
                })
            })
            .collect()
    }

    pub fn notification_roots(&self) -> Vec<PathBuf> {
        let mut roots = BTreeSet::new();
        for path in self
            .signature
            .iter()
            .map(|(path, _)| path)
            .chain(self.paths.values())
        {
            if !local_path(path) {
                continue;
            }
            let marker = path.join(".git");
            let git = if marker.is_dir() && local_path(&marker) {
                Some(marker)
            } else if marker.is_file() && local_path(&marker) {
                read_marker(&marker)
                    .and_then(|value| {
                        value
                            .strip_prefix("gitdir: ")
                            .map(|value| resolve_path(path, value))
                    })
                    .filter(|path| local_path(path))
            } else if path.join("HEAD").is_file() && path.join("objects").is_dir() {
                Some(path.clone())
            } else {
                None
            };
            let Some(git) = git.filter(|path| path.is_dir()) else {
                continue;
            };
            roots.insert(git.clone());
            if let Some(common) = read_marker(&git.join("commondir"))
                .map(|value| resolve_path(&git, &value))
                .filter(|path| local_path(path) && path.is_dir())
            {
                roots.insert(common);
            }
            if roots.len() >= ROOT_LIMIT {
                break;
            }
        }
        roots.into_iter().take(ROOT_LIMIT).collect()
    }

    pub fn refresh(
        &mut self,
        projects: &BTreeMap<String, Value>,
        workdirs: &BTreeMap<String, String>,
        roots_status: super::projects::RootStatus,
        enabled: bool,
        force: bool,
    ) -> bool {
        self.refresh_with(
            projects,
            workdirs,
            roots_status,
            enabled,
            force,
            Duration::from_secs(5),
            git_list,
        )
    }

    fn refresh_with<F: FnMut(&Path, Duration) -> Result<Vec<Worktree>, QueryError>>(
        &mut self,
        projects: &BTreeMap<String, Value>,
        workdirs: &BTreeMap<String, String>,
        roots_status: super::projects::RootStatus,
        enabled: bool,
        force: bool,
        budget: Duration,
        mut query: F,
    ) -> bool {
        let previous = self.snapshot();
        if !enabled {
            self.items.clear();
            self.paths.clear();
            self.root_items.clear();
            self.signature.clear();
            self.cursor = 0;
            self.next_read = None;
            self.pending = false;
            self.remaining = 0;
            self.cycle_failures.clear();
            self.result = json!({"health":"disabled","items":[],"checked_at":null});
            return self.snapshot() != previous;
        }
        if !force && !self.pending && self.next_read.is_some_and(|next| Instant::now() < next) {
            self.associate(workdirs);
            return self.snapshot() != previous;
        }
        let mut continuing = self.pending && !force;
        self.next_read = Some(Instant::now() + Duration::from_secs(30));
        self.pending = false;
        let mut roots = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        let mut limited = roots_status.limited;
        for (id, project) in projects {
            if roots.len() >= ROOT_LIMIT {
                limited = true;
                break;
            }
            for folder in project["_folders"]
                .as_array()
                .into_iter()
                .flatten()
                .take(32)
                .filter_map(Value::as_str)
            {
                let path = PathBuf::from(folder);
                if local_path(&path) {
                    roots.entry(path).or_default().insert(id.clone());
                }
                if roots.len() >= ROOT_LIMIT {
                    limited = true;
                    break;
                }
            }
        }
        let managed_root = self.home.join("worktrees");
        let mut managed_health = "missing";
        let mut managed_directories = 0usize;
        if !local_path(&managed_root) {
            managed_health = "rejected";
        } else if managed_root.is_dir() {
            managed_health = "ok";
            match std::fs::read_dir(&managed_root) {
                Ok(dirs) => {
                    for folder in dirs.take(ROOT_LIMIT + 1) {
                        let Ok(folder) = folder else {
                            managed_health = "unavailable";
                            continue;
                        };
                        let path = folder.path();
                        if !path.is_dir() || !local_path(&path) {
                            continue;
                        }
                        managed_directories += 1;
                        if managed_directories > ROOT_LIMIT {
                            limited = true;
                            break;
                        }
                        match std::fs::read_dir(path) {
                            Ok(children) => {
                                for entry in children.take(32).flatten() {
                                    let path = entry.path();
                                    if roots.len() >= ROOT_LIMIT {
                                        limited = true;
                                        break;
                                    }
                                    if path.is_dir()
                                        && local_path(&path)
                                        && path.join(".git").exists()
                                    {
                                        roots.entry(path).or_default();
                                    }
                                }
                            }
                            Err(_) => managed_health = "unavailable",
                        }
                    }
                }
                Err(_) => managed_health = "unavailable",
            }
        }
        let selected: Vec<_> = roots.into_iter().collect();
        if selected != self.signature {
            self.items.clear();
            self.paths.clear();
            self.root_items.clear();
            self.cursor = 0;
            self.signature = selected.clone();
            continuing = false;
        }
        if !continuing {
            self.remaining = selected.len();
            self.cycle_failures.clear();
        }
        let checked = super::project::stamp(super::project::now());
        let deadline = Instant::now() + budget;
        let mut seen = BTreeSet::new();
        let mut failures = std::mem::take(&mut self.cycle_failures);
        let mut queries = 0usize;
        for _ in 0..self.remaining {
            let index = self.cursor % selected.len();
            let (root, project_ids) = &selected[index];
            if deadline <= Instant::now() {
                limited = true;
                self.pending = true;
                break;
            }
            self.cursor = (index + 1) % selected.len();
            self.remaining -= 1;
            if seen.contains(root) {
                continue;
            }
            let marker = root.join(".git");
            if std::fs::symlink_metadata(&marker)
                .ok()
                .is_some_and(|meta| super::redirected(&meta))
            {
                failures.push(json!({"root_id":hash(root),"project_ids":project_ids,"reason":"linked_git_metadata"}));
                continue;
            }
            if !marker.exists() && !(root.join("HEAD").is_file() && root.join("objects").is_dir()) {
                let previous_ids = self.root_items.remove(root).unwrap_or_default();
                let still_used: BTreeSet<_> = self.root_items.values().flatten().cloned().collect();
                for id in previous_ids.difference(&still_used) {
                    self.items.remove(id);
                    self.paths.remove(id);
                }
                continue;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                self.cursor = index;
                self.remaining += 1;
                limited = true;
                self.pending = true;
                break;
            };
            queries += 1;
            match query(root, remaining) {
                Ok(records) => {
                    let previous_ids=self.root_items.remove(root).unwrap_or_default();
                    let still_used:BTreeSet<_>=self.root_items.values().flatten().cloned().collect();
                    for id in previous_ids.difference(&still_used) { self.items.remove(id); self.paths.remove(id); }
                    let aliases:BTreeSet<_>=records.iter().map(|item|item.path.clone()).collect();
                    let mut projects=project_ids.clone();
                    for (seed,ids) in &selected { if aliases.contains(seed) { projects.extend(ids.iter().cloned()); } }
                    let mut identities=BTreeSet::new();
                    for record in records {
                        if record.bare || !local_path(&record.path) { continue; }
                        let id=hash(&record.path);
                        if !self.items.contains_key(&id) && self.items.len() >= ITEM_LIMIT { limited=true; continue; }
                        let managed=record.path.starts_with(&managed_root) || record.path.parent().is_some_and(|parent|parent.join(".codex-worktree-name").is_file());
                        let name=record.path.file_name().map(|value|value.to_string_lossy().to_string()).unwrap_or_default();
                        let available=record.path.is_dir();
                        self.items.insert(id.clone(),json!({"id":id,"name":name,"branch":record.branch,"commit":record.commit,"detached":record.detached,"locked":record.locked,"prunable":record.prunable,"available":available,"managed":managed,"project_ids":projects,"thread_ids":[],"thread_count":0,"checked_at":checked}));
                        self.paths.insert(id.clone(),record.path); identities.insert(id);
                    }
                    for alias in aliases { seen.insert(alias.clone()); if selected.iter().any(|(seed,_)|*seed==alias) { self.root_items.insert(alias,identities.clone()); } }
                    self.root_items.insert(root.clone(),identities);
                }
                Err(error) => failures.push(json!({"root_id":hash(root),"project_ids":project_ids,"reason":error.reason,"exit_code":error.exit_code})),
            }
        }
        let missing_git =
            !git_available() || failures.iter().any(|row| row["reason"] == "missing_git");
        let complete = !missing_git
            && failures.is_empty()
            && !limited
            && roots_status.complete
            && !matches!(managed_health, "unavailable" | "rejected");
        if self.pending {
            self.cycle_failures = failures.clone();
        }
        self.result = json!({"health":if missing_git {"missing_git"} else if !roots_status.complete || !failures.is_empty() || matches!(managed_health,"unavailable"|"rejected") {"partly_unavailable"} else {"ok"},"checked_at":checked,"managed_root":managed_root.to_string_lossy(),"managed_health":managed_health,"managed_directories":managed_directories,"total":complete.then_some(self.items.len()),"observed_total":self.items.len(),"count_complete":complete,"root_source_health":if roots_status.complete {"ok"} else {"partly_unavailable"},"queries":queries,"failed_queries":failures.len(),"query_failures":failures,"limited":limited,"root_limit":ROOT_LIMIT,"item_limit":ITEM_LIMIT,"byte_limit":BYTE_LIMIT,"cache_seconds":30,"association_scope":"loaded_thread_cwd"});
        self.associate(workdirs);
        self.snapshot() != previous
    }

    fn associate(&mut self, workdirs: &BTreeMap<String, String>) {
        let mut associations = BTreeMap::<String, Vec<String>>::new();
        for (thread, cwd) in workdirs {
            let path = Path::new(cwd);
            if !local_path(path) {
                continue;
            }
            if let Some((id, _)) = self
                .paths
                .iter()
                .filter(|(_, root)| path.starts_with(root))
                .max_by_key(|(_, root)| root.components().count())
            {
                associations
                    .entry(id.clone())
                    .or_default()
                    .push(thread.clone());
            }
        }
        for (id, item) in &mut self.items {
            let threads = associations.remove(id).unwrap_or_default();
            item["thread_count"] = json!(threads.len());
            item["thread_ids"] = json!(threads);
        }
    }
}

fn read_marker(path: &Path) -> Option<String> {
    if !local_path(path) || !path.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    super::open_source(path.parent()?, path)
        .ok()?
        .take(4097)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    Some(std::str::from_utf8(&bytes).ok()?.trim().to_owned())
}

fn resolve_path(base: &Path, value: &str) -> PathBuf {
    let value = Path::new(value);
    let input = if value.is_absolute() {
        value.to_path_buf()
    } else {
        base.join(value)
    };
    let mut path = PathBuf::new();
    for part in input.components() {
        match part {
            std::path::Component::ParentDir => {
                path.pop();
            }
            std::path::Component::CurDir => {}
            value => path.push(value.as_os_str()),
        }
    }
    path
}

fn hash(path: &Path) -> String {
    let text = path.to_string_lossy();
    let normalized = if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.into_owned()
    };
    format!("{:x}", Sha256::digest(normalized.as_bytes()))[..24].to_owned()
}

fn git_available() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(if cfg!(windows) { "git.exe" } else { "git" });
        let Ok(metadata) = std::fs::metadata(candidate) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Worktree {
    pub path: PathBuf,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct QueryError {
    pub reason: &'static str,
    pub exit_code: Option<i32>,
}

pub(super) fn local_path(path: &Path) -> bool {
    let value = path.to_string_lossy();
    if !path.is_absolute()
        || value.starts_with("//")
        || value.starts_with("\\\\")
        || value.chars().any(char::is_control)
    {
        return false;
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if super::redirected(&metadata) => return false,
            Err(error) if error.kind() != io::ErrorKind::NotFound => return false,
            _ => {}
        }
    }
    true
}

pub(super) fn parse(output: &[u8]) -> Vec<Worktree> {
    let decoded = String::from_utf8_lossy(output);
    let mut result = Vec::new();
    let mut item = Worktree::default();
    for field in decoded.split('\0') {
        if field.is_empty() {
            if !item.path.as_os_str().is_empty() {
                result.push(std::mem::take(&mut item));
            }
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => item.path = PathBuf::from(value),
            "HEAD"
                if matches!(value.len(), 40 | 64)
                    && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                item.commit = Some(value.to_owned())
            }
            "branch"
                if value.starts_with("refs/heads/")
                    && value.len() <= 512
                    && !value.chars().any(char::is_control) =>
            {
                item.branch = Some(value[11..].to_owned())
            }
            "detached" => item.detached = true,
            "bare" => item.bare = true,
            "locked" => item.locked = true,
            "prunable" => item.prunable = true,
            _ => {}
        }
    }
    if !item.path.as_os_str().is_empty() {
        result.push(item);
    }
    result
}

pub(super) fn git_list(root: &Path, timeout: Duration) -> Result<Vec<Worktree>, QueryError> {
    git_list_with(Path::new("git"), root, timeout)
}

fn git_list_with(
    executable: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<Vec<Worktree>, QueryError> {
    if !local_path(root) {
        return Err(QueryError {
            reason: "invalid_root",
            exit_code: None,
        });
    }
    let mut command = Command::new(executable);
    command.args(["--no-optional-locks", "-C"]).arg(root).args([
        "worktree",
        "list",
        "--porcelain",
        "-z",
    ]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    let mut output_file = tempfile::tempfile().map_err(|_| QueryError {
        reason: "temporary_file_unavailable",
        exit_code: None,
    })?;
    let mut error_file = tempfile::tempfile().map_err(|_| QueryError {
        reason: "temporary_file_unavailable",
        exit_code: None,
    })?;
    let output_child = output_file.try_clone().map_err(|_| QueryError {
        reason: "temporary_file_unavailable",
        exit_code: None,
    })?;
    let error_child = error_file.try_clone().map_err(|_| QueryError {
        reason: "temporary_file_unavailable",
        exit_code: None,
    })?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_child))
        .stderr(Stdio::from(error_child));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|error| QueryError {
        reason: if error.kind() == io::ErrorKind::NotFound {
            "missing_git"
        } else {
            "launch_failed"
        },
        exit_code: None,
    })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("timeout");
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("query_io_error");
            }
        }
    };
    let status = status.map_err(|reason| QueryError {
        reason,
        exit_code: None,
    })?;
    if !status.success() {
        error_file
            .seek(SeekFrom::Start(0))
            .map_err(|_| QueryError {
                reason: "query_io_error",
                exit_code: None,
            })?;
        let mut diagnostics = Vec::new();
        error_file
            .take(4096)
            .read_to_end(&mut diagnostics)
            .map_err(|_| QueryError {
                reason: "query_io_error",
                exit_code: None,
            })?;
        let diagnostic = String::from_utf8_lossy(&diagnostics).to_ascii_lowercase();
        let reason = [
            ("dubious ownership", "ownership_rejected"),
            ("not a git repository", "not_repository"),
            ("unknown option", "unsupported_option"),
            ("permission denied", "permission_denied"),
            ("invalid gitfile", "invalid_git_metadata"),
        ]
        .iter()
        .find_map(|(needle, reason)| diagnostic.contains(needle).then_some(*reason))
        .unwrap_or("git_exit");
        return Err(QueryError {
            reason,
            exit_code: status.code(),
        });
    }
    output_file
        .seek(SeekFrom::Start(0))
        .map_err(|_| QueryError {
            reason: "query_io_error",
            exit_code: None,
        })?;
    let mut output = Vec::new();
    output_file
        .take((BYTE_LIMIT + 1) as u64)
        .read_to_end(&mut output)
        .map_err(|_| QueryError {
            reason: "query_io_error",
            exit_code: None,
        })?;
    if output.len() > BYTE_LIMIT {
        return Err(QueryError {
            reason: "oversized",
            exit_code: None,
        });
    }
    Ok(parse(&output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_nul_records_without_exposing_lock_reasons() {
        let hash = "a".repeat(40);
        let raw = format!("worktree /tmp/main\0HEAD {hash}\0branch refs/heads/main\0\0worktree /tmp/linked\0HEAD {hash}\0detached\0locked PRIVATE\0prunable SECRET\0\0");
        let items = parse(raw.as_bytes());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].branch.as_deref(), Some("main"));
        assert!(items[1].detached && items[1].locked && items[1].prunable);
        assert!(!format!("{items:?}").contains("PRIVATE"));
        assert!(!format!("{items:?}").contains("SECRET"));
    }

    #[test]
    fn time_budget_continues_remaining_roots_without_an_endless_rescan() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        for index in 0..3 {
            std::fs::create_dir_all(home.join(format!("worktrees/group/repo{index}/.git")))
                .unwrap();
        }
        let mut collector = Collector::new(home);
        let mut queried = Vec::new();
        for _ in 0..4 {
            collector.refresh_with(
                &BTreeMap::new(),
                &BTreeMap::new(),
                super::super::projects::RootStatus::default(),
                true,
                false,
                Duration::from_millis(25),
                |root, _| {
                    queried.push(root.to_owned());
                    thread::sleep(Duration::from_millis(20));
                    Ok(vec![Worktree {
                        path: root.to_owned(),
                        ..Worktree::default()
                    }])
                },
            );
            if !collector.pending() {
                break;
            }
        }
        assert!(!collector.pending());
        assert_eq!(queried.len(), 3);
        assert_eq!(collector.snapshot()["total"], 3);
    }

    #[test]
    fn incomplete_project_roots_do_not_claim_complete_count() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let root = home.join("worktrees/group/repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let mut collector = Collector::new(home);
        collector.refresh_with(
            &BTreeMap::new(),
            &BTreeMap::new(),
            super::super::projects::RootStatus {
                complete: false,
                limited: false,
            },
            true,
            false,
            Duration::from_secs(1),
            |root, _| {
                Ok(vec![Worktree {
                    path: root.to_owned(),
                    ..Worktree::default()
                }])
            },
        );
        let snapshot = collector.snapshot();
        assert_eq!(snapshot["health"], "partly_unavailable");
        assert_eq!(snapshot["observed_total"], 1);
        assert!(snapshot["total"].is_null());
        assert_eq!(snapshot["count_complete"], false);
    }

    #[test]
    fn bounded_git_query_reads_only_fixture_repository() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().canonicalize().unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let items = git_list(&path, Duration::from_secs(3)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, path);
        let error = git_list(&path.join("missing"), Duration::from_secs(3)).unwrap_err();
        assert_eq!(error.reason, "git_exit");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_roots_and_stops_timed_out_git() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().canonicalize().unwrap();
        symlink(&path, path.join("linked")).unwrap();
        assert!(!local_path(&path.join("linked")));
        let executable = path.join("fake-git");
        std::fs::write(&executable, "#!/bin/sh\nexec sleep 10\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let began = Instant::now();
        let error = git_list_with(&executable, &path, Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.reason, "timeout");
        assert!(began.elapsed() < Duration::from_secs(3));
    }
}
