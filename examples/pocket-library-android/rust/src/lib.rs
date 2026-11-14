//! Links Pocket Library's Rust core into Android's `libforge_ffi.so`.
//!
//! Nothing is exported from here. Pocket Library's API (`pocket_library_core`)
//! and the application runtime's contract (`forge_app`) keep their own UniFFI
//! namespaces, so the Kotlin bindings are `uniffi.pocket_library_core` and
//! `uniffi.forge_app`, the same API the SwiftUI app calls.

pub use forge_runtime;
pub use pocket_library_core;
