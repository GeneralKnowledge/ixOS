//! `relay`: the second Relay device, on a Linux (or macOS) machine. Same
//! code as the app, so one iPhone and a laptop are enough to try word
//! codes, trusted devices and transfers. Everything goes over iroh.
//!
//! ```text
//! relay listen                       take what trusted devices send, and
//!                                    send them what waits for them
//! relay devices                      trusted and revoked devices (on iroh)
//! relay send <device> (--text TEXT | PATH...) [--to host:port]
//!                                    send to a trusted device, no code:
//!                                    text is a message in its conversation
//! relay revoke <device>              stop trusting a device, for good
//! relay forget <device>              take a device off the list
//! relay trust <device>               trust a revoked device again
//! relay policy [codes-only | ask | automatic]
//!                                    what happens after a code transfer
//! relay connections [everywhere | private | this-network]
//!                                    which routes reach trusted devices
//! relay servers [--own] [--relay URL] [--lookup URL] [--mailbox ID] [--none]
//!                                    Relay's own relay and lookup server
//! relay send PATH...                 send files and folders with a word code
//!                                    to anyone on this network (the phone
//!                                    can scan the code's QR)
//! relay receive <code>               receive what someone sends with a code
//! relay grant <device> sites on|off  whether a trusted device may open
//!                                    the site this one shares (N5)
//! relay site [FOLDER | 127.0.0.1:PORT | --stop]
//!                                    share a folder, or a server on this
//!                                    machine's localhost, as this device's
//!                                    site, while `relay listen` runs (N6)
//! relay web <device> [--port N]      open a trusted device's site at
//!                                    http://127.0.0.1:N in any browser here
//! relay status                       this device: Connections, servers,
//!                                    and whether they answer
//! relay status <device>              ask a trusted device about itself
//! relay history                      what went across, newest first
//! relay name [NEW]                   this machine's name in Relay
//! relay serve-ai --upstream URL [--model M]
//!                                    listen, and answer trusted devices'
//!                                    AI requests with an OpenAI-compatible
//!                                    server (llama.cpp, Ollama, LM Studio)
//! relay ask <device> [--to host:port] [--model M] [--system TEXT]
//!           [--max-tokens N] PROMPT  ask a trusted device's model; the
//!                                    answer streams to the terminal
//! ```
//!
//! `serve-ai` sends a bearer token from `$RELAY_AI_KEY` if it is set. Only
//! trusted devices can ask, and the server's URL is never shared: keep it on
//! localhost.
//!
//! Data lives in `$RELAY_DATA`, else `$XDG_DATA_HOME/forge-relay`, else
//! `~/.local/share/forge-relay`. Received files are in its `inbox/`, and
//! what came with a word code in `codes/received/`.
//!
//! Word codes are found the way the app finds them: the sender advertises
//! `_forgerelay._udp` with mDNS (TXT `n` = the code's number, `id` = its
//! endpoint id), and the receiver looks for the number it was told.
//! Trusted devices advertise the same service type with TXT `d` = 1 and
//! `id` (no number), while `relay listen` runs.
//!
//! After a code transfer, the pairing policy may offer to trust the other
//! device; the command asks on the terminal.
//!
//! While `relay listen` runs, it owns the data folder. From another terminal,
//! the other commands go to the listener over `relay.sock` in the data
//! folder (0600) and print what it answers, so listening and sending work
//! at the same time. Word-code transfers run there too, and their trust
//! question is asked in the terminal that started them.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chat_core::MessageStatus;
use forge_services::ai::{ChatMessage, ChatRequest, Collect, LanguageModel};
use relay_core::{
    word_code_number, CodeSend, ConnectionMode, Direction, Item, NearbySender, RelayCore,
    RelayError, ServerHealth, TransferStatus, CODE_SERVICE_TYPE,
};

const USAGE: &str = "usage: relay listen | devices | send PATH... | receive <code> | \
send <device> (--text TEXT | PATH...) [--to host:port] | \
messages <device> | groups | group create NAME <device>... | \
group (add GROUP <device>... | remove GROUP <device> | rename GROUP NAME | leave GROUP | \
send GROUP TEXT | messages GROUP) | status [<device>] | revoke <device> | forget <device> | \
trust <device> | asks <device> on|off | introduces <device> on|off | \
sees-devices <device> on|off | grant <device> sites on|off | \
site [FOLDER | 127.0.0.1:PORT | --stop] | web <device> [--port N] | listen [--site FOLDER|127.0.0.1:PORT] | person <device> (NAME | --mine) | sync <device> [--accept] | policy [codes-only | ask | automatic] | \
connections [everywhere | private | this-network] | servers [--own] [--relay URL] [--lookup URL] [--mailbox ID] [--none] | \
history | name [NEW] | \
serve-ai --upstream URL [--model M] | \
ask <device> [--to host:port] [--model M] [--system TEXT] [--max-tokens N] PROMPT | \
receive-code <code> <host:port> <endpoint-id> | send-code FILE|FOLDER";

fn main() {
    if let Err(problem) = run(std::env::args().skip(1).collect()) {
        eprintln!("relay: {problem}");
        std::process::exit(1);
    }
}

fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RELAY_DATA") {
        return dir.into();
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("forge-relay")
}

