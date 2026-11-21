//! Full articles (slice C4): the readable part of an article's own web
//! page, for feeds that send only a summary.
//!
//! The page is fetched like a feed (forge-net, URLSession on iOS) and its
//! main text found by `dom_smoothie`, a Rust port of Mozilla's
//! Readability. What comes back is HTML that ForgeReader lays out like a
//! feed's own text; nothing in the page runs.
//!
//! An article's address comes from the feed, so it is untrusted. Before
//! any request, [`check_destination`] refuses addresses on this device or
//! its networks (loopback, private, link-local, carrier-grade NAT such as
//! Tailscale, `.local` names and the like), both as written and as the
//! name resolves. Redirects are not left to the transport: each request
//! asks it not to follow them, and Commonplace follows at most
//! [`MAX_REDIRECTS`] itself, checking every new address the same way
//! before asking there, and refusing a loop. The body is read as a stream
//! that stops at [`MAX_PAGE_BYTES`], and the whole fetch, redirects and
//! body included, has one deadline.
//!
//! One explicit exception, [`HomeFeed`]: when the person subscribed to a
//! feed on their own network (a home server, a tailnet), pages at exactly
//! that feed's origin (scheme, host and port) may be fetched although
//! they are local. Even then the host's addresses are looked up and
//! checked: private and carrier-grade NAT ranges pass, but never this
//! device (loopback), link-local (cloud metadata), multicast or reserved
//! ranges. A feed on a public host gets no exception at all.

use std::collections::HashSet;
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
/// Redirects followed for one page before giving up.
pub const MAX_REDIRECTS: usize = 5;
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
/// is the address of the feed the article came from (see [`HomeFeed`]).
pub fn fetch_article(
    http: &dyn HttpClient,
    url: &str,
    feed_url: Option<&str>,
    policy: &Policy,
    cancel: &CancelHandle,
) -> Result<String, AppError> {
    let home = feed_url.and_then(|f| HomeFeed::of(f, policy.lookup));
    let url = check(url, home.as_ref(), policy.lookup)?;
    let (url, page) = with_deadline(policy.timeout, cancel, |cancel| {
        follow(http, url, home.as_ref(), policy, cancel)
    })?;
    extract(&String::from_utf8_lossy(&page), url.as_str())
}

/// The origin of a feed the person subscribed to on their own network:
/// pages there may be fetched although the address is local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeFeed {
    origin: url::Origin,
}

impl HomeFeed {
    /// `Some` only when the feed's own address is on a local network (as
    /// written, or as its name resolves); a public feed has no exception.
    pub fn of(feed_url: &str, lookup: Lookup) -> Option<Self> {
        let feed = url::Url::parse(feed_url.trim()).ok()?;
        let local = match feed.host()? {
            url::Host::Ipv4(ip) => !is_public(IpAddr::V4(ip)),
            url::Host::Ipv6(ip) => !is_public(IpAddr::V6(ip)),
            url::Host::Domain(name) => {
                let name = name.trim_end_matches('.').to_ascii_lowercase();
                is_local_name(&name)
                    || lookup(&name, feed.port_or_known_default()?)
                        .map(|ips| ips.into_iter().any(|ip| !is_public(ip)))
                        .unwrap_or(false)
            }
        };
        local.then(|| Self {
            origin: feed.origin(),
        })
    }
}

/// The address to fetch, written the way the transport will read it, or
/// why it may not be fetched. `feed_url` is the article's feed. (The
/// fetch itself checks with its [`HomeFeed`] worked out once.)
#[cfg(test)]
pub fn check_destination(
    url: &str,
    feed_url: Option<&str>,
    lookup: Lookup,
) -> Result<url::Url, AppError> {
    let home = feed_url.and_then(|f| HomeFeed::of(f, lookup));
    check(url, home.as_ref(), lookup)
}

