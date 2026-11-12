use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

const FILE_BYTES: u64 = 8 * 1024 * 1024;
const READ_BYTES: u64 = 256 * 1024;
const RECORD_LIMIT: usize = 150;
const KINDS: &[&str] = &[
    "enabled",
    "disabled",
    "collection",
    "stream_open",
    "stream_frame",
    "stream_close",
    "stream_error",
    "heartbeat",
];
const FIELDS: &[&str] = &[
    "refresh_ms",
    "cpu_ms",
    "read_bytes",
    "retained_calls",
    "buffer_bytes",
    "trimmed_calls",
    "sql_records",
    "web_records",
    "mcp_records",
    "snapshot_bytes",
    "transfer_bytes",
    "serialization_ms",
    "write_ms",
    "active_streams",
    "uptime_ms",
    "process_cpu_ms",
    "rss_bytes",
    "peak_rss_bytes",
];
const PHASES: &[&str] = &[
    "sessions",
    "projections",
    "worktrees",
    "account",
    "diagnostics",
    "history",
    "mcp",
    "jev",
    "hardware",
];

#[cfg(unix)]
fn process_metrics() -> Map<String, Value> {
    let mut metrics = Map::new();
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage initializes the output on success and reads no user-provided pointer.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
        let usage = unsafe { usage.assume_init() };
        let cpu_ms = (usage.ru_utime.tv_sec as f64 + usage.ru_stime.tv_sec as f64) * 1000.0
            + (usage.ru_utime.tv_usec as f64 + usage.ru_stime.tv_usec as f64) / 1000.0;
        if cpu_ms.is_finite() && cpu_ms >= 0.0 {
            metrics.insert("process_cpu_ms".into(), json!(cpu_ms));
        }
        #[cfg(target_os = "macos")]
        let multiplier = 1_u64;
        #[cfg(not(target_os = "macos"))]
        let multiplier = 1024_u64;
        if let Some(peak) = u64::try_from(usage.ru_maxrss)
            .ok()
            .and_then(|value| value.checked_mul(multiplier))
        {
            metrics.insert("peak_rss_bytes".into(), json!(peak));
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut contents = String::new();
        let pages = fs::File::open("/proc/self/statm").ok().and_then(|file| {
            file.take(256).read_to_string(&mut contents).ok()?;
            contents
                .split_ascii_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
        });
        // sysconf returns the system page size or a negative value on failure.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if let Some(bytes) = pages.and_then(|pages| {
            u64::try_from(page_size)
                .ok()
                .filter(|size| *size > 0)
                .and_then(|size| pages.checked_mul(size))
        }) {
            metrics.insert("rss_bytes".into(), json!(bytes));
        }
    }
    metrics
}

pub fn process_cpu_ms() -> Option<f64> {
    process_metrics()
        .get("process_cpu_ms")
        .and_then(Value::as_f64)
}

pub struct PhaseTimer {
    began: Instant,
    cpu: Option<f64>,
}

impl PhaseTimer {
    pub fn start(enabled: bool) -> Option<Self> {
        enabled.then(|| Self {
            began: Instant::now(),
            cpu: process_cpu_ms(),
        })
    }
    pub fn finish(self) -> Value {
        let mut result = json!({"elapsed_ms":self.began.elapsed().as_secs_f64()*1000.0});
        if let Some(cpu) = self
            .cpu
            .zip(process_cpu_ms())
            .map(|(before, after)| (after - before).max(0.0))
        {
            result["cpu_ms"] = json!(cpu);
        }
        result
    }
}

