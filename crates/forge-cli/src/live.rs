//! `forge device pair | unpair | status | snapshot | cmd | connect`: the live
//! connection to a development build (Forge Device M1). The protocol and the
//! phone side are in `forge-devproto`, `forge-devtools` and `forge-device`;
//! this file picks the phone and formats.

use std::io::Write;
use std::time::Duration;

use forge_device::live::{self, Session};
use forge_device::{pick_device, DeviceBackend, DeviceError, DeviceRef, PairingStore};
use forge_devproto::client::{Bye, ClientError, Event, From};
use forge_devproto::{ByeReason, Command, Record, Reply};

/// Which phone and app to talk to, and where pairings are kept.
pub struct Target<'a> {
    pub backend: &'a dyn DeviceBackend,
    pub udid: Option<&'a str>,
    pub port: u16,
    pub store: &'a PairingStore,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Target<'_> {
    fn device(&self) -> Result<DeviceRef, String> {
        pick_device(self.backend, self.udid).map_err(err)
    }

    fn open(&self, device: &DeviceRef, from: From) -> Result<Session, DeviceError> {
        live::open(self.backend, device, self.port, self.store, from)
    }

    fn command(&self, command: Command) -> Result<Reply, String> {
        let device = self.device()?;
        let mut session = self.open(&device, From::Tail(0)).map_err(err)?;
        session.command(command).map_err(err)
    }
}

/// `forge device pair`
pub fn pair(t: &Target, out: &mut dyn Write) -> Result<(), String> {
    let device = t.device()?;
    let (app, path) = live::pair(t.backend, &device, t.port, t.store).map_err(err)?;
    writeln!(
        out,
        "Paired with {app} on {}. The key is in {}.",
        device.udid,
        path.display()
    )
    .map_err(err)
}

/// `forge device unpair`: forgets the pairing on both ends.
pub fn unpair(t: &Target, out: &mut dyn Write) -> Result<(), String> {
    let device = t.device()?;
    let mut session = t.open(&device, From::Tail(0)).map_err(err)?;
    let app = session.preamble.app.clone();
    match session.command(Command::Unpair) {
        Ok(_) | Err(ClientError::Ended(Bye(ByeReason::Unpaired))) => {}
        Err(e) => return Err(err(e)),
    }
    t.store.remove(&device.udid, &app).map_err(err)?;
    writeln!(out, "Unpaired {app} on {}.", device.udid).map_err(err)
}

/// `forge device status`
pub fn status(t: &Target, out: &mut dyn Write) -> Result<(), String> {
    let Reply::Status(s) = t.command(Command::Status)? else {
        return Err("the app sent something other than its status".into());
    };
    let list = |items: &[String]| {
        if items.is_empty() {
            "none".to_string()
        } else {
            items.join(", ")
        }
    };
    let secs = s.uptime_ms / 1000;
    writeln!(
        out,
        "App:      {} (build {})\nPhase:    {}\nRunning:  {}m {}s\nRecords:  {} buffered (#{} to #{})\nProbes:   {}\nCommands: {}",
        s.app,
        s.build,
        s.phase,
        secs / 60,
        secs % 60,
        s.buffered,
        s.oldest_seq,
        s.next_seq.saturating_sub(1),
        list(&s.probes),
        list(&s.commands),
    )
    .map_err(err)
}

/// `forge device snapshot`: every state probe the app registered.
pub fn snapshot(t: &Target, out: &mut dyn Write) -> Result<(), String> {
    let Reply::Snapshot(probes) = t.command(Command::Snapshot)? else {
        return Err("the app sent something other than a snapshot".into());
    };
    if probes.is_empty() {
        return writeln!(out, "The app registers no probes.").map_err(err);
    }
    let width = probes.iter().map(|p| p.name.len()).max().unwrap_or(0);
    for p in probes {
        writeln!(out, "{:width$}  {}", p.name, p.value).map_err(err)?;
    }
    Ok(())
}

