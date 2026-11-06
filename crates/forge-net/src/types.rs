use std::time::Duration;

use thiserror::Error;

use crate::cancel::{AbortRegistration, CancelHandle};

/// HTTP method. Kept to the methods apps commonly need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// A single header. A `Vec` keeps order and allows repeated names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// An outgoing request. Build one with [`Request::get`] / [`Request::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// Absolute `http://` or `https://` URL.
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    /// `None` leaves the timeout to the implementation.
    pub timeout: Option<Duration>,
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout: None,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self::new(Method::Post, url).body(body)
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Checks what every implementation would otherwise have to check: the
    /// URL is absolute and uses `http` or `https`.
    pub fn validate(&self) -> Result<(), NetError> {
        let rest = self
            .url
            .strip_prefix("https://")
            .or_else(|| self.url.strip_prefix("http://"));
        match rest {
            Some(host) if !host.is_empty() && !host.starts_with('/') => Ok(()),
            _ => Err(NetError::InvalidRequest {
                reason: format!("URL must be absolute http(s): {:?}", self.url),
            }),
        }
    }
}

/// A completed HTTP exchange. A non-2xx status is still a `Response`, not an
/// error: whether a 404 is a failure is the caller's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First header with this name, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

/// A response whose body is read incrementally, for long-lived bodies such
/// as Server-Sent Events. Returned by [`crate::HttpClient::open_stream`].
///
/// Reading from `body` blocks until bytes arrive, the server closes the
/// connection (`Ok(0)`), or the transport fails. Dropping it closes the
/// connection.
pub struct StreamingResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Box<dyn std::io::Read + Send>,
}

impl StreamingResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First header with this name, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

impl std::fmt::Debug for StreamingResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl StreamingResponse {
    /// Makes every read of `body` check `cancel` first, failing with a
    /// [`cancelled_io_error`] once it is cancelled. A read already blocked
    /// in the transport is *not* interrupted by this alone; transports that
    /// can block for long register an abort hook as well (see
    /// [`crate::HttpClient::open_stream_cancellable`]).
    pub fn cancel_on(self, cancel: &CancelHandle) -> Self {
        self.cancel_on_with(cancel, None)
    }

    /// Like [`cancel_on`](Self::cancel_on), keeping `registration` (the
    /// transport's abort hook) alive for as long as the body is.
    pub fn cancel_on_with(
        self,
        cancel: &CancelHandle,
        registration: Option<AbortRegistration>,
    ) -> Self {
        Self {
            body: Box::new(CancellableBody {
                inner: self.body,
                cancel: cancel.clone(),
                _registration: registration,
            }),
            ..self
        }
    }
}

/// The `std::io::Error` a cancelled stream read fails with. Its kind is
/// `ConnectionAborted` (not `Interrupted`, which `BufRead` helpers retry
/// forever) and it wraps [`NetError::Cancelled`].
pub fn cancelled_io_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::ConnectionAborted, NetError::Cancelled)
}

/// Whether `error` is a [`cancelled_io_error`].
pub fn is_cancelled_io_error(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<NetError>())
        .is_some_and(|e| *e == NetError::Cancelled)
}

struct CancellableBody {
    inner: Box<dyn std::io::Read + Send>,
    cancel: CancelHandle,
    _registration: Option<AbortRegistration>,
}

impl std::io::Read for CancellableBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(cancelled_io_error());
        }
        match self.inner.read(buf) {
            // The abort hook broke the read: report it as a cancellation,
            // not as whatever the socket said.
            Err(_) if self.cancel.is_cancelled() => Err(cancelled_io_error()),
            // Bytes or EOF that raced the cancel are still discarded.
            Ok(_) if self.cancel.is_cancelled() => Err(cancelled_io_error()),
            other => other,
        }
    }
}

impl From<Response> for StreamingResponse {
    /// A buffered response as a stream over its (already complete) body.
    fn from(response: Response) -> Self {
        Self {
            status: response.status,
            headers: response.headers,
            body: Box::new(std::io::Cursor::new(response.body)),
        }
    }
}

/// Why a request produced no [`Response`].
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum NetError {
    /// No transport is configured (the [`crate::NoopClient`] default).
    #[error("networking is not available in this build")]
    Unavailable,
    #[error("invalid request: {reason}")]
    InvalidRequest { reason: String },
    /// The host could not be reached at all: no route, DNS failure,
    /// connection refused. Apps usually present this as "offline".
    #[error("offline: {reason}")]
    Offline { reason: String },
    /// The server's TLS certificate was rejected: expired or not yet valid,
    /// issued by an authority the platform does not trust, issued for a
    /// different host name, or revoked. Retrying does not help; the server
    /// (or the URL) has to change.
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    /// `https` cannot be used yet because the platform certificate verifier
    /// is not set up. On Android the host must call `ForgeAndroid.init` with
    /// the app `Context` first (see [`crate::https_ready`]). This is a host
    /// integration bug, not a network condition.
    #[error("TLS is not initialised on this host: {reason}")]
    TlsNotInitialised { reason: String },
    /// Any other transport failure: a TLS protocol error, a broken body,
    /// and similar.
    #[error("transport error: {reason}")]
    Transport { reason: String },
    #[error("request timed out")]
    Timeout,
    #[error("request was cancelled")]
    Cancelled,
}
