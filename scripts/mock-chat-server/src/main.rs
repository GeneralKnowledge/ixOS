//! `cargo run -p mock-chat-server [-- --bind 127.0.0.1:8787] [--heartbeat-ms 15000]`
//! (or `make mock-chat`). See `src/lib.rs` and `docs/chat-demo.md`.

use std::net::SocketAddr;
use std::time::Duration;

use mock_chat_server::{router, AppState, DEFAULT_HEARTBEAT, ROOMS};

const USAGE: &str = "usage: mock-chat-server [--bind ADDR:PORT] [--heartbeat-ms MS]
  --bind          address to listen on (default 127.0.0.1:8787; use 0.0.0.0:8787
                  to reach it from a phone on the same network)
  --heartbeat-ms  idle-stream heartbeat interval (default 15000)";

fn parse_args() -> Result<(SocketAddr, Duration), String> {
    let mut bind: SocketAddr = "127.0.0.1:8787".parse().expect("valid default");
    let mut heartbeat = DEFAULT_HEARTBEAT;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bind" => bind = value()?.parse().map_err(|e| format!("--bind: {e}"))?,
            "--heartbeat-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|e| format!("--heartbeat-ms: {e}"))?;
                if ms == 0 {
                    return Err("--heartbeat-ms must be > 0".into());
                }
                heartbeat = Duration::from_millis(ms);
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok((bind, heartbeat))
}

#[tokio::main]
async fn main() {
    let (bind, heartbeat) = match parse_args() {
        Ok(parsed) => parsed,
        Err(problem) => {
            if !problem.is_empty() {
                eprintln!("error: {problem}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("error: cannot listen on {bind}: {err}");
            std::process::exit(1);
        }
    };
    // Print the real address (it differs from `bind` with port 0), so
    // scripts and tests can start the server on a free port.
    let bind = listener.local_addr().unwrap_or(bind);
    let rooms: Vec<&str> = ROOMS.iter().map(|(id, _)| *id).collect();
    println!(
        "mock chat server on http://{bind} (rooms: {}; heartbeat {} ms). Ctrl-C to stop.",
        rooms.join(", "),
        heartbeat.as_millis()
    );
    let app = router(AppState::new(heartbeat));
    // No graceful shutdown: open event streams never finish on their own,
    // and "kill the server" is part of the demo. Ctrl-C ends it at once.
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
