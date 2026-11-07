//! A real HTTPS transport backed by `ureq` (blocking) and `rustls` (ring).
//!
//! Certificates are checked with `rustls-platform-verifier`, so iOS and macOS
//! use the system trust store (and any profile-installed roots) rather than a
//! bundled root list. Android needs a one-time init first (see
//! [`crate::platform`]); until then `https` requests fail with
//! [`NetError::TlsNotInitialised`]. Verification is never turned off: there is
//! no "accept any certificate" switch, and a rejected certificate is reported
//! as [`NetError::Certificate`]. Proxy settings are read from the standard environment
//! variables (`HTTPS_PROXY`, `NO_PROXY`, ...).

use std::sync::Arc;
use std::time::Duration;

use crate::cancel::CancelHandle;
use crate::client::HttpClient;
use crate::types::{Header, Method, NetError, Request, Response, StreamingResponse};

/// Timeout applied when a [`Request`] does not set one.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Responses larger than this are refused (`Transport` error) rather than
/// buffered. Generous for JSON APIs, small enough to protect a phone.
pub const DEFAULT_MAX_BODY_BYTES: u64 = 8 * 1024 * 1024;

/// Blocking HTTPS client. Cheap to clone (it shares one connection pool), and
/// safe to share across threads.
#[derive(Debug, Clone)]
pub struct UreqClient {
    agent: ureq::Agent,
    max_body_bytes: u64,
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqClient {
    pub fn new() -> Self {
        Self::with_user_agent(concat!("forge-net/", env!("CARGO_PKG_VERSION")))
    }

    /// A client that sends `user_agent` on every request.
    pub fn with_user_agent(user_agent: &str) -> Self {
        Self::with_roots(user_agent, ureq::tls::RootCerts::PlatformVerifier)
    }

    /// Tests only: trust exactly `roots` (checked by webpki, not the
    /// platform), so local HTTPS servers with a throwaway CA can be used.
    #[cfg(test)]
    fn trusting_only(roots: &[&[u8]]) -> Self {
        let certs: Vec<_> = roots
            .iter()
            .map(|der| ureq::tls::Certificate::from_der(der).to_owned())
            .collect();
        Self::with_roots(
            "forge-net-test",
            ureq::tls::RootCerts::new_with_certs(&certs),
        )
    }

    fn with_roots(user_agent: &str, roots: ureq::tls::RootCerts) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(roots)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            // A 404 is a Response, not an error: see `Response` docs.
            .http_status_as_error(false)
            .user_agent(user_agent)
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .build()
            .new_agent();
        Self {
            agent,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    /// Caps the response body size (default [`DEFAULT_MAX_BODY_BYTES`]).
    pub fn max_body_bytes(mut self, limit: u64) -> Self {
        self.max_body_bytes = limit;
        self
    }
}

impl UreqClient {
    /// Sends `request` and returns once the status and headers arrived.
    ///
    /// Buffered calls bound the whole exchange (body included) by the
    /// request timeout. Streaming calls bound only connecting and waiting for
    /// the headers, so a Server-Sent Events body can stay open.
    fn run(
        &self,
        agent: &ureq::Agent,
        request: &Request,
        streaming: bool,
    ) -> Result<ureq::http::Response<ureq::Body>, NetError> {
        request.validate()?;
        crate::platform::ensure_ready_for(&request.url)?;

        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for header in &request.headers {
            builder = builder.header(header.name.as_str(), header.value.as_str());
        }
        let invalid = |e: ureq::http::Error| NetError::InvalidRequest {
            reason: e.to_string(),
        };
        let timeout = Some(request.timeout.unwrap_or(DEFAULT_TIMEOUT));
        // A macro, not a fn: ureq's config builder is generic over a sealed
        // scope trait that cannot be named here.
        macro_rules! configure {
            ($config:expr) => {
                if streaming {
                    $config
                        .timeout_global(None)
                        .timeout_connect(timeout)
                        .timeout_recv_response(timeout)
                        .timeout_recv_body(None)
                } else {
                    $config.timeout_global(timeout)
                }
            };
        }

        let result = if request.body.is_empty()
            && !matches!(request.method, Method::Post | Method::Put | Method::Patch)
        {
            let http_request = builder.body(()).map_err(invalid)?;
            let http_request = configure!(agent.configure_request(http_request)).build();
            agent.run(http_request)
        } else {
            let http_request = builder.body(request.body.as_slice()).map_err(invalid)?;
            let http_request = configure!(agent.configure_request(http_request)).build();
            agent.run(http_request)
        };
        result.map_err(map_error)
    }
}

fn response_headers(response: &ureq::http::Response<ureq::Body>) -> Vec<Header> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            Some(Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect()
}

impl UreqClient {
    fn read_response(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<Response, NetError> {
        let status = response.status().as_u16();
        let headers = response_headers(&response);
        let body = response
            .body_mut()
            .with_config()
            .limit(self.max_body_bytes)
            .read_to_vec()
            .map_err(map_error)?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

impl HttpClient for UreqClient {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        let response = self.run(&self.agent, request, false)?;
        self.read_response(response)
    }

    /// Runs on a connection of its own, so cancelling shuts down exactly
    /// that socket: a request waiting for its response or reading the body
    /// fails at once with [`NetError::Cancelled`]. (Name resolution and the
    /// TCP connect itself are not interrupted; the request timeout bounds
    /// them, and the result is discarded.)
    fn send_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        // A private agent: `Agent::shutdown` closes every connection of the
        // agent's pool, and this pool only ever holds this request.
        let agent = ureq::Agent::new_with_config(self.agent.config().clone());
        let _registration = {
            let agent = agent.clone();
            cancel.on_cancel(move || agent.shutdown())
        };
        let result = self
            .run(&agent, request, false)
            .and_then(|response| self.read_response(response));
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        result
    }

    /// Streams the body. There is no size limit (the body may be endless) and
    /// no idle timeout: a reader blocked on a silent connection stays blocked
    /// until the server writes or closes, so streaming protocols should send
    /// heartbeats. Read errors surface as `std::io::Error`.
    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        let response = self.run(&self.agent, request, true)?;
        Ok(streaming_response(response))
    }

    /// Streams on a connection of its own, so cancelling shuts down exactly
    /// that socket: a read blocked on a silent server fails at once with
    /// [`crate::cancelled_io_error`], and a request still connecting or
    /// waiting for headers fails with [`NetError::Cancelled`]. (Name
    /// resolution and the TCP connect itself are not interrupted; the
    /// request timeout bounds them.)
    ///
    /// The connection does not come from or go back to the shared pool. Each
    /// call builds its own TLS state, which costs a little per (re)connect
    /// and nothing per event.
    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        // A private agent: `Agent::shutdown` closes every connection of the
        // agent's pool, and this pool only ever holds this stream.
        let agent = ureq::Agent::new_with_config(self.agent.config().clone());
        let registration = {
            let agent = agent.clone();
            cancel.on_cancel(move || agent.shutdown())
        };
        let response = match self.run(&agent, request, true) {
            Ok(response) => response,
            Err(_) if cancel.is_cancelled() => return Err(NetError::Cancelled),
            Err(err) => return Err(err),
        };
        Ok(streaming_response(response).cancel_on_with(cancel, Some(registration)))
    }
}

fn streaming_response(response: ureq::http::Response<ureq::Body>) -> StreamingResponse {
    let status = response.status().as_u16();
    let headers = response_headers(&response);
    let body = response.into_body().into_reader();
    StreamingResponse {
        status,
        headers,
        body: Box::new(body),
    }
}

/// Maps ureq's errors onto the small, stable [`NetError`] set. "Could not
/// reach the host at all" becomes [`NetError::Offline`] so apps can show an
/// offline state; everything else that is not a timeout is `Transport`.
fn map_error(error: ureq::Error) -> NetError {
    use std::io::ErrorKind;
    if let Some(reason) = certificate_problem(&error) {
        return NetError::Certificate { reason };
    }
    match error {
        ureq::Error::Timeout(_) => NetError::Timeout,
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => NetError::Offline {
            reason: error.to_string(),
        },
        ureq::Error::Io(ref io) => match io.kind() {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => NetError::Timeout,
            ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
            | ErrorKind::AddrNotAvailable
            | ErrorKind::NetworkUnreachable
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkDown => NetError::Offline {
                reason: error.to_string(),
            },
            _ => NetError::Transport {
                reason: error.to_string(),
            },
        },
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => NetError::InvalidRequest {
            reason: error.to_string(),
        },
        other => NetError::Transport {
            reason: other.to_string(),
        },
    }
}

/// The certificate failure inside `error`, if that is what it is. rustls
/// reports it as `InvalidCertificate`; ureq passes it on either directly or
/// wrapped in the `io::Error` of the handshake.
fn certificate_problem(error: &ureq::Error) -> Option<String> {
    let tls = match error {
        ureq::Error::Rustls(tls) => tls,
        ureq::Error::Io(io) => io.get_ref()?.downcast_ref::<rustls::Error>()?,
        _ => return None,
    };
    match tls {
        rustls::Error::InvalidCertificate(problem) => Some(describe_certificate(problem)),
        _ => None,
    }
}

fn describe_certificate(problem: &rustls::CertificateError) -> String {
    use rustls::CertificateError as C;
    let summary = match problem {
        C::Expired | C::ExpiredContext { .. } => "the certificate has expired",
        C::NotValidYet | C::NotValidYetContext { .. } => "the certificate is not valid yet",
        C::UnknownIssuer => "the certificate is not issued by an authority this device trusts",
        C::NotValidForName | C::NotValidForNameContext { .. } => {
            "the certificate is for a different host name"
        }
        C::Revoked => "the certificate has been revoked",
        C::BadEncoding => "the certificate is malformed",
        C::BadSignature => "the certificate signature is invalid",
        _ => "the certificate was rejected",
    };
    format!("{summary} ({problem:?})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    /// Serves exactly one HTTP/1.1 response on a local port and returns the
    /// URL plus a handle yielding the raw request it received.
    fn one_shot_server(response: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut raw = Vec::new();
            loop {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
                if n == 0 || raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&raw).into_owned()
        });
        (url, handle)
    }

