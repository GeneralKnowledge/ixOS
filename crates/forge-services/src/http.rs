//! Ordinary HTTP requests through the host's networking stack: the Rust
//! side of the `http` service (`native_services!(http)`, feature `http`).
//!
//! On iOS the host sends each request with `URLSession`, so apps get the
//! system's proxy and VPN settings, HTTP/2 and HTTP/3, Low Data Mode and the
//! system trust store without Rust opening its own TLS connections.
//! [`HostClient`] implements `forge_net::HttpClient` over a [`HostHttp`], so
//! app code written against the trait does not change.
//!
//! Streaming bodies (`open_stream`, Server-Sent Events) are not sent through
//! the host yet: [`PreferInstalled`] passes them to its fallback client.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// `forge_net`, for code the `http` service's macro emits into the app.
pub use forge_net as net;

use forge_net::{CancelHandle, HttpClient, NetError, Request, Response, StreamingResponse};

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

/// The host's HTTP stack (iOS: `URLSession`). `send` blocks the calling
/// thread, which is always a Rust worker thread, never the UI.
pub trait HostHttp: Send + Sync {
    fn send(&self, request: HostRequest) -> Result<Response, NetError>;
    /// Stops request `id` if it is still running; its `send` then returns
    /// [`NetError::Cancelled`].
    fn cancel(&self, id: u64);
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
}

type Lookup = dyn Fn() -> Option<Arc<dyn HttpClient>> + Send + Sync;

/// Sends through the host's client once the host has installed it, and
/// through `fallback` before that and where the host has none (Linux tests,
/// Android). Looked up on every request, because an app's Rust core is
/// often created before the host installs its services.
///
/// Streams always use `fallback` for now.
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
        self.fallback.open_stream(request)
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        self.fallback.open_stream_cancellable(request, cancel)
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

        fn cancel(&self, id: u64) {
            self.cancelled.lock().unwrap().push(id);
            self.changed.notify_all();
        }
    }
}
