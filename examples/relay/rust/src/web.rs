//! `relay/web/1`: sites only Relay can reach (N6, docs/relay-web.md), the
//! first capability (N5, docs/relay-capabilities.md).
//!
//! A device **hosts a site**: a folder, or a web server that listens only
//! on its own localhost. Its trusted devices granted `Capability::Sites`
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
//!   (`relay web <device>`), listening on 127.0.0.1 only, or the iPhone's
//!   in-app viewer, which asks for each request with `fetch`.

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

use crate::capability::Capability;
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

/// When a request last moved, in either direction. A transfer is idle only
/// when nothing has moved either way for `IDLE`: a server still taking a
/// big upload hasn't stopped answering.
struct Activity(std::sync::Mutex<tokio::time::Instant>);

impl Activity {
    fn new() -> Self {
        Self(std::sync::Mutex::new(tokio::time::Instant::now()))
    }

    fn moved(&self) {
        *lock(&self.0) = tokio::time::Instant::now();
    }

    /// Runs `step`, giving up once nothing has moved for `IDLE`. The
    /// caller says when bytes moved (`moved`): an end or an error isn't
    /// progress.
    async fn within<F: std::future::Future>(&self, step: F) -> Result<F::Output, RelayError> {
        tokio::pin!(step);
        loop {
            let deadline = *lock(&self.0) + IDLE;
            tokio::select! {
                out = &mut step => return Ok(out),
                _ = tokio::time::sleep_until(deadline) => {
                    if *lock(&self.0) + IDLE <= tokio::time::Instant::now() {
                        return Err(bad("idle"));
                    }
                }
            }
        }
    }
}
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
        if !allowed(self.host.as_ref(), &them) {
            log::info!("web: refused a device without the grant, or nothing is shared");
            conn.close(NOT_ALLOWED.into(), b"not allowed");
            return Ok(());
        }
        // Revoking the device, taking the grant away or stopping the site
        // closes the connection at once, even mid-download.
        let watched = conn.clone();
        let host = Arc::clone(&self.host);
        let who = them.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = watched.closed() => break,
                    _ = tokio::time::sleep(WATCH) => {}
                }
                let still = {
                    let host = Arc::clone(&host);
                    let who = who.clone();
                    tokio::task::spawn_blocking(move || allowed(host.as_ref(), &who))
                        .await
                        .unwrap_or(false)
                };
                if !still {
                    watched.close(NOT_ALLOWED.into(), b"not allowed");
                    break;
                }
            }
        });
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

/// How often an open connection checks it is still allowed.
const WATCH: Duration = Duration::from_millis(250);

/// Whether `them` may open this device's site now: one is shared, and the
/// device is trusted and granted `Capability::Sites`.
fn allowed(host: &dyn DeviceInbound, them: &str) -> bool {
    host.site().is_some() && host.granted(them, Capability::Sites)
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
        if HOP_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) {
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
        Some(site) if host.granted(them, Capability::Sites) => site,
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
    send.write_all(body)
        .await
        .map_err(|e| bad(&e.to_string()))?;
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
        // The index may be a symlink out of the folder too.
        file = file.join("index.html").canonicalize().ok()?;
        if !file.starts_with(&root) {
            return None;
        }
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
        copy_idle(&mut opened, send, &Activity::new()).await?;
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
    let (mut from_server, mut to_server) = server.split();
    // The answer is read while the body goes up: a server may answer (and
    // stop reading) before the whole body is sent.
    let moving = Activity::new();
    both_ways(
        copy_body(
            recv,
            &mut to_server,
            &head.extra,
            head.content_length,
            &moving,
        ),
        copy_idle(&mut from_server, send, &moving),
    )
    .await
}

/// Runs an upload and a download together, and ends with the download: an
/// upload the other side stopped taking doesn't hold the answer back.
async fn both_ways(
    upload: impl std::future::Future<Output = Result<(), RelayError>>,
    download: impl std::future::Future<Output = Result<u64, RelayError>>,
) -> Result<(), RelayError> {
    tokio::pin!(upload, download);
    let mut uploading = true;
    loop {
        tokio::select! {
            sent = &mut upload, if uploading => {
                uploading = false;
                if let Err(err) = sent {
                    log::info!("web: a request body stopped early: {err}");
                }
            }
            got = &mut download => return got.map(|_| ()),
        }
    }
}

