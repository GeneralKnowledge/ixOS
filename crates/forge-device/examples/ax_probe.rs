//! Spike: does Apple's accessibility audit service work from Linux?
//!
//! This is the service Xcode's Accessibility Inspector uses
//! (`com.apple.accessibility.axAuditDaemon.remoteserver`). It speaks DTX, the
//! Instruments protocol, over a plain lockdown connection through usbmuxd.
//! The protocol is taken from pymobiledevice3's `accessibilityaudit.py`.
//!
//! It reports each stage separately, so a failure says how far it got:
//! 1. start the service (does it need the Developer Disk Image?);
//! 2. read its capabilities and supported audit types;
//! 3. walk the foreground app's elements in VoiceOver order, reading each
//!    one's Label, Identifier and Value;
//! 4. optionally run Apple's audit (`--audit`), or press the element with an
//!    accessibility identifier (`--press library.add`).
//!
//! ```text
//! cargo run -p forge-device --features ax-probe --example ax_probe -- [--udid U] [--max 40] [--audit] [--press ID]
//! ```
//!
//! Put the app under test in the foreground and unlock the phone first.
//! Findings go in docs/research/ax-audit.md.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use idevice::provider::IdeviceProvider;
use idevice::services::dvt::message::{AuxValue, Message};
use idevice::services::dvt::remote_server::RemoteServerClient;
use idevice::services::lockdown::LockdownClient;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use idevice::{IdeviceService, ReadWrite};
use plist::{Dictionary, Value};

const SERVICE: &str = "com.apple.accessibility.axAuditDaemon.remoteserver";
const FOCUS_CHANGED: &str = "hostInspectorCurrentElementChanged:";
const AUDIT_DONE: &str = "hostDeviceDidCompleteAuditCategoriesWithAuditIssues:";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Args {
    udid: Option<String>,
    max: usize,
    audit: bool,
    press: Option<String>,
}

