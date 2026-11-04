//! The Chat example: rooms, messages and a live per-room event stream over
//! `chat_core`. See `app.rs` for the concurrency contract, `events.rs` for
//! event delivery and `docs/chat-demo.md` for the design.

mod app;
mod events;
mod types;

pub use app::ForgeChatApp;
pub use events::{ChatEventSubscription, CHAT_QUEUE_CAPACITY};
pub use types::*;