/// Copies until the end, giving up when nothing moves for `IDLE`, either
/// way (`moving` is shared with the request's other direction).
async fn copy_idle<R, W>(from: &mut R, to: &mut W, moving: &Activity) -> Result<u64, RelayError>
where
    R: tokio::io::AsyncRead + Unpin + ?Sized,
    W: tokio::io::AsyncWrite + Unpin + ?Sized,
{
    let mut buf = vec![0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = moving
            .within(from.read(&mut buf))
            .await?
            .map_err(|e| bad(&e.to_string()))?;
        if n == 0 {
            return Ok(total);
        }
        moving.moved();
        write_moving(to, &buf[..n], moving).await?;
        total += n as u64;
    }
}

/// Writes all of `bytes`, counting every partial write as progress: a slow
/// reader that keeps taking bytes isn't idle.
async fn write_moving<W>(to: &mut W, mut bytes: &[u8], moving: &Activity) -> Result<(), RelayError>
where
    W: tokio::io::AsyncWrite + Unpin + ?Sized,
{
    while !bytes.is_empty() {
        let n = moving
            .within(to.write(bytes))
            .await?
            .map_err(|e| bad(&e.to_string()))?;
        if n == 0 {
            return Err(bad("the other side stopped taking data"));
        }
        moving.moved();
        bytes = &bytes[n..];
    }
    Ok(())
}

/// Copies a request body of `length` bytes: what came with the head, then
/// the rest from `from`.
async fn copy_body<R, W>(
    from: &mut R,
    to: &mut W,
    extra: &[u8],
    length: u64,
    moving: &Activity,
) -> Result<(), RelayError>
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
        let copied = copy_idle(&mut limited, to, moving).await?;
        if copied < rest {
            return Err(bad("the request body ended early"));
        }
    }
    Ok(())
}

/// The viewing side: a local port whose connections go to a device's site.
/// Dropping it closes the port.
pub struct Tunnel {
    pub port: u16,
    /// Where a browser on this machine opens the site: a name of its own
    /// under `localhost`, so each device's site keeps its own cookies.
    pub url: String,
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

/// Whether the device whose site a viewer opens is still trusted here:
/// asked before every request, so revoking it stops the viewer at once.
/// Where the site's device may be dialled now: asked before every request,
/// so a revocation or a change of Connections (to Local, say) applies to an
/// open port at once. An error ends the request.
pub(crate) type SiteRoute = Arc<dyn Fn() -> Result<EndpointAddr, RelayError> + Send + Sync>;

/// The connection kept to a device's site. Closing it starts a new
/// generation, so a dial that was running at the time is not kept: it
/// may have taken a route that is no longer allowed.
#[derive(Debug, Default)]
pub(crate) struct Kept {
    conn: Option<Connection>,
    generation: u64,
}

impl Kept {
    pub(crate) fn close(&mut self, why: &'static [u8]) {
        self.generation += 1;
        if let Some(c) = self.conn.take() {
            c.close(0u32.into(), why);
        }
    }

