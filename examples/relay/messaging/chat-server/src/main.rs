//! `cargo run -p chat-server [-- --data FILE] [--http 0.0.0.0:8787]` (or
//! `make chat-server`). See `src/lib.rs`, `src/room.rs` and
//! `examples/relay/docs/chat-demo.md`.
//!
//! Rooms are served on `relay/room/1` over iroh (N4). The old HTTP API
//! runs only with `--http` (or its old name `--bind`), for moving token
//! accounts, tests, `curl` and the frozen Android example.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use chat_server::{room, router, AppState, DEFAULT_HEARTBEAT};

const USAGE: &str = "usage: chat-server [--data FILE | --in-memory] [--http ADDR:PORT]
                   [--port N] [--relay URL] [--allow-anonymous] [--heartbeat-ms MS]
       chat-server invite|accounts|export [--data FILE]
  (serving)          rooms on relay/room/1 over iroh, always
  --http, --bind     also serve the old HTTP API on this address (off by default;
                     0.0.0.0:8787 to move a phone's token account over)
  --port             UDP port for relay/room/1 (default: any)
  --relay            Relay's relay server, so devices away from home reach it
  invite             a one-time word code to join (ten minutes)
  accounts           the accounts, and which are still on a token
  export             every room's full history as JSON, on stdout
  --data             where accounts and messages are kept (default
                     $XDG_DATA_HOME/ixos-chat/server.json, or ~/.local/share/...)
  --in-memory        keep nothing on disk (tests)
  --allow-anonymous  also accept requests without an account token, with the
                     author taken from the request (the frozen Android example, curl)
  --heartbeat-ms     idle-stream heartbeat interval (default 15000)";

struct Options {
    bind: Option<SocketAddr>,
    port: u16,
    relay: Option<String>,
    data: Option<PathBuf>,
    allow_anonymous: bool,
    heartbeat: Duration,
}

fn default_data_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("ixos-chat").join("server.json"))
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options {
        bind: None,
        port: 0,
        relay: None,
        data: None,
        allow_anonymous: false,
        heartbeat: DEFAULT_HEARTBEAT,
    };
    let mut in_memory = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bind" | "--http" => {
                options.bind = Some(value()?.parse().map_err(|e| format!("{arg}: {e}"))?)
            }
            "--port" => options.port = value()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--relay" => options.relay = Some(value()?),
            "--data" => options.data = Some(PathBuf::from(value()?)),
            "--in-memory" => in_memory = true,
            "--allow-anonymous" => options.allow_anonymous = true,
            "--heartbeat-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|e| format!("--heartbeat-ms: {e}"))?;
                if ms == 0 {
                    return Err("--heartbeat-ms must be > 0".into());
                }
                options.heartbeat = Duration::from_millis(ms);
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if in_memory && options.data.is_some() {
        return Err("--data and --in-memory cannot be used together".into());
    }
    if !in_memory && options.data.is_none() {
        options.data =
            Some(default_data_file().ok_or("no HOME or XDG_DATA_HOME; pass --data FILE")?);
    }
    Ok(options)
}

