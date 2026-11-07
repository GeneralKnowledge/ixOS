# Weather example

A small SwiftUI app with a deliberately complex FFI payload: current
conditions, the next 24 hours and the week ahead for your location or a
searched city, fetched from [Open-Meteo](https://open-meteo.com) (no API key).

Where Counter exercises the FFI *contract* (async commands, events,
cancellation, lifecycle), Weather exercises the FFI *type mapping* and real I/O:

| What | Where |
|------|-------|
| Real HTTPS from Rust (`ureq` + rustls, system trust store) | `crates/forge-net` (`UreqClient`, `ureq` feature) |
| Nested payload: records in records, optional records, lists of records, enums with data holding enums | `crates/forge-ffi/src/weather/types.rs` → generated Swift structs and enums |
| Refresh / cancellation: one refresh at a time, a new one supersedes the old, `cancel()` resolves at once | `crates/forge-ffi/src/weather/app.rs`, `WeatherClient.report(for:)` |
| Error paths: offline, timeout, HTTP status (with the API's reason), bad JSON, invalid input | `WeatherError` (Rust) → `WeatherClientError` (Swift) |
| Offline fallback: a cached report marked `.cached(ageSeconds:problem:)` instead of an error | `app.rs` cache, `WeatherViewModel.staleNotice` |
| Location stays in Swift: CoreLocation one-shot fix, when-in-use permission, city search fallback | `Sources/WeatherBridge/LocationProvider.swift` |

```
WeatherView → WeatherViewModel → WeatherBridge (WeatherClient, LocationProvider)
  → Generated Swift (ForgeFFI) → forge-ffi (ForgeWeatherApp) → forge-net (UreqClient) → Open-Meteo
```

## Design notes

- **No serial command thread.** Weather's only state is the cache, so each
  refresh and search runs on its own Rust worker thread and callers never wait
  behind each other.
- **Cancellation.** `ureq` is blocking and cannot abort a request in flight.
  `cancel()` therefore resolves `result()` with `Cancelled` straight away; the
  worker finishes in the background and its response is thrown away (it never
  touches the cache). Swift forwards task cancellation explicitly, because
  UniFFI does not.
- **Cache.** One report, written atomically (`forge_storage::write_atomic`)
  after every live refresh. A failed refresh for a network reason falls back to
  it when it is for a place within 5 km and younger than 24 h. A 4xx is never
  masked: it means the request is wrong. Corrupt cache files are ignored.
- **Units** are fixed (°C, km/h, mm) and encoded in field names. Times are the
  place's local time as ISO strings, so the UI never converts through the
  device's time zone.
- **Bindings.** `forge-ffi` is one library, so its generated bindings include
  both Counter and Weather. Each iOS example commits its own copy under
  `Generated/`; `scripts/check-bindings.sh` checks both.

## Run

Tests (macOS, no network needed; offline and cancellation paths use local ports):

```bash
./scripts/test-swift-host.sh examples/weather
cargo test -p forge-ffi weather          # Rust side, recorded JSON fixtures
cargo test -p forge-ffi -- --ignored live_open_meteo   # real network
```

On a device:

```bash
cargo run -p forge-cli -- run ios --project examples/weather        # Linux, xtool
cargo run -p forge-cli -- build ios --project examples/weather --package-only
open examples/weather/Xcode/Weather.xcodeproj                        # macOS, shared "App" scheme
```

The location permission text is set in `forge.toml` (`[ios.permissions]
location_when_in_use`). Forge writes it into `Info.plist`, which xtool merges
and the Xcode target reads through `INFOPLIST_FILE` ([docs/app-config.md](../../docs/app-config.md)).

## Limits

- iOS only: there is no Compose host for Weather yet. The Rust side is ready
  for one; Android HTTPS works once the host calls `ForgeAndroid.init` (see
  [docs/android-setup.md](../../docs/android-setup.md#https-certificate-verification)).
- The forecast is fixed at 24 hours and 7 days, in metric units.
- A search cannot be cancelled once sent; a cancelled caller just never sees
  its results.
