//! `relay/web/1`: sites only Relay can reach (docs/relay-web.md).
//!
//! A device **hosts a site**: a folder, or a web server that listens only
//! on its own localhost. Its trusted devices with the "opens sites" grant
//! **open it** over iroh; nothing listens on a public port.
//!
//! - **Who.** A connection is accepted only from a trusted device with the
//!   grant, and only while a site is shared; the grant is checked again on
//!   every request, so revoking ends access mid-visit.
//! - **One HTTP/1.1 exchange per QUIC stream.** The viewer writes a request
//!   and reads the response until the host closes the stream. Many streams
//!   share one connection, so a page's parts load in parallel. Requests go
//!   out with `Connection: close`, so every response ends with its stream.
//! - **Only loopback targets,** so a site is never a proxy into the host's
//!   network. A folder site serves files under its folder only.
//! - **The viewer** is a local port on the viewing computer
//!   (`relay web <device>`), listening on 127.0.0.1 only.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::EndpointAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

use crate::device::DeviceInbound;
use crate::model::RelayError;
use crate::node::{lock, Node};

pub(crate) const WEB_ALPN: &[u8] = b"forge-relay/web/1";
/// The close code for a device that may not open this device's sites.
const NOT_ALLOWED: u32 = 403;
/// A request's head (request line and headers) is at most this long.
const MAX_HEAD: usize = 64 * 1024;
/// Streams served at once on one connection.
const MAX_STREAMS: usize = 32;
/// How long a stream may sit idle.
const IDLE: Duration = Duration::from_secs(60);
/// Headers the viewer may not set: the host sets or drops them.
const HOP_HEADERS: [&str; 5] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "upgrade",
    "x-relay-device",
];

/// What a device shares as its site.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SiteSource {
    /// Files under a folder; `/` serves its `index.html`.
    Folder { path: String },
    /// A web server on this device's localhost (127.0.0.1) at `port`.
    Local { port: u16 },
}

impl SiteSource {
    /// Checks a site before it is shared: the folder exists, the port isn't 0.
    pub(crate) fn checked(self) -> Result<Self, RelayError> {
        match &self {
            SiteSource::Folder { path } if !Path::new(path).is_dir() => Err(RelayError::File {
                reason: format!("{path} isn't a folder"),
            }),
            SiteSource::Local { port: 0 } => Err(RelayError::Connection {
                reason: "a site's port can't be 0".into(),
            }),
            _ => Ok(self),
        }
    }
}

/// The hosting side of `relay/web/1`.
#[derive(Clone)]
pub(crate) struct WebHandler {
    pub host: Arc<dyn DeviceInbound>,
}

impl std::fmt::Debug for WebHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WebHandler")
    }
}

impl ProtocolHandler for WebHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let them = conn.remote_id().to_string();
        if self.host.site().is_none() || !self.host.opens_sites(&them) {
            log::info!("web: refused a device without the grant, or nothing is shared");
            conn.close(NOT_ALLOWED.into(), b"not allowed");
            return Ok(());
        }
        let slots = Arc::new(Semaphore::new(MAX_STREAMS));
        loop {
            let (send, recv) = match conn.accept_bi().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                let mut send = send;
                let _ = send.reset(0u32.into());
                continue;
            };
            let host = Arc::clone(&self.host);
            let them = them.clone();
            tokio::spawn(async move {
                let _slot = slot;
                if let Err(err) = serve(host, &them, send, recv).await {
                    log::info!("web: a request ended early: {err}");
                }
            });
        }
        Ok(())
    }
}

