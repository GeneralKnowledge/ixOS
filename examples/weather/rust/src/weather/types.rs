//! Records and enums the Weather example sends across the FFI boundary.
//!
//! The payload is deliberately nested: a [`WeatherReport`] holds records,
//! optional records, lists of records, and enums whose variants carry data
//! (some of which hold further enums). That exercises UniFFI's type mapping far
//! harder than the Counter's single `i64`.
//!
//! Stored units are fixed (Celsius, km/h, millimetres, hectopascals, metres,
//! µg/m³, grains/m³) and encoded in field names; the person's display units
//! are [`UnitSettings`], applied by [`crate::format_quantity`].

use serde::{Deserialize, Serialize};

/// A point on Earth in WGS84 degrees.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

/// Where a [`Place`] came from.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaceSource {
    /// The device's location services. `horizontal_accuracy_m` is the radius
    /// of uncertainty the platform reported, when it reported one.
    Device { horizontal_accuracy_m: Option<f64> },
    /// A result of [`crate::ForgeWeatherApp::search_places`].
    /// `geocoding_id` is the provider's stable id for the place.
    Search { geocoding_id: Option<u64> },
    /// Typed in or hard-coded by the host.
    Manual,
}

/// A named location to fetch weather for.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// Display name, e.g. "Paris" or "Current location".
    pub name: String,
    /// First-level administrative area (state, region), if known.
    pub region: Option<String>,
    pub country: Option<String>,
    /// ISO 3166-1 alpha-2, e.g. "FR".
    pub country_code: Option<String>,
    pub coordinate: Coordinate,
    /// IANA time zone, e.g. "Europe/Paris", if known.
    pub timezone: Option<String>,
    pub population: Option<u64>,
    pub source: PlaceSource,
}

/// Rain, drizzle and snow strength as WMO weather codes report it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intensity {
    Light,
    Moderate,
    Heavy,
}

/// Sky and precipitation, decoded from a WMO weather interpretation code
/// (WW). Codes this build does not know arrive as `Unknown { code }` rather
/// than failing the whole report.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherCondition {
    Clear,
    MainlyClear,
    PartlyCloudy,
    Overcast,
    Fog {
        depositing_rime: bool,
    },
    Drizzle {
        intensity: Intensity,
        freezing: bool,
    },
    Rain {
        intensity: Intensity,
        freezing: bool,
    },
    Snow {
        intensity: Intensity,
    },
    SnowGrains,
    RainShowers {
        intensity: Intensity,
    },
    SnowShowers {
        intensity: Intensity,
    },
    /// `hail` is `None` for a thunderstorm without hail.
    Thunderstorm {
        hail: Option<Intensity>,
    },
    Unknown {
        code: u16,
    },
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wind {
    pub speed_kmh: f64,
    pub gusts_kmh: Option<f64>,
    /// Direction the wind comes *from*, degrees clockwise from north.
    pub direction_deg: Option<u16>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentConditions {
    /// Local time at the place, ISO 8601 without offset ("2026-11-04T19:30").
    pub observed_at: String,
    pub temperature_c: f64,
    pub apparent_temperature_c: Option<f64>,
    pub relative_humidity_pct: Option<u8>,
    pub precipitation_mm: Option<f64>,
    pub wind: Option<Wind>,
    pub condition: WeatherCondition,
    pub is_day: bool,
    #[serde(default)]
    pub dew_point_c: Option<f64>,
    /// Pressure reduced to mean sea level.
    #[serde(default)]
    pub pressure_msl_hpa: Option<f64>,
    #[serde(default)]
    pub visibility_m: Option<f64>,
    #[serde(default)]
    pub cloud_cover_pct: Option<u8>,
    #[serde(default)]
    pub uv_index: Option<f64>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HourlyPoint {
    /// Local time, ISO 8601 without offset.
    pub time: String,
    pub temperature_c: f64,
    pub precipitation_probability_pct: Option<u8>,
    pub condition: WeatherCondition,
    /// Between that day's sunrise and sunset (true when they are unknown).
    #[serde(default = "default_true")]
    pub is_day: bool,
    #[serde(default)]
    pub wind_gusts_kmh: Option<f64>,
    /// Pressure reduced to mean sea level.
    #[serde(default)]
    pub pressure_msl_hpa: Option<f64>,
    #[serde(default)]
    pub uv_index: Option<f64>,
}

fn default_true() -> bool {
    true
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyForecast {
    /// Local date, "YYYY-MM-DD".
    pub date: String,
    pub condition: WeatherCondition,
    pub high_c: f64,
    pub low_c: f64,
    pub precipitation_probability_max_pct: Option<u8>,
    pub precipitation_sum_mm: Option<f64>,
    /// Local times, ISO 8601 without offset. `None` in polar day or night.
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
    pub uv_index_max: Option<f64>,
}

/// Why a refresh fell back to cached data.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FetchProblem {
    Offline,
    TimedOut,
    ServerError { status: u16 },
    BadResponse,
}

/// How current a [`WeatherReport`] is.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Freshness {
    /// Fetched by the refresh that returned it.
    Live,
    /// Loaded from the on-device cache. `problem` says why the refresh could
    /// not fetch new data; it is `None` when the host asked for the cache
    /// directly (`cached_report`).
    Cached {
        age_seconds: u64,
        problem: Option<FetchProblem>,
    },
}

/// Who provided a part of a report, shown next to it. Most open data asks
/// for this credit (CC BY 4.0, Copernicus).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCredit {
    /// Stable id, e.g. "open-meteo".
    pub id: String,
    /// "Open-Meteo".
    pub name: String,
    /// The line to show, e.g. "Weather data by Open-Meteo.com (CC BY 4.0)".
    pub credit: String,
    pub url: String,
}

