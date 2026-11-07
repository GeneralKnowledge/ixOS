//! The frozen Android Chat example's Rust library: `ForgeChatApp`, a thin
//! UniFFI layer over `chat-core`. Kept as it was before Chat M0 moved the iOS
//! app to `examples/chat/rust`; see `examples/chat-android/README.md`.

mod chat;
mod signal;

pub use chat::*;

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library,
// which HTTPS needs on Android.
pub use forge_runtime as _runtime;

uniffi::setup_scaffolding!("forge_ffi");
