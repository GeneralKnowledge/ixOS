//! One logging pipeline for every build of a Forge app.
//!
//! App code logs with the standard [`log`] macros, re-exported here
//! (`forge_runtime::logging::info!(...)`), so dependencies that already use
//! `log` (HTTP, TLS) land in the same place. [`init`] installs the pipeline
//! once per process:
//!
//! - **Platform log.** Android: logcat (tag = the record's target). iOS:
//!   the system log via `syslog(3)`, as `LEVEL [target] message` at notice
//!   priority or above, so `forge device logs` (the USB syslog relay) shows
//!   every level. Elsewhere (host tests, Linux tools): standard error, only
//!   when `FORGE_LOG` is set.
//! - **Recent records.** The last [`RECENT_CAPACITY`] records stay in
//!   memory ([`recent`]) for diagnostics: an error report, a support
//!   bundle, a developer tool.
//! - **Extra sinks.** [`add_sink`] lets a component (a developer tool, a
//!   test) receive every record as it is written. Production builds add
//!   none.
//! - **Panics.** A panic is logged (message and location) before the
//!   previous panic hook runs, so it reaches the platform log even when the
//!   host only sees a UniFFI error.
//!
//! Records are text written by app code. Never log secrets: tokens,
//! passwords, Keychain contents, message bodies.
//!
//! The level is Info in release builds and Debug in debug builds;
//! [`set_level`] changes it, and `FORGE_LOG=trace|debug|info|warn|error`
//! sets it at [`init`] on hosts that pass environment variables.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub use log::{debug, error, info, trace, warn, Level, LevelFilter};

use crate::shell::{lock, now_ms};

/// How many records [`recent`] keeps.
pub const RECENT_CAPACITY: usize = 512;

/// One log record, as sinks and [`recent`] see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    /// Unix time in milliseconds.
    pub time_ms: i64,
    pub level: Level,
    /// Where it came from: the module path, or the `target:` given to the macro.
    pub target: String,
    pub message: String,
}

/// Receives every record that passes the level filter. Called on the
/// logging thread, so keep it quick and never log from inside it.
pub trait LogSink: Send + Sync {
    fn write(&self, record: &LogRecord);
}

/// Identifies a sink added with [`add_sink`], to remove it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SinkId(u64);

struct Pipeline {
    recent: Mutex<VecDeque<LogRecord>>,
    sinks: RwLock<Vec<(SinkId, Arc<dyn LogSink>)>>,
    next_sink: AtomicU64,
    console: bool,
}

fn pipeline() -> &'static Pipeline {
    static PIPELINE: OnceLock<Pipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| Pipeline {
        recent: Mutex::new(VecDeque::with_capacity(RECENT_CAPACITY)),
        sinks: RwLock::new(Vec::new()),
        next_sink: AtomicU64::new(1),
        console: cfg!(any(target_os = "ios", target_os = "android"))
            || std::env::var_os("FORGE_LOG").is_some(),
    })
}

struct ForgeLogger;

impl log::Log for ForgeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let record = LogRecord {
            time_ms: now_ms(),
            level: record.level(),
            target: record.target().to_string(),
            message: record.args().to_string(),
        };
        let pipeline = pipeline();
        if pipeline.console {
            platform::write(&record);
        }
        {
            let mut recent = lock(&pipeline.recent);
            if recent.len() == RECENT_CAPACITY {
                recent.pop_front();
            }
            recent.push_back(record.clone());
        }
        let sinks = pipeline
            .sinks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, sink) in sinks.iter() {
            sink.write(&record);
        }
    }

    fn flush(&self) {}
}

/// Installs the pipeline and the panic hook. Idempotent and cheap after the
/// first call, so every app object's constructor can call it. If another
/// `log` logger is already installed (a host tool's own), records go there
/// and this does nothing.
pub fn init() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        if log::set_logger(&ForgeLogger).is_err() {
            return;
        }
        log::set_max_level(initial_level());
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(non-string panic payload)".into());
            log::error!(target: "panic", "{payload}{location}");
            previous(info);
        }));
    });
}

