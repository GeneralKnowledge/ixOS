//! The Counter example's Rust library: a counter whose rules live in Rust
//! ([`domain`]), persisted as a versioned JSON snapshot ([`store`]), exposed to
//! Swift and Kotlin as `ForgeCounterApp` with a serial command thread, pulled
//! events and cancellable long-running operations.

mod api;
mod app;
mod domain;
mod errors;
mod events;
mod session;
mod signal;
mod store;

pub use api::*;
pub use errors::ForgeAppError;
pub use events::{CounterEvent, CounterEventSubscription, OperationOutcome, QUEUE_CAPACITY};

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library.
pub use forge_runtime as _runtime;

uniffi::setup_scaffolding!("forge_ffi");
