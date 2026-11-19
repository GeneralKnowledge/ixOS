//! Relay's own servers (Relay M2 PR 7): with a relay and a lookup server
//! set, each device publishes a record of where it can be reached, signed
//! by its key, and a trusted device it never saw on a network is found by
//! key alone. With a relay, the record names only the relay; the laptop
//! is reached through it first. Both servers run in process: a plain-HTTP relay, as on
//! Hetzner, and iroh's test pkarr server.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{CodeSend, RelayCore, RelayServers};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// Opens a device, waiting while a closing one still holds its folder.
fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let core = loop {
        match RelayCore::new(dir.join(name).to_string_lossy().into_owned(), name.into()) {
            Ok(core) => break core,
            Err(err) => {
                assert!(Instant::now() < deadline, "couldn't open {name}: {err}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
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
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A phone and a laptop that made one code transfer and trusted each other.
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
    block_on(laptop.trust_device(on_laptop.endpoint_id.clone())).unwrap();
}

/// Notes `core` received from the device called `from`, oldest first:
/// they are messages in its conversation.
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
fn a_trusted_device_is_found_by_key_through_the_lookup_server() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt
        .block_on(iroh::test_utils::DnsPkarrServer::run())
        .unwrap();
    let mut relay = iroh_relay::server::RelayConfig::new((std::net::Ipv4Addr::LOCALHOST, 0));
    relay.tls = None;
    let mut config = iroh_relay::server::ServerConfig::default();
    config.relay = Some(relay);
    let relay_server = rt
        .block_on(iroh_relay::server::Server::spawn(config))
        .unwrap();
    let servers = RelayServers {
        relay_url: Some(format!("http://{}", relay_server.http_addr().unwrap())),
        lookup_url: Some(server.pkarr_url().to_string()),
    };
    let dir = tempfile::tempdir().unwrap();
    // Set the servers, then open Relay again: they apply from the next open.
    for name in ["phone", "laptop"] {
        let core = device(dir.path(), name);
        assert!(block_on(core.set_servers(RelayServers {
            relay_url: Some("not a url".into()),
            lookup_url: None,
        }))
        .is_err());
        block_on(core.set_servers(servers.clone())).unwrap();
        assert_eq!(core.servers(), servers);
    }
    std::thread::sleep(Duration::from_millis(500));
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    trust_each_other(dir.path(), &phone, &laptop);
    let laptop_id = phone
        .devices()
        .into_iter()
        .find(|d| d.name == "laptop")
        .unwrap()
        .endpoint_id;
    assert!(phone.devices().iter().all(|d| d.route.is_none()));

    // The laptop's Relay is open: it publishes where it is.
    block_on(laptop.device_service()).unwrap();
    let id: iroh::EndpointId = laptop_id.parse().unwrap();
    rt.block_on(server.on_endpoint(&id, Duration::from_secs(20)))
        .unwrap();
    // What anyone who knows its id can read: the relay, and no address of
    // this machine at all, not even a public one.
    let deadline = Instant::now() + Duration::from_secs(20);
    let published = loop {
        let record = rt
            .block_on(
                server
                    .dns_resolver()
                    .lookup_endpoint_by_id(&id, "dns.iroh.test"),
            )
            .unwrap();
        let published: Vec<_> = record.into_endpoint_addr().addrs.into_iter().collect();
        if published.iter().any(|a| a.is_relay()) {
            break published;
        }
        assert!(Instant::now() < deadline, "no relay in {published:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    // With a relay set, the relay is all it names.
    assert!(published.iter().all(|a| a.is_relay()), "{published:?}");
    for addr in &published {
        if let iroh::TransportAddr::Ip(addr) = addr {
            let private = match addr.ip() {
                std::net::IpAddr::V4(ip) => {
                    let [a, b, ..] = ip.octets();
                    ip.is_private()
                        || ip.is_loopback()
                        || ip.is_link_local()
                        || (a == 100 && (64..128).contains(&b))
                }
                std::net::IpAddr::V6(ip) => {
                    ip.is_loopback()
                        || (ip.segments()[0] & 0xfe00) == 0xfc00
                        || (ip.segments()[0] & 0xffc0) == 0xfe80
                }
            };
            assert!(!private, "published a private address: {published:?}");
        }
    }

    // The phone has never seen it on a network (no Bonjour, no address it
    // reached it at), and still finds it.
    block_on(phone.send_note_as_older_build(laptop_id, "found by key".into())).unwrap();
    eventually("the note", || {
        notes_from(&laptop, "phone") == ["found by key"]
    });
    // Once connected through the relay, the two may move to a direct path
    // they found together; any route will do here.
    eventually("the route", || {
        phone
            .devices()
            .iter()
            .any(|d| d.name == "laptop" && d.route.is_some())
    });
}
