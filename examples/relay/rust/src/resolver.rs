//! The connection resolver (brief M2 PR 6): which addresses a trusted
//! device is dialled at, which kind of route a connection used, and, when
//! nothing answers, why, in words.
//!
//! Every dial to a trusted device goes through [`plan`]. It puts together
//! what Relay knows about where the device might be:
//!
//! - **nearby:** what Bonjour sees now, on this network;
//! - **told:** the addresses the device said it has, the last time the two
//!   met (in [`crate::device::DeviceMsg::Hello`]), which include its
//!   Tailscale address when it has one;
//! - **remembered:** where it was last reached.
//!
//! Each address is a [`RouteKind`]: the local network, a private network
//! (Tailscale), or the internet. The [`ConnectionMode`] setting limits which
//! kinds are tried. iroh dials the remaining addresses together and keeps
//! the best path; [`route_of`] names the one it chose.
//!
//! A key says who a device is, never where it is: whichever address
//! answers, the connection's endpoint id must still match the trusted key.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use serde::{Deserialize, Serialize};

/// The most addresses kept for a device: enough for a laptop's Wi-Fi,
/// Ethernet, Tailscale and IPv6 addresses, without letting a device fill
/// the record.
pub(crate) const MAX_ADDRESSES: usize = 12;

/// Which kinds of route Relay may use to reach a trusted device.
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, uniffi::Enum,
)]
pub enum ConnectionMode {
    /// Only devices on the same local network.
    Local,
    /// The local network, then private networks such as Tailscale. Never
    /// over the open internet, and never through a relay server.
    Private,
    /// Every route Relay has: local, private, direct over the internet, and
    /// Relay's own relay server. The only mode that uses a relay.
    #[default]
    Hybrid,
}

impl ConnectionMode {
    pub(crate) fn allows(self, kind: RouteKind) -> bool {
        match self {
            ConnectionMode::Local => kind == RouteKind::Local,
            ConnectionMode::Private => matches!(kind, RouteKind::Local | RouteKind::Private),
            ConnectionMode::Hybrid => true,
        }
    }
}

/// The kind of route an address, or a connection, uses.
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, uniffi::Enum,
)]
pub enum RouteKind {
    /// The same local network (Wi-Fi or Ethernet at home).
    Local,
    /// A private network between the devices, such as Tailscale.
    Private,
    /// Directly over the internet.
    Internet,
    /// Through a relay server (none yet: PR 7).
    Relay,
}

impl RouteKind {
    /// What the app shows for a connection on this route.
    pub fn label(self) -> &'static str {
        match self {
            RouteKind::Local => "Direct, local network",
            RouteKind::Private => "Private network",
            RouteKind::Internet => "Direct, internet",
            RouteKind::Relay => "Via relay",
        }
    }
}

/// What a device publishes to the lookup server, which anyone who knows its
/// endpoint id can read (and ids aren't secret: every connection and
/// Bonjour record shows one). With a relay, only the relay: a trusted
/// device connects through it, and the two find a direct path from there.
/// Without one, only public internet addresses, and never an IPv6 address
/// made from the network card's hardware address (EUI-64), which would
/// name this machine wherever it goes. Home-network and Tailscale
/// addresses go only to trusted devices, in Hello.
pub(crate) fn publishable(addrs: &[iroh::TransportAddr]) -> Vec<iroh::TransportAddr> {
    let relays: Vec<_> = addrs.iter().filter(|a| a.is_relay()).cloned().collect();
    if !relays.is_empty() {
        return relays;
    }
    addrs
        .iter()
        .filter(|a| match a {
            iroh::TransportAddr::Ip(addr) => {
                address_kind(addr) == RouteKind::Internet && !from_hardware(addr)
            }
            _ => false,
        })
        .cloned()
        .collect()
}

/// An IPv6 address whose second half is made from a MAC address
/// (`xx:xxff:fexx:xxxx`, EUI-64).
fn from_hardware(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V6(ip) => {
            let s = ip.segments();
            ip.to_ipv4_mapped().is_none() && (s[5] & 0x00ff) == 0x00ff && (s[6] >> 8) == 0xfe
        }
        IpAddr::V4(_) => false,
    }
}

