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
pub(crate) fn process_metrics() -> Map<String, Value> {
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

pub struct CollectionStatus {
    phase: &'static str,
    started: Instant,
    cpu: Option<f64>,
    revision: u64,
}
impl CollectionStatus {
    pub fn new() -> Self {
        Self {
            phase: "waiting",
            started: Instant::now(),
            cpu: None,
            revision: 0,
        }
    }
    pub fn set(&mut self, phase: &'static str) {
        self.phase = phase;
        self.started = Instant::now();
        self.cpu = process_cpu_ms();
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn snapshot(&self) -> Value {
        if ["waiting", "idle", "stopped", "error"].contains(&self.phase) {
            return json!({"phase":self.phase,"phase_ms":0,"phase_cpu_ms":0});
        }
        let elapsed = round_millis(self.started.elapsed().as_secs_f64() * 1000.0);
        let cpu = self
            .cpu
            .zip(process_cpu_ms())
            .map(|(before, after)| round_millis((after - before).max(0.0)));
        json!({"phase":self.phase,"phase_ms":elapsed,"phase_cpu_ms":cpu})
    }
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
        phase_metrics(
            self.began.elapsed().as_secs_f64() * 1000.0,
            self.cpu.zip(process_cpu_ms()).map(|(before, after)| (after - before).max(0.0)),
        )
    }
}

fn round_millis(value: f64) -> f64 {
    format!("{value:.2}").parse::<f64>().expect("finite phase duration")
}

fn phase_metrics(elapsed_ms: f64, cpu_ms: Option<f64>) -> Value {
    let mut result = json!({"elapsed_ms":round_millis(elapsed_ms)});
    if let Some(cpu_ms) = cpu_ms {
        result["cpu_ms"] = json!(round_millis(cpu_ms));
    }
    result
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
pub(crate) fn process_metrics() -> Map<String, Value> {
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
pub(crate) fn process_metrics() -> Map<String, Value> {
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
            let first = crate::json_bytes::parse(&first)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid debug header"))?;
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
            let mut lines: Vec<_> = crate::journal::byte_lines(&bytes).collect();
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
                if let Some(record) = crate::json_bytes::parse(line)
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
        transfer_bytes: usize,
        write_ms: f64,
        cpu_ms: Option<f64>,
    ) -> Option<Value> {
        let mut metrics = json!({"serialization_ms":serialization_ms,"snapshot_bytes":snapshot_bytes,"transfer_bytes":transfer_bytes,"write_ms":write_ms,"active_streams":self.active_streams});
        if let Some(cpu_ms) = cpu_ms {
            metrics["cpu_ms"] = json!(cpu_ms);
        }
        self.record(if heartbeat {"heartbeat"} else {"stream_frame"}, metrics);
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
            json!(round_millis(self.started.elapsed().as_secs_f64() * 1000.0)),
        );
        let mut process = process_metrics();
        if let Some(cpu) = process.get("process_cpu_ms").and_then(Value::as_f64) {
            process.insert("process_cpu_ms".into(), json!(round_millis(cpu)));
        }
        value.extend(process);
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
    #[test]
    fn live_collection_rounding_matches_python_and_idle_states_stay_zero() {
        for (value, expected) in [(0.125, 0.12), (0.375, 0.38), (2.675, 2.67),
            (1.225, 1.23), (1.005, 1.0), (12.34567, 12.35), (0.0, 0.0)] {
            assert_eq!(round_millis(value), expected, "{value}");
        }
        let mut collection = CollectionStatus::new();
        for phase in ["waiting", "idle", "stopped", "error"] {
            collection.set(phase);
            let revision = collection.revision();
            assert_eq!(collection.snapshot(), json!({"phase":phase,"phase_ms":0,"phase_cpu_ms":0}));
            assert_eq!(collection.revision(), revision);
        }
        collection.set("sessions");
        collection.cpu = None;
        assert_eq!(collection.snapshot()["phase_cpu_ms"], Value::Null);
    }

    #[test]
    fn completed_phase_metrics_round_like_python_and_preserve_missing_cpu() {
        for (value, expected) in [(2.675, 2.67), (1.225, 1.23), (0.125, 0.12),
            (0.375, 0.38), (1.005, 1.0), (12.34567, 12.35), (0.0, 0.0)] {
            assert_eq!(phase_metrics(value, Some(value)),
                json!({"elapsed_ms":expected,"cpu_ms":expected}), "{value}");
            assert_eq!(phase_metrics(value, None), json!({"elapsed_ms":expected}));
        }
    }

    fn fixture_row(value: u64) -> Vec<u8> {
        json!({"application":"local-activity-monitor","version":1,"kind":"collection","refresh_ms":value,"secret":"PRIVATE"}).to_string().into_bytes()
    }

    fn encoded(text: &[u8], width: usize, big_endian: bool, bom: bool) -> Vec<u8> {
        let mut raw = if bom {
            match (width, big_endian) {
                (1, _) => b"\xef\xbb\xbf".to_vec(),
                (2, false) => b"\xff\xfe".to_vec(),
                (2, true) => b"\xfe\xff".to_vec(),
                (4, false) => b"\xff\xfe\0\0".to_vec(),
                (4, true) => b"\0\0\xfe\xff".to_vec(),
                _ => unreachable!(),
            }
        } else { Vec::new() };
        for byte in text {
            assert!(byte.is_ascii());
            match width {
                1 => raw.push(*byte),
                2 => raw.extend(if big_endian { u16::from(*byte).to_be_bytes() } else { u16::from(*byte).to_le_bytes() }),
                4 => raw.extend(if big_endian { u32::from(*byte).to_be_bytes() } else { u32::from(*byte).to_le_bytes() }),
                _ => unreachable!(),
            }
        }
        raw
    }

    #[test]
    fn encoded_header_retains_raw_readline_and_body_splitlines_boundaries() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        let row = fixture_row(2);
        for (width, big_endian) in [(1, false), (2, false), (2, true), (4, false), (4, true)] {
            for bom in [false, true] {
                for separator in [vec![], b"\n".to_vec(), b"\r\n".to_vec(), encoded(b"\n", width, big_endian, false)] {
                    let raw = [encoded(&row, width, big_endian, bom), separator.clone()].concat();
                    fs::write(&path, &raw).unwrap();
                    let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
                    let encoded_lf = width != 1 && separator == encoded(b"\n", width, big_endian, false);
                    let invalid = width != 1 && !separator.is_empty() && !(encoded_lf && big_endian);
                    assert_eq!(debug.invalid, invalid, "{width} {big_endian} {bom} {separator:?}");
                    assert_eq!(debug.records.len(), usize::from(!invalid && !encoded_lf));
                    assert_eq!(fs::read(&path).unwrap(), raw);
                }
            }
        }
    }

    #[test]
    fn encoded_body_rows_follow_valid_header_and_raw_byte_caps() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        let header = fixture_row(1);
        let row = fixture_row(2);
        for (width, big_endian) in [(1, false), (2, false), (2, true), (4, false), (4, true)] {
            for bom in [false, true] {
                for separator in [vec![], b"\n".to_vec(), b"\r".to_vec(), b"\r\n".to_vec(), encoded(b"\n", width, big_endian, false)] {
                    let raw = [header.as_slice(), b"\n", &encoded(&row, width, big_endian, bom), &separator].concat();
                    fs::write(&path, &raw).unwrap();
                    let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
                    let encoded_lf = width != 1 && separator == encoded(b"\n", width, big_endian, false);
                    assert!(!debug.invalid);
                    assert_eq!(debug.records.len(), 1 + usize::from(!(encoded_lf && big_endian)), "{width} {big_endian} {bom} {separator:?}");
                    assert!(!debug.snapshot().to_string().contains("PRIVATE"));
                    assert_eq!(fs::read(&path).unwrap(), raw);
                }
            }
        }
        let raw = [header.as_slice(), b"\n", &vec![b'x'; READ_BYTES as usize], b"\r\n", &encoded(&row, 4, false, true)].concat();
        fs::write(&path, &raw).unwrap();
        let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        assert_eq!(debug.records.len(), 1);
        assert_eq!(debug.records[0]["refresh_ms"], 2);
        assert_eq!(fs::read(&path).unwrap(), raw);
        let oversized = String::from_utf8(header).unwrap().replace("PRIVATE", &"x".repeat(2200));
        let raw = encoded(oversized.as_bytes(), 4, false, true);
        fs::write(&path, &raw).unwrap();
        let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        assert!(debug.invalid);
        assert!(debug.records.is_empty());
        assert_eq!(fs::read(&path).unwrap(), raw);
    }

    #[test]
    fn reload_uses_raw_line_boundaries_and_counts_blank_rows_before_retention() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        let a = fixture_row(1);
        let b = fixture_row(2);
        let c = fixture_row(3);
        for (raw, expected) in [
            ([a.as_slice(), b"\n", &b, b"\r", &c, b"\r\n"].concat(), vec![1, 2, 3]),
            ([a.as_slice(), b"\n", &vec![b'\n'; 150]].concat(), vec![]),
            ([a.as_slice(), b"\n", &vec![b'\n'; 149], &b, b"\n"].concat(), vec![2]),
            ([a.as_slice(), b"\n", &vec![b'\n'; 148], &b, b"\n"].concat(), vec![1, 2]),
        ] {
            fs::write(&path, &raw).unwrap();
            let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
            assert!(!debug.invalid);
            assert_eq!(debug.records.iter().map(|row| row["refresh_ms"].as_u64().unwrap()).collect::<Vec<_>>(), expected);
            assert!(!debug.snapshot().to_string().contains("PRIVATE"));
            assert_eq!(fs::read(&path).unwrap(), raw);
        }
    }

    #[test]
    fn reload_discards_first_raw_tail_row_before_blank_row_filtering() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("performance-debug.jsonl");
        let a = fixture_row(1);
        let b = fixture_row(2);
        let mut tail = vec![b'x'; READ_BYTES as usize - b.len() - 4];
        tail.extend([b"\r\n".as_slice(), &b, b"\r\n"].concat());
        let raw = [a.as_slice(), b"\n", &tail].concat();
        fs::write(&path, &raw).unwrap();
        let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        assert!(!debug.invalid);
        assert_eq!(debug.records.len(), 1);
        assert_eq!(debug.records[0]["refresh_ms"], 2);
        assert_eq!(fs::read(&path).unwrap(), raw);
        let mut tail = [b"\r\n".as_slice(), &b, b"\n"].concat();
        tail.resize(READ_BYTES as usize, b'x');
        let raw = [a.as_slice(), b"\n", &tail].concat();
        fs::write(&path, &raw).unwrap();
        let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        assert_eq!(debug.records.len(), 1);
        assert_eq!(debug.records[0]["refresh_ms"], 2);
        assert_eq!(fs::read(&path).unwrap(), raw);
        let raw = [a.as_slice(), b"\r", &b].concat();
        fs::write(&path, &raw).unwrap();
        let debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        assert!(debug.invalid);
        assert!(debug.records.is_empty());
        assert_eq!(fs::read(&path).unwrap(), raw);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn generated_debug_timing_precision_matches_persisted_records() {
        let folder = tempfile::tempdir().unwrap();
        let mut debug = PerformanceDebug::new(folder.path(), "abcdef012345".into());
        debug.set_enabled(true);
        debug.record("collection", json!({"refresh_ms":1.234567,"secret":"PRIVATE"}));
        let snapshot = debug.snapshot();
        let records = snapshot["records"].as_array().unwrap();
        let saved: Vec<Value> = fs::read_to_string(folder.path().join("performance-debug.jsonl"))
            .unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(records.len(), 2);
        assert_eq!(saved.len(), 2);
        for (record, persisted) in records.iter().rev().zip(&saved) {
            for key in ["uptime_ms", "process_cpu_ms"] {
                if let Some(value) = record[key].as_f64() {
                    assert_eq!(value, round_millis(value), "{key}");
                }
                assert_eq!(record.get(key), persisted.get(key));
            }
            assert!(record["peak_rss_bytes"].as_u64().is_some());
            assert_eq!(record["peak_rss_bytes"], persisted["peak_rss_bytes"]);
            assert!(!persisted.to_string().contains("PRIVATE"));
        }
        // Supplied measurements retain their own precision; only generated timing is rounded.
        assert_eq!(records[0]["refresh_ms"], 1.234567);
    }

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