fn open() -> Result<Arc<RelayCore>, RelayError> {
    let host = std::fs::read_to_string("/etc/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|_| "Linux".into());
    RelayCore::new(data_dir().to_string_lossy().into_owned(), host)
}

fn run(args: Vec<String>) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or(USAGE)?;
    let core = match open() {
        Ok(core) => core,
        // `relay listen` has the data: hand it the command.
        Err(RelayError::InUse) => return control::forward(&args),
        Err(err) => return Err(err.to_string()),
    };
    if command == "site" && !rest.is_empty() {
        core.shutdown();
        return Err(
            "a site is shared only while Relay listens: `relay listen --site FOLDER`, \
             or `relay site ...` while `relay listen` runs"
                .into(),
        );
    }
    if let Some(result) = shared(&core, command, rest) {
        let result = result.map(|out| print!("{out}"));
        if command == "web" && result.is_ok() {
            // The site stays open here until Ctrl-C.
            println!("Ctrl-C to stop.");
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        core.shutdown();
        return result;
    }
    let result = match command.as_str() {
        "listen" => listen(&core, rest),
        // `send <device> ...` to a trusted device is `shared`; otherwise the
        // arguments are files and folders to send with a word code.
        "send" | "receive" => code_transfer(&core, command, rest, &Terminal),
        "serve-ai" => serve_ai(&core, rest),
        "ask" => ask(&core, rest),
        // Receiving from a given address, without mDNS.
        "receive-code" => {
            let [code, address, id] = rest else {
                return Err(USAGE.into());
            };
            let got =
                block_on(core.receive_with_code(code.clone(), vec![address.clone()], id.clone()))
                    .map_err(|e| e.to_string())?;
            println!(
                "Received {} ({} bytes, {} already here) in {:.1}s at {:.1} MB/s: {}",
                got.name,
                got.size,
                got.resumed_from,
                got.seconds,
                got.megabytes_per_second,
                got.path
            );
            Ok(())
        }
        // Older names for `send PATH...`.
        "send-code" if !rest.is_empty() => send_code(&core, rest, &Terminal),
        "name" => match rest.first() {
            Some(name) => block_on(core.rename_device(name.clone())).map_err(|e| e.to_string()),
            None => {
                println!("{}", core.identity().name);
                Ok(())
            }
        },
        _ => Err(USAGE.into()),
    };
    core.shutdown();
    result
}

/// "nearby", "reachable, seen 2 min ago" or "offline, seen 3 h ago".
fn where_now(d: &relay_core::Device) -> String {
    let word = match d.presence {
        relay_core::Presence::Nearby => return "nearby".into(),
        relay_core::Presence::Reachable => "reachable",
        relay_core::Presence::Offline => "offline",
    };
    match d.last_seen_ms {
        None => format!("{word}, not reached yet"),
        Some(t) => {
            let secs = (now_millis() - t).max(0) / 1000;
            let ago = match secs {
                0..=59 => "just now".to_string(),
                60..=3599 => format!("{} min ago", secs / 60),
                3600..=172_799 => format!("{} h ago", secs / 3600),
                _ => format!("{} days ago", secs / 86_400),
            };
            format!("{word}, seen {ago}")
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The commands that also work while `relay listen` runs, from another
/// terminal: they go to the listener (`control`), which runs them on its
/// core and sends back what they print. `None` for any other command.
fn shared(core: &RelayCore, command: &str, rest: &[String]) -> Option<Result<String, String>> {
    let mut out = String::new();
    let result = match command {
        "send" => match rest.first() {
            Some(first) if find_device(core, first).is_some() => {
                send_to_device(core, rest).map(|line| out += &line)
            }
            _ => return None,
        },
        "groups" => (|| {
            for g in block_on(core.groups()).map_err(|e| e.to_string())? {
                out += &group_lines(&g);
            }
            Ok(())
        })(),
        "group" => group_command(core, rest).map(|text| out += &text),
        "messages" => (|| {
            let who = rest.first().ok_or(USAGE)?;
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            for m in core
                .device_messages(&device.endpoint_id)
                .map_err(|e| e.to_string())?
            {
                let mine = m.author_id != device.endpoint_id;
                let who = if mine { "me" } else { device.name.as_str() };
                // An attachment's message has no text: say what it carries.
                let line = message_line(&m);
                let status = if mine { message_status(&m) } else { "" };
                out += &format!("{who}: {line}{status}\n");
            }
            Ok(())
        })(),
        "devices" => {
            for d in core.devices() {
                let state = if d.trusted { "trusted" } else { "revoked" };
                let takes = if d.trusted && !d.receives_from {
                    ", may not send here"
                } else {
                    ""
                };
                let asks = if d.trusted && d.may_ask {
                    ", may ask"
                } else {
                    ""
                };
                let introduces = if d.trusted && d.introduces {
                    ", introduces devices"
                } else {
                    ""
                };
                let sees = if d.trusted && d.sees_devices {
                    ", sees your devices"
                } else {
                    ""
                };
                let whose = match &d.person {
                    Some(person) => format!(" ({person}'s)"),
                    None => String::new(),
                };
                let sites = if d.grants.contains(&relay_core::Capability::Sites) {
                    ", may open your site"
                } else {
                    ""
                };
                out += &format!(
                    "{}{whose}  {state}{takes}{asks}{introduces}{sees}{sites}  {}  {}\n",
                    d.name,
                    where_now(&d),
                    d.endpoint_id
                );
                if let Some(by) = &d.introduced_by {
                    out += &format!("  introduced by {by}\n");
                }
                if let Some(by) = &d.revoked_by {
                    out += &format!("  revoked by {by}\n");
                }
                if let Some(route) = &d.route {
                    out += &format!("  last connected: {route}\n");
                }
                if let Some(problem) = &d.problem {
                    out += &format!("  {problem}\n");
                }
            }
            out += &format!("Pairing policy: {}\n", policy_name(core.pairing_policy()));
            out += &format!("Connections: {}\n", mode_name(core.connection_mode()));
            Ok(())
        }
        // This device: how it is reached, and whether the servers answer.
        "status" if rest.is_empty() => (|| {
            let service = block_on(core.device_service()).map_err(|e| e.to_string())?;
            out += &format!("{}  {}\n", core.identity().name, service.endpoint_id);
            out += &format!("Connections: {}\n", mode_name(core.connection_mode()));
            let s = core.servers();
            out += &format!(
                "Servers: relay {}, lookup {}, mailbox {}\n",
                s.relay_url.as_deref().unwrap_or("none"),
                s.lookup_url.as_deref().unwrap_or("none"),
                s.mailbox.as_deref().unwrap_or("none")
            );
            // A process that just opened Relay gives the relay a moment.
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut health = core.server_health();
            while health == ServerHealth::Connecting && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(200));
                health = core.server_health();
            }
            out += match health {
                ServerHealth::Off if core.connection_mode() == ConnectionMode::Hybrid => {
                    "Relay's servers: no relay set\n"
                }
                ServerHealth::Off => "Relay's servers: never used in this mode\n",
                ServerHealth::Connecting => "Relay's servers: still connecting\n",
                ServerHealth::Reachable => "Relay's servers: the relay answers\n",
                ServerHealth::Unreachable => "Relay's servers: the relay can't be reached\n",
            };
            // The lookup server is used in Everywhere, relay or not.
            if core.connection_mode() == ConnectionMode::Hybrid && s.lookup_url.is_some() {
                let published = if s.relay_url.is_some() {
                    "the relay's address"
                } else {
                    "public addresses only"
                };
                out += &format!(
                    "The lookup server has, signed by this device's key: {published}. \
                     Home and Tailscale addresses go only to trusted devices.\n"
                );
            }
            out += &format!(
                "Addresses for trusted devices: {}\n",
                service.addresses.join(", ")
            );
            Ok(())
        })(),
        // Asks a trusted device about itself (its platform and offers).
        "status" => (|| {
            let who = rest.first().ok_or(USAGE)?;
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            let d = block_on(core.device_status(device.endpoint_id.clone()))
                .map_err(|e| e.to_string())?;
            out += &format!("{}  {}\n", d.name, where_now(&d));
            out += &format!(
                "  platform: {}\n  offers: {}\n",
                d.platform.as_deref().unwrap_or("not said"),
                if d.offers.is_empty() {
                    "nothing said".to_string()
                } else {
                    d.offers.join(", ")
                }
            );
            Ok(())
        })(),
        "revoke" | "forget" | "trust" => (|| {
            let who = rest.first().ok_or(USAGE)?;
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            let id = device.endpoint_id.clone();
            let done = match command {
                "revoke" => block_on(core.revoke_device(id)),
                "forget" => block_on(core.forget_device(id)),
                _ => block_on(core.trust_device(id)),
            };
            done.map_err(|e| e.to_string())?;
            let what = match command {
                "revoke" => "revoked; it can only send here with a code now",
                "forget" => "forgotten",
                _ => "trusted",
            };
            out += &format!("{}: {what}.\n", device.name);
            Ok(())
        })(),
        // N5: a capability granted to a trusted device. Off when trusted.
        "grant" => (|| {
            let (who, capability, on) = match rest {
                [who, what, on] if on == "on" => (who, what, true),
                [who, what, off] if off == "off" => (who, what, false),
                _ => return Err(USAGE.to_string()),
            };
            let capability = relay_core::Capability::ALL
                .into_iter()
                .find(|c| c.name() == capability)
                .ok_or_else(|| format!("no capability {capability}; there is: sites"))?;
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            block_on(core.set_grant(device.endpoint_id.clone(), capability, on))
                .map_err(|e| e.to_string())?;
            let what = match (capability, on) {
                (relay_core::Capability::Sites, true) => "may open this device's site",
                (relay_core::Capability::Sites, false) => "may not open this device's site",
            };
            out += &format!("{}: {what}.\n", device.name);
            Ok(())
        })(),
        // N6: the site this device shares while it listens.
        "site" => (|| {
            match rest {
                [] => {}
                [stop] if stop == "--stop" => core.stop_site(),
                [site] => {
                    block_on(core.share_site(site_source(site)?)).map_err(|e| e.to_string())?
                }
                _ => return Err(USAGE.to_string()),
            }
            out += &match core.shared_site() {
                Some(site) => site_line(&site) + "\n",
                None => "No site is shared.\n".to_string(),
            };
            Ok(())
        })(),
        // N6: a trusted device's site, on a port on this machine.
        "web" => (|| {
            let (who, port) = match rest {
                [who] => (who, 0),
                [who, flag, port] if flag == "--port" => (
                    who,
                    port.parse().map_err(|_| format!("not a port: {port}"))?,
                ),
                _ => return Err(USAGE.to_string()),
            };
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            // Found on this network, as `send` does: a device never met
            // has no saved address yet.
            let addresses = find_device_addresses(&device.endpoint_id).unwrap_or_default();
            if !addresses.is_empty() {
                core.devices_nearby(vec![relay_core::NearbyDevice {
                    endpoint_id: device.endpoint_id.clone(),
                    addresses,
                }]);
            }
            let opened = core
                .open_site_port(&device.endpoint_id, port)
                .map_err(|e| e.to_string())?;
            out += &format!("{}'s site: {}\n", device.name, opened.url);
            // Kept open for as long as this process runs.
            lock_ports().push(opened);
            Ok(())
        })(),
        // Whether a trusted device may ask this one's AI: off when trusted.
        "asks" => (|| {
            let (who, on) = match rest {
                [who, on] if on == "on" => (who, true),
                [who, off] if off == "off" => (who, false),
                _ => return Err(USAGE.to_string()),
            };
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            block_on(core.set_may_ask(device.endpoint_id.clone(), on))
                .map_err(|e| e.to_string())?;
            let what = if on { "may ask" } else { "may not ask" };
            out += &format!("{}: {what} this device's AI.\n", device.name);
            Ok(())
        })(),
        // My Devices, step 1: a device that introduces devices here, and
        // one that may see the devices trusted here. Both off when trusted.
        "introduces" | "sees-devices" => (|| {
            let (who, on) = match rest {
                [who, on] if on == "on" => (who, true),
                [who, off] if off == "off" => (who, false),
                _ => return Err(USAGE.to_string()),
            };
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            let id = device.endpoint_id.clone();
            let what = if command == "introduces" {
                block_on(core.set_introduces(id, on)).map_err(|e| e.to_string())?;
                if on {
                    "introduces devices here; `relay sync` asks it now"
                } else {
                    "doesn't introduce devices here"
                }
            } else {
                block_on(core.set_sees_devices(id, on)).map_err(|e| e.to_string())?;
                if on {
                    "may see the devices trusted here"
                } else {
                    "may not see the devices trusted here"
                }
            };
            out += &format!("{}: {what}.\n", device.name);
            Ok(())
        })(),
        // N4 step 1: a trusted device that belongs to someone else, shown
        // by their name in Chats, or (--mine) one of your own devices.
        "person" => (|| {
            let [who, person] = rest else {
                return Err(USAGE.to_string());
            };
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            let person = (person != "--mine").then(|| person.clone());
            block_on(core.set_person(device.endpoint_id.clone(), person.clone()))
                .map_err(|e| e.to_string())?;
            out += &match person {
                Some(person) => format!(
                    "{} is {}'s: it may send messages and files, and never sees your devices.\n",
                    device.name,
                    person.trim()
                ),
                None => format!("{} is one of your devices.\n", device.name),
            };
            Ok(())
        })(),
        // Asks an introducer for its devices: new ones are listed (and
        // trusted with --accept), revoked ones are revoked here.
        "sync" => (|| {
            let (who, accept) = match rest {
                [who] => (who, false),
                [who, flag] if flag == "--accept" => (who, true),
                _ => return Err(USAGE.to_string()),
            };
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            let synced = block_on(core.sync_devices(device.endpoint_id.clone()))
                .map_err(|e| e.to_string())?;
            if synced.revoked > 0 {
                out += &format!(
                    "{} revoked here: {} revoked them.\n",
                    synced.revoked, device.name
                );
            }
            let mut offered = 0;
            while let Some(offer) = core.trust_offer() {
                if offer.introducer_id.as_deref() != Some(device.endpoint_id.as_str()) {
                    break;
                }
                offered += 1;
                if accept {
                    block_on(core.trust_device(offer.endpoint_id.clone()))
                        .map_err(|e| e.to_string())?;
                    out += &format!("Trusted {} ({}).\n", offer.name, offer.endpoint_id);
                } else {
                    core.dismiss_trust_offer_for(&offer.endpoint_id);
                    out += &format!(
                        "{} introduces {} ({}).\n",
                        device.name, offer.name, offer.endpoint_id
                    );
                }
            }
            if offered == 0 && synced.revoked == 0 {
                out += &format!("Nothing new from {}.\n", device.name);
            } else if offered > 0 && !accept {
                out += "Trust them with `relay sync <device> --accept`.\n";
            }
            Ok(())
        })(),
        "policy" => match rest.first().map(String::as_str) {
            None => {
                out += &format!("{}\n", policy_name(core.pairing_policy()));
                Ok(())
            }
            Some(name) => {
                let policy = match name {
                    "codes-only" => relay_core::PairingPolicy::CodesOnly,
                    "ask" => relay_core::PairingPolicy::AskToPair,
                    "automatic" => relay_core::PairingPolicy::PairAutomatically,
                    _ => return Some(Err(USAGE.into())),
                };
                block_on(core.set_pairing_policy(policy)).map_err(|e| e.to_string())
            }
        },
        "connections" => match rest.first().map(String::as_str) {
            None => {
                out += &format!("{}\n", mode_name(core.connection_mode()));
                Ok(())
            }
            Some(name) => {
                let mode = match name {
                    // The app's words (N2), and the older ones.
                    "this-network" | "local" => relay_core::ConnectionMode::Local,
                    "private" => relay_core::ConnectionMode::Private,
                    "everywhere" | "any" => relay_core::ConnectionMode::Hybrid,
                    _ => return Some(Err(USAGE.into())),
                };
                block_on(core.set_connection_mode(mode)).map_err(|e| e.to_string())
            }
        },
        "servers" => (|| {
            if rest.is_empty() {
                let s = core.servers();
                out += &format!(
                    "relay: {}\nlookup: {}\nmailbox: {}\n",
                    s.relay_url.as_deref().unwrap_or("none"),
                    s.lookup_url.as_deref().unwrap_or("none"),
                    s.mailbox.as_deref().unwrap_or("none")
                );
                return Ok(());
            }
            let mut servers = core.servers();
            let mut args = rest.iter();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    // A flag without its value is a mistake, never a reset
                    // (that is --none).
                    "--relay" | "--lookup" | "--mailbox" => {
                        let value = args.next().ok_or_else(|| USAGE.to_string())?.clone();
                        match arg.as_str() {
                            "--relay" => servers.relay_url = Some(value),
                            "--lookup" => servers.lookup_url = Some(value),
                            _ => servers.mailbox = Some(value),
                        }
                    }
                    "--none" => servers = relay_core::RelayServers::default(),
                    "--own" => servers = relay_core::own_relay_servers(),
                    _ => return Err(USAGE.to_string()),
                }
            }
            block_on(core.set_servers(servers)).map_err(|e| e.to_string())?;
            out += "Saved, and in use now.\n";
            Ok(())
        })(),
        "history" => {
            for t in core.transfers() {
                out += &format!("{}\n", describe(&t));
            }
            Ok(())
        }
        _ => return None,
    };
    Some(result.map(|()| out))
}

