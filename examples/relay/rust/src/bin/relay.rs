//! `relay`: the second Relay device, on a Linux (or macOS) machine. Same
//! code as the app, so one iPhone and a laptop are enough to try pairing
//! and transfers.
//!
//! ```text
//! relay listen [--port N]            wait for the phone; prints a pairing QR code,
//!                                    and takes what trusted devices send
//! relay pair <code>                  pair with a phone showing its code
//! relay peers                        paired devices (QR pairing)
//! relay devices                      trusted and revoked devices (on iroh)
//! relay send <device> (--note TEXT | PATH...) [--to host:port]
//!                                    send to a trusted device, no code
//! relay revoke <device>              stop trusting a device, for good
//! relay forget <device>              take a device off the list
//! relay trust <device>               trust a revoked device again
//! relay policy [codes-only | ask | automatic]
//!                                    what happens after a code transfer
//! relay send <peer> [--to host:port] (--note TEXT | FILE)
//! relay send PATH...                 send files and folders with a word code
//!                                    to anyone on this network
//! relay receive <code>               receive what someone sends with a code
//! relay history                      what went across, newest first
//! relay name [NEW]                   this machine's name in Relay
//! relay serve-ai --upstream URL [--model M] [--port N]
//!                                    listen, and answer paired devices'
//!                                    AI requests with an OpenAI-compatible
//!                                    server (llama.cpp, Ollama, LM Studio)
//! relay ask <peer> [--to host:port] [--model M] [--system TEXT]
//!           [--max-tokens N] PROMPT  ask a paired device's model; the
//!                                    answer streams to the terminal
//! ```
//!
//! `serve-ai` sends a bearer token from `$RELAY_AI_KEY` if it is set. Only
//! paired devices can ask, and the server's URL is never shared: keep it on
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
//! `send <device>`, `devices`, `revoke`, `forget`, `trust`, `policy`,
//! `peers` and `history` go to the listener over `relay.sock` in the data
//! folder (0600) and print what it answers, so listening and sending work
//! at the same time.

use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use forge_services::ai::{ChatMessage, ChatRequest, Collect, LanguageModel};
use relay_core::{
    word_code_number, CodeSend, Direction, Item, NearbySender, RelayCore, RelayError,
    TransferStatus, CODE_SERVICE_TYPE,
};

