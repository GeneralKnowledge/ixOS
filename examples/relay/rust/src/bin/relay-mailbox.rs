//! `relay-mailbox`: Relay's encrypted mailbox server (N3,
//! examples/relay/docs/relay-mailbox.md). It holds sealed items for
//! devices that are closed, and can't read them.
//!
//! ```text
//! relay-mailbox [--data DIR] [--port N] [--relay URL] [--lookup URL]
//! ```
//!
//! Prints the endpoint id devices dial it by, then serves until stopped.

const USAGE: &str = "usage: relay-mailbox [--data DIR] [--port N] [--relay URL] [--lookup URL]";

fn main() {
    let mut options = relay_core::mailbox_server::ServeOptions {
        dir: "/var/lib/relay-mailbox".into(),
        ..Default::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next().unwrap_or_else(|| {
                eprintln!("{USAGE}");
                std::process::exit(2)
            })
        };
        let url = |text: String| {
            text.parse().unwrap_or_else(|e| {
                eprintln!("relay-mailbox: {text} isn't a URL: {e}");
                std::process::exit(2)
            })
        };
        match arg.as_str() {
            "--data" => options.dir = value().into(),
            "--port" => {
                options.port = value().parse().unwrap_or_else(|_| {
                    eprintln!("{USAGE}");
                    std::process::exit(2)
                })
            }
            "--relay" => options.relay = Some(url(value())),
            "--lookup" => options.lookup = Some(url(value())),
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2)
            }
        }
    }
    match relay_core::mailbox_server::serve(options) {
        Ok(never) => match never {},
        Err(e) => {
            eprintln!("relay-mailbox: {e}");
            std::process::exit(1)
        }
    }
}