/// The listener's end of `shared`: a Unix socket in the data folder, one
/// command per connection. Only this user can reach it (the folder is
/// theirs, and the socket is made 0600).
mod control {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::Arc;

    use relay_core::RelayCore;

    fn path() -> PathBuf {
        super::data_dir().join("relay.sock")
    }

    /// Takes commands from other `relay` processes while listening.
    pub fn serve(core: Arc<RelayCore>) -> Result<(), String> {
        let path = path();
        // No other process has this data folder (we hold its lock), so a
        // socket left here is from a listener that has ended.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let core = core.clone();
                std::thread::spawn(move || answer(&core, stream));
            }
        });
        Ok(())
    }

    /// One line each way, as JSON. The listener says lines and asks
    /// questions while a command runs (a word-code transfer), then sends
    /// `Done`; the other process prints, answers, and ends with it.
    #[derive(serde::Serialize, serde::Deserialize)]
    enum Wire {
        Say(String),
        Ask(String),
        /// Nothing to say: only checks the asking process is still there.
        Tick,
        Done(Result<String, String>),
    }

    /// What a `relay` process asks the listener: its arguments, and the
    /// folder it runs in, so relative paths mean what they mean there.
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Request {
        args: Vec<String>,
        cwd: Option<std::path::PathBuf>,
    }

    /// `args` with each relative path that names something in `cwd` made
    /// absolute: a device name, and the values of `--to`, `--text` and
    /// `--note`, stay as they are.
    fn resolve(core: &RelayCore, cwd: Option<&std::path::Path>, args: Vec<String>) -> Vec<String> {
        let Some(cwd) = cwd else {
            return args;
        };
        if args.first().map(String::as_str) == Some("site") {
            // `relay site FOLDER`: a relative folder is the caller's, even
            // one that isn't there (then it is reported missing, never
            // looked for where the listener runs).
            return args
                .into_iter()
                .enumerate()
                .map(|(i, arg)| {
                    let server = ["127.0.0.1:", "localhost:"]
                        .iter()
                        .any(|h| arg.starts_with(h));
                    if i == 1 && arg != "--stop" && !server {
                        cwd.join(&arg).to_string_lossy().into_owned()
                    } else {
                        arg
                    }
                })
                .collect();
        }
        if args.first().map(String::as_str) != Some("send") {
            return args;
        }
        let mut out = Vec::with_capacity(args.len());
        let mut takes_value = false;
        for (i, arg) in args.into_iter().enumerate() {
            let keep = i == 0
                || takes_value
                || arg.starts_with("--")
                || (i == 1 && super::find_device(core, &arg).is_some())
                || std::path::Path::new(&arg).is_absolute()
                || !cwd.join(&arg).exists();
            takes_value = matches!(arg.as_str(), "--to" | "--text" | "--note");
            out.push(if keep {
                arg
            } else {
                cwd.join(&arg).to_string_lossy().into_owned()
            });
        }
        out
    }

    fn send(stream: &UnixStream, wire: &Wire) -> std::io::Result<()> {
        let line = serde_json::to_string(wire).map_err(std::io::Error::other)? + "\n";
        let mut stream = stream;
        stream.write_all(line.as_bytes())
    }

    /// The asking process's terminal, over the socket.
    struct Remote {
        stream: UnixStream,
        reader: std::sync::Mutex<BufReader<UnixStream>>,
        /// Set once a write fails: the asking process has gone.
        gone: std::sync::atomic::AtomicBool,
    }

    impl Remote {
        fn send(&self, wire: &Wire) -> bool {
            let ok = send(&self.stream, wire).is_ok();
            if !ok {
                self.gone.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            ok
        }
    }

    impl super::Io for Remote {
        fn say(&self, line: &str) {
            self.send(&Wire::Say(line.to_string()));
        }

        fn alive(&self) -> bool {
            !self.gone.load(std::sync::atomic::Ordering::SeqCst) && self.send(&Wire::Tick)
        }

        fn ask(&self, question: &str) -> Option<String> {
            if !self.send(&Wire::Ask(question.to_string())) {
                return None;
            }
            let mut line = String::new();
            forge_runtime::lock(&self.reader)
                .read_line(&mut line)
                .ok()?;
            serde_json::from_str::<Option<String>>(&line).ok()?
        }
    }

    fn answer(core: &Arc<RelayCore>, stream: UnixStream) {
        let Ok(reading) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(reading);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                match resolve(core, request.cwd.as_deref(), request.args).split_first() {
                    Some((command, rest)) => match super::shared(core, command, rest) {
                        Some(reply) => reply,
                        // A word-code transfer runs here, talking to the
                        // other process's terminal.
                        None if matches!(command.as_str(), "send" | "receive") => {
                            let Ok(talking) = stream.try_clone() else {
                                return;
                            };
                            let io = Remote {
                                stream: talking,
                                reader: std::sync::Mutex::new(reader),
                                gone: Default::default(),
                            };
                            super::code_transfer(core, command, rest, &io).map(|()| String::new())
                        }
                        None => Err(busy(command)),
                    },
                    None => Err(super::USAGE.into()),
                }
            }
            Err(_) => Err("a garbled command".into()),
        };
        let _ = send(&stream, &Wire::Done(reply));
    }

    fn busy(command: &str) -> String {
        format!("`relay listen` is running with this data; stop it to use `relay {command}`")
    }

    /// Runs `args` on the running listener, printing what it printed.
    pub fn forward(args: &[String]) -> Result<(), String> {
        let command = args.first().map(String::as_str).unwrap_or("");
        let mut stream = UnixStream::connect(path())
            .map_err(|_| "Relay's data is in use by another relay process".to_string())?;
        let request = Request {
            args: args.to_vec(),
            cwd: std::env::current_dir().ok(),
        };
        let line = serde_json::to_string(&request).map_err(|e| e.to_string())? + "\n";
        stream
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
            if read == 0 {
                return Err(busy(command));
            }
            match serde_json::from_str::<Wire>(&line) {
                Ok(Wire::Say(said)) => println!("{said}"),
                Ok(Wire::Tick) => {}
                Ok(Wire::Ask(question)) => {
                    let answer = super::Io::ask(&super::Terminal, &question);
                    let line = serde_json::to_string(&answer).map_err(|e| e.to_string())? + "\n";
                    stream
                        .write_all(line.as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                Ok(Wire::Done(Ok(out))) => {
                    print!("{out}");
                    return Ok(());
                }
                Ok(Wire::Done(Err(problem))) => return Err(problem),
                Err(_) => return Err(busy(command)),
            }
        }
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn listen(core: &Arc<RelayCore>, args: &[String]) -> Result<(), String> {
    let site = match args {
        [] => None,
        [flag, site] if flag == "--site" => Some(site_source(site)?),
        _ => return Err(USAGE.into()),
    };
    // Trusted devices reach this one on iroh while it listens.
    let service = block_on(core.device_service()).map_err(|e| e.to_string())?;
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    if let Err(err) = advertise_device(&mdns, service.port, &service.endpoint_id) {
        eprintln!("relay: trusted devices won't find this one: {err}");
    }
    println!(
        "Relay is listening as \"{}\" ({}).",
        core.identity().name,
        service.endpoint_id
    );
    println!("To trust a new device, send it something with a word code first.");
    if let Some(site) = site {
        block_on(core.share_site(site.clone())).map_err(|e| e.to_string())?;
        println!("{}", site_line(&site));
    }
    match control::serve(core.clone()) {
        Ok(()) => println!(
            "Waiting. Ctrl-C to stop. In another terminal, other `relay` commands \
             (word codes too) use this listener."
        ),
        Err(err) => println!("Waiting. Ctrl-C to stop. (Other terminals can't use it: {err}.)"),
    }
    let mut seen = core.transfers().first().map(|t| t.id).unwrap_or(0);
    let mut cursors = HashMap::new();
    watch_messages(core, &mut cursors, true);
    let mut round = 0u32;
    loop {
        std::thread::sleep(Duration::from_millis(300));
        round = round.wrapping_add(1);
        if round.is_multiple_of(3) {
            watch_messages(core, &mut cursors, false);
        }
        for t in core.transfers().into_iter().rev() {
            if t.id > seen
                && !matches!(
                    t.status,
                    TransferStatus::InProgress { .. }
                        | TransferStatus::Waiting
                        | TransferStatus::InMailbox
                )
            {
                println!("{}", describe(&t));
                seen = t.id;
            }
        }
    }
}

#[cfg(feature = "cli")]
fn qr(code: &str) -> String {
    use qrcode::render::unicode::Dense1x2;
    match qrcode::QrCode::new(code.as_bytes()) {
        Ok(qr) => qr
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
        Err(err) => format!("(no QR code: {err})"),
    }
}

fn policy_name(policy: relay_core::PairingPolicy) -> &'static str {
    match policy {
        relay_core::PairingPolicy::CodesOnly => "codes-only (nothing is remembered)",
        relay_core::PairingPolicy::AskToPair => "ask (offer to trust after a code transfer)",
        relay_core::PairingPolicy::PairAutomatically => {
            "automatic (trust devices trusted before again by themselves)"
        }
    }
}

fn mode_name(mode: relay_core::ConnectionMode) -> &'static str {
    match mode {
        relay_core::ConnectionMode::Local => "this-network (the same Wi-Fi or wired network only)",
        relay_core::ConnectionMode::Private => {
            "private (the same network or a private one such as Tailscale; no servers)"
        }
        relay_core::ConnectionMode::Hybrid => {
            "everywhere (the same network, Tailscale, the internet, then the relay)"
        }
    }
}

