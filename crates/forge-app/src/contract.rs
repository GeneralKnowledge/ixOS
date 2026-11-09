//! The shared contract: the types every app's API uses and ForgeKit knows.
//! Changing any of them is a contract change: bump [`CONTRACT_VERSION`].

use std::sync::Arc;

use forge_runtime::{CancelHandle, Delivery, Phase, Subscription};

/// The version of this contract. ForgeKit refuses a Rust library built
/// against a different one, so a mismatched framework fails at launch.
pub const CONTRACT_VERSION: u32 = 1;

/// The contract version the linked Rust library implements.
#[uniffi::export]
pub fn forge_app_contract_version() -> u32 {
    CONTRACT_VERSION
}

/// The application lifecycle phase, as the host reports it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// What kind of failure an error is, so a host treats every app's errors
/// alike. Apps map their own error enums onto it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The input was refused. Show the app's message.
    Invalid,
    /// The thing no longer exists.
    NotFound,
    /// Offline or a service is down. Try again later.
    Unavailable,
    /// Stopped because the app left the screen. Safe to retry on return.
    Interrupted,
    /// Cancelled by the user or the host.
    Cancelled,
    /// Needs a permission or a sign-in.
    NeedsUser,
    /// Saved data could not be read. Offer recovery.
    Corrupt,
    /// Saved data is from a newer app version. Never offer a reset.
    NewerData,
    /// The core has shut down.
    ShutDown,
    /// A bug or an unexpected failure.
    Internal,
}

/// Why the runtime stopped or refused work. App error types implement
/// `From<Stopped>` so runtime calls return the app's own error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped {
    pub kind: ErrorKind,
}

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            ErrorKind::ShutDown => f.write_str("the app has shut down"),
            ErrorKind::Interrupted => f.write_str("interrupted: the app went to the background"),
            ErrorKind::Cancelled => f.write_str("cancelled"),
            kind => write!(f, "stopped ({kind:?})"),
        }
    }
}

impl std::error::Error for Stopped {}

/// What the runtime tells the host. The same in every app; an app with
/// domain events of its own keeps a second stream.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// A command committed. Re-query whatever is on screen.
    Changed { revision: u64 },
    /// The host reported a phase. `resumed` is true on return from the
    /// background: refresh, and restart work the background stopped.
    PhaseChanged { phase: AppPhase, resumed: bool },
    /// This stream fell behind and missed `count` events. Re-read everything.
    EventsDropped { count: u64 },
}

/// One consumer's event stream, pulled by the host.
#[derive(uniffi::Object)]
pub struct RuntimeEvents {
    subscription: Subscription<RuntimeEvent>,
}

impl RuntimeEvents {
    pub(crate) fn new(subscription: Subscription<RuntimeEvent>) -> Arc<Self> {
        Arc::new(Self { subscription })
    }
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

    /// Ends this stream.
    pub fn cancel(&self) {
        self.subscription.cancel();
    }
}

/// Cancels one call into Rust from the host. Swift `Task` cancellation does
/// not reach Rust by itself; ForgeKit's `withRustCancellation` passes one of
/// these and cancels it when the task is cancelled.
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
    /// The Rust handle, for linking into the runtime's scopes.
    pub fn handle(&self) -> &CancelHandle {
        &self.handle
    }
}