const USAGE: &str = "usage: relay listen [--port N] | pair <code> | peers | devices | \
send <peer> [--to host:port] (--note TEXT | FILE) | send PATH... | receive <code> | \
send <device> (--note TEXT | --text TEXT | PATH...) [--to host:port] | \
messages <device> | revoke <device> | forget <device> | \
trust <device> | policy [codes-only | ask | automatic] | \
history | name [NEW] | \
serve-ai --upstream URL [--model M] [--port N] | \
ask <peer> [--to host:port] [--model M] [--system TEXT] [--max-tokens N] PROMPT | \
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
    if let Some(result) = shared(&core, command, rest) {
        core.shutdown();
        return result.map(|out| print!("{out}"));
    }
    let result = match command.as_str() {
        "listen" => listen(&core, rest),
        "pair" => {
            let code = rest.first().ok_or(USAGE)?;
            let peer = block_on(core.pair(code.clone(), None)).map_err(|e| e.to_string())?;
            println!("Paired with {}.", peer.name);
            Ok(())
        }
        // `send <peer> ...` to a paired device; otherwise the arguments are
        // files and folders to send with a word code.
        "send" => match rest.first() {
            Some(first) if find_peer(&core, first).is_some() => send(&core, rest),
            Some(_) => send_code(&core, rest).and_then(|()| offer_trust(&core)),
            None => Err(USAGE.into()),
        },
        "receive" => match rest {
            [code] => receive_code(&core, code).and_then(|()| offer_trust(&core)),
            _ => Err(USAGE.into()),
        },
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
        "send-code" if !rest.is_empty() => send_code(&core, rest),
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

/// The commands that also work while `relay listen` runs, from another
/// terminal: they go to the listener (`control`), which runs them on its
/// core and sends back what they print. `None` for any other command.
fn shared(core: &RelayCore, command: &str, rest: &[String]) -> Option<Result<String, String>> {
    let mut out = String::new();
    let result = match command {
        "peers" => {
            for p in core.peers() {
                let at = p.last_address.unwrap_or_else(|| "not reached yet".into());
                out += &format!("{}  {}  ({at})\n", p.name, p.device_id);
            }
            Ok(())
        }
        "send" => match rest.first() {
            Some(first)
                if find_peer(core, first).is_none() && find_device(core, first).is_some() =>
            {
                send_to_device(core, rest).map(|line| out += &line)
            }
            _ => return None,
        },
        "messages" => (|| {
            let who = rest.first().ok_or(USAGE)?;
            let device = find_device(core, who)
                .ok_or_else(|| format!("no device {who}; `relay devices` lists them"))?;
            for m in core
                .device_messages(&device.endpoint_id)
                .map_err(|e| e.to_string())?
            {
                let who = if m.author_id == device.endpoint_id {
                    device.name.as_str()
                } else {
                    "me"
                };
                out += &format!("{who}: {}\n", m.body);
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
                out += &format!("{}  {state}{takes}  {}\n", d.name, d.endpoint_id);
            }
            out += &format!("Pairing policy: {}\n", policy_name(core.pairing_policy()));
            Ok(())
        }
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

    fn answer(core: &RelayCore, stream: UnixStream) {
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            return;
        }
        let reply = match serde_json::from_str::<Vec<String>>(&line) {
            Ok(args) => match args.split_first() {
                Some((command, rest)) => {
                    super::shared(core, command, rest).unwrap_or_else(|| Err(busy(command)))
                }
                None => Err(super::USAGE.into()),
            },
            Err(_) => Err("a garbled command".into()),
        };
        let reply = serde_json::to_string(&reply.map_err(|e| e.to_string()))
            .unwrap_or_else(|_| "null".into());
        let _ = (&stream).write_all(reply.as_bytes());
    }

    fn busy(command: &str) -> String {
        format!("`relay listen` is running with this data; stop it to use `relay {command}`")
    }

    /// Runs `args` on the running listener, printing what it printed.
    pub fn forward(args: &[String]) -> Result<(), String> {
        let command = args.first().map(String::as_str).unwrap_or("");
        let mut stream = UnixStream::connect(path())
            .map_err(|_| "Relay's data is in use by another relay process".to_string())?;
        let line = serde_json::to_string(args).map_err(|e| e.to_string())? + "\n";
        stream
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut reply = String::new();
        BufReader::new(stream)
            .read_line(&mut reply)
            .map_err(|e| e.to_string())?;
        match serde_json::from_str::<Result<String, String>>(&reply) {
            Ok(Ok(out)) => {
                print!("{out}");
                Ok(())
            }
            Ok(Err(problem)) => Err(problem),
            Err(_) => Err(busy(command)),
        }
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// This machine's address on the local network: the one the system would
/// send from. Nothing is sent.
fn local_address() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn listen(core: &Arc<RelayCore>, args: &[String]) -> Result<(), String> {
    let port = match args {
        [] => 0,
        [flag, n] if flag == "--port" => n.parse().map_err(|_| "--port takes a number")?,
        _ => return Err(USAGE.into()),
    };
    let port = core.start_listening(port).map_err(|e| e.to_string())?;
    let host = local_address().unwrap_or_else(|| "127.0.0.1".into());
    // Trusted devices reach this one on iroh while it listens.
    let service = block_on(core.device_service()).map_err(|e| e.to_string())?;
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    if let Err(err) = advertise_device(&mdns, service.port, &service.endpoint_id) {
        eprintln!("relay: trusted devices won't find this one: {err}");
    }
    let invitation = core
        .create_invitation(vec![host.clone()])
        .map_err(|e| e.to_string())?;
    println!(
        "Relay is listening on {host}:{port} as \"{}\".",
        core.identity().name
    );
    println!("To pair, scan this with Relay on the phone (valid for five minutes):\n");
    print_qr(&invitation.code);
    println!("\n{}\n", invitation.code);
    match control::serve(core.clone()) {
        Ok(()) => println!(
            "Waiting. Ctrl-C to stop. In another terminal, `relay send <device> ...`, \
             `devices`, `revoke`, `policy` and `history` use this listener."
        ),
        Err(err) => println!("Waiting. Ctrl-C to stop. (Other terminals can't use it: {err}.)"),
    }
    let mut seen = core.transfers().first().map(|t| t.id).unwrap_or(0);
    let mut peers = core.peers().len();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let now = core.peers();
        if now.len() > peers {
            if let Some(p) = now.last() {
                println!("Paired with {}.", p.name);
            }
            peers = now.len();
        }
        for t in core.transfers().into_iter().rev() {
            if t.id > seen
                && !matches!(
                    t.status,
                    TransferStatus::InProgress { .. } | TransferStatus::Waiting
                )
            {
                println!("{}", describe(&t));
                seen = t.id;
            }
        }
    }
}

#[cfg(feature = "cli")]
fn print_qr(code: &str) {
    use qrcode::render::unicode::Dense1x2;
    match qrcode::QrCode::new(code.as_bytes()) {
        Ok(qr) => println!(
            "{}",
            qr.render::<Dense1x2>()
                .dark_color(Dense1x2::Light)
                .light_color(Dense1x2::Dark)
                .quiet_zone(true)
                .build()
        ),
        Err(err) => println!("(no QR code: {err})"),
    }
}

fn send(core: &RelayCore, args: &[String]) -> Result<(), String> {
    let (who, mut rest) = args.split_first().ok_or(USAGE)?;
    let peer = find_peer(core, who)
        .ok_or_else(|| format!("not paired with {who}; `relay peers` lists them"))?;
    let mut addresses = Vec::new();
    if let [flag, address, more @ ..] = rest {
        if flag == "--to" {
            addresses.push(address.clone());
            rest = more;
        }
    }
    let transfer = match rest {
        [flag, text] if flag == "--note" => {
            block_on(core.send_note(peer.device_id, text.clone(), addresses, None))
        }
        [file] => block_on(core.send_file(peer.device_id, file.clone(), addresses, None)),
        _ => return Err(USAGE.into()),
    }
    .map_err(|e| e.to_string())?;
    println!("{}", describe(&transfer));
    Ok(())
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

fn find_device(core: &RelayCore, who: &str) -> Option<relay_core::Device> {
    core.devices()
        .into_iter()
        .find(|d| d.endpoint_id == who || d.name.eq_ignore_ascii_case(who))
}

/// After a code transfer, asks on the terminal whether to trust the other
/// device, when the pairing policy offers it.
fn offer_trust(core: &RelayCore) -> Result<(), String> {
    use std::io::{BufRead, IsTerminal, Write};
    let Some(offer) = core.trust_offer() else {
        return Ok(());
    };
    let name = if offer.name.is_empty() {
        "the other device".to_string()
    } else {
        format!("\"{}\"", offer.name)
    };
    if !std::io::stdin().is_terminal() {
        println!("Trust {name}? Not asked: this isn't a terminal.");
        return Ok(());
    }
    print!("Trust {name}? You'll be able to send to each other without a code. [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    if answer.trim().eq_ignore_ascii_case("y") {
        block_on(core.trust_device(offer.endpoint_id)).map_err(|e| e.to_string())?;
        println!("Trusted. `relay devices` lists it.");
    } else {
        core.dismiss_trust_offer();
    }
    Ok(())
}

/// `send <device> (--note TEXT | PATH...) [--to host:port]`: to a trusted
/// device, found with mDNS unless `--to` says where.
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
    if let Some(text) = text {
        if note.is_some() || !paths.is_empty() {
            return Err(USAGE.into());
        }
        block_on(core.send_device_message(&device.endpoint_id, text)).map_err(|e| e.to_string())?;
        return Ok(format!("to {}: message sent\n", device.name));
    }
    let transfer = match (note, paths) {
        (Some(text), []) => block_on(core.send_note_to_device(device.endpoint_id, text)),
        (None, paths) if !paths.is_empty() => {
            block_on(core.send_to_device(device.endpoint_id, paths.to_vec()))
        }
        _ => return Err(USAGE.into()),
    }
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

fn find_peer(core: &RelayCore, who: &str) -> Option<relay_core::Peer> {
    core.peers()
        .into_iter()
        .find(|p| p.device_id == who || p.name.eq_ignore_ascii_case(who))
}

/// Sends `paths` with a word code: advertises the code with mDNS while it
/// is open, tells the core which numbers other senders nearby use, and
/// prints what happens until the receiver has everything.
fn send_code(core: &Arc<RelayCore>, paths: &[String]) -> Result<(), String> {
    core.send_paths_with_code(paths.to_vec())
        .map_err(|e| e.to_string())?;
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    let done = Arc::new(AtomicBool::new(false));
    let watcher = watch_senders(Arc::clone(core), Arc::clone(&done));
    let mut advertised: Option<String> = None;
    let mut last = CodeSend::Idle;
    let result = loop {
        let now = core.code_send_status();
        if now != last {
            match &now {
                CodeSend::Preparing { name, .. } => println!("Getting {name} ready..."),
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
                        println!("{note}");
                    }
                    if let Some(old) = advertised.take() {
                        let _ = mdns.unregister(&old);
                    }
                    match advertise(&mdns, *number, *port, endpoint_id) {
                        Ok(full) => advertised = Some(full),
                        Err(err) => eprintln!("relay: couldn't advertise the code: {err}"),
                    }
                    println!("{name}, {size} bytes. On the other device: relay receive {code}");
                    println!("Code: {code}");
                }
                CodeSend::Sending { .. } => println!("Sending..."),
                CodeSend::Sent {
                    size,
                    seconds,
                    megabytes_per_second,
                    ..
                } => {
                    println!(
                        "Sent {size} bytes in {seconds:.1}s at {megabytes_per_second:.1} MB/s."
                    );
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
fn receive_code(core: &Arc<RelayCore>, code: &str) -> Result<(), String> {
    let number = word_code_number(code.to_string()).map_err(|e| e.to_string())?;
    let (id, addresses) = find_sender(number)?;
    let done = Arc::new(AtomicBool::new(false));
    let progress = {
        let core = Arc::clone(core);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut last = Instant::now();
            while !done.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(200));
                if last.elapsed() < Duration::from_secs(2) {
                    continue;
                }
                last = Instant::now();
                if let Some(size) = core.code_receive_size() {
                    let percent = size.done * 100 / size.total.max(1);
                    println!("{percent}% ({} of {} bytes)", size.done, size.total);
                }
            }
        })
    };
    let got = block_on(core.receive_with_code(code.to_string(), addresses, id));
    done.store(true, Ordering::SeqCst);
    let _ = progress.join();
    let got = got.map_err(|e| e.to_string())?;
    println!(
        "Received {} ({} bytes, {} already here) in {:.1}s at {:.1} MB/s: {}",
        got.name, got.size, got.resumed_from, got.seconds, got.megabytes_per_second, got.path
    );
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
    println!("Answering AI requests from paired devices with {model} at {upstream}.");
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
    let peer = core
        .peers()
        .into_iter()
        .find(|p| &p.device_id == who || p.name.eq_ignore_ascii_case(who))
        .ok_or_else(|| format!("not paired with {who}; `relay peers` lists them"))?;
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
        .ask(
            &peer.device_id,
            request,
            address.into_iter().collect(),
            &out,
        )
        .map_err(|e| e.to_string())?;
    println!();
    match answer {
        Ok(model) => {
            eprintln!("(answered by {model} on {})", peer.name);
            Ok(())
        }
        Err(err) => Err(format!("{}'s model gave no answer: {err}", peer.name)),
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
        TransferStatus::InProgress { done } => format!("{done} bytes so far"),
        TransferStatus::Failed { reason } => format!("failed: {reason}"),
    };
    format!("{arrow}: {what}, {status}")
}