/// The sites opened with `relay web`, open while this process runs.
fn lock_ports() -> std::sync::MutexGuard<'static, Vec<relay_core::SitePort>> {
    static PORTS: Mutex<Vec<relay_core::SitePort>> = Mutex::new(Vec::new());
    PORTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A site from the command line: `127.0.0.1:PORT` or `localhost:PORT` is
/// a server on this machine, anything else a folder.
fn site_source(arg: &str) -> Result<relay_core::SiteSource, String> {
    for host in ["127.0.0.1:", "localhost:"] {
        if let Some(port) = arg.strip_prefix(host) {
            let port = port.parse().map_err(|_| format!("not a port: {port}"))?;
            return Ok(relay_core::SiteSource::Local { port });
        }
    }
    if arg.contains("://") || arg.parse::<std::net::SocketAddr>().is_ok() {
        return Err("a site's server must be on this machine: 127.0.0.1:PORT".into());
    }
    let folder = std::path::Path::new(arg)
        .canonicalize()
        .map_err(|_| format!("no folder {arg}"))?;
    Ok(relay_core::SiteSource::Folder {
        path: folder.to_string_lossy().into_owned(),
    })
}

fn site_line(site: &relay_core::SiteSource) -> String {
    let what = match site {
        relay_core::SiteSource::Folder { path } => format!("the folder {path}"),
        relay_core::SiteSource::Local { port } => format!("http://127.0.0.1:{port}"),
    };
    format!(
        "Sharing {what} as this device's site, with devices granted it \
         (`relay grant <device> sites on`)."
    )
}