/// The state of one optional part of a report (air quality, pollen). Each
/// part is fetched from its own source and fails or ages on its own, so a
/// failure never takes the rest of the report with it.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SectionState {
    /// Fetched by the refresh that returned the report.
    Live,
    /// Kept from an earlier refresh. `problem` is why this refresh could not
    /// fetch it, `None` when nothing was asked (a cached report).
    Cached {
        age_seconds: u64,
        problem: Option<FetchProblem>,
    },
    /// The source answered, and has no data for this place right now. Not
    /// an error, and never filled in. For pollen this covers both "outside
    /// the model's area" and "nothing forecast now" (the source sends the
    /// same empty answer); `PollenOverview::gap` tells them apart.
    NotAvailableHere,
    /// There is no data to show: the fetch failed with `problem` and nothing
    /// usable was kept, or (`None`) it was never fetched or what was fetched
    /// no longer reaches the present. Never the same as "nothing to report".
    Unavailable { problem: Option<FetchProblem> },
}

/// One hour of air quality. Concentrations in µg/m³; each value is `None`
/// when the source has none for that hour.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AirQualityHour {
    /// Local time, ISO 8601 without offset.
    pub time: String,
    /// European Air Quality Index (0 = clean, over 100 = extremely poor).
    pub european_aqi: Option<u16>,
    /// US EPA Air Quality Index (0 to 500).
    pub us_aqi: Option<u16>,
    pub pm2_5: Option<f64>,
    pub pm10: Option<f64>,
    pub ozone: Option<f64>,
    pub nitrogen_dioxide: Option<f64>,
    pub sulphur_dioxide: Option<f64>,
    pub carbon_monoxide: Option<f64>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AirQualitySection {
    pub state: SectionState,
    pub source: Option<SourceCredit>,
    /// Unix seconds when the hours were fetched.
    pub fetched_at_unix: Option<i64>,
    /// As far ahead as the source forecasts (about 5 to 7 days), which is
    /// shorter than the weather forecast.
    pub hours: Vec<AirQualityHour>,
}

/// Pollen species the sources report.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PollenSpecies {
    Alder,
    Birch,
    Grass,
    Mugwort,
    Olive,
    Ragweed,
}

