//! Ordinary HTTP requests through the host's networking stack: the Rust
//! side of the `http` service (`native_services!(http)`, feature `http`).
//!
//! On iOS the host sends each request with `URLSession`, so apps get the
//! system's proxy and VPN settings, HTTP/2 and HTTP/3, Low Data Mode and the
//! system trust store without Rust opening its own TLS connections.
//! [`HostClient`] implements `forge_net::HttpClient` over a [`HostHttp`], so
//! app code written against the trait does not change.
//!
//! Streaming bodies (`open_stream`, Server-Sent Events) go through the host
//! too: it returns once the headers arrive and then feeds the body into a
//! [`StreamFeed`], which Rust reads as an ordinary `std::io::Read`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// `forge_net`, for code the `http` service's macro emits into the app.
pub use forge_net as net;

use forge_net::{
    cancelled_io_error, CancelHandle, Header, HttpClient, NetError, Request, Response,
    StreamingResponse,
};

/// A request is given up after this long unless it sets its own timeout
/// (the same default as `forge_net::UreqClient`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// Response bodies larger than this are refused unless the client sets
/// another limit (the same default as `forge_net::UreqClient`).
pub const DEFAULT_MAX_BODY_BYTES: u64 = 8 * 1024 * 1024;

/// One request as the host sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRequest {
    /// Names the request for [`HostHttp::cancel`].
    pub id: u64,
    pub request: Request,
    /// Send this file as the body instead of `request.body` (uploads).
    pub body_file: Option<PathBuf>,
    /// The host should stop and fail with [`NetError::Transport`] once the
    /// response body grows past this.
    pub max_body_bytes: u64,
}

/// The status and headers of a streamed response, which the host returns
/// before the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamHead {
    pub status: u16,
    pub headers: Vec<Header>,
}

enum Chunk {
    Data(Vec<u8>),
    End,
    Failed(NetError),
}

/// Where the host delivers a streamed body, in order, from any thread.
/// Delivering never blocks. After `finish` or `fail`, further calls are
/// ignored by the reader.
#[derive(Clone)]
pub struct StreamFeed {
    sender: mpsc::Sender<Chunk>,
}

impl StreamFeed {
    fn new() -> (Self, mpsc::Receiver<Chunk>) {
        let (sender, receiver) = mpsc::channel();
        (Self { sender }, receiver)
    }

    /// The next bytes of the body.
    pub fn push(&self, data: Vec<u8>) {
        if !data.is_empty() {
            let _ = self.sender.send(Chunk::Data(data));
        }
    }

    /// The server closed the body normally.
    pub fn finish(&self) {
        let _ = self.sender.send(Chunk::End);
    }

    /// The body stopped with an error ([`NetError::Cancelled`] when the
    /// request was cancelled).
    pub fn fail(&self, error: NetError) {
        let _ = self.sender.send(Chunk::Failed(error));
    }
}

/// The host's HTTP stack (iOS: `URLSession`). `send` blocks the calling
/// thread, which is always a Rust worker thread, never the UI.
pub trait HostHttp: Send + Sync {
    fn send(&self, request: HostRequest) -> Result<Response, NetError>;
    /// Starts a request whose body is read as it arrives (Server-Sent
    /// Events). Blocks until the status and headers arrive, then returns
    /// and keeps delivering the body into `feed` until it ends, fails or
    /// is cancelled. `request.request.timeout` covers waiting for the
    /// headers only; the body may stay open indefinitely.
    fn open_stream(&self, request: HostRequest, feed: StreamFeed) -> Result<StreamHead, NetError> {
        let _ = (request, feed);
        Err(NetError::Unavailable)
    }
    /// Stops request `id` if it is still running; its `send` then returns
    /// [`NetError::Cancelled`] (a stream's feed fails with it).
    fn cancel(&self, id: u64);
}

/// A streamed body as Rust reads it. Dropping it before the end cancels
/// the request.
struct HostBody {
    receiver: mpsc::Receiver<Chunk>,
    buffer: Vec<u8>,
    position: usize,
    done: bool,
    host: Arc<dyn HostHttp>,
    id: u64,
}

