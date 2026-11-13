use bytes::Bytes;
use flate2::{write::GzEncoder, Compression};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::future::Future;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_io_timeout::TimeoutStream;

use crate::{
    assets::Assets,
    events,
    history::{self, HistoryError},
    sessions::Snapshots,
};

const CONNECTIONS: usize = 64;
const PLAIN: &str = "text/plain; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";
const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'";

pub type Body = UnsyncBoxBody<Bytes, Infallible>;

#[derive(Clone, Copy, Default)]
pub struct Traffic {
    pub revision: u64,
    pub requests: u64,
    pub http_errors: u64,
    pub snapshot_bytes: usize,
    pub transfer_bytes: usize,
}

#[derive(Clone, Copy)]
struct ResponseSizes {
    plain: usize,
    transferred: usize,
}

pub struct State {
    pub home_id: String,
    pub monitoring: PathBuf,
    pub codex: Arc<std::sync::atomic::AtomicBool>,
    pub traffic: Arc<std::sync::Mutex<Traffic>>,
    pub account: Arc<crate::sessions::account::Source>,
    pub cloud: Arc<crate::sessions::cloud::Source>,
    pub public_openai: Arc<crate::sessions::public_api::Source>,
    pub public_codex: Arc<crate::sessions::public_api::Source>,
    pub platform_sources: Vec<Arc<crate::sessions::platform::Source>>,
    pub metadata_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub tool_events_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub files_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub errors_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub sqlite_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub model_api_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub command_switches: Arc<std::sync::RwLock<crate::sessions::CommandSwitches>>,
    pub mcp_sources: Arc<std::sync::RwLock<std::collections::BTreeMap<String, bool>>>,
    pub mcp_switches: Arc<std::sync::RwLock<crate::sessions::McpSwitches>>,
    pub customizations: Arc<std::sync::RwLock<crate::sessions::McpCustomizations>>,
    pub tracking: Arc<std::sync::RwLock<crate::sessions::TrackingSettings>>,
    pub cleanup_wake: std::sync::mpsc::SyncSender<()>,
    pub metadata_lock: tokio::sync::Mutex<()>,
    pub journal: Arc<std::sync::Mutex<crate::journal::Journal>>,
    pub debug: Arc<std::sync::Mutex<crate::performance_debug::PerformanceDebug>>,
    pub details: Arc<std::sync::RwLock<crate::sessions::details::Index>>,
    pub core_source: Arc<std::sync::RwLock<Option<crate::sessions::core_logs::Source>>>,
    pub desktop_sources: Arc<std::sync::RwLock<Vec<crate::sessions::desktop::ObservedSource>>>,
    pub assets: Option<Assets>,
    pub snapshots: tokio::sync::watch::Receiver<Snapshots>,
    pub refresh: Option<(
        std::sync::mpsc::SyncSender<()>,
        Arc<std::sync::atomic::AtomicBool>,
    )>,
    pub event_clients: Arc<Semaphore>,
}

pub async fn serve<T>(
    listener: TcpListener,
    state: Arc<State>,
    shutdown: impl Future<Output = T>,
) -> io::Result<T> {
    let port = listener.local_addr()?.port();
    let hosts = Arc::new([format!("127.0.0.1:{port}"), format!("localhost:{port}")]);
    let mut tasks = JoinSet::new();
    let (closing, _) = tokio::sync::watch::channel(false);
    tokio::pin!(shutdown);
    let outcome = loop {
        tokio::select! {
            biased;
            value = &mut shutdown => break value,
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept(), if tasks.len() < CONNECTIONS => {
                let (stream, peer) = accepted?;
                if !peer.ip().is_loopback() {
                    continue;
                }
                let state = Arc::clone(&state);
                let hosts = Arc::clone(&hosts);
                let shutdown = closing.subscribe();
                tasks.spawn(async move {
                    let tracker = Arc::new(std::sync::Mutex::new(None));
                    let socket_tracker = Arc::clone(&tracker);
                    let streaming = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let stream_state = Arc::clone(&streaming);
                    let debug = Arc::clone(&state.debug);
                    let service = service_fn(move |request| {
                        let state = Arc::clone(&state);
                        let hosts = Arc::clone(&hosts);
                        let shutdown = shutdown.clone();
                        let streaming = Arc::clone(&streaming);
                        let tracker = Arc::clone(&tracker);
                        async move {
                            let snapshot_request = request.method() == Method::GET && request.uri().path() == "/api/snapshot";
                            let response = dispatch(request, Arc::clone(&state), &hosts, shutdown, tracker).await;
                            let is_stream = response.headers().get(header::CONTENT_TYPE).is_some_and(|value| value.as_bytes().starts_with(b"text/event-stream"));
                            if is_stream {
                                streaming.store(true, std::sync::atomic::Ordering::Release);
                            } else {
                                let mut traffic = state.traffic.lock().unwrap_or_else(|error| error.into_inner());
                                traffic.revision = traffic.revision.wrapping_add(1);
                                traffic.requests = traffic.requests.saturating_add(1);
                                if response.status().as_u16() >= 400 { traffic.http_errors = traffic.http_errors.saturating_add(1); }
                                if snapshot_request && response.status() == StatusCode::OK {
                                    if let Some(sizes) = response.extensions().get::<ResponseSizes>() { traffic.snapshot_bytes = sizes.plain; traffic.transfer_bytes = sizes.transferred; }
                                }
                            }
                            if response.status().is_client_error() || response.status().is_server_error() {
                                let journal = Arc::clone(&state.journal);
                                let status = response.status().as_u16();
                                let recorded = tokio::task::spawn_blocking(move || journal.lock().unwrap_or_else(|error| error.into_inner()).http_error(status)).await.unwrap_or(false);
                                if recorded {
                                    if let Some((wake, manual)) = &state.refresh {
                                        manual.store(true, std::sync::atomic::Ordering::Release);
                                        let _ = wake.try_send(());
                                    }
                                }
                            }
                            Ok::<_, Infallible>(response)
                        }
                    });
                    let mut stream = TimeoutStream::new(stream);
                    stream.set_write_timeout(Some(Duration::from_secs(15)));
                    let mut builder = http1::Builder::new();
                    let connection = builder
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(15))
                        .max_buf_size(16 * 1024)
                        .serve_connection(TokioIo::new(crate::sse_write::TrackedIo::new(stream, socket_tracker, Arc::clone(&stream_state))), service);
                    if connection.await.is_err() && stream_state.load(std::sync::atomic::Ordering::Acquire) {
                        let _ = tokio::task::spawn_blocking(move || debug.lock().unwrap_or_else(|error| error.into_inner()).record("stream_error", json!({}))).await;
                    }
                });
            }
        }
    };
    let _ = closing.send(true);
    if tokio::time::timeout(Duration::from_secs(2), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.shutdown().await;
    }
    Ok(outcome)
}