/// One hour of pollen, grains per cubic metre. A species the source has no
/// data for at this place is left out, never reported as zero.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollenHour {
    pub time: String,
    pub readings: Vec<PollenReading>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollenReading {
    pub species: PollenSpecies,
    pub grains_per_m3: f64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollenSection {
    pub state: SectionState,
    pub source: Option<SourceCredit>,
    pub fetched_at_unix: Option<i64>,
    pub hours: Vec<PollenHour>,
}

impl AirQualitySection {
    pub(crate) fn never_fetched() -> Self {
        Self {
            state: SectionState::Unavailable { problem: None },
            source: None,
            fetched_at_unix: None,
            hours: Vec::new(),
        }
    }
}

impl Default for AirQualitySection {
    fn default() -> Self {
        Self::never_fetched()
    }
}

impl PollenSection {
    pub(crate) fn never_fetched() -> Self {
        Self {
            state: SectionState::Unavailable { problem: None },
            source: None,
            fetched_at_unix: None,
            hours: Vec::new(),
        }
    }
}

impl Default for PollenSection {
    fn default() -> Self {
        Self::never_fetched()
    }
}

/// Why saved data (the saved places) could not be read. It is left
/// exactly as it is.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedDataProblem {
    /// The file is damaged. The person can start again; the old file is
    /// kept, renamed, next to the new one.
    Unreadable,
    /// A newer version of the app wrote it. Updating the app opens it;
    /// nothing is ever written over it.
    FromNewerVersion,
    /// The file could not be read or written (storage full, permissions).
    /// Trying again later may work.
    CouldNotOpen,
}

/// The full payload of one refresh.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherReport {
    pub place: Place,
    /// Where the forecast model actually sampled (snapped to its grid).
    pub grid_coordinate: Coordinate,
    pub elevation_m: Option<f64>,
    /// IANA zone all local times in the report use.
    pub timezone: String,
    pub timezone_abbreviation: Option<String>,
    pub utc_offset_seconds: i32,
    pub current: CurrentConditions,
    /// The next 48 hours, starting with the current hour.
    pub hourly: Vec<HourlyPoint>,
    /// Today and up to the following 15 days, as far as the source goes.
    pub daily: Vec<DailyForecast>,
    /// Unix seconds when the forecast was fetched.
    pub fetched_at_unix: i64,
    /// How current the forecast (current, hourly, daily) is. Air quality
    /// and pollen carry their own state.
    pub freshness: Freshness,
    /// Who provided the forecast. Reports cached before sources were named
    /// came from Open-Meteo.
    #[serde(default = "crate::weather::sources::open_meteo_credit")]
    pub forecast_source: SourceCredit,
    #[serde(default)]
    pub air_quality: AirQualitySection,
    #[serde(default)]
    pub pollen: PollenSection,
}

/// Where a refresh is. Reported by `WeatherOperation::phase`.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshPhase {
    /// Waiting for the HTTP response.
    Fetching,
    /// Parsing and mapping the response.
    Decoding,
    /// `result()` is ready (success, failure or cancellation).
    Finished,
}

/// Optional overrides for [`crate::ForgeWeatherApp::with_config`]. Every
/// `None` takes the default.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Eq)]
pub struct WeatherConfig {
    /// Default `https://api.open-meteo.com`.
    #[uniffi(default)]
    pub forecast_base_url: Option<String>,
    /// Default `https://geocoding-api.open-meteo.com`.
    #[uniffi(default)]
    pub geocoding_base_url: Option<String>,
    /// Default `https://air-quality-api.open-meteo.com` (air quality and pollen).
    #[uniffi(default)]
    pub air_quality_base_url: Option<String>,
    /// Per-request timeout. Default 15 s.
    #[uniffi(default)]
    pub request_timeout_ms: Option<u32>,
    /// A cached report older than this is not used as a fallback. Default 24 h.
    #[uniffi(default)]
    pub max_cache_age_seconds: Option<u64>,
}

