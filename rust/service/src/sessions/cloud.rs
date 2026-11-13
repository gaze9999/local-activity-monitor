//! Explicit, bounded Codex Cloud metadata reads.
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::Ordering, Arc, LazyLock},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::Mutex,
    time::{timeout, Instant},
};

static CURSOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_+/=:.\-]{1,2048}$").unwrap());
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{1,160}$").unwrap());
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_]{1,40}$").unwrap());
const LIMIT: usize = 1024 * 1024;

pub fn valid_cursor(value: &str) -> bool {
    CURSOR.is_match(value)
}
fn text(value: &Value, limit: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then(|| value.chars().take(limit).collect())
}
fn timestamp(value: &Value) -> Option<String> {
    let time = if let Some(s) = value.as_str() {
        if s.len() > 64 || !s.contains('T') {
            return None;
        }
        DateTime::parse_from_rfc3339(s).ok()?.with_timezone(&Utc)
    } else {
        let n = value.as_f64()?;
        if !n.is_finite() {
            return None;
        }
        let sec = n.floor();
        if sec < i64::MIN as f64 || sec >= i64::MAX as f64 {
            return None;
        }
        DateTime::<Utc>::from_timestamp(sec as i64, ((n - sec) * 1_000_000_000.0) as u32)?
    };
    Some(time.to_rfc3339_opts(SecondsFormat::Millis, true))
}
fn project(value: &Value) -> Option<Value> {
    let rows = value.get("tasks")?.as_array()?;
    if rows.len() > 20 {
        return None;
    }
    let mut items = Vec::new();
    for row in rows {
        let id = row.get("id")?.as_str().filter(|s| ID.is_match(s))?;
        let mut item = json!({"id":id,"title":text(&row["title"],512),"updated_at":timestamp(&row["updated_at"]),"environment_label":text(&row["environment_label"],160),"status":row["status"].as_str().filter(|s|STATUS.is_match(s))});
        if let Some(v) = row["is_review"].as_bool() {
            item["is_review"] = json!(v)
        }
        if let Some(v) = row["attempt_total"].as_u64().filter(|v| *v <= 1_000_000) {
            item["attempt_total"] = json!(v)
        }
        items.push(item);
    }
    let cursor = match value.get("cursor") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().filter(|s| valid_cursor(s))?),
    };
    Some(json!({"items":items,"cursor":cursor}))
}
async fn read(
    executable: &Path,
    home: &Path,
    cursor: Option<&str>,
    duration: Duration,
) -> Option<Value> {
    let mut command = Command::new(executable);
    command
        .args([
            "cloud",
            "list",
            "--json",
            "--limit",
            "20",
            "-c",
            "analytics.enabled=false",
        ])
        .env("CODEX_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(cursor) = cursor {
        command.args(["--cursor", cursor]);
    }
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let result = timeout(duration, async {
        let mut raw = Vec::new();
        stdout
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut raw)
            .await
            .ok()?;
        if raw.len() > LIMIT || !child.wait().await.ok()?.success() {
            return None;
        }
        project(&serde_json::from_slice::<Value>(&raw).ok()?)
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
pub struct Source {
    home: PathBuf,
    cache: Mutex<VecDeque<(Option<String>, Instant, Value)>>,
    shared: Arc<super::account::Shared>,
}
impl Source {
    pub fn new(home: PathBuf, shared: Arc<super::account::Shared>) -> Self {
        Self {
            home,
            shared,
            cache: Mutex::new(VecDeque::new()),
        }
    }
    pub fn settings(&self) -> super::account::Settings {
        self.shared.settings()
    }
    pub fn configure(&self, settings: super::account::Settings) {
        self.shared.configure(settings);
    }
    pub async fn query(&self, cursor: Option<&str>) -> Value {
        let mut cache = self.cache.lock().await;
        if let Some((_, _, v)) = cache.iter().find(|(key, time, _)| {
            key.as_deref() == cursor && time.elapsed() < Duration::from_secs(60)
        }) {
            if cursor.is_none() {
                self.shared.publish(v.clone());
            }
            return v.clone();
        }
        let mut result = json!({"health":"unavailable","items":[],"cursor":null,"checked_at":DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339()});
        if let Some(executable) = super::account::executable() {
            if let Some(value) =
                read(&executable, &self.home, cursor, Duration::from_secs(10)).await
            {
                result["health"] = json!("ok");
                result["items"] = value["items"].clone();
                result["cursor"] = value["cursor"].clone();
            } else {
                result["health"] = json!("error");
            }
        }
        if let Some(index) = cache
            .iter()
            .position(|(key, _, _)| key.as_deref() == cursor)
        {
            cache[index] = (cursor.map(str::to_owned), Instant::now(), result.clone());
        } else {
            if cache.len() >= 10 {
                cache.pop_front();
            }
            cache.push_back((cursor.map(str::to_owned), Instant::now(), result.clone()));
        }
        if cursor.is_none() {
            self.shared.publish(result.clone());
        }
        result
    }
    pub async fn run(self: Arc<Self>) {
        loop {
            let settings = self.shared.settings();
            if !settings.enabled {
                self.shared.notify.notified().await;
                continue;
            }
            let last = *self
                .shared
                .completed
                .read()
                .unwrap_or_else(|error| error.into_inner());
            if self.shared.force_due.swap(false, Ordering::AcqRel) {
                if self.shared.settings().enabled {
                    self.query(None).await;
                }
                continue;
            }
            let due = last
                .map(|last| last + Duration::from_secs(settings.interval))
                .unwrap_or_else(std::time::Instant::now);
            tokio::select! {
                _ = self.shared.notify.notified() => {},
                _ = tokio::time::sleep_until(Instant::from_std(due)) => { self.query(None).await; }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_child_passes_cursor_and_uses_selected_home() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let executable = home.path().join("codex");
        std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$CODEX_HOME/args\"\nprintf '%s\\n' '{\"tasks\":[{\"id\":\"task_1\",\"title\":\"Example\",\"secret\":\"private\"}],\"cursor\":null}'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let value = read(
            &executable,
            home.path(),
            Some("abc+/="),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(value["items"][0]["id"], "task_1");
        assert!(!value.to_string().contains("private"));
        assert_eq!(
            std::fs::read_to_string(home.path().join("args"))
                .unwrap()
                .trim(),
            "cloud list --json --limit 20 -c analytics.enabled=false --cursor abc+/="
        );
    }
    #[test]
    fn projection_validates_and_minimizes() {
        let v=project(&json!({"tasks":[{"id":"task_1","title":" test ","status":"done","updated_at":"2026-10-02T03:04:05Z","attempt_total":0,"secret":"private"}],"cursor":"a+/="})).unwrap();
        assert_eq!(v["items"][0]["title"], "test");
        assert_eq!(v["items"][0]["attempt_total"], 0);
        assert!(!v.to_string().contains("private"));
        assert!(project(&json!({"tasks":[{"id":"bad/id"}]})).is_none());
        assert!(project(&json!({"tasks":[],"cursor":"bad cursor"})).is_none());
        assert!(project(&json!({"tasks":vec![json!({"id":"x"});21]})).is_none());
    }
}
