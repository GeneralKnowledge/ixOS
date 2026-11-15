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
//! `~/.local/share/forge-relay`. Received files are in its `inbox/`.

use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use forge_services::ai::{ChatMessage, ChatRequest, Collect, LanguageModel};
use relay_core::{Direction, Item, RelayCore, TransferStatus};

const USAGE: &str = "usage: relay listen [--port N] | pair <code> | peers | \
send <peer> [--to host:port] (--note TEXT | FILE) | history | name [NEW] | \
serve-ai --upstream URL [--model M] [--port N] | \
ask <peer> [--to host:port] [--model M] [--system TEXT] [--max-tokens N] PROMPT | \
receive-code <code> <host:port> <endpoint-id>";

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
        "serve-ai" => serve_ai(&core, rest),
        "ask" => ask(&core, rest),
        // The receiver the app uses for word codes, against `relay-spike send`.
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

fn serve_ai(core: &RelayCore, args: &[String]) -> Result<(), String> {
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
        TransferStatus::InProgress { done } => format!("{done} bytes so far"),
        TransferStatus::Failed { reason } => format!("failed: {reason}"),
    };
    format!("{arrow}: {what}, {status}")
}