fn check(url: &str, home: Option<&HomeFeed>, lookup: Lookup) -> Result<url::Url, AppError> {
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
    let at_home = home.is_some_and(|h| h.origin == parsed.origin());
    // Which addresses may be asked: anything public, and at the home
    // feed's origin, private networks too (never this device itself).
    let allowed = |ip: IpAddr| is_public(ip) || (at_home && is_home_network(ip));
    let local = || refused("the page is on this device or a local network");
    let lookup_failed = || AppError::Network {
        reason: "the page's address could not be looked up".into(),
    };
    match parsed.host().ok_or_else(|| refused("not a web address"))? {
        url::Host::Ipv4(ip) => {
            if !allowed(IpAddr::V4(ip)) {
                return Err(local());
            }
        }
        url::Host::Ipv6(ip) => {
            if !allowed(IpAddr::V6(ip)) {
                return Err(local());
            }
        }
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            if is_local_name(&name) && !at_home {
                return Err(local());
            }
            let port = parsed.port_or_known_default().unwrap_or(443);
            let addresses = lookup(&name, port).map_err(|_| lookup_failed())?;
            if addresses.is_empty() {
                return Err(lookup_failed());
            }
            if !addresses.into_iter().all(allowed) {
                return Err(local());
            }
        }
    }
    Ok(parsed)
}

/// A private network a home server can be on: RFC 1918, carrier-grade
/// NAT (Tailscale) and IPv6 unique-local. Not loopback, link-local,
/// multicast or reserved ranges.
fn is_home_network(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_private() || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_home_network(IpAddr::V4(v4)),
            None => (v6.segments()[0] & 0xfe00) == 0xfc00,
        },
    }
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

/// Runs `work` with its own cancel handle that is cancelled at
/// `timeout`: a fetch then fails with [`AppError::Timeout`], however far
/// it got (redirects, a body that trickles in).
fn with_deadline<T>(
    timeout: Duration,
    cancel: &CancelHandle,
    work: impl FnOnce(&CancelHandle) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let (fetch, _link) = cancel.child();
    let timed_out = Arc::new(AtomicBool::new(false));
    let deadline = {
        let fetch = fetch.clone();
        let timed_out = Arc::clone(&timed_out);
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
    let result = work(&fetch);
    // Wakes the deadline thread if it is still waiting.
    fetch.cancel();
    let _ = deadline.join();
    match result {
        Err(_) if cancel.is_cancelled() => Err(AppError::Cancelled),
        Err(_) if timed_out.load(Ordering::SeqCst) => Err(AppError::Timeout),
        other => other,
    }
}

/// Asks for `url` with redirects off, follows up to [`MAX_REDIRECTS`]
/// itself (checking each new address first), and returns the address
/// that answered with the page, and the page.
fn follow(
    http: &dyn HttpClient,
    mut url: url::Url,
    home: Option<&HomeFeed>,
    policy: &Policy,
    cancel: &CancelHandle,
) -> Result<(url::Url, Vec<u8>), AppError> {
    let mut visited = HashSet::from([url.to_string()]);
    for _ in 0..=MAX_REDIRECTS {
        let request = Request::get(url.as_str())
            .timeout(policy.timeout)
            .follow_redirects(false)
            .header(
                "Accept",
                "text/html, application/xhtml+xml;q=0.9, */*;q=0.1",
            )
            .header("User-Agent", "Commonplace/0.1 (ixOS)");
        let response = http.open_stream_cancellable(&request, cancel)?;
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let page = read_body(response, policy.max_bytes)?;
            return Ok((url, page));
        }
        let status = response.status;
        let next = response
            .header("Location")
            .and_then(|to| url.join(to.trim()).ok())
            .ok_or(AppError::Http { status })?;
        drop(response);
        let next = check(next.as_str(), home, policy.lookup)?;
        if !visited.insert(next.to_string()) {
            return Err(AppError::NoReadableText {
                reason: "the page redirects in a loop".into(),
            });
        }
        url = next;
    }
    Err(AppError::NoReadableText {
        reason: "the page redirects too many times".into(),
    })
}

/// The body of a page's answer, read as it arrives: a body that grows
/// past `max_bytes` (or says it will) stops there.
fn read_body(
    mut response: forge_net::StreamingResponse,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    let too_large = || AppError::NoReadableText {
        reason: "page too large".into(),
    };
    if !response.is_success() {
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