/// A group by its id, the start of its id, or its name.
fn find_group(core: &RelayCore, which: &str) -> Result<relay_core::group::Group, String> {
    let groups = block_on(core.groups()).map_err(|e| e.to_string())?;
    let found: Vec<_> = groups
        .into_iter()
        .filter(|g| g.id.starts_with(which) || g.name.eq_ignore_ascii_case(which))
        .collect();
    match found.len() {
        1 => Ok(found.into_iter().next().unwrap()),
        0 => Err(format!("no group {which}; `relay groups` lists them")),
        _ => Err(format!("more than one group is {which}: use its id")),
    }
}

fn group_lines(g: &relay_core::group::Group) -> String {
    let state = if g.left { "  (left)" } else { "" };
    let mut out = format!("{}  {}{state}\n", g.name, g.id);
    for m in &g.members {
        let admin = if m.endpoint_id == g.admin {
            ", admin"
        } else {
            ""
        };
        let note = if m.is_me {
            " (this device)"
        } else if !m.trusted {
            " (not paired with this device: skipped for now)"
        } else {
            ""
        };
        out += &format!("  {}{note}{admin}  {}\n", m.name, m.endpoint_id);
    }
    out
}

/// `relay group create|add|remove|rename|leave|send|messages ...`.
fn group_command(core: &RelayCore, rest: &[String]) -> Result<String, String> {
    let device_id = |who: &String| {
        find_device(core, who)
            .map(|d| d.endpoint_id)
            .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))
    };
    let err = |e: RelayError| e.to_string();
    match rest {
        [verb, name, members @ ..] if verb == "create" && !members.is_empty() => {
            let ids = members
                .iter()
                .map(device_id)
                .collect::<Result<Vec<_>, _>>()?;
            let g = block_on(core.create_group(name.clone(), ids)).map_err(err)?;
            Ok(group_lines(&g))
        }
        [verb, which, members @ ..] if verb == "add" && !members.is_empty() => {
            let g = find_group(core, which)?;
            let ids = members
                .iter()
                .map(device_id)
                .collect::<Result<Vec<_>, _>>()?;
            let g = block_on(core.add_group_members(g.id, ids)).map_err(err)?;
            Ok(group_lines(&g))
        }
        [verb, which, member] if verb == "remove" => {
            let g = find_group(core, which)?;
            let id = g
                .members
                .iter()
                .find(|m| m.endpoint_id == *member || m.name.eq_ignore_ascii_case(member))
                .map(|m| m.endpoint_id.clone())
                .ok_or_else(|| format!("{member} isn't in {}", g.name))?;
            let g = block_on(core.remove_group_member(g.id, id)).map_err(err)?;
            Ok(group_lines(&g))
        }
        [verb, which, name] if verb == "rename" => {
            let g = find_group(core, which)?;
            let g = block_on(core.rename_group(g.id, name.clone())).map_err(err)?;
            Ok(group_lines(&g))
        }
        [verb, which] if verb == "leave" => {
            let g = find_group(core, which)?;
            block_on(core.leave_group(g.id)).map_err(err)?;
            Ok(format!("Left {}.\n", g.name))
        }
        [verb, which, text @ ..] if verb == "send" && !text.is_empty() => {
            let g = find_group(core, which)?;
            let m = block_on(core.send_group_message(&g.id, text.join(" "))).map_err(err)?;
            let d = core.group_delivery(&g.id, &m.id).map_err(err)?;
            Ok(format!(
                "Sent to {}: delivered to {} of {}{}\n",
                g.name,
                d.delivered,
                d.of,
                message_status(&m)
            ))
        }
        [verb, which] if verb == "messages" => {
            let g = find_group(core, which)?;
            let mut out = String::new();
            for m in core.group_messages(&g.id).map_err(err)? {
                let mine = m.author_id == relay_core::device_chat::ME;
                let who = if mine { "me" } else { m.author.as_str() };
                let status = if mine {
                    let d = core.group_delivery(&g.id, &m.id).map_err(err)?;
                    format!("  (delivered to {} of {})", d.delivered, d.of)
                } else {
                    String::new()
                };
                out += &format!("{who}: {}{status}\n", message_line(&m));
            }
            Ok(out)
        }
        _ => Err(USAGE.into()),
    }
}