fn args() -> Args {
    let mut out = Args {
        udid: None,
        max: 40,
        audit: false,
        press: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--udid" => out.udid = it.next(),
            "--max" => out.max = it.next().and_then(|v| v.parse().ok()).unwrap_or(40),
            "--audit" => out.audit = true,
            "--press" => out.press = it.next(),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    out
}

fn main() {
    let args = args();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    if let Err(err) = rt.block_on(run(args)) {
        eprintln!("FAILED: {err}");
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<()> {
    let mut mux = UsbmuxdConnection::default().await?;
    let devices = mux.get_devices().await?;
    let device = match &args.udid {
        Some(u) => devices.into_iter().find(|d| &d.udid == u),
        None => devices.into_iter().next(),
    }
    .ok_or("no iPhone found through usbmuxd")?;
    let provider = device.to_provider(UsbmuxdAddr::default(), "forge-ax-probe");
    let pairing = provider.get_pairing_file().await?;

    let mut lockdown = LockdownClient::connect(&provider).await?;
    let version = lockdown
        .get_value(Some("ProductVersion"), None)
        .await
        .ok()
        .and_then(|v| v.as_string().map(str::to_string))
        .unwrap_or_default();
    println!("device {} iOS {version}", device.udid);
    let legacy = lockdown.start_session(&pairing).await?;

    // Stage 1. On Cati the handshake once failed with "InvalidContentType"
    // right after a previous run, so a failed handshake starts the service
    // again once.
    let started = Instant::now();
    let _ = legacy;
    let mut attempt = 0;
    let socket = loop {
        attempt += 1;
        let (port, ssl) = lockdown
            .start_service(SERVICE)
            .await
            .map_err(|e| format!("stage 1, start {SERVICE}: {e}"))?;
        println!("[1] service started on port {port} (ssl {ssl})");
        let socket = provider
            .connect(port)
            .await?
            .get_socket()
            .ok_or("no socket")?;
        if !ssl {
            break socket;
        }
        match strip_ssl(socket, &pairing).await {
            Ok(raw) => break raw,
            Err(err) if attempt == 1 => {
                println!("[1] TLS handshake failed ({err}); starting the service again");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(err) => return Err(format!("stage 1, TLS handshake: {err}").into()),
        }
    };
    let mut ax = Ax::new(RemoteServerClient::new(socket));
    ax.hello().await?;

    // Stage 2.
    let caps = ax.call("deviceCapabilities", vec![]).await?;
    println!("[2] capabilities: {}", short(&caps));
    let types = ax.call("deviceAllSupportedAuditTypes", vec![]).await?;
    println!("[2] audit types: {}", short(&types));
    println!("    ({} ms so far)", started.elapsed().as_millis());

    // Stage 4 runs before the walk, with app monitoring still off, as
    // pymobiledevice3's run_audit does. Every other event seen while waiting
    // is listed, to tell "no answer" from "an answer we didn't recognise".
    if args.audit {
        let t = Instant::now();
        let types = match unwrap(types) {
            Value::Array(a) => a,
            other => vec![other],
        };
        ax.send("deviceBeginAuditTypes:", vec![arg(Value::Array(types))])
            .await?;
        match ax.wait_event(AUDIT_DONE, Duration::from_secs(120)).await? {
            Some(issues) => {
                let issues = match unwrap(issues) {
                    Value::Array(a) => a,
                    other => vec![other],
                };
                println!(
                    "[4] audit: {} issues in {} ms",
                    issues.len(),
                    t.elapsed().as_millis()
                );
                for i in issues.iter().take(20) {
                    println!("    {}", short(&unwrap(i.clone())));
                }
            }
            None => println!(
                "[4] audit: no result within 120 s; other events: {}",
                ax.other_events()
            ),
        }
    }

    // Stage 3.
    ax.send(
        "deviceSetAppMonitoringEnabled:",
        vec![arg(Value::Boolean(true))],
    )
    .await?;
    ax.send(
        "deviceInspectorSetMonitoredEventType:",
        vec![arg(Value::Integer(0.into()))],
    )
    .await?;
    let walk = Instant::now();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut pressed = false;
    ax.move_focus(4).await?;
    let mut misses = 0;
    while seen.len() < args.max {
        let Some(focus) = ax.wait_event(FOCUS_CHANGED, Duration::from_secs(1)).await? else {
            misses += 1;
            if misses >= 5 {
                println!("[3] no focus events after 5 tries");
                break;
            }
            ax.move_focus(4).await?;
            continue;
        };
        misses = 0;
        let focus = first(focus);
        let caption = field(&focus, "CaptionTextValue_v1");
        let element = element_bytes(&focus).ok_or("focus event without an element")?;
        if seen.contains(&element) {
            break;
        }
        let t = Instant::now();
        let mut attrs = Vec::new();
        for name in ["Label", "Identifier", "Value"] {
            let v = ax
                .call(
                    "deviceElement:valueForAttribute:",
                    vec![arg(element_arg(&element)), arg(attribute_arg(name))],
                )
                .await?;
            attrs.push((name, short(&unwrap(v))));
        }
        println!(
            "[3] {:>2} caption={} {} ({} ms)",
            seen.len() + 1,
            caption,
            attrs
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" "),
            t.elapsed().as_millis()
        );
        if let Some(target) = &args.press {
            if !pressed
                && attrs
                    .iter()
                    .any(|(k, v)| *k == "Identifier" && v.trim_matches('"') == target)
            {
                ax.send(
                    "deviceElement:performAction:withValue:",
                    vec![
                        arg(element_arg(&element)),
                        arg(press_action()),
                        arg(Value::Integer(0.into())),
                    ],
                )
                .await?;
                println!("[4] pressed {target}");
                pressed = true;
            }
        }
        seen.push(element);
        ax.move_focus(4).await?;
    }
    println!(
        "[3] walked {} elements in {} ms",
        seen.len(),
        walk.elapsed().as_millis()
    );
    if let Some(target) = &args.press {
        if !pressed {
            println!("[4] no element with identifier {target} was reached");
        }
    }

    Ok(())
}

/// The service wants a TLS handshake and then plain DTX on the same socket
/// (pymobiledevice3's `strip_ssl`). idevice keeps TLS on, so do it here.
async fn strip_ssl(
    socket: Box<dyn ReadWrite>,
    pairing: &idevice::pairing_file::PairingFile,
) -> Result<Box<dyn ReadWrite>> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, ServerName, UnixTime};

    #[derive(Debug)]
    struct AnyCert(Arc<rustls::crypto::CryptoProvider>);
    impl ServerCertVerifier for AnyCert {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> std::result::Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = PrivateKeyDer::from_pem_slice(&pairing.host_private_key)?;
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert(provider)))
        .with_client_auth_cert(vec![pairing.host_certificate.clone()], key)?;
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("Device")?, socket)
        .await?;
    let (raw, _) = tls.into_inner();
    Ok(raw)
}

