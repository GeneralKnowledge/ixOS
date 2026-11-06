use std::collections::HashMap;
use std::sync::Mutex;

use crate::cancel::CancelHandle;
use crate::types::{Method, NetError, Request, Response, StreamingResponse};

/// Narrow networking interface. A real transport (Rust HTTP library, or a
/// host `URLSession`/OkHttp adapter exposed through UniFFI) implements this.
///
/// Implementations must be `Send + Sync` so one client can be shared by an
/// app instance's command thread and anything else that needs it.
pub trait HttpClient: Send + Sync {
    /// Performs `request` and blocks until a response or error. Call it from
    /// a worker thread (e.g. `forge-ffi`'s command thread), never the UI.
    fn send(&self, request: &Request) -> Result<Response, NetError>;

    /// Performs `request` and returns once the status and headers have
    /// arrived, leaving the body to be read incrementally. Use it for
    /// long-lived bodies such as Server-Sent Events.
    ///
    /// The request's `timeout` covers connecting and receiving the headers
    /// only; the body may stay open indefinitely.
    ///
    /// The default implementation buffers the whole body with [`send`]
    /// first, which is right for test doubles (a canned SSE body is finite)
    /// but not for a real transport, which should override it.
    ///
    /// [`send`]: HttpClient::send
    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        self.send(request).map(StreamingResponse::from)
    }

    /// [`open_stream`](HttpClient::open_stream) that `cancel` can stop at
    /// any point: while connecting (the call fails with
    /// [`NetError::Cancelled`]) or while the body is being read (the read
    /// fails with [`crate::cancelled_io_error`], promptly, even if it was
    /// blocked waiting for a silent server).
    ///
    /// The default implementation checks `cancel` before connecting and
    /// before every body read, which is enough for test doubles whose bodies
    /// never block. A transport whose reads can block must override it and
    /// register an abort hook with [`CancelHandle::on_cancel`] that unblocks
    /// them (`UreqClient` shuts the socket down).
    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let response = self.open_stream(request)?;
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        Ok(response.cancel_on(cancel))
    }
}

impl<T: HttpClient + ?Sized> HttpClient for std::sync::Arc<T> {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        (**self).send(request)
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        (**self).open_stream(request)
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        (**self).open_stream_cancellable(request, cancel)
    }
}

impl<T: HttpClient + ?Sized> HttpClient for Box<T> {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        (**self).send(request)
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        (**self).open_stream(request)
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        (**self).open_stream_cancellable(request, cancel)
    }
}

/// The default client: validates the request, then refuses it with
/// [`NetError::Unavailable`]. Lets app code compile and handle the
/// "no network" path before a real transport exists.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopClient;

impl HttpClient for NoopClient {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        request.validate()?;
        Err(NetError::Unavailable)
    }
}

/// Test double: returns canned responses keyed by method and URL, and
/// records every request it receives. Unknown routes get a 404.
#[derive(Debug, Default)]
pub struct StaticClient {
    routes: HashMap<(Method, String), Result<Response, NetError>>,
    received: Mutex<Vec<Request>>,
}

