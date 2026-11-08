//! Deliberate crashes, so crash reading can be checked on a phone down to a
//! file and line in this app's own Rust (`forge device crashes --pull`,
//! docs/forge-device.md#crash-reports). Only the Crash Test screen calls it.

use forge_runtime::logging;

/// Crashes the app with a Rust panic that nothing catches.
///
/// UniFFI catches panics in exported calls and hands Swift an error, so a
/// panic there would not crash. A real uncaught panic happens where Rust
/// can't unwind, such as a C callback, and Rust then aborts the app. This
/// does the same: the callback below can't unwind.
#[uniffi::export]
pub fn crash_test_rust_panic() {
    logging::warn!(target: "crash-test", "crashing on purpose with a Rust panic");
    run_callback(on_reading);
}

#[inline(never)]
fn run_callback(callback: extern "C" fn()) {
    callback();
}

extern "C" fn on_reading() {
    // `black_box` keeps the compiler from seeing the list is empty.
    let readings: Vec<u32> = std::hint::black_box(Vec::new());
    let latest = latest_reading(&readings);
    logging::info!(target: "crash-test", "latest reading {latest}");
}

/// The bug the crash test plants: assumes there is always a reading.
pub(crate) fn latest_reading(readings: &[u32]) -> u32 {
    *readings.last().expect("crash test: no readings yet")
}