/// A DTX connection to the audit daemon: calls on the root channel, with the
/// daemon's own notifications kept aside until asked for.
struct Ax {
    client: RemoteServerClient<Box<dyn ReadWrite>>,
    events: VecDeque<(String, Vec<Value>)>,
    others: Vec<String>,
}

impl Ax {
    fn new(client: RemoteServerClient<Box<dyn ReadWrite>>) -> Self {
        Self {
            client,
            events: VecDeque::new(),
            others: Vec::new(),
        }
    }

    async fn hello(&mut self) -> Result<()> {
        let mut caps = Dictionary::new();
        caps.insert(
            "com.apple.private.DTXConnection".into(),
            Value::Integer(1.into()),
        );
        caps.insert(
            "com.apple.private.DTXBlockCompression".into(),
            Value::Integer(0.into()),
        );
        self.send(
            "_notifyOfPublishedCapabilities:",
            vec![arg(Value::Dictionary(caps))],
        )
        .await
    }

    async fn send(&mut self, selector: &str, args: Vec<AuxValue>) -> Result<()> {
        self.client
            .root_channel()
            .call_method(Some(selector), Some(args), false)
            .await?;
        Ok(())
    }

    /// Calls and returns the first message on the root channel that isn't a
    /// notification from the daemon (DTX replies are not correlated here).
    async fn call(&mut self, selector: &str, args: Vec<AuxValue>) -> Result<Value> {
        self.client
            .root_channel()
            .call_method(Some(selector), Some(args), true)
            .await?;
        loop {
            let msg = self
                .read(Duration::from_secs(10))
                .await?
                .ok_or_else(|| format!("{selector}: no reply within 10 s"))?;
            match notification(&msg) {
                Some(event) => self.events.push_back(event),
                None => {
                    return Ok(match (msg.data, msg.raw_data) {
                        (Some(v), _) => deserialize(v),
                        (None, Some(raw)) if !raw.is_empty() => {
                            Value::String(format!("<{} undecoded bytes>", raw.len()))
                        }
                        _ => Value::String("<empty>".into()),
                    });
                }
            }
        }
    }

