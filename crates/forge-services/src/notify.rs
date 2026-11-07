//! Checks for local notifications, done in Rust so every platform rejects
//! the same requests with the same reasons.

use std::time::{SystemTime, UNIX_EPOCH};

/// Longest notification id accepted.
pub const MAX_ID_LEN: usize = 256;

/// Shortest repeating interval iOS accepts.
pub const MIN_REPEAT_SECONDS: u32 = 60;

/// The trigger of a notification, without the FFI types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Now,
    After { seconds: u32, repeats: bool },
    At { unix_ms: i64 },
}

/// Checks a notification before it reaches the platform. `now_ms` is the
/// current Unix time in milliseconds.
pub fn validate(
    id: &str,
    title: &str,
    body: &str,
    deep_link: Option<&str>,
    trigger: Trigger,
    now_ms: i64,
) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("a notification needs an id".into());
    }
    if id.len() > MAX_ID_LEN {
        return Err(format!("a notification id is at most {MAX_ID_LEN} bytes"));
    }
    if title.trim().is_empty() && body.trim().is_empty() {
        return Err("a notification needs a title or a body".into());
    }
    if let Some(link) = deep_link {
        crate::link::parse(link).map_err(|reason| format!("deep_link: {reason}"))?;
    }
    match trigger {
        Trigger::Now => {}
        Trigger::After { seconds: 0, .. } => {
            return Err("an After trigger needs at least 1 second (use Now)".into());
        }
        Trigger::After {
            seconds,
            repeats: true,
        } if seconds < MIN_REPEAT_SECONDS => {
            return Err(format!(
                "a repeating notification needs at least {MIN_REPEAT_SECONDS} seconds"
            ));
        }
        Trigger::After { .. } => {}
        Trigger::At { unix_ms } if unix_ms <= now_ms => {
            return Err("an At trigger must be in the future".into());
        }
        Trigger::At { .. } => {}
    }
    Ok(())
}

/// The current Unix time in milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(trigger: Trigger) -> Result<(), String> {
        validate("id", "Title", "", None, trigger, 1_000)
    }

    #[test]
    fn accepts_ordinary_notifications() {
        assert_eq!(check(Trigger::Now), Ok(()));
        assert_eq!(
            check(Trigger::After {
                seconds: 5,
                repeats: false
            }),
            Ok(())
        );
        assert_eq!(
            check(Trigger::After {
                seconds: 60,
                repeats: true
            }),
            Ok(())
        );
        assert_eq!(check(Trigger::At { unix_ms: 1_001 }), Ok(()));
        assert_eq!(
            validate("id", "", "body only", Some("myapp://x"), Trigger::Now, 0),
            Ok(())
        );
    }

    #[test]
    fn rejects_what_ios_would_reject_or_drop() {
        assert!(validate(" ", "T", "", None, Trigger::Now, 0).is_err());
        assert!(validate(&"x".repeat(MAX_ID_LEN + 1), "T", "", None, Trigger::Now, 0).is_err());
        assert!(validate("id", " ", "", None, Trigger::Now, 0).is_err());
        assert!(check(Trigger::After {
            seconds: 0,
            repeats: false
        })
        .is_err());
        let err = check(Trigger::After {
            seconds: 59,
            repeats: true,
        })
        .unwrap_err();
        assert!(err.contains("60"), "{err}");
        assert!(check(Trigger::At { unix_ms: 1_000 }).is_err());
        let err = validate("id", "T", "", Some("no scheme"), Trigger::Now, 0).unwrap_err();
        assert!(err.starts_with("deep_link:"), "{err}");
    }
}
