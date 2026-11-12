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
    pending_debug: Option<Bytes>,
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
) -> Response<Body> {
    let client = Client {
        snapshots,
        window,
        first: true,
        shutdown,
        _permit: permit,
        debug,
        opened: false,
        pending_debug: None,
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
        if let Some(frame) = client.pending_debug.take() {
            return Some((Ok::<_, Infallible>(Frame::data(frame)), client));
        }
        let (frame, heartbeat, serialization_ms) = if client.first {
            let began = std::time::Instant::now();
            client.first = false;
            let frame = client.snapshots.borrow_and_update().event(client.window);
            (frame, false, began.elapsed().as_secs_f64() * 1000.0)
        } else {
            let change = tokio::select! {
                _ = client.shutdown.changed() => return None,
                change = tokio::time::timeout(HEARTBEAT, client.snapshots.changed()) => change,
            };
            match change {
                Ok(Ok(())) => {
                    let began = std::time::Instant::now();
                    let frame = client.snapshots.borrow_and_update().event(client.window);
                    (frame, false, began.elapsed().as_secs_f64() * 1000.0)
                }
                Ok(Err(_)) => return None,
                Err(_) => (Bytes::from_static(b": heartbeat\n\n"), true, 0.0),
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
        let debug = Arc::clone(&client.debug);
        client.pending_debug = tokio::task::spawn_blocking(move || {
            debug
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .stream_frame(heartbeat, serialization_ms, snapshot_bytes)
                .and_then(|value| serde_json::to_string(&value).ok())
                .map(|value| Bytes::from(format!("event: debug\ndata: {value}\n\n")))
        })
        .await
        .ok()
        .flatten();
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
