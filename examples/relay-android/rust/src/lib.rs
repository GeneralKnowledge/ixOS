//! Links Relay's Rust core into Android's `libforge_ffi.so`.
//!
//! Relay's API (`relay_core`), the application runtime's contract
//! (`forge_app`) and the native services (`forge_services`) keep their own
//! UniFFI namespaces, so the Kotlin bindings are `uniffi.relay_core` and the
//! rest: the same API the SwiftUI app calls. Nothing here is Android's own.

pub use forge_runtime;
pub use relay_core;