    async fn wait_event(&mut self, name: &str, wait: Duration) -> Result<Option<Value>> {
        if let Some(i) = self.events.iter().position(|(n, _)| n == name) {
            let (_, args) = self.events.remove(i).unwrap_or_default();
            return Ok(Some(payload(args)));
        }
        let deadline = Instant::now() + wait;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(msg) = self.read(left).await? else {
                break;
            };
            match notification(&msg) {
                Some((n, args)) if n == name => return Ok(Some(payload(args))),
                Some((n, _)) => self.others.push(n),
                None => self.others.push(format!(
                    "reply {}",
                    short(&msg.data.unwrap_or(Value::Boolean(false)))
                )),
            }
        }
        Ok(None)
    }

    /// Distinct names of the events `wait_event` skipped, with counts.
    fn other_events(&self) -> String {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for n in &self.others {
            match counts.iter_mut().find(|(k, _)| k == n) {
                Some((_, c)) => *c += 1,
                None => counts.push((n.clone(), 1)),
            }
        }
        if counts.is_empty() {
            return "none".into();
        }
        counts
            .iter()
            .map(|(k, c)| format!("{k} ×{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    async fn read(&mut self, wait: Duration) -> Result<Option<Message>> {
        match tokio::time::timeout(wait, self.client.read_message(0)).await {
            Ok(msg) => Ok(Some(msg?)),
            Err(_) => Ok(None),
        }
    }

    async fn move_focus(&mut self, direction: i64) -> Result<()> {
        let mut opts = Dictionary::new();
        opts.insert("allowNonAX".into(), pass(Value::Integer(0.into())));
        opts.insert("direction".into(), pass(Value::Integer(direction.into())));
        opts.insert("includeContainers".into(), pass(Value::Integer(1.into())));
        self.send(
            "deviceInspectorMoveWithOptions:",
            vec![arg(pass(Value::Dictionary(opts)))],
        )
        .await
    }
}

/// A message the daemon sent on its own: a selector plus archived arguments.
fn notification(msg: &Message) -> Option<(String, Vec<Value>)> {
    let Some(Value::String(s)) = &msg.data else {
        return None;
    };
    if !(s.ends_with(':') && (s.starts_with("host") || s.starts_with('_'))) {
        return None;
    }
    let args = msg
        .aux
        .as_ref()
        .map(|aux| {
            aux.values
                .iter()
                .filter_map(|v| match v {
                    AuxValue::Array(bytes) => ns_keyed_archive::decode::from_bytes(bytes).ok(),
                    _ => None,
                })
                .map(deserialize)
                .collect()
        })
        .unwrap_or_default();
    Some((s.clone(), args))
}

fn payload(mut args: Vec<Value>) -> Value {
    if args.len() == 1 {
        args.remove(0)
    } else {
        Value::Array(args)
    }
}

/// pymobiledevice3's `deserialize_object`: `{ObjectType: passthrough, Value}`
/// unwraps; a typed object keeps its type under `_type`.
fn deserialize(v: Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.into_iter().map(deserialize).collect()),
        Value::Dictionary(mut d) => match d.remove("ObjectType") {
            None => Value::Dictionary(d.into_iter().map(|(k, v)| (k, deserialize(v))).collect()),
            Some(Value::String(t)) if t == "passthrough" => d
                .remove("Value")
                .map(deserialize)
                .unwrap_or(Value::Boolean(false)),
            Some(t) => {
                let mut out = match d.remove("Value").map(deserialize) {
                    Some(Value::Dictionary(inner)) => inner,
                    Some(other) => {
                        let mut m = Dictionary::new();
                        m.insert("value".into(), other);
                        m
                    }
                    None => Dictionary::new(),
                };
                out.insert("_type".into(), t);
                Value::Dictionary(out)
            }
        },
        other => other,
    }
}

fn first(v: Value) -> Value {
    match v {
        Value::Array(mut a) if !a.is_empty() => a.remove(0),
        other => other,
    }
}

/// Some replies come wrapped as `[{"value": …}]` or `{"value": …}`.
fn unwrap(v: Value) -> Value {
    match v {
        Value::Dictionary(mut d) if d.len() == 1 && d.contains_key("value") => {
            d.remove("value").unwrap_or(Value::Boolean(false))
        }
        other => other,
    }
}

fn field(v: &Value, key: &str) -> String {
    v.as_dictionary()
        .and_then(|d| d.get(key))
        .map(short)
        .unwrap_or_else(|| "-".into())
}