fn find_device(core: &RelayCore, who: &str) -> Option<relay_core::Device> {
    core.devices()
        .into_iter()
        .find(|d| d.endpoint_id == who || d.name.eq_ignore_ascii_case(who))
}

/// Where a word-code transfer talks to the person: this terminal, or the
/// terminal of a `relay` process whose command `relay listen` runs
/// (`control`), so code transfers work while it listens.
trait Io: Send + Sync {
    fn say(&self, line: &str);
    /// The person's answer to `question`, or `None` when nobody can
    /// answer (not a terminal).
    fn ask(&self, question: &str) -> Option<String>;
    /// Whether whoever started the transfer is still there: a `relay`
    /// process that ended takes its transfer with it.
    fn alive(&self) -> bool {
        true
    }
}

struct Terminal;

impl Io for Terminal {
    fn say(&self, line: &str) {
        println!("{line}");
    }

    fn ask(&self, question: &str) -> Option<String> {
        use std::io::{BufRead, IsTerminal, Write};
        if !std::io::stdin().is_terminal() {
            return None;
        }
        print!("{question} ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).ok()?;
        Some(answer)
    }
}

/// `send PATH...` or `receive <code>`, then the trust offer.
fn code_transfer(
    core: &Arc<RelayCore>,
    command: &str,
    rest: &[String],
    io: &dyn Io,
) -> Result<(), String> {
    match (command, rest) {
        ("send", [_, ..]) => send_code(core, rest, io)?,
        ("receive", [code]) => receive_code(core, code, io)?,
        _ => return Err(USAGE.into()),
    }
    offer_trust(core, io)
}

/// After a code transfer, asks whether to trust the other device, when
/// the pairing policy offers it.
fn offer_trust(core: &RelayCore, io: &dyn Io) -> Result<(), String> {
    let Some(offer) = core.trust_offer() else {
        return Ok(());
    };
    let name = if offer.name.is_empty() {
        "the other device".to_string()
    } else {
        format!("\"{}\"", offer.name)
    };
    let Some(answer) = io.ask(&format!(
        "Trust {name}? You'll be able to send to each other without a code. [y/N]"
    )) else {
        io.say(&format!("Trust {name}? Not asked: this isn't a terminal."));
        return Ok(());
    };
    if answer.trim().eq_ignore_ascii_case("y") {
        block_on(core.trust_device(offer.endpoint_id)).map_err(|e| e.to_string())?;
        io.say("Trusted. `relay devices` lists it.");
    } else {
        core.dismiss_trust_offer_for(&offer.endpoint_id);
    }
    Ok(())
}

/// `send <device> (--text TEXT | PATH...) [--to host:port]`: to a trusted
/// device, found with mDNS unless `--to` says where. Text is a message in
/// its conversation (`--note` is the older name for `--text`).
fn send_to_device(core: &RelayCore, args: &[String]) -> Result<String, String> {
    let mut args = args.to_vec();
    let to = take_flag(&mut args, "--to")?;
    let note = take_flag(&mut args, "--note")?;
    let text = take_flag(&mut args, "--text")?;
    let (who, paths) = args.split_first().ok_or(USAGE)?;
    let device = find_device(core, who).ok_or(USAGE)?;
    // Not found: it is queued, and goes when it is reached (where it was
    // last seen, or when it connects here) while `relay listen` runs.
    let addresses = match to {
        Some(address) => vec![address],
        None => find_device_addresses(&device.endpoint_id).unwrap_or_default(),
    };
    if !addresses.is_empty() {
        core.devices_nearby(vec![relay_core::NearbyDevice {
            endpoint_id: device.endpoint_id.clone(),
            addresses,
        }]);
    }
    if let Some(text) = text.or(note) {
        if !paths.is_empty() {
            return Err(USAGE.into());
        }
        let message = block_on(core.send_device_message(&device.endpoint_id, text))
            .map_err(|e| e.to_string())?;
        let how = match message.status {
            chat_core::MessageStatus::Sent => "message delivered".to_string(),
            _ => format!(
                "message waiting: it goes when {} is reached, while relay listen runs",
                device.name
            ),
        };
        return Ok(format!("to {}: {how}\n", device.name));
    }
    if paths.is_empty() {
        return Err(USAGE.into());
    }
    let transfer = block_on(core.send_to_device(device.endpoint_id, paths.to_vec()))
        .map_err(|e| e.to_string())?;
    Ok(format!("{}\n", describe(&transfer)))
}

/// Advertises this device for trusted devices: TXT `d` = 1 and its id.
fn advertise_device(
    mdns: &mdns_sd::ServiceDaemon,
    port: u16,
    endpoint_id: &str,
) -> Result<String, String> {
    let short = &endpoint_id[..8.min(endpoint_id.len())];
    let txt = [
        ("v", "1".to_string()),
        ("d", "1".to_string()),
        ("id", endpoint_id.to_string()),
    ];
    let info = mdns_sd::ServiceInfo::new(
        &service_type(),
        &format!("relay-{short}"),
        &format!("relay-{short}.local."),
        "",
        port,
        &txt[..],
    )
    .map_err(|e| e.to_string())?
    .enable_addr_auto();
    let full = info.get_fullname().to_string();
    mdns.register(info).map_err(|e| e.to_string())?;
    Ok(full)
}

/// Where the trusted device `endpoint_id` advertises itself on this network.
fn find_device_addresses(endpoint_id: &str) -> Result<Vec<String>, String> {
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    let events = mdns.browse(&service_type()).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            if info.get_property_val_str("d") == Some("1")
                && info.get_property_val_str("id") == Some(endpoint_id)
            {
                let port = info.get_port();
                found.extend(
                    info.get_addresses()
                        .iter()
                        .map(|ip| std::net::SocketAddr::new(*ip, port).to_string()),
                );
                break;
            }
        }
    }
    let _ = mdns.shutdown();
    if found.is_empty() {
        return Err("that device isn't on this network (is Relay open on it?); \
                    or say where with --to host:port"
            .into());
    }
    Ok(found)
}

/// Held by the terminal whose word-code send or receive is running.
static SEND_OWNER: Mutex<()> = Mutex::new(());
static RECEIVE_OWNER: Mutex<()> = Mutex::new(());

/// Takes `owner` unless another terminal holds it.
fn own(owner: &Mutex<()>) -> Option<std::sync::MutexGuard<'_, ()>> {
    match owner.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Sends `paths` with a word code: advertises the code with mDNS while it
