//! `cargo run -p chat-server [-- --bind 0.0.0.0:8787] [--data FILE]` (or
//! `make chat-server`). See `src/lib.rs` and `docs/chat-demo.md`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use chat_server::{router, AppState, DEFAULT_HEARTBEAT};

const USAGE: &str = "usage: chat-server [--bind ADDR:PORT] [--data FILE | --in-memory]
                   [--allow-anonymous] [--heartbeat-ms MS]
  --bind             address to listen on (default 127.0.0.1:8787; use 0.0.0.0:8787
                     to reach it from a phone on the same network)
  --data             where accounts and messages are kept (default
                     $XDG_DATA_HOME/ixos-chat/server.json, or ~/.local/share/...)
  --in-memory        keep nothing on disk (tests)
  --allow-anonymous  also accept requests without an account token, with the
                     author taken from the request (the frozen Android example, curl)
  --heartbeat-ms     idle-stream heartbeat interval (default 15000)";

struct Options {
    bind: SocketAddr,
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
        bind: "127.0.0.1:8787".parse().expect("valid default"),
        data: None,
        allow_anonymous: false,
        heartbeat: DEFAULT_HEARTBEAT,
    };
    let mut in_memory = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bind" => options.bind = value()?.parse().map_err(|e| format!("--bind: {e}"))?,
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

#[tokio::main]
async fn main() {
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

    let listener = match tokio::net::TcpListener::bind(options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("error: cannot listen on {}: {err}", options.bind);
            std::process::exit(1);
        }
    };
    // Print the real address (it differs from `bind` with port 0), so
    // scripts and tests can start the server on a free port. Tests read this
    // first line up to the space after the URL.
    let bind = listener.local_addr().unwrap_or(options.bind);
    let storage = match &options.data {
        Some(path) => format!("data in {}", path.display()),
        None => "in memory".into(),
    };
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
            "Reachable from other devices on your network. Anyone who can reach it can create an account."
        );
    }
    let app = router(state);
    // No graceful shutdown: open event streams never finish on their own,
    // and "kill the server" is part of the demo. Ctrl-C ends it at once;
    // every change is already on disk.
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
