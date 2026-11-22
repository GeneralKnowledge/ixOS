//! N5 and N6: a device shares a small site, and only the trusted devices it
//! granted `Capability::Sites` can open it, over Relay alone. Two Relay
//! devices on one machine, over localhost.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{
    Capability, CodeSend, NearbyDevice, RelayCore, SiteRequest, SiteResponse, SiteSource,
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let core = RelayCore::with_default_servers(
        dir.join(name).to_string_lossy().into_owned(),
        name.into(),
        relay_core::RelayServers::default(),
    )
    .unwrap();
    core.receive_into(
        dir.join(name)
            .join("Received")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    core
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn id(core: &RelayCore) -> String {
    block_on(core.device_service()).unwrap().endpoint_id
}

/// `a` and `b` make one code transfer and trust each other.
fn pair(dir: &Path, a: &RelayCore, b: &RelayCore) {
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    a.send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(a.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = a.code_send_status()
    else {
        unreachable!()
    };
    block_on(b.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id)).unwrap();
    eventually("both offers", || {
        a.trust_offer().is_some() && b.trust_offer().is_some()
    });
    for core in [a, b] {
        let offer = core.trust_offer().unwrap();
        block_on(core.trust_device(offer.endpoint_id)).unwrap();
    }
}

/// Tells `core` where `other` is, as Bonjour would.
fn sees(core: &RelayCore, other: &RelayCore) {
    let service = block_on(other.device_service()).unwrap();
    core.devices_nearby(vec![NearbyDevice {
        endpoint_id: service.endpoint_id,
        addresses: vec![format!("127.0.0.1:{}", service.port)],
    }]);
}

fn get(path: &str) -> SiteRequest {
    SiteRequest {
        method: "GET".into(),
        path: path.into(),
        headers: vec![],
        body: vec![],
    }
}

fn fetch(
    viewer: &RelayCore,
    host: &str,
    path: &str,
) -> Result<SiteResponse, relay_core::RelayError> {
    block_on(viewer.fetch_site(host.to_string(), get(path)))
}

/// The site wouldn't open: the connection was refused, or (on one opened
/// before the change) the request was answered 403.
fn refused(viewer: &RelayCore, host: &str) -> bool {
    match fetch(viewer, host, "/") {
        Err(_) => true,
        Ok(page) => page.status == 403,
    }
}

fn offers_sites(viewer: &RelayCore, host: &str) -> bool {
    block_on(viewer.device_status(host.to_string()))
        .unwrap()
        .offers
        .iter()
        .any(|o| o == "sites")
}

/// A laptop sharing a folder, and a phone it trusts.
fn laptop_and_phone(dir: &Path) -> (Arc<RelayCore>, Arc<RelayCore>, String, String) {
    let laptop = device(dir, "laptop");
    let phone = device(dir, "phone");
    pair(dir, &laptop, &phone);
    sees(&phone, &laptop);
    sees(&laptop, &phone);
    let site = dir.join("site");
    std::fs::create_dir_all(site.join("notes")).unwrap();
    std::fs::write(site.join("index.html"), "<h1>Hello from the laptop</h1>").unwrap();
    std::fs::write(site.join("notes/today.txt"), "milk").unwrap();
    std::fs::write(dir.join("secret.txt"), "not shared").unwrap();
    block_on(laptop.share_site(SiteSource::Folder {
        path: site.to_string_lossy().into_owned(),
    }))
    .unwrap();
    let (l, p) = (id(&laptop), id(&phone));
    (laptop, phone, l, p)
}

#[test]
fn only_a_granted_device_sees_and_opens_the_site() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, laptop_id, phone_id) = laptop_and_phone(dir.path());

    // Trusted, but not granted: the site is neither offered nor opened.
    assert!(!offers_sites(&phone, &laptop_id));
    assert!(refused(&phone, &laptop_id));

    // Granted: offered, and it opens.
    block_on(laptop.set_grant(phone_id.clone(), Capability::Sites, true)).unwrap();
    assert_eq!(
        laptop
            .devices()
            .iter()
            .find(|d| d.endpoint_id == phone_id)
            .unwrap()
            .grants,
        [Capability::Sites]
    );
    assert!(offers_sites(&phone, &laptop_id));
    let page = fetch(&phone, &laptop_id, "/").unwrap();
    assert_eq!(page.status, 200);
    assert_eq!(page.body, b"<h1>Hello from the laptop</h1>");
    assert!(page
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.starts_with("text/html")));
    let note = fetch(&phone, &laptop_id, "/notes/today.txt").unwrap();
    assert_eq!(note.body, b"milk");
    // Nothing outside the folder.
    assert_eq!(
        fetch(&phone, &laptop_id, "/../secret.txt").unwrap().status,
        404
    );
    assert_eq!(
        fetch(&phone, &laptop_id, "/%2e%2e/secret.txt")
            .unwrap()
            .status,
        404
    );
    // Only reading: a folder site takes no uploads.
    let put = block_on(phone.fetch_site(
        laptop_id.clone(),
        SiteRequest {
            method: "PUT".into(),
            path: "/index.html".into(),
            headers: vec![],
            body: b"changed".to_vec(),
        },
    ))
    .unwrap();
    assert_eq!(put.status, 405);

    // The grant is the laptop's to give: the phone granting itself
    // something on its own side changes nothing here.
    block_on(laptop.set_grant(phone_id.clone(), Capability::Sites, false)).unwrap();
    block_on(phone.set_grant(laptop_id.clone(), Capability::Sites, true)).unwrap();
    assert!(refused(&phone, &laptop_id));
    assert!(!offers_sites(&phone, &laptop_id));

    // Stopping the site ends it, even for a granted device.
    block_on(laptop.set_grant(phone_id.clone(), Capability::Sites, true)).unwrap();
    assert_eq!(fetch(&phone, &laptop_id, "/").unwrap().status, 200);
    laptop.stop_site();
    assert!(refused(&phone, &laptop_id));
    assert!(!offers_sites(&phone, &laptop_id));
}

