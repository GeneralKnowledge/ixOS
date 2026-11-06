//! The runtime half of a Forge app's Rust library.
//!
//! An app's Rust crate (the `rust/` folder `forge init` creates) defines its
//! own UniFFI API: objects, records, errors and events. This crate supplies
//! the plumbing that API needs, so app code stays about the app:
//!
//! - [`background`]: run blocking work (file I/O, an HTTP request) on its own
//!   thread and `await` it from an exported `async fn`. Swift and Kotlin see
//!   an ordinary `async`/`suspend` function; no async runtime is involved.
//! - [`EventHub`] / [`Subscription`]: bounded, pull-based event streams. The
//!   host awaits `next()`; a slow consumer loses the oldest events and is told
//!   how many ([`Delivery::Dropped`]) instead of growing a queue forever.
//! - The Android JNI entry point behind `ForgeAndroid.init(context)`, which
//!   HTTPS needs on Android (see `forge_net::platform`). Linking this crate is
//!   enough; apps call nothing.
//!
//! See `docs/app-crate.md` for how an app crate uses these.

mod background;
mod events;
#[cfg(not(target_os = "ios"))]
mod jni_init;

pub use background::{background, block_on, Background};
pub use events::{Delivery, EventHub, Subscription, DEFAULT_QUEUE_CAPACITY};

/// Re-exported so app crates use the exact transport and storage versions the
/// runtime was built against.
pub use forge_net as net;
