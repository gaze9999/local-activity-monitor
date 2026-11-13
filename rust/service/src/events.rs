use bytes::Bytes;
use futures_util::stream;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;
use hyper::header::{self, HeaderValue};
use hyper::Response;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{watch, OwnedSemaphorePermit};

use crate::server::Body;
use crate::sessions::Snapshots;

pub const CLIENT_LIMIT: usize = 8;
const HEARTBEAT: Duration = Duration::from_secs(15);

struct Client {
    snapshots: watch::Receiver<Snapshots>,
    window: usize,
    first: bool,
    shutdown: watch::Receiver<bool>,
    _permit: OwnedSemaphorePermit,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
    opened: bool,
    pending: Option<Pending>,
    tracker: Arc<Mutex<Option<crate::sse_write::Tracker>>>,
    traffic: Arc<Mutex<crate::server::Traffic>>,
}

struct Pending {
    receipt: tokio::sync::oneshot::Receiver<crate::sse_write::Completion>,
    heartbeat: bool,
    serialization_ms: f64,
    snapshot_bytes: usize,
    transfer_bytes: usize,
    cpu_started: Option<f64>,
}

impl Drop for Client {
    fn drop(&mut self) {
        if !self.opened {
            return;
        }
        self.debug
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .connection(false);
    }
}

pub fn response(
    snapshots: watch::Receiver<Snapshots>,
    window: usize,
    permit: OwnedSemaphorePermit,
    shutdown: watch::Receiver<bool>,
    debug: Arc<Mutex<crate::performance_debug::PerformanceDebug>>,
    traffic: Arc<Mutex<crate::server::Traffic>>,
    tracker: Arc<Mutex<Option<crate::sse_write::Tracker>>>,
) -> Response<Body> {
    *tracker.lock().unwrap_or_else(|error| error.into_inner()) =
        Some(crate::sse_write::Tracker::new());
    let client = Client {
        snapshots,
        window,
        first: true,
        shutdown,
        _permit: permit,
        debug,
        opened: false,
        pending: None,
        tracker,
        traffic,
    };
    let frames = stream::unfold(client, |mut client| async move {
        if *client.shutdown.borrow() {
            return None;
        }
        if !client.opened {
            client
                .debug
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .connection(true);
            client.opened = true;
        }
        if let Some(pending) = client.pending.take() {
            let receipt = tokio::select! {
                _ = client.shutdown.changed() => return None,
                result = tokio::time::timeout(Duration::from_secs(15), pending.receipt) => result,
            };
            match receipt {
                Ok(Ok(completion)) => {
                    if !pending.heartbeat {
                        let mut traffic = client
                            .traffic
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        traffic.snapshot_bytes = pending.snapshot_bytes;
                        traffic.transfer_bytes = pending.transfer_bytes;
                        traffic.revision = traffic.revision.wrapping_add(1);
                    }
                    let debug = Arc::clone(&client.debug);
                    let frame = tokio::task::spawn_blocking(move || {
                        debug
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .stream_frame(
                                pending.heartbeat,
                                pending.serialization_ms,
                                pending.snapshot_bytes,
                                pending.transfer_bytes,
                                completion.duration.as_secs_f64() * 1000.0,
                                pending.cpu_started.zip(completion.process_cpu_ms).map(|(start, end)| (end - start).max(0.0)),
                            )
                            .and_then(|value| serde_json::to_string(&value).ok())
                            .map(|value| Bytes::from(format!("event: debug\ndata: {value}\n\n")))
                    })
                    .await
                    .ok()
                    .flatten();
                    if let Some(frame) = frame {
                        if let Some(tracker) = client
                            .tracker
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .as_mut()
                        {
                            let _ = tracker.register(frame.len());
                        }
                        return Some((Ok::<_, Infallible>(Frame::data(frame)), client));
                    }
                }
                _ => {
                    if let Some(tracker) = client
                        .tracker
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .as_mut()
                    {
                        tracker.abort();
                    }
                }
            }
        }
        let (frame, heartbeat, serialization_ms, cpu_started) = if client.first {
            let began = std::time::Instant::now();
            let cpu_started = crate::performance_debug::process_cpu_ms();
            client.first = false;
            let frame = client.snapshots.borrow_and_update().event(client.window);
            (frame, false, began.elapsed().as_secs_f64() * 1000.0, cpu_started)
        } else {
            let change = tokio::select! {
                _ = client.shutdown.changed() => return None,
                change = tokio::time::timeout(HEARTBEAT, client.snapshots.changed()) => change,
            };
            match change {
                Ok(Ok(())) => {
                    let began = std::time::Instant::now();
                    let cpu_started = crate::performance_debug::process_cpu_ms();
                    let frame = client.snapshots.borrow_and_update().event(client.window);
                    (frame, false, began.elapsed().as_secs_f64() * 1000.0, cpu_started)
                }
                Ok(Err(_)) => return None,
                Err(_) => (Bytes::from_static(b": heartbeat\n\n"), true, 0.0, crate::performance_debug::process_cpu_ms()),
            }
        };
        let snapshot_bytes = if heartbeat {
            0
        } else {
            frame
                .windows(7)
                .position(|window| window == b"\ndata: ")
                .map_or(0, |position| frame.len().saturating_sub(position + 7 + 2))
        };
        let receipt = {
            let mut tracker = client
                .tracker
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match tracker.as_mut() {
                Some(tracker) => tracker.register(frame.len()),
                None => {
                    let (_, receiver) = tokio::sync::oneshot::channel();
                    receiver
                }
            }
        };
        client.pending = Some(Pending {
            receipt,
            heartbeat,
            serialization_ms,
            snapshot_bytes,
            transfer_bytes: frame.len(),
            cpu_started,
        });
        Some((Ok::<_, Infallible>(Frame::data(frame)), client))
    });
    let mut response = Response::new(StreamBody::new(frames).boxed_unsync());
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
