use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{Method, NetError, Request, Response};

/// Narrow networking interface. A real transport (Rust HTTP library, or a
/// host `URLSession`/OkHttp adapter exposed through UniFFI) implements this.
///
/// Implementations must be `Send + Sync` so one client can be shared by an
/// app instance's command thread and anything else that needs it.
pub trait HttpClient: Send + Sync {
    /// Performs `request` and blocks until a response or error. Call it from
    /// a worker thread (e.g. `forge-ffi`'s command thread), never the UI.
    fn send(&self, request: &Request) -> Result<Response, NetError>;
}

impl<T: HttpClient + ?Sized> HttpClient for std::sync::Arc<T> {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        (**self).send(request)
    }
}

impl<T: HttpClient + ?Sized> HttpClient for Box<T> {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        (**self).send(request)
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
