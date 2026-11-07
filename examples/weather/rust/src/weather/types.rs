//! Records and enums the Weather example sends across the FFI boundary.
//!
//! The payload is deliberately nested: a [`WeatherReport`] holds records,
//! optional records, lists of records, and enums whose variants carry data
//! (some of which hold further enums). That exercises UniFFI's type mapping far
//! harder than the Counter's single `i64`.
//!
//! Units are fixed (Celsius, km/h, millimetres) and encoded in field names;
//! hosts convert for display.

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
    /// Today and the following nine days.
    pub daily: Vec<DailyForecast>,
    /// Unix seconds when the data was fetched.
    pub fetched_at_unix: i64,
    pub freshness: Freshness,
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

/// How the summary sentence states wind speeds.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindUnit {
    KilometresPerHour,
    MilesPerHour,
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
}