/// The kind of route `addr` is on, from here: a public IPv6 address in
/// one of this device's own /64 networks is the same Wi-Fi or LAN, so the
/// local network.
pub(crate) fn kind_of(addr: &SocketAddr) -> RouteKind {
    match (address_kind(addr), addr.ip()) {
        (RouteKind::Internet, IpAddr::V6(ip)) if on_my_network(ip) => RouteKind::Local,
        (kind, _) => kind,
    }
}

/// The kind of network `addr` belongs to, wherever it is seen from.
fn address_kind(addr: &SocketAddr) -> RouteKind {
    match addr.ip() {
        IpAddr::V4(ip) => kind_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => kind_v4(v4),
            None => kind_v6(ip),
        },
    }
}

fn kind_v4(ip: Ipv4Addr) -> RouteKind {
    let [a, b, ..] = ip.octets();
    if a == 100 && (64..128).contains(&b) {
        // 100.64.0.0/10, which Tailscale uses.
        RouteKind::Private
    } else if ip.is_private() || ip.is_link_local() || ip.is_loopback() || never_on_the_internet(ip)
    {
        RouteKind::Local
    } else {
        RouteKind::Internet
    }
}

/// Blocks set aside for documentation (192.0.2.0/24, 198.51.100.0/24,
/// 203.0.113.0/24), benchmarking (198.18.0.0/15) and future use
/// (240.0.0.0/4), which no internet route reaches. Some test and lab
/// networks give machines such addresses.
fn never_on_the_internet(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    matches!((a, b, c), (192, 0, 2) | (198, 51, 100) | (203, 0, 113))
        || (a == 198 && (b & 0xfe) == 18)
        || a >= 240
}

fn kind_v6(ip: Ipv6Addr) -> RouteKind {
    let s = ip.segments();
    if s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0 {
        // fd7a:115c:a1e0::/48, Tailscale's IPv6 range.
        RouteKind::Private
    } else if ip.is_loopback()
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] == 0x2001 && s[1] == 0x0db8)
    {
        // Link-local, other unique local addresses, and 2001:db8::/32,
        // set aside for documentation.
        RouteKind::Local
    } else {
        RouteKind::Internet
    }
}

/// The /64 networks of this device's own global IPv6 addresses (the first
/// four segments), so a device on the same Wi-Fi reached by its public
/// IPv6 address counts as the local network, not the internet.
static MY_V6_NETWORKS: std::sync::RwLock<Vec<[u16; 4]>> = std::sync::RwLock::new(Vec::new());

/// Records this device's own addresses (when the endpoint starts and
/// whenever its addresses change).
pub(crate) fn learn_own_addresses<'a>(addrs: impl IntoIterator<Item = &'a SocketAddr>) {
    let mut networks: Vec<[u16; 4]> = addrs
        .into_iter()
        .filter_map(|a| match a.ip() {
            IpAddr::V6(ip)
                if ip.to_ipv4_mapped().is_none() && (ip.segments()[0] & 0xe000) == 0x2000 =>
            {
                let s = ip.segments();
                Some([s[0], s[1], s[2], s[3]])
            }
            _ => None,
        })
        .collect();
    networks.sort_unstable();
    networks.dedup();
    if let Ok(mut mine) = MY_V6_NETWORKS.write() {
        // Several endpoints can run in one process (tests, the CLI's
        // devices): every one of them is on these networks.
        for n in networks {
            if !mine.contains(&n) {
                mine.push(n);
            }
        }
    }
}

fn on_my_network(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let network = [s[0], s[1], s[2], s[3]];
    MY_V6_NETWORKS
        .read()
        .map(|mine| mine.contains(&network))
        .unwrap_or(false)
}