/// The kind of sky to paint behind a report. Hosts pair it with
/// [`WeatherOverview::is_day`] to pick a day or night palette.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkyScene {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Thunderstorm,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindUnit {
    KilometresPerHour,
    MilesPerHour,
    MetresPerSecond,
    Knots,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrecipitationUnit {
    Millimetres,
    Inches,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PressureUnit {
    Hectopascals,
    InchesOfMercury,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceUnit {
    Kilometres,
    Miles,
}

/// Which air quality index the page shows.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirQualityScale {
    European,
    UnitedStates,
}

/// The person's display units. Reports always store metric values; these
/// apply only when showing them, everywhere through [`crate::format_quantity`].
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitSettings {
    pub temperature: TemperatureUnit,
    pub wind: WindUnit,
    pub precipitation: PrecipitationUnit,
    pub pressure: PressureUnit,
    pub distance: DistanceUnit,
    pub air_quality: AirQualityScale,
}

/// A stored value to show in the person's units.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq)]
pub enum Quantity {
    /// "12°" (the unit letter is left out, as weather apps do).
    Temperature {
        celsius: f64,
    },
    Speed {
        kmh: f64,
    },
    Precipitation {
        mm: f64,
    },
    Pressure {
        hpa: f64,
    },
    Distance {
        metres: f64,
    },
}

/// An air quality index value placed in its band.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AqiReading {
    pub value: u16,
    pub scale: AirQualityScale,
    /// 1 (best) to 6 (worst), for colour.
    pub level: u8,
    /// The band's official name, e.g. "Fair" or "Unhealthy for sensitive groups".
    pub label: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AirQualityDay {
    pub date: String,
    /// The day's worst hour.
    pub worst: AqiReading,
}

/// Air quality on the page: now, the days the source covers, and the state.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AirQualityOverview {
    pub state: SectionState,
    pub source: Option<SourceCredit>,
    /// This hour's index on the chosen scale, if the source has it.
    pub index: Option<AqiReading>,
    /// This hour's concentrations.
    pub now: Option<AirQualityHour>,
    /// Where this hour's index sits on the scale's six bands, 0 to 1, for
    /// a marker on a colour bar.
    pub scale_position: Option<f64>,
    /// "Similar to yesterday at this time." when the source has yesterday's
    /// reading for this hour.
    pub comparison: Option<String>,
    /// Today onward, only days with data. Shorter than the weather forecast.
    pub days: Vec<AirQualityDay>,
    /// The last local hour the source covers, ISO 8601 without offset.
    pub covers_until: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PollenSpeciesNow {
    pub species: PollenSpecies,
    pub now_grains_per_m3: f64,
    /// The highest hour still ahead today (including now).
    pub today_max_grains_per_m3: f64,
}

/// Why the pollen source had nothing for a place. The source sends the
/// same empty answer for both, so this is told apart by where the place is.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollenGap {
    /// The place is outside the area the pollen model covers (Europe).
    OutsideModelArea,
    /// The place is inside it, and the source has no pollen forecast right
    /// now. Its documentation says pollen is forecast during the pollen
    /// season, but the empty answer itself doesn't say why, so the page
    /// words this neutrally.
    NothingForecastNow,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PollenOverview {
    pub state: SectionState,
    /// Set when `state` is `NotAvailableHere`.
    pub gap: Option<PollenGap>,
    pub source: Option<SourceCredit>,
    /// Species with pollen in the air today, highest first. Species the
    /// source reports as zero all day are left out.
    pub species: Vec<PollenSpeciesNow>,
    /// One line for the card: "No pollen in the air right now." or
    /// "Birch is highest today, up to 12 grains per cubic metre." Empty
    /// when the state has no data.
    pub summary: String,
    pub covers_until: Option<String>,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoonPhaseName {
    NewMoon,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
    FullMoon,
    WaningGibbous,
    LastQuarter,
    WaningCrescent,
}

/// The moon now, computed (no source needed).
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct MoonPhase {
    pub name: MoonPhaseName,
    /// Lit fraction of the disc, 0 to 100.
    pub illumination_pct: u8,
    /// Through the cycle: 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter.
    pub cycle_fraction: f64,
}

/// One slot of the hourly strip: an hour, or a sunrise or sunset between
/// two hours.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum TimelineEntry {
    /// `is_now` marks the first slot, the current hour.
    Hour {
        point: HourlyPoint,
        is_now: bool,
    },
    /// Local time, ISO 8601 without offset.
    Sunrise {
        time: String,
    },
    Sunset {
        time: String,
    },
}

/// What the screen shows for a report, worked out for a given moment by
/// [`crate::weather_overview`]. A cached report gets the same treatment:
/// hours and days already past at the place are left out.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct WeatherOverview {
    pub scene: SkyScene,
    pub is_day: bool,
    /// "Cloudy conditions will continue for the rest of the day. Wind gusts
    /// are up to 18 km/h." Empty when there is nothing worth saying.
    pub summary: String,
    /// The place's local date, "YYYY-MM-DD".
    pub local_date: String,
    /// Today's forecast at the place, when the report still covers it.
    pub today: Option<DailyForecast>,
    /// Up to 24 hours from the current hour, with sunrises and sunsets in
    /// between. Empty when the report is too old to cover the current hour.
    pub timeline: Vec<TimelineEntry>,
    /// Today onward.
    pub days: Vec<DailyForecast>,
    /// Lowest low and highest high across `days`, so every row's
    /// temperature bar shares one scale.
    pub range_low_c: f64,
    pub range_high_c: f64,
    pub air_quality: AirQualityOverview,
    pub pollen: PollenOverview,
    pub moon: MoonPhase,
    /// Today's daylight, when sunrise and sunset are known.
    pub daylight_minutes: Option<u32>,
    /// Every source on the page, forecast first, once each.
    pub credits: Vec<SourceCredit>,
    /// The detail cards: each value with its one-line explanation.
    pub details: WeatherDetails,
}