pub fn combine_phases(first: Option<Value>, second: Option<Value>) -> Option<Value> {
    match (first, second) {
        (Some(first), Some(second)) => {
            let mut total = json!({"elapsed_ms":first["elapsed_ms"].as_f64()? + second["elapsed_ms"].as_f64()?});
            if let Some(cpu) = first["cpu_ms"]
                .as_f64()
                .zip(second["cpu_ms"].as_f64())
                .map(|(a, b)| a + b)
            {
                total["cpu_ms"] = json!(cpu);
            }
            Some(total)
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(windows)]
fn process_metrics() -> Map<String, Value> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let mut metrics = Map::new();
    // The pseudo-handle belongs to this process and all output structures are initialized.
    unsafe {
        let process = GetCurrentProcess();
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        counters.cb = size;
        if GetProcessMemoryInfo(process, &mut counters, size) != 0 {
            metrics.insert("rss_bytes".into(), json!(counters.WorkingSetSize));
            metrics.insert("peak_rss_bytes".into(), json!(counters.PeakWorkingSetSize));
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
            let ticks = |time: FILETIME| {
                (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
            };
            let cpu_ms = (ticks(kernel) as f64 + ticks(user) as f64) / 10_000.0;
            metrics.insert("process_cpu_ms".into(), json!(cpu_ms));
        }
    }
    metrics
}

#[cfg(not(any(unix, windows)))]
fn process_metrics() -> Map<String, Value> {
    Map::new()
}

fn project(value: &Value) -> Option<Value> {
    let kind = value
        .get("kind")?
        .as_str()
        .filter(|kind| KINDS.contains(kind))?;
    let mut result = Map::new();
    result.insert("kind".into(), json!(kind));
    for field in FIELDS {
        if let Some(number) = value
            .get(field)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && (0.0..=1e18).contains(n))
        {
            let _ = number;
            result.insert((*field).into(), value[*field].clone());
        }
    }
    for (field, limit) in [("timestamp", 40), ("session", 32), ("revision", 40)] {
        if let Some(text) = value.get(field).and_then(Value::as_str).filter(|text| {
            text.len() <= limit && text.bytes().all(|b| b"0123456789abcdef-:.TZ+".contains(&b))
        }) {
            result.insert(field.into(), json!(text));
        }
    }
    if let Some(phases) = value.get("phase_metrics").and_then(Value::as_object) {
        let mut output = Map::new();
        for (name, metrics) in phases {
            if !PHASES.contains(&name.as_str()) {
                continue;
            }
            let Some(metrics) = metrics.as_object() else {
                continue;
            };
            let mut selected = Map::new();
            for key in ["cpu_ms", "elapsed_ms"] {
                if metrics
                    .get(key)
                    .and_then(Value::as_f64)
                    .is_some_and(|n| n.is_finite() && (0.0..=1e18).contains(&n))
                {
                    selected.insert(key.into(), metrics[key].clone());
                }
            }
            output.insert(name.clone(), Value::Object(selected));
        }
        result.insert("phase_metrics".into(), Value::Object(output));
    }
    Some(Value::Object(result))
}

pub struct PerformanceDebug {
    path: PathBuf,
    records: VecDeque<Value>,
    enabled: bool,
    health: &'static str,
    error_type: Option<&'static str>,
    invalid: bool,
    session: String,
    revision: String,
    started: Instant,
    active_streams: usize,
}
impl PerformanceDebug {
    pub fn new(directory: &Path, revision: String) -> Self {
        let seed = format!("{}:{:?}", std::process::id(), SystemTime::now());
        let mut result = Self {
            path: directory.join("performance-debug.jsonl"),
            records: VecDeque::new(),
            enabled: false,
            health: "disabled",
            error_type: None,
            invalid: false,
            session: format!("{:x}", Sha256::digest(seed))[..32].into(),
            revision,
            started: Instant::now(),
            active_streams: 0,
        };
        result.load();
        result
    }
    fn paths(&self) -> Vec<PathBuf> {
        (0..4)
            .map(|index| {
                if index == 0 {
                    self.path.clone()
                } else {
                    self.path
                        .with_file_name(format!("performance-debug.jsonl.{index}"))
                }
            })
            .collect()
    }
    fn check_paths(&self) -> io::Result<()> {
        crate::journal::safe(self.path.parent().expect("parent"))?;
        for path in self.paths() {
            crate::journal::safe(&path)?;
        }
        Ok(())
    }
    fn load(&mut self) {
        if let Err(error) = self.load_inner() {
            self.health = "unavailable";
            self.error_type = Some(if error.kind() == io::ErrorKind::InvalidData {
                self.invalid = true;
                "ValueError"
            } else {
                "OSError"
            });
        }
    }
    fn load_inner(&mut self) -> io::Result<()> {
        self.check_paths()?;
        for path in self.paths().into_iter().rev() {
            let file = match fs::File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let size = file.metadata()?.len();
            let mut reader = BufReader::new(file);
            let mut first = Vec::new();
            (&mut reader).take(8192).read_until(b'\n', &mut first)?;
            let first: Value = serde_json::from_slice(&first)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid debug header"))?;
            if first["application"] != "local-activity-monitor"
                || first["version"] != 1
                || project(&first).is_none()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid debug header",
                ));
            }
            reader.seek(SeekFrom::Start(size.saturating_sub(READ_BYTES)))?;
            let mut bytes = Vec::new();
            reader.take(READ_BYTES).read_to_end(&mut bytes)?;
            let mut lines: Vec<_> = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .collect();
            if size > READ_BYTES && !lines.is_empty() {
                lines.remove(0);
            }
            for line in lines
                .into_iter()
                .rev()
                .take(RECORD_LIMIT)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                if let Some(record) = serde_json::from_slice::<Value>(line)
                    .ok()
                    .filter(|item| {
                        item["application"] == "local-activity-monitor" && item["version"] == 1
                    })
                    .and_then(|item| project(&item))
                {
                    self.push(record);
                }
            }
        }
        Ok(())
    }
    fn push(&mut self, record: Value) {
        if self.records.len() == RECORD_LIMIT {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn connection(&mut self, opened: bool) {
        self.active_streams = if opened {
            self.active_streams.saturating_add(1)
        } else {
            self.active_streams.saturating_sub(1)
        };
        self.record(
            if opened {
                "stream_open"
            } else {
                "stream_close"
            },
            json!({"active_streams":self.active_streams}),
        );
    }
    pub fn stream_frame(
        &mut self,
        heartbeat: bool,
        serialization_ms: f64,
        snapshot_bytes: usize,
    ) -> Option<Value> {
        self.record(if heartbeat {"heartbeat"} else {"stream_frame"}, json!({"serialization_ms":serialization_ms,"snapshot_bytes":snapshot_bytes,"active_streams":self.active_streams}));
        self.enabled.then(|| self.snapshot())
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        if enabled {
            self.enabled = true;
            self.record("enabled", json!({}));
        } else {
            self.record("disabled", json!({}));
            self.enabled = false;
        }
    }
    pub fn record(&mut self, kind: &str, metrics: Value) {
        if !self.enabled || !KINDS.contains(&kind) {
            return;
        }
        let mut value = metrics.as_object().cloned().unwrap_or_default();
        value.insert("kind".into(), json!(kind));
        value.insert(
            "timestamp".into(),
            json!(DateTime::<Utc>::from(SystemTime::now())
                .to_rfc3339_opts(SecondsFormat::Millis, true)),
        );
        value.insert("session".into(), json!(self.session));
        value.insert("revision".into(), json!(self.revision));
        value.insert(
            "uptime_ms".into(),
            json!(self.started.elapsed().as_secs_f64() * 1000.0),
        );
        value.extend(process_metrics());
        let Some(record) = project(&Value::Object(value)) else {
            return;
        };
        self.push(record.clone());
        if self.invalid {
            return;
        }
        let outcome = self.append(&record);
        match outcome {
            Ok(()) => {
                self.health = "ok";
                self.error_type = None;
            }
            Err(_) => {
                self.health = "unavailable";
                self.error_type = Some("OSError");
            }
        }
    }
    fn append(&self, record: &Value) -> io::Result<()> {
        self.check_paths()?;
        let parent = self.path.parent().expect("parent");
        let mut dir = fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            dir.mode(0o700);
        }
        dir.create(parent)?;
        let mut record = record.clone();
        record["application"] = json!("local-activity-monitor");
        record["version"] = json!(1);
        let mut raw = serde_json::to_vec(&record).map_err(io::Error::other)?;
        raw.push(b'\n');
        let paths = self.paths();
        if fs::metadata(&paths[0]).is_ok_and(|meta| meta.len() + raw.len() as u64 > FILE_BYTES) {
            for index in (1..paths.len()).rev() {
                if paths[index - 1].exists() {
                    fs::rename(&paths[index - 1], &paths[index])?;
                }
            }
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(&raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Value {
        let files: Vec<_> = self
            .paths()
            .iter()
            .filter_map(|path| {
                let meta = fs::symlink_metadata(path).ok()?;
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return None;
                }
                Some(json!({"name":path.file_name()?.to_str()?,"bytes":meta.len()}))
            })
            .collect();
        let bytes: u64 = files.iter().filter_map(|file| file["bytes"].as_u64()).sum();
        json!({"enabled":self.enabled,"health":self.health,"error_type":self.error_type,"active_streams":self.active_streams,"byte_limit":FILE_BYTES*4,"bytes":bytes,"files":files,"path":self.path.to_string_lossy(),"record_limit":RECORD_LIMIT,"records":self.records.iter().rev().collect::<Vec<_>>()})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(unix, windows))]
    #[test]
    fn records_available_process_cpu_and_peak_memory() {
        let folder = tempfile::tempdir().unwrap();
        let mut debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        debug.set_enabled(true);
        let snapshot = debug.snapshot();
        let record = &snapshot["records"][0];
        assert!(record["process_cpu_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
        assert!(record["peak_rss_bytes"]
            .as_u64()
            .is_some_and(|value| value > 0));
        #[cfg(windows)]
        assert!(record["rss_bytes"].as_u64().is_some_and(|value| value > 0));
    }
    #[test]
    fn invalid_existing_debug_file_is_preserved() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        fs::write(&path, b"not a debug record\n").unwrap();
        let mut debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        debug.set_enabled(true);
        assert_eq!(debug.snapshot()["health"], "unavailable");
        assert_eq!(fs::read(path).unwrap(), b"not a debug record\n");
    }
    #[test]
    fn debug_is_opt_in_and_projects_numeric_metrics() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        let mut debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        debug.record("collection", json!({"refresh_ms":2,"secret":"private"}));
        assert!(!path.exists());
        debug.set_enabled(true);
        debug.record("collection", json!({"refresh_ms":2,"secret":"private"}));
        debug.set_enabled(false);
        let records = fs::read_to_string(path).unwrap();
        assert!(!records.contains("private"));
        assert_eq!(records.lines().count(), 3);
    }
}
