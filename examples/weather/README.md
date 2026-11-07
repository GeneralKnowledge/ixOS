# Weather example

A SwiftUI weather app in the style of the iPhone's own: a page per place
("My Location", then the cities you save) that you swipe between, each over a
drawn sky that matches the conditions, with a big current temperature, a
summary sentence over the hourly strip (sunrise and sunset included), a
10-day forecast with temperature range bars, and detail tiles. Data comes
from [Open-Meteo](https://open-meteo.com) (no API key). It is ixOS's own
design built from native SwiftUI parts, not a copy of Apple's app or its
assets.

Where Counter exercises the FFI *contract* (async commands, events,
cancellation, lifecycle), Weather exercises the FFI *type mapping* and real I/O:

| What | Where |
|------|-------|
| Real HTTPS from Rust (`ureq` + rustls, system trust store) | `crates/forge-net` (`UreqClient`, `ureq` feature) |
| Nested payload: records in records, optional records, lists of records, enums with data holding enums | `rust/src/weather/types.rs` → generated Swift structs and enums |
| Refresh / cancellation: one refresh at a time, a new one supersedes the old, `cancel()` resolves at once | `rust/src/weather/app.rs`, `WeatherClient.report(for:)` |
| Error paths: offline, timeout, HTTP status (with the API's reason), bad JSON, invalid input | `WeatherError` (Rust) → `WeatherClientError` (Swift) |
| Offline fallback: a cached report marked `.cached(ageSeconds:problem:)` instead of an error | `app.rs` cache (one report per place), `WeatherPage.staleNotice` |
| Screen logic in Rust: sky scene, the summary sentence, the hourly strip with sunrise/sunset, past hours and days dropped from cached reports, the shared temperature scale | `rust/src/weather/overview.rs` (`weather_overview`) |
| Saved places: add, remove, reorder, persisted atomically | `rust/src/weather/places.rs` |
| Location stays in Swift: CoreLocation one-shot fix, when-in-use permission, city search fallback | `Sources/WeatherBridge/LocationProvider.swift` |

```
WeatherView (pager, ForecastPage, LocationsView, SkyBackground) → WeatherViewModel → WeatherBridge (WeatherClient, LocationProvider)
  → Generated Swift (ForgeFFI) → the app's Rust crate, rust/ (ForgeWeatherApp) → forge-net (UreqClient) → Open-Meteo
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
- **Cache.** The newest report per place (up to 16), written atomically (`forge_storage::write_atomic`)
  after every live refresh. A failed refresh for a network reason falls back to
  it when it is for a place within 5 km and younger than 24 h. A 4xx is never
  masked: it means the request is wrong. Corrupt cache files are ignored.
- **What a page shows** is worked out in Rust by `weather_overview(report,
  now, wind_unit)`, so a report cached yesterday drops the hours and days
  already past, and the summary ("Rain expected around 15:00 (70% chance).
  Wind gusts are up to 18 mph.") follows tested rules. Swift only lays it out.
- **Look.** Pages force the dark appearance over a drawn sky (gradient, sun
  glow, stars, drifting clouds, rain or snow; motion stops with Reduce
  Motion), and cards are `.ultraThinMaterial` tinted by the sky. No image
  assets, no third-party UI code.
- **My Location** is named by the platform's reverse geocoder (CoreLocation)
  and is never saved; the cities you add are.
- **Units** are fixed (°C, km/h, mm) and encoded in field names; wind is
  shown in mph where the locale uses it (US, UK). Times are the
  place's local time as ISO strings, so the UI never converts through the
  device's time zone.
- **Bindings.** The app owns its Rust crate (`rust/`), so its committed
  `Generated/` holds only the Weather API; `scripts/check-app-crates.sh`
  checks it for drift.

## Run

Tests (macOS, no network needed; offline and cancellation paths use local ports):

```bash
./scripts/test-swift-host.sh examples/weather
cargo test --manifest-path rust/Cargo.toml          # Rust side, recorded JSON fixtures
cargo test --manifest-path rust/Cargo.toml -- --ignored live_open_meteo   # real network
```

On a device:

```bash
cargo run -p forge-cli -- run ios --project examples/weather        # Linux, xtool
cargo run -p forge-cli -- build ios --project examples/weather --package-only
./scripts/weather-screenshots.sh <Weather.app> <out-dir>                # simulator review aid (macOS)
open examples/weather/Xcode/Weather.xcodeproj                        # macOS, shared "App" scheme
```

The location permission text is set in `forge.toml` (`[ios.permissions]
location_when_in_use`). Forge writes it into `Info.plist`, which xtool merges
and the Xcode target reads through `INFOPLIST_FILE` ([docs/app-config.md](../../docs/app-config.md)).

## Limits

- iOS only: there is no Compose host for Weather yet. The Rust side is ready
  for one; Android HTTPS works once the host calls `ForgeAndroid.init` (see
  [docs/android-setup.md](../../docs/android-setup.md#https-certificate-verification)).
- The forecast is fixed at 48 hours and 10 days, in °C.
- Only the page on screen refreshes (Rust runs one refresh at a time); the
  others show their cached report until you swipe to them.
- No radar map, unlike Apple's app.
- A search cannot be cancelled once sent; a cancelled caller just never sees
  its results.