/// is open, tells the core which numbers other senders nearby use, and
/// prints what happens until the receiver has everything.
fn send_code(core: &Arc<RelayCore>, paths: &[String], io: &dyn Io) -> Result<(), String> {
    // One terminal owns the send from start to end, so its status and a
    // cancel when it closes are never another terminal's send.
    let _owner = own(&SEND_OWNER).ok_or("already sending")?;
    // The daemon first: a send started without it would run unadvertised.
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    if let Err(err) = core.send_paths_with_code(paths.to_vec()) {
        let _ = mdns.shutdown();
        return Err(err.to_string());
    }
    let done = Arc::new(AtomicBool::new(false));
    let watcher = watch_senders(Arc::clone(core), Arc::clone(&done));
    let mut advertised: Option<String> = None;
    let mut last = CodeSend::Idle;
    let result = loop {
        if !io.alive() {
            core.cancel_code_send();
            break Err("the terminal that started it closed".into());
        }
        let now = core.code_send_status();
        if now != last {
            match &now {
                CodeSend::Preparing { name, .. } => io.say(&format!("Getting {name} ready...")),
                CodeSend::Waiting {
                    code,
                    number,
                    port,
                    endpoint_id,
                    name,
                    size,
                    note,
                } => {
                    if let Some(note) = note {
                        io.say(note);
                    }
                    if let Some(old) = advertised.take() {
                        let _ = mdns.unregister(&old);
                    }
                    match advertise(&mdns, *number, *port, endpoint_id) {
                        Ok(full) => advertised = Some(full),
                        Err(err) => eprintln!("relay: couldn't advertise the code: {err}"),
                    }
                    io.say(&format!(
                        "{name}, {size} bytes. On the other device: relay receive {code}"
                    ));
                    io.say("or scan this with Relay on the phone:\n");
                    io.say(&qr(code));
                    io.say(&format!("\nCode: {code}"));
                }
                CodeSend::Sending { .. } => io.say("Sending..."),
                CodeSend::Sent {
                    size,
                    seconds,
                    megabytes_per_second,
                    ..
                } => {
                    io.say(&format!(
                        "Sent {size} bytes in {seconds:.1}s at {megabytes_per_second:.1} MB/s."
                    ));
                    break Ok(());
                }
                CodeSend::Failed { reason } => break Err(reason.clone()),
                CodeSend::Idle => {}
            }
            last = now;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if let Some(old) = advertised.take() {
        let _ = mdns.unregister(&old);
    }
    let _ = mdns.shutdown();
    done.store(true, Ordering::SeqCst);
    let _ = watcher.join();
    result
}

/// Advertises code number `number` at `port` for `endpoint_id`; returns the
/// service's full name, to withdraw it.
fn advertise(
    mdns: &mdns_sd::ServiceDaemon,
    number: u8,
    port: u16,
    endpoint_id: &str,
) -> Result<String, String> {
    let short = &endpoint_id[..8.min(endpoint_id.len())];
    let txt = [
        ("v", "1".to_string()),
        ("n", number.to_string()),
        ("id", endpoint_id.to_string()),
    ];
    let info = mdns_sd::ServiceInfo::new(
        &service_type(),
        &format!("relay-{short}-{number}"),
        &format!("relay-{short}.local."),
        "",
        port,
        &txt[..],
    )
    .map_err(|e| e.to_string())?
    .enable_addr_auto();
    let full = info.get_fullname().to_string();
    mdns.register(info).map_err(|e| e.to_string())?;
    Ok(full)
}

/// mDNS wants the domain on the service type.
fn service_type() -> String {
    format!("{CODE_SERVICE_TYPE}.local.")
}

/// A sender nearby: its code number, endpoint id and "ip:port"s.
fn read_sender(info: &mdns_sd::ServiceInfo) -> Option<(u8, String, Vec<String>)> {
    let number: u8 = info.get_property_val_str("n")?.parse().ok()?;
    let id = info.get_property_val_str("id")?.to_string();
    let port = info.get_port();
    let addresses = info
        .get_addresses()
        .iter()
        .map(|ip| std::net::SocketAddr::new(*ip, port).to_string())
        .collect();
    Some((number, id, addresses))
}

/// Keeps telling the core which code numbers other senders nearby use,
/// until `done`.
fn watch_senders(core: Arc<RelayCore>, done: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Ok(mdns) = mdns_sd::ServiceDaemon::new() else {
            return;
        };
        let Ok(events) = mdns.browse(&service_type()) else {
            return;
        };
        let mut seen: HashMap<String, NearbySender> = HashMap::new();
        while !done.load(Ordering::SeqCst) {
            let Ok(event) = events.recv_timeout(Duration::from_millis(300)) else {
                continue;
            };
            let changed = match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => match read_sender(&info) {
                    Some((number, endpoint_id, _)) => {
                        let full = info.get_fullname().to_string();
                        seen.insert(
                            full,
                            NearbySender {
                                number,
                                endpoint_id,
                            },
                        )
                        .is_none()
                    }
                    None => false,
                },
                mdns_sd::ServiceEvent::ServiceRemoved(_, full) => seen.remove(&full).is_some(),
                _ => false,
            };
            if changed {
                core.code_send_nearby(seen.values().cloned().collect());
            }
        }
        let _ = mdns.shutdown();
    })
}

/// Finds the sender advertising `code`'s number on this network and
/// receives from it. Two senders with that number is an error, never a
/// guess.
fn receive_code(core: &Arc<RelayCore>, code: &str, io: &dyn Io) -> Result<(), String> {
    // As with sends: a refused receive never cancels the running one.
    let _owner = own(&RECEIVE_OWNER).ok_or("already receiving")?;
    let number = word_code_number(code.to_string()).map_err(|e| e.to_string())?;
    let (id, addresses) = find_sender(number)?;
    let receiving = {
        let core = Arc::clone(core);
        let code = code.to_string();
        std::thread::spawn(move || block_on(core.receive_with_code(code, addresses, id)))
    };
    // Progress every two seconds, said here, where `io` lives.
    let mut last = Instant::now();
    while !receiving.is_finished() {
        std::thread::sleep(Duration::from_millis(200));
        if !io.alive() {
            core.cancel_code_receive();
        }
        if last.elapsed() < Duration::from_secs(2) {
            continue;
        }
        last = Instant::now();
        if let Some(size) = core.code_receive_size() {
            let percent = size.done * 100 / size.total.max(1);
            io.say(&format!(
                "{percent}% ({} of {} bytes)",
                size.done, size.total
            ));
        }
    }
    let got = receiving
        .join()
        .map_err(|_| "the receive ended unexpectedly".to_string())?
        .map_err(|e| e.to_string())?;
    io.say(&format!(
        "Received {} ({} bytes, {} already here) in {:.1}s at {:.1} MB/s: {}",
        got.name, got.size, got.resumed_from, got.seconds, got.megabytes_per_second, got.path
    ));
    Ok(())
}

/// The endpoint id and addresses of the one sender advertising `number`.
fn find_sender(number: u8) -> Result<(String, Vec<String>), String> {
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    let events = mdns.browse(&service_type()).map_err(|e| e.to_string())?;
    let mut deadline = Instant::now() + Duration::from_secs(10);
    let mut found: HashMap<String, HashSet<String>> = HashMap::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            if let Some((n, id, addresses)) = read_sender(&info) {
                if n == number {
                    if found.is_empty() {
                        // Keep listening a little for a second sender.
                        deadline = Instant::now() + Duration::from_millis(1500);
                    }
                    found.entry(id).or_default().extend(addresses);
                }
            }
        }
    }
    let _ = mdns.shutdown();
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (None, _) => Err(format!(
            "no sender with code number {number} on this network"
        )),
        (Some((id, addresses)), None) => Ok((id, addresses.into_iter().collect())),
        (Some(_), Some(_)) => Err(format!(
            "two senders nearby are using number {number}; ask the sender for a new code"
        )),
    }
}

