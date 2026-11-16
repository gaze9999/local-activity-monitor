//! Opt-in, read-only Codex app-server account projection.
use chrono::{DateTime, Datelike, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::SyncSender,
    Arc, RwLock,
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, Notify};
use tokio::time::{timeout_at, Instant};

const METHODS: [&str; 3] = [
    "account/read",
    "account/rateLimits/read",
    "account/usage/read",
];
const LIMIT: usize = 1024 * 1024;
static LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9 _./:-]{1,120}$").unwrap());
static BALANCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-?\d{1,20}(?:\.\d{1,12})?$").unwrap());

fn label(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| LABEL.is_match(value))
}
fn integer(value: &Value) -> Option<i64> {
    value.as_i64().filter(|value| *value >= 0)
}
fn credits(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    let mut result = Map::new();
    for (input, output) in [("hasCredits", "has_credits"), ("unlimited", "unlimited")] {
        if let Some(value) = map.get(input).and_then(Value::as_bool) {
            result.insert(output.into(), json!(value));
        }
    }
    if let Some(value) = map.get("balance") {
        if value.is_null() || value.as_str().is_some_and(|text| BALANCE.is_match(text)) {
            result.insert("balance".into(), value.clone());
        }
    } else {
        result.insert("balance".into(), Value::Null);
    }
    (!result.is_empty()).then_some(Value::Object(result))
}
fn window(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    let used = map.get("usedPercent").and_then(integer);
    let mut result = json!({"used_percent":used,"remaining_percent":used.map(|value|100i64.saturating_sub(value).max(0))});
    if let Some(value) = map.get("windowDurationMins") {
        result["window_minutes"] = json!(integer(value));
    }
    if let Some(value) = map.get("resetsAt") {
        result["resets_at"] = json!(integer(value)
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
            .filter(|time| time.year() <= 9999)
            .map(|time| time.to_rfc3339()));
    }
    Some(result)
}
fn limits(payload: &Value, order: Option<&[String]>) -> Map<String, Value> {
    let mut entries = Vec::new();
    if let Some(values) = payload["rateLimitsByLimitId"].as_object() {
        for key in order.unwrap_or_default() {
            if let Some(value) = values.get(key) {
                entries.push((key.clone(), value));
            }
        }
    } else if payload["rateLimits"].is_object() {
        entries.push((
            label(&payload["rateLimits"]["limitId"])
                .unwrap_or("codex")
                .to_owned(),
            &payload["rateLimits"],
        ));
    }
    let mut result = Map::new();
    for (key, bucket) in entries {
        if !LABEL.is_match(&key) || !bucket.is_object() {
            continue;
        }
        let mut item = json!({"limit_id":label(&bucket["limitId"]).unwrap_or(&key)});
        for (input, output) in [
            ("limitName", "limit_name"),
            ("planType", "plan_type"),
            ("rateLimitReachedType", "reached_type"),
        ] {
            if bucket.get(input).is_some() {
                item[output] = json!(label(&bucket[input]));
            }
        }
        for field in ["primary", "secondary"] {
            if bucket.get(field).is_some() {
                item[field] = json!(window(&bucket[field]));
            }
        }
        if bucket.get("credits").is_some() {
            item["credits"] = json!(credits(&bucket["credits"]));
        }
        result.insert(key, item);
    }
    result
}
fn usage(payload: &Value) -> Value {
    let summary = payload["summary"].as_object().map(|values| {
        let mut result = Map::new();
        for key in [
            "lifetimeTokens",
            "peakDailyTokens",
            "longestRunningTurnSec",
            "currentStreakDays",
            "longestStreakDays",
        ] {
            result.insert(key.into(), json!(values.get(key).and_then(integer)));
        }
        Value::Object(result)
    });
    let daily = payload["dailyUsageBuckets"].as_array().map(|rows| {
        rows.iter()
            .take(366)
            .filter_map(|row| {
                let date = row["startDate"].as_str()?;
                let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
                if !(1..=9999).contains(&parsed.year())
                    || parsed.format("%Y-%m-%d").to_string() != date
                {
                    return None;
                }
                let tokens = integer(&row["tokens"])?;
                Some(json!({"start_date":date,"tokens":tokens}))
            })
            .collect::<Vec<_>>()
    });
    json!({"summary":summary,"daily_usage_buckets":daily})
}
pub(crate) fn empty(status: &str) -> Value {
    let methods: Map<_, _> = METHODS
        .iter()
        .map(|name| ((*name).to_owned(), json!({"health":status})))
        .collect();
    json!({"health":status,"updated_at":null,"limits":{},"limit_id":null,"credits":null,"plan_type":null,"account_usage":null,"methods":methods})
}

