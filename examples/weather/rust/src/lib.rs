//! The Weather example's Rust library: place search and forecasts from
//! Open-Meteo over `forge-net`, with an offline cache, exposed to Swift as
//! `ForgeWeatherApp`.

mod signal;
mod weather;

pub use weather::*;

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library.
pub use forge_runtime as _runtime;

uniffi::setup_scaffolding!("forge_ffi");
