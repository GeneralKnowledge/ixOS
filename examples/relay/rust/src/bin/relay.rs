//! `relay`: the second Relay device, on a Linux (or macOS) machine. Same
//! code as the app, so one iPhone and a laptop are enough to try pairing
//! and transfers.
//!
//! ```text
//! relay listen [--port N]            wait for the phone; prints a pairing QR code
//! relay pair <code>                  pair with a phone showing its code
//! relay peers                        paired devices
//! relay send <peer> [--to host:port] (--note TEXT | FILE)
//! relay history                      what went across, newest first
//! relay name [NEW]                   this machine's name in Relay
//! ```
//!
//! Data lives in `$RELAY_DATA`, else `$XDG_DATA_HOME/forge-relay`, else
//! `~/.local/share/forge-relay`. Received files are in its `inbox/`.

use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use relay_core::{Direction, Item, RelayCore, TransferStatus};

const USAGE: &str = "usage: relay listen [--port N] | pair <code> | peers | \
send <peer> [--to host:port] (--note TEXT | FILE) | history | name [NEW]";

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

fn open() -> Result<Arc<RelayCore>, String> {
    let host = std::fs::read_to_string("/etc/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|_| "Linux".into());
    RelayCore::new(data_dir().to_string_lossy().into_owned(), host).map_err(|e| e.to_string())
}

fn run(args: Vec<String>) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or(USAGE)?;
    let core = open()?;
    let result = match command.as_str() {
        "listen" => listen(&core, rest),
        "pair" => {
            let code = rest.first().ok_or(USAGE)?;
            let peer = block_on(core.pair(code.clone(), None)).map_err(|e| e.to_string())?;
            println!("Paired with {}.", peer.name);
            Ok(())
        }
        "peers" => {
            for p in core.peers() {
                let at = p.last_address.unwrap_or_else(|| "not reached yet".into());
                println!("{}  {}  ({at})", p.name, p.device_id);
            }
            Ok(())
        }
        "send" => send(&core, rest),
        "history" => {
            for t in core.transfers() {
                println!("{}", describe(&t));
            }
            Ok(())
        }
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

fn listen(core: &RelayCore, args: &[String]) -> Result<(), String> {
    let port = match args {
        [] => 0,
        [flag, n] if flag == "--port" => n.parse().map_err(|_| "--port takes a number")?,
        _ => return Err(USAGE.into()),
    };
    let port = core.start_listening(port).map_err(|e| e.to_string())?;
    let host = local_address().unwrap_or_else(|| "127.0.0.1".into());
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
    println!("Waiting. Ctrl-C to stop.");
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
            if t.id > seen && !matches!(t.status, TransferStatus::InProgress { .. }) {
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
    let peer = core
        .peers()
        .into_iter()
        .find(|p| &p.device_id == who || p.name.eq_ignore_ascii_case(who))
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
        TransferStatus::InProgress { done } => format!("{done} bytes so far"),
        TransferStatus::Failed { reason } => format!("failed: {reason}"),
    };
    format!("{arrow}: {what}, {status}")
}