/// `forge device cmd [name [args]]`: runs one of the app's commands, or
/// lists them.
pub fn cmd(
    t: &Target,
    name: Option<&str>,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), String> {
    let Some(name) = name else {
        let Reply::Commands(commands) = t.command(Command::ListCommands)? else {
            return Err("the app sent something other than its commands".into());
        };
        if commands.is_empty() {
            return writeln!(out, "The app registers no commands.").map_err(err);
        }
        let width = commands.iter().map(|c| c.name.len()).max().unwrap_or(0);
        for c in commands {
            writeln!(out, "{:width$}  {}", c.name, c.help).map_err(err)?;
        }
        return Ok(());
    };
    match t.command(Command::App {
        name: name.into(),
        args: args.to_vec(),
    })? {
        Reply::Done(text) if text.is_empty() => Ok(()),
        Reply::Done(text) => writeln!(out, "{text}").map_err(err),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

/// A record as one line, timed from when the app started.
fn line(started_ms: i64, record: &Record) -> String {
    let at = |ms: i64| {
        let ms = (ms - started_ms).max(0);
        format!("+{:>4}.{:03}s", ms / 1000, ms % 1000)
    };
    match record {
        Record::Log(l) => format!(
            "{} {:<5} [{}] {}",
            at(l.time_ms),
            l.level,
            l.target,
            l.message
        ),
        Record::Phase { phase } => format!("--- phase: {phase}"),
    }
}

/// Whether a failed reconnect is worth retrying (the app is not back yet,
/// the cable was pulled) rather than a problem the user must fix.
fn retryable(e: &DeviceError) -> bool {
    !matches!(
        e,
        DeviceError::Live(ClientError::NotPaired { .. } | ClientError::KeyRejected { .. })
            | DeviceError::Pairings(_)
            | DeviceError::Usbmuxd { .. }
    )
}

/// `forge device connect`: the app's log and lifecycle, live. With
/// `follow`, waits through the app going to the background and through
/// disconnects, and carries on where it left off. Stops after `limit`
/// records, or on Ctrl-C.
pub fn connect(
    t: &Target,
    tail: u32,
    limit: Option<usize>,
    follow: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let device = t.device()?;
    let mut from = From::Tail(tail);
    let mut shown = 0usize;
    let mut waiting = false;
    loop {
        let mut session = match t.open(&device, from) {
            Ok(session) => session,
            Err(e) if waiting && retryable(&e) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(e) => return Err(err(e)),
        };
        let hello = session.hello.clone();
        if waiting {
            writeln!(out, "[back: {} is {}]", hello.app, hello.phase).map_err(err)?;
        } else {
            writeln!(
                out,
                "Connected to {} (build {}), {}. Ctrl-C stops.",
                hello.app, hello.build, hello.phase
            )
            .map_err(err)?;
        }
        let ended = loop {
            match session.next_event() {
                Ok(Some(Event::Record { record, .. })) => {
                    writeln!(out, "{}", line(hello.started_ms, &record)).map_err(err)?;
                    shown += 1;
                    if limit.is_some_and(|l| shown >= l) {
                        return Ok(());
                    }
                }
                Ok(Some(Event::Gap { missed })) => {
                    writeln!(
                        out,
                        "[{missed} records were dropped on the phone before they could be sent]"
                    )
                    .map_err(err)?;
                }
                Ok(None) => {}
                Err(e) => break e,
            }
        };
        let last = session.last_seq();
        match ended {
            ClientError::Ended(Bye(ByeReason::Background)) => {
                writeln!(
                    out,
                    "[suspended: the app is in the background{}]",
                    if follow {
                        "; waiting for it to come back"
                    } else {
                        ""
                    }
                )
                .map_err(err)?;
            }
            other => {
                if !follow {
                    return Err(err(other));
                }
                writeln!(out, "[disconnected: {other}; reconnecting]").map_err(err)?;
            }
        }
        if !follow {
            return Ok(());
        }
        out.flush().map_err(err)?;
        from = From::After(last);
        waiting = true;
    }
}

#[cfg(test)]
mod tests;
