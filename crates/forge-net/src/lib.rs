//! Networking interface for Forge Mobile apps.
//!
//! **Prototype.** This crate fixes the *shape* of networking in Forge so app
//! code and adapters can be written against it; it ships no real transport.
//! [`NoopClient`] refuses every request with [`NetError::Unavailable`], and
//! [`StaticClient`] returns canned responses for tests.
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
//! Nothing in `forge-ffi` uses this crate yet. See `docs/architecture.md`.

mod client;
mod types;

pub use client::{HttpClient, NoopClient, StaticClient};
pub use types::{Header, Method, NetError, Request, Response};
