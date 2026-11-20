//! Full articles (slice C4): the readable part of an article's own web
//! page, for feeds that send only a summary.
//!
//! The page is fetched like a feed (forge-net, URLSession on iOS) and its
//! main text found by `dom_smoothie`, a Rust port of Mozilla's
//! Readability. What comes back is HTML that ForgeReader lays out like a
//! feed's own text; nothing in the page runs.
//!
//! An article's address comes from the feed, so it is untrusted: before a
//! page is fetched, [`check_destination`] refuses addresses on this
//! device or its networks (loopback, private, link-local, carrier-grade
//! NAT such as Tailscale, `.local` names and the like), both as written
//! and as the name resolves. The one exception is the feed's own host,
//! which the person chose by subscribing. The body is read as a stream
//! and stops at [`MAX_PAGE_BYTES`], and the whole fetch, body included,
//! has a deadline.
//!
//! Redirects are not checked hop by hop: both transports (URLSession and
//! ureq) follow them internally, and `forge_net` has no way to turn that
//! off yet. A redirect answer that does reach Commonplace is refused, not
//! followed (see `docs/research/android-ports.md`, C4).

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, NetError, Request};

use crate::model::AppError;

/// The largest page read. Pages bigger than this are almost never an
/// article.
pub const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;
/// How long one page may take, from asking to the last byte.
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
/// Elements read from one page, so a huge page can't take long to score.
const MAX_ELEMENTS: usize = 20_000;
/// Less text than this is no article (a menu, a sign-in prompt): the
/// feed's own text is kept instead.
const MIN_TEXT_CHARS: usize = 200;

/// Finds the addresses a host name stands for (the system resolver in
/// the app; a fixed table in tests, which have no network).
pub type Lookup = fn(host: &str, port: u16) -> std::io::Result<Vec<IpAddr>>;

/// The system resolver.
pub fn system_lookup(host: &str, port: u16) -> std::io::Result<Vec<IpAddr>> {
    Ok((host, port).to_socket_addrs()?.map(|a| a.ip()).collect())
}

/// How one page is fetched: limits and the resolver. Tests shrink them.
#[derive(Clone, Copy)]
pub struct Policy {
    pub max_bytes: usize,
    pub timeout: Duration,
    pub lookup: Lookup,
}

impl Policy {
    /// The limits the app uses, with `lookup` for host names.
    pub fn new(lookup: Lookup) -> Self {
        Self {
            max_bytes: MAX_PAGE_BYTES,
            timeout: PAGE_TIMEOUT,
            lookup,
        }
    }
}

/// Fetches the page at `url` and returns its article as HTML. `feed_url`
/// is the address of the feed the article came from: its host may be on
/// a private network, because the person subscribed to it there.
pub fn fetch_article(
    http: &dyn HttpClient,
    url: &str,
    feed_url: Option<&str>,
    policy: &Policy,
    cancel: &CancelHandle,
) -> Result<String, AppError> {
    let url = check_destination(url, feed_url, policy.lookup)?;
    let request = Request::get(url.as_str())
        .timeout(policy.timeout)
        .header(
            "Accept",
            "text/html, application/xhtml+xml;q=0.9, */*;q=0.1",
        )
        .header("User-Agent", "Commonplace/0.1 (ixOS)");
    let page = read_page(http, &request, policy, cancel)?;
    extract(&String::from_utf8_lossy(&page), url.as_str())
}

/// The address to fetch, written the way the transport will read it, or
/// why it may not be fetched.
pub fn check_destination(
    url: &str,
    feed_url: Option<&str>,
    lookup: Lookup,
) -> Result<url::Url, AppError> {
    let refused = |reason: &str| AppError::NoReadableText {
        reason: reason.into(),
    };
    let parsed = url::Url::parse(url.trim()).map_err(|_| refused("not a web address"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(refused("not a web address"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(refused("the address carries a user name or password"));
    }
    let host = parsed.host().ok_or_else(|| refused("not a web address"))?;
    let feed_host = feed_url
        .and_then(|f| url::Url::parse(f.trim()).ok())
        .and_then(|f| f.host().map(|h| h.to_owned()));
    if feed_host.as_ref() == Some(&host.to_owned()) {
        return Ok(parsed);
    }
    let local = || refused("the page is on this device or a local network");
    match host {
        url::Host::Ipv4(ip) if !is_public(IpAddr::V4(ip)) => return Err(local()),
        url::Host::Ipv6(ip) if !is_public(IpAddr::V6(ip)) => return Err(local()),
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => {}
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            if is_local_name(&name) {
                return Err(local());
            }
            let port = parsed.port_or_known_default().unwrap_or(443);
            let addresses = lookup(&name, port).map_err(|_| AppError::Network {
                reason: "the page's address could not be looked up".into(),
            })?;
            if addresses.is_empty() {
                return Err(AppError::Network {
                    reason: "the page's address could not be looked up".into(),
                });
            }
            if !addresses.into_iter().all(is_public) {
                return Err(local());
            }
        }
    }
    Ok(parsed)
}

/// Names that only mean something on this device or its own network.
fn is_local_name(name: &str) -> bool {
    const SUFFIXES: [&str; 7] = [
        ".localhost",
        ".local",
        ".lan",
        ".home",
        ".internal",
        ".home.arpa",
        ".ts.net",
    ];
    name == "localhost" || !name.contains('.') || SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// Whether `ip` is an ordinary address on the internet: not this device,
/// a private or link-local network, carrier-grade NAT (Tailscale), a
/// multicast group or a reserved range.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || a == 0
        || a >= 240
        // Carrier-grade NAT, 100.64.0.0/10: Tailscale addresses live here.
        || (a == 100 && (64..128).contains(&b))
        // IETF protocol assignments, 192.0.0.0/24.
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking, 198.18.0.0/15.
        || (a == 198 && (b == 18 || b == 19)))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // An IPv4 address carried inside IPv6 is judged as IPv4: mapped
    // (::ffff:a.b.c.d), NAT64 (64:ff9b::a.b.c.d, which iPhones on
    // IPv6-only networks are given for every IPv4 site) and 6to4.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        return is_public_v4(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            s[6] as u8,
            (s[7] >> 8) as u8,
            s[7] as u8,
        ));
    }
    if s[0] == 0x2002 {
        return is_public_v4(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // Unique local, fc00::/7.
        || (s[0] & 0xfe00) == 0xfc00
        // Link-local, fe80::/10, and the old site-local, fec0::/10.
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        // IPv4-compatible (deprecated) and other ::/96 forms.
        || s[0..6] == [0, 0, 0, 0, 0, 0])
}