    fn no_proxy() {
        // Local test servers must not go through a configured proxy.
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
    }

    #[test]
    fn get_returns_status_headers_and_body() {
        no_proxy();
        let (url, server) = one_shot_server(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
        );
        let response = UreqClient::new()
            .send(&Request::get(format!("{url}/v1/thing?x=1")).header("X-Test", "yes"))
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.body, b"{\"ok\":true}");
        let raw = server.join().unwrap();
        assert!(raw.starts_with("GET /v1/thing?x=1 HTTP/1.1"), "{raw}");
        assert!(raw.to_ascii_lowercase().contains("x-test: yes"), "{raw}");
    }

    #[test]
    fn non_2xx_is_a_response_not_an_error() {
        no_proxy();
        let (url, server) = one_shot_server(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy",
        );
        let response = UreqClient::new().send(&Request::get(url)).unwrap();
        assert_eq!(response.status, 503);
        assert!(!response.is_success());
        server.join().unwrap();
    }

    #[test]
    fn refused_connection_is_offline() {
        no_proxy();
        // Bind then drop to get a port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = UreqClient::new()
            .send(&Request::get(format!("http://127.0.0.1:{port}/")))
            .unwrap_err();
        assert!(matches!(err, NetError::Offline { .. }), "{err:?}");
    }

    #[test]
    fn slow_server_times_out() {
        no_proxy();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(800));
        });
        let err = UreqClient::new()
            .send(&Request::get(url).timeout(Duration::from_millis(200)))
            .unwrap_err();
        assert_eq!(err, NetError::Timeout);
        server.join().unwrap();
    }

    #[test]
    fn oversized_body_is_refused() {
        no_proxy();
        let (url, server) = one_shot_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let err = UreqClient::new()
            .max_body_bytes(4)
            .send(&Request::get(url))
            .unwrap_err();
        assert!(matches!(err, NetError::Transport { .. }), "{err:?}");
        server.join().unwrap();
    }

    #[test]
    fn open_stream_yields_chunks_before_the_body_ends() {
        use std::io::{BufRead, BufReader};
        use std::sync::mpsc;
        no_proxy();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stream", listener.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut raw = Vec::new();
            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: one\n\n")
                .unwrap();
            stream.flush().unwrap();
            // Hold the connection open until the client has seen the first event.
            release_rx.recv().unwrap();
            stream.write_all(b"data: two\n\n").unwrap();
        });

        let response = UreqClient::new()
            .open_stream(&Request::get(url).timeout(Duration::from_secs(5)))
            .unwrap();
        assert!(response.is_success());
        assert_eq!(response.header("content-type"), Some("text/event-stream"));
        let mut lines = BufReader::new(response.body).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "data: one");
        release_tx.send(()).unwrap();
        let rest: Vec<String> = lines.map(Result::unwrap).collect();
        assert_eq!(rest, vec!["", "data: two", ""]);
        server.join().unwrap();
    }

    /// A server that answers the stream's headers, then goes silent with the
    /// connection held open (no heartbeat) until `release` fires.
    fn silent_stream_server() -> (String, mpsc::Sender<()>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stream", listener.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut raw = Vec::new();
            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: one\n\n")
                .unwrap();
            stream.flush().unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(30));
        });
        (url, release_tx, server)
    }

    #[test]
    fn cancel_interrupts_a_read_blocked_on_a_silent_stream() {
        use std::io::{BufRead, BufReader};
        no_proxy();
        let (url, release, server) = silent_stream_server();
        let cancel = CancelHandle::new();
        let response = UreqClient::new()
            .open_stream_cancellable(&Request::get(url).timeout(Duration::from_secs(5)), &cancel)
            .unwrap();
        let mut lines = BufReader::new(response.body).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "data: one");
        assert_eq!(lines.next().unwrap().unwrap(), "");

        let reader = thread::spawn(move || {
            let started = Instant::now();
            let next = lines.next();
            (next, started.elapsed())
        });
        thread::sleep(Duration::from_millis(100)); // let it block in read
        cancel.cancel();
        let (next, blocked_for) = reader.join().unwrap();
        let err = next.expect("an error, not EOF").unwrap_err();
        assert!(crate::is_cancelled_io_error(&err), "{err:?}");
        // The server never wrote again: only the cancel can have woken it.
        assert!(blocked_for < Duration::from_secs(5), "{blocked_for:?}");
        assert_eq!(cancel.pending_hooks(), 0);
        release.send(()).ok();
        server.join().unwrap();
    }

    #[test]
    fn cancel_before_or_while_waiting_for_headers() {
        no_proxy();
        let cancelled = CancelHandle::new();
        cancelled.cancel();
        assert_eq!(
            UreqClient::new()
                .open_stream_cancellable(&Request::get("http://127.0.0.1:9/"), &cancelled)
                .unwrap_err(),
            NetError::Cancelled
        );

        // Accepts, never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stream", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_secs(2));
        });
        let cancel = CancelHandle::new();
        let canceller = {
            let cancel = cancel.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(150));
                cancel.cancel();
            })
        };
        let started = Instant::now();
        let err = UreqClient::new()
            .open_stream_cancellable(&Request::get(url).timeout(Duration::from_secs(30)), &cancel)
            .unwrap_err();
        assert_eq!(err, NetError::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        canceller.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn cancelling_one_stream_leaves_another_open() {
        use std::io::{BufRead, BufReader};
        no_proxy();
        let client = UreqClient::new();
        let (url_a, release_a, server_a) = silent_stream_server();
        let (url_b, release_b, server_b) = silent_stream_server();
        let (cancel_a, cancel_b) = (CancelHandle::new(), CancelHandle::new());
        let a = client
            .open_stream_cancellable(
                &Request::get(url_a).timeout(Duration::from_secs(5)),
                &cancel_a,
            )
            .unwrap();
        let b = client
            .open_stream_cancellable(
                &Request::get(url_b).timeout(Duration::from_secs(5)),
                &cancel_b,
            )
            .unwrap();
        cancel_a.cancel();
        let mut a = BufReader::new(a.body);
        let mut line = String::new();
        let err = a.read_line(&mut line).unwrap_err();
        assert!(crate::is_cancelled_io_error(&err), "{err:?}");
        // B still has its buffered event, then keeps waiting.
        let mut b = BufReader::new(b.body).lines();
        assert_eq!(b.next().unwrap().unwrap(), "data: one");
        assert!(!cancel_b.is_cancelled());
        release_a.send(()).ok();
        release_b.send(()).ok();
        server_a.join().unwrap();
        drop(b);
        server_b.join().unwrap();
        // The shared pool still works for ordinary requests.
        let refused = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(
            client.send(&Request::get(format!("http://127.0.0.1:{refused}/"))),
            Err(NetError::Offline { .. })
        ));
    }

    /// A server that accepts, reads the request, then sends `prefix` and
    /// goes silent until released.
    fn stalling_server(
        prefix: &'static [u8],
    ) -> (String, mpsc::Sender<()>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/slow", listener.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut raw = Vec::new();
            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
            }
            stream.write_all(prefix).unwrap();
            stream.flush().unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(30));
        });
        (url, release_tx, server)
    }

    /// Sends on another thread, cancels after 150 ms, and returns the result
    /// and how long the send took.
    fn send_then_cancel(url: String) -> (Result<Response, NetError>, Duration, CancelHandle) {
        let cancel = CancelHandle::new();
        let sender = {
            let cancel = cancel.clone();
            thread::spawn(move || {
                let started = Instant::now();
                let result = UreqClient::new()
                    .send_cancellable(&Request::get(url).timeout(Duration::from_secs(30)), &cancel);
                (result, started.elapsed())
            })
        };
        thread::sleep(Duration::from_millis(150));
        cancel.cancel();
        let (result, took) = sender.join().unwrap();
        (result, took, cancel)
    }

    #[test]
    fn cancel_interrupts_a_request_waiting_for_its_response() {
        no_proxy();
        let (url, release, server) = stalling_server(b"");
        let (result, took, cancel) = send_then_cancel(url);
        assert_eq!(result.unwrap_err(), NetError::Cancelled);
        // The timeout is 30 s and the server never answered: only the
        // cancel can have ended it.
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert_eq!(cancel.pending_hooks(), 0);
        release.send(()).ok();
        server.join().unwrap();
    }

    #[test]
    fn cancel_interrupts_a_request_reading_a_slow_body() {
        no_proxy();
        let (url, release, server) =
            stalling_server(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nonly part");
        let (result, took, _) = send_then_cancel(url);
        assert_eq!(result.unwrap_err(), NetError::Cancelled);
        assert!(took < Duration::from_secs(5), "{took:?}");
        release.send(()).ok();
        server.join().unwrap();
    }

    #[test]
    fn send_cancellable_without_a_cancel_behaves_like_send() {
        no_proxy();
        let (url, server) =
            one_shot_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        let cancel = CancelHandle::new();
        let response = UreqClient::new()
            .send_cancellable(&Request::get(url), &cancel)
            .unwrap();
        assert_eq!(
            (response.status, response.body.as_slice()),
            (200, &b"ok"[..])
        );
        assert_eq!(cancel.pending_hooks(), 0);
        server.join().unwrap();

        let cancelled = CancelHandle::new();
        cancelled.cancel();
        assert_eq!(
            UreqClient::new()
                .send_cancellable(&Request::get("http://127.0.0.1:9/"), &cancelled)
                .unwrap_err(),
            NetError::Cancelled
        );
    }

    #[test]
    fn open_stream_refused_is_offline() {
        no_proxy();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = UreqClient::new()
            .open_stream(&Request::get(format!("http://127.0.0.1:{port}/stream")))
            .unwrap_err();
        assert!(matches!(err, NetError::Offline { .. }), "{err:?}");
    }
}