fn element_bytes(focus: &Value) -> Option<Vec<u8>> {
    focus
        .as_dictionary()?
        .get("ElementValue_v1")?
        .as_dictionary()?
        .get("PlatformElementValue_v1")?
        .as_data()
        .map(<[u8]>::to_vec)
}

fn pass(v: Value) -> Value {
    let mut d = Dictionary::new();
    d.insert("ObjectType".into(), Value::String("passthrough".into()));
    d.insert("Value".into(), v);
    Value::Dictionary(d)
}

fn typed(kind: &str, fields: Dictionary) -> Value {
    let mut d = Dictionary::new();
    d.insert("ObjectType".into(), Value::String(kind.into()));
    d.insert("Value".into(), pass(Value::Dictionary(fields)));
    Value::Dictionary(d)
}

fn element_arg(element: &[u8]) -> Value {
    let mut f = Dictionary::new();
    f.insert(
        "PlatformElementValue_v1".into(),
        pass(Value::Data(element.to_vec())),
    );
    typed("AXAuditElement_v1", f)
}

fn attribute_arg(name: &str) -> Value {
    let mut f = Dictionary::new();
    f.insert(
        "AttributeNameValue_v1".into(),
        pass(Value::String(name.into())),
    );
    typed("AXAuditElementAttribute_v1", f)
}

fn press_action() -> Value {
    let mut f = Dictionary::new();
    f.insert(
        "AttributeNameValue_v1".into(),
        pass(Value::String("AXAction-2010".into())),
    );
    f.insert("DisplayAsTree_v1".into(), pass(Value::Integer(0.into())));
    f.insert(
        "HumanReadableNameValue_v1".into(),
        pass(Value::String("Activate".into())),
    );
    f.insert("IsInternal_v1".into(), pass(Value::Integer(0.into())));
    f.insert(
        "PerformsActionValue_v1".into(),
        pass(Value::Integer(1.into())),
    );
    f.insert("SettableValue_v1".into(), pass(Value::Integer(0.into())));
    f.insert("ValueTypeValue_v1".into(), pass(Value::Integer(1.into())));
    typed("AXAuditElementAttribute_v1", f)
}

fn arg(v: Value) -> AuxValue {
    AuxValue::archived_value(v)
}

fn short(v: &Value) -> String {
    let s = match v {
        Value::String(s) => format!("{s:?}"),
        Value::Data(d) => format!("<{} bytes>", d.len()),
        other => format!("{other:?}"),
    };
    match s.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_argument_survives_the_archive() {
        let element = vec![0x27, 0, 0, 0, 0, 0x70, 0x41, 0xa2];
        let bytes = ns_keyed_archive::encode::encode_to_bytes(element_arg(&element)).unwrap();
        let back = deserialize(ns_keyed_archive::decode::from_bytes(&bytes).unwrap());
        let d = back.as_dictionary().unwrap();
        assert_eq!(
            d.get("_type").and_then(Value::as_string),
            Some("AXAuditElement_v1")
        );
        assert_eq!(
            d.get("PlatformElementValue_v1").and_then(Value::as_data),
            Some(element.as_slice())
        );
    }

    #[test]
    fn focus_event_yields_its_element() {
        let mut el = Dictionary::new();
        el.insert(
            "PlatformElementValue_v1".into(),
            pass(Value::Data(vec![1, 2])),
        );
        let mut f = Dictionary::new();
        f.insert(
            "CaptionTextValue_v1".into(),
            pass(Value::String("Add, Button".into())),
        );
        f.insert("ElementValue_v1".into(), typed("AXAuditElement_v1", el));
        let focus = first(deserialize(Value::Array(vec![typed(
            "AXAuditInspectorFocus_v1",
            f,
        )])));
        assert_eq!(element_bytes(&focus), Some(vec![1, 2]));
        assert_eq!(field(&focus, "CaptionTextValue_v1"), "\"Add, Button\"");
    }
}