/// `chat-server invite|accounts|export [--data FILE]`: works on the data
/// file without starting a server (and while one runs).
fn admin(command: &str) -> Result<(), String> {
    let mut data = None;
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = Some(PathBuf::from(args.next().ok_or("--data needs a value")?)),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let data = match data {
        Some(path) => path,
        None => default_data_file().ok_or("no HOME or XDG_DATA_HOME; pass --data FILE")?,
    };
    match command {
        "invite" => {
            let code = room::add_invite(&data).map_err(|e| e.to_string())?;
            let dir = data.parent().unwrap_or(std::path::Path::new("."));
            let host = room::host_key(dir).map_err(|e| e.to_string())?.public();
            println!("{code}");
            println!(
                "Enter it under Chats, Join a Room, within ten minutes. It works once. This host is {host}."
            );
        }
        "accounts" => {
            let accounts = room::accounts(&data).map_err(|e| e.to_string())?;
            let left = accounts.iter().filter(|a| a.on_token).count();
            for a in &accounts {
                let how = match (&a.key, a.on_token) {
                    (_, true) => "still on a token".to_string(),
                    (Some(key), false) => format!("key {key}"),
                    (None, false) => "no way in".to_string(),
                };
                println!("{}  {}  {how}", a.display_name, a.user_id);
            }
            if left == 0 {
                println!("No account is on a token: the server can run without --http.");
            } else {
                println!("{left} account(s) still on a token: keep --http until they move.");
            }
        }
        "export" => {
            let all = room::export(&data).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Some(command) = std::env::args()
        .nth(1)
        .filter(|c| matches!(c.as_str(), "invite" | "accounts" | "export"))
    {
        if let Err(problem) = admin(&command) {
            eprintln!("error: {problem}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
        return;
    }
    let options = match parse_args() {
        Ok(parsed) => parsed,
        Err(problem) => {
            if !problem.is_empty() {
                eprintln!("error: {problem}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let state = match &options.data {
        Some(path) => match AppState::open(path, options.heartbeat) {
            Ok(state) => state,
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        },
        None => AppState::new(options.heartbeat),
    }
    .allow_anonymous(options.allow_anonymous);
    let storage = match &options.data {
        Some(path) => format!("data in {}", path.display()),
        None => "in memory".into(),
    };

    // The old HTTP API, only when asked. Its line comes first: tests and
    // scripts read the URL up to the space after it.
    let http = match options.bind {
        None => None,
        Some(bind) => {
            let listener = match tokio::net::TcpListener::bind(bind).await {
                Ok(listener) => listener,
                Err(err) => {
                    eprintln!("error: cannot listen on {bind}: {err}");
                    std::process::exit(1);
                }
            };
            let bind = listener.local_addr().unwrap_or(bind);
            println!(
                "chat server on http://{bind} ({storage}; {} accounts; {}heartbeat {} ms). Ctrl-C to stop.",
                state.account_count(),
                if options.allow_anonymous {
                    "anonymous posts allowed; "
                } else {
                    ""
                },
                options.heartbeat.as_millis()
            );
            if bind.ip().is_unspecified() {
                println!(
                    "HTTP is reachable from other devices on your network, in the clear. Keep it only until every account has moved (chat-server accounts)."
                );
            }
            Some(listener)
        }
    };

    // relay/room/1: always.
    let key = match &options.data {
        Some(path) => match room::host_key(path.parent().unwrap_or(std::path::Path::new("."))) {
            Ok(key) => key,
            Err(err) => {
                eprintln!("error: the host key: {err}");
                std::process::exit(1);
            }
        },
        None => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).expect("randomness");
            iroh::SecretKey::from_bytes(&bytes)
        }
    };
    let mut builder = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .secret_key(key)
        .relay_mode(match &options.relay {
            Some(url) => match url.parse::<iroh::RelayUrl>() {
                Ok(url) => iroh::RelayMode::Custom(iroh::RelayMap::from(url)),
                Err(err) => {
                    eprintln!("error: --relay: {err}");
                    std::process::exit(2);
                }
            },
            None => iroh::RelayMode::Disabled,
        });
    if options.port != 0 {
        builder = match builder.clear_ip_transports().bind_addr(SocketAddr::from((
            std::net::Ipv4Addr::UNSPECIFIED,
            options.port,
        ))) {
            Ok(b) => b,
            Err(err) => {
                eprintln!("error: --port: {err}");
                std::process::exit(1);
            }
        };
    }
    let endpoint = match builder.bind().await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            eprintln!("error: relay/room/1: {err}");
            std::process::exit(1);
        }
    };
    println!(
        "rooms on relay/room/1 as {} ({storage}). `chat-server invite` makes a code to join.",
        endpoint.id()
    );
    let _rooms = room::serve(state.clone(), endpoint);

    // No graceful shutdown: open event streams never finish on their own,
    // and "kill the server" is part of the demo. Ctrl-C ends it at once;
    // every change is already on disk.
    match http {
        Some(listener) => {
            if let Err(err) = axum::serve(listener, router(state)).await {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
        None => std::future::pending::<()>().await,
    }
}
