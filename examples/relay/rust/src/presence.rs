//! Where a trusted device is now, as the person sees it (brief PR 5,
//! examples/relay/docs/relay-m1-m2-brief.md, "Device discovery and
//! status"). Pure rules, no I/O.
//!
//! - **Nearby:** Bonjour sees it on this network now.
//! - **Reachable:** not seen on this network, but a trusted connection with
//!   it worked recently, either way (it reached this device, or this one
//!   reached it). Until the connection resolver and our relay (PRs 6 and
//!   7) can find a device away from home, that is the only evidence of it
//!   away from Bonjour.
//! - **Offline:** neither. "Last seen" says when it was last reached.
//!
//! Presence is a statement about the past, never a promise: a device shown
//! as reachable may have gone a second ago. Senders still try, and the
//! outbox still waits.

/// How long a worked connection keeps a device "reachable".
pub const REACHABLE_FOR_MS: i64 = 5 * 60 * 1000;

/// Where a trusted device is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Presence {
    Nearby,
    Reachable,
    Offline,
}

/// The presence of a device that Bonjour does or doesn't see (`nearby`)
/// and that was last reached at `last_reached_ms`, at time `now_ms`.
pub fn presence(nearby: bool, last_reached_ms: Option<i64>, now_ms: i64) -> Presence {
    if nearby {
        return Presence::Nearby;
    }
    match last_reached_ms {
        // A clock that went backwards still counts as recent.
        Some(t) if now_ms - t <= REACHABLE_FOR_MS => Presence::Reachable,
        _ => Presence::Offline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_wins_then_a_recent_connection_then_offline() {
        let now = 10 * REACHABLE_FOR_MS;
        assert_eq!(presence(true, None, now), Presence::Nearby);
        assert_eq!(presence(true, Some(0), now), Presence::Nearby);
        assert_eq!(presence(false, Some(now - 1000), now), Presence::Reachable);
        assert_eq!(
            presence(false, Some(now - REACHABLE_FOR_MS), now),
            Presence::Reachable
        );
        assert_eq!(
            presence(false, Some(now - REACHABLE_FOR_MS - 1), now),
            Presence::Offline
        );
        assert_eq!(presence(false, None, now), Presence::Offline);
        assert_eq!(presence(false, Some(now + 5000), now), Presence::Reachable);
    }
}
