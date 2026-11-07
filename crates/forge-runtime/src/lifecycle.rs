//! The app lifecycle as Rust sees it.
//!
//! The host reports what the OS tells it (SwiftUI `scenePhase` and
//! `willTerminate` on iOS, the activity's `onStart`/`onResume`/`onPause`/
//! `onStop` on Android) by calling one exported method on the app's core,
//! which forwards it to [`Lifecycle::enter`]. Rust then owns what happens:
//! which work stops when the app leaves the screen, what to refresh on
//! resume, and when to shut down.
//!
//! [`Lifecycle`] gives that code three things:
//!
//! - the [`Transition`] each call caused (or `None` for a repeat), with
//!   `resumed` set when the app comes back from the background;
//! - two cancellation scopes: [`Lifecycle::lifetime`], cancelled when the app
//!   terminates, and [`Lifecycle::foreground`], cancelled when it goes to the
//!   background and renewed when it comes back. Run work that only makes
//!   sense on screen (a long poll, a live stream, a user-initiated request)
//!   under a child of the foreground scope;
//! - [`Lifecycle::subscribe`], so code on other threads can follow changes.
//!
//! It holds no platform code and calls nothing in Swift or Kotlin.

use std::sync::{Arc, Mutex, MutexGuard};

use forge_net::{AbortRegistration, CancelHandle};

use crate::events::{EventHub, Subscription};

/// Where the app is in its lifecycle. The host maps the OS's states onto
/// these; see `docs/app-crate.md#lifecycle` for the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Created, not yet on screen. The initial phase.
    Launching,
    /// On screen and receiving input.
    Active,
    /// On screen but not receiving input: a system alert, the app switcher,
    /// a phone call banner (iOS), or a dialog over the activity (Android).
    Inactive,
    /// Not visible. iOS may suspend the process at any moment from here, and
    /// either OS may kill it without another call.
    Background,
    /// The app is going away. Final: later calls are ignored. iOS reports
    /// this only sometimes, so never rely on it to save data.
    Terminated,
}

impl Phase {
    /// `Launching`, `Active` or `Inactive`.
    pub fn is_foreground(self) -> bool {
        matches!(self, Phase::Launching | Phase::Active | Phase::Inactive)
    }
}

/// One change of phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub from: Phase,
    pub to: Phase,
    /// The app came back from `Background`. Refresh anything that may have
    /// changed while it was away, and restart work the background stopped.
    pub resumed: bool,
}

/// Tracks the phase and owns the lifetime and foreground cancellation
/// scopes. Cheap to clone; clones share state. Thread-safe.
#[derive(Clone)]
pub struct Lifecycle {
    inner: Arc<Inner>,
}

struct Inner {
    lifetime: CancelHandle,
    state: Mutex<State>,
    changes: EventHub<Transition>,
}

struct State {
    phase: Phase,
    foreground: CancelHandle,
    /// Links `foreground` under `lifetime`; dropped with the scope.
    _link: AbortRegistration,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lifecycle")
            .field("phase", &self.phase())
            .finish()
    }
}

impl Lifecycle {
    /// Starts in [`Phase::Launching`] with a live foreground scope.
    pub fn new() -> Self {
        let lifetime = CancelHandle::new();
        let (foreground, link) = lifetime.child();
        Self {
            inner: Arc::new(Inner {
                lifetime,
                state: Mutex::new(State {
                    phase: Phase::Launching,
                    foreground,
                    _link: link,
                }),
                changes: EventHub::new(),
            }),
        }
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.lock().phase
    }

    /// Moves to `phase` and returns what changed, or `None` when the app is
    /// already in `phase` or has terminated (hosts may report the same phase
    /// twice, so repeats are expected and harmless).
    ///
    /// Entering `Background` cancels the foreground scope; leaving it for
    /// `Active` or `Inactive` starts a new one. Entering `Terminated`
    /// cancels the lifetime (and so every scope under it) and ends every
    /// subscription after delivering this last transition.
    ///
    /// Runs on the caller's thread, which is the UI thread when a host calls
    /// it from a lifecycle callback, so it does no I/O. Cancel hooks
    /// registered on the scopes run here too and must be quick.
    pub fn enter(&self, phase: Phase) -> Option<Transition> {
        let (transition, stopped) = {
            let mut state = self.lock();
            let from = state.phase;
            if from == phase || from == Phase::Terminated {
                return None;
            }
            state.phase = phase;
            let mut stopped = None;
            if phase == Phase::Background {
                stopped = Some(state.foreground.clone());
            } else if from == Phase::Background && phase.is_foreground() {
                let (foreground, link) = self.inner.lifetime.child();
                state.foreground = foreground;
                state._link = link;
            }
            let transition = Transition {
                from,
                to: phase,
                resumed: from == Phase::Background && phase.is_foreground(),
            };
            (transition, stopped)
        };
        // Cancel outside the lock: hooks may call back into `Lifecycle`.
        if let Some(scope) = stopped {
            scope.cancel();
        }
        self.inner.changes.publish(transition);
        if phase == Phase::Terminated {
            self.inner.lifetime.cancel();
            self.inner.changes.close();
        }
        Some(transition)
    }

