//! The mailbox (N3 step 3, examples/relay/docs/relay-mailbox.md): a note
//! for a device that is closed is left, sealed, in its mailbox; when the
//! device opens it collects the note, saves it once and leaves a receipt,
//! and the sender's delivery ends Done without the two ever meeting again.
//! A relay, a lookup server and a mailbox server run in process.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::mailbox_server::{start, ServeOptions};
use relay_core::{CodeSend, NearbyDevice, RelayCore, RelayServers, TransferStatus};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// Opens a device, waiting while a closing one still holds its folder.
fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match RelayCore::with_default_servers(
            dir.join(name).to_string_lossy().into_owned(),
            name.into(),
            RelayServers::default(),
        ) {
            Ok(core) => return core,
            Err(err) => {
                assert!(Instant::now() < deadline, "couldn't open {name}: {err}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn close(core: Arc<RelayCore>) {
    core.shutdown();
    let closing = Arc::downgrade(&core);
    drop(core);
    let deadline = Instant::now() + Duration::from_secs(45);
    while closing.strong_count() > 0 {
        assert!(Instant::now() < deadline, "Relay didn't close");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn sees(core: &RelayCore, other: &RelayCore) {
    let service = block_on(other.device_service()).unwrap();
    core.devices_nearby(vec![NearbyDevice {
        endpoint_id: service.endpoint_id,
        addresses: vec![format!("127.0.0.1:{}", service.port)],
    }]);
}

fn trust_each_other(dir: &Path, phone: &RelayCore, laptop: &RelayCore) {
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    laptop
        .send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(laptop.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = laptop.code_send_status()
    else {
        unreachable!()
    };
    block_on(phone.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id))
        .unwrap();
    eventually("the laptop's trust offer", || {
        laptop.trust_offer().is_some()
    });
    let on_phone = phone.trust_offer().unwrap();
    let on_laptop = laptop.trust_offer().unwrap();
    block_on(phone.trust_device(on_phone.endpoint_id)).unwrap();
    block_on(laptop.trust_device(on_laptop.endpoint_id)).unwrap();
}

/// What `core` has from the device called `from`, oldest first.
fn notes_from(core: &RelayCore, from: &str) -> Vec<String> {
    let Some(device) = core.devices().into_iter().find(|d| d.name == from) else {
        return Vec::new();
    };
    core.device_messages(&device.endpoint_id)
        .unwrap()
        .into_iter()
        .filter(|m| m.author_id == device.endpoint_id)
        .map(|m| m.body)
        .collect()
}

#[test]
fn a_note_for_a_closed_device_goes_through_the_mailbox_once_with_a_receipt() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lookup_url = rt.block_on(pkarr_relay());
    let mut relay = iroh_relay::server::RelayConfig::new((std::net::Ipv4Addr::LOCALHOST, 0));
    relay.tls = None;
    let mut config = iroh_relay::server::ServerConfig::default();
    config.relay = Some(relay);
    let relay_server = rt
        .block_on(iroh_relay::server::Server::spawn(config))
        .unwrap();
    let relay_url = format!("http://{}", relay_server.http_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mailbox = rt
        .block_on(start(&ServeOptions {
            dir: dir.path().join("mailbox"),
            port: 0,
            relay: Some(relay_url.parse().unwrap()),
            lookup: Some(lookup_url.parse().unwrap()),
        }))
        .unwrap();
    // It publishes its record (naming its relay) once it has connected
    // to the relay: devices find it by key through that.
    let record = format!("{lookup_url}/{}", z32(&mailbox.id()));
    eventually("the mailbox's record", || {
        rt.block_on(async { http_ok(&record).await })
    });
    let servers = RelayServers {
        relay_url: Some(relay_url),
        lookup_url: Some(lookup_url.clone()),
        mailbox: Some(mailbox.id().to_string()),
    };

    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    for core in [&phone, &laptop] {
        block_on(core.device_service()).unwrap();
        block_on(core.set_servers(servers.clone())).unwrap();
    }
    trust_each_other(dir.path(), &phone, &laptop);
    let laptop_id = phone
        .devices()
        .into_iter()
        .find(|d| d.name == "laptop")
        .unwrap()
        .endpoint_id;

    // They meet once directly: each Hello tells the other its mailbox.
    sees(&phone, &laptop);
    block_on(phone.send_note_as_older_build(laptop_id.clone(), "hello".into())).unwrap();
    eventually("the first note", || {
        notes_from(&laptop, "phone") == ["hello"]
    });
    // Having told the phone its pass, the laptop registers it, and the
    // phone the laptop's.
    eventually("both passes registered", || {
        mailbox.store.passes_of(&id_bytes(&laptop_id)) == 1
            && mailbox.store.passes_of(&id_bytes(&phone_id(&laptop))) == 1
    });

    // The laptop closes. A note for it is left in its mailbox.
    close(laptop);
    let away =
        block_on(phone.send_note_as_older_build(laptop_id.clone(), "while away".into())).unwrap();
    assert_eq!(away.status, TransferStatus::Waiting);
    eventually("the note in the mailbox", || {
        mailbox_has(&mailbox, &laptop_id)
    });

    // The laptop opens again and collects it: once, with a receipt.
    let laptop = device(dir.path(), "laptop");
    block_on(laptop.device_service()).unwrap();
    eventually("the note from the mailbox", || {
        notes_from(&laptop, "phone") == ["hello", "while away"]
    });
    eventually("the receipt", || {
        phone.deliver_waiting();
        phone
            .transfers()
            .iter()
            .find(|t| t.id == away.id)
            .is_some_and(|t| t.status == TransferStatus::Done)
    });
    // Nothing more arrives, and the mailbox is empty.
    laptop.deliver_waiting();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(notes_from(&laptop, "phone"), ["hello", "while away"]);
    assert!(!mailbox_has(&mailbox, &laptop_id));
    close(laptop);
    close(phone);
}

fn phone_id(laptop: &RelayCore) -> String {
    laptop
        .devices()
        .into_iter()
        .find(|d| d.name == "phone")
        .unwrap()
        .endpoint_id
}

fn id_bytes(id: &str) -> [u8; 32] {
    *id.parse::<iroh::PublicKey>().unwrap().as_bytes()
}

fn mailbox_has(mailbox: &relay_core::mailbox_server::Running, id: &str) -> bool {
    mailbox.store.waiting_for(&id_bytes(id)) > 0
}

/// A pkarr relay as Relay's lookup server runs one: signed records put
/// and got by key, in memory. (iroh's test server only takes them.)
async fn pkarr_relay() -> String {
    use axum::body::Bytes;
    use axum::extract::{Path as UrlPath, State};
    use axum::http::StatusCode;
    use std::collections::HashMap;
    use std::sync::Mutex;
    type Records = Arc<Mutex<HashMap<String, Bytes>>>;
    async fn put(
        State(records): State<Records>,
        UrlPath(key): UrlPath<String>,
        body: Bytes,
    ) -> StatusCode {
        records.lock().unwrap().insert(key, body);
        StatusCode::NO_CONTENT
    }
    async fn get(
        State(records): State<Records>,
        UrlPath(key): UrlPath<String>,
    ) -> Result<Bytes, StatusCode> {
        records
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or(StatusCode::NOT_FOUND)
    }
    let app = axum::Router::new()
        .route("/pkarr/{key}", axum::routing::put(put).get(get))
        .with_state(Records::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/pkarr")
}

/// An endpoint id as pkarr names it: z-base-32.
fn z32(id: &iroh::EndpointId) -> String {
    const ALPHABET: &[u8] = b"ybndrfg8ejkmcpqxot1uwisza345h769";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for byte in id.as_bytes() {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Whether `url` answers 200.
async fn http_ok(url: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let url: url::Url = url.parse().unwrap();
    let Ok(mut stream) =
        tokio::net::TcpStream::connect((url.host_str().unwrap(), url.port().unwrap())).await
    else {
        return false;
    };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        url.path()
    );
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer).await;
    answer.starts_with(b"HTTP/1.1 200")
}