/// The page's bytes, read as they arrive: a body that grows past
/// `policy.max_bytes` (or says it will) stops there, and the whole fetch,
/// body included, stops at `policy.timeout`.
fn read_page(
    http: &dyn HttpClient,
    request: &Request,
    policy: &Policy,
    cancel: &CancelHandle,
) -> Result<Vec<u8>, AppError> {
    // Its own handle, so the deadline can stop this fetch alone.
    let (fetch, _link) = cancel.child();
    let timed_out = Arc::new(AtomicBool::new(false));
    let deadline = {
        let fetch = fetch.clone();
        let timed_out = Arc::clone(&timed_out);
        let timeout = policy.timeout;
        std::thread::Builder::new()
            .name("full-article-deadline".into())
            .spawn(move || {
                if !fetch.wait_timeout(timeout) {
                    timed_out.store(true, Ordering::SeqCst);
                    fetch.cancel();
                }
            })
            .map_err(|_| AppError::Network {
                reason: "could not start the fetch".into(),
            })?
    };
    let result = read_body(http, request, policy.max_bytes, &fetch);
    // Wakes the deadline thread if it is still waiting.
    fetch.cancel();
    let _ = deadline.join();
    match result {
        Err(_) if cancel.is_cancelled() => Err(AppError::Cancelled),
        Err(_) if timed_out.load(Ordering::SeqCst) => Err(AppError::Timeout),
        other => other,
    }
}

fn read_body(
    http: &dyn HttpClient,
    request: &Request,
    max_bytes: usize,
    cancel: &CancelHandle,
) -> Result<Vec<u8>, AppError> {
    let too_large = || AppError::NoReadableText {
        reason: "page too large".into(),
    };
    let mut response = http.open_stream_cancellable(request, cancel)?;
    if !response.is_success() {
        // Includes redirects a transport hands back instead of following:
        // they are not followed here.
        return Err(AppError::Http {
            status: response.status,
        });
    }
    if let Some(kind) = response.header("Content-Type") {
        let kind = kind.to_ascii_lowercase();
        if !(kind.contains("html") || kind.contains("xml") || kind.starts_with("text/")) {
            return Err(AppError::NoReadableText {
                reason: "not a web page".into(),
            });
        }
    }
    if let Some(length) = response
        .header("Content-Length")
        .and_then(|l| l.trim().parse::<u64>().ok())
    {
        if length > max_bytes as u64 {
            return Err(too_large());
        }
    }
    let mut page = Vec::new();
    (&mut response.body)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut page)
        .map_err(|e| {
            if forge_net::is_cancelled_io_error(&e) {
                AppError::Cancelled
            } else {
                AppError::from(NetError::Transport {
                    reason: e.to_string(),
                })
            }
        })?;
    if page.len() > max_bytes {
        return Err(too_large());
    }
    Ok(page)
}

/// The article in a page's HTML, as HTML. `url` resolves its relative
/// links and pictures.
pub fn extract(page: &str, url: &str) -> Result<String, AppError> {
    let config = dom_smoothie::Config {
        max_elements_to_parse: MAX_ELEMENTS,
        ..Default::default()
    };
    let mut readability =
        dom_smoothie::Readability::new(page, Some(url), Some(config)).map_err(|e| {
            AppError::NoReadableText {
                reason: e.to_string(),
            }
        })?;
    let article = readability.parse().map_err(|e| AppError::NoReadableText {
        reason: e.to_string(),
    })?;
    let html = article.content.to_string();
    if article.text_content.trim().chars().count() < MIN_TEXT_CHARS {
        return Err(AppError::NoReadableText {
            reason: "too little text".into(),
        });
    }
    Ok(html)
}
