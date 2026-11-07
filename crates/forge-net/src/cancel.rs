//! Cooperative cancellation that can also interrupt blocked I/O.
//!
//! A [`CancelHandle`] is a shared flag plus a list of *abort hooks*. Code
//! that may block (a transport reading a socket, a test double waiting on a
//! channel) registers a hook that unblocks it; [`CancelHandle::cancel`] sets
//! the flag and runs every hook once. Code that only sleeps can use
//! [`CancelHandle::wait_timeout`], which wakes as soon as the handle is
//! cancelled.
//!
//! Hooks are removed when their [`AbortRegistration`] is dropped, so a
//! long-lived handle (one per chat-room subscription, say) does not collect a
//! hook for every reconnect.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct State {
    cancelled: bool,
    next_id: u64,
    hooks: BTreeMap<u64, Hook>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// A cloneable cancellation token. Clones share one state: cancelling any
/// clone cancels them all. Cancelling is permanent and idempotent.
#[derive(Clone, Default)]
pub struct CancelHandle {
    shared: Arc<Shared>,
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("CancelHandle")
            .field("cancelled", &state.cancelled)
            .field("hooks", &state.hooks.len())
            .finish()
    }
}

impl CancelHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Cancels, wakes every [`wait_timeout`](Self::wait_timeout), and runs
    /// the registered abort hooks on this thread (outside the internal lock,
    /// so a hook may use the handle). Later calls do nothing.
    pub fn cancel(&self) {
        let hooks = {
            let mut state = self.lock();
            if state.cancelled {
                return;
            }
            state.cancelled = true;
            std::mem::take(&mut state.hooks)
        };
        self.shared.wake.notify_all();
        for (_, hook) in hooks {
            hook();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    /// Registers `hook` to run when the handle is cancelled. If it already
    /// is, `hook` runs now, on this thread. The hook is dropped unrun when
    /// the returned registration is dropped first.
    pub fn on_cancel(&self, hook: impl FnOnce() + Send + 'static) -> AbortRegistration {
        let mut state = self.lock();
        if state.cancelled {
            drop(state);
            hook();
            return AbortRegistration { slot: None };
        }
        let id = state.next_id;
        state.next_id += 1;
        state.hooks.insert(id, Box::new(hook));
        AbortRegistration {
            slot: Some((Arc::downgrade(&self.shared), id)),
        }
    }

    /// Blocks for up to `timeout`, returning early once the handle is
    /// cancelled. Returns `true` if it was cancelled.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        while !state.cancelled {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .shared
                .wake
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        true
    }

    /// A new handle that is cancelled when this one is (or on its own). Use
    /// it for one piece of work under a longer-lived owner, such as a
    /// request under an app instance that `shutdown` cancels. The link holds
    /// a hook on this handle until the returned registration is dropped, so
    /// keep the registration for as long as the work runs, then drop it.
    pub fn child(&self) -> (CancelHandle, AbortRegistration) {
        let child = CancelHandle::new();
        let registration = {
            let child = child.clone();
            self.on_cancel(move || child.cancel())
        };
        (child, registration)
    }

    /// Number of hooks waiting to run. For tests that check hooks do not
    /// pile up.
    pub fn pending_hooks(&self) -> usize {
        self.lock().hooks.len()
    }
}

/// Keeps an abort hook registered. Dropping it unregisters the hook (once
/// the blocking operation it guards is finished, it has nothing to abort).
#[must_use = "dropping the registration removes the hook at once"]
pub struct AbortRegistration {
    slot: Option<(std::sync::Weak<Shared>, u64)>,
}

impl fmt::Debug for AbortRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbortRegistration")
            .field("active", &self.slot.is_some())
            .finish()
    }
}

impl Drop for AbortRegistration {
    fn drop(&mut self) {
        if let Some((shared, id)) = self.slot.take() {
            if let Some(shared) = shared.upgrade() {
                let hook = shared
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .hooks
                    .remove(&id);
                // Drop the hook outside the lock: it may own things whose
                // destructors take other locks.
                drop(hook);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn cancel_runs_hooks_once_and_is_idempotent() {
        let handle = CancelHandle::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&runs);
        let _reg = handle.on_cancel(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!handle.is_cancelled());
        handle.clone().cancel();
        handle.cancel();
        assert!(handle.is_cancelled());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(handle.pending_hooks(), 0);
    }

    #[test]
    fn hook_registered_after_cancel_runs_immediately() {
        let handle = CancelHandle::new();
        handle.cancel();
        let runs = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&runs);
        let _reg = handle.on_cancel(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropped_registration_removes_its_hook() {
        let handle = CancelHandle::new();
        let runs = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            let r = Arc::clone(&runs);
            let reg = handle.on_cancel(move || {
                r.fetch_add(1, Ordering::SeqCst);
            });
            drop(reg);
        }
        assert_eq!(handle.pending_hooks(), 0);
        handle.cancel();
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn child_follows_its_parent_but_not_the_other_way() {
        let parent = CancelHandle::new();
        let (child, link) = parent.child();
        child.cancel();
        assert!(!parent.is_cancelled());
        drop(link);
        assert_eq!(parent.pending_hooks(), 0);

        let (child, _link) = parent.child();
        parent.cancel();
        assert!(child.is_cancelled());

        // A child of a cancelled parent starts cancelled.
        let (late, _) = parent.child();
        assert!(late.is_cancelled());
    }

    #[test]
    fn wait_timeout_wakes_on_cancel() {
        let handle = CancelHandle::new();
        assert!(!handle.wait_timeout(Duration::from_millis(10)));
        let canceller = {
            let handle = handle.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                handle.cancel();
            })
        };
        let started = Instant::now();
        assert!(handle.wait_timeout(Duration::from_secs(30)));
        assert!(started.elapsed() < Duration::from_secs(5));
        canceller.join().unwrap();
    }
}
