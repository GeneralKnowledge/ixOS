//! The ixOS application runtime (docs/app-model-review.md, phase 1).
//!
//! What every app needs and none should write: one core per process and per
//! data directory, commands that run one at a time and commit before they
//! are announced, a revision the host re-queries on, the application
//! lifecycle with on-screen work that stops in the background, cancellation
//! from the host, and shutdown. The contract types ([`AppPhase`],
//! [`ErrorKind`], [`RuntimeEvent`], [`RuntimeEvents`], [`Cancellation`]) are
//! a UniFFI component of their own: apps use them in their exports as
//! external types, and ForgeKit ships their Swift.
//!
//! What it deliberately does not do: own the app's state type or its
//! storage. The executor runs the app's closures over whatever state the
//! app keeps; JSON, SQLite, files or nothing stay the app's choice.
//!
//! Two optional foundations sit beside the runtime: [`document`], an app's
//! state as one versioned JSON file with the save and recovery rules, and
//! [`validation`], field-by-field input errors ([`FieldError`] is part of
//! the shared contract, so hosts show every app's errors alike).
//!
//! ```ignore
//! #[derive(uniffi::Object)]
//! pub struct AppCore { rt: forge_app::Runtime<Notes> }
//!
//! #[uniffi::export]
//! impl AppCore {
//!     pub async fn add_note(&self, text: String) -> Result<u64, AppError> {
//!         self.rt.command("add-note", move |notes| notes.add(text)).await
//!     }
//! }
//! forge_app::export_runtime!(AppCore, rt);
//! ```

mod contract;
pub mod document;
mod executor;
mod published;
mod runtime;
pub mod validation;

pub use contract::{
    forge_app_contract_version, AppPhase, Cancellation, ErrorKind, RuntimeEvent, RuntimeEvents,
    Stopped, CONTRACT_VERSION,
};
pub use document::{Document, DocumentError};
pub use published::Published;
pub use runtime::{DataDirectory, OpenError, Runtime, Work};
pub use validation::{FieldError, Invalid, Validator};

// Re-exported so `export_runtime!` and apps need no direct dependency.
#[doc(hidden)]
pub use forge_runtime as __runtime;

uniffi::setup_scaffolding!("forge_app");

/// Exports the runtime half of an app's interface on the app's core object:
/// `events()`, `set_phase(phase)`, `phase()`, `revision()` and
/// `shutdown()`. `$field` names the core's [`Runtime`].
#[macro_export]
macro_rules! export_runtime {
    ($core:ty, $field:ident) => {
        #[::uniffi::export]
        impl $core {
            /// The runtime's event stream: `Changed { revision }` after each
            /// committed command, phase changes, and drops. Nil after shutdown.
            pub fn events(&self) -> ::std::option::Option<::std::sync::Arc<$crate::RuntimeEvents>> {
                self.$field.events()
            }

            /// The host reports the application lifecycle phase.
            pub fn set_phase(&self, phase: $crate::AppPhase) {
                self.$field.set_phase(phase)
            }

            /// The phase last reported.
            pub fn phase(&self) -> $crate::AppPhase {
                self.$field.phase()
            }

            /// The last committed revision.
            pub fn revision(&self) -> u64 {
                self.$field.revision()
            }

            /// Refuses new commands, stops work in flight and ends event
            /// streams. Commands already accepted still finish and save.
            pub fn shutdown(&self) {
                self.$field.shutdown()
            }
        }
    };
}
