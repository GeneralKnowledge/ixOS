//! The Logs screen (Tools tab): the records Forge's logging pipeline keeps in
//! memory ([`forge_runtime::logging::recent`]). Every hosted app links into
//! this one library, so its Rust and its Swift `ForgeLog` calls land in the
//! same pipeline, and one screen shows them all without a cable.

use forge_runtime::logging::{self, Level};

/// A record's level, for the screen's filter and colour.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::Error => Self::Error,
            Level::Warn => Self::Warn,
            Level::Info => Self::Info,
            Level::Debug => Self::Debug,
            Level::Trace => Self::Trace,
        }
    }
}

/// One record, as the Logs screen shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// Unix time in milliseconds.
    pub time_ms: i64,
    pub level: LogLevel,
    /// Where it came from: a module path, or the target the app named
    /// (`relay::transfer`, `crash-test`).
    pub target: String,
    pub message: String,
}

/// The newest `limit` records, oldest first.
#[uniffi::export]
pub fn recent_logs(limit: u32) -> Vec<LogLine> {
    logging::recent(limit as usize)
        .into_iter()
        .map(|record| LogLine {
            time_ms: record.time_ms,
            level: record.level.into(),
            target: record.target,
            message: record.message,
        })
        .collect()
}

/// The records as plain text, one per line, for sharing:
/// `2026-11-17 15:02:11.042 WARN [soundscape::player] message`.
#[uniffi::export]
pub fn logs_as_text(lines: Vec<LogLine>) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&format!(
            "{} {} [{}] {}\n",
            format_time(line.time_ms),
            level_name(line.level),
            line.target,
            line.message
        ));
    }
    out
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN",
        LogLevel::Info => "INFO",
        LogLevel::Debug => "DEBUG",
        LogLevel::Trace => "TRACE",
    }
}

/// UTC date and time with milliseconds, without a date crate.
fn format_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_logs_include_what_was_just_logged() {
        logging::init();
        logging::warn!(target: "logs-test", "a record for the Logs screen");
        let lines = recent_logs(512);
        let line = lines
            .iter()
            .rev()
            .find(|l| l.target == "logs-test")
            .expect("the record is kept");
        assert_eq!(line.level, LogLevel::Warn);
        assert_eq!(line.message, "a record for the Logs screen");
    }

    #[test]
    fn text_has_one_line_per_record_with_utc_time() {
        let text = logs_as_text(vec![LogLine {
            time_ms: 1_763_391_731_042,
            level: LogLevel::Warn,
            target: "soundscape::player".into(),
            message: "interrupted".into(),
        }]);
        assert_eq!(
            text,
            "2025-11-17 15:02:11.042 WARN [soundscape::player] interrupted\n"
        );
    }
}
