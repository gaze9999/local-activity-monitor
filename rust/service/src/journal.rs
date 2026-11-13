use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const LIMIT: u64 = 64 * 1024;
const RECORDS: usize = 1000;
const EVENTS: usize = 200;

pub(crate) fn safe(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = meta.is_symlink();
            if redirected || (!meta.is_file() && !meta.is_dir()) {
                Err(io::Error::other("unsafe journal path"))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn simple(value: &str, max: usize, first_alpha: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && (!first_alpha || value.as_bytes()[0].is_ascii_alphabetic())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn project(value: Value) -> Option<Value> {
    if value["application"] != "local-activity-monitor" || value["version"] != 1 {
        return None;
    }
    let timestamp = value["timestamp"].as_str()?;
    chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let kind = value["kind"].as_str()?;
    if kind.is_empty()
        || kind.len() > 80
        || !kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return None;
    }
    let error = &value["error_type"];
    if !error.is_null() && !error.as_str().is_some_and(|s| simple(s, 80, false)) {
        return None;
    }
    let mut event = json!({"timestamp":timestamp,"kind":kind,"error_type":error});
    if let Some(status) = value["http_status"]
        .as_u64()
        .filter(|v| (100..=599).contains(v))
    {
        event["http_status"] = json!(status);
    }
    if kind == "frontend_error" {
        let phase = value["phase"].as_str()?;
        if !matches!(phase, "render" | "script" | "promise")
            || !error.as_str().is_some_and(|s| simple(s, 80, true))
        {
            return None;
        }
        let revision = &value["frontend_revision"];
        if !revision.is_null()
            && !revision.as_str().is_some_and(|s| {
                s.len() == 12
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return None;
        }
        let frames = value["frames"].as_array().filter(|f| f.len() <= 8)?;
        for frame in frames {
            let object = frame.as_object().filter(|o| o.len() == 4)?;
            if !matches!(
                object.get("file").and_then(Value::as_str),
                Some("index.html" | "app.js" | "workbench-ui.js")
            ) {
                return None;
            }
            let function = object.get("function").and_then(Value::as_str)?;
            if function.is_empty()
                || function.len() > 80
                || !function
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.$<>".contains(&b))
            {
                return None;
            }
            if !["line", "column"].iter().all(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n <= 10_000_000)
            }) {
                return None;
            }
        }
        event["phase"] = json!(phase);
        event["frontend_revision"] = revision.clone();
        event["frames"] = json!(frames);
    }
    Some(event)
}

pub struct Journal {
    path: PathBuf,
    logs: VecDeque<Value>,
    events: VecDeque<Value>,
    health: &'static str,
    error_type: Option<&'static str>,
    checked_at: Option<String>,
    bytes: u64,
    skipped: u64,
    client_reports: VecDeque<(std::time::Instant, String)>,
    last_event: Option<(String, Option<u16>)>,
    enabled: bool,
}

impl Journal {
    pub fn new(directory: &Path) -> Self {
        let mut journal = Self {
            path: directory.join("local-activity-monitor.jsonl"),
            logs: VecDeque::new(),
            events: VecDeque::new(),
            health: "waiting",
            error_type: None,
            checked_at: None,
            bytes: 0,
            skipped: 0,
            client_reports: VecDeque::new(),
            last_event: None,
            enabled: true,
        };
        let result = journal.load();
        journal.update_health(result);
        journal
    }
    fn previous(&self) -> PathBuf {
        self.path.with_file_name("local-activity-monitor.jsonl.1")
    }
    fn update_health(&mut self, result: io::Result<()>) {
        match result {
            Ok(()) => {
                self.health = "ok";
                self.error_type = None;
            }
            Err(error) => {
                self.health = "unavailable";
                self.error_type = Some(match error.kind() {
                    io::ErrorKind::PermissionDenied => "PermissionError",
                    io::ErrorKind::NotFound => "FileNotFoundError",
                    _ => "OSError",
                });
            }
        }
        self.checked_at = Some(
            chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        );
    }
    fn push(&mut self, event: Value) {
        if self.logs.len() == RECORDS {
            self.logs.pop_front();
        }
        self.logs.push_back(event);
    }
    fn push_live(&mut self, event: Value) {
        if self.events.len() == EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event.clone());
        self.push(event);
    }
    pub fn events(&self) -> Vec<Value> {
        self.events.iter().cloned().collect()
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    fn load(&mut self) -> io::Result<()> {
        safe(self.path.parent().expect("journal parent"))?;
        for path in [self.previous(), self.path.clone()] {
            safe(&path)?;
            let mut file = match fs::File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let size = file.metadata()?.len();
            self.bytes += size;
            file.seek(SeekFrom::Start(size.saturating_sub(LIMIT)))?;
            let mut bytes = Vec::new();
            file.take(LIMIT).read_to_end(&mut bytes)?;
            let mut lines = bytes.split(|byte| *byte == b'\n');
            if size > LIMIT {
                lines.next();
            }
            for line in lines.filter(|line| !line.is_empty()) {
                match serde_json::from_slice::<Value>(line).ok().and_then(project) {
                    Some(event) => self.push(event),
                    None => self.skipped += 1,
                }
            }
        }
        Ok(())
    }
    pub fn event(&mut self, kind: &str) {
        let now = chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let event = json!({"timestamp":now,"kind":kind,"error_type":null});
        self.last_event = Some((kind.to_owned(), None));
        self.push_live(event.clone());
        if self.enabled {
            let result = self.append(&event);
            self.update_health(result);
        }
    }
    pub fn frontend_failed(&mut self, value: Value) -> Result<bool, ()> {
        let object = value
            .as_object()
            .filter(|object| object.len() == 4)
            .ok_or(())?;
        if !["phase", "error_type", "frames", "frontend_revision"]
            .iter()
            .all(|key| object.contains_key(*key))
        {
            return Err(());
        }
        let signature = serde_json::to_string(&value).map_err(|_| ())?;
        let now = chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut record = value;
        record["application"] = json!("local-activity-monitor");
        record["version"] = json!(1);
        record["timestamp"] = json!(now);
        record["kind"] = json!("frontend_error");
        let event = project(record).ok_or(())?;
        let instant = std::time::Instant::now();
        while self
            .client_reports
            .front()
            .is_some_and(|(time, _)| instant.duration_since(*time).as_secs() >= 60)
        {
            self.client_reports.pop_front();
        }
        if self.client_reports.len() >= 20
            || self
                .client_reports
                .iter()
                .any(|(_, previous)| previous == &signature)
        {
            return Ok(false);
        }
        self.client_reports.push_back((instant, signature));
        self.last_event = Some(("frontend_error".into(), None));
        self.push_live(event.clone());
        if self.enabled {
            let result = self.append(&event);
            self.update_health(result);
        }
        Ok(true)
    }
    pub fn http_error(&mut self, status: u16) -> bool {
        if status < 400
            || self.last_event.as_ref().is_some_and(|(kind, previous)| {
                kind == "http_response_error" && *previous == Some(status)
            })
        {
            return false;
        }
        let timestamp = chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let event = json!({"timestamp":timestamp,"kind":"http_response_error","error_type":null,"http_status":status});
        self.last_event = Some(("http_response_error".into(), Some(status)));
        self.push_live(event.clone());
        if self.enabled {
            let result = self.append(&event);
            self.update_health(result);
        }
        true
    }
    fn append(&mut self, event: &Value) -> io::Result<()> {
        let parent = self.path.parent().expect("journal parent");
        safe(parent)?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent)?;
        let previous = self.previous();
        safe(&self.path)?;
        safe(&previous)?;
        let mut record = event.clone();
        record["application"] = json!("local-activity-monitor");
        record["version"] = json!(1);
        let mut raw = serde_json::to_vec(&record).map_err(io::Error::other)?;
        raw.push(b'\n');
        let mut size = match fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if size + raw.len() as u64 > LIMIT {
            fs::rename(&self.path, &previous)?;
            size = 0;
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
        self.bytes = size + raw.len() as u64 + fs::metadata(previous).map(|m| m.len()).unwrap_or(0);
        Ok(())
    }
    pub fn entries(&self) -> Vec<Value> {
        if !self.enabled {
            return Vec::new();
        }
        self.logs
            .iter()
            .map(|event| {
                let mut event = event.clone();
                let kind = event["kind"].as_str().unwrap_or("");
                let severity = if matches!(
                    kind,
                    "refresh_failed" | "http_response_error" | "frontend_error"
                ) {
                    "error"
                } else {
                    "info"
                };
                event["source"] = json!("monitor");
                event["severity"] = json!(severity);
                event["code"] = event["kind"].clone();
                event["module"] = json!("local_activity_monitor");
                event["file"] = json!("local-activity-monitor.jsonl");
                event
            })
            .collect()
    }
    pub fn source(&self) -> Value {
        json!({"source":"monitor","health":if self.enabled {self.health} else {"disabled"},"checked_at":self.checked_at,"error_type":self.error_type,"files":[{"name":"local-activity-monitor.jsonl"}],"bytes":self.bytes,"byte_limit":LIMIT*2,"retained":self.logs.len(),"unsupported_lines":self.skipped})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_events_are_bounded_and_exclude_reloaded_log_history() {
        let home = tempfile::tempdir().unwrap();
        let mut first = Journal::new(home.path());
        first.event("started");
        drop(first);
        let mut restarted = Journal::new(home.path());
        assert!(restarted.events().is_empty());
        assert_eq!(restarted.entries().len(), 1);
        restarted.set_enabled(false);
        for index in 0..201 {
            assert!(restarted.http_error(400 + index % 200));
        }
        let events = restarted.events();
        assert_eq!(events.len(), 200);
        assert_eq!(events[0]["http_status"], 401);
        assert_eq!(events[199]["http_status"], 400);
        assert!(restarted.entries().is_empty());
    }

    #[test]
    fn frontend_reports_are_limited_to_twenty_distinct_entries() {
        let home = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(home.path());
        for line in 0..21 {
            let report = json!({"phase":"script","error_type":"TypeError","frontend_revision":null,"frames":[{"file":"app.js","function":"run","line":line,"column":0}]});
            assert_eq!(journal.frontend_failed(report), Ok(line < 20));
        }
    }
    #[test]
    fn loads_whitelisted_events_and_rotates_owned_log() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("local-activity-monitor.jsonl");
        fs::write(&path, format!("{{\"application\":\"local-activity-monitor\",\"version\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"kind\":\"started\",\"error_type\":null,\"secret\":\"private\"}}\n{}", "x".repeat(LIMIT as usize))).unwrap();
        let mut journal = Journal::new(home.path());
        journal.event("started");
        assert_eq!(journal.source()["health"], "ok");
        assert!(home.path().join("local-activity-monitor.jsonl.1").exists());
        assert!(!json!(journal.entries()).to_string().contains("private"));
        assert_eq!(
            Journal::new(home.path()).entries().last().unwrap()["kind"],
            "started"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_redirected_journal_without_touching_target() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("other");
        fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, home.path().join("local-activity-monitor.jsonl"))
            .unwrap();
        let mut journal = Journal::new(home.path());
        journal.event("started");
        assert_eq!(journal.source()["health"], "unavailable");
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