async fn dispatch(
    request: Request<Incoming>,
    state: Arc<State>,
    hosts: &[String; 2],
    shutdown: tokio::sync::watch::Receiver<bool>,
    tracker: Arc<std::sync::Mutex<Option<crate::sse_write::Tracker>>>,
) -> Response<Body> {
    if !local(&request, hosts) {
        return reply(
            StatusCode::FORBIDDEN,
            "Local access only".into(),
            PLAIN,
            POLICY,
            false,
            true,
        );
    }
    if request.method() == Method::POST && request.uri() == "/api/refresh" {
        return refresh(request, &state).await;
    }
    if request.method() == Method::POST && request.uri() == "/api/settings" {
        return set_settings(request, state).await;
    }
    if request.method() == Method::POST && request.uri() == "/api/jev/recording" {
        return set_jev_recording(request, state).await;
    }
    if request.method() == Method::POST && request.uri() == "/api/mcp/file" {
        return set_mcp_file(request, state).await;
    }
    if request.method() == Method::POST && request.uri() == "/api/diagnostics" {
        return frontend_diagnostic(request, state).await;
    }
    if request.method() == Method::GET
        && (request.headers().contains_key(header::TRANSFER_ENCODING)
            || request
                .headers()
                .get(header::CONTENT_LENGTH)
                .is_some_and(|value| value != "0"))
    {
        return reply(
            StatusCode::BAD_REQUEST,
            "Invalid request body".into(),
            PLAIN,
            POLICY,
            false,
            true,
        );
    }
    if request.method() == Method::GET && request.uri() == "/api/codex/account" {
        let value = state.account.snapshot(true).await;
        return reply(
            StatusCode::OK,
            value.to_string().into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    if request.method() == Method::GET && request.uri().path() == "/api/codex/cloud" {
        let query = request.uri().query().unwrap_or_default();
        let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
        if query.len() > 4096
            || params.len() > 1
            || params
                .iter()
                .any(|(key, value)| key != "cursor" || !crate::sessions::cloud::valid_cursor(value))
        {
            return reply(
                StatusCode::BAD_REQUEST,
                "Invalid cloud query".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        let value = state
            .cloud
            .query(params.first().map(|(_, value)| value.as_ref()))
            .await;
        return reply(
            StatusCode::OK,
            value.to_string().into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    if request.method() == Method::GET && request.uri().path() == "/api/public-query" {
        let query = request.uri().query().unwrap_or_default();
        let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
        if query.len() > 128 || params.len() != 1 || params[0].0 != "source" {
            return reply(
                StatusCode::BAD_REQUEST,
                "Invalid public status query".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        let source = match crate::sessions::public_api::Kind::parse(&params[0].1) {
            Some(crate::sessions::public_api::Kind::Openai) => &state.public_openai,
            Some(crate::sessions::public_api::Kind::Codex) => &state.public_codex,
            None => {
                return reply(
                    StatusCode::BAD_REQUEST,
                    "Invalid public status query".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                )
            }
        };
        return reply(
            StatusCode::OK,
            source.query().await.to_string().into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    if request.method() == Method::GET && request.uri().path() == "/api/openai-platform" {
        let query = request.uri().query().unwrap_or_default();
        let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
        if query.len() > 128 || params.len() != 1 || params[0].0 != "source" {
            return reply(
                StatusCode::BAD_REQUEST,
                "Invalid Platform query".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        let index = match crate::sessions::platform::Kind::parse(&params[0].1) {
            Some(crate::sessions::platform::Kind::Models) => 0,
            Some(crate::sessions::platform::Kind::Usage) => 1,
            Some(crate::sessions::platform::Kind::Decisions) => 2,
            Some(crate::sessions::platform::Kind::Costs) => 3,
            None => {
                return reply(
                    StatusCode::BAD_REQUEST,
                    "Invalid Platform query".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                )
            }
        };
        return reply(
            StatusCode::OK,
            state.platform_sources[index]
                .query()
                .await
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    if request.method() != Method::GET {
        return reply(
            StatusCode::METHOD_NOT_ALLOWED,
            "Unsupported operation".into(),
            PLAIN,
            POLICY,
            false,
            true,
        );
    }
    let headers = request.headers();
    if request.uri().path() == "/api/events" {
        return match Snapshots::window(request.uri().query().unwrap_or_default()) {
            None => unavailable_events(StatusCode::BAD_REQUEST),
            Some(window) => match Arc::clone(&state.event_clients).try_acquire_owned() {
                Ok(permit) => events::response(
                    state.snapshots.clone(),
                    window,
                    permit,
                    shutdown,
                    Arc::clone(&state.debug),
                    Arc::clone(&state.traffic),
                    tracker,
                ),
                Err(_) => unavailable_events(StatusCode::SERVICE_UNAVAILABLE),
            },
        };
    }
    let gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|item| item.trim() == "gzip"));
    let uri = request.into_parts().0.uri;
    tokio::task::spawn_blocking(move || {
        route(&state, uri.path(), uri.query().unwrap_or_default(), gzip)
    })
    .await
    .unwrap_or_else(|_| {
        reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal error".into(),
            PLAIN,
            POLICY,
            false,
            true,
        )
    })
}

async fn frontend_diagnostic(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let value = match json_body(request, 2048).await {
        Ok(value) => value,
        Err(error) => return bad_body(error),
    };
    let journal = Arc::clone(&state.journal);
    let result = tokio::task::spawn_blocking(move || {
        journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .frontend_failed(value)
    })
    .await;
    match result {
        Ok(Ok(recorded)) => {
            if recorded {
                if let Some((wake, manual)) = &state.refresh {
                    manual.store(true, std::sync::atomic::Ordering::Release);
                    let _ = wake.try_send(());
                }
            }
            reply(
                StatusCode::ACCEPTED,
                json!({"recorded":recorded}).to_string().into(),
                JSON,
                POLICY,
                false,
                false,
            )
        }
        Ok(Err(())) => reply(
            StatusCode::BAD_REQUEST,
            "Invalid diagnostic metadata".into(),
            PLAIN,
            POLICY,
            false,
            false,
        ),
        Err(_) => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal error".into(),
            PLAIN,
            POLICY,
            false,
            false,
        ),
    }
}

async fn refresh(request: Request<Incoming>, state: &State) -> Response<Body> {
    match json_body(request, 512).await {
        Ok(value) if value.as_object().is_some_and(|object| object.is_empty()) => {
            if let Some((refresh, manual)) = &state.refresh {
                manual.store(true, std::sync::atomic::Ordering::Release);
                let _ = refresh.try_send(());
            }
            reply(
                StatusCode::OK,
                state.snapshots.borrow().activity.clone(),
                JSON,
                POLICY,
                false,
                false,
            )
        }
        Ok(_) => bad_body("Invalid recording setting"),
        Err(error) => bad_body(error),
    }
}

async fn set_jev_recording(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let value = match json_body(request, 512).await {
        Ok(value) => value,
        Err(error) => return bad_body(error),
    };
    let enabled = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool);
    let Some(enabled) = enabled else {
        return bad_body("Invalid recording setting");
    };
    let _guard = state.metadata_lock.lock().await;
    let directory = state.monitoring.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::sessions::jev::configure(&directory, enabled, None)
    })
    .await;
    let since = match result {
        Ok(Ok((_, since))) => since,
        Ok(Err(crate::sessions::jev::ConfigureError::Invalid)) => {
            return reply(
                StatusCode::CONFLICT,
                json!({"error":"既有 Jev 設定無效, 請先檢查設定檔"})
                    .to_string()
                    .into(),
                JSON,
                POLICY,
                false,
                false,
            )
        }
        _ => {
            return reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"無法寫入本機 Jev 設定, 請檢查存取權限"})
                    .to_string()
                    .into(),
                JSON,
                POLICY,
                false,
                false,
            )
        }
    };
    state
        .journal
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .event(if enabled {
            "recording_enabled"
        } else {
            "recording_disabled"
        });
    if let Some((wake, manual)) = &state.refresh {
        manual.store(true, std::sync::atomic::Ordering::Release);
        let _ = wake.try_send(());
    }
    reply(
        StatusCode::OK,
        json!({"enabled":enabled,"enabled_at":since})
            .to_string()
            .into(),
        JSON,
        POLICY,
        false,
        false,
    )
}

async fn set_mcp_file(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let value = match json_body(request, 262144).await {
        Ok(value) => value,
        Err(error) => return bad_body(error),
    };
    let Some(object) = value.as_object().filter(|value| value.len() == 4) else {
        return bad_body("Invalid MCP document");
    };
    let (Some(server), Some(document), Some(sha), Some(text)) = (
        object.get("server").and_then(Value::as_str),
        object.get("document").and_then(Value::as_str),
        object.get("sha256").and_then(Value::as_str),
        object.get("text").and_then(Value::as_str),
    ) else {
        return bad_body("Invalid MCP document");
    };
    let valid_server = (1..=80).contains(&server.len())
        && server
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte));
    let hex = |value: &str, len: usize| {
        value.len() == len
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if !valid_server || !hex(document, 32) || !hex(sha, 64) {
        return bad_body("Invalid MCP document");
    }
    let _guard = state.metadata_lock.lock().await;
    let home = state
        .monitoring
        .parent()
        .unwrap_or(&state.monitoring)
        .to_path_buf();
    let (server, document, sha, text) = (
        server.to_owned(),
        document.to_owned(),
        sha.to_owned(),
        text.to_owned(),
    );
    let result = tokio::task::spawn_blocking(move || {
        crate::sessions::mcp_documents::write_document(&home, &server, &document, &text, &sha)
    })
    .await;
    match result {
        Ok(Ok(value)) => {
            if let Some((wake, manual)) = &state.refresh {
                manual.store(true, std::sync::atomic::Ordering::Release);
                let _ = wake.try_send(());
            }
            reply(
                StatusCode::OK,
                value.to_string().into(),
                JSON,
                POLICY,
                false,
                false,
            )
        }
        Ok(Err(crate::sessions::mcp_documents::WriteError::Conflict)) => reply(
            StatusCode::CONFLICT,
            json!({"error":"檔案已變更, 請重新讀取後再儲存"})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        ),
        Ok(Err(crate::sessions::mcp_documents::WriteError::Invalid)) => reply(
            StatusCode::CONFLICT,
            json!({"error":"檔案無法編輯或內容格式無效"})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        ),
        _ => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error":"無法儲存檔案, 請檢查存取權限"})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        ),
    }
}

fn bad_body(error: &'static str) -> Response<Body> {
    reply(
        StatusCode::BAD_REQUEST,
        error.into(),
        PLAIN,
        POLICY,
        false,
        true,
    )
}

async fn json_body(
    request: Request<Incoming>,
    limit: usize,
) -> Result<serde_json::Value, &'static str> {
    let headers = request.headers();
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if content_type != Some("application/json") || headers.contains_key(header::TRANSFER_ENCODING) {
        return Err("Invalid content type");
    }
    if !length.is_some_and(|length| (1..=limit).contains(&length)) {
        return Err("Invalid recording setting");
    }
    match tokio::time::timeout(
        Duration::from_secs(5),
        Limited::new(request.into_body(), limit).collect(),
    )
    .await
    {
        Ok(Ok(body)) => {
            serde_json::from_slice(&body.to_bytes()).map_err(|_| "Invalid recording setting")
        }
        _ => Err("Invalid recording setting"),
    }
}

fn merge_customizations(
    value: &serde_json::Map<String, serde_json::Value>,
    current: &crate::sessions::McpCustomizations,
) -> Option<crate::sessions::McpCustomizations> {
    let mut next = current.clone();
    let source = |name: &str| {
        (1..=80).contains(&name.len())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    };
    let tool = |name: &str| {
        (1..=160).contains(&name.len())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    };
    for (kind, entries) in value {
        let entries = entries.as_object().filter(|entries| entries.len() <= 64)?;
        for (name, item) in entries {
            if !(if kind == "tool_descriptions" {
                tool(name)
            } else {
                source(name)
            }) {
                return None;
            }
            match kind.as_str() {
                "mcp_categories" => {
                    let category = item.as_str()?;
                    if ![
                        "documents",
                        "workspace",
                        "runtime",
                        "workflow",
                        "review",
                        "deployment",
                        "apps",
                        "jev",
                        "web",
                        "other",
                    ]
                    .contains(&category)
                    {
                        return None;
                    }
                    next.mcp_categories.insert(name.clone(), category.into());
                }
                "mcp_descriptions" | "tool_descriptions" => {
                    let text = item.as_str().filter(|text| text.chars().count() <= 400)?;
                    let target = if kind == "mcp_descriptions" {
                        &mut next.mcp_descriptions
                    } else {
                        &mut next.tool_descriptions
                    };
                    if text.trim().is_empty() {
                        target.remove(name);
                    } else {
                        target.insert(name.clone(), text.into());
                    }
                }
                "mcp_tags" => {
                    let tags = item.as_array().filter(|items| items.len() <= 4)?;
                    let mut cleaned = Vec::new();
                    for tag in tags {
                        let tag = tag
                            .as_str()
                            .filter(|text| !text.chars().any(|ch| ch < ' '))?
                            .trim();
                        if tag.is_empty() || tag.chars().count() > 40 {
                            return None;
                        }
                        if !cleaned.iter().any(|existing| existing == tag) {
                            cleaned.push(tag.to_owned());
                        }
                    }
                    if cleaned.is_empty() {
                        next.mcp_tags.remove(name);
                    } else {
                        next.mcp_tags.insert(name.clone(), cleaned);
                    }
                }
                _ => return None,
            }
        }
    }
    (next.mcp_categories.len() <= 64
        && next.mcp_descriptions.len() <= 64
        && next.mcp_tags.len() <= 64
        && next.tool_descriptions.len() <= 64)
        .then_some(next)
}

fn batch_settings_response(
    state: &State,
    applied: bool,
    retention_days: Option<i64>,
) -> Response<Body> {
    let mut settings = if applied {
        serde_json::from_slice::<serde_json::Value>(&state.snapshots.borrow().windows[3])
            .ok()
            .and_then(|snapshot| snapshot.get("settings").cloned())
            .unwrap_or_else(|| json!({}))
    } else {
        let customizations = state
            .customizations
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let sources = state
            .mcp_sources
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let switches = *state
            .mcp_switches
            .read()
            .unwrap_or_else(|error| error.into_inner());
        json!({"mcp_categories":customizations.mcp_categories,"mcp_descriptions":customizations.mcp_descriptions,
            "mcp_tags":customizations.mcp_tags,"tool_descriptions":customizations.tool_descriptions,"mcp_sources":sources,
            "debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),
            "api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),
                "tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),
                "files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":switches.mcp,"web":switches.web,
                "logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()},"refresh_pending":true})
    };
    let tracking = *state
        .tracking
        .read()
        .unwrap_or_else(|error| error.into_inner());
    settings["max_files"] = json!(tracking.max_files);
    settings["track_all"] = json!(tracking.track_all);
    if let Some(days) = retention_days {
        settings["activity_retention_days"] = json!(days);
    }
    reply(
        if applied {
            StatusCode::OK
        } else {
            StatusCode::ACCEPTED
        },
        settings.to_string().into(),
        JSON,
        POLICY,
        false,
        false,
    )
}

async fn set_settings(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let value = match json_body(request, 262144).await {
        Ok(value) => value,
        Err(error) => return bad_body(error),
    };
    if let Some(batch) = value.as_object().filter(|object| {
        !object.is_empty()
            && object.keys().all(|key| {
                [
                    "mcp_sources",
                    "mcp_categories",
                    "mcp_descriptions",
                    "mcp_tags",
                    "tool_descriptions",
                    "replace_customizations",
                    "observations",
                    "debug_mode",
                    "interval",
                    "idle_minutes",
                    "activity_retention_days",
                    "max_files",
                    "track_all",
                    "recording",
                    "api_intervals",
                ]
                .contains(&key.as_str())
            })
            && (object.contains_key("replace_customizations")
                || object.contains_key("max_files")
                || object.contains_key("track_all")
                || object.contains_key("recording")
                || object.contains_key("api_intervals")
                || object.contains_key("interval")
                || object.contains_key("idle_minutes")
                || object.len() > 1
                || object
                    .get("observations")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|items| {
                        items.len() > 1
                            || items.contains_key("git")
                            || items.contains_key("checks")
                            || items.contains_key("model_api")
                            || items.contains_key("jev_calls")
                            || items.contains_key("skills")
                            || items.contains_key("jev")
                            || items.contains_key("worktrees")
                            || items.contains_key("usage")
                            || items.contains_key("codex")
                            || items.contains_key("codex_account")
                            || items.contains_key("codex_cloud")
                            || items.contains_key("public_openai")
                            || items.contains_key("public_codex")
                            || [
                                "platform_models",
                                "platform_usage",
                                "platform_decisions",
                                "platform_costs",
                            ]
                            .iter()
                            .any(|key| items.contains_key(*key))
                    }))
    }) {
        for (key, minimum, maximum) in [("interval", 1, 3600), ("idle_minutes", 0, 1440)] {
            if batch.get(key).is_some_and(|value| {
                value
                    .as_u64()
                    .is_none_or(|number| !(minimum..=maximum).contains(&number))
            }) {
                return reply(
                    StatusCode::CONFLICT,
                    "Invalid settings".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                );
            }
        }
        let max_files = match batch.get("max_files") {
            None => None,
            Some(value) => match value.as_u64().filter(|value| (1..=5000).contains(value)) {
                Some(value) => Some(value as u32),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let track_all = match batch.get("track_all") {
            None => None,
            Some(value) => match value.as_bool() {
                Some(value) => Some(value),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let recording = match batch.get("recording") {
            None => None,
            Some(value) => match value.as_bool() {
                Some(enabled) => Some(enabled),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let retention_days = match batch.get("activity_retention_days") {
            None => None,
            Some(value) => match value.as_i64().filter(|days| (0..=3650).contains(days)) {
                Some(days) => Some(days),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let debug_enabled = match batch.get("debug_mode") {
            None => None,
            Some(value) => match value.as_bool() {
                Some(value) => Some(value),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let observations = match batch.get("observations") {
            None => None,
            Some(value) => match value.as_object().filter(|items| {
                items.iter().all(|(name, enabled)| {
                    [
                        "metadata",
                        "logs",
                        "tool_events",
                        "files",
                        "mcp",
                        "web",
                        "errors",
                        "sqlite",
                        "model_api",
                        "git",
                        "checks",
                        "jev_calls",
                        "skills",
                        "jev",
                        "worktrees",
                        "usage",
                        "codex",
                        "codex_account",
                        "codex_cloud",
                        "public_openai",
                        "public_codex",
                        "platform_models",
                        "platform_usage",
                        "platform_decisions",
                        "platform_costs",
                    ]
                    .contains(&name.as_str())
                        && enabled.is_boolean()
                })
            }) {
                Some(items) => Some(items),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let observation = |key: &str| {
            observations
                .and_then(|values| values.get(key))
                .and_then(serde_json::Value::as_bool)
        };
        let codex_enabled = observation("codex");
        let metadata_enabled = observation("metadata");
        let logs_enabled = observation("logs");
        let tools_enabled = observation("tool_events");
        let files_enabled = observation("files");
        let errors_enabled = observation("errors");
        let sqlite_enabled = observation("sqlite");
        let model_api_enabled = observation("model_api");
        let git_enabled = observation("git");
        let checks_enabled = observation("checks");
        let jev_calls_enabled = observation("jev_calls");
        let skills_enabled = observation("skills");
        let jev_enabled = observation("jev");
        let worktrees_enabled = observation("worktrees");
        let usage_enabled = observation("usage");
        let account_enabled = observation("codex_account");
        let cloud_enabled = observation("codex_cloud");
        let public_enabled = [observation("public_openai"), observation("public_codex")];
        let platform_enabled = [
            "platform_models",
            "platform_usage",
            "platform_decisions",
            "platform_costs",
        ]
        .map(observation);
        let account_interval = match batch.get("api_intervals") {
            None => None,
            Some(value) => match value.as_object().filter(|values| {
                values.iter().all(|(key, value)| {
                    [
                        "codex_account",
                        "codex_cloud",
                        "public_openai",
                        "public_codex",
                        "platform_models",
                        "platform_usage",
                        "platform_decisions",
                        "platform_costs",
                    ]
                    .contains(&key.as_str())
                        && value
                            .as_u64()
                            .is_some_and(|seconds| (60..=86400).contains(&seconds))
                })
            }) {
                Some(values) => values.get("codex_account").and_then(Value::as_u64),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let cloud_interval = batch
            .get("api_intervals")
            .and_then(Value::as_object)
            .and_then(|values| values.get("codex_cloud"))
            .and_then(Value::as_u64);
        let public_intervals = ["public_openai", "public_codex"].map(|key| {
            batch
                .get("api_intervals")
                .and_then(Value::as_object)
                .and_then(|values| values.get(key))
                .and_then(Value::as_u64)
        });
        let platform_intervals = [
            "platform_models",
            "platform_usage",
            "platform_decisions",
            "platform_costs",
        ]
        .map(|key| {
            batch
                .get("api_intervals")
                .and_then(Value::as_object)
                .and_then(|values| values.get(key))
                .and_then(Value::as_u64)
        });
        let mcp_enabled = observation("mcp");
        let web_enabled = observation("web");
        let replace = match batch.get("replace_customizations") {
            None => false,
            Some(value) => match value.as_bool() {
                Some(value) => value,
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let source_changes = match batch.get("mcp_sources") {
            None => None,
            Some(value) => match value.as_object().filter(|entries| {
                entries.len() <= 64
                    && entries.iter().all(|(name, enabled)| {
                        (1..=80).contains(&name.len())
                            && name
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                            && enabled.is_boolean()
                    })
            }) {
                Some(value) => Some(value),
                None => {
                    return reply(
                        StatusCode::CONFLICT,
                        "Invalid settings".into(),
                        PLAIN,
                        POLICY,
                        false,
                        false,
                    )
                }
            },
        };
        let customization_changes: serde_json::Map<String, serde_json::Value> = batch
            .iter()
            .filter(|(key, _)| {
                [
                    "mcp_categories",
                    "mcp_descriptions",
                    "mcp_tags",
                    "tool_descriptions",
                ]
                .contains(&key.as_str())
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let (updated_sources, updated_customizations) = {
            let sources = state
                .mcp_sources
                .read()
                .unwrap_or_else(|error| error.into_inner());
            let customizations = state
                .customizations
                .read()
                .unwrap_or_else(|error| error.into_inner());
            let mut next_sources = if replace {
                std::collections::BTreeMap::new()
            } else {
                sources.clone()
            };
            if let Some(changes) = source_changes {
                for (name, enabled) in changes {
                    next_sources
                        .insert(name.clone(), enabled.as_bool().expect("validated boolean"));
                }
            }
            let base = if replace {
                crate::sessions::McpCustomizations::default()
            } else {
                customizations.clone()
            };
            let Some(next_customizations) = merge_customizations(&customization_changes, &base)
                .filter(|_| next_sources.len() <= 64)
            else {
                return reply(
                    StatusCode::CONFLICT,
                    "Invalid settings".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                );
            };
            (next_sources, next_customizations)
        };
        let updated_tracking = {
            let mut tracking = *state
                .tracking
                .read()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(value) = max_files {
                tracking.max_files = value;
            }
            if let Some(value) = track_all {
                if codex_enabled
                    .unwrap_or_else(|| state.codex.load(std::sync::atomic::Ordering::Acquire))
                {
                    tracking.track_all = value;
                }
            }
            tracking
        };
        let mut recording_applied = None;
        if let Some(enabled) = recording {
            let home = state
                .monitoring
                .parent()
                .unwrap_or(&state.monitoring)
                .to_path_buf();
            let current = crate::sessions::jev::snapshot(
                &home,
                "all",
                std::time::SystemTime::now().into(),
                false,
            );
            let configured = current["reader"]["locations"]
                .as_array()
                .is_some_and(|locations| !locations.is_empty());
            if enabled || configured {
                let directory = state.monitoring.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::sessions::jev::configure(&directory, enabled, None)
                })
                .await
                {
                    Ok(Ok((changed, _))) => {
                        recording_applied = Some(enabled);
                        if changed {
                            state
                                .journal
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .event(if enabled {
                                    "recording_enabled"
                                } else {
                                    "recording_disabled"
                                });
                        }
                    }
                    Ok(Err(crate::sessions::jev::ConfigureError::Invalid)) => {
                        return reply(
                            StatusCode::CONFLICT,
                            "Invalid settings".into(),
                            PLAIN,
                            POLICY,
                            false,
                            false,
                        )
                    }
                    _ => {
                        return reply(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            json!({"error":"無法寫入本機 Jev 設定, 請檢查存取權限"})
                                .to_string()
                                .into(),
                            JSON,
                            POLICY,
                            false,
                            false,
                        )
                    }
                }
            }
        }
        if let Some(days) = retention_days {
            let directory = state.monitoring.clone();
            let saved = tokio::task::spawn_blocking(move || {
                use lam_history_store::{Access, Store};
                let mut store =
                    Store::open(&directory, Access::ReadWrite)?.expect("writer opens storage");
                store.set_activity_retention(days, std::time::SystemTime::now().into())
            })
            .await;
            if !matches!(saved, Ok(Ok(_))) {
                return reply(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "History temporarily unavailable".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                );
            }
        }
        {
            let mut sources = state
                .mcp_sources
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let mut customizations = state
                .customizations
                .write()
                .unwrap_or_else(|error| error.into_inner());
            *sources = updated_sources.clone();
            *customizations = updated_customizations.clone();
        }
        *state
            .tracking
            .write()
            .unwrap_or_else(|error| error.into_inner()) = updated_tracking;
        if let Some(enabled) = metadata_enabled {
            state
                .metadata_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        if let Some(enabled) = tools_enabled {
            state
                .tool_events_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        if let Some(enabled) = files_enabled {
            state
                .files_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        if let Some(enabled) = errors_enabled {
            state
                .errors_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        if let Some(enabled) = sqlite_enabled {
            state
                .sqlite_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        if let Some(enabled) = model_api_enabled {
            state
                .model_api_enabled
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        {
            let mut commands = state
                .command_switches
                .write()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(enabled) = git_enabled {
                commands.git = enabled;
            }
            if let Some(enabled) = checks_enabled {
                commands.checks = enabled;
            }
            if let Some(enabled) = jev_calls_enabled {
                commands.jev_calls = enabled;
            }
            if let Some(enabled) = skills_enabled {
                commands.skills = enabled;
            }
            if let Some(enabled) = jev_enabled {
                commands.jev = enabled;
            }
            if let Some(enabled) = worktrees_enabled {
                commands.worktrees = enabled;
            }
            if let Some(enabled) = usage_enabled {
                commands.usage = enabled;
            }
        }
        {
            let mut switches = state
                .mcp_switches
                .write()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(enabled) = mcp_enabled {
                switches.mcp = enabled;
            }
            if let Some(enabled) = web_enabled {
                switches.web = enabled;
            }
        }
        if let Some(enabled) = codex_enabled {
            state
                .codex
                .store(enabled, std::sync::atomic::Ordering::Release);
        }
        let mut account_settings = state.account.settings();
        if let Some(enabled) = account_enabled {
            account_settings.enabled = enabled;
        }
        if let Some(interval) = account_interval {
            account_settings.interval = interval;
        }
        state.account.configure(account_settings);
        let mut cloud_settings = state.cloud.settings();
        if let Some(enabled) = cloud_enabled {
            cloud_settings.enabled = enabled;
        }
        if let Some(interval) = cloud_interval {
            cloud_settings.interval = interval;
        }
        state.cloud.configure(cloud_settings);
        for ((source, enabled), interval) in state
            .platform_sources
            .iter()
            .zip(platform_enabled)
            .zip(platform_intervals)
        {
            let mut setting = source.settings();
            if let Some(enabled) = enabled {
                setting.enabled = enabled;
            }
            if let Some(interval) = interval {
                setting.interval = interval;
            }
            source.configure(setting);
        }
        for ((source, enabled), interval) in [&state.public_openai, &state.public_codex]
            .into_iter()
            .zip(public_enabled)
            .zip(public_intervals)
        {
            let mut setting = source.settings();
            if let Some(enabled) = enabled {
                setting.enabled = enabled;
            }
            if let Some(interval) = interval {
                setting.interval = interval;
            }
            source.configure(setting);
        }
        let journal = Arc::clone(&state.journal);
        let debug = Arc::clone(&state.debug);
        let _ = tokio::task::spawn_blocking(move || {
            if let Some(enabled) = debug_enabled {
                debug
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .set_enabled(enabled);
            }
            let mut journal = journal.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(enabled) = logs_enabled {
                journal.set_enabled(enabled);
            }
            journal.event("settings_applied");
        })
        .await;
        let previous_version = snapshots.borrow_and_update().version();
        if retention_days.is_some() {
            let _ = state.cleanup_wake.try_send(());
        }
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let matches = {
                    let snapshot = snapshots.borrow_and_update();
                    snapshot.mcp_sources == updated_sources
                        && snapshot.customizations == updated_customizations
                        && snapshot.tracking == updated_tracking
                        && retention_days.is_none_or(|days| {
                            snapshot.retention_days == Some(days)
                                && snapshot.version() > previous_version
                        })
                        && metadata_enabled
                            .is_none_or(|enabled| snapshot.metadata_enabled == enabled)
                        && logs_enabled.is_none_or(|enabled| snapshot.logs_enabled == enabled)
                        && tools_enabled
                            .is_none_or(|enabled| snapshot.tool_events_enabled == enabled)
                        && files_enabled.is_none_or(|enabled| snapshot.files_enabled == enabled)
                        && errors_enabled.is_none_or(|enabled| snapshot.errors_enabled == enabled)
                        && sqlite_enabled.is_none_or(|enabled| snapshot.sqlite_enabled == enabled)
                        && model_api_enabled
                            .is_none_or(|enabled| snapshot.model_api_enabled == enabled)
                        && git_enabled
                            .is_none_or(|enabled| snapshot.command_switches.git == enabled)
                        && checks_enabled
                            .is_none_or(|enabled| snapshot.command_switches.checks == enabled)
                        && jev_calls_enabled
                            .is_none_or(|enabled| snapshot.command_switches.jev_calls == enabled)
                        && skills_enabled
                            .is_none_or(|enabled| snapshot.command_switches.skills == enabled)
                        && jev_enabled
                            .is_none_or(|enabled| snapshot.command_switches.jev == enabled)
                        && worktrees_enabled
                            .is_none_or(|enabled| snapshot.command_switches.worktrees == enabled)
                        && usage_enabled
                            .is_none_or(|enabled| snapshot.command_switches.usage == enabled)
                        && codex_enabled.is_none_or(|enabled| snapshot.codex_enabled == enabled)
                        && account_enabled
                            .is_none_or(|enabled| snapshot.account_settings.enabled == enabled)
                        && account_interval
                            .is_none_or(|interval| snapshot.account_settings.interval == interval)
                        && cloud_enabled
                            .is_none_or(|enabled| snapshot.cloud_settings.enabled == enabled)
                        && cloud_interval
                            .is_none_or(|interval| snapshot.cloud_settings.interval == interval)
                        && (0..2).all(|i| {
                            public_enabled[i].is_none_or(|enabled| {
                                snapshot.public_settings[i].enabled == enabled
                            }) && public_intervals[i].is_none_or(|interval| {
                                snapshot.public_settings[i].interval == interval
                            })
                        })
                        && (0..4).all(|i| {
                            platform_enabled[i].is_none_or(|enabled| {
                                snapshot.platform_settings[i].enabled == enabled
                            }) && platform_intervals[i].is_none_or(|interval| {
                                snapshot.platform_settings[i].interval == interval
                            })
                        })
                        && mcp_enabled.is_none_or(|enabled| snapshot.mcp_switches.mcp == enabled)
                        && web_enabled.is_none_or(|enabled| snapshot.mcp_switches.web == enabled)
                        && debug_enabled.is_none_or(|enabled| snapshot.debug_enabled == enabled)
                        && recording_applied.is_none_or(|enabled| {
                            serde_json::from_slice::<Value>(&snapshot.windows[3])
                                .ok()
                                .is_some_and(|value| value["jev"]["enabled"] == enabled)
                        })
                };
                if matches {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        let directory = state.monitoring.clone();
        let persisted_days = if retention_days.is_some() {
            retention_days
        } else {
            tokio::task::spawn_blocking(move || {
                use lam_history_store::{Access, Store};
                Store::open(&directory, Access::ReadOnly)
                    .and_then(|store| store.map_or(Ok(90), |store| store.activity_retention()))
            })
            .await
            .ok()
            .and_then(Result::ok)
        };
        return batch_settings_response(&state, applied, persisted_days);
    }
    if let Some(changes) = value.as_object().filter(|object| {
        !object.is_empty()
            && object.keys().all(|key| {
                [
                    "mcp_categories",
                    "mcp_descriptions",
                    "mcp_tags",
                    "tool_descriptions",
                ]
                .contains(&key.as_str())
            })
    }) {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let updated = {
            let mut current = state
                .customizations
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let Some(next) = merge_customizations(changes, &current) else {
                return reply(
                    StatusCode::CONFLICT,
                    "Invalid settings".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                );
            };
            *current = next.clone();
            next
        };
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied")
        })
        .await;
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().customizations == updated {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        let response = snapshots.borrow().windows[3].clone();
        let settings = serde_json::from_slice::<serde_json::Value>(&response)
            .ok()
            .and_then(|snapshot| snapshot.get("settings").cloned())
            .unwrap_or_else(|| json!({}));
        return reply(
            StatusCode::OK,
            settings.to_string().into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    if let Some(changes) = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("mcp_sources"))
    {
        let Some(changes) = changes.as_object().filter(|items| {
            items.len() <= 64
                && items.iter().all(|(name, enabled)| {
                    (1..=80).contains(&name.len())
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                        && enabled.is_boolean()
                })
        }) else {
            return reply(
                StatusCode::CONFLICT,
                "Invalid settings".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        };
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let updated = {
            let mut current = state
                .mcp_sources
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let mut next = current.clone();
            for (name, value) in changes {
                next.insert(name.clone(), value.as_bool().expect("validated boolean"));
            }
            if next.len() > 64 {
                return reply(
                    StatusCode::CONFLICT,
                    "Invalid settings".into(),
                    PLAIN,
                    POLICY,
                    false,
                    false,
                );
            }
            *current = next.clone();
            next
        };
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied")
        })
        .await;
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().mcp_sources == updated {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":updated,"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let debug_mode = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("debug_mode"))
        .and_then(serde_json::Value::as_bool);
    if let Some(enabled) = debug_mode {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let debug = Arc::clone(&state.debug);
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            debug
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .set_enabled(enabled);
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied");
        })
        .await;
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().debug_enabled == enabled {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":enabled,"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let logs = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("logs"))
        .and_then(serde_json::Value::as_bool);
    if let Some(logs) = logs {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            let mut journal = journal.lock().unwrap_or_else(|error| error.into_inner());
            journal.set_enabled(logs);
            journal.event("settings_applied");
        })
        .await;
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().logs_enabled == logs {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":logs}}).to_string().into(), JSON, POLICY, false, false);
    }
    let tool_events = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("tool_events"))
        .and_then(serde_json::Value::as_bool);
    if let Some(enabled) = tool_events {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        state
            .tool_events_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().tool_events_enabled == enabled {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":enabled,"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let files = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("files"))
        .and_then(serde_json::Value::as_bool);
    if let Some(enabled) = files {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        state
            .files_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().files_enabled == enabled {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":enabled,"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let errors = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("errors"))
        .and_then(serde_json::Value::as_bool);
    if let Some(enabled) = errors {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        state
            .errors_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().errors_enabled == enabled {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":enabled,"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let sqlite = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("sqlite"))
        .and_then(serde_json::Value::as_bool);
    if let Some(enabled) = sqlite {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        state
            .sqlite_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().sqlite_enabled == enabled {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"sqlite":enabled,"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let mcp_observation = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|object| object.len() == 1)
        .and_then(|object| {
            if let Some(value) = object.get("mcp").and_then(serde_json::Value::as_bool) {
                Some((true, value))
            } else {
                object
                    .get("web")
                    .and_then(serde_json::Value::as_bool)
                    .map(|value| (false, value))
            }
        });
    if let Some((is_mcp, enabled)) = mcp_observation {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        let updated = {
            let mut switches = state
                .mcp_switches
                .write()
                .unwrap_or_else(|error| error.into_inner());
            if is_mcp {
                switches.mcp = enabled;
            } else {
                switches.web = enabled;
            }
            *switches
        };
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied")
        })
        .await;
        if let Some((wake, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().mcp_switches == updated {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(StatusCode::OK, json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":updated.mcp,"web":updated.web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
    }
    let metadata = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("observations"))
        .and_then(serde_json::Value::as_object)
        .filter(|observations| observations.len() == 1)
        .and_then(|observations| observations.get("metadata"))
        .and_then(serde_json::Value::as_bool);
    if let Some(metadata) = metadata {
        let _guard = state.metadata_lock.lock().await;
        let mut snapshots = state.snapshots.clone();
        state
            .metadata_enabled
            .store(metadata, std::sync::atomic::Ordering::Release);
        let journal = Arc::clone(&state.journal);
        let _ = tokio::task::spawn_blocking(move || {
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied");
        })
        .await;
        if let Some((refresh, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = refresh.try_send(());
        }
        let applied = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if snapshots.borrow_and_update().metadata_enabled == metadata {
                    return true;
                }
                if snapshots.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);
        if !applied {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "Settings temporarily unavailable".into(),
                PLAIN,
                POLICY,
                false,
                false,
            );
        }
        return reply(
            StatusCode::OK,
            json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":metadata,"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        );
    }
    let days = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("activity_retention_days"))
        .and_then(serde_json::Value::as_i64)
        .filter(|days| (0..=3650).contains(days));
    let Some(days) = days else {
        return reply(
            StatusCode::CONFLICT,
            json!({"error":"觀察設定的格式或數值無效"})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        );
    };
    let _guard = state.metadata_lock.lock().await;
    let retention_state = Arc::clone(&state);
    let updated = tokio::task::spawn_blocking(move || {
        let state = retention_state;
        use lam_history_store::{Access, Store};
        let mut store =
            Store::open(&state.monitoring, Access::ReadWrite)?.expect("writer opens storage");
        let now = chrono::DateTime::from(std::time::SystemTime::now());
        let result = (|| {
            let mut removed = store.set_activity_retention(days, now)?;
            while removed == 500 {
                removed = store.continue_retention_cleanup(days, now)?;
            }
            store.activity_retention()
        })();
        if result.is_ok() {
            let _ = state.cleanup_wake.try_send(());
            state
                .journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .event("settings_applied");
        }
        if let Some((refresh, manual)) = &state.refresh {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = refresh.try_send(());
        }
        result
    })
    .await;
    if let Ok(Ok(current_days)) = updated {
        reply(
            StatusCode::OK,
            json!({"activity_retention_days":current_days})
                .to_string()
                .into(),
            JSON,
            POLICY,
            false,
            false,
        )
    } else {
        reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "History temporarily unavailable".into(),
            PLAIN,
            POLICY,
            false,
            false,
        )
    }
}

fn local(request: &Request<Incoming>, hosts: &[String; 2]) -> bool {
    if request.uri().authority().is_some() {
        return false;
    }
    let headers = request.headers();
    let mut hosts_received = headers.get_all(header::HOST).iter();
    let Some(host) = hosts_received.next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if hosts_received.next().is_some() || !hosts.iter().any(|allowed| host == allowed) {
        return false;
    }
    let mut origins = headers.get_all(header::ORIGIN).iter();
    if let Some(origin) = origins.next() {
        if origins.next().is_some()
            || origin
                .to_str()
                .ok()
                .and_then(|value| value.strip_prefix("http://"))
                != Some(host)
        {
            return false;
        }
    }
    !headers
        .get_all("sec-fetch-site")
        .iter()
        .any(|value| value == "cross-site")
}

fn unavailable_events(status: StatusCode) -> Response<Body> {
    reply(
        status,
        "Event stream unavailable".into(),
        PLAIN,
        POLICY,
        false,
        false,
    )
}

fn bounded_decimal(value: &str, max_digits: usize) -> Option<usize> {
    crate::history::decimal_digits(value, max_digits).and_then(|number| usize::try_from(number).ok())
}

fn route(state: &State, path: &str, query: &str, gzip: bool) -> Response<Body> {
    let (status, body, mime, policy) = match path {
        "/api/codex/sql" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let id = values.get("id").copied();
            let mask = values.get("mask").copied().unwrap_or("1");
            let lazy = values.get("lazy").copied();
            let field = values.get("field").copied();
            let offset = values.get("offset").copied();
            let revision = values.get("revision").copied();
            let paging = if field.is_some() {
                offset.is_some() && revision.is_some()
            } else {
                offset.is_none() && revision.is_none()
            };
            let valid = unique
                && query.len() <= if lazy.is_some() { 1024 } else { 80 }
                && values.keys().all(|key| {
                    matches!(
                        *key,
                        "id" | "mask" | "lazy" | "field" | "offset" | "revision"
                    )
                })
                && id.is_some_and(|id| {
                    id.len() == 64
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && paging
                && (lazy.is_some() || field.is_none())
                && field.is_none_or(|value| {
                    matches!(value, "request" | "response" | "output" | "sql" | "text")
                })
                && offset.is_none_or(|value| {
                    bounded_decimal(value, 12).is_some()
                })
                && revision.is_none_or(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if lazy.is_some() {
                        "Invalid content page"
                    } else {
                        "Invalid SQL record"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .sqlite_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let snapshot = serde_json::from_slice::<serde_json::Value>(
                    &state.snapshots.borrow().windows[3],
                )
                .ok();
                let event = snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot["codex"]["sqlite"]["events"].as_array())
                    .and_then(|events| events.iter().find(|event| event["id"].as_str() == id))
                    .cloned();
                if let Some(event) = event {
                    let value = if event["recognition"] == "diagnostic_log" {
                        let body = match event["source"].as_str() {
                            Some("codex_core") => state
                                .core_source
                                .read()
                                .unwrap_or_else(|error| error.into_inner())
                                .clone()
                                .and_then(|source| source.diagnostic_body(&event, lazy.is_some())),
                            Some("codex_desktop") => state
                                .desktop_sources
                                .read()
                                .unwrap_or_else(|error| error.into_inner())
                                .clone()
                                .into_iter()
                                .find_map(|source| source.diagnostic_body(&event)),
                            _ => None,
                        };
                        body.as_deref()
                            .map(|body| {
                                crate::sessions::details::diagnostic_sql(
                                    event["module"].as_str().unwrap_or(""),
                                    body,
                                    mask == "1",
                                    lazy.is_some(),
                                )
                            })
                            .unwrap_or_else(|| {
                                crate::sessions::details::sql_content(
                                    None,
                                    mask == "1",
                                    lazy.is_some(),
                                )
                            })
                    } else {
                        let candidates = event["thread_id"]
                            .as_str()
                            .map(|thread| {
                                state
                                    .details
                                    .read()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .candidates(thread)
                            })
                            .unwrap_or_default();
                        candidates.into_iter().find_map(|file| file.sql(&event, mask == "1", lazy.is_some()))
                            .unwrap_or_else(|| json!({"sql":null,"truncated":false,"content_status":"unavailable"}))
                    };
                    if lazy.is_none() {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    } else {
                        match offset
                            .map(|value| bounded_decimal(value, 12))
                            .unwrap_or(Some(0))
                            .ok_or(true)
                            .and_then(|offset| {
                                crate::sessions::details::page_tool(
                                    value,
                                    field,
                                    offset,
                                    revision,
                                    mask == "1",
                                )
                            }) {
                            Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                            Err(false) => (StatusCode::BAD_REQUEST, "null".into(), JSON, POLICY),
                            Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                        }
                    }
                } else {
                    (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                }
            }
        }
        "/api/codex/error" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let id = values.get("id").copied();
            let mask = values.get("mask").copied().unwrap_or("1");
            let lazy = values.get("lazy").copied();
            let field = values.get("field").copied();
            let offset = values.get("offset").copied();
            let revision = values.get("revision").copied();
            let paging = if field.is_some() {
                offset.is_some() && revision.is_some()
            } else {
                offset.is_none() && revision.is_none()
            };
            let valid = unique
                && query.len() <= if lazy.is_some() { 1024 } else { 256 }
                && values.keys().all(|key| {
                    matches!(
                        *key,
                        "id" | "mask" | "lazy" | "field" | "offset" | "revision"
                    )
                })
                && id.is_some_and(|id| {
                    id.len() == 64
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && paging
                && (lazy.is_some() || field.is_none())
                && field.is_none_or(|value| {
                    matches!(value, "request" | "response" | "output" | "sql" | "text")
                })
                && offset.is_none_or(|value| {
                    bounded_decimal(value, 12).is_some()
                })
                && revision.is_none_or(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if lazy.is_some() {
                        "Invalid content page"
                    } else {
                        "Invalid error identity"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || (!state
                    .errors_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
                    && !state
                        .journal
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .enabled())
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let event = id.and_then(|id| state.snapshots.borrow().error_details.get(id).cloned());
                if let Some(event) = event {
                    let source = event["source"].as_str().unwrap_or("unknown");
                    let candidates = event["thread_id"]
                        .as_str()
                        .map(|thread| {
                            state
                                .details
                                .read()
                                .unwrap_or_else(|error| error.into_inner())
                                .candidates(thread)
                        })
                        .unwrap_or_default();
                    let core_source = state
                        .core_source
                        .read()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    let value = if source == "codex_core" {
                        core_source.and_then(|source| source.error_detail(&event, mask == "1", lazy.is_some()))
                    } else if source == "codex_desktop" {
                        let sources = state.desktop_sources.read().unwrap_or_else(|error| error.into_inner()).clone();
                        sources.into_iter().find_map(|source| source.error_detail(&event, mask == "1", lazy.is_some()))
                    } else {
                        candidates.into_iter().find_map(|file| file.error(&event, mask == "1", lazy.is_some()))
                    }
                        .unwrap_or_else(|| json!({"text":null,"truncated":false,"content_status":"unavailable","source":source}));
                    if lazy.is_none() {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    } else {
                        match offset
                            .map(|value| bounded_decimal(value, 12))
                            .unwrap_or(Some(0))
                            .ok_or(true)
                            .and_then(|offset| {
                                crate::sessions::details::page_tool(
                                    value,
                                    field,
                                    offset,
                                    revision,
                                    mask == "1",
                                )
                            }) {
                            Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                            Err(false) => (StatusCode::BAD_REQUEST, "null".into(), JSON, POLICY),
                            Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                        }
                    }
                } else {
                    (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                }
            }
        }
        "/api/codex/file" | "/api/codex/file-summary" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let is_summary = path == "/api/codex/file-summary";
            let window = values.get("window").and_then(|value| {
                ["1h", "24h", "7d", "all"]
                    .iter()
                    .position(|name| name == value)
            });
            let valid = if is_summary {
                unique && query.len() <= 30 && values.len() == 1 && window.is_some()
            } else {
                let thread = values.get("thread_id").copied();
                let call = values.get("call_id").copied();
                let name = values.get("path").copied();
                unique
                    && query.len() <= 8192
                    && values.len() == 3
                    && thread.is_some_and(|id| {
                        id.len() == 36
                            && id
                                .bytes()
                                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                    })
                    && call.is_some_and(|id| {
                        !id.is_empty()
                            && id.len() <= 160
                            && id
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                    })
                    && name.is_some_and(|name| {
                        (1..=2048).contains(&name.chars().count())
                            && !name.chars().any(|ch| ch < ' ' || ch == '\u{7f}')
                    })
            };
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if is_summary {
                        "Invalid file window"
                    } else {
                        "Invalid file event"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .files_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let index = if is_summary {
                    window.expect("validated window")
                } else {
                    3
                };
                let snapshot = serde_json::from_slice::<serde_json::Value>(
                    &state.snapshots.borrow().windows[index],
                );
                let events = snapshot
                    .as_ref()
                    .ok()
                    .and_then(|snapshot| snapshot["codex"]["file_activity"]["events"].as_array());
                if let Some(events) = events {
                    if is_summary {
                        let mut selected = std::collections::BTreeSet::new();
                        let mut rows = Vec::new();
                        for event in events {
                            let Some(name) = event["path"].as_str() else {
                                continue;
                            };
                            let key = (
                                event
                                    .get("workdir")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_owned),
                                name.to_owned(),
                            );
                            if selected.insert(key) && rows.len() < 100 {
                                let mut event = event.clone();
                                let metadata = crate::sessions::file_metadata::observed(&event);
                                if let (Some(row), Some(metadata)) =
                                    (event.as_object_mut(), metadata.as_object())
                                {
                                    row.extend(metadata.clone());
                                }
                                rows.push(event);
                            }
                        }
                        let checked =
                            chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        (StatusCode::OK, json!({"files":rows,"checked_at":checked,"limit":100,"total":selected.len()}).to_string().into(), JSON, POLICY)
                    } else {
                        let event = events.iter().find(|event| {
                            event["thread_id"].as_str() == values.get("thread_id").copied()
                                && event["call_id"].as_str() == values.get("call_id").copied()
                                && event["path"].as_str() == values.get("path").copied()
                        });
                        if let Some(event) = event {
                            (
                                StatusCode::OK,
                                crate::sessions::file_metadata::observed(event)
                                    .to_string()
                                    .into(),
                                JSON,
                                POLICY,
                            )
                        } else {
                            (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                        }
                    }
                } else {
                    (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                }
            }
        }
        "/api/codex/git" | "/api/codex/check" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let thread = values.get("thread_id").copied();
            let call = values.get("call_id").copied();
            let operation = values.get("operation").copied();
            let checks = path == "/api/codex/check";
            let mask = values.get("mask").copied().unwrap_or("1");
            let lazy = values.get("lazy").copied();
            let field = values.get("field").copied();
            let offset = values.get("offset").copied();
            let revision = values.get("revision").copied();
            let paging = if field.is_some() {
                offset.is_some() && revision.is_some()
            } else {
                offset.is_none() && revision.is_none()
            };
            let valid = unique
                && query.len() <= if lazy.is_some() { 1024 } else { 512 }
                && values.keys().all(|key| {
                    matches!(
                        *key,
                        "thread_id"
                            | "call_id"
                            | "operation"
                            | "mask"
                            | "lazy"
                            | "field"
                            | "offset"
                            | "revision"
                    )
                })
                && thread.is_some_and(|id| {
                    id.len() == 36
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                })
                && call.is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 160
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                })
                && operation.is_some_and(|value| {
                    if checks {
                        (1..=160).contains(&value.chars().count())
                            && !value.chars().any(|ch| ch < ' ' || ch == '\u{7f}')
                    } else {
                        (1..=40).contains(&value.len())
                            && value
                                .bytes()
                                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
                    }
                })
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && paging
                && (lazy.is_some() || field.is_none())
                && field.is_none_or(|value| {
                    matches!(value, "request" | "response" | "output" | "sql" | "text")
                })
                && offset.is_none_or(|value| {
                    bounded_decimal(value, 12).is_some()
                })
                && revision.is_none_or(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if lazy.is_some() {
                        "Invalid content page"
                    } else {
                        "Invalid command event"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire) || {
                let switches = state
                    .command_switches
                    .read()
                    .unwrap_or_else(|error| error.into_inner());
                if checks {
                    !switches.checks
                } else {
                    !switches.git
                }
            } {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let call = call.expect("validated call");
                let selected = state
                    .details
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .select_call(thread.expect("validated thread"), call);
                let result = selected.and_then(|file| file.command(call, operation.expect("validated operation"), checks, mask == "1", lazy.is_some())).or_else(|| Some(json!({"commands":[],"request":null,"response":null,"output":null,"output_scope":null,"result_scope":null,"truncated":false,"content_status":"unavailable"})));
                match result {
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    Some(value) if lazy.is_none() => {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    }
                    Some(value) => match offset
                        .map(|value| bounded_decimal(value, 12))
                        .unwrap_or(Some(0))
                        .ok_or(true)
                        .and_then(|offset| {
                            crate::sessions::details::page_tool(
                                value,
                                field,
                                offset,
                                revision,
                                mask == "1",
                            )
                        }) {
                        Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                        Err(false) => (StatusCode::BAD_REQUEST, "null".into(), JSON, POLICY),
                        Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    },
                }
            }
        }
        "/api/mcp/files" => {
            let mut values = std::collections::BTreeMap::new();
            let unique = form_urlencoded::parse(query.as_bytes()).all(|(key, value)| {
                values
                    .insert(key.into_owned(), value.into_owned())
                    .is_none()
            });
            let server = values.get("server").map(String::as_str);
            let document = values.get("document").map(String::as_str);
            let valid = unique
                && query.len() <= 256
                && values
                    .keys()
                    .all(|key| matches!(key.as_str(), "server" | "document"))
                && server.is_some_and(|name| {
                    (1..=80).contains(&name.len())
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                })
                && document.is_none_or(|id| {
                    id.len() == 32
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid MCP document".into(),
                    PLAIN,
                    POLICY,
                )
            } else {
                let server = server.expect("validated server");
                let home = state.monitoring.parent().unwrap_or(&state.monitoring);
                match crate::sessions::mcp_documents::configuration(home, server) {
                    Some(config) => {
                        let result = if let Some(id) = document {
                            crate::sessions::mcp_documents::read_document(&config, id)
                        } else {
                            Some(
                                json!({"server":server,"files":crate::sessions::mcp_documents::documents(&config)}),
                            )
                        };
                        match result {
                            Some(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                            None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                        }
                    }
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                }
            }
        }
        "/api/codex/worktree" => {
            let values: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let id = values
                .first()
                .filter(|(key, _)| key == "id")
                .map(|(_, value)| value.as_ref());
            let valid = query.len() <= 64
                && values.len() == 1
                && id.is_some_and(|id| {
                    id.len() == 24
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid worktree query".into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .command_switches
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .worktrees
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else if let Some(value) = state
                .snapshots
                .borrow()
                .worktree_details
                .get(id.expect("validated id"))
            {
                (StatusCode::OK, value.to_string().into(), JSON, POLICY)
            } else {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            }
        }
        "/api/codex/skill" => {
            let mut values = std::collections::BTreeMap::new();
            let unique = form_urlencoded::parse(query.as_bytes()).all(|(key, value)| {
                values
                    .insert(key.into_owned(), value.into_owned())
                    .is_none()
            });
            let skill = values.get("skill").map(String::as_str);
            let file = values.get("file").map(String::as_str);
            let thread = values.get("thread_id").map(String::as_str);
            let call = values.get("call_id").map(String::as_str);
            let valid = unique
                && query.len() <= 2048
                && values
                    .keys()
                    .all(|key| matches!(key.as_str(), "skill" | "file" | "thread_id" | "call_id"))
                && skill.is_some_and(|name| {
                    (1..=160).contains(&name.chars().count())
                        && name != "."
                        && name != ".."
                        && !name
                            .chars()
                            .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
                })
                && (thread.is_some() == call.is_some())
                && thread.is_none_or(|value| {
                    value.len() == 36
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                })
                && call.is_none_or(|value| {
                    (1..=160).contains(&value.len())
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                })
                && file.is_none_or(|value| {
                    (1..=512).contains(&value.chars().count())
                        && !value
                            .chars()
                            .any(|ch| ch.is_control() || ch == '\\' || ch == ':')
                        && !value.starts_with('/')
                        && value
                            .split('/')
                            .all(|part| !["", ".", ".."].contains(&part))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid skill".into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .command_switches
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .skills
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let skill = skill.expect("validated skill");
                let snapshot =
                    serde_json::from_slice::<Value>(&state.snapshots.borrow().windows[3]).ok();
                let events = snapshot
                    .as_ref()
                    .and_then(|value| value["codex"]["skills"]["events"].as_array());
                let mut candidates = Vec::new();
                let mut tail_budget = 8 * 1024 * 1024;
                if let Some(events) = events {
                    let index = state
                        .details
                        .read()
                        .unwrap_or_else(|error| error.into_inner());
                    for event in events {
                        let (Some(event_thread), Some(event_call)) =
                            (event["thread_id"].as_str(), event["call_id"].as_str())
                        else {
                            continue;
                        };
                        if event["skill"] != skill
                            || thread.is_some_and(|id| id != event_thread)
                            || call.is_some_and(|id| id != event_call)
                        {
                            continue;
                        }
                        let current = index
                            .select_call(event_thread, event_call)
                            .map(|source| source.skill_paths(event_call, skill))
                            .unwrap_or_default();
                        let needs_tail = current.is_empty() && event["has_document"] == false;
                        candidates.extend(current);
                        if needs_tail && tail_budget > 0 {
                            for source in index.candidates(event_thread) {
                                if tail_budget == 0 {
                                    break;
                                }
                                if let Some(timestamp) = event["timestamp"].as_str() {
                                    candidates.extend(source.retained_skill_paths(
                                        event_call,
                                        skill,
                                        timestamp,
                                        &mut tail_budget,
                                    ));
                                }
                            }
                        }
                    }
                }
                let mut identities = std::collections::BTreeSet::new();
                for item in &candidates {
                    identities.insert((item.path.clone(), item.workdir.clone()));
                }
                let empty =
                    || json!({"skill":skill,"files":[],"files_truncated":false,"documents":[]});
                let result = if thread.is_none() && identities.len() > 1 {
                    empty()
                } else {
                    candidates
                        .iter()
                        .rev()
                        .take(16)
                        .find_map(|candidate| {
                            crate::sessions::skill_detail::render(skill, candidate, file)
                        })
                        .unwrap_or_else(empty)
                };
                (StatusCode::OK, result.to_string().into(), JSON, POLICY)
            }
        }
        "/api/codex/jev" => {
            let mut values = std::collections::BTreeMap::new();
            let unique = form_urlencoded::parse(query.as_bytes()).all(|(key, value)| {
                values
                    .insert(key.into_owned(), value.into_owned())
                    .is_none()
            });
            let thread = values.get("thread").map(String::as_str);
            let call = values.get("call").map(String::as_str);
            let lazy = values.get("lazy").map(String::as_str);
            let index = values
                .get("index")
                .and_then(|value| if lazy.is_some() {
                    crate::history::decimal_digits(value, 2)
                } else {
                    crate::history::query_number(value).map(u64::from)
                })
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value < 100);
            let mask = values.get("mask").map(String::as_str).unwrap_or("1");
            let field = values.get("field").map(String::as_str);
            let offset = values
                .get("offset")
                .and_then(|value| crate::history::decimal_digits(value, 12))
                .and_then(|value| usize::try_from(value).ok());
            let revision = values.get("revision").map(String::as_str);
            let valid = unique
                && query.len() <= 1024
                && values.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "thread" | "call" | "index" | "mask" | "lazy" | "field" | "offset" | "revision"
                    )
                })
                && thread.is_some_and(|value| (1..=160).contains(&value.chars().count()))
                && call.is_some_and(|value| (1..=160).contains(&value.chars().count()))
                && index.is_some()
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && (lazy.is_none() || (thread.is_some_and(|id| id.len() == 36 && id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-'))
                    && call.is_some_and(|id| id.len() <= 160 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte)))))
                && field.is_none_or(|value| matches!(value, "request" | "response" | "output" | "sql" | "text"))
                && revision.is_none_or(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
                && if field.is_some() {
                    lazy.is_some() && offset.is_some() && revision.is_some()
                } else {
                    !values.contains_key("offset") && revision.is_none()
                };
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid Jev call".into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .command_switches
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .jev_calls
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let thread = thread.expect("validated thread");
                let call = call.expect("validated call");
                let index = index.expect("validated index");
                let snapshot = serde_json::from_slice::<serde_json::Value>(
                    &state.snapshots.borrow().windows[3],
                )
                .ok();
                let event = snapshot
                    .as_ref()
                    .and_then(|value| value["codex"]["threads"].as_array())
                    .into_iter()
                    .flatten()
                    .find(|row| row["thread_id"] == thread)
                    .and_then(|row| row["jev_calls"].as_array())
                    .into_iter()
                    .flatten()
                    .find(|event| {
                        event["call_id"] == call && event["index"].as_u64() == Some(index as u64)
                    });
                let detail = event
                    .and_then(|event| event["operation"].as_str())
                    .and_then(|operation| {
                        state
                            .details
                            .read()
                            .unwrap_or_else(|error| error.into_inner())
                            .select_call(thread, call)
                            .and_then(|file| file.jev(call, index, operation, mask == "1", lazy.is_some()))
                    });
                match detail {
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    Some(value) if lazy.is_none() => {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    }
                    Some(value) => match crate::sessions::details::page_tool(
                        value,
                        field,
                        offset.unwrap_or(0),
                        revision,
                        mask == "1",
                    ) {
                        Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                        Err(false) => (StatusCode::BAD_REQUEST, "null".into(), PLAIN, POLICY),
                        Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    },
                }
            }
        }
        "/api/codex/mcp" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let thread = values.get("thread_id").copied();
            let call = values.get("call_id").copied();
            let index = values.get("index").copied();
            let mask = values.get("mask").copied().unwrap_or("1");
            let lazy = values.get("lazy").copied();
            let field = values.get("field").copied();
            let offset = values.get("offset").copied();
            let revision = values.get("revision").copied();
            let paging = if field.is_some() {
                offset.is_some() && revision.is_some()
            } else {
                offset.is_none() && revision.is_none()
            };
            let valid = unique
                && query.len() <= if lazy.is_some() { 1024 } else { 512 }
                && values.keys().all(|key| {
                    matches!(
                        *key,
                        "thread_id"
                            | "call_id"
                            | "index"
                            | "mask"
                            | "lazy"
                            | "field"
                            | "offset"
                            | "revision"
                    )
                })
                && thread.is_some_and(|id| {
                    id.len() == 36
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                })
                && call.is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 160
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                })
                && index.is_some_and(|value| {
                    bounded_decimal(value, 2).is_some()
                })
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && paging
                && (lazy.is_some() || field.is_none())
                && field.is_none_or(|value| {
                    matches!(value, "request" | "response" | "output" | "sql" | "text")
                })
                && offset.is_none_or(|value| {
                    bounded_decimal(value, 12).is_some()
                })
                && revision.is_none_or(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if lazy.is_some() {
                        "Invalid content page"
                    } else {
                        "Invalid MCP call"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire) {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let call = call.expect("validated call");
                let selected = state
                    .details
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .select_call(thread.expect("validated thread"), call);
                let index = bounded_decimal(index.expect("validated index"), 2)
                    .expect("bounded index");
                let snapshot = serde_json::from_slice::<serde_json::Value>(
                    &state.snapshots.borrow().windows[3],
                )
                .ok();
                let event = snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot["mcp"]["events"].as_array())
                    .and_then(|events| {
                        events.iter().find(|event| {
                            event["thread_id"].as_str() == thread
                                && event["call_id"].as_str() == Some(call)
                                && event["index"].as_u64() == Some(index as u64)
                        })
                    });
                let result = event.and_then(|event| {
                    let server = event["server"].as_str()?;
                    if !state
                        .mcp_switches
                        .read()
                        .unwrap_or_else(|error| error.into_inner())
                        .allows(server)
                    {
                        return None;
                    }
                    if state
                        .mcp_sources
                        .read()
                        .unwrap_or_else(|error| error.into_inner())
                        .get(server)
                        == Some(&false)
                    {
                        return None;
                    }
                    let operation = event["tool"].as_str()?;
                    selected.and_then(|file| {
                        file.mcp(call, index, server, operation, mask == "1", lazy.is_some())
                    })
                });
                match result {
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    Some(value) if lazy.is_none() => {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    }
                    Some(value) => match offset
                        .map(|value| bounded_decimal(value, 12))
                        .unwrap_or(Some(0))
                        .ok_or(true)
                        .and_then(|offset| {
                            crate::sessions::details::page_tool(
                                value,
                                field,
                                offset,
                                revision,
                                mask == "1",
                            )
                        }) {
                        Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                        Err(false) => (StatusCode::BAD_REQUEST, "null".into(), JSON, POLICY),
                        Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    },
                }
            }
        }
        "/api/codex/tool" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let thread = values.get("thread_id").copied();
            let call = values.get("call_id").copied();
            let mask = values.get("mask").copied().unwrap_or("1");
            let lazy = values.get("lazy").copied();
            let field = values.get("field").copied();
            let offset = values.get("offset").copied();
            let revision = values.get("revision").copied();
            let paging = if field.is_some() {
                offset.is_some() && revision.is_some()
            } else {
                offset.is_none() && revision.is_none()
            };
            let valid = unique
                && query.len() <= if lazy.is_some() { 1024 } else { 512 }
                && values.keys().all(|key| {
                    matches!(
                        *key,
                        "thread_id" | "call_id" | "mask" | "lazy" | "field" | "offset" | "revision"
                    )
                })
                && thread.is_some_and(|id| {
                    id.len() == 36
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                })
                && call.is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 160
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                })
                && matches!(mask, "0" | "1")
                && lazy.is_none_or(|value| value == "1")
                && paging
                && (lazy.is_some() || field.is_none())
                && field.is_none_or(|value| {
                    matches!(value, "request" | "response" | "output" | "sql" | "text")
                })
                && offset.is_none_or(|value| {
                    bounded_decimal(value, 12).is_some()
                })
                && revision.is_none_or(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    if lazy.is_some() {
                        "Invalid content page"
                    } else {
                        "Invalid tool call"
                    }
                    .into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || !state
                    .tool_events_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let call = call.expect("validated call");
                let selected = state
                    .details
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .select_call(thread.expect("validated thread"), call);
                let result = selected.and_then(|file| file.tool(call, mask == "1", lazy.is_some()));
                match result {
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    Some(value) if lazy.is_none() => {
                        (StatusCode::OK, value.to_string().into(), JSON, POLICY)
                    }
                    Some(value) => match offset
                        .map(|value| bounded_decimal(value, 12))
                        .unwrap_or(Some(0))
                        .ok_or(true)
                        .and_then(|offset| {
                            crate::sessions::details::page_tool(
                                value,
                                field,
                                offset,
                                revision,
                                mask == "1",
                            )
                        }) {
                        Ok(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                        Err(false) => (StatusCode::BAD_REQUEST, "null".into(), JSON, POLICY),
                        Err(true) => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                    },
                }
            }
        }
        "/api/codex/context" | "/api/codex/agent-message" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let mut values = std::collections::BTreeMap::new();
            let unique = params
                .iter()
                .all(|(key, value)| values.insert(key.as_ref(), value.as_ref()).is_none());
            let thread = values.get("thread_id").copied();
            let call = values.get("call_id").copied();
            let mask = values.get("mask").copied().unwrap_or("1");
            let index = values.get("index").copied();
            let valid = query.len() <= 256
                && unique
                && values.keys().all(|key| {
                    matches!(*key, "thread_id" | "call_id" | "mask")
                        || (*key == "index" && path == "/api/codex/agent-message")
                })
                && thread.is_some_and(|id| {
                    id.len() == 36
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
                })
                && call.is_none_or(|id| {
                    !id.is_empty()
                        && id.len() <= 160
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
                })
                && matches!(mask, "0" | "1");
            let valid = valid
                && index.is_none_or(|value| {
                    bounded_decimal(value, 2).is_some()
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid context query".into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state.codex.load(std::sync::atomic::Ordering::Acquire)
                || (path == "/api/codex/agent-message"
                    && !state
                        .tool_events_enabled
                        .load(std::sync::atomic::Ordering::Acquire))
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let detail_index = state
                    .details
                    .read()
                    .unwrap_or_else(|error| error.into_inner());
                let selected = if path == "/api/codex/agent-message" {
                    call.and_then(|call| {
                        detail_index.select_call(thread.expect("validated thread"), call)
                    })
                } else {
                    detail_index.select(thread.expect("validated thread"))
                };
                drop(detail_index);
                let result = selected.and_then(|file| {
                    if path == "/api/codex/agent-message" {
                        file.agent_message(
                            call?,
                            index.and_then(|value| bounded_decimal(value, 2)).unwrap_or(0),
                            mask == "1",
                        )
                    } else {
                        file.context(call, mask == "1")
                    }
                });
                match result {
                    Some(value) => (StatusCode::OK, value.to_string().into(), JSON, POLICY),
                    None => (StatusCode::CONFLICT, "null".into(), JSON, POLICY),
                }
            }
        }
        "/api/codex/instructions" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let scope = params
                .iter()
                .find(|(key, _)| key == "scope")
                .map(|(_, value)| value.as_ref());
            let project_id = params
                .iter()
                .find(|(key, _)| key == "project_id")
                .map(|(_, value)| value.as_ref());
            let valid_id = project_id.is_none_or(|id| {
                (1..=160).contains(&id.chars().count())
                    && !id
                        .chars()
                        .any(|ch| ch <= '\u{1f}' || ch == '\u{7f}' || ch == '/' || ch == '\\')
            });
            let valid = query.len() <= 1024
                && params
                    .iter()
                    .all(|(key, _)| key == "scope" || key == "project_id")
                && valid_id
                && match scope {
                    Some("global") => params.len() == 1 && project_id.is_none(),
                    Some("project") => params.len() == 2 && project_id.is_some(),
                    _ => false,
                };
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid project query".into(),
                    PLAIN,
                    POLICY,
                )
            } else {
                let home = state.monitoring.parent().expect("monitoring parent");
                let roots = if scope == Some("global") {
                    Some(vec![home.to_path_buf()])
                } else if !state
                    .metadata_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    None
                } else {
                    let id = project_id.expect("validated project id");
                    let snapshot: Value =
                        serde_json::from_slice(&state.snapshots.borrow().windows[3])
                            .unwrap_or(Value::Null);
                    let known = snapshot["codex"]["projects"]
                        .as_array()
                        .is_some_and(|projects| projects.iter().any(|project| project["id"] == id));
                    if known {
                        let threads: Vec<_> = snapshot["codex"]["threads"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|row| row["project_id"] == id)
                            .cloned()
                            .collect();
                        Some(
                            crate::sessions::projects::detail_folders(home, id, &threads)
                                .0
                                .into_iter()
                                .map(PathBuf::from)
                                .collect(),
                        )
                    } else {
                        None
                    }
                };
                if let Some(roots) = roots {
                    let mut result = crate::sessions::instructions::read(&roots);
                    result["scope"] = json!(scope);
                    if let Some(id) = project_id {
                        result["project_id"] = json!(id);
                    }
                    (StatusCode::OK, result.to_string().into(), JSON, POLICY)
                } else {
                    (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                }
            }
        }
        "/api/codex/project" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            let project_id = params
                .first()
                .filter(|(key, _)| key == "project_id")
                .map(|(_, value)| value.as_ref());
            let valid = query.len() <= 1024
                && params.len() == 1
                && project_id.is_some_and(|id| {
                    let length = id.chars().count();
                    (1..=160).contains(&length)
                        && !id
                            .chars()
                            .any(|ch| ch <= '\u{1f}' || ch == '\u{7f}' || ch == '/' || ch == '\\')
                });
            if !valid {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid project query".into(),
                    PLAIN,
                    POLICY,
                )
            } else if !state
                .metadata_enabled
                .load(std::sync::atomic::Ordering::Acquire)
            {
                (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
            } else {
                let project_id = project_id.expect("validated project id");
                let bytes = state.snapshots.borrow().windows[3].clone();
                let snapshot: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let project = snapshot["codex"]["projects"]
                    .as_array()
                    .and_then(|projects| {
                        projects.iter().find(|project| project["id"] == project_id)
                    });
                if let Some(project) = project {
                    let selected: Vec<_> = snapshot["codex"]["threads"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|row| row["project_id"] == project_id)
                        .cloned()
                        .collect();
                    let (folders, source) = crate::sessions::projects::detail_folders(
                        state.monitoring.parent().expect("monitoring parent"),
                        project_id,
                        &selected,
                    );
                    let threads: Vec<Value> = selected
                        .iter()
                        .map(|row| {
                            let mut result = serde_json::Map::new();
                            for key in [
                                "thread_id",
                                "thread_name",
                                "environment",
                                "status",
                                "archived",
                                "model",
                                "updated_at",
                            ] {
                                result.insert(key.into(), row[key].clone());
                            }
                            Value::Object(result)
                        })
                        .collect();
                    let mut result = project.clone();
                    result["project_id"] = json!(project_id);
                    result["folders"] = json!(folders);
                    result["source"] = json!(source);
                    result["threads"] = json!(threads);
                    (StatusCode::OK, result.to_string().into(), JSON, POLICY)
                } else {
                    (StatusCode::CONFLICT, "null".into(), JSON, POLICY)
                }
            }
        }
        "/api/codex/dots" => {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes()).collect();
            if query.len() > 64 || params.iter().any(|(key, _)| key != "window") {
                (StatusCode::NOT_FOUND, "Not found".into(), PLAIN, POLICY)
            } else if params.len() > 1
                || params
                    .first()
                    .is_some_and(|(_, value)| !["1h", "24h", "7d", "all"].contains(&value.as_ref()))
            {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid activity window".into(),
                    PLAIN,
                    POLICY,
                )
            } else {
                let window = params.first().map_or("all", |(_, value)| value.as_ref());
                let result = if state
                    .metadata_enabled
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    crate::dots::read(
                        state.monitoring.parent().expect("monitoring parent"),
                        window,
                        chrono::DateTime::from(std::time::SystemTime::now()),
                    )
                } else {
                    json!({"health":"disabled"})
                };
                (StatusCode::OK, result.to_string().into(), JSON, POLICY)
            }
        }
        "/api/settings" if query.is_empty() => {
            use lam_history_store::{Access, Store};
            let days = Store::open(&state.monitoring, Access::ReadOnly)
                .and_then(|store| store.map_or(Ok(90), |store| store.activity_retention()));
            match days {
                Ok(days) => (
                    StatusCode::OK,
                    json!({"max_files":state.tracking.read().unwrap_or_else(|error| error.into_inner()).max_files,"track_all":state.tracking.read().unwrap_or_else(|error| error.into_inner()).track_all,"mcp_categories":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_categories.clone(),"mcp_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_descriptions.clone(),"mcp_tags":state.customizations.read().unwrap_or_else(|error| error.into_inner()).mcp_tags.clone(),"tool_descriptions":state.customizations.read().unwrap_or_else(|error| error.into_inner()).tool_descriptions.clone(),"mcp_sources":state.mcp_sources.read().unwrap_or_else(|error| error.into_inner()).clone(),"activity_retention_days":days,"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"api_intervals":{"codex_account":state.account.settings().interval,"codex_cloud":state.cloud.settings().interval,"public_openai":state.public_openai.settings().interval,"public_codex":state.public_codex.settings().interval,"platform_models":state.platform_sources[0].settings().interval,"platform_usage":state.platform_sources[1].settings().interval,"platform_decisions":state.platform_sources[2].settings().interval,"platform_costs":state.platform_sources[3].settings().interval},"observations":{"codex":state.codex.load(std::sync::atomic::Ordering::Acquire),"codex_account":state.account.settings().enabled,"codex_cloud":state.cloud.settings().enabled,"public_openai":state.public_openai.settings().enabled,"public_codex":state.public_codex.settings().enabled,"platform_models":state.platform_sources[0].settings().enabled,"platform_usage":state.platform_sources[1].settings().enabled,"platform_decisions":state.platform_sources[2].settings().enabled,"platform_costs":state.platform_sources[3].settings().enabled,"usage":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).usage,"worktrees":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).worktrees,"jev":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev,"skills":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).skills,"jev_calls":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).jev_calls,"model_api":state.model_api_enabled.load(std::sync::atomic::Ordering::Acquire),"git":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).git,"checks":state.command_switches.read().unwrap_or_else(|error| error.into_inner()).checks,"sqlite":state.sqlite_enabled.load(std::sync::atomic::Ordering::Acquire),"errors":state.errors_enabled.load(std::sync::atomic::Ordering::Acquire),"mcp":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).mcp,"web":state.mcp_switches.read().unwrap_or_else(|error| error.into_inner()).web,"files":state.files_enabled.load(std::sync::atomic::Ordering::Acquire),"tool_events":state.tool_events_enabled.load(std::sync::atomic::Ordering::Acquire),"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(),
                    JSON,
                    POLICY,
                ),
                Err(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "History temporarily unavailable".into(),
                    PLAIN,
                    POLICY,
                ),
            }
        }
        "/api/instance" => (
            StatusCode::OK,
            json!({"application":"local-activity-monitor","home_id":state.home_id})
                .to_string()
                .into(),
            JSON,
            POLICY,
        ),
        "/api/snapshot" => match Snapshots::snapshot_window(query) {
            Some(index) => (
                StatusCode::OK,
                state.snapshots.borrow().window_bytes(index),
                JSON,
                POLICY,
            ),
            None => (
                StatusCode::BAD_REQUEST,
                "Invalid window".into(),
                PLAIN,
                POLICY,
            ),
        },
        "/api/activity" if query.is_empty() => (
            StatusCode::OK,
            state.snapshots.borrow().activity.clone(),
            JSON,
            POLICY,
        ),
        "/api/logs" => match Snapshots::window_or(query, 3) {
            Some(index) => (
                StatusCode::OK,
                state.snapshots.borrow().logs[index].clone(),
                JSON,
                POLICY,
            ),
            None => (
                StatusCode::BAD_REQUEST,
                "Invalid log query".into(),
                PLAIN,
                POLICY,
            ),
        },
        "/api/history" => match {
            let sources = state
                .mcp_sources
                .read()
                .unwrap_or_else(|error| error.into_inner());
            let excluded: Vec<&str> = sources
                .iter()
                .filter_map(|(name, enabled)| (!enabled).then_some(name.as_str()))
                .collect();
            let switches = *state
                .mcp_switches
                .read()
                .unwrap_or_else(|error| error.into_inner());
            history::read(
                &state.monitoring,
                query,
                state.codex.load(std::sync::atomic::Ordering::Acquire),
                &excluded,
                switches,
                state
                    .errors_enabled
                    .load(std::sync::atomic::Ordering::Acquire),
                state
                    .sqlite_enabled
                    .load(std::sync::atomic::Ordering::Acquire),
                state
                    .command_switches
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .skills,
            )
        } {
            Ok(page) => match serde_json::to_vec(&page) {
                Ok(body) => (StatusCode::OK, body.into(), JSON, POLICY),
                Err(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "History temporarily unavailable".into(),
                    PLAIN,
                    POLICY,
                ),
            },
            Err(HistoryError::Invalid) => (
                StatusCode::BAD_REQUEST,
                "Invalid history query".into(),
                PLAIN,
                POLICY,
            ),
            Err(HistoryError::Unavailable) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "History temporarily unavailable".into(),
                PLAIN,
                POLICY,
            ),
        },
        _ => match state
            .assets
            .as_ref()
            .and_then(|assets| assets.get(path).map(|asset| (assets, asset)))
        {
            Some((assets, (body, mime))) => (
                StatusCode::OK,
                body,
                mime,
                if path == "/" {
                    &assets.page_policy
                } else {
                    POLICY
                },
            ),
            None => (StatusCode::NOT_FOUND, "Not found".into(), PLAIN, POLICY),
        },
    };
    reply(status, body, mime, policy, gzip, false)
}

fn reply(
    status: StatusCode,
    mut body: Bytes,
    mime: &'static str,
    policy: &str,
    gzip: bool,
    close: bool,
) -> Response<Body> {
    let plain = body.len();
    let mut headers = HeaderMap::new();
    if gzip && body.len() >= 1024 {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        if let Ok(encoded) = encoder.write_all(&body).and_then(|()| encoder.finish()) {
            body = encoded.into();
            headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        }
    }
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_str(policy).expect("generated CSP is ASCII"),
    );
    if close {
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    let transferred = body.len();
    let mut response = Response::new(Full::new(body).boxed_unsync());
    response
        .extensions_mut()
        .insert(ResponseSizes { plain, transferred });
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_batch_response_uses_applied_state_when_snapshot_is_stale() {
        let home = tempfile::tempdir().unwrap();
        let (monitor, snapshots) =
            crate::sessions::Monitor::start(home.path(), false, None, 20).unwrap();
        let (cleanup_wake, _cleanup_signal) = std::sync::mpsc::sync_channel(1);
        let state = State {
            home_id: "fixture".into(),
            monitoring: home.path().join("monitoring"),
            codex: monitor.codex_setting(),
            traffic: monitor.traffic(),
            account: Arc::new(crate::sessions::account::Source::new(
                home.path().to_path_buf(),
                monitor.account_shared(),
            )),
            cloud: Arc::new(crate::sessions::cloud::Source::new(
                home.path().to_path_buf(),
                monitor.cloud_shared(),
            )),
            public_openai: Arc::new(
                crate::sessions::public_api::Source::new(
                    crate::sessions::public_api::Kind::Openai,
                    monitor.public_shared()[0].clone(),
                )
                .unwrap(),
            ),
            public_codex: Arc::new(
                crate::sessions::public_api::Source::new(
                    crate::sessions::public_api::Kind::Codex,
                    monitor.public_shared()[1].clone(),
                )
                .unwrap(),
            ),
            platform_sources: [
                crate::sessions::platform::Kind::Models,
                crate::sessions::platform::Kind::Usage,
                crate::sessions::platform::Kind::Decisions,
                crate::sessions::platform::Kind::Costs,
            ]
            .into_iter()
            .zip(monitor.platform_shared())
            .map(|(kind, shared)| {
                Arc::new(crate::sessions::platform::Source::new(kind, shared).unwrap())
            })
            .collect(),
            metadata_enabled: monitor.metadata_setting(),
            tool_events_enabled: monitor.tool_events_setting(),
            files_enabled: monitor.files_setting(),
            errors_enabled: monitor.errors_setting(),
            sqlite_enabled: monitor.sqlite_setting(),
            model_api_enabled: monitor.model_api_setting(),
            command_switches: monitor.command_switches_setting(),
            mcp_sources: monitor.mcp_sources_setting(),
            mcp_switches: monitor.mcp_switches_setting(),
            customizations: monitor.customizations_setting(),
            tracking: monitor.tracking_setting(),
            cleanup_wake,
            metadata_lock: tokio::sync::Mutex::new(()),
            journal: monitor.journal(),
            debug: monitor.debug(),
            details: monitor.detail_index(),
            core_source: monitor.core_source(),
            desktop_sources: monitor.desktop_sources(),
            assets: None,
            snapshots,
            refresh: monitor.refresh_sender(),
            event_clients: Arc::new(Semaphore::new(events::CLIENT_LIMIT)),
        };
        state
            .files_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        state
            .mcp_sources
            .write()
            .unwrap()
            .insert("fixture".into(), false);
        state
            .customizations
            .write()
            .unwrap()
            .mcp_descriptions
            .insert("fixture".into(), "Purpose".into());
        state.debug.lock().unwrap().set_enabled(true);
        let response = batch_settings_response(&state, false, Some(7));
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let bytes = runtime
            .block_on(response.into_body().collect())
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["activity_retention_days"], 7);
        assert_eq!(body["observations"]["files"], false);
        assert_eq!(body["mcp_sources"]["fixture"], false);
        assert_eq!(body["mcp_descriptions"]["fixture"], "Purpose");
        assert_eq!(body["debug_mode"], true);
        assert_eq!(body["refresh_pending"], true);
    }
}
