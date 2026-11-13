//! Explicit, bounded reads of fixed public status and release sources.
use super::account::{Settings, Shared};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use regex::Regex;
use reqwest::{redirect::Policy, Client};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc, LazyLock},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:rust-v|v)?[0-9]+\.[0-9]+\.[0-9]+$").unwrap());
#[derive(Clone, Copy)]
pub enum Kind {
    Openai,
    Codex,
}
impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai" => Some(Self::Openai),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Codex => "Codex",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Codex => "codex",
        }
    }
    fn url(self) -> &'static str {
        match self {
            Self::Openai => "https://status.openai.com/api/v2/status.json",
            Self::Codex => "https://api.github.com/repos/openai/codex/releases/latest",
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Openai => 65536,
            Self::Codex => 1048576,
        }
    }
}
fn valid_time(value: &str) -> bool {
    value.len() <= 64
        && (DateTime::parse_from_rfc3339(value).is_ok()
            || NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
            || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok())
}
fn project(kind: Kind, value: &Value) -> Option<Value> {
    match kind {
        Kind::Openai => {
            let indicator = value.get("status")?.get("indicator")?.as_str()?;
            if !["none", "minor", "major", "critical"].contains(&indicator) {
                return None;
            }
            let updated = value
                .get("page")
                .and_then(|page| page.get("updated_at"))
                .unwrap_or(&Value::Null);
            if !updated.is_null() && !updated.as_str().is_some_and(valid_time) {
                return None;
            }
            Some(json!({"indicator":indicator,"updated_at":updated}))
        }
        Kind::Codex => {
            if value.get("draft")?.as_bool() != Some(false)
                || value.get("prerelease")?.as_bool() != Some(false)
            {
                return None;
            }
            let tag = value.get("tag_name")?.as_str()?;
            let date = value.get("published_at")?.as_str()?;
            if !VERSION.is_match(tag) || !valid_time(date) {
                return None;
            }
            let version = tag
                .strip_prefix("rust-v")
                .or_else(|| tag.strip_prefix('v'))
                .unwrap_or(tag);
            Some(json!({"version":version,"updated_at":date}))
        }
    }
}
async fn read(client: &Client, kind: Kind, url: &str) -> Option<Value> {
    let mut response = client
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", "LocalActivityMonitor")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > kind.limit().saturating_sub(body.len()) {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    project(kind, &serde_json::from_slice::<Value>(&body).ok()?)
}
pub struct Source {
    kind: Kind,
    client: Client,
    cache: Mutex<Option<(Instant, Value)>>,
    shared: Arc<Shared>,
}
impl Source {
    pub fn new(kind: Kind, shared: Arc<Shared>) -> Result<Self, reqwest::Error> {
        Ok(Self {
            kind,
            shared,
            cache: Mutex::new(None),
            client: Client::builder()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(4))
                .build()?,
        })
    }
    pub fn settings(&self) -> Settings {
        self.shared.settings()
    }
    pub fn configure(&self, settings: Settings) {
        self.shared.configure(settings)
    }
    pub async fn query(&self) -> Value {
        let mut cache = self.cache.lock().await;
        if let Some((time, value)) = &*cache {
            if time.elapsed() < Duration::from_secs(60) {
                self.shared.publish(value.clone());
                return value.clone();
            }
        }
        let mut value = json!({"source":self.kind.key(),"name":self.kind.name(),"url":self.kind.url(),"checked_at":DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339(),"health":"error"});
        if let Some(result) = read(&self.client, self.kind, self.kind.url()).await {
            if let (Some(target), Some(data)) = (value.as_object_mut(), result.as_object()) {
                target.extend(data.clone());
            }
            value["health"] = json!("ok");
        }
        *cache = Some((Instant::now(), value.clone()));
        self.shared.publish(value.clone());
        value
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
                    self.query().await;
                }
                continue;
            }
            let due = last
                .map(|last| last + Duration::from_secs(settings.interval))
                .unwrap_or_else(std::time::Instant::now);
            tokio::select! {_=self.shared.notify.notified()=>{},_=tokio::time::sleep_until(Instant::from_std(due))=>{self.query().await;}}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    async fn fake_response(status: &str, body: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: /other\r\nConnection: close\r\n\r\n{body}", body.len());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            let _ = stream.read(&mut request).await;
            let _ = stream.write_all(response.as_bytes()).await;
        });
        format!("http://{address}/")
    }
    #[tokio::test]
    async fn bounded_http_rejects_redirects_and_oversized_data() {
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let body = r#"{"status":{"indicator":"none"}}"#;
        let url = fake_response("200 OK", body).await;
        assert_eq!(
            read(&client, Kind::Openai, &url).await.unwrap()["indicator"],
            "none"
        );
        let url = fake_response("302 Found", body).await;
        assert!(read(&client, Kind::Openai, &url).await.is_none());
        let url = fake_response("200 OK", &"x".repeat(65537)).await;
        assert!(read(&client, Kind::Openai, &url).await.is_none());
    }
    #[tokio::test]
    async fn enabled_source_publishes_cached_result_without_network() {
        let shared = Arc::new(Shared::new());
        let source = Arc::new(Source::new(Kind::Openai, Arc::clone(&shared)).unwrap());
        *source.cache.lock().await =
            Some((Instant::now(), json!({"health":"ok","indicator":"none"})));
        shared.configure(Settings {
            enabled: true,
            interval: 60,
        });
        let worker = tokio::spawn(source.run());
        tokio::time::timeout(Duration::from_secs(1), async {
            while shared.result().is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(shared.result().unwrap()["indicator"], "none");
        worker.abort();
        let _ = worker.await;
    }
    #[test]
    fn validates_status_and_release_projections() {
        assert_eq!(project(Kind::Openai,&json!({"status":{"indicator":"minor"},"page":{"updated_at":"2026-01-01T00:00:00Z"}})).unwrap()["indicator"],"minor");
        assert!(project(Kind::Openai, &json!({"status":{"indicator":"unknown"}})).is_none());
        assert_eq!(project(Kind::Codex,&json!({"draft":false,"prerelease":false,"tag_name":"rust-v1.2.3","published_at":"2026-01-01T00:00:00Z","secret":"private"})).unwrap(),json!({"version":"1.2.3","updated_at":"2026-01-01T00:00:00Z"}));
        assert!(project(Kind::Codex,&json!({"draft":false,"prerelease":true,"tag_name":"v1.2.3","published_at":"2026-01-01"})).is_none());
    }
}