/// Whether this device should tell others about `addr`: not loopback, and
/// not IPv6 link-local, which needs an interface to mean anything.
pub(crate) fn worth_telling(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(ip) => !ip.is_loopback() && !ip.is_unspecified(),
        IpAddr::V6(ip) => {
            !ip.is_loopback() && !ip.is_unspecified() && (ip.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

/// Addresses from another device, parsed, limited and without repeats.
pub(crate) fn clean(addresses: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    addresses
        .iter()
        .filter_map(|a| a.parse::<SocketAddr>().ok())
        .filter(worth_telling)
        .filter(|a| seen.insert(*a))
        .take(MAX_ADDRESSES)
        .map(|a| a.to_string())
        .collect()
}

/// Relay's own servers (brief M2 PR 7), both optional: Relay works with
/// neither, on the local and private routes.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct RelayServers {
    /// An `iroh-relay` server, for connections no direct route reaches.
    pub relay_url: Option<String>,
    /// A pkarr lookup server: each device publishes a record of where it can
    /// be reached there, signed by its key, so a trusted device finds it by
    /// key alone.
    pub lookup_url: Option<String>,
}

/// The servers Relay's maintainer runs: an iroh relay and a pkarr lookup
/// server on one machine, with TLS (Let's Encrypt, through Caddy).
const OWN_RELAY: &str = "https://relay.avedeus.ovh";
const OWN_LOOKUP: &str = "https://lookup.avedeus.ovh/pkarr";

/// Relay's own servers, for the "Use Relay's Servers" setting and
/// `relay servers --own`. Nothing uses them until they are set.
#[uniffi::export]
pub fn own_relay_servers() -> RelayServers {
    RelayServers {
        relay_url: Some(OWN_RELAY.into()),
        lookup_url: Some(OWN_LOOKUP.into()),
    }
}

impl RelayServers {
    /// The relay, if set and a valid URL.
    pub(crate) fn relay(&self) -> Option<iroh::RelayUrl> {
        self.relay_url.as_deref()?.parse().ok()
    }

    /// The lookup server, if set and a valid URL.
    pub(crate) fn lookup(&self) -> Option<url::Url> {
        self.lookup_url.as_deref()?.parse().ok()
    }

    /// The servers the endpoint is opened with in `mode`: none outside
    /// Hybrid, so in Local and Private no relay path, or lookup by key, is
    /// ever tried (decided 2026-11-19: Private means no relay).
    pub(crate) fn in_mode(&self, mode: ConnectionMode) -> RelayServers {
        match mode {
            ConnectionMode::Hybrid => self.clone(),
            ConnectionMode::Local | ConnectionMode::Private => RelayServers::default(),
        }
    }

    /// Whether a device can be looked up by key at all.
    pub(crate) fn finds_by_key(&self) -> bool {
        self.lookup().is_some()
    }

    /// Checks both URLs, in words.
    pub(crate) fn check(&self) -> Result<(), String> {
        for (what, url) in [
            ("relay", &self.relay_url),
            ("lookup server", &self.lookup_url),
        ] {
            if let Some(url) = url {
                let parsed: url::Url = url
                    .parse()
                    .map_err(|e| format!("The {what} address isn't a URL: {e}"))?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err(format!("The {what} address must start with https://"));
                }
            }
        }
        Ok(())
    }
}

/// What Relay knows about where a trusted device might be.
#[derive(Clone, Debug, Default)]
pub(crate) struct Whereabouts<'a> {
    pub name: &'a str,
    /// What Bonjour sees on this network now.
    pub nearby: &'a [String],
    /// What the device said it has when the two last met.
    pub told: &'a [String],
    /// Where it was last reached.
    pub remembered: &'a [String],
    /// Whether a lookup server can find it by key (PR 7).
    pub by_key: bool,
}

/// The addresses to dial, best first, or why there are none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub addresses: Vec<SocketAddr>,
    /// The kinds of route the plan covers, best first.
    pub kinds: Vec<RouteKind>,
}

