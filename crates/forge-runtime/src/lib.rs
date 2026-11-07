//! The runtime half of a Forge app's Rust library.
//!
//! An app's Rust crate (the `rust/` folder `forge init` creates) defines its
//! own UniFFI API: objects, records, errors and events. This crate supplies
//! the plumbing that API needs, so app code stays about the app:
//!
//! - [`background`]: run blocking work (file I/O, an HTTP request) on its own
//!   thread and `await` it from an exported `async fn`. Swift and Kotlin see
//!   an ordinary `async`/`suspend` function; no async runtime is involved.
//! - [`background_cancellable`]: the same for work that can be stopped part
//!   way (an HTTP request), cancelled explicitly, by `shutdown`, or when a
//!   Kotlin coroutine drops the call.
//! - [`EventHub`] / [`Subscription`]: bounded, pull-based event streams. The
//!   host awaits `next()`; a slow consumer loses the oldest events and is told
//!   how many ([`Delivery::Dropped`]) instead of growing a queue forever.
//! - [`Lifecycle`]: the app's phase (launching, active, inactive,
//!   background, terminated) as the host reports it, with a lifetime scope
//!   and a foreground scope that cancel work when the app terminates or
//!   leaves the screen, and `resumed` on the way back.
//! - The Android JNI entry point behind `ForgeAndroid.init(context)`, which
//!   HTTPS needs on Android (see `forge_net::platform`). Linking this crate is
//!   enough; apps call nothing.
//!
//! See `docs/app-crate.md` for how an app crate uses these.

mod background;
mod events;
#[cfg(not(target_os = "ios"))]
mod jni_init;
mod lifecycle;

pub use background::{background, background_cancellable, block_on, Background};
pub use events::{Delivery, EventHub, Subscription, DEFAULT_QUEUE_CAPACITY};
pub use forge_net::CancelHandle;
pub use lifecycle::{Lifecycle, Phase, Transition};

/// Re-exported so app crates use the exact transport and storage versions the
/// runtime was built against.
pub use forge_net as net;
