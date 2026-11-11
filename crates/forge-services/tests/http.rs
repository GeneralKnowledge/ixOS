//! `native_services!(http)`: `forge_net::HttpClient` over the host's HTTP
//! stack, against fake hosts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, NetError, Request, Response, StaticClient};
use forge_services::http::testing::FakeHost;
use forge_services::http::{HostClient, PreferInstalled, DEFAULT_MAX_BODY_BYTES, DEFAULT_TIMEOUT};

uniffi::setup_scaffolding!("http_test");
forge_services::native_services!(http);

#[test]
fn a_request_reaches_the_host_with_a_timeout_and_its_response_comes_back() {
    let host = Arc::new(FakeHost::answering(|_| {
        Ok(Response::new(200, b"hello".to_vec()))
    }));
    let client = HostClient::new(host.clone());
    let response = client
        .send(
            &Request::post("https://example.com/a", b"body".to_vec())
                .header("Accept", "text/plain"),
        )
        .unwrap();
    assert_eq!(response.body, b"hello");
    let seen = host.seen();
    assert_eq!(seen[0].request.timeout, Some(DEFAULT_TIMEOUT));
    assert_eq!(seen[0].request.body, b"body");
    assert_eq!(seen[0].max_body_bytes, DEFAULT_MAX_BODY_BYTES);
    assert!(seen[0].body_file.is_none());
}

#[test]
fn invalid_urls_never_reach_the_host() {
    let host = Arc::new(FakeHost::answering(|_| Ok(Response::new(200, Vec::new()))));
    let client = HostClient::new(host.clone());
    assert!(matches!(
        client.send(&Request::get("file:///etc/passwd")),
        Err(NetError::InvalidRequest { .. })
    ));
    assert!(host.seen().is_empty());
}

#[test]
fn cancelling_stops_a_request_the_host_is_still_sending() {
    let host = Arc::new(FakeHost::hanging());
    let client = HostClient::new(host.clone());
    let cancel = CancelHandle::new();
    let canceller = cancel.clone();
    let worker = std::thread::spawn(move || {
        client.send_cancellable(&Request::get("https://example.com/slow"), &cancel)
    });
    while host.seen().is_empty() {
        std::thread::sleep(Duration::from_millis(5));
    }
    canceller.cancel();
    assert_eq!(worker.join().unwrap(), Err(NetError::Cancelled));
    assert_eq!(host.cancelled(), vec![host.seen()[0].id]);
}

#[test]
fn a_body_over_the_limit_is_refused_even_if_the_host_sends_it() {
    let host = Arc::new(FakeHost::answering(|_| Ok(Response::new(200, vec![0; 11]))));
    let client = HostClient::new(host).max_body_bytes(10);
    assert!(matches!(
        client.send(&Request::get("https://example.com/big")),
        Err(NetError::Transport { .. })
    ));
}

#[test]
fn an_upload_names_its_file_for_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("audio.m4a");
    std::fs::write(&file, b"bytes").unwrap();
    let host = Arc::new(FakeHost::answering(|_| Ok(Response::new(201, Vec::new()))));
    let client = HostClient::new(host.clone());
    client
        .send_file(
            &Request::new(forge_net::Method::Put, "https://example.com/up"),
            &file,
            &CancelHandle::new(),
        )
        .unwrap();
    assert_eq!(host.seen()[0].body_file.as_deref(), Some(file.as_path()));
    assert!(client
        .send_file(
            &Request::get("https://example.com/up"),
            &dir.path().join("missing"),
            &CancelHandle::new()
        )
        .is_err());
}

#[test]
fn prefer_installed_uses_the_fallback_until_the_host_installs() {
    let installed: Arc<Mutex<Option<Arc<dyn HttpClient>>>> = Arc::new(Mutex::new(None));
    let lookup = installed.clone();
    let fallback = Arc::new(StaticClient::new().route(
        forge_net::Method::Get,
        "https://example.com/",
        Ok(Response::new(200, b"ureq".to_vec())),
    ));
    let client = PreferInstalled::new(move || lookup.lock().unwrap().clone(), fallback);
    let request = Request::get("https://example.com/");
    assert_eq!(client.send(&request).unwrap().body, b"ureq");

    let host = Arc::new(FakeHost::answering(|_| {
        Ok(Response::new(200, b"urlsession".to_vec()))
    }));
    *installed.lock().unwrap() = Some(Arc::new(HostClient::new(host)));
    assert_eq!(client.send(&request).unwrap().body, b"urlsession");
    // Streams stay on the fallback for now.
    let stream = client.open_stream(&request).unwrap();
    assert_eq!(stream.status, 200);
}

/// The generated interface: the host's errors map to `NetError`, headers
/// and the method cross over, and `prefer_installed` finds the service.
#[test]
fn the_host_installs_the_service_and_its_errors_map_to_net_errors() {
    use native::http::*;

    struct Transport {
        seen: Mutex<Vec<HttpRequest>>,
    }
    impl HttpTransport for Transport {
        fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError> {
            self.seen.lock().unwrap().push(request.clone());
            if request.url.ends_with("/offline") {
                return Err(HttpTransportError::Offline {
                    reason: "no route".into(),
                });
            }
            Ok(HttpResponse {
                status: 200,
                headers: vec![HttpHeader {
                    name: "Content-Type".into(),
                    value: "text/plain".into(),
                }],
                body: b"ok".to_vec(),
            })
        }
        fn cancel(&self, _: u64) {}
    }

    let fallback = Arc::new(StaticClient::new());
    let client = HttpService::prefer_installed(fallback, |c| c.timeout(Duration::from_secs(5)));
    let transport = Arc::new(Transport {
        seen: Mutex::new(Vec::new()),
    });
    install_native_http(transport.clone());

    let response = client
        .send(&Request::new(forge_net::Method::Delete, "https://example.com/x").header("X-Id", "7"))
        .unwrap();
    assert_eq!(response.header("content-type"), Some("text/plain"));
    let sent = transport.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.method, "DELETE");
    assert_eq!(sent.timeout_ms, 5_000);
    assert_eq!(sent.headers[0].name, "X-Id");

    assert!(matches!(
        client.send(&Request::get("https://example.com/offline")),
        Err(NetError::Offline { .. })
    ));
    HttpService::uninstall();
}
