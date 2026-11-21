//! Links Pocket Library's Rust core into Android's `libforge_ffi.so`.
//!
//! Pocket Library's API (`pocket_library_core`) and the application runtime's
//! contract (`forge_app`) keep their own UniFFI namespaces, so the Kotlin
//! bindings are `uniffi.pocket_library_core` and `uniffi.forge_app`, the same
//! API the SwiftUI app calls.
//!
//! One function is Android's own, in `uniffi.forge_ffi`: UniFFI 0.32 doesn't
//! apply the Kotlin rename (`Library` → `PocketLibrary`) to the return type of
//! the core's second constructor, `Library.open_fresh`, so the Kotlin for it
//! doesn't compile and it is left out (`uniffi.toml`). The app recovers an
//! unreadable library with [`set_aside_unreadable_library`] and then opens it
//! with the ordinary constructor, which is what `open_fresh` does in one step.

pub use forge_runtime;
pub use pocket_library_core;

use pocket_library_core::{Library, LibraryError};

uniffi::setup_scaffolding!();

/// Recovery for an unreadable library: moves `library.json` aside (kept as
/// `library.unreadable-<time>.json`, never deleted) so the next open starts
/// empty. Refuses a library saved by a newer app version, like
/// `Library.open_fresh`, whose work this is; the library it opens is shut down
/// again at once, so the directory is free for the app to open.
#[uniffi::export]
pub fn set_aside_unreadable_library(storage_directory: String) -> Result<(), LibraryError> {
    let library = Library::open_fresh(storage_directory)?;
    library.shutdown();
    Ok(())
}