/// The addresses to try for a device, local first, then private, then
/// internet, limited by `mode`. Err is the reason, in words, when nothing
/// is left to try.
pub(crate) fn plan(w: &Whereabouts<'_>, mode: ConnectionMode) -> Result<Plan, String> {
    let mut seen = HashSet::new();
    let mut all: Vec<(RouteKind, usize, SocketAddr)> = w
        .nearby
        .iter()
        .chain(w.told)
        .chain(w.remembered)
        .filter_map(|a| a.parse::<SocketAddr>().ok())
        .filter(|a| seen.insert(*a))
        .enumerate()
        .map(|(i, a)| (kind_of(&a), i, a))
        .collect();
    let by_key = w.by_key && mode == ConnectionMode::Hybrid;
    if all.is_empty() && w.by_key && !by_key {
        return Err(format!(
            "{} can only be found through Relay's servers, and Connections is set to {}, which never uses them.",
            w.name,
            mode_words(mode)
        ));
    }
    if all.is_empty() && !by_key {
        return Err(format!(
            "Relay hasn't met {} on a network yet. Open Relay on both devices on the same Wi-Fi once, so they learn where to find each other.",
            w.name
        ));
    }
    let known: Vec<RouteKind> = kinds(&all);
    all.retain(|(kind, _, _)| mode.allows(*kind));
    if all.is_empty() && by_key {
        // Nothing to dial directly: the lookup server says where it is.
        return Ok(Plan {
            addresses: Vec::new(),
            kinds: vec![RouteKind::Internet],
        });
    }
    if all.is_empty() {
        return Err(format!(
            "{} can only be reached {}, and Connections is set to {}.",
            w.name,
            known_ways(&known),
            mode_words(mode)
        ));
    }
    all.sort_by_key(|(kind, i, _)| (*kind, *i));
    let mut found = kinds(&all);
    if by_key && !found.contains(&RouteKind::Internet) {
        found.push(RouteKind::Internet);
    }
    Ok(Plan {
        kinds: found,
        addresses: all.into_iter().map(|(_, _, a)| a).collect(),
    })
}

