//! A real HTTPS transport backed by `ureq` (blocking) and `rustls` (ring).
//!
//! Certificates are checked with `rustls-platform-verifier`, so iOS and macOS
//! use the system trust store (and any profile-installed roots) rather than a
//! bundled root list. Proxy settings are read from the standard environment
//! variables (`HTTPS_PROXY`, `NO_PROXY`, ...).

use std::sync::Arc;
use std::time::Duration;

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
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
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
        request: &Request,
        streaming: bool,
    ) -> Result<ureq::http::Response<ureq::Body>, NetError> {
        request.validate()?;

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
            let http_request = configure!(self.agent.configure_request(http_request)).build();
            self.agent.run(http_request)
        } else {
            let http_request = builder.body(request.body.as_slice()).map_err(invalid)?;
            let http_request = configure!(self.agent.configure_request(http_request)).build();
            self.agent.run(http_request)
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

impl HttpClient for UreqClient {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        let mut response = self.run(request, false)?;
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

    /// Streams the body. There is no size limit (the body may be endless) and
    /// no idle timeout: a reader blocked on a silent connection stays blocked
    /// until the server writes or closes, so streaming protocols should send
    /// heartbeats. Read errors surface as `std::io::Error`.
    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        let response = self.run(request, true)?;
        let status = response.status().as_u16();
        let headers = response_headers(&response);
        let body = response.into_body().into_reader();
        Ok(StreamingResponse {
            status,
            headers,
            body: Box::new(body),
        })
    }
}

/// Maps ureq's errors onto the small, stable [`NetError`] set. "Could not
/// reach the host at all" becomes [`NetError::Offline`] so apps can show an
/// offline state; everything else that is not a timeout is `Transport`.
fn map_error(error: ureq::Error) -> NetError {
    use std::io::ErrorKind;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

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