    fn live(&self) -> Option<&Connection> {
        self.conn.as_ref().filter(|c| c.close_reason().is_none())
    }
}

/// The browser's name for a device's site: `<id>.localhost`. Browsers send
/// `*.localhost` to this machine, and a name per device keeps one site's
/// cookies away from another's (cookies ignore the port). The whole
/// endpoint id is in the name, as one DNS label: its 32 bytes in base32
/// (52 letters and digits), so no two devices share a name.
pub(crate) fn site_host(endpoint_id: &str) -> String {
    let label = hex_bytes(endpoint_id).map_or_else(
        || {
            endpoint_id
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(63)
                .collect::<String>()
                .to_ascii_lowercase()
        },
        |bytes| base32(&bytes),
    );
    format!("{label}.localhost")
}

fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// RFC 4648 base32, lowercase, without padding.
fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
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

/// Opens `127.0.0.1:port` (0 picks one) on this device; each browser
/// connection's request for `host` goes to the site of the device `route`
/// finds, asked again for every request.
pub(crate) async fn open_tunnel(
    node: Arc<Node>,
    conn: Arc<Mutex<Kept>>,
    port: u16,
    host: String,
    route: SiteRoute,
) -> Result<Tunnel, RelayError> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .map_err(|e| bad(&format!("can't open port {port}: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| bad(&e.to_string()))?
        .port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let url = format!("http://{host}:{port}/");
    tokio::spawn(async move {
        while !stopped.load(Ordering::SeqCst) {
            let accepted =
                tokio::time::timeout(Duration::from_millis(250), listener.accept()).await;
            let Ok(Ok((browser, _))) = accepted else {
                continue;
            };
            let node = Arc::clone(&node);
            let conn = Arc::clone(&conn);
            let host = host.clone();
            let route = Arc::clone(&route);
            tokio::spawn(async move {
                if let Err(err) = forward(&node, &conn, browser, &host, route.as_ref()).await {
                    log::info!("web: a page request failed: {err}");
                }
            });
        }
    });
    Ok(Tunnel { port, url, stop })
}

/// The connection to the site's device, opened once and reused. The
/// generation is read before the route is asked for, so a route found
/// under an old Connections choice (or before a revocation) is never
/// kept: the dial is dropped and the route asked for again.
async fn connection(
    node: &Node,
    cached: &Mutex<Kept>,
    route: impl Fn() -> Result<EndpointAddr, RelayError>,
) -> Result<Connection, RelayError> {
    for _ in 0..3 {
        let generation = {
            let kept = lock(cached);
            if let Some(c) = kept.live() {
                return Ok(c.clone());
            }
            kept.generation
        };
        let to = route()?;
        let c = tokio::time::timeout(crate::code::STEP, node.endpoint.connect(to, WEB_ALPN))
            .await
            .map_err(|_| RelayError::Unreachable {
                reason: "the other device didn't answer".into(),
            })?
            .map_err(|e| RelayError::Unreachable {
                reason: e.to_string(),
            })?;
        if let Some(c) = keep(cached, generation, c) {
            return Ok(c);
        }
    }
    Err(RelayError::Unreachable {
        reason: "the way to the device kept changing; try again".into(),
    })
}

/// Keeps `c`, dialled in `generation`. A connection closed since then (a
/// revocation, a change of Connections) means its route may no longer be
/// allowed, so it isn't kept. Another request that dialled at the same
/// time keeps its own: one connection, so closing it closes every one in
/// use.
fn keep(cached: &Mutex<Kept>, generation: u64, c: Connection) -> Option<Connection> {
    let mut kept = lock(cached);
    if kept.generation != generation {
        c.close(0u32.into(), b"route changed");
        return None;
    }
    if let Some(other) = kept.live() {
        c.close(0u32.into(), b"duplicate");
        return Some(other.clone());
    }
    kept.conn = Some(c.clone());
    Some(c)
}

/// One browser connection: one request over a new stream, one response.
async fn forward(
    node: &Node,
    cached: &Mutex<Kept>,
    mut browser: TcpStream,
    host: &str,
    route: &(dyn Fn() -> Result<EndpointAddr, RelayError> + Send + Sync),
) -> Result<(), RelayError> {
    let head = read_head(&mut browser).await?;
    // Only the site's own name: another name (127.0.0.1, or a page
    // rebinding its own name here) would share cookies or reach the site
    // from somewhere else.
    let asked_for = head
        .header("host")
        .map(|h| h.rsplit_once(':').map_or(h, |(name, _)| name))
        .unwrap_or("");
    if !asked_for.eq_ignore_ascii_case(host) {
        let text = format!("Open this site at http://{host}:<port>/");
        let page = format!(
            "HTTP/1.1 421 Misdirected Request\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
            text.len()
        );
        let _ = browser.write_all(page.as_bytes()).await;
        let _ = browser.shutdown().await;
        return Ok(());
    }
    // No way to it now (revoked, or not allowed by Connections): nothing
    // more goes to it, on any connection.
    let route_now = || route().inspect_err(|_| lock(cached).close(b"no route"));
    // Once the site's answer has started reaching the browser, a failure
    // can only end the connection: a 502 after it would read as its body.
    let answered = AtomicBool::new(false);
    let attempt = async {
        let conn = connection(node, cached, route_now).await?;
        // Asked again once the connection is kept: a revocation that
        // finished while it was being dialled couldn't close it, and one
        // that finishes from now on closes it (it is the kept one).
        route_now()?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| bad(&e.to_string()))?;
        send.write_all(&rewritten(&head, None))
            .await
            .map_err(|e| bad(&e.to_string()))?;
        let (mut from_browser, mut to_browser) = browser.split();
        let moving = Activity::new();
        let upload = async {
            copy_body(
                &mut from_browser,
                &mut send,
                &head.extra,
                head.content_length,
                &moving,
            )
            .await?;
            let _ = send.finish();
            Ok(())
        };
        let download = async {
            let mut first = [0u8; 1];
            // The first byte shows the site answered at all.
            let n = moving
                .within(recv.read(&mut first))
                .await
                .map_err(|_| bad("the site stopped answering"))?
                .map_err(|e| bad(&e.to_string()))?
                .unwrap_or(0);
            if n == 0 {
                return Err(RelayError::Refused);
            }
            moving.moved();
            answered.store(true, Ordering::SeqCst);
            to_browser
                .write_all(&first[..n])
                .await
                .map_err(|e| bad(&e.to_string()))?;
            copy_idle(&mut recv, &mut to_browser, &moving).await
        };
        both_ways(upload, download).await
    };
    let result = attempt.await;
    if let Some(err) = result
        .as_ref()
        .err()
        .filter(|_| !answered.load(Ordering::SeqCst))
    {
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

/// A header of a site request or response, for the iPhone's viewer.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SiteHeader {
    pub name: String,
    pub value: String,
}

/// One request the iPhone's in-app viewer makes of a device's site.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SiteRequest {
    pub method: String,
    /// The path and query, starting with `/`.
    pub path: String,
    pub headers: Vec<SiteHeader>,
    pub body: Vec<u8>,
}

/// The site's whole answer to a `SiteRequest`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SiteResponse {
    pub status: u16,
    pub headers: Vec<SiteHeader>,
    pub body: Vec<u8>,
}

/// The most a response to the in-app viewer may hold: it is kept whole.
const MAX_FETCHED: usize = 64 * 1024 * 1024;

/// Asks the site of the device at `to` for one request and reads the whole
/// answer (the iPhone's viewer, which has no local port).
pub(crate) async fn fetch(
    node: &Node,
    cached: &Mutex<Kept>,
    request: SiteRequest,
    route: &(dyn Fn() -> Result<EndpointAddr, RelayError> + Send + Sync),
) -> Result<SiteResponse, RelayError> {
    if !request.path.starts_with('/') || request.path.contains(['\r', '\n', ' ']) {
        return Err(bad("a site path starts with / and has no spaces"));
    }
    let method = request.method.to_ascii_uppercase();
    let is_head = method == "HEAD";
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(bad("not an HTTP method"));
    }
    let head = Head {
        method,
        path: request.path,
        headers: request
            .headers
            .into_iter()
            .filter(|h| !h.name.eq_ignore_ascii_case("content-length"))
            .filter(|h| !h.name.contains(['\r', '\n', ':']) && !h.value.contains(['\r', '\n']))
            .map(|h| (h.name, h.value))
            .chain(
                (!request.body.is_empty())
                    .then(|| ("Content-Length".to_string(), request.body.len().to_string())),
            )
            .collect(),
        extra: Vec::new(),
        content_length: request.body.len() as u64,
    };
    // As for a computer's port: the route is asked again once the
    // connection is kept, so a revocation during the dial stops it.
    let route_now = || route().inspect_err(|_| lock(cached).close(b"no route"));
    let conn = connection(node, cached, route_now).await?;
    route_now()?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| bad(&e.to_string()))?;
    let mut answer = Vec::new();
    let moving = Activity::new();
    let upload = async {
        send.write_all(&rewritten(&head, None))
            .await
            .map_err(|e| bad(&e.to_string()))?;
        write_moving(&mut send, &request.body, &moving).await?;
        let _ = send.finish();
        Ok(())
    };
    // Read while the body goes up: the site may answer (and stop reading)
    // before it has the whole body.
    let download = async {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = moving
                .within(recv.read(&mut chunk))
                .await
                .map_err(|_| bad("the site stopped answering"))?
                .map_err(|e| bad(&e.to_string()))?
                .unwrap_or(0);
            if n == 0 {
                return Ok(answer.len() as u64);
            }
            moving.moved();
            if answer.len() + n > MAX_FETCHED {
                return Err(bad("the page is too big to show"));
            }
            answer.extend_from_slice(&chunk[..n]);
        }
    };
    both_ways(upload, download).await?;
    if answer.is_empty() {
        return Err(RelayError::Refused);
    }
    parse_response(&answer, is_head)
}

