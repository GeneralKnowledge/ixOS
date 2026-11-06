mod api;
mod app;
mod chat;
mod errors;
mod events;
mod session;
mod signal;
mod weather;

pub use api::*;
pub use chat::*;
pub use errors::ForgeAppError;
pub use events::{CounterEvent, CounterEventSubscription, OperationOutcome, QUEUE_CAPACITY};
pub use weather::*;

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library.
pub use forge_runtime as _runtime;

uniffi::setup_scaffolding!("forge_ffi");
