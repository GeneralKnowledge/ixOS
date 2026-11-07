//! The app end of the Forge Device live connection (M1).
//!
//! A development build starts [`DevTools`] when the app opens. While the
//! app is on screen it listens on the phone's loopback interface
//! ([`forge_devproto::DEFAULT_PORT`]); `forge device` on the computer
//! reaches that port through usbmuxd over the USB cable. Nothing listens on
//! Wi-Fi.
//!
//! What the host gets, after pairing:
//!
//! - every `forge_runtime::logging` record and every lifecycle change, as a
//!   numbered stream that survives the app going to the background (the
//!   host reconnects and gets what it missed);
//! - status, and the values of the state probes the app registered with
//!   [`DevTools::probe`];
//! - the commands the app registered with [`DevTools::command`], and
//!   nothing else.
//!
//! This crate is linked only when the app crate's `devtools` Cargo feature
//! is on (`forge run ios --devtools`), so production builds contain none of
//! it. Never expose secrets through a probe or a command: tokens, Keychain
//! values, message bodies.

use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use forge_devproto::client::{format_secret, parse_secret};
use forge_devproto::conn::{Conn, ConnError};
use forge_devproto::secure::{Purpose, Role, MAX_PLAINTEXT};
use forge_devproto::{
    decode, AppMsg, ByeReason, Command, CommandInfo, Hello, HostMsg, LogLine, Open, Outbox,
    Preamble, Probe, Record, Reply, Secret, Status, SECRET_LEN,
};
use forge_runtime::logging::{self, LogSink, SinkId};
use forge_runtime::{lock, now_ms, Lifecycle, Phase};

/// Records kept for a host that reconnects.
pub const DEFAULT_BUFFER: usize = 2_000;

/// A new pairing may replace an existing one only this soon after the app
/// started, so re-pairing needs someone who can relaunch the app.
pub const PAIRING_WINDOW: Duration = Duration::from_secs(120);

/// Longest log message sent; longer ones are cut.
const MAX_MESSAGE: usize = 8 * 1024;
/// How often the server checks the phase and the stop flag.
const TICK: Duration = Duration::from_millis(100);
/// Send a heartbeat after this much silence.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// End a session after this much silence from the host.
const SILENCE: Duration = Duration::from_secs(15);
/// Each step of opening a connection.
const STEP: Duration = Duration::from_secs(5);

/// Where and as what the development server runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The app's identity, normally its bundle ID.
    pub app: String,
    /// The build, for example the crate version.
    pub build: String,
    /// Loopback port to listen on; 0 picks a free one (tests).
    pub port: u16,
    /// Where the pairing secret is kept, readable only by the app. Put it in
    /// the app's own storage, so deleting the app forgets the pairing.
    pub pairing_file: PathBuf,
    /// Records kept for reconnecting hosts.
    pub buffer: usize,
    /// How long after start a new pairing may replace an existing one.
    pub pairing_window: Duration,
}

impl Config {
    pub fn new(app: impl Into<String>, build: impl Into<String>, pairing_file: PathBuf) -> Self {
        Self {
            app: app.into(),
            build: build.into(),
            port: forge_devproto::DEFAULT_PORT,
            pairing_file,
            buffer: DEFAULT_BUFFER,
            pairing_window: PAIRING_WINDOW,
        }
    }
}

type ProbeFn = Box<dyn Fn() -> String + Send + Sync>;
type CommandFn = Box<dyn Fn(&[String]) -> Result<String, String> + Send + Sync>;

struct Shared {
    config: Config,
    lifecycle: Lifecycle,
    outbox: Mutex<Outbox>,
    probes: RwLock<Vec<(String, ProbeFn)>>,
    commands: RwLock<Vec<(String, String, CommandFn)>>,
    stop: AtomicBool,
    started: Instant,
    started_ms: i64,
    last_phase: Mutex<Phase>,
    port: Mutex<Option<u16>>,
}