/// WHO UV index bands.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvLevel {
    Low,
    Moderate,
    High,
    VeryHigh,
    Extreme,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureTrend {
    Rising,
    Steady,
    Falling,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FeelsLikeDetail {
    pub celsius: f64,
    /// "Wind makes it feel colder."
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct WindDetail {
    pub speed_kmh: f64,
    pub gusts_kmh: Option<f64>,
    /// Where the wind comes from, degrees clockwise from north.
    pub direction_deg: Option<u16>,
    /// "60° ENE".
    pub direction: Option<String>,
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct HumidityDetail {
    pub percent: u8,
    pub dew_point_c: Option<f64>,
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PrecipitationDetail {
    pub today_mm: f64,
    /// "Next expected is 4 mm on Tue."
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct UvDetail {
    /// Now, or today's peak when the source has no reading for now.
    pub index: f64,
    pub level: UvLevel,
    /// "Low", "Very high".
    pub label: String,
    /// 0 to 1 along the 0 to 11+ scale, for a marker.
    pub scale_position: f64,
    /// "Low for the rest of the day."
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct VisibilityDetail {
    pub metres: f64,
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PressureDetail {
    pub hpa: f64,
    /// Over the next three hours, when the forecast has hourly pressure.
    pub trend: Option<PressureTrend>,
    /// 0 to 1 between 960 and 1060 hPa, for a dial.
    pub scale_position: f64,
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SunDetail {
    /// Today's, local "HH:MM". `None` in polar day or night.
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
    /// True when the next of the two is the sunset (the sun is up).
    pub next_is_sunset: bool,
    /// How far the sun is through today's daylight, 0 to 1; `None` while
    /// it is down.
    pub progress: Option<f64>,
    /// "Sunrise: 08:04." or "Sunset: 15:57." (the other of the two), with
    /// the day's length.
    pub note: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MoonDetail {
    pub phase: MoonPhase,
    /// Local "HH:MM" today, `None` on a day the moon doesn't rise or set.
    pub moonrise: Option<String>,
    pub moonset: Option<String>,
    /// Local days until the next full moon (0 = today).
    pub days_to_full_moon: u32,
    /// "Next full moon in 3 days."
    pub note: String,
}

/// Every detail card the report has data for; a card whose value the
/// source didn't send is `None` and not shown.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct WeatherDetails {
    pub feels_like: Option<FeelsLikeDetail>,
    pub wind: Option<WindDetail>,
    pub humidity: Option<HumidityDetail>,
    pub precipitation: Option<PrecipitationDetail>,
    pub uv: Option<UvDetail>,
    pub visibility: Option<VisibilityDetail>,
    pub pressure: Option<PressureDetail>,
    pub sun: Option<SunDetail>,
    pub moon: MoonDetail,
}
