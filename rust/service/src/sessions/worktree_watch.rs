//! Observe only selected Git metadata and shallow managed-worktree directories.
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc, RwLock,
};
use walkdir::WalkDir;

const WATCH_LIMIT: usize = 5000;
#[derive(Default)]
struct Selection {
    metadata: Vec<PathBuf>,
    managed: PathBuf,
    fallback: Option<PathBuf>,
    watched: BTreeSet<PathBuf>,
}

pub(super) struct SourceWatcher {
    watcher: RecommendedWatcher,
    selection: Arc<RwLock<Selection>>,
    failed: Arc<AtomicBool>,
    healthy: bool,
}

fn relevant(path: &Path, selection: &Selection) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
    {
        return false;
    }
    if selection.metadata.iter().any(|root| {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        let parts: Vec<_> = relative.components().collect();
        if parts.is_empty() {
            return true;
        }
        let first = parts[0].as_os_str().to_string_lossy();
        first == "refs"
            || (first == "worktrees"
                && (parts.len() <= 2
                    || (parts.len() == 3
                        && ["HEAD", "gitdir", "commondir", "locked"]
                            .contains(&parts[2].as_os_str().to_string_lossy().as_ref()))))
            || (parts.len() == 1
                && ["HEAD", "packed-refs", "gitdir", "commondir", "locked"]
                    .contains(&first.as_ref()))
    }) {
        return true;
    }
    if let Ok(relative) = path.strip_prefix(&selection.managed) {
        let parts: Vec<_> = relative.components().collect();
        if parts.len() <= 2 || (parts.len() == 3 && parts[2].as_os_str() == ".git") {
            return true;
        }
    }
    cfg!(target_os = "macos")
        && (selection.watched.contains(path) || selection.fallback.as_deref() == Some(path))
}