/// The running development server. Stops when dropped.
pub struct DevTools {
    shared: Arc<Shared>,
    sink: SinkId,
    thread: Option<JoinHandle<()>>,
}

impl DevTools {
    /// Starts listening (while the app is in the foreground) on a background
    /// thread, and starts recording logs and lifecycle changes.
    pub fn start(config: Config, lifecycle: Lifecycle) -> io::Result<Self> {
        logging::init();
        let phase = lifecycle.phase();
        let shared = Arc::new(Shared {
            outbox: Mutex::new(Outbox::new(config.buffer)),
            config,
            lifecycle,
            probes: RwLock::new(Vec::new()),
            commands: RwLock::new(Vec::new()),
            stop: AtomicBool::new(false),
            started: Instant::now(),
            started_ms: now_ms(),
            last_phase: Mutex::new(phase),
            port: Mutex::new(None),
        });
        lock(&shared.outbox).push(Record::Phase {
            phase: format!("{phase:?}"),
        });
        let sink = logging::add_sink(Arc::new(OutboxSink(Arc::downgrade(&shared))));
        let thread = std::thread::Builder::new()
            .name("forge-devtools".into())
            .spawn({
                let shared = shared.clone();
                move || serve_forever(&shared)
            })?;
        Ok(Self {
            shared,
            sink,
            thread: Some(thread),
        })
    }

    /// Exposes a named value (`outbox`, `connection`, `rooms`) that a host can
    /// read with `forge device snapshot`. `read` runs on the server thread;
    /// keep it quick and never return secrets or user content.
    pub fn probe(&self, name: &str, read: impl Fn() -> String + Send + Sync + 'static) {
        let mut probes = self
            .shared
            .probes
            .write()
            .unwrap_or_else(|p| p.into_inner());
        probes.retain(|(n, _)| n != name);
        probes.push((name.into(), Box::new(read)));
    }

    /// Adds a command a host can run with `forge device cmd <name> [args]`.
    /// `run` gets the arguments and returns text for the host, or an error.
    pub fn command(
        &self,
        name: &str,
        help: &str,
        run: impl Fn(&[String]) -> Result<String, String> + Send + Sync + 'static,
    ) {
        let mut commands = self
            .shared
            .commands
            .write()
            .unwrap_or_else(|p| p.into_inner());
        commands.retain(|(n, _, _)| n != name);
        commands.push((name.into(), help.into(), Box::new(run)));
    }

    /// The port the server is listening on right now, if it is.
    pub fn port(&self) -> Option<u16> {
        *lock(&self.shared.port)
    }

    /// Stops the server and the recording. Also happens on drop.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        logging::remove_sink(self.sink);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for DevTools {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Copies every log record into the outbox. Must not log.
struct OutboxSink(std::sync::Weak<Shared>);

impl LogSink for OutboxSink {
    fn write(&self, record: &logging::LogRecord) {
        let Some(shared) = self.0.upgrade() else {
            return;
        };
        let mut message = record.message.clone();
        if message.len() > MAX_MESSAGE {
            let mut cut = MAX_MESSAGE;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
            message.push_str(" …");
        }
        lock(&shared.outbox).push(Record::Log(LogLine {
            time_ms: record.time_ms,
            level: record.level.to_string(),
            target: record.target.clone(),
            message,
        }));
    }
}

impl Shared {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Records a lifecycle change since the last look; returns the phase.
    fn note_phase(&self) -> Phase {
        let phase = self.lifecycle.phase();
        let mut last = lock(&self.last_phase);
        if *last != phase {
            *last = phase;
            lock(&self.outbox).push(Record::Phase {
                phase: format!("{phase:?}"),
            });
        }
        phase
    }

    fn secret(&self) -> Option<Secret> {
        let text = std::fs::read_to_string(&self.config.pairing_file).ok()?;
        parse_secret(&text)
    }

    fn pairing_open(&self, paired: bool) -> bool {
        !paired || self.started.elapsed() < self.config.pairing_window
    }