fn kinds(all: &[(RouteKind, usize, SocketAddr)]) -> Vec<RouteKind> {
    let mut kinds: Vec<RouteKind> = all.iter().map(|(k, _, _)| *k).collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

fn known_ways(kinds: &[RouteKind]) -> String {
    let words: Vec<&str> = kinds
        .iter()
        .map(|k| match k {
            RouteKind::Local => "on the local network",
            RouteKind::Private => "over a private network such as Tailscale",
            RouteKind::Internet => "over the internet",
            RouteKind::Relay => "through a relay",
        })
        .collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

fn mode_words(mode: ConnectionMode) -> &'static str {
    match mode {
        ConnectionMode::Local => "Local network only",
        ConnectionMode::Private => "Local and private networks",
        ConnectionMode::Hybrid => "Any route",
    }
}

/// Why a device that had addresses to try didn't answer, in words.
pub(crate) fn no_answer(name: &str, plan: &Plan) -> String {
    let tried = known_ways(&plan.kinds);
    let hint = if plan.kinds == [RouteKind::Local] {
        " It may be away from this network: with Tailscale on both devices, Relay can reach it from anywhere once they have met."
    } else {
        ""
    };
    format!("{name} didn't answer {tried}. Is Relay open on it?{hint}")
}

/// The kind of route a connection is using now: the path iroh selected.
pub(crate) fn route_of(conn: &iroh::endpoint::Connection) -> Option<RouteKind> {
    path_of(conn).map(|(kind, _)| kind)
}

/// The selected path's kind and, when it is direct, the address it uses.
pub(crate) fn path_of(
    conn: &iroh::endpoint::Connection,
) -> Option<(RouteKind, Option<SocketAddr>)> {
    let paths = conn.paths();
    // The selected path; or, while iroh hasn't marked one yet, the first.
    let path = paths
        .iter()
        .find(|p| p.is_selected())
        .or_else(|| paths.iter().next())?;
    match path.remote_addr() {
        iroh::TransportAddr::Ip(addr) if !path.is_relay() => Some((kind_of(addr), Some(*addr))),
        _ => Some((RouteKind::Relay, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_public_ipv6_address_on_my_own_network_is_the_local_network() {
        // Found on the phone, 2026-11-21: on one Wi-Fi the route showed as
        // "Direct, internet" over the home's public IPv6 addresses.
        let other = sa("[2a00:23c4:11ec:7502:1c2b:3d4e:5f60:7182]:4000");
        assert_eq!(kind_of(&other), RouteKind::Internet);
        learn_own_addresses([&sa("[2a00:23c4:11ec:7502:9a2c:bcff:fe73:2b93]:46787")]);
        assert_eq!(kind_of(&other), RouteKind::Local);
        assert_eq!(
            kind_of(&sa("[2a00:23c4:11ec:7503::1]:4000")),
            RouteKind::Internet,
            "a neighbouring /64 is not this network"
        );
        assert!(ConnectionMode::Private.allows(kind_of(&other)));
    }

    #[test]
    fn with_a_relay_only_the_relay_is_published() {
        let relay: iroh::RelayUrl = "https://relay.avedeus.ovh".parse().unwrap();
        let ip = |s: &str| iroh::TransportAddr::Ip(sa(s));
        let all = vec![
            ip("192.168.1.169:4000"),
            ip("100.72.135.124:4000"),
            ip("[fd7a:115c:a1e0::1]:4000"),
            ip("[fe80::1]:4000"),
            ip("127.0.0.1:4000"),
            ip("81.2.69.160:4000"),
            // A home IPv6 address made from the Wi-Fi card's MAC address.
            ip("[2a00:23c4:11ec:7501:9a2c:bcff:fe73:2b93]:46787"),
            iroh::TransportAddr::Relay(relay.clone()),
        ];
        assert_eq!(publishable(&all), vec![iroh::TransportAddr::Relay(relay)]);
    }

    #[test]
    fn without_a_relay_only_public_addresses_not_made_from_hardware() {
        let ip = |s: &str| iroh::TransportAddr::Ip(sa(s));
        let all = vec![
            ip("192.168.1.169:4000"),
            ip("100.72.135.124:4000"),
            ip("[fd7a:115c:a1e0::1]:4000"),
            ip("[fd12:3456::1]:4000"),
            ip("[fe80::1]:4000"),
            ip("127.0.0.1:4000"),
            ip("[::1]:4000"),
            ip("81.2.69.160:4000"),
            ip("[2a00:23c4:11ec:7501:9a2c:bcff:fe73:2b93]:46787"),
            ip("[2a00:23c4:11ec:7501:1d4e:77a0:3c21:9b08]:46787"),
        ];
        assert_eq!(
            publishable(&all),
            vec![
                ip("81.2.69.160:4000"),
                ip("[2a00:23c4:11ec:7501:1d4e:77a0:3c21:9b08]:46787"),
            ]
        );
    }

    #[test]
    fn kinds_of_addresses() {
        assert_eq!(kind_of(&sa("192.0.2.2:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("198.19.0.1:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("[2001:db8::1]:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("198.20.0.1:4000")), RouteKind::Internet);
        assert_eq!(kind_of(&sa("192.168.1.169:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("10.0.0.2:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("172.20.1.1:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("169.254.3.4:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("100.72.135.124:4000")), RouteKind::Private);
        assert_eq!(kind_of(&sa("100.63.1.1:4000")), RouteKind::Internet);
        assert_eq!(kind_of(&sa("100.128.0.1:4000")), RouteKind::Internet);
        assert_eq!(kind_of(&sa("81.2.69.160:4000")), RouteKind::Internet);
        assert_eq!(kind_of(&sa("[fd7a:115c:a1e0::1]:4000")), RouteKind::Private);
        assert_eq!(kind_of(&sa("[fe80::1]:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("[fd00::1]:4000")), RouteKind::Local);
        assert_eq!(kind_of(&sa("[2a01:4f8::1]:4000")), RouteKind::Internet);
        assert_eq!(kind_of(&sa("[::ffff:192.168.1.2]:4000")), RouteKind::Local);
    }

    #[test]
    fn local_then_private_then_internet_without_repeats() {
        let told = vec![
            "81.2.69.160:4000".to_string(),
            "100.72.135.124:4000".to_string(),
            "192.168.1.169:4000".to_string(),
        ];
        let remembered = vec!["192.168.1.169:4000".to_string()];
        let w = Whereabouts {
            name: "create",
            nearby: &[],
            told: &told,
            remembered: &remembered,
            by_key: false,
        };
        let p = plan(&w, ConnectionMode::Hybrid).unwrap();
        assert_eq!(
            p.addresses,
            vec![
                sa("192.168.1.169:4000"),
                sa("100.72.135.124:4000"),
                sa("81.2.69.160:4000")
            ]
        );
        assert_eq!(
            p.kinds,
            vec![RouteKind::Local, RouteKind::Private, RouteKind::Internet]
        );
        let p = plan(&w, ConnectionMode::Private).unwrap();
        assert_eq!(p.kinds, vec![RouteKind::Local, RouteKind::Private]);
        let p = plan(&w, ConnectionMode::Local).unwrap();
        assert_eq!(p.addresses, vec![sa("192.168.1.169:4000")]);
    }

    #[test]
    fn nearby_comes_first_within_a_kind() {
        let nearby = vec!["192.168.1.50:4000".to_string()];
        let remembered = vec!["192.168.1.169:4000".to_string()];
        let w = Whereabouts {
            name: "create",
            nearby: &nearby,
            told: &[],
            remembered: &remembered,
            by_key: false,
        };
        let p = plan(&w, ConnectionMode::Hybrid).unwrap();
        assert_eq!(p.addresses[0], sa("192.168.1.50:4000"));
    }

    #[test]
    fn says_why_in_words() {
        let none = Whereabouts {
            name: "create",
            ..Default::default()
        };
        let err = plan(&none, ConnectionMode::Hybrid).unwrap_err();
        assert!(err.contains("hasn't met create"), "{err}");

        let told = vec!["100.72.135.124:4000".to_string()];
        let w = Whereabouts {
            name: "create",
            nearby: &[],
            told: &told,
            remembered: &[],
            by_key: false,
        };
        let err = plan(&w, ConnectionMode::Local).unwrap_err();
        assert!(err.contains("over a private network"), "{err}");
        assert!(err.contains("Local network only"), "{err}");

        let local = vec!["192.168.1.169:4000".to_string()];
        let w = Whereabouts {
            name: "create",
            nearby: &[],
            told: &local,
            remembered: &[],
            by_key: false,
        };
        let p = plan(&w, ConnectionMode::Hybrid).unwrap();
        let why = no_answer("create", &p);
        assert!(why.contains("on the local network"), "{why}");
        assert!(why.contains("Tailscale"), "{why}");
    }

    #[test]
    fn a_lookup_server_finds_a_device_by_key_only_on_any_route() {
        let w = Whereabouts {
            name: "create",
            by_key: true,
            ..Default::default()
        };
        let p = plan(&w, ConnectionMode::Hybrid).unwrap();
        assert!(p.addresses.is_empty());
        assert_eq!(p.kinds, vec![RouteKind::Internet]);
        let err = plan(&w, ConnectionMode::Private).unwrap_err();
        assert!(err.contains("through Relay's servers"), "{err}");
        assert!(err.contains("Local and private networks"), "{err}");
    }

    #[test]
    fn only_hybrid_uses_a_relay() {
        use ConnectionMode::*;
        assert!(Hybrid.allows(RouteKind::Relay));
        assert!(!Private.allows(RouteKind::Relay));
        assert!(!Private.allows(RouteKind::Internet));
        assert!(Private.allows(RouteKind::Private) && Private.allows(RouteKind::Local));
        assert!(!Local.allows(RouteKind::Relay));
        let own = RelayServers {
            relay_url: Some("https://relay.example.org".into()),
            lookup_url: Some("https://relay.example.org/pkarr".into()),
        };
        assert_eq!(own.in_mode(Hybrid), own);
        assert_eq!(own.in_mode(Private), RelayServers::default());
        assert_eq!(own.in_mode(Local), RelayServers::default());
    }

    #[test]
    fn server_addresses_are_checked() {
        let ok = RelayServers {
            relay_url: Some("https://relay.example.org".into()),
            lookup_url: Some("https://relay.example.org/pkarr".into()),
        };
        assert_eq!(ok.check(), Ok(()));
        assert!(ok.finds_by_key());
        let bad = RelayServers {
            relay_url: Some("relay.example.org".into()),
            lookup_url: None,
        };
        assert!(bad.check().is_err());
        assert!(!RelayServers::default().finds_by_key());
    }

    #[test]
    fn cleans_what_another_device_says() {
        let mut many: Vec<String> = (0..40).map(|i| format!("10.0.0.{i}:4000")).collect();
        many.push("not an address".into());
        many.push("127.0.0.1:4000".into());
        many.push("[fe80::1]:4000".into());
        many.insert(0, "10.0.0.1:4000".into());
        let cleaned = clean(&many);
        assert_eq!(cleaned.len(), MAX_ADDRESSES);
        assert_eq!(cleaned[0], "10.0.0.1:4000");
        assert_eq!(cleaned[1], "10.0.0.0:4000");
    }
}
