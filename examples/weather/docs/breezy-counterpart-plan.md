# Weather as an iOS counterpart to Breezy Weather: plan

Status: proposal, 2026-11-19, waiting for Reuben's approval. Nothing here is built yet.

Breezy Weather ([breezy-weather/breezy-weather](https://github.com/breezy-weather/breezy-weather), Kotlin, Jetpack Compose, LGPL-3.0, looked at on commit `d91fcf4`, 2026-11-18) is the largest open-source weather app on Android: forecasts, nowcasting, air quality, pollen, alerts and normals from more than 50 sources. It has no iOS client. This plan grows our own Weather app until it covers Breezy's product on iOS, under our own name and design, so that it proves what ixOS can do and could later be offered to the open-source community as a proper iOS counterpart.

It follows the roadmap rule of growing existing apps before adding new ones: Weather already has places, location, a cache, a Rust forecast client and a native look, so Breezy is treated as a **feature specification** for Weather, not as code to translate.

## Decisions this plan asks for

1. **Clean-room, not a code port** (recommended; [licence](#licence-lgpl-30)). We use Breezy's documentation and source list as a map of what exists, and write every source from the provider's own public API documentation. Weather stays MIT/Apache like the rest of ixOS.
2. **Our name, our look.** It stays "Weather" in Device Lab. A public name comes only with phase W6 ([branding](#branding-and-trademarks)).
3. **Key-free sources first**, then bring-your-own-key sources. We never ship or reuse anyone else's API keys.
4. **Phase order** W1 to W6 below, one PR per phase, each ending in a short phone check in the next combined Device Lab build.
5. **Contacting the Breezy maintainers** is Reuben's call. Recommended: before anything is published under a public name (W6), not before building.

## How Breezy is built

| Part | What it does | What it means for us |
|---|---|---|
| `domain/` | The model: `Location`, `Weather` (`Current`, `Hourly`, `Daily` with day and night half-days, `Minutely`, `Alert`, `Normals`, `AirQuality`, `Pollen`, `Astro`, `MoonPhase`, `UV`, dew point, pressure, visibility, cloud cover, degree days) | Our `WeatherReport` grows towards this shape, written as our own Rust types |
| `app/.../common/source/` | Interfaces: `WeatherSource`, `FeatureSource`, `LocationSearchSource`, `ReverseGeocodingSource`, `ConfigurableSource` (API keys), `NonFreeNetSource` | One Rust trait per role in Weather's crate; same idea, own design |
| `SourceFeature` | `FORECAST`, `CURRENT`, `AIR_QUALITY`, `POLLEN`, `MINUTELY`, `ALERT`, `NORMALS`, `REVERSE_GEOCODING` | The feature list we copy as a fact: one main forecast source per place, and a chosen source for each other feature |
| `sources/<id>/` (58 folders, about 50 of them weather providers) | One service per provider: Retrofit API, JSON classes, converter to the domain model, priority per country | Each provider becomes a Rust module written from that provider's docs, tested with recorded fixtures |
| `RefreshHelper` (1,485 lines) | Asks the main source and each secondary source in parallel, merges, keeps the old data on failure, decides when data is still fresh | Our `app.rs` refresh grows into this: parallel per-feature requests, merge, per-feature fallback to the cache |
| `CommonConverter` (1,481 lines) | Fills gaps: daily from hourly, half-days, wind direction, sun and moon times and moon phase computed locally, current from hourly, UV from the day's maximum | A `complete.rs` and `astro.rs` in Rust, from published rules (WMO, Meeus-style astronomy), tested |
| `domain/weather/index/` | Air quality and pollen indices | Index tables from the official breakpoint tables (EU EAQI, US EPA AQI) |
| `ui/`, `ui-weather-view/`, `remoteviews/`, `wallpaper/` | Compose screens, animated weather backgrounds, widgets, live wallpaper | Nothing carried over: SwiftUI on ForgeUI, our drawn `SkyBackground`, WidgetKit |
| Flavours `basic` and `freenet` | `freenet` refuses sources whose servers are not free software (F-Droid's NonFreeNet anti-feature) | Not needed on iOS; we record each source's licence and terms instead |

The homepage behaviour Breezy documents (`docs/HOMEPAGE.md`) is useful as a product spec: a header with condition, temperature, feels-like and the next two half-day temperatures; then reorderable blocks (alerts always first, nowcasting, daily, hourly, precipitation, wind, air quality, pollen, humidity, UV, visibility, pressure, sun, moon); every block opens a details screen. Days run 06:00 to 05:59, split into day (06:00 to 17:59) and night.

## Sources we can use from iOS

Weather calls providers straight from the phone, from Rust through `forge-net`. A source qualifies for the first phases only if it needs **no API key and no server of ours**. Breezy's own release builds carry the project's keys; they are not in the repository and we must never use them.

### Key-free, worldwide (W1 and W2)

| Source | Gives | Terms to respect |
|---|---|---|
| [Open-Meteo](https://open-meteo.com) forecast API (already used) | Current, hourly and daily up to 16 days, 15-minute data (nowcasting, best in Europe and North America), many models including UK Met Office (`ukmo_seamless`), DWD ICON, ECMWF, Météo-France | Free for non-commercial use, about 10,000 calls a day, CC BY 4.0 attribution shown in the app. A paid or ad-supported release needs their commercial plan |
| Open-Meteo air-quality API | European and US AQI, PM2.5, PM10, ozone, NO₂, SO₂, CO, hourly | As above |
| Open-Meteo air-quality API, pollen fields | Alder, birch, grass, mugwort, olive, ragweed (Europe only, CAMS) | As above; show "not available here" outside Europe |
| Open-Meteo geocoding (already used) | City search | As above |
| [FOSS Public Alert Server](https://alerts.kde.org) | Official warnings (CAP) from many national services, by point | Community-run (KDE), with its own terms of service: cache, don't poll often |
| [WMO Severe Weather](https://severeweather.wmo.int) | Official warnings worldwide | Public feeds |
| NCEI | Climate normals (monthly averages) | US public data |
| Apple CoreLocation (`CLGeocoder`, already used) | Reverse geocoding, the place name | Platform; replaces Breezy's Android and Nominatim address sources |
| Sun, moon, moon phase | Computed in Rust, no source | Nothing |

### Key-free, national (W3, one at a time, highest use first)

| Source | Country | Gives | Terms |
|---|---|---|---|
| MET Norway (Locationforecast, Nowcast, MetAlerts) | Worldwide forecast; Nordic nowcast and alerts | Forecast, nowcasting, alerts, air quality (Norway) | CC BY 4.0; must send an identifying User-Agent (`forge-net` already supports one), respect `Expires`, at most 4 decimals in coordinates |
| NWS (api.weather.gov) | United States | Forecast, current, alerts | Public domain; identifying User-Agent required |
| Bright Sky (DWD open data) | Germany | Forecast, current, nowcasting, alerts | Open data, no key |
| Others Breezy lists without a key (DMI, FMI, SMHI, KNMI, IPMA, JMA, HKO, GeoSphere Austria, Veðurstofa Íslands and more) | Their countries | Varies | Read each provider's terms before writing its module |

### Need a key (W3, bring your own key)

AccuWeather, OpenWeather, Pirate Weather, Met Office DataHub, AEMET, CWA, Infoplaza, GeoNames, Météo-France, ECCC, MET Éireann, BMKG, Atmo (France), Polleninfo. Breezy ships its own keys for some of them; we can't. A person pastes their own key in settings, it is kept in the Keychain (`forge-services` keychain service already exists), and the source shows as unavailable until then. Met Office matters most for Reuben (UK), but Open-Meteo's UK Met Office model covers the forecast without a key, so this is not urgent.

### Not possible or not worth it on iOS for now

- **Apple WeatherKit** needs a paid developer team; we have a free one. Revisit only with a paid team, and even then it would be one more source, not a replacement.
- **China, Baidu IP, IP.SB location** sources: IP location is pointless with CoreLocation; China's source is a reverse-engineered app API.
- **Radar**: Breezy has none either (`docs/RADAR.md`: no free radar API good enough). Out of scope.

## Where Weather is today and the gap

| Area | Weather now | Breezy | Phase |
|---|---|---|---|
| Places | My Location + saved places, reorder, CoreLocation, city search | Same, plus per-place sources and settings | W3 (per-place sources) |
| Current | Temperature, feels like, humidity, precipitation, wind, condition | Plus dew point, pressure, visibility, cloud cover, UV, ceiling | W1 |
| Hourly | 48 hours: temperature, rain chance, condition, gusts | Up to 16 days, all fields, detailed 24-hour charts | W1 (data), W4 (charts) |
| Daily | 10 days: high, low, rain chance and amount, sunrise, sunset, UV max | Up to 16 days, day and night halves, degree days, sunshine duration | W1 |
| Nowcasting (next hour) | None | Minute or 15-minute precipitation block | W2 |
| Air quality | None | Index and pollutants, hourly and daily | W1 |
| Pollen | None | Per species, daily | W1 |
| Alerts | None | Always-first block, details, notifications | W2 |
| Normals | None | Monthly normals compared with today | W3 |
| Sun and moon | Sunrise and sunset from Open-Meteo | Sun and moon times, moon phase, computed when a source lacks them | W1 |
| Sources | Open-Meteo only, hard-wired | 50+ sources, main + secondary per feature, priorities per country | W1 (structure), W3 (more sources) |
| Units | Fixed °C, km/h (mph in US/UK), mm | All units, per user | W1 |
| Details screens | None | Every block opens one | W4 |
| Block order | Fixed | Reorderable, can hide | W4 |
| Widgets | None | Many | W5 |
| Background refresh, notifications | None | Scheduled refresh, alert and precipitation notifications | W2 (alerts), W5 (refresh) |
| Offline | Newest report per place, 24 h fallback | Keeps old data per feature | W1 |
| Accessibility | Identifiers, VoiceOver labels, Dynamic Type | Content descriptions, partial drag-and-drop | Every phase |
| Live wallpaper, icon packs, Gadgetbridge sharing | n/a | Yes | Not planned (Android-specific) |

## Design in ixOS terms

Rust owns sources, merging, completion, indices, units and freshness; Swift shows the result and owns CoreLocation, WidgetKit and Swift Charts. All of it stays in Weather's own crate (`examples/weather/rust/`): a weather source registry is app domain logic, and nothing in this plan becomes framework code without a second app needing it.

```
rust/src/weather/
  sources/mod.rs        Source trait: id, name, terms, features, priority for a coordinate, fetch -> partial report
  sources/open_meteo/   forecast.rs, air_quality.rs, geocoding.rs (today's open_meteo.rs split up)
  sources/met_norway.rs, nws.rs, bright_sky.rs, fpas.rs, ...   (W2, W3)
  merge.rs              main source + one source per feature -> WeatherReport, per-feature fallback to cache
  complete.rs           fill gaps (daily from hourly, half-days, dew point, feels like)
  astro.rs              sun and moon times, moon phase
  indices.rs            AQI and pollen levels from official tables
  units.rs              unit settings and conversion (stored values stay SI)
  overview.rs           (exists) grows to describe blocks and the header
```

- **Requests.** One refresh still supersedes the previous one, but inside it each feature's source is asked in parallel on its own worker. A failed secondary source never fails the page: that block falls back to its cached copy or hides with a reason.
- **Cache.** Schema 3: per-feature data with its own fetch time and source, migrated from schema 2 on read (persistence.md rules).
- **Attribution.** Every block names its source, and the settings list each source's licence and terms (CC BY attribution is a requirement, not decoration).
- **Tests.** Each source gets recorded JSON fixtures and parser tests; merge and completion get table tests; live tests stay `#[ignore]` as today. Requirements WX-5 onwards go into `requirements.toml` as each phase lands, each with its twin tests and one device scenario.
- **Framework touches**, only where already planned for other apps: Swift Charts style into ForgeUI after FieldKit and Good Dog (roadmap wave 5 already lists "then Weather's hourly chart"); widgets stay in Weather until Good Dog or Checklist needs them (they are the ledger's candidate pair); alert notifications and background refresh use the existing `forge-services` notifications and background services. The capability ledger is updated in the PR that first uses each capability in Weather.

## Phases

Each phase is one PR with Linux checks green, then a short check in the next combined Device Lab build (no separate install, about 10 minutes, a row in `docs/device-validation.md`).

| Phase | What lands | Phone check (what it proves) |
|---|---|---|
| **W1 Source model and the missing data** | Source trait, Open-Meteo split into forecast, air quality and pollen; merge, completion, sun and moon, units settings, 16 days; cache schema 3; new blocks: air quality, pollen, humidity and dew point, pressure, visibility, UV, sun, moon; source attribution | Edinburgh and one non-European city: blocks show, pollen says "not available" outside Europe, units switch, airplane mode keeps the cached blocks |
| **W2 Alerts and nowcasting** | FPAS and WMO alerts, alerts block always first, alert details; precipitation in the next hour from Open-Meteo 15-minute data; a local notification for a new severe alert while the app is open or refreshing | A place with an active warning shows it first; next-hour rain chart appears only when rain is due |
| **W3 More sources and choice per place** | MET Norway, NWS, Bright Sky; per-place main source and per-feature sources with sensible defaults by country; bring-your-own-key sources (Met Office first), keys in the Keychain; NCEI normals | Switch a UK place between Open-Meteo and MET Norway; a key-needing source stays off until a key is pasted |
| **W4 Charts and details** | Swift Charts hourly and daily charts, a details screen for every block, block order and hiding | Charts read correctly with VoiceOver and at the largest text size |
| **W5 Widgets and background refresh** | WidgetKit widget (small and medium) in Device Lab's extension, background refresh feeding the widget and alert notifications | Widget on the home screen updates after a refresh. Uses one App ID from the free team's 10 a week |
| **W6 Community release** | Decide name and icon, contact Breezy maintainers, choose distribution (needs a paid Apple team for TestFlight or the App Store, see the App Store release thread), Open-Meteo terms for that distribution, standalone app target | Decided with Reuben; not started without his go |

W1 is the largest phase (about the size of the native redesign, #52); W2 to W4 are each smaller. W5 waits for the widget capability work and W6 is a decision, not a build.

## Licence: LGPL-3.0

Breezy is LGPL-3.0 with an additional term (`LICENSE_ADDITIONAL`): no rights in its trademarks or logos, and modified versions must be marked as different from the original.

What is free to use without any obligation: **facts**. Which providers exist, their endpoints and parameters, which features each covers, WMO weather codes, official AQI breakpoint tables, the astronomy formulas, and the product behaviour Breezy documents. None of these are Breezy's copyright.

What carries LGPL obligations: Breezy's **code, strings, translations, icons and documentation text**. Translating its Kotlin into Rust or Swift is a derivative work and would have to be LGPL-3.0, with its notices, source available, and the right for users to relink a modified library. Static linking into an iOS app makes relinking awkward, and distributing LGPLv3 code through the App Store is a disputed area (its "no further restrictions" term against Apple's usage rules).

**Recommendation: clean-room.**

- Read Breezy's docs and source list for orientation only. Write each source module from the provider's own documentation and real responses, and say in the module's header which documentation it was written from.
- Never copy Breezy's Kotlin, string resources, translations, icons, animations or screenshots.
- Keep Weather under ixOS's MIT/Apache licence.
- If the maintainers later want to share code in either direction, put that code in a separate crate with its own licence, decided with them. The provider modules are small enough that writing them from the API docs costs little more than translating Kotlin.

This is a plan, not legal advice; a public release (W6) should get a short licence review first.

## Branding and trademarks

- No "Breezy" in the app name, bundle id, icon, App Store text or screenshots, and no use of its icon or artwork.
- Docs may say factually that Weather aims for feature parity with Breezy Weather (naming another product to describe yours is fine); the app must not suggest it is an official Breezy client.
- Our sky drawings and layout stay our own (the current `SkyBackground`).
- Whether and when to contact the Breezy maintainers is Reuben's decision. They might welcome a linked iOS counterpart, and they know the providers' quirks. A courteous note before W6 avoids surprises either way.

## Constraints and risks

- **Free Apple team**: Weather lives in Device Lab (3 app slots are full). A widget adds one App ID. A standalone public app needs a paid team.
- **Rate limits**: every phone calls providers directly. Fine for us; at community scale, Open-Meteo's non-commercial limits and MET Norway's User-Agent rules matter, and caching must be strict.
- **Provider churn**: national APIs change. Recorded fixtures catch parser breaks in CI; a source that breaks is disabled with a message rather than failing the page.
- **Swift is compiled only on the laptop or macOS CI**, so each phase's SwiftUI needs the laptop check before its phone sitting.
- **Breezy's "no official iOS client" status** was checked only against its README; check again before W6.

## Next step

With approval of the five decisions above, start W1 on a branch of its own.