impl Read for HostBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.position == self.buffer.len() {
            if self.done || buf.is_empty() {
                return Ok(0);
            }
            match self.receiver.recv() {
                Ok(Chunk::Data(data)) => {
                    self.buffer = data;
                    self.position = 0;
                }
                Ok(Chunk::End) => {
                    self.done = true;
                    return Ok(0);
                }
                Ok(Chunk::Failed(NetError::Cancelled)) => {
                    self.done = true;
                    return Err(cancelled_io_error());
                }
                Ok(Chunk::Failed(error)) => {
                    self.done = true;
                    let kind = match error {
                        NetError::Timeout => std::io::ErrorKind::TimedOut,
                        _ => std::io::ErrorKind::ConnectionReset,
                    };
                    return Err(std::io::Error::new(kind, error));
                }
                // The host let go of the feed without ending the body.
                Err(_) => {
                    self.done = true;
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        NetError::Transport {
                            reason: "the stream ended without being closed".into(),
                        },
                    ));
                }
            }
        }
        let n = buf.len().min(self.buffer.len() - self.position);
        buf[..n].copy_from_slice(&self.buffer[self.position..self.position + n]);
        self.position += n;
        Ok(n)
    }
}

impl Drop for HostBody {
    fn drop(&mut self) {
        if !self.done {
            self.host.cancel(self.id);
        }
    }
}

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// `forge_net::HttpClient` over the host's HTTP stack.
#[derive(Clone)]
pub struct HostClient {
    host: Arc<dyn HostHttp>,
    timeout: Duration,
    max_body_bytes: u64,
}

impl HostClient {
    pub fn new(host: Arc<dyn HostHttp>) -> Self {
        Self {
            host,
            timeout: DEFAULT_TIMEOUT,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    /// The timeout for requests that don't set one.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Caps response bodies (default [`DEFAULT_MAX_BODY_BYTES`]).
    pub fn max_body_bytes(mut self, limit: u64) -> Self {
        self.max_body_bytes = limit;
        self
    }

    fn run(
        &self,
        request: &Request,
        body_file: Option<&Path>,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        request.validate()?;
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let mut request = request.clone();
        request.timeout = Some(request.timeout.unwrap_or(self.timeout));
        let host = Arc::clone(&self.host);
        let registration = cancel.on_cancel(move || host.cancel(id));
        let result = self.host.send(HostRequest {
            id,
            request,
            body_file: body_file.map(Path::to_path_buf),
            max_body_bytes: self.max_body_bytes,
        });
        drop(registration);
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let response = result?;
        // The host should have stopped already; never hand Rust more.
        if response.body.len() as u64 > self.max_body_bytes {
            return Err(NetError::Transport {
                reason: format!(
                    "the response body is larger than {} bytes",
                    self.max_body_bytes
                ),
            });
        }
        Ok(response)
    }
}

impl HostClient {
    fn stream(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        request.validate()?;
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let mut request = request.clone();
        request.timeout = Some(request.timeout.unwrap_or(self.timeout));
        let host = Arc::clone(&self.host);
        let registration = cancel.on_cancel(move || host.cancel(id));
        let (feed, receiver) = StreamFeed::new();
        let head = self.host.open_stream(
            HostRequest {
                id,
                request,
                body_file: None,
                // A stream may be endless.
                max_body_bytes: u64::MAX,
            },
            feed,
        );
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let head = head?;
        let body = HostBody {
            receiver,
            buffer: Vec::new(),
            position: 0,
            done: false,
            host: Arc::clone(&self.host),
            id,
        };
        Ok(StreamingResponse {
            status: head.status,
            headers: head.headers,
            body: Box::new(body),
        }
        .cancel_on_with(cancel, Some(registration)))
    }
}

impl HttpClient for HostClient {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        self.run(request, None, &CancelHandle::new())
    }

    fn send_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        self.run(request, None, cancel)
    }

    fn send_file(
        &self,
        request: &Request,
        path: &Path,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        if !path.is_file() {
            return Err(NetError::InvalidRequest {
                reason: format!("{}: not a file", path.display()),
            });
        }
        self.run(request, Some(path), cancel)
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        self.stream(request, &CancelHandle::new())
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        self.stream(request, cancel)
    }
}

type Lookup = dyn Fn() -> Option<Arc<dyn HttpClient>> + Send + Sync;

