//! Sample this process and the monitor's fixed set of storage files.
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const NAMES: [&str; 10] = [
    "lam-history.sqlite3",
    "lam-history.sqlite3-wal",
    "lam-history.sqlite3-shm",
    "lam-history.sqlite3-journal",
    "local-activity-monitor.jsonl",
    "local-activity-monitor.jsonl.1",
    "performance-debug.jsonl",
    "performance-debug.jsonl.1",
    "performance-debug.jsonl.2",
    "performance-debug.jsonl.3",
];
fn checked_metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = metadata.file_type().is_symlink();
            if redirected {
                Err(io::Error::other("redirected storage path"))
            } else {
                Ok(Some(metadata))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
fn storage(directory: &Path) -> (Option<u64>, Option<u64>, &'static str) {
    if checked_metadata(directory).is_err() {
        return (None, None, "partly_unavailable");
    }
    let mut bytes = 0u64;
    let mut files = 0u64;
    for name in NAMES {
        match checked_metadata(&directory.join(name)) {
            Ok(Some(metadata)) if metadata.is_file() => {
                let Some(total) = bytes.checked_add(metadata.len()) else {
                    return (None, None, "partly_unavailable");
                };
                bytes = total;
                files += 1;
            }
            Ok(_) => {}
            Err(_) => return (None, None, "partly_unavailable"),
        }
    }
    (Some(bytes), Some(files), "ok")
}

pub struct Sampler {
    directory: PathBuf,
    previous: Instant,
    cpu: Option<f64>,
    checked: Option<Instant>,
    value: Value,
}
impl Sampler {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            previous: Instant::now(),
            cpu: crate::performance_debug::process_cpu_ms(),
            checked: None,
            value: Value::Null,
        }
    }
    pub fn sample(&mut self) -> Value {
        let now = Instant::now();
        if self
            .checked
            .is_some_and(|checked| now.duration_since(checked) < Duration::from_secs(5))
        {
            return self.value.clone();
        }
        let elapsed = now.duration_since(self.previous).as_secs_f64() * 1000.0;
        let metrics = crate::performance_debug::process_metrics();
        let cpu = metrics.get("process_cpu_ms").and_then(Value::as_f64);
        let percent = cpu.zip(self.cpu).and_then(|(current, previous)| {
            let used = current - previous;
            (elapsed > 0.0 && used >= 0.0).then_some((used / elapsed * 1000.0).round() / 10.0)
        });
        let (disk, files, health) = storage(&self.directory);
        self.value = json!({"pid":std::process::id(),"cpu_percent":percent,"rss_bytes":metrics.get("rss_bytes"),"peak_rss_bytes":metrics.get("peak_rss_bytes"),"disk_bytes":disk,"storage_files":files,"storage_health":health,"checked_at":DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Secs,false)});
        self.previous = now;
        self.cpu = cpu;
        self.checked = Some(now);
        self.value.clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_counts_only_owned_files_and_tolerates_absent_files() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("lam-history.sqlite3"), b"123").unwrap();
        fs::write(home.path().join("performance-debug.jsonl.1"), b"4567").unwrap();
        fs::write(home.path().join("unrelated"), b"other").unwrap();
        assert_eq!(storage(home.path()), (Some(7), Some(2), "ok"));
    }
    #[cfg(unix)]
    #[test]
    fn storage_does_not_follow_redirected_files() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("target"), b"secret").unwrap();
        symlink(
            outside.path().join("target"),
            home.path().join("lam-history.sqlite3"),
        )
        .unwrap();
        assert_eq!(storage(home.path()), (None, None, "partly_unavailable"));
    }
    #[test]
    fn sampler_caches_and_refreshes_storage_without_paths() {
        let home = tempfile::tempdir().unwrap();
        let mut sampler = Sampler::new(home.path().into());
        let first = sampler.sample();
        assert_eq!(first["storage_files"], 0);
        fs::write(home.path().join("lam-history.sqlite3"), b"123").unwrap();
        assert_eq!(sampler.sample()["storage_files"], 0);
        sampler.checked = Some(Instant::now() - Duration::from_secs(6));
        let updated = sampler.sample();
        assert_eq!(updated["storage_files"], 1);
        assert_eq!(updated["disk_bytes"], 3);
        assert!(updated["checked_at"].is_string());
        assert!(!updated
            .to_string()
            .contains(&home.path().to_string_lossy().to_string()));
    }
}