impl StaticClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Responds to `method url` with `result`.
    pub fn route(
        mut self,
        method: Method,
        url: impl Into<String>,
        result: Result<Response, NetError>,
    ) -> Self {
        self.routes.insert((method, url.into()), result);
        self
    }

    /// Every request sent so far, in order.
    pub fn received(&self) -> Vec<Request> {
        self.received
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl HttpClient for StaticClient {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        request.validate()?;
        self.received
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        self.routes
            .get(&(request.method, request.url.clone()))
            .cloned()
            .unwrap_or_else(|| Ok(Response::new(404, Vec::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn noop_refuses_valid_requests() {
        let err = NoopClient
            .send(&Request::get("https://example.com/items"))
            .unwrap_err();
        assert_eq!(err, NetError::Unavailable);
    }

    #[test]
    fn invalid_urls_are_rejected_before_transport() {
        for url in [
            "",
            "example.com",
            "ftp://example.com",
            "https://",
            "http:///x",
        ] {
            let err = NoopClient.send(&Request::get(url)).unwrap_err();
            assert!(
                matches!(err, NetError::InvalidRequest { .. }),
                "{url:?}: {err:?}"
            );
        }
    }

    #[test]
    fn request_builder_keeps_order_and_fields() {
        let req = Request::post("https://api.example.com/v1", b"{}".to_vec())
            .header("Content-Type", "application/json")
            .header("X-Trace", "a")
            .header("X-Trace", "b")
            .timeout(Duration::from_secs(5));
        assert_eq!(req.method.as_str(), "POST");
        assert_eq!(req.body, b"{}");
        assert_eq!(req.headers.len(), 3);
        assert_eq!(req.headers[2].value, "b");
        assert_eq!(req.timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn static_client_routes_records_and_404s() {
        let client = StaticClient::new()
            .route(
                Method::Get,
                "https://example.com/ok",
                Ok(Response::new(200, "hi")),
            )
            .route(
                Method::Get,
                "https://example.com/slow",
                Err(NetError::Timeout),
            );

        let ok = client
            .send(&Request::get("https://example.com/ok"))
            .unwrap();
        assert!(ok.is_success());
        assert_eq!(ok.body, b"hi");

        assert_eq!(
            client.send(&Request::get("https://example.com/slow")),
            Err(NetError::Timeout)
        );
        let missing = client
            .send(&Request::new(Method::Delete, "https://example.com/ok"))
            .unwrap();
        assert_eq!(missing.status, 404);
        assert!(!missing.is_success());
        assert_eq!(client.received().len(), 3);
    }

    #[test]
    fn default_open_stream_buffers_send() {
        use std::io::Read;
        let client = StaticClient::new().route(
            Method::Get,
            "https://example.com/stream",
            Ok(Response::new(200, "data: hi\n\n")),
        );
        let mut stream = client
            .open_stream(&Request::get("https://example.com/stream"))
            .unwrap();
        assert!(stream.is_success());
        let mut body = String::new();
        stream.body.read_to_string(&mut body).unwrap();
        assert_eq!(body, "data: hi\n\n");
        assert_eq!(
            NoopClient
                .open_stream(&Request::get("https://example.com"))
                .unwrap_err(),
            NetError::Unavailable
        );
    }

    #[test]
    fn default_cancellable_stream_honours_cancel() {
        use std::io::Read;
        let client = StaticClient::new().route(
            Method::Get,
            "https://example.com/stream",
            Ok(Response::new(200, "data: hi\n\n")),
        );
        let request = Request::get("https://example.com/stream");

        let cancelled = CancelHandle::new();
        cancelled.cancel();
        assert_eq!(
            client
                .open_stream_cancellable(&request, &cancelled)
                .unwrap_err(),
            NetError::Cancelled
        );

        let cancel = CancelHandle::new();
        let mut stream = client.open_stream_cancellable(&request, &cancel).unwrap();
        let mut first = [0u8; 4];
        stream.body.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"data");
        cancel.cancel();
        let err = stream.body.read(&mut first).unwrap_err();
        assert!(crate::is_cancelled_io_error(&err), "{err:?}");
    }

    #[test]
    fn response_header_lookup_is_case_insensitive() {
        let mut res = Response::new(200, Vec::new());
        res.headers.push(crate::Header {
            name: "Content-Type".into(),
            value: "text/plain".into(),
        });
        assert_eq!(res.header("content-type"), Some("text/plain"));
        assert_eq!(res.header("etag"), None);
    }

    #[test]
    fn clients_work_behind_shared_trait_objects() {
        let shared: Arc<dyn HttpClient> = Arc::new(NoopClient);
        let boxed: Box<dyn HttpClient> = Box::new(shared.clone());
        assert_eq!(
            boxed.send(&Request::get("https://example.com")),
            Err(NetError::Unavailable)
        );
    }
}