/// Sends through the host's client once the host has installed it, and
/// through `fallback` before that and where the host has none (Linux tests,
/// Android). Looked up on every request, because an app's Rust core is
/// often created before the host installs its services.
pub struct PreferInstalled {
    lookup: Box<Lookup>,
    fallback: Arc<dyn HttpClient>,
}

impl PreferInstalled {
    pub fn new(
        lookup: impl Fn() -> Option<Arc<dyn HttpClient>> + Send + Sync + 'static,
        fallback: Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            lookup: Box::new(lookup),
            fallback,
        }
    }

    fn client(&self) -> Arc<dyn HttpClient> {
        (self.lookup)().unwrap_or_else(|| Arc::clone(&self.fallback))
    }

    /// Whether requests go through the host now (rather than the
    /// fallback), so an app can say in its log which transport ran.
    pub fn uses_host(&self) -> bool {
        (self.lookup)().is_some()
    }
}

impl HttpClient for PreferInstalled {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        self.client().send(request)
    }

    fn send_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        self.client().send_cancellable(request, cancel)
    }

    fn send_file(
        &self,
        request: &Request,
        path: &Path,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        self.client().send_file(request, path, cancel)
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        self.client().open_stream(request)
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        self.client().open_stream_cancellable(request, cancel)
    }
}

/// A host for tests: it answers from a closure, and can block until
/// cancelled to test cancellation.
pub mod testing {
    use super::*;
    use std::sync::{Condvar, Mutex};

    type Answer = dyn Fn(&HostRequest) -> Result<Response, NetError> + Send + Sync;

    pub struct FakeHost {
        answer: Box<Answer>,
        /// Feeds of streams still open, by request id, for `stream_feed`.
        feeds: Mutex<Vec<(u64, StreamFeed)>>,
        /// When true, `send` waits until its request is cancelled.
        hang: bool,
        seen: Mutex<Vec<HostRequest>>,
        cancelled: Mutex<Vec<u64>>,
        changed: Condvar,
    }

    impl FakeHost {
        pub fn answering(
            answer: impl Fn(&HostRequest) -> Result<Response, NetError> + Send + Sync + 'static,
        ) -> Self {
            Self {
                answer: Box::new(answer),
                feeds: Mutex::new(Vec::new()),
                hang: false,
                seen: Mutex::new(Vec::new()),
                cancelled: Mutex::new(Vec::new()),
                changed: Condvar::new(),
            }
        }

        /// A host whose requests never finish until cancelled.
        pub fn hanging() -> Self {
            Self {
                hang: true,
                ..Self::answering(|_| Err(NetError::Unavailable))
            }
        }

        pub fn seen(&self) -> Vec<HostRequest> {
            self.seen.lock().unwrap().clone()
        }

        pub fn cancelled(&self) -> Vec<u64> {
            self.cancelled.lock().unwrap().clone()
        }

        /// The feed of the streamed request `id`, to deliver its body the
        /// way the host would. A stream answers with the status and
        /// headers of `answer`, and its body is whatever the test feeds.
        pub fn stream_feed(&self, id: u64) -> Option<StreamFeed> {
            let feeds = self.feeds.lock().unwrap();
            feeds.iter().find(|(i, _)| *i == id).map(|(_, f)| f.clone())
        }
    }

    impl HostHttp for FakeHost {
        fn send(&self, request: HostRequest) -> Result<Response, NetError> {
            self.seen.lock().unwrap().push(request.clone());
            if self.hang {
                let mut cancelled = self.cancelled.lock().unwrap();
                while !cancelled.contains(&request.id) {
                    cancelled = self.changed.wait(cancelled).unwrap();
                }
                return Err(NetError::Cancelled);
            }
            (self.answer)(&request)
        }

        fn open_stream(
            &self,
            request: HostRequest,
            feed: StreamFeed,
        ) -> Result<StreamHead, NetError> {
            self.seen.lock().unwrap().push(request.clone());
            let response = (self.answer)(&request)?;
            self.feeds.lock().unwrap().push((request.id, feed));
            Ok(StreamHead {
                status: response.status,
                headers: response.headers,
            })
        }

        fn cancel(&self, id: u64) {
            self.cancelled.lock().unwrap().push(id);
            // A cancelled stream's body fails, as URLSession reports it.
            if let Some(feed) = self.stream_feed(id) {
                feed.fail(NetError::Cancelled);
            }
            self.changed.notify_all();
        }
    }
}