// Read only the final matching member, preserving JSON duplicate-key last-wins semantics.
fn raw_member<'a>(raw: &'a str, name: &str) -> Result<Option<&'a serde_json::value::RawValue>, serde_json::Error> {
    struct Member<'a>(&'a str);
    impl<'de> serde::de::Visitor<'de> for Member<'_> {
        type Value = Option<&'de serde_json::value::RawValue>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut result = None;
            while let Some(key) = map.next_key::<String>()? {
                if key == self.0 {
                    result = Some(map.next_value()?);
                } else {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
            Ok(result)
        }
    }
    serde::Deserializer::deserialize_map(&mut serde_json::Deserializer::from_str(raw), Member(name))
}

fn limit_order(line: &[u8]) -> Result<Option<Vec<String>>, serde_json::Error> {
    struct Keys;
    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = Vec<String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut keys = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                if keys.len() < 32 && !keys.contains(&key) {
                    keys.push(key);
                }
                map.next_value::<serde::de::IgnoredAny>()?;
            }
            Ok(keys)
        }
    }
    let raw: &serde_json::value::RawValue = serde_json::from_slice(line)?;
    let Some(result) = raw_member(raw.get(), "result")? else { return Ok(None) };
    let Some(mapped) = raw_member(result.get(), "rateLimitsByLimitId")? else { return Ok(None) };
    if !mapped.get().starts_with('{') {
        return Ok(None);
    }
    serde::Deserializer::deserialize_map(&mut serde_json::Deserializer::from_str(mapped.get()), Keys).map(Some)
}

#[derive(Debug)]
enum RpcError {
    Protocol,
    Remote(Option<i64>),
}
struct Rpc {
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    read: usize,
    deadline: Instant,
}
impl Rpc {
    async fn send(&mut self, method: &str, params: Value, id: Option<i64>) -> Result<(), RpcError> {
        let mut value = json!({"method":method,"params":params});
        if let Some(id) = id {
            value["id"] = json!(id);
        }
        let mut bytes = serde_json::to_vec(&value).map_err(|_| RpcError::Protocol)?;
        bytes.push(b'\n');
        timeout_at(self.deadline, self.input.write_all(&bytes))
            .await
            .map_err(|_| RpcError::Protocol)?
            .map_err(|_| RpcError::Protocol)?;
        timeout_at(self.deadline, self.input.flush())
            .await
            .map_err(|_| RpcError::Protocol)?
            .map_err(|_| RpcError::Protocol)
    }
    async fn request(&mut self, method: &str, params: Value, id: i64) -> Result<(Value, Option<Vec<String>>), RpcError> {
        self.send(method, params, Some(id)).await?;
        loop {
            let mut line = Vec::new();
            let remaining = LIMIT.saturating_sub(self.read);
            let count = timeout_at(
                self.deadline,
                (&mut self.output)
                    .take((remaining + 1) as u64)
                    .read_until(b'\n', &mut line),
            )
            .await
            .map_err(|_| RpcError::Protocol)?
            .map_err(|_| RpcError::Protocol)?;
            self.read += count;
            if count == 0 || self.read > LIMIT {
                return Err(RpcError::Protocol);
            }
            let value: Value = serde_json::from_slice(&line).map_err(|_| RpcError::Protocol)?;
            if !value.is_object() {
                return Err(RpcError::Protocol);
            }
            if value.get("method").is_some() || value["id"].as_i64() != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(RpcError::Remote(error["code"].as_i64()));
            }
            let payload = value.get("result").filter(|value| value.is_object())
                .cloned().ok_or(RpcError::Protocol)?;
            let order = if method == "account/rateLimits/read" {
                limit_order(&line).map_err(|_| RpcError::Protocol)?
            } else {
                None
            };
            return Ok((payload, order));
        }
    }
}