/// A whole HTTP/1.1 response: status, headers, and the body, unchunked.
/// Informational answers (`100 Continue`, `103 Early Hints`) before it
/// are skipped. A body shorter than its `Content-Length` is an error,
/// except for a `HEAD` request (`head`), whose answer has none.
pub(crate) fn parse_response(mut answer: &[u8], head: bool) -> Result<SiteResponse, RelayError> {
    let mut parsed = [httparse::EMPTY_HEADER; 100];
    let mut response = httparse::Response::new(&mut parsed);
    let mut end;
    loop {
        end = match response.parse(answer) {
            Ok(httparse::Status::Complete(end)) => end,
            _ => return Err(bad("the site's answer isn't HTTP")),
        };
        if !matches!(response.code, Some(100..=199)) {
            break;
        }
        answer = &answer[end..];
        response = httparse::Response::new(&mut parsed);
    }
    let status = response.code.unwrap_or(502);
    let mut chunked = false;
    let mut length = None;
    let mut headers = Vec::new();
    for h in response.headers.iter() {
        let value = String::from_utf8_lossy(h.value).trim().to_string();
        if h.name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
            continue;
        }
        if h.name.eq_ignore_ascii_case("content-length") {
            length = value.parse::<usize>().ok();
        }
        if HOP_HEADERS.iter().any(|n| h.name.eq_ignore_ascii_case(n)) {
            continue;
        }
        headers.push(SiteHeader {
            name: h.name.to_string(),
            value,
        });
    }
    let rest = &answer[end..];
    let body = if head || matches!(status, 204 | 304) {
        Vec::new()
    } else if chunked {
        let body = unchunk(rest)?;
        headers.retain(|h| !h.name.eq_ignore_ascii_case("content-length"));
        headers.push(SiteHeader {
            name: "Content-Length".into(),
            value: body.len().to_string(),
        });
        body
    } else {
        match length {
            Some(n) if n > rest.len() => return Err(bad("the site's answer ended early")),
            Some(n) => rest[..n].to_vec(),
            None => rest.to_vec(),
        }
    };
    Ok(SiteResponse {
        status,
        headers,
        body,
    })
}