/// A request's head, split from what came after it.
struct Head {
    method: String,
    path: String,
    /// Header lines as received, name and value.
    headers: Vec<(String, String)>,
    /// Body bytes read along with the head.
    extra: Vec<u8>,
    content_length: u64,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn bad(reason: &str) -> RelayError {
    RelayError::Connection {
        reason: reason.to_string(),
    }
}

/// Reads one request head (up to `MAX_HEAD`) from `read`.
async fn read_head<R: tokio::io::AsyncRead + Unpin>(read: &mut R) -> Result<Head, RelayError> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(bad("request head too long"));
        }
        let n = tokio::time::timeout(IDLE, read.read(&mut chunk))
            .await
            .map_err(|_| bad("idle"))?
            .map_err(|e| bad(&e.to_string()))?;
        if n == 0 {
            return Err(bad("closed before the request ended"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let mut parsed = [httparse::EMPTY_HEADER; 100];
    let mut request = httparse::Request::new(&mut parsed);
    match request.parse(&buf[..end]) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(bad("not an HTTP request")),
    }
    let headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).trim().to_string(),
            )
        })
        .collect();
    let mut head = Head {
        method: request.method.unwrap_or("GET").to_string(),
        path: request.path.unwrap_or("/").to_string(),
        headers,
        extra: buf[end..].to_vec(),
        content_length: 0,
    };
    if head.header("transfer-encoding").is_some() {
        return Err(bad("chunked request bodies aren't supported yet"));
    }
    head.content_length = match head.header("content-length") {
        Some(v) => v.parse().map_err(|_| bad("bad content-length"))?,
        None => 0,
    };
    Ok(head)
}

/// The head as sent on: hop headers dropped, `Connection: close`, and
/// `X-Relay-Device` set to who asked (`None` leaves it out).
fn rewritten(head: &Head, device: Option<&str>) -> Vec<u8> {
    let mut out = format!("{} {} HTTP/1.1\r\n", head.method, head.path).into_bytes();
    for (name, value) in &head.headers {
        if HOP_HEADERS
            .iter()
            .any(|h| name.eq_ignore_ascii_case(h))
        {
            continue;
        }
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    if let Some(device) = device {
        out.extend_from_slice(format!("X-Relay-Device: {device}\r\n").as_bytes());
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out
}

/// Serves one request on one stream.
async fn serve(
    host: Arc<dyn DeviceInbound>,
    them: &str,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), RelayError> {
    let head = read_head(&mut recv).await?;
    // Checked again for every request: a revoked device stops here.
    let site = match host.site() {
        Some(site) if host.opens_sites(them) => site,
        _ => {
            respond(&mut send, 403, "Forbidden", "text/plain", b"Not allowed.").await?;
            return Ok(());
        }
    };
    match site {
        SiteSource::Folder { path } => serve_file(&mut send, &head, Path::new(&path)).await?,
        SiteSource::Local { port } => {
            proxy(&mut send, &mut recv, &head, port, them).await?;
        }
    }
    let _ = send.finish();
    Ok(())
}

async fn respond(
    send: &mut SendStream,
    status: u16,
    reason: &str,
    kind: &str,
    body: &[u8],
) -> Result<(), RelayError> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    send.write_all(head.as_bytes())
        .await
        .map_err(|e| bad(&e.to_string()))?;
    send.write_all(body).await.map_err(|e| bad(&e.to_string()))?;
    let _ = send.finish();
    Ok(())
}

/// The file a request path names under `root`, or `None` when it escapes
/// the folder or doesn't exist. A folder means its `index.html`.
pub(crate) fn file_for(root: &Path, request_path: &str) -> Option<PathBuf> {
    let path = request_path.split(['?', '#']).next().unwrap_or("/");
    let decoded = percent_decode(path)?;
    let mut relative = PathBuf::new();
    for part in Path::new(&decoded).components() {
        match part {
            Component::Normal(p) => relative.push(p),
            Component::RootDir | Component::CurDir => {}
            // `..` or a drive prefix: not under the folder.
            _ => return None,
        }
    }
    let root = root.canonicalize().ok()?;
    let mut file = root.join(relative).canonicalize().ok()?;
    // Symlinks out of the folder are refused too.
    if !file.starts_with(&root) {
        return None;
    }
    if file.is_dir() {
        file = file.join("index.html");
    }
    file.is_file().then_some(file)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    // A NUL or a backslash has no business in a site's path.
    (!text.contains(['\0', '\\'])).then_some(text)
}

fn content_type(file: &Path) -> &'static str {
    match file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

async fn serve_file(send: &mut SendStream, head: &Head, root: &Path) -> Result<(), RelayError> {
    if head.method != "GET" && head.method != "HEAD" {
        return respond(send, 405, "Method Not Allowed", "text/plain", b"").await;
    }
    let Some(file) = file_for(root, &head.path) else {
        return respond(send, 404, "Not Found", "text/plain", b"Not found.").await;
    };
    let mut opened = tokio::fs::File::open(&file)
        .await
        .map_err(|e| bad(&e.to_string()))?;
    let size = opened
        .metadata()
        .await
        .map_err(|e| bad(&e.to_string()))?
        .len();
    let start = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {size}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        content_type(&file)
    );
    send.write_all(start.as_bytes())
        .await
        .map_err(|e| bad(&e.to_string()))?;
    if head.method == "GET" {
        tokio::io::copy(&mut opened, send)
            .await
            .map_err(|e| bad(&e.to_string()))?;
    }
    Ok(())
}

