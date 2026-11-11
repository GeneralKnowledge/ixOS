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

/// Most actions iOS shows on one notification.
pub const MAX_ACTIONS: usize = 10;

/// Checks a set of notification categories: ids present and unique, each
/// category with at least one action and at most [`MAX_ACTIONS`], action
/// titles present and action ids unique within a category. Takes
/// `(category id, [(action id, title)])`.
pub fn validate_categories(categories: &[(&str, Vec<(&str, &str)>)]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for (id, actions) in categories {
        if id.trim().is_empty() {
            return Err("a notification category needs an id".into());
        }
        if !seen.insert(*id) {
            return Err(format!("notification category \"{id}\" is listed twice"));
        }
        if actions.is_empty() || actions.len() > MAX_ACTIONS {
            return Err(format!(
                "notification category \"{id}\" needs 1 to {MAX_ACTIONS} actions"
            ));
        }
        let mut action_ids = std::collections::HashSet::new();
        for (action, title) in actions {
            if action.trim().is_empty() || title.trim().is_empty() {
                return Err(format!(
                    "every action in notification category \"{id}\" needs an id and a title"
                ));
            }
            if !action_ids.insert(*action) {
                return Err(format!(
                    "action \"{action}\" is listed twice in notification category \"{id}\""
                ));
            }
        }
    }
    Ok(())
}

/// Where notification responses wait for the app's handler: a response
/// that arrives before the core registered one (a cold launch from an
/// action) is kept, up to [`Inbox::MAX_PENDING`], and delivered to the
/// first handler.
pub struct Inbox<T> {
    state: std::sync::Mutex<InboxState<T>>,
}

type Handler<T> = std::sync::Arc<dyn Fn(T) + Send + Sync>;

struct InboxState<T> {
    handler: Option<Handler<T>>,
    pending: Vec<T>,
}

impl<T> Default for Inbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Inbox<T> {
    /// Kept while no handler is registered; the oldest are dropped first.
    pub const MAX_PENDING: usize = 16;

    pub const fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(InboxState {
                handler: None,
                pending: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, InboxState<T>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Hands `item` to the handler, or keeps it until there is one.
    pub fn deliver(&self, item: T) {
        let handler = {
            let mut state = self.lock();
            match &state.handler {
                Some(handler) => handler.clone(),
                None => {
                    state.pending.push(item);
                    let extra = state.pending.len().saturating_sub(Self::MAX_PENDING);
                    state.pending.drain(..extra);
                    return;
                }
            }
        };
        handler(item);
    }

    /// Sets the handler (replacing any earlier one) and gives it whatever
    /// arrived before.
    pub fn set_handler(&self, handler: impl Fn(T) + Send + Sync + 'static) {
        let handler: Handler<T> = std::sync::Arc::new(handler);
        let pending = {
            let mut state = self.lock();
            state.handler = Some(handler.clone());
            std::mem::take(&mut state.pending)
        };
        for item in pending {
            handler(item);
        }
    }

    /// Removes the handler (for tests).
    pub fn clear(&self) {
        let mut state = self.lock();
        state.handler = None;
        state.pending.clear();
    }
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

    #[test]
    fn categories_need_ids_titles_and_unique_actions() {
        assert!(validate_categories(&[(
            "message",
            vec![("reply", "Reply"), ("read", "Mark as Read")]
        )])
        .is_ok());
        assert!(validate_categories(&[("", vec![("a", "A")])]).is_err());
        assert!(validate_categories(&[("c", vec![])]).is_err());
        assert!(validate_categories(&[("c", vec![("a", " ")])]).is_err());
        assert!(validate_categories(&[("c", vec![("a", "A"), ("a", "B")])]).is_err());
        assert!(validate_categories(&[("c", vec![("a", "A")]), ("c", vec![("b", "B")])]).is_err());
    }

    #[test]
    fn the_inbox_keeps_items_until_there_is_a_handler() {
        let inbox = Inbox::new();
        for i in 0..20 {
            inbox.deliver(i);
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        inbox.set_handler(move |i| sink.lock().unwrap().push(i));
        // The oldest four were dropped.
        assert_eq!(*seen.lock().unwrap(), (4..20).collect::<Vec<_>>());
        inbox.deliver(99);
        assert_eq!(seen.lock().unwrap().last(), Some(&99));
    }

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