fn initial_level() -> LevelFilter {
    let default = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    std::env::var("FORGE_LOG")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Sets the most detailed level that is recorded.
pub fn set_level(level: LevelFilter) {
    log::set_max_level(level);
}

/// The most recent records, oldest first, at most `limit`.
pub fn recent(limit: usize) -> Vec<LogRecord> {
    let recent = lock(&pipeline().recent);
    let skip = recent.len().saturating_sub(limit);
    recent.iter().skip(skip).cloned().collect()
}

/// Sends every later record to `sink` as well, until [`remove_sink`].
pub fn add_sink(sink: Arc<dyn LogSink>) -> SinkId {
    let pipeline = pipeline();
    let id = SinkId(pipeline.next_sink.fetch_add(1, Ordering::Relaxed));
    pipeline
        .sinks
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((id, sink));
    id
}

/// Stops sending records to a sink. Unknown ids are ignored.
pub fn remove_sink(id: SinkId) {
    pipeline()
        .sinks
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|(sink, _)| *sink != id);
}

#[cfg(target_os = "android")]
mod platform {
    use super::LogRecord;
    use log::Level;
    use std::ffi::{c_char, c_int, CString};

    #[link(name = "log")]
    extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    pub(super) fn write(record: &LogRecord) {
        // android/log.h priorities.
        let prio = match record.level {
            Level::Trace => 2,
            Level::Debug => 3,
            Level::Info => 4,
            Level::Warn => 5,
            Level::Error => 6,
        };
        let tag = CString::new(record.target.replace('\0', "")).unwrap_or_default();
        let text = CString::new(record.message.replace('\0', "")).unwrap_or_default();
        // SAFETY: both pointers are valid NUL-terminated strings for the call.
        unsafe {
            __android_log_write(prio, tag.as_ptr(), text.as_ptr());
        }
    }
}

#[cfg(target_os = "ios")]
mod platform {
    use super::LogRecord;
    use log::Level;
    use std::ffi::{c_char, c_int, CString};

    extern "C" {
        fn syslog(priority: c_int, format: *const c_char, ...);
    }

    pub(super) fn write(record: &LogRecord) {
        // sys/syslog.h priorities. The USB syslog relay (what
        // `forge device logs` and idevicesyslog read) drops LOG_INFO and
        // LOG_DEBUG, so everything below Warn goes out as LOG_NOTICE and
        // the level is kept in the text instead.
        let priority = match record.level {
            Level::Trace | Level::Debug | Level::Info => 5,
            Level::Warn => 4,
            Level::Error => 3,
        };
        let line =
            format!("{} [{}] {}", record.level, record.target, record.message).replace('\0', "");
        let line = CString::new(line).unwrap_or_default();
        // SAFETY: a constant "%s" format with one valid C string argument.
        unsafe {
            syslog(priority, c"%s".as_ptr(), line.as_ptr());
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod platform {
    use super::LogRecord;

    pub(super) fn write(record: &LogRecord) {
        eprintln!("{} [{}] {}", record.level, record.target, record.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Collect(Mutex<Vec<LogRecord>>);

    impl LogSink for Collect {
        fn write(&self, record: &LogRecord) {
            lock(&self.0).push(record.clone());
        }
    }

    // One test: the logger, level and sinks are process-wide.
    #[test]
    fn records_reach_sinks_and_recent_until_removed() {
        init();
        init(); // idempotent
        set_level(LevelFilter::Debug);
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        let id = add_sink(sink.clone());

        info!(target: "test-sink", "hello {}", 42);
        trace!(target: "test-sink", "below the level");
        let seen = lock(&sink.0).clone();
        let mine: Vec<_> = seen.iter().filter(|r| r.target == "test-sink").collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].message, "hello 42");
        assert_eq!(mine[0].level, Level::Info);
        assert!(recent(RECENT_CAPACITY)
            .iter()
            .any(|r| r.target == "test-sink" && r.message == "hello 42"));

        remove_sink(id);
        warn!(target: "test-sink", "after removal");
        assert!(!lock(&sink.0).iter().any(|r| r.message == "after removal"));

        let caught = std::panic::catch_unwind(|| panic!("boom in test"));
        assert!(caught.is_err());
        assert!(recent(RECENT_CAPACITY)
            .iter()
            .any(|r| r.target == "panic" && r.message.starts_with("boom in test at ")));
    }
}