/// Passes the request to `127.0.0.1:port` and streams the answer back.
async fn proxy(
    send: &mut SendStream,
    recv: &mut RecvStream,
    head: &Head,
    port: u16,
    them: &str,
) -> Result<(), RelayError> {
    let Ok(mut server) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await else {
        return respond(
            send,
            502,
            "Bad Gateway",
            "text/plain",
            b"The site's server isn't running.",
        )
        .await;
    };
    server
        .write_all(&rewritten(head, Some(them)))
        .await
        .map_err(|e| bad(&e.to_string()))?;
    copy_body(recv, &mut server, &head.extra, head.content_length).await?;
    let (mut from_server, _to_server) = server.split();
    tokio::io::copy(&mut from_server, send)
        .await
        .map_err(|e| bad(&e.to_string()))?;
    Ok(())
}

/// Copies a request body of `length` bytes: what came with the head, then
/// the rest from `from`.
async fn copy_body<R, W>(from: &mut R, to: &mut W, extra: &[u8], length: u64) -> Result<(), RelayError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let first = extra.len().min(length as usize);
    to.write_all(&extra[..first])
        .await
        .map_err(|e| bad(&e.to_string()))?;
    let rest = length.saturating_sub(first as u64);
    if rest > 0 {
        let mut limited = from.take(rest);
        tokio::io::copy(&mut limited, to)
            .await
            .map_err(|e| bad(&e.to_string()))?;
    }
    Ok(())
}