/// HTTPS against local servers with throwaway certificates (rcgen), so the
/// suite needs no network and no public endpoints.
#[cfg(test)]
mod tls_tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    struct Ca {
        params: CertificateParams,
        key: KeyPair,
        der: Vec<u8>,
    }

    fn ca(name: &str) -> Ca {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name);
        let key = KeyPair::generate().unwrap();
        let der = params.clone().self_signed(&key).unwrap().der().to_vec();
        Ca { params, key, der }
    }

    /// A server certificate for `names`, signed by `ca` (or self-signed).
    fn leaf(
        names: &[&str],
        ca: Option<&Ca>,
        expired: bool,
    ) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        let mut params =
            CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>())
                .unwrap();
        if expired {
            params.not_before = rcgen::date_time_ymd(2019, 1, 1);
            params.not_after = rcgen::date_time_ymd(2020, 1, 1);
        }
        let key = KeyPair::generate().unwrap();
        let mut chain = Vec::new();
        match ca {
            Some(ca) => {
                let issuer = ca.params.clone().self_signed(&ca.key).unwrap();
                let cert = params.signed_by(&key, &issuer, &ca.key).unwrap();
                chain.push(cert.der().clone());
                chain.push(CertificateDer::from(ca.der.clone()));
            }
            None => chain.push(params.self_signed(&key).unwrap().der().clone()),
        }
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        (chain, key)
    }

    /// Serves one HTTPS exchange on 127.0.0.1 and returns `https://localhost:<port>`.
    /// The handshake may fail (that is what most tests want); the server
    /// then just ends.
    fn https_server(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> (String, thread::JoinHandle<()>) {
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut tls = rustls::StreamOwned::new(conn, tcp);
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                match tls.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
            }
            let _ = tls.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
            );
            let _ = tls.flush();
            tls.conn.send_close_notify();
            let _ = tls.flush();
        });
        (format!("https://localhost:{port}/"), handle)
    }

    fn no_proxy() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
    }

    fn get(client: &UreqClient, url: &str) -> Result<Response, NetError> {
        client.send(&Request::get(url).timeout(Duration::from_secs(5)))
    }

    #[test]
    fn valid_certificate_from_a_trusted_ca_works() {
        no_proxy();
        let ca = ca("Forge Test CA");
        let (chain, key) = leaf(&["localhost"], Some(&ca), false);
        let (url, server) = https_server(chain, key);
        let response = get(&UreqClient::trusting_only(&[&ca.der]), &url).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"secure");
        server.join().unwrap();
    }

    #[test]
    fn expired_certificate_is_a_certificate_error() {
        no_proxy();
        let ca = ca("Forge Test CA");
        let (chain, key) = leaf(&["localhost"], Some(&ca), true);
        let (url, server) = https_server(chain, key);
        let err = get(&UreqClient::trusting_only(&[&ca.der]), &url).unwrap_err();
        match &err {
            NetError::Certificate { reason } => assert!(reason.contains("expired"), "{reason}"),
            other => panic!("expected Certificate, got {other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn certificate_for_another_host_is_a_certificate_error() {
        no_proxy();
        let ca = ca("Forge Test CA");
        let (chain, key) = leaf(&["chat.example"], Some(&ca), false);
        let (url, server) = https_server(chain, key);
        let err = get(&UreqClient::trusting_only(&[&ca.der]), &url).unwrap_err();
        match &err {
            NetError::Certificate { reason } => {
                assert!(reason.contains("different host"), "{reason}")
            }
            other => panic!("expected Certificate, got {other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn untrusted_ca_is_a_certificate_error() {
        no_proxy();
        let trusted = ca("Trusted CA");
        let other = ca("Some Other CA");
        let (chain, key) = leaf(&["localhost"], Some(&other), false);
        let (url, server) = https_server(chain, key);
        let err = get(&UreqClient::trusting_only(&[&trusted.der]), &url).unwrap_err();
        match &err {
            NetError::Certificate { reason } => {
                assert!(reason.contains("not issued by an authority"), "{reason}")
            }
            other => panic!("expected Certificate, got {other:?}"),
        }
        server.join().unwrap();
    }

    /// The default client (the platform verifier, as shipped) rejects a
    /// self-signed certificate: verification is on by default.
    #[test]
    fn default_client_rejects_a_self_signed_certificate() {
        no_proxy();
        let (chain, key) = leaf(&["localhost"], None, false);
        let (url, server) = https_server(chain, key);
        let err = get(&UreqClient::new(), &url).unwrap_err();
        assert!(matches!(err, NetError::Certificate { .. }), "{err:?}");
        server.join().unwrap();
    }

    #[test]
    fn refused_https_connection_is_offline_not_a_certificate_error() {
        no_proxy();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = get(&UreqClient::new(), &format!("https://localhost:{port}/")).unwrap_err();
        assert!(matches!(err, NetError::Offline { .. }), "{err:?}");
    }

    /// A server that is not speaking TLS at all is a transport failure.
    #[test]
    fn plain_http_server_on_an_https_url_is_a_transport_error() {
        no_proxy();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let _ = tcp.read(&mut buf);
            let _ = tcp.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
        });
        let err = get(&UreqClient::new(), &format!("https://localhost:{port}/")).unwrap_err();
        assert!(
            matches!(err, NetError::Transport { .. } | NetError::Offline { .. }),
            "{err:?}"
        );
        server.join().unwrap();
    }
}

/// Hits the public internet; run with
/// `cargo test -p forge-net --features ureq -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "needs network access"]
    fn https_round_trip() {
        let url = std::env::var("FORGE_NET_LIVE_URL")
            .unwrap_or_else(|_| "https://api.open-meteo.com/v1/forecast?latitude=0&longitude=0&current=temperature_2m".into());
        let response = UreqClient::new().send(&Request::get(url)).unwrap();
        assert!(response.is_success(), "status {}", response.status);
    }
}
