//! Domain core of the Chat example.
//!
//! The Chat example stresses what Counter and Weather do not: a long-lived,
//! high-frequency event stream that outlives any single command, explicit
//! subscribe/unsubscribe with provable cancellation, multi-entity storage
//! with a `since` cursor, and typed network/protocol errors. See
//! `docs/chat-demo.md`.
//!
//! This crate has no FFI. It is plain, blocking Rust, testable on Linux:
//!
//! - [`ChatClient`]: the engine the hosts will drive (commands, the
//!   per-room stream pumps, the store, one event sink).
//! - [`ChatRepo`]: the server as the client sees it, with
//!   [`InMemoryRepo`] (tests, and the reference semantics) and [`HttpRepo`]
//!   (HTTP + Server-Sent Events through `forge_net`).
//! - [`sse`]: a small Server-Sent Events reader.
//! - [`store`]: the SQLite store (snapshot in memory, changes written per
//!   save), its search, and the import of the JSON store from before M2.

mod client;
mod http;
mod memory;
mod repo;
pub mod sse;
pub mod store;
mod types;

pub use client::{
    ChatClient, ChatConfig, EventSink, MAX_BODY_CHARS, MAX_REACTION_CHARS, OUTBOX_INITIAL_DELAY,
    OUTBOX_MAX_DELAY,
};
pub use http::{
    validate_display_name, validate_title, Account, HttpRepo, Identity, DEFAULT_TIMEOUT,
    MAX_DISPLAY_NAME_CHARS, MAX_TITLE_CHARS,
};
pub use memory::{InMemoryRepo, MEMORY_HEARTBEAT};
pub use repo::{ChatRepo, OutgoingMessage, RemoteConversation, RemoteEvent, RemoteStream};
pub use store::{ChatSnapshot, ChatStore, StoreError};
pub use types::{ChatError, ChatEvent, Conversation, Message, MessageStatus};

/// The storage engine compiled into this build, e.g. "SQLite 3.50.2".
pub fn storage_engine() -> String {
    format!("SQLite {}", forge_storage::Database::sqlite_version())
}

/// Wall-clock Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