/// The viewing side: a local port whose connections go to a device's site.
pub(crate) struct Tunnel {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl Tunnel {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Opens `127.0.0.1:port` (0 picks one) on this device; each browser
/// connection's request goes to the site of the device at `to`.
pub(crate) async fn open_tunnel(
    node: Arc<Node>,
    to: EndpointAddr,
    port: u16,
) -> Result<Tunnel, RelayError> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .map_err(|e| bad(&format!("can't open port {port}: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| bad(&e.to_string()))?
        .port();
    let stop = Arc::new(AtomicBool::new(false));
    let conn: Arc<Mutex<Option<Connection>>> = Arc::default();
    let stopped = Arc::clone(&stop);
    tokio::spawn(async move {
        while !stopped.load(Ordering::SeqCst) {
            let accepted =
                tokio::time::timeout(Duration::from_millis(250), listener.accept()).await;
            let Ok(Ok((browser, _))) = accepted else {
                continue;
            };
            let node = Arc::clone(&node);
            let to = to.clone();
            let conn = Arc::clone(&conn);
            tokio::spawn(async move {
                if let Err(err) = forward(&node, to, &conn, browser).await {
                    log::info!("web: a page request failed: {err}");
                }
            });
        }
    });
    Ok(Tunnel { port, stop })
}

/// The connection to the site's device, opened once and reused.
async fn connection(
    node: &Node,
    to: EndpointAddr,
    cached: &Mutex<Option<Connection>>,
) -> Result<Connection, RelayError> {
    if let Some(c) = lock(cached).as_ref().filter(|c| c.close_reason().is_none()) {
        return Ok(c.clone());
    }
    let c = tokio::time::timeout(crate::code::STEP, node.endpoint.connect(to, WEB_ALPN))
        .await
        .map_err(|_| RelayError::Unreachable {
            reason: "the other device didn't answer".into(),
        })?
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })?;
    *lock(cached) = Some(c.clone());
    Ok(c)
}

/// One browser connection: one request over a new stream, one response.
async fn forward(
    node: &Node,
    to: EndpointAddr,
    cached: &Mutex<Option<Connection>>,
    mut browser: TcpStream,
) -> Result<(), RelayError> {
    let head = read_head(&mut browser).await?;
    let attempt = async {
        let conn = connection(node, to, cached).await?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| bad(&e.to_string()))?;
        send.write_all(&rewritten(&head, None))
            .await
            .map_err(|e| bad(&e.to_string()))?;
        copy_body(&mut browser, &mut send, &head.extra, head.content_length).await?;
        let _ = send.finish();
        let mut first = [0u8; 1];
        // The first byte shows the site answered at all.
        let n = recv
            .read(&mut first)
            .await
            .map_err(|e| bad(&e.to_string()))?
            .unwrap_or(0);
        if n == 0 {
            return Err(RelayError::Refused);
        }
        browser
            .write_all(&first[..n])
            .await
            .map_err(|e| bad(&e.to_string()))?;
        tokio::io::copy(&mut recv, &mut browser)
            .await
            .map_err(|e| bad(&e.to_string()))?;
        Ok(())
    };
    let result = attempt.await;
    if let Err(err) = &result {
        // Tell the browser in words; the site may simply not be shared.
        let text = match err {
            RelayError::Refused => {
                "The other device isn't sharing a site with this one.".to_string()
            }
            other => format!("Couldn't reach the site: {other}"),
        };
        let page = format!(
            "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
            text.len()
        );
        let _ = browser.write_all(page.as_bytes()).await;
    }
    let _ = browser.shutdown().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("site");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("index.html"), "home").unwrap();
        std::fs::write(root.join("docs/a b.txt"), "a").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let ok = |p: &str| file_for(&root, p).map(|f| f.file_name().unwrap().to_owned());
        assert_eq!(ok("/").unwrap(), "index.html");
        assert_eq!(ok("/index.html?x=1").unwrap(), "index.html");
        assert_eq!(ok("/docs/a%20b.txt").unwrap(), "a b.txt");
        assert!(ok("/../secret.txt").is_none());
        assert!(ok("/docs/../../secret.txt").is_none());
        assert!(ok("/%2e%2e/secret.txt").is_none());
        assert!(ok("/docs%5c..%5csecret.txt").is_none());
        assert!(ok("/missing").is_none());
        assert!(ok("/%zz").is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link")).unwrap();
            assert!(ok("/link").is_none(), "a symlink out of the folder");
        }
    }

    #[test]
    fn the_viewer_cant_set_who_it_is_or_keep_the_connection() {
        let head = Head {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![
                ("Host".into(), "localhost".into()),
                ("X-Relay-Device".into(), "someone else".into()),
                ("Connection".into(), "keep-alive".into()),
                ("Upgrade".into(), "websocket".into()),
            ],
            extra: vec![],
            content_length: 0,
        };
        let out = String::from_utf8(rewritten(&head, Some("abc"))).unwrap();
        assert!(out.starts_with("GET / HTTP/1.1\r\n"));
        assert!(out.contains("Host: localhost\r\n"));
        assert!(out.contains("X-Relay-Device: abc\r\n"));
        assert!(!out.contains("someone else"));
        assert!(!out.contains("keep-alive") && !out.contains("websocket"));
        assert!(out.ends_with("Connection: close\r\n\r\n"));
    }

    #[test]
    fn a_site_is_checked_before_it_is_shared() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().into_owned();
        assert!(SiteSource::Folder { path: folder }.checked().is_ok());
        assert!(SiteSource::Folder {
            path: "/no/such/folder".into()
        }
        .checked()
        .is_err());
        assert!(SiteSource::Local { port: 0 }.checked().is_err());
        assert!(SiteSource::Local { port: 8000 }.checked().is_ok());
    }
}
