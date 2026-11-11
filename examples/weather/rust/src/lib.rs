//! The Weather example's Rust library: place search and forecasts from
//! Open-Meteo over `forge-net`, with an offline cache, exposed to Swift as
//! `ForgeWeatherApp`.

mod signal;
mod weather;

pub use weather::*;

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library.
pub use forge_runtime as _runtime;

// No namespace argument: UniFFI takes the library's name, forge_ffi here.
// The Device Lab showcase links this crate as a library with another name
// (examples/device-lab/rust/embedded/), and the name keeps the two apart.
uniffi::setup_scaffolding!();
