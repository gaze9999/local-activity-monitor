//! Track completed HTTP/1 chunk writes without retaining event contents.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

struct Pending {
    length: usize,
    started: Instant,
    completed: oneshot::Sender<Completion>,
}
pub(crate) struct Completion {
    pub duration: Duration,
    pub process_cpu_ms: Option<f64>,
}
enum State {
    Headers {
        matched: usize,
        bytes: usize,
    },
    Size {
        value: usize,
        digits: usize,
        cr: bool,
    },
    Body {
        remaining: usize,
    },
    Trailer {
        matched: usize,
    },
    Failed,
}
pub(crate) struct Tracker {
    state: State,
    pending: VecDeque<Pending>,
}
impl Tracker {
    pub fn new() -> Self {
        Self {
            state: State::Headers {
                matched: 0,
                bytes: 0,
            },
            pending: VecDeque::new(),
        }
    }
    pub fn register(&mut self, length: usize) -> oneshot::Receiver<Completion> {
        let (sender, receiver) = oneshot::channel();
        if !matches!(self.state, State::Failed) {
            self.pending.push_back(Pending {
                length,
                started: Instant::now(),
                completed: sender,
            });
        }
        receiver
    }
    fn fail(&mut self) {
        self.state = State::Failed;
        self.pending.clear();
    }
    pub fn abort(&mut self) {
        self.fail();
    }
    pub fn written(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match &mut self.state {
                State::Headers {
                    matched,
                    bytes: count,
                } => {
                    let byte = bytes[0];
                    bytes = &bytes[1..];
                    *count += 1;
                    if *count > 32768 {
                        self.fail();
                        continue;
                    }
                    let marker = b"\r\n\r\n";
                    *matched = if byte == marker[*matched] {
                        *matched + 1
                    } else if byte == marker[0] {
                        1
                    } else {
                        0
                    };
                    if *matched == marker.len() {
                        self.state = State::Size {
                            value: 0,
                            digits: 0,
                            cr: false,
                        };
                    }
                }
                State::Size { value, digits, cr } => {
                    let byte = bytes[0];
                    bytes = &bytes[1..];
                    if *cr {
                        if byte != b'\n' || *digits == 0 {
                            self.fail();
                            continue;
                        }
                        let size = *value;
                        if size == 0 {
                            self.fail();
                            continue;
                        }
                        if self
                            .pending
                            .front()
                            .is_none_or(|frame| frame.length != size)
                        {
                            self.fail();
                            continue;
                        }
                        self.state = State::Body { remaining: size };
                    } else if byte == b'\r' {
                        *cr = true;
                    } else if let Some(digit) = (byte as char).to_digit(16) {
                        if *digits >= 16 {
                            self.fail();
                            continue;
                        }
                        let Some(next) = value
                            .checked_mul(16)
                            .and_then(|value| value.checked_add(digit as usize))
                        else {
                            self.fail();
                            continue;
                        };
                        *value = next;
                        *digits += 1;
                    } else {
                        self.fail();
                    }
                }
                State::Body { remaining } => {
                    let consumed = bytes.len().min(*remaining);
                    bytes = &bytes[consumed..];
                    *remaining -= consumed;
                    if *remaining == 0 {
                        self.state = State::Trailer { matched: 0 };
                    }
                }
                State::Trailer { matched } => {
                    let byte = bytes[0];
                    bytes = &bytes[1..];
                    if byte != b"\r\n"[*matched] {
                        self.fail();
                        continue;
                    }
                    *matched += 1;
                    if *matched == 2 {
                        if let Some(frame) = self.pending.pop_front() {
                            if !frame.completed.is_closed() {
                                let _ = frame.completed.send(Completion {
                                    duration: frame.started.elapsed(),
                                    process_cpu_ms: crate::performance_debug::process_cpu_ms(),
                                });
                            }
                        }
                        self.state = State::Size {
                            value: 0,
                            digits: 0,
                            cr: false,
                        };
                    }
                }
                State::Failed => return,
            }
        }
    }
}

pub(crate) struct TrackedIo<T> {
    inner: std::pin::Pin<Box<T>>,
    tracker: std::sync::Arc<std::sync::Mutex<Option<Tracker>>>,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl<T> TrackedIo<T> {
    pub fn new(
        inner: T,
        tracker: std::sync::Arc<std::sync::Mutex<Option<Tracker>>>,
        active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            tracker,
            active,
        }
    }
}
impl<T: tokio::io::AsyncRead> tokio::io::AsyncRead for TrackedIo<T> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buffer)
    }
}
impl<T: tokio::io::AsyncWrite> tokio::io::AsyncWrite for TrackedIo<T> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = self.inner.as_mut().poll_write(cx, buffer);
        if let std::task::Poll::Ready(Ok(written)) = &result {
            if !self.active.load(std::sync::atomic::Ordering::Acquire) {
                return result;
            }
            if let Some(tracker) = self
                .tracker
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_mut()
            {
                tracker.written(&buffer[..*written]);
            }
        }
        result
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_shutdown(cx)
    }
}
impl<T> Drop for TrackedIo<T> {
    fn drop(&mut self) {
        if let Some(tracker) = self
            .tracker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
        {
            tracker.abort();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_writes_complete_only_after_chunk_trailer() {
        let mut tracker = Tracker::new();
        let mut done = tracker.register(5);
        tracker.written(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r");
        assert!(done.try_recv().is_err());
        tracker.written(b"\n5\r\nhel");
        assert!(done.try_recv().is_err());
        tracker.written(b"lo\r");
        assert!(done.try_recv().is_err());
        tracker.written(b"\n");
        assert!(done.try_recv().is_ok());
    }
    #[test]
    fn coalesced_chunks_follow_registered_order() {
        let mut tracker = Tracker::new();
        let mut first = tracker.register(3);
        let mut second = tracker.register(4);
        tracker.written(b"HTTP/1.1 200 OK\r\n\r\n3\r\none\r\n4\r\ntwo!");
        assert!(first.try_recv().is_ok());
        assert!(second.try_recv().is_err());
        tracker.written(b"\r\n");
        assert!(second.try_recv().is_ok());
    }
    #[test]
    fn malformed_or_unexpected_framing_drops_pending_receipts() {
        let mut tracker = Tracker::new();
        let mut done = tracker.register(5);
        tracker.written(b"HTTP/1.1 200 OK\r\n\r\n4\r\ntest\r\n");
        assert!(matches!(
            done.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
}