async fn read_from(executable: &Path, home: &Path, deadline: Duration) -> Value {
    let mut command = Command::new(executable);
    command
        .args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            "analytics.enabled=false",
        ])
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return empty("error"),
    };
    let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return empty("error");
    };
    let mut rpc = Rpc {
        input,
        output: BufReader::new(output),
        read: 0,
        deadline: Instant::now() + deadline,
    };
    let operation = async {
        let mut result = empty("unavailable");
        rpc.request("initialize",json!({"clientInfo":{"name":"local_activity_monitor","title":"Local Activity Monitor","version":env!("CARGO_PKG_VERSION")}}),0).await?;
        rpc.send("initialized", json!({}), None).await?;
        for (index, method) in METHODS.iter().enumerate() {
            match rpc
                .request(
                    method,
                    if *method == "account/read" {
                        json!({"refreshToken":false})
                    } else {
                        json!({})
                    },
                    (index + 1) as i64,
                )
                .await
            {
                Ok((payload, order)) => {
                    match *method {
                        "account/read" => {
                            result["plan_type"] = json!(label(&payload["account"]["planType"]))
                        }
                        "account/rateLimits/read" => {
                            let values = limits(&payload, order.as_deref());
                            let preferred = values.get("codex").or_else(|| {
                                order.as_ref().and_then(|keys| keys.iter().find_map(|key| values.get(key)))
                                    .or_else(|| values.values().next())
                            });
                            if let Some(bucket) = preferred {
                                result["limit_id"] = bucket["limit_id"].clone();
                                result["credits"] = bucket["credits"].clone();
                                if result["plan_type"].is_null() {
                                    result["plan_type"] = bucket["plan_type"].clone();
                                }
                            }
                            result["limits"] = Value::Object(values);
                        }
                        "account/usage/read" => result["account_usage"] = usage(&payload),
                        _ => {}
                    }
                    result["methods"][method]["health"] = json!("ok");
                }
                Err(RpcError::Remote(code)) => {
                    result["methods"][method]["health"] = json!(if code == Some(-32601) {
                        "unavailable"
                    } else {
                        "error"
                    })
                }
                Err(RpcError::Protocol) => {
                    result["methods"][method]["health"] = json!("error");
                    break;
                }
            }
        }
        let statuses: Vec<_> = METHODS
            .iter()
            .filter_map(|method| result["methods"][method]["health"].as_str())
            .collect();
        result["health"] = json!(if statuses.iter().all(|status| *status == "ok") {
            "ok"
        } else if statuses.contains(&"ok") {
            "partial"
        } else {
            "error"
        });
        result["updated_at"] =
            json!(DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339());
        Ok::<_, RpcError>(result)
    };
    let result = match operation.await {
        Ok(result) => result,
        Err(_) => empty("error"),
    };
    drop(rpc);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

pub(crate) fn executable() -> Option<PathBuf> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|entry| entry.join(name))
            .find(|path| {
                let Ok(metadata) = std::fs::metadata(path) else {
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
    }) {
        return Some(path);
    }
    #[cfg(windows)]
    {
        let path = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Programs/OpenAI/Codex/bin/codex.exe");
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub interval: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 300,
        }
    }
}

pub(crate) struct Shared {
    settings: RwLock<Settings>,
    result: RwLock<Option<Value>>,
    pub(super) completed: RwLock<Option<std::time::Instant>>,
    version: AtomicU64,
    pub(super) force_due: AtomicBool,
    pub(super) notify: Notify,
    wake: RwLock<Option<SyncSender<()>>>,
}
impl Shared {
    pub fn new() -> Self {
        Self {
            settings: RwLock::new(Settings::default()),
            result: RwLock::new(None),
            completed: RwLock::new(None),
            version: AtomicU64::new(0),
            force_due: AtomicBool::new(false),
            notify: Notify::new(),
            wake: RwLock::new(None),
        }
    }
    pub fn settings(&self) -> Settings {
        *self
            .settings
            .read()
            .unwrap_or_else(|error| error.into_inner())
    }
    pub fn result(&self) -> Option<Value> {
        self.result
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
    pub fn set_wake(&self, wake: SyncSender<()>) {
        *self.wake.write().unwrap_or_else(|error| error.into_inner()) = Some(wake);
    }
    fn signal(&self) {
        self.version.fetch_add(1, Ordering::Release);
        self.notify.notify_one();
        if let Some(wake) = self
            .wake
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            let _ = wake.try_send(());
        }
    }
    pub fn configure(&self, settings: Settings) {
        let mut current = self
            .settings
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if *current != settings {
            if current.enabled != settings.enabled {
                self.force_due.store(settings.enabled, Ordering::Release);
            }
            *current = settings;
            drop(current);
            self.signal();
        }
    }
    pub(super) fn publish(&self, result: Value) {
        *self
            .result
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(result);
        *self
            .completed
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(std::time::Instant::now());
        self.signal();
    }
}