    /// Cancelled when the app terminates (or the owner cancels it, for
    /// example from its own `shutdown`). Every foreground scope is under it.
    pub fn lifetime(&self) -> CancelHandle {
        self.inner.lifetime.clone()
    }

    /// The current foreground scope: cancelled when the app next enters
    /// `Background` or terminates. While the app is in the background this
    /// is the scope that was cancelled, so work started under it then stops
    /// at once. Take a [`CancelHandle::child`] of it per piece of work.
    pub fn foreground(&self) -> CancelHandle {
        self.lock().foreground.clone()
    }

    /// Follows every transition from now on, or `None` after `Terminated`.
    pub fn subscribe(&self) -> Option<Subscription<Transition>> {
        self.inner.changes.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{block_on, Delivery};

    #[test]
    fn transitions_report_resume_and_ignore_repeats() {
        let lifecycle = Lifecycle::new();
        assert_eq!(lifecycle.phase(), Phase::Launching);
        let first = lifecycle.enter(Phase::Active).unwrap();
        assert_eq!(
            first,
            Transition {
                from: Phase::Launching,
                to: Phase::Active,
                resumed: false
            }
        );
        assert_eq!(lifecycle.enter(Phase::Active), None);
        assert!(!lifecycle.enter(Phase::Inactive).unwrap().resumed);
        assert!(!lifecycle.enter(Phase::Background).unwrap().resumed);
        // iOS comes back through Inactive: that step is the resume.
        assert!(lifecycle.enter(Phase::Inactive).unwrap().resumed);
        assert!(!lifecycle.enter(Phase::Active).unwrap().resumed);
    }

    #[test]
    fn background_cancels_the_foreground_scope_and_resume_renews_it() {
        let lifecycle = Lifecycle::new();
        lifecycle.enter(Phase::Active);
        let before = lifecycle.foreground();
        let (request, _link) = before.child();

        lifecycle.enter(Phase::Background);
        assert!(before.is_cancelled());
        assert!(request.is_cancelled());
        // Work started while in the background stops at once.
        assert!(lifecycle.foreground().is_cancelled());
        assert!(!lifecycle.lifetime().is_cancelled());

        lifecycle.enter(Phase::Active);
        let after = lifecycle.foreground();
        assert!(!after.is_cancelled());
        lifecycle.lifetime().cancel();
        assert!(after.is_cancelled());
    }

    #[test]
    fn a_launch_straight_into_the_background_stops_foreground_work() {
        let lifecycle = Lifecycle::new();
        let launch = lifecycle.foreground();
        let transition = lifecycle.enter(Phase::Background).unwrap();
        assert!(!transition.resumed);
        assert!(launch.is_cancelled());
        assert!(lifecycle.enter(Phase::Active).unwrap().resumed);
    }

    #[test]
    fn terminate_is_final_and_ends_subscriptions() {
        let lifecycle = Lifecycle::new();
        let changes = lifecycle.subscribe().unwrap();
        lifecycle.enter(Phase::Active);
        let foreground = lifecycle.foreground();
        let last = lifecycle.enter(Phase::Terminated).unwrap();
        assert_eq!(last.from, Phase::Active);
        assert!(lifecycle.lifetime().is_cancelled());
        assert!(foreground.is_cancelled());
        assert_eq!(lifecycle.enter(Phase::Active), None);
        assert_eq!(lifecycle.phase(), Phase::Terminated);
        assert!(lifecycle.subscribe().is_none());

        let seen: Vec<_> = std::iter::from_fn(|| block_on(changes.next()))
            .map(|delivery| match delivery {
                Delivery::Event(t) => t.to,
                Delivery::Dropped(_) => panic!("nothing should be dropped"),
            })
            .collect();
        assert_eq!(seen, vec![Phase::Active, Phase::Terminated]);
    }

    #[test]
    fn clones_share_state_across_threads() {
        let lifecycle = Lifecycle::new();
        let other = lifecycle.clone();
        std::thread::spawn(move || {
            other.enter(Phase::Background);
        })
        .join()
        .unwrap();
        assert_eq!(lifecycle.phase(), Phase::Background);
        assert!(lifecycle.foreground().is_cancelled());
    }
}
