//! Chat over HTTPS with the shipped certificate verifier.
//!
//! The mock server speaks plain HTTP, so this test puts a small TLS front in
//! front of it, with a certificate from a throwaway CA. The client is the
//! real one (`HttpRepo::with_ureq`, i.e. `UreqClient` with
//! `rustls-platform-verifier`), not a test configuration: on Linux the
//! verifier reads the system trust store, which `SSL_CERT_FILE` points at the
//! test CA for this process only (each integration test file is its own
//! process). Commands, the SSE stream and its cancellation, and a rejected
//! certificate are all checked over TLS.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chat_core::{ChatClient, ChatConfig, ChatError, ChatEvent, ChatStore, HttpRepo, MessageStatus};
use mock_chat_server::{router, AppState};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

const WAIT: Duration = Duration::from_secs(10);

struct Pki {
    /// Signed by the CA in `SSL_CERT_FILE`.
    trusted: Arc<rustls::ServerConfig>,
    /// Self-signed: no trusted CA issued it.
    untrusted: Arc<rustls::ServerConfig>,
}

/// Writes the test CA, points the trust store at it, and builds the two
/// server configs. Runs once per process, before any client exists.
fn pki() -> &'static Pki {
    static PKI: OnceLock<Pki> = OnceLock::new();
    PKI.get_or_init(|| {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Forge HTTPS test CA");
        let ca_key = KeyPair::generate().unwrap();
        let ca = ca_params.self_signed(&ca_key).unwrap();

        let dir = std::env::temp_dir().join(format!("forge-https-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ca_file = dir.join("ca.pem");
        std::fs::write(&ca_file, ca.pem()).unwrap();
        std::env::set_var("SSL_CERT_FILE", &ca_file);
        std::env::remove_var("SSL_CERT_DIR");
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");

        let leaf = |signed: bool| {
            let params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
            let key = KeyPair::generate().unwrap();
            let cert = if signed {
                params.signed_by(&key, &ca, &ca_key).unwrap()
            } else {
                params.self_signed(&key).unwrap()
            };
            let chain = vec![cert.der().clone(), CertificateDer::from(ca.der().to_vec())];
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
            Arc::new(
                rustls::ServerConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .unwrap(),
            )
        };
        Pki {
            trusted: leaf(true),
            untrusted: leaf(false),
        }
    })
}

/// The mock server on its own runtime (plain HTTP on 127.0.0.1).
struct Server {
    runtime: Option<tokio::runtime::Runtime>,
    addr: SocketAddr,
}

impl Server {
    fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let state = AppState::new(Duration::from_millis(100));
        runtime.spawn(async move { axum::serve(listener, router(state)).await });
        Self {
            runtime: Some(runtime),
            addr,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

/// Terminates TLS on a local port and forwards plain bytes to `backend`.
/// Returns `https://localhost:<port>`.
fn tls_front(config: Arc<rustls::ServerConfig>, backend: SocketAddr) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for tcp in listener.incoming() {
            let Ok(tcp) = tcp else { return };
            let config = Arc::clone(&config);
            thread::spawn(move || {
                let _ = forward(config, tcp, backend);
            });
        }
    });
    format!("https://localhost:{port}")
}

/// Pumps bytes both ways with short read timeouts until either side closes.
fn forward(
    config: Arc<rustls::ServerConfig>,
    tcp: TcpStream,
    backend: SocketAddr,
) -> std::io::Result<()> {
    let poll = Some(Duration::from_millis(5));
    tcp.set_read_timeout(poll)?;
    let mut tls = rustls::StreamOwned::new(
        rustls::ServerConnection::new(config).map_err(std::io::Error::other)?,
        tcp,
    );
    let mut plain = TcpStream::connect(backend)?;
    plain.set_read_timeout(poll)?;
    let mut buf = [0u8; 16 * 1024];
    let idle = |e: &std::io::Error| matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut);
    loop {
        match tls.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => plain.write_all(&buf[..n])?,
            Err(e) if idle(&e) => {}
            Err(e) => return Err(e),
        }
        match plain.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                tls.write_all(&buf[..n])?;
                tls.flush()?;
            }
            Err(e) if idle(&e) => {}
            Err(e) => return Err(e),
        }
    }
}

fn client(base_url: &str) -> (ChatClient, Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    let repo = HttpRepo::with_ureq(base_url).timeout(Duration::from_secs(5));
    let config = ChatConfig {
        reconnect_initial: Duration::from_millis(20),
        reconnect_max: Duration::from_millis(100),
        ..ChatConfig::default()
    };
    let client = ChatClient::open(Arc::new(repo), ChatStore::in_memory(), config, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (client, rx)
}

fn wait_for(rx: &Receiver<ChatEvent>, what: &str, pred: impl Fn(&ChatEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(event) if pred(&event) => return,
            Ok(event) => seen.push(event),
            Err(_) => panic!("timed out waiting for {what}; saw {seen:#?}"),
        }
    }
}

#[test]
fn chat_commands_and_stream_over_verified_https() {
    let pki = pki();
    let server = Server::start();
    let url = tls_front(Arc::clone(&pki.trusted), server.addr);
    let (me, rx) = client(&url);
    let (other, _) = client(&url);

    let rooms = me.list_conversations().unwrap();
    assert!(rooms.iter().any(|r| r.id == "general"), "{rooms:?}");

    me.subscribe("general").unwrap();
    wait_for(
        &rx,
        "stream connect over TLS",
        |e| matches!(e, ChatEvent::StreamConnected { conversation_id } if conversation_id == "general"),
    );

    let sent = other.send_message("general", "over tls").unwrap();
    assert_eq!(sent.status, MessageStatus::Sent);
    wait_for(
        &rx,
        "the message over the TLS stream",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == sent.id),
    );

    // Unsubscribing closes the TLS stream at once (M13 cancellation).
    me.unsubscribe("general").unwrap();
    let deadline = Instant::now() + WAIT;
    while me.live_pumps() > 0 {
        assert!(Instant::now() < deadline, "stream thread did not end");
        thread::sleep(Duration::from_millis(10));
    }
    me.shutdown();
    other.shutdown();
}

#[test]
fn untrusted_certificate_is_a_typed_error_not_a_network_blip() {
    let pki = pki();
    let server = Server::start();
    let url = tls_front(Arc::clone(&pki.untrusted), server.addr);
    let (me, _) = client(&url);
    match me.list_conversations() {
        Err(ChatError::Certificate { reason }) => {
            assert!(reason.contains("not issued by an authority"), "{reason}")
        }
        other => panic!("expected a certificate error, got {other:?}"),
    }
    match me.send_message("general", "never sent") {
        Err(ChatError::Certificate { .. }) => {}
        other => panic!("expected a certificate error, got {other:?}"),
    }
    me.shutdown();
}
