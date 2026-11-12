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

pub struct State {
    pub home_id: String,
    pub monitoring: PathBuf,
    pub codex: bool,
    pub metadata_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub metadata_lock: tokio::sync::Mutex<()>,
    pub journal: Arc<std::sync::Mutex<crate::journal::Journal>>,
    pub debug: Arc<std::sync::Mutex<crate::performance_debug::PerformanceDebug>>,
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
                    let service = service_fn(move |request| {
                        let state = Arc::clone(&state);
                        let hosts = Arc::clone(&hosts);
                        let shutdown = shutdown.clone();
                        async move {
                            let response = dispatch(request, Arc::clone(&state), &hosts, shutdown).await;
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
                        .serve_connection(TokioIo::new(Box::pin(stream)), service);
                    let _ = connection.await;
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
    if request.method() == Method::POST && request.uri() == "/api/diagnostics" {
        return frontend_diagnostic(request, state).await;
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
    if headers.contains_key(header::TRANSFER_ENCODING)
        || headers
            .get(header::CONTENT_LENGTH)
            .is_some_and(|value| value != "0")
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

async fn set_settings(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let value = match json_body(request, 262144).await {
        Ok(value) => value,
        Err(error) => return bad_body(error),
    };
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
        return reply(StatusCode::OK, json!({"debug_mode":enabled,"observations":{"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(), JSON, POLICY, false, false);
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
        return reply(StatusCode::OK, json!({"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"observations":{"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":logs}}).to_string().into(), JSON, POLICY, false, false);
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
            json!({"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"observations":{"metadata":metadata,"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}})
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
    let updated = tokio::task::spawn_blocking(move || {
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

fn route(state: &State, path: &str, query: &str, gzip: bool) -> Response<Body> {
    let (status, body, mime, policy) = match path {
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
                    json!({"activity_retention_days":days,"debug_mode":state.debug.lock().unwrap_or_else(|error| error.into_inner()).enabled(),"observations":{"metadata":state.metadata_enabled.load(std::sync::atomic::Ordering::Acquire),"logs":state.journal.lock().unwrap_or_else(|error| error.into_inner()).enabled()}}).to_string().into(),
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
        "/api/snapshot" => match Snapshots::window(query) {
            Some(index) => (
                StatusCode::OK,
                state.snapshots.borrow().windows[index].clone(),
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
        "/api/history" => match history::read(&state.monitoring, query, state.codex) {
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
    let mut response = Response::new(Full::new(body).boxed_unsync());
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}
