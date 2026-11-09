//! Phase 0 spike: the runtime contract every ixOS app shares, exported once
//! as the UniFFI component `forge_app` (Swift module `ForgeAppFFI`, shipped
//! in ForgeKit). App crates name these types in their own exports; UniFFI
//! treats them as external types and the app's bindings import ForgeAppFFI.
//!
//! Only the types and a minimal `Runtime` are here: enough to prove the
//! boundary, not the executor or operations of phase 1.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use forge_runtime::{CancelHandle, Delivery, EventHub, Lifecycle, Phase, Subscription};

uniffi::setup_scaffolding!("forge_app");

/// The interface version of this component. ForgeKit compares it with the
/// one it was built for, so a mismatched framework fails loudly at launch.
pub const CONTRACT_VERSION: u32 = 1;

/// The application lifecycle phase the host reports.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPhase {
    Launching,
    Active,
    Inactive,
    Background,
    Terminated,
}

impl From<AppPhase> for Phase {
    fn from(p: AppPhase) -> Self {
        match p {
            AppPhase::Launching => Phase::Launching,
            AppPhase::Active => Phase::Active,
            AppPhase::Inactive => Phase::Inactive,
            AppPhase::Background => Phase::Background,
            AppPhase::Terminated => Phase::Terminated,
        }
    }
}

impl From<Phase> for AppPhase {
    fn from(p: Phase) -> Self {
        match p {
            Phase::Launching => AppPhase::Launching,
            Phase::Active => AppPhase::Active,
            Phase::Inactive => AppPhase::Inactive,
            Phase::Background => AppPhase::Background,
            Phase::Terminated => AppPhase::Terminated,
        }
    }
}

/// What kind of failure an app error is, so the host can treat every app's
/// errors alike (retry, recovery screen, never offer a reset, ...).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Invalid,
    NotFound,
    Unavailable,
    Interrupted,
    Cancelled,
    NeedsUser,
    Corrupt,
    NewerData,
    ShutDown,
    Internal,
}

/// What the runtime tells the host. Apps with their own domain events keep
/// a second stream; this one is the same in every app.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// A command committed. Re-query whatever is on screen.
    Changed { revision: u64 },
    /// The host reported a phase. `resumed` is true on return from the background.
    PhaseChanged { phase: AppPhase, resumed: bool },
    /// This stream fell behind and missed `count` events: re-read everything.
    EventsDropped { count: u64 },
}

/// A host-side handle that cancels one call into Rust (Swift Task
/// cancellation does not reach Rust by itself).
#[derive(uniffi::Object, Default)]
pub struct Cancellation {
    handle: CancelHandle,
}

#[uniffi::export]
impl Cancellation {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn cancel(&self) {
        self.handle.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.handle.is_cancelled()
    }
}

impl Cancellation {
    /// The Rust handle, for app code linking it to its own scopes.
    pub fn handle(&self) -> &CancelHandle {
        &self.handle
    }
}

/// The runtime's event stream, pulled by the host.
#[derive(uniffi::Object)]
pub struct RuntimeEvents {
    subscription: Subscription<RuntimeEvent>,
}

#[uniffi::export]
impl RuntimeEvents {
    /// The next event, or nil once the stream has ended.
    pub async fn next(&self) -> Option<RuntimeEvent> {
        match self.subscription.next().await? {
            Delivery::Event(event) => Some(event),
            Delivery::Dropped(count) => Some(RuntimeEvent::EventsDropped { count }),
        }
    }

    pub fn cancel(&self) {
        self.subscription.cancel();
    }
}

/// The contract version the linked Rust library implements.
#[uniffi::export]
pub fn forge_app_contract_version() -> u32 {
    CONTRACT_VERSION
}

/// The plain-Rust part an app embeds: revision, lifecycle, events, shutdown.
/// Not exported; the app exports its own object and delegates to this.
pub struct Runtime {
    revision: AtomicU64,
    lifecycle: Lifecycle,
    events: EventHub<RuntimeEvent>,
    shut_down: AtomicBool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            lifecycle: Lifecycle::new(),
            events: EventHub::new(),
            shut_down: AtomicBool::new(false),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// Call after a command has committed.
    pub fn committed(&self) -> u64 {
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.events.publish(RuntimeEvent::Changed { revision });
        revision
    }

    pub fn subscribe(&self) -> Option<Arc<RuntimeEvents>> {
        self.events
            .subscribe()
            .map(|subscription| Arc::new(RuntimeEvents { subscription }))
    }

    pub fn phase(&self) -> AppPhase {
        self.lifecycle.phase().into()
    }

    pub fn set_phase(&self, phase: AppPhase) {
        if phase == AppPhase::Terminated {
            return self.shutdown();
        }
        if let Some(change) = self.lifecycle.enter(phase.into()) {
            self.events.publish(RuntimeEvent::PhaseChanged {
                phase,
                resumed: change.resumed,
            });
        }
    }

    pub fn is_shut_down(&self) -> bool {
        self.shut_down.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        if self.lifecycle.enter(Phase::Terminated).is_some() {
            self.events.publish(RuntimeEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false,
            });
        }
        self.events.close();
    }

    /// A cancel handle for work that should stop when the app leaves the
    /// screen, also cancelled by `host` if given.
    pub fn foreground_work(&self, host: Option<&Cancellation>) -> ForegroundWork {
        let (cancel, link) = self.lifecycle.foreground().child();
        let host_link = host.map(|c| {
            let cancel = cancel.clone();
            c.handle.on_cancel(move || cancel.cancel())
        });
        ForegroundWork {
            cancel,
            _links: Box::new((link, host_link)),
            lifetime: self.lifecycle.lifetime(),
            foreground: self.lifecycle.foreground(),
        }
    }
}

/// A unit of on-screen work and the reason it may stop.
pub struct ForegroundWork {
    pub cancel: CancelHandle,
    _links: Box<dyn std::any::Any + Send>,
    lifetime: CancelHandle,
    foreground: CancelHandle,
}

impl ForegroundWork {
    /// Why the work was cancelled, as an error kind.
    pub fn stop_kind(&self) -> ErrorKind {
        if self.lifetime.is_cancelled() {
            ErrorKind::ShutDown
        } else if self.foreground.is_cancelled() {
            ErrorKind::Interrupted
        } else {
            ErrorKind::Cancelled
        }
    }
}