pub(crate) struct Source {
    home: PathBuf,
    cache: Mutex<Option<(Instant, Value)>>,
    shared: Arc<Shared>,
}
impl Source {
    pub fn settings(&self) -> Settings {
        self.shared.settings()
    }
    pub fn configure(&self, settings: Settings) {
        self.shared.configure(settings);
    }
    pub fn new(home: PathBuf, shared: Arc<Shared>) -> Self {
        Self {
            home,
            cache: Mutex::new(None),
            shared,
        }
    }
    pub async fn snapshot(&self, enabled: bool) -> Value {
        if !enabled {
            return empty("disabled");
        }
        let mut cache = self.cache.lock().await;
        if let Some((at, value)) = &*cache {
            if at.elapsed() < Duration::from_secs(60) {
                self.shared.publish(value.clone());
                return value.clone();
            }
        }
        let result = match executable() {
            Some(path) => read_from(&path, &self.home, Duration::from_secs(10)).await,
            None => empty("unavailable"),
        };
        *cache = Some((Instant::now(), result.clone()));
        self.shared.publish(result.clone());
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
                    self.snapshot(true).await;
                }
                continue;
            }
            let due = last
                .map(|last| last + Duration::from_secs(settings.interval))
                .unwrap_or_else(std::time::Instant::now);
            tokio::select! {
                _=self.shared.notify.notified()=>{},
                _=tokio::time::sleep_until(Instant::from_std(due))=>{self.snapshot(true).await;}
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn enabling_after_manual_read_schedules_immediately_from_cache() {
        let shared = Arc::new(Shared::new());
        let source = Arc::new(Source::new(PathBuf::new(), Arc::clone(&shared)));
        let value = empty("ok");
        *source.cache.lock().await = Some((Instant::now(), value.clone()));
        shared.publish(value);
        let previous = shared.version();
        shared.configure(Settings {
            enabled: true,
            interval: 300,
        });
        let worker = tokio::spawn(source.run());
        tokio::time::timeout(Duration::from_secs(1), async {
            while shared.version() < previous + 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("enabling should schedule a cached read immediately");
        worker.abort();
        let _ = worker.await;
    }
    fn fake(script: &str) -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("codex");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (home, path)
    }
    #[tokio::test]
    async fn projected_protocol_uses_only_account_methods_and_hides_identity() {
        let script = r#"while IFS= read -r line; do
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"method":"notice","params":{"secret":"PRIVATE"}}' '{"id":1,"result":{"account":{"email":"private@example.test","planType":"pro"}}}' ;;
*'"id":2'*) printf '%s\n' '{"id":2,"result":{"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":0},"credits":{"hasCredits":false,"unlimited":false,"balance":"0"}}}}}' ;;
*'"id":3'*) printf '%s\n' '{"id":3,"result":{"summary":{"lifetimeTokens":0,"email":"private@example.test"},"dailyUsageBuckets":[{"startDate":"2026-10-06","tokens":0}]}}' ;;
esac
done"#;
        let (home, path) = fake(script);
        let value = read_from(&path, home.path(), Duration::from_secs(3)).await;
        assert_eq!(value["health"], "ok");
        assert_eq!(value["plan_type"], "pro");
        assert_eq!(
            value["limits"]["codex"]["primary"]["remaining_percent"],
            100
        );
        assert_eq!(
            value["limits"]["codex"]["primary"]["resets_at"],
            "1970-01-01T00:00:00+00:00"
        );
        assert_eq!(value["account_usage"]["summary"]["lifetimeTokens"], 0);
        assert!(!value.to_string().contains("private@example.test"));
        assert!(!value.to_string().contains("PRIVATE"));
    }
    #[tokio::test]
    async fn final_rpc_response_accepts_eof_without_relaxing_byte_limit() {
        let prefix = [
            b"{\"id\":0,\"result\":{}}\n".as_slice(),
            b"{\"id\":1,\"result\":{\"account\":{\"planType\":\"pro\"}}}\n".as_slice(),
            b"{\"id\":2,\"result\":{}}\n".as_slice(),
        ];
        let base = br#"{"id":3,"result":{"summary":{"lifetimeTokens":42}}}"#;
        let mut cr = base.to_vec();
        cr.push(b'\r');
        let mut exact = base.to_vec();
        exact.resize(LIMIT - prefix.iter().map(|row| row.len()).sum::<usize>(), b' ');
        let mut oversized = exact.clone();
        oversized.push(b' ');
        let cases = [
            (base.to_vec(), "ok"), (cr, "ok"),
            (base[..base.len() - 1].to_vec(), "partial"), (Vec::new(), "partial"),
            (exact, "ok"), (oversized, "partial"),
        ];
        for (index, (body, health)) in cases.into_iter().enumerate() {
            let (home, path) = fake(r#"while IFS= read -r line; do
case "$line" in
*'"id":0'*) cat "$CODEX_HOME/0" ;;
*'"id":1'*) cat "$CODEX_HOME/1" ;;
*'"id":2'*) cat "$CODEX_HOME/2" ;;
*'"id":3'*) cat "$CODEX_HOME/3"; exit 0 ;;
esac
done"#);
            for (index, row) in prefix.iter().enumerate() {
                std::fs::write(home.path().join(index.to_string()), row).unwrap();
            }
            std::fs::write(home.path().join("3"), body).unwrap();
            let value = read_from(&path, home.path(), Duration::from_secs(10)).await;
            assert_eq!(value["health"], health, "fixture {index}: {value}");
            assert_eq!(value["plan_type"], "pro");
            assert_eq!(value["methods"]["account/usage/read"]["health"],
                if health == "ok" { "ok" } else { "error" });
            if health == "ok" {
                assert_eq!(value["account_usage"]["summary"]["lifetimeTokens"], 42);
            }
        }
    }
    #[tokio::test]
    async fn raw_rpc_order_selects_primary_account_bucket() {
        let script = r#"while IFS= read -r line; do
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"id":1,"result":{"account":{}}}' ;;
*'"id":2'*) printf '%s\n' '{"id":2,"result":{"rateLimitsByLimitId":{"zulu":{"planType":"pro","credits":{"balance":"9"}},"alpha":{"planType":"free","credits":{"balance":"1"}},"zulu":{"planType":"team","credits":{"balance":"7"}}}}}' ;;
*'"id":3'*) printf '%s\n' '{"id":3,"result":{}}' ;;
esac
done"#;
        let (home, path) = fake(script);
        let result = read_from(&path, home.path(), Duration::from_secs(3)).await;
        assert_eq!(result["health"], "ok");
        assert_eq!(result["limit_id"], "zulu");
        assert_eq!(result["credits"]["balance"], "7");
        assert_eq!(result["plan_type"], "team");
    }
    #[tokio::test]
    async fn disabled_never_runs_and_timeout_cleans_child() {
        let (home, path) = fake("exec sleep 10");
        let source = Source::new(home.path().to_path_buf(), Arc::new(Shared::new()));
        assert_eq!(source.snapshot(false).await["health"], "disabled");
        let start = Instant::now();
        let value = read_from(&path, home.path(), Duration::from_millis(50)).await;
        assert_eq!(value["health"], "error");
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[tokio::test]
    async fn unsupported_method_and_timed_out_method_preserve_prior_success() {
        let partial = r#"while IFS= read -r line; do
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"id":1,"result":{"account":{"planType":"pro"}}}' ;;
*'"id":2'*) printf '%s\n' '{"id":2,"error":{"code":-32601,"message":"PRIVATE"}}' ;;
*'"id":3'*) printf '%s\n' '{"id":3,"result":{"summary":null,"dailyUsageBuckets":null}}' ;;
esac
done"#;
        let (home, path) = fake(partial);
        let result = read_from(&path, home.path(), Duration::from_secs(3)).await;
        assert_eq!(result["health"], "partial");
        assert_eq!(
            result["methods"]["account/rateLimits/read"]["health"],
            "unavailable"
        );
        assert!(!result.to_string().contains("PRIVATE"));
        let timeout = r#"while IFS= read -r line; do
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"id":1,"result":{"account":{"planType":"pro"}}}' ;;
*'"id":2'*) exec sleep 10 ;;
esac
done"#;
        let (home, path) = fake(timeout);
        let result = read_from(&path, home.path(), Duration::from_millis(500)).await;
        assert_eq!(result["health"], "partial");
        assert_eq!(
            result["methods"]["account/rateLimits/read"]["health"],
            "error"
        );
        assert_eq!(result["plan_type"], "pro");
    }

    #[tokio::test]
    async fn oversized_protocol_output_is_rejected() {
        let (home, path) = fake("head -c 1048577 /dev/zero");
        let result = read_from(&path, home.path(), Duration::from_secs(3)).await;
        assert_eq!(result["health"], "error");
    }
    #[test]
    fn projection_rejects_invalid_values_and_retains_zero() {
        assert_eq!(credits(&json!({})), Some(json!({"balance":null})));
        assert_eq!(
            window(&json!({"usedPercent":0,"resetsAt":0})).unwrap()["remaining_percent"],
            100
        );
        assert!(window(&json!({"usedPercent":true})).unwrap()["used_percent"].is_null());
        assert_eq!(
            window(&json!({"usedPercent":110})).unwrap()["remaining_percent"],
            0
        );
        assert_eq!(
            credits(&json!({"hasCredits":false,"balance":"secret"})).unwrap(),
            json!({"has_credits":false})
        );
        assert_eq!(
            usage(
                &json!({"summary":null,"dailyUsageBuckets":[{"startDate":"invalid","tokens":0},{"startDate":"2026-10-06","tokens":true}]})
            )["daily_usage_buckets"],
            json!([])
        );
    }
    #[test]
    fn daily_usage_dates_match_python_calendar_bounds() {
        for (date, accepted) in [
            ("0000-01-01", false),
            ("-0001-01-01", false),
            ("+10000-01-01", false),
            ("0001-01-01", true),
            ("9999-12-31", true),
            ("2024-02-29", true),
            ("2025-02-29", false),
            ("20260101", false),
            ("2026-1-01", false),
        ] {
            let result = usage(&json!({"dailyUsageBuckets":[{"startDate":date,"tokens":0}]}));
            let expected = if accepted { json!([{"start_date":date,"tokens":0}]) } else { json!([]) };
            assert_eq!(result["daily_usage_buckets"], expected, "{date}");
        }
    }
}