    fn status(&self) -> Status {
        let outbox = lock(&self.outbox);
        Status {
            app: self.config.app.clone(),
            build: self.config.build.clone(),
            phase: format!("{:?}", self.lifecycle.phase()),
            uptime_ms: self.started.elapsed().as_millis() as u64,
            buffered: outbox.len() as u64,
            oldest_seq: outbox.oldest_seq(),
            next_seq: outbox.next_seq(),
            probes: self
                .probes
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(|(n, _)| n.clone())
                .collect(),
            commands: self
                .commands
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .map(|(n, _, _)| n.clone())
                .collect(),
        }
    }

    fn run(&self, command: Command) -> Result<Reply, String> {
        Ok(match command {
            Command::Ping => Reply::Pong,
            Command::Status => Reply::Status(self.status()),
            Command::Snapshot => Reply::Snapshot(
                self.probes
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .map(|(name, read)| Probe {
                        name: name.clone(),
                        value: read(),
                    })
                    .collect(),
            ),
            Command::ListCommands => Reply::Commands(
                self.commands
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .map(|(name, help, _)| CommandInfo {
                        name: name.clone(),
                        help: help.clone(),
                    })
                    .collect(),
            ),
            Command::Unpair => {
                match std::fs::remove_file(&self.config.pairing_file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("could not delete the pairing: {e}")),
                }
                Reply::Unpaired
            }
            Command::App { name, args } => {
                let commands = self.commands.read().unwrap_or_else(|p| p.into_inner());
                let (_, _, run) = commands
                    .iter()
                    .find(|(n, _, _)| *n == name)
                    .ok_or_else(|| format!("the app has no command named {name:?}"))?;
                let mut out = run(&args)?;
                if out.len() > MAX_PLAINTEXT / 2 {
                    out.truncate(MAX_PLAINTEXT / 2);
                }
                Reply::Done(out)
            }
        })
    }
}

/// Writes the secret readable only by the app.
fn store_secret(path: &Path, secret: &Secret) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(path)?, format_secret(secret).as_bytes())
}