impl SourceWatcher {
    pub fn new(wake: SyncSender<()>, dirty: Arc<AtomicBool>) -> notify::Result<Self> {
        let selection = Arc::new(RwLock::new(Selection::default()));
        let selected = Arc::clone(&selection);
        let failed = Arc::new(AtomicBool::new(false));
        let callback_failed = Arc::clone(&failed);
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let changed = match event {
                    Ok(event) if !event.kind.is_access() => {
                        event.need_rescan()
                            || event.paths.iter().any(|path| {
                                relevant(
                                    path,
                                    &selected.read().unwrap_or_else(|error| error.into_inner()),
                                )
                            })
                    }
                    Err(_) => {
                        callback_failed.store(true, Ordering::Release);
                        true
                    }
                    _ => false,
                };
                if changed {
                    dirty.store(true, Ordering::Release);
                    let _ = wake.try_send(());
                }
            },
            Config::default().with_follow_symlinks(false),
        )?;
        Ok(Self {
            watcher,
            selection,
            failed,
            healthy: true,
        })
    }

    pub fn healthy(&self) -> bool {
        self.healthy && !self.failed.load(Ordering::Acquire)
    }

    pub fn refresh(&mut self, home: &Path, metadata: Vec<PathBuf>, enabled: bool) -> bool {
        let managed = home.join("worktrees");
        let mut wanted = BTreeSet::new();
        let mut fallback = None;
        let mut limited = false;
        let mut unavailable = false;
        if enabled {
            if managed.is_dir() && super::worktrees::local_path(&managed) {
                wanted.insert(managed.clone());
                if let Ok(groups) = std::fs::read_dir(&managed) {
                    for group in groups.take(100).flatten() {
                        let group = group.path();
                        if !group.is_dir() || !super::worktrees::local_path(&group) {
                            continue;
                        }
                        wanted.insert(group.clone());
                        if let Ok(children) = std::fs::read_dir(group) {
                            for child in children.take(32).flatten() {
                                let child = child.path();
                                if child.is_dir() && super::worktrees::local_path(&child) {
                                    wanted.insert(child.clone());
                                    let marker = child.join(".git");
                                    if marker.is_file() && super::worktrees::local_path(&marker) {
                                        wanted.insert(marker);
                                    }
                                }
                            }
                        } else {
                            unavailable = true;
                        }
                    }
                } else {
                    unavailable = true;
                }
            } else if home.is_dir() && super::worktrees::local_path(home) {
                wanted.insert(home.to_path_buf());
                fallback = Some(home.to_path_buf());
            }
            for root in &metadata {
                if !root.is_dir() || !super::worktrees::local_path(root) {
                    continue;
                }
                wanted.insert(root.clone());
                for name in ["HEAD", "packed-refs", "gitdir", "commondir", "locked"] {
                    let file = root.join(name);
                    if file.is_file() && super::worktrees::local_path(&file) {
                        wanted.insert(file);
                    }
                }
                let refs = root.join("refs");
                if refs.is_dir() {
                    for (visited, entry) in WalkDir::new(refs)
                        .follow_links(false)
                        .into_iter()
                        .enumerate()
                    {
                        if wanted.len() >= WATCH_LIMIT || visited >= WATCH_LIMIT {
                            limited = true;
                            break;
                        }
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => {
                                unavailable = true;
                                continue;
                            }
                        };
                        let path = entry.path();
                        if !entry.file_type().is_symlink()
                            && !path
                                .file_name()
                                .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
                        {
                            wanted.insert(path.to_path_buf());
                        }
                    }
                }
                let worktrees = root.join("worktrees");
                if worktrees.is_dir() {
                    wanted.insert(worktrees.clone());
                    if let Ok(entries) = std::fs::read_dir(worktrees) {
                        for entry in entries.take(500).flatten() {
                            if wanted.len() >= WATCH_LIMIT {
                                limited = true;
                                break;
                            }
                            let path = entry.path();
                            if !path.is_dir() || !super::worktrees::local_path(&path) {
                                continue;
                            }
                            wanted.insert(path.clone());
                            for name in ["HEAD", "gitdir", "commondir", "locked"] {
                                let file = path.join(name);
                                if file.is_file() && super::worktrees::local_path(&file) {
                                    wanted.insert(file);
                                }
                            }
                        }
                    } else {
                        unavailable = true;
                    }
                }
            }
        }
        if wanted.len() > WATCH_LIMIT {
            wanted = wanted.into_iter().take(WATCH_LIMIT).collect();
            limited = true;
        }
        let previous = self
            .selection
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .watched
            .clone();
        for path in previous.difference(&wanted) {
            let _ = self.watcher.unwatch(path);
        }
        let mut actual: BTreeSet<_> = previous.intersection(&wanted).cloned().collect();
        self.healthy = !limited && !unavailable;
        for path in wanted.difference(&previous) {
            if self
                .watcher
                .watch(path, RecursiveMode::NonRecursive)
                .is_ok()
            {
                actual.insert(path.clone());
            } else {
                self.healthy = false;
            }
        }
        let changed = actual != previous;
        *self
            .selection
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Selection {
            metadata,
            managed,
            fallback,
            watched: actual,
        };
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_git_metadata_and_shallow_managed_entries() {
        let selected = Selection {
            metadata: vec![PathBuf::from("/repo/.git")],
            managed: PathBuf::from("/home/worktrees"),
            fallback: None,
            watched: BTreeSet::new(),
        };
        assert!(relevant(Path::new("/repo/.git/HEAD"), &selected));
        assert!(relevant(Path::new("/repo/.git/refs/heads/main"), &selected));
        assert!(relevant(
            Path::new("/repo/.git/worktrees/a/locked"),
            &selected
        ));
        assert!(!relevant(
            Path::new("/repo/.git/refs/heads/main.lock"),
            &selected
        ));
        assert!(!relevant(
            Path::new("/repo/.git/objects/ab/file"),
            &selected
        ));
        assert!(relevant(
            Path::new("/home/worktrees/group/child/.git"),
            &selected
        ));
        assert!(!relevant(
            Path::new("/home/worktrees/group/child/src/private"),
            &selected
        ));
    }
}