#[cfg(test)]
mod limit_order_tests {
    use super::*;
    fn project_line(raw: &str) -> (Map<String, Value>, Option<String>) {
        let response: Value = serde_json::from_str(raw).unwrap();
        let order = limit_order(raw.as_bytes()).unwrap();
        let values = limits(&response["result"], order.as_deref());
        let preferred = values.get("codex").or_else(|| {
            order.as_ref().and_then(|keys| keys.iter().find_map(|key| values.get(key)))
                .or_else(|| values.values().next())
        }).and_then(|value| value["limit_id"].as_str()).map(str::to_owned);
        (values, preferred)
    }
    #[test]
    fn source_order_selects_bucket_and_preserves_duplicate_values() {
        let (values, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":{"zulu":{"credits":{"balance":"9"}},"alpha":{},"zulu":{"credits":{"balance":"7"}}}}}"#);
        assert_eq!(preferred.as_deref(), Some("zulu"));
        assert_eq!(values["zulu"]["credits"]["balance"], "7");
        let (_, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":{"zulu":{},"codex":{},"alpha":{}}}}"#);
        assert_eq!(preferred.as_deref(), Some("codex"));
        let (_, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":{"bad!":{},"missing":null,"zulu":{},"alpha":{}}}}"#);
        assert_eq!(preferred.as_deref(), Some("zulu"));
    }
    #[test]
    fn cap_uses_first_unique_keys_before_validation() {
        let rows = (0..32).map(|n| format!("\"a{n:02}\":{{}}")).collect::<Vec<_>>().join(",");
        let (values, preferred) = project_line(&format!("{{\"result\":{{\"rateLimitsByLimitId\":{{\"zulu\":{{}},{rows}}}}}}}"));
        assert_eq!(values.len(), 32);
        assert!(values.contains_key("zulu"));
        assert!(!values.contains_key("a31"));
        assert_eq!(preferred.as_deref(), Some("zulu"));
        let (values, _) = project_line(&format!("{{\"result\":{{\"rateLimitsByLimitId\":{{\"bad!\":{{}},{rows}}}}}}}"));
        assert_eq!(values.len(), 31);
        assert!(!values.contains_key("a31"));
    }
    #[test]
    fn duplicate_containers_and_legacy_keep_last_member() {
        let (_, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":{"old":{}}},"result":{"rateLimitsByLimitId":{"wrong":{}},"rateLimitsByLimitId":{"zulu":{},"alpha":{}}}}"#);
        assert_eq!(preferred.as_deref(), Some("zulu"));
        let (_, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":null,"rateLimits":{"limitId":"legacy"}}}"#);
        assert_eq!(preferred.as_deref(), Some("legacy"));
        let (values, preferred) = project_line(r#"{"result":{"rateLimitsByLimitId":{},"rateLimits":{"limitId":"legacy"}}}"#);
        assert!(values.is_empty());
        assert_eq!(preferred, None);
    }
}