/// The value after `flag`, taken out of `args`.
fn take_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    if i + 1 >= args.len() {
        return Err(format!("{flag} needs a value"));
    }
    let value = args.remove(i + 1);
    args.remove(i);
    Ok(Some(value))
}

fn serve_ai(core: &Arc<RelayCore>, args: &[String]) -> Result<(), String> {
    let mut args = args.to_vec();
    let upstream = take_flag(&mut args, "--upstream")?
        .ok_or("serve-ai needs --upstream, the server's OpenAI address (http://127.0.0.1:11434/v1 for Ollama)")?;
    let model = take_flag(&mut args, "--model")?;
    let client = Arc::new(forge_net::UreqClient::with_user_agent(concat!(
        "relay/",
        env!("CARGO_PKG_VERSION")
    )));
    let mut server = forge_services::ai::openai::OpenAiServer::new(client, &upstream);
    if let Ok(key) = std::env::var("RELAY_AI_KEY") {
        server = server.api_key(key);
    }
    let model = match model {
        Some(model) => model,
        None => {
            let models = server.models().map_err(|e| e.to_string())?;
            models
                .into_iter()
                .next()
                .ok_or("the server lists no models; load one, or name it with --model")?
        }
    };
    let server = server.model(&model);
    println!("Answering AI requests from trusted devices with {model} at {upstream}.");
    core.answer_asks(Some(Arc::new(server) as Arc<dyn LanguageModel>));
    listen(core, &args)
}

fn ask(core: &RelayCore, args: &[String]) -> Result<(), String> {
    let mut args = args.to_vec();
    let address = take_flag(&mut args, "--to")?;
    let model = take_flag(&mut args, "--model")?;
    let system = take_flag(&mut args, "--system")?;
    let max_tokens = match take_flag(&mut args, "--max-tokens")? {
        Some(n) => n.parse().map_err(|_| "--max-tokens takes a number")?,
        None => 512,
    };
    let [who, prompt] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let device = find_device(core, who)
        .filter(|d| d.trusted)
        .ok_or_else(|| format!("{who} isn't a trusted device; `relay devices` lists them"))?;
    if let Some(address) = address {
        core.devices_nearby(vec![relay_core::NearbyDevice {
            endpoint_id: device.endpoint_id.clone(),
            addresses: vec![address],
        }]);
    }
    let mut messages = Vec::new();
    if let Some(system) = system {
        messages.push(ChatMessage::system(system));
    }
    messages.push(ChatMessage::user(prompt.clone()));
    let request = ChatRequest {
        model,
        messages,
        max_tokens,
        temperature: 0.7,
    };
    let out = Collect::new().listen({
        let printed = std::sync::Mutex::new(0usize);
        move |so_far| {
            use std::io::Write;
            let mut printed = printed.lock().unwrap_or_else(|p| p.into_inner());
            print!("{}", &so_far[*printed..]);
            let _ = std::io::stdout().flush();
            *printed = so_far.len();
        }
    });
    let answer = core
        .ask(&device.endpoint_id, request, &out)
        .map_err(|e| e.to_string())?;
    println!();
    match answer {
        Ok(model) => {
            eprintln!("(answered by {model} on {})", device.name);
            Ok(())
        }
        Err(err) => Err(format!("{}'s model gave no answer: {err}", device.name)),
    }
}

/// How one of our messages stands, after its text: nothing once the
/// device has it.
fn message_status(m: &chat_core::Message) -> &'static str {
    match m.status {
        MessageStatus::Sent => "",
        MessageStatus::Failed => "  (not delivered)",
        _ if m.in_mailbox => "  (in its mailbox: it collects it when it next opens Relay)",
        _ => "  (waiting: it goes when the device is reached)",
    }
}

/// A message's text and files on one line.
fn message_line(m: &chat_core::Message) -> String {
    [m.body.clone()]
        .into_iter()
        .filter(|b| !b.is_empty())
        .chain(
            m.attachments
                .iter()
                .map(|a| format!("[file {}, {} bytes]", a.name, a.size)),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prints what changed in device conversations since `cursors` (per
/// device, a change stamp): messages that arrived, directly or from the
/// mailbox, and ours that were delivered or left in a mailbox. With
/// `quiet`, only moves the cursors (the first look, at start).
fn watch_messages(core: &RelayCore, cursors: &mut HashMap<String, i64>, quiet: bool) {
    for device in core.devices().into_iter().filter(|d| d.trusted) {
        let since = cursors.get(&device.endpoint_id).copied().unwrap_or(0);
        let Ok(changed) = core.device_messages_since(&device.endpoint_id, since) else {
            continue;
        };
        // In sent order, not change order: the cursor is the batch's
        // greatest stamp, kept once after it.
        let mut cursor = since;
        for m in changed {
            cursor = cursor.max(m.changed_at());
            if quiet {
                continue;
            }
            if m.author_id == device.endpoint_id {
                // Once: a message from a device only changes when it arrives.
                if m.updated_at.is_none() {
                    println!("from {}: {}", device.name, message_line(&m));
                }
            } else if m.status == MessageStatus::Sent {
                println!("to {}: {}, delivered", device.name, message_line(&m));
            } else if m.in_mailbox || m.status == MessageStatus::Failed {
                println!(
                    "to {}: {}{}",
                    device.name,
                    message_line(&m),
                    message_status(&m)
                );
            }
        }
        cursors.insert(device.endpoint_id.clone(), cursor);
    }
}

fn describe(t: &relay_core::Transfer) -> String {
    let arrow = match t.direction {
        Direction::Sent => format!("to {}", t.peer_name),
        Direction::Received => format!("from {}", t.peer_name),
    };
    let what = match &t.item {
        Item::Note { text } => format!("note \"{text}\""),
        Item::File { name, size, path } => match path {
            Some(path) => format!("file {name} ({size} bytes) at {path}"),
            None => format!("file {name} ({size} bytes)"),
        },
    };
    let status = match &t.status {
        TransferStatus::Done => "done".to_string(),
        TransferStatus::Waiting => {
            "waiting: it goes when the device is reached, while relay listen runs".to_string()
        }
        TransferStatus::InMailbox => {
            "in the mailbox: it arrives when the device next opens relay".to_string()
        }
        TransferStatus::InProgress { done } => format!("{done} bytes so far"),
        TransferStatus::Failed { reason } => format!("failed: {reason}"),
    };
    format!("{arrow}: {what}, {status}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_messages_say_where_they_are_and_nothing_once_delivered() {
        let mut m = chat_core::Message {
            body: "hi".into(),
            status: MessageStatus::Queued,
            ..Default::default()
        };
        assert!(message_status(&m).contains("waiting"));
        m.in_mailbox = true;
        assert!(message_status(&m).contains("mailbox"));
        m.status = MessageStatus::Failed;
        assert!(message_status(&m).contains("not delivered"));
        m.status = MessageStatus::Sent;
        assert_eq!(message_status(&m), "");
        assert_eq!(message_line(&m), "hi");
    }
}