/// A chunked body, joined.
fn unchunk(mut rest: &[u8]) -> Result<Vec<u8>, RelayError> {
    let mut body = Vec::new();
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| bad("a chunk ended early"))?;
        let line = std::str::from_utf8(&rest[..line_end]).map_err(|_| bad("a bad chunk"))?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| bad("a bad chunk size"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            // Trailer fields, if any, then the empty line that ends them.
            loop {
                let end = rest
                    .windows(2)
                    .position(|w| w == b"\r\n")
                    .ok_or_else(|| bad("a chunked answer ended early"))?;
                if end == 0 {
                    return Ok(body);
                }
                rest = &rest[end + 2..];
            }
        }
        if size.checked_add(2).is_none_or(|need| rest.len() < need) {
            return Err(bad("a chunk ended early"));
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
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
            // An index that is a symlink out of the folder too.
            let other = dir.path().join("other");
            std::fs::create_dir_all(&other).unwrap();
            std::os::unix::fs::symlink(dir.path().join("secret.txt"), other.join("index.html"))
                .unwrap();
            assert!(file_for(&other, "/").is_none(), "a symlinked index");
            std::fs::create_dir_all(root.join("sub")).unwrap();
            std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("sub/index.html"))
                .unwrap();
            assert!(ok("/sub/").is_none(), "a symlinked index under the folder");
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

    #[test]
    fn the_viewer_reads_plain_and_chunked_answers() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
        let r = parse_response(plain, false).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello");
        assert!(r.headers.iter().any(|h| h.name == "Content-Type"));
        assert!(!r.headers.iter().any(|h| h.name == "Connection"));

        let chunked = b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nnot \r\n5;x=1\r\nfound\r\n0\r\n\r\n";
        let r = parse_response(chunked, false).unwrap();
        assert_eq!(r.status, 404);
        assert_eq!(r.body, b"not found");
        assert!(r
            .headers
            .iter()
            .any(|h| h.name == "Content-Length" && h.value == "9"));

        assert!(parse_response(b"nonsense", false).is_err());
        assert!(parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\nshort",
            false
        )
        .is_err());
        // A chunk size too big to add to doesn't overflow.
        assert!(parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\nx",
            false
        )
        .is_err());

        // Early hints come before the real answer.
        let hinted = b"HTTP/1.1 103 Early Hints\r\nLink: </a.css>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        let r = parse_response(hinted, false).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"ok");

        // A body shorter than it said is an error, except for HEAD.
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhalf";
        assert!(parse_response(short, false).is_err());
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n", true).unwrap();
        assert!(r.body.is_empty());

        // A chunked answer ends with the line after its trailers.
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n";
        assert!(parse_response(cut, false).is_err());
        let trailers = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\nX-Sum: 1\r\n\r\n";
        assert_eq!(parse_response(trailers, false).unwrap().body, b"ok");
        // HEAD, 204 and 304 have no body, even when chunked.
        let chunked_head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(parse_response(chunked_head, true).unwrap().body.is_empty());
        let not_modified = b"HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(parse_response(not_modified, false).unwrap().status, 304);
    }

    /// A step waiting on one direction isn't idle while the other moves.
    #[test]
    fn a_transfer_is_idle_only_when_nothing_moves_either_way() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();
        rt.block_on(async {
            let moving = Activity::new();
            let waiting = moving.within(tokio::time::sleep(IDLE * 3));
            let uploading = async {
                for _ in 0..8 {
                    tokio::time::sleep(IDLE / 2).await;
                    moving.moved();
                }
            };
            let (waited, ()) = tokio::join!(waiting, uploading);
            assert!(waited.is_ok(), "the upload kept it alive");
            // Nothing moving at all: idle after IDLE.
            assert!(moving.within(tokio::time::sleep(IDLE * 3)).await.is_err());
        });

        // An end of the data isn't progress: it doesn't put off the limit.
        rt.block_on(async {
            let moving = Activity::new();
            tokio::time::sleep(IDLE * 9 / 10).await;
            let mut empty: &[u8] = b"";
            assert_eq!(
                copy_idle(&mut empty, &mut tokio::io::sink(), &moving)
                    .await
                    .unwrap(),
                0
            );
            let started = tokio::time::Instant::now();
            assert!(moving.within(tokio::time::sleep(IDLE)).await.is_err());
            assert!(started.elapsed() <= IDLE / 10);
        });

        // A slow reader that keeps taking a byte every half IDLE isn't
        // idle, though the whole write takes longer than IDLE.
        rt.block_on(async {
            let moving = Activity::new();
            let (mut writer, mut reader) = tokio::io::duplex(1);
            let slow_reader = async {
                let mut got = Vec::new();
                let mut byte = [0u8; 1];
                while reader.read(&mut byte).await.unwrap() == 1 {
                    got.push(byte[0]);
                    tokio::time::sleep(IDLE / 2).await;
                }
                got
            };
            let writing = async {
                let mut from: &[u8] = b"abcdef";
                let copied = copy_idle(&mut from, &mut writer, &moving).await;
                drop(writer);
                copied
            };
            let (copied, got) = tokio::join!(writing, slow_reader);
            assert_eq!(copied.unwrap(), 6);
            assert_eq!(got, b"abcdef");
        });
    }

    #[test]
    fn every_device_has_its_own_site_name() {
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
        let a = format!("{}{}", "ab".repeat(16), "00".repeat(16));
        let b = format!("{}{}", "ab".repeat(16), "01".repeat(16));
        let (ha, hb) = (site_host(&a), site_host(&b));
        assert_ne!(ha, hb, "ids sharing a long prefix get different names");
        let label = ha.strip_suffix(".localhost").unwrap();
        assert_eq!(label.len(), 52);
        assert!(label
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(site_host(&a.to_uppercase()), ha);
    }

    /// A device revoked while its connection was being dialled gets no
    /// request: trust is asked again once the connection is in place.
    #[test]
    fn a_revocation_during_the_dial_stops_the_request() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        crate::code::TOKIO.block_on(async {
            let start = |name: &str| {
                let key_dir = dir.path().join(name);
                async move {
                    Arc::new(
                        Node::start(
                            &key_dir,
                            &key_dir.join("store"),
                            Arc::new(crate::device::NoDevices),
                            crate::device::NodeLink::default(),
                            &crate::resolver::RelayServers::default(),
                        )
                        .await
                        .unwrap(),
                    )
                }
            };
            let host = start("host").await;
            let viewer = start("viewer").await;
            // Trusted when the request starts, revoked by the time the
            // dial has finished.
            let asked = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&asked);
            let addr = host.endpoint.addr();
            let route: SiteRoute = Arc::new(move || {
                if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(addr.clone())
                } else {
                    Err(RelayError::Declined {
                        reason: "That device is no longer trusted here.".into(),
                    })
                }
            });
            let cached: Arc<Mutex<Kept>> = Arc::default();
            let name = site_host(&host.id().to_string());
            let tunnel = open_tunnel(
                Arc::clone(&viewer),
                Arc::clone(&cached),
                0,
                name.clone(),
                route,
            )
            .await
            .unwrap();
            let mut browser = TcpStream::connect(("127.0.0.1", tunnel.port))
                .await
                .unwrap();
            browser
                .write_all(format!("GET / HTTP/1.1\r\nHost: {name}\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut answer = String::new();
            browser.read_to_string(&mut answer).await.unwrap();
            assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
            assert!(answer.contains("no longer trusted"), "{answer}");
            assert_eq!(
                asked.load(Ordering::SeqCst),
                2,
                "asked before and after the dial"
            );
            assert!(
                lock(&cached).conn.is_none(),
                "the dialled connection is closed"
            );

            // A dial still running when the kept connection was closed (a
            // revocation, a change of Connections) isn't kept: its route
            // may no longer be allowed.
            let generation = lock(&cached).generation;
            let late = viewer
                .endpoint
                .connect(host.endpoint.addr(), WEB_ALPN)
                .await
                .unwrap();
            lock(&cached).close(b"closed");
            assert!(keep(&cached, generation, late.clone()).is_none());
            assert!(lock(&cached).conn.is_none());
            assert!(late.close_reason().is_some(), "and it is closed");

            // The iPhone's fetch asks again after the dial too.
            let asked = AtomicUsize::new(0);
            let addr = host.endpoint.addr();
            let route = move || {
                if asked.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(addr.clone())
                } else {
                    Err(RelayError::Declined {
                        reason: "That device is no longer trusted here.".into(),
                    })
                }
            };
            let request = SiteRequest {
                method: "GET".into(),
                path: "/".into(),
                headers: vec![],
                body: vec![],
            };
            let fetched = fetch(&viewer, &cached, request, &route).await;
            assert!(
                matches!(fetched, Err(RelayError::Declined { .. })),
                "{fetched:?}"
            );
            assert!(lock(&cached).conn.is_none());
            drop(tunnel);
            viewer.shutdown().await;
            host.shutdown().await;
        });
    }
}
