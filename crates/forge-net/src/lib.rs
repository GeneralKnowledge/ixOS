//! Networking interface for Forge Mobile apps.
//!
//! This crate fixes the *shape* of networking in Forge so app code and
//! adapters can be written against it. [`NoopClient`] refuses every request
//! with [`NetError::Unavailable`], [`StaticClient`] returns canned responses
//! for tests, and with the `ureq` feature [`UreqClient`] is a real blocking
//! HTTPS transport (rustls with the platform certificate verifier).
//!
//! The design mirrors `forge-storage`'s `SnapshotStore`:
//!
//! - A narrow, synchronous [`HttpClient`] trait. Forge's FFI layer already runs
//!   commands on a serial worker thread per app instance, so a blocking call
//!   there does not block the UI and no async runtime is required.
//! - Plain request/response values with no dependency on a particular HTTP
//!   library, so a real implementation can be backed by `ureq`/`reqwest` in
//!   Rust, or by `URLSession` / OkHttp in the host through a UniFFI callback.
//! - Typed errors the FFI layer can map to its own error enum.
//!
//! `forge-ffi` uses it for the Weather example (`ForgeWeatherApp`). See
//! `docs/architecture.md`.

mod cancel;
mod client;
mod types;
#[cfg(feature = "ureq")]
mod ureq_client;

pub use cancel::{AbortRegistration, CancelHandle};
pub use client::{HttpClient, NoopClient, StaticClient};
pub use types::{
    cancelled_io_error, is_cancelled_io_error, Header, Method, NetError, Request, Response,
    StreamingResponse,
};
#[cfg(feature = "ureq")]
pub use ureq_client::{UreqClient, DEFAULT_MAX_BODY_BYTES, DEFAULT_TIMEOUT};