#[test]
fn revoking_the_device_ends_its_visit_and_its_grant() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, laptop_id, phone_id) = laptop_and_phone(dir.path());
    block_on(laptop.set_grant(phone_id.clone(), Capability::Sites, true)).unwrap();
    assert_eq!(fetch(&phone, &laptop_id, "/").unwrap().status, 200);

    block_on(laptop.revoke_device(phone_id.clone())).unwrap();
    assert!(refused(&phone, &laptop_id));
    // The open connection is closed too, and a new one is refused.
    eventually("the connection refused", || {
        fetch(&phone, &laptop_id, "/").is_err()
    });

    // Trusted again, it starts with no grant.
    block_on(laptop.trust_device(phone_id.clone())).unwrap();
    assert!(laptop
        .devices()
        .iter()
        .find(|d| d.endpoint_id == phone_id)
        .unwrap()
        .grants
        .is_empty());
    assert!(refused(&phone, &laptop_id));
}

#[test]
fn a_computer_opens_the_site_in_any_browser_through_a_local_port() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, laptop_id, phone_id) = laptop_and_phone(dir.path());
    block_on(laptop.set_grant(phone_id, Capability::Sites, true)).unwrap();

    let port = phone.open_site_port(&laptop_id, 0).unwrap();
    for _ in 0..3 {
        let answer = http_get(port.port, "/notes/today.txt");
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(answer.ends_with("milk"), "{answer}");
    }
    drop(port);
}

#[test]
fn a_server_on_the_hosts_localhost_is_reached_and_told_who_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, laptop_id, phone_id) = laptop_and_phone(dir.path());
    block_on(laptop.set_grant(phone_id.clone(), Capability::Sites, true)).unwrap();

    // A tiny web server on the laptop's localhost: it answers with who
    // Relay said was asking.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in server.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).to_lowercase();
            let who: Vec<&str> = head
                .lines()
                .filter_map(|l| l.strip_prefix("x-relay-device: "))
                .collect();
            let body = who.join(",");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            );
        }
    });
    block_on(laptop.share_site(SiteSource::Local { port: server_port })).unwrap();

    // The phone can't pretend to be someone else.
    let mut request = get("/");
    request.headers.push(relay_core::SiteHeader {
        name: "X-Relay-Device".into(),
        value: "someone-else".into(),
    });
    let page = block_on(phone.fetch_site(laptop_id.clone(), request)).unwrap();
    assert_eq!(page.status, 200);
    assert_eq!(
        String::from_utf8(page.body).unwrap(),
        phone_id.to_lowercase()
    );

    // A server that isn't running is said plainly.
    block_on(laptop.share_site(SiteSource::Local { port: free_port() })).unwrap();
    assert_eq!(fetch(&phone, &laptop_id, "/").unwrap().status, 502);
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}