/// Listens while the app is in the foreground, one connection at a time.
fn serve_forever(shared: &Shared) {
    let mut listener: Option<TcpListener> = None;
    let mut bind_failed = false;
    while !shared.stopping() {
        if !shared.note_phase().is_foreground() {
            // iOS suspends a background app; stop listening so a host gets a
            // refused connection, not a hang, and another app can listen.
            if listener.take().is_some() {
                *lock(&shared.port) = None;
            }
            std::thread::sleep(TICK);
            continue;
        }
        let Some(open) = listener.as_ref() else {
            match bind(shared.config.port) {
                Ok((l, port)) => {
                    *lock(&shared.port) = Some(port);
                    bind_failed = false;
                    logging::info!(target: "forge-devtools", "listening on 127.0.0.1:{port}");
                    listener = Some(l);
                }
                Err(e) => {
                    if !bind_failed {
                        logging::warn!(target: "forge-devtools", "cannot listen on port {}: {e}", shared.config.port);
                        bind_failed = true;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            continue;
        };
        match open.accept() {
            Ok((stream, _)) => {
                if let Err(e) = connection(shared, stream) {
                    if !matches!(e, ConnError::Closed) {
                        logging::debug!(target: "forge-devtools", "connection ended: {e}");
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(TICK),
            Err(_) => std::thread::sleep(TICK),
        }
    }
}

fn bind(port: u16) -> io::Result<(TcpListener, u16)> {
    // Loopback only: reachable through usbmuxd, never from the network.
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

fn connection(shared: &Shared, stream: TcpStream) -> Result<(), ConnError> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(TICK))?;
    stream.set_nodelay(true)?;
    let mut conn = Conn::new(stream);
    let secret = shared.secret();
    let pairing_open = shared.pairing_open(secret.is_some());
    conn.send_clear(&Preamble::new(
        &shared.config.app,
        secret.is_some(),
        pairing_open,
    ))?;
    let open: Open = decode(&conn.recv_clear_bytes(STEP)?).map_err(ConnError::from)?;
    match (open, secret) {
        (Open::Pair, _) if pairing_open => pair(shared, conn),
        (Open::Session, Some(secret)) => {
            conn.handshake(Role::App, Purpose::Session(&secret), STEP)?;
            session(shared, conn)
        }
        // Pairing closed, or a session without a pairing: the preamble
        // already told the host why; just hang up.
        _ => Ok(()),
    }
}

fn pair(shared: &Shared, mut conn: Conn<TcpStream>) -> Result<(), ConnError> {
    conn.handshake(Role::App, Purpose::Pair, STEP)?;
    match conn.recv_within::<HostMsg>(STEP)? {
        HostMsg::Pair { secret } if secret.len() == SECRET_LEN => {
            let mut s = [0u8; SECRET_LEN];
            s.copy_from_slice(&secret);
            store_secret(&shared.config.pairing_file, &s)?;
            logging::info!(target: "forge-devtools", "paired with a computer");
            conn.send(&AppMsg::Paired)
        }
        _ => Ok(()),
    }
}

fn session(shared: &Shared, mut conn: Conn<TcpStream>) -> Result<(), ConnError> {
    let (ack, tail) = match conn.recv_within::<HostMsg>(STEP)? {
        HostMsg::Attach { ack, tail } => (ack, tail),
        _ => return Ok(()),
    };
    let (gap, backlog) = {
        let outbox = lock(&shared.outbox);
        match tail {
            Some(n) if ack == 0 => (0, outbox.tail(n as usize)),
            _ => outbox.after(ack),
        }
    };
    let next_seq = lock(&shared.outbox).next_seq();
    conn.send(&AppMsg::Hello(Hello {
        app: shared.config.app.clone(),
        build: shared.config.build.clone(),
        phase: format!("{:?}", shared.lifecycle.phase()),
        started_ms: shared.started_ms,
        next_seq,
    }))?;
    if gap > 0 {
        conn.send(&AppMsg::Gap { missed: gap })?;
    }
    let mut sent = if backlog.is_empty() {
        // Nothing to replay: continue from here, not from `ack`.
        next_seq - 1
    } else {
        backlog.last().map_or(0, |(seq, _)| *seq)
    };
    for (seq, record) in backlog {
        conn.send(&AppMsg::Record { seq, record })?;
    }
    let mut last_out = Instant::now();
    let mut last_in = Instant::now();
    loop {
        if shared.stopping() {
            return conn.send(&AppMsg::Bye {
                reason: ByeReason::Stopped,
            });
        }
        let phase = shared.note_phase();
        // New records first, so the phase change that ends the session is
        // the last thing the host sees.
        let (gap, fresh) = lock(&shared.outbox).after(sent);
        if gap > 0 {
            conn.send(&AppMsg::Gap { missed: gap })?;
        }
        for (seq, record) in fresh {
            sent = seq;
            conn.send(&AppMsg::Record { seq, record })?;
            last_out = Instant::now();
        }
        if !phase.is_foreground() {
            return conn.send(&AppMsg::Bye {
                reason: ByeReason::Background,
            });
        }
        match conn.recv::<HostMsg>()? {
            Some(HostMsg::Command { id, command }) => {
                last_in = Instant::now();
                let unpair = command == Command::Unpair;
                let result = shared.run(command);
                let done = unpair && result.is_ok();
                conn.send(&AppMsg::Reply { id, result })?;
                last_out = Instant::now();
                if done {
                    return conn.send(&AppMsg::Bye {
                        reason: ByeReason::Unpaired,
                    });
                }
            }
            Some(_) => last_in = Instant::now(),
            None => {}
        }
        if last_in.elapsed() >= SILENCE {
            return Ok(());
        }
        if last_out.elapsed() >= HEARTBEAT {
            conn.send(&AppMsg::Heartbeat)?;
            last_out = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests;
