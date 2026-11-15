//! The Weather example's Rust library: place search and forecasts from
//! Open-Meteo, with an offline cache, exposed to Swift as `ForgeWeatherApp`.
//! It runs on the application runtime (`forge_app`), hosted by ForgeKit's
//! `ForgeHost`: one core per process, refreshes and searches stop when the
//! app leaves the screen, and requests go through URLSession on iOS.

mod signal;
mod weather;

pub use weather::*;

// Links forge-runtime's `ForgeAndroid.nativeInit` JNI export into this library.
pub use forge_runtime as _runtime;

// No namespace argument: UniFFI takes the library's name (weather_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// Native services the host implements (Keychain, app directories,
// notifications, permissions, deep links) as `crate::native`, plus `http`
// (on iOS forecasts and searches go through URLSession) and `location`
// ("My Location" asks Core Location through it). See docs/native-services.md.
forge_services::native_services!(http, location);

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes into the
// same log as Rust.
forge_runtime::log_export!();
