//! Open-Meteo (https://open-meteo.com) request building and response decoding.
//! No API key is needed. Everything here is pure, so it is tested with
//! recorded JSON and no network.

use serde::Deserialize;

use super::error::WeatherError;
use super::types::{
    Coordinate, CurrentConditions, DailyForecast, Freshness, HourlyPoint, Intensity, Place,
    PlaceSource, WeatherCondition, WeatherReport, Wind,
};

pub(crate) const DEFAULT_FORECAST_BASE: &str = "https://api.open-meteo.com";
pub(crate) const DEFAULT_GEOCODING_BASE: &str = "https://geocoding-api.open-meteo.com";

pub(crate) const HOURLY_POINTS: usize = 24;
pub(crate) const DAILY_POINTS: usize = 7;
pub(crate) const MAX_SEARCH_RESULTS: u32 = 10;
pub(crate) const MAX_QUERY_CHARS: usize = 100;

const CURRENT_FIELDS: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,weather_code,is_day,wind_speed_10m,wind_direction_10m,wind_gusts_10m";
const HOURLY_FIELDS: &str = "temperature_2m,precipitation_probability,weather_code";
const DAILY_FIELDS: &str = "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum,precipitation_probability_max,sunrise,sunset,uv_index_max";

pub(crate) fn validate_coordinate(c: Coordinate) -> Result<(), WeatherError> {
    if !c.latitude.is_finite() || !(-90.0..=90.0).contains(&c.latitude) {
        return Err(WeatherError::invalid(format!(
            "latitude {} is outside -90...90",
            c.latitude
        )));
    }
    if !c.longitude.is_finite() || !(-180.0..=180.0).contains(&c.longitude) {
        return Err(WeatherError::invalid(format!(
            "longitude {} is outside -180...180",
            c.longitude
        )));
    }
    Ok(())
}

pub(crate) fn forecast_url(base: &str, c: Coordinate) -> String {
    format!(
        "{}/v1/forecast?latitude={:.4}&longitude={:.4}&current={CURRENT_FIELDS}&hourly={HOURLY_FIELDS}&daily={DAILY_FIELDS}&timezone=auto&forecast_days={DAILY_POINTS}&forecast_hours={HOURLY_POINTS}&wind_speed_unit=kmh&temperature_unit=celsius&precipitation_unit=mm&timeformat=iso8601",
        base.trim_end_matches('/'),
        c.latitude,
        c.longitude,
    )
}

/// Returns the trimmed query, or why it cannot be searched.
pub(crate) fn validate_query(query: &str) -> Result<&str, WeatherError> {
    let q = query.trim();
    if q.chars().count() < 2 {
        return Err(WeatherError::invalid("search needs at least 2 characters"));
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(WeatherError::invalid(format!(
            "search is limited to {MAX_QUERY_CHARS} characters"
        )));
    }
    Ok(q)
}

pub(crate) fn geocoding_url(base: &str, query: &str) -> String {
    format!(
        "{}/v1/search?name={}&count={MAX_SEARCH_RESULTS}&language=en&format=json",
        base.trim_end_matches('/'),
        percent_encode(query),
    )
}

/// RFC 3986 percent-encoding of everything but unreserved characters.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Maps a non-2xx response to an error, using Open-Meteo's
/// `{"error": true, "reason": "..."}` body when present.
pub(crate) fn status_error(status: u16, body: &[u8]) -> WeatherError {
    #[derive(Deserialize)]
    struct ApiError {
        reason: Option<String>,
    }
    let reason = serde_json::from_slice::<ApiError>(body)
        .ok()
        .and_then(|e| e.reason);
    WeatherError::Server { status, reason }
}

// ---- Forecast ----

#[derive(Deserialize)]
struct ForecastResponse {
    latitude: f64,
    longitude: f64,
    elevation: Option<f64>,
    timezone: String,
    timezone_abbreviation: Option<String>,
    utc_offset_seconds: i32,
    current: CurrentJson,
    hourly: HourlyJson,
    daily: DailyJson,
}

#[derive(Deserialize)]
struct CurrentJson {
    time: String,
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    precipitation: Option<f64>,
    weather_code: Option<f64>,
    is_day: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    wind_gusts_10m: Option<f64>,
}

#[derive(Deserialize)]
struct HourlyJson {
    time: Vec<String>,
    temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
    weather_code: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct DailyJson {
    time: Vec<String>,
    weather_code: Vec<Option<f64>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_sum: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
    #[serde(default)]
    sunrise: Vec<Option<String>>,
    #[serde(default)]
    sunset: Vec<Option<String>>,
    #[serde(default)]
    uv_index_max: Vec<Option<f64>>,
}

/// `values[i]`, flattening "missing" and "null" into `None`.
fn at<T: Clone>(values: &[Option<T>], i: usize) -> Option<T> {
    values.get(i).cloned().flatten()
}

fn percent(v: Option<f64>) -> Option<u8> {
    v.filter(|v| v.is_finite())
        .map(|v| v.round().clamp(0.0, 100.0) as u8)
}

fn condition(code: Option<f64>) -> WeatherCondition {
    match code {
        Some(c) if c.is_finite() && (0.0..=u16::MAX as f64).contains(&c) => wmo_condition(c as u16),
        // Missing code: say so rather than inventing clear skies.
        _ => WeatherCondition::Unknown { code: u16::MAX },
    }
}

/// WMO weather interpretation codes, as documented by Open-Meteo.
pub(crate) fn wmo_condition(code: u16) -> WeatherCondition {
    use Intensity::{Heavy, Light, Moderate};
    use WeatherCondition as W;
    match code {
        0 => W::Clear,
        1 => W::MainlyClear,
        2 => W::PartlyCloudy,
        3 => W::Overcast,
        45 => W::Fog {
            depositing_rime: false,
        },
        48 => W::Fog {
            depositing_rime: true,
        },
        51 => W::Drizzle {
            intensity: Light,
            freezing: false,
        },
        53 => W::Drizzle {
            intensity: Moderate,
            freezing: false,
        },
        55 => W::Drizzle {
            intensity: Heavy,
            freezing: false,
        },
        56 => W::Drizzle {
            intensity: Light,
            freezing: true,
        },
        57 => W::Drizzle {
            intensity: Heavy,
            freezing: true,
        },
        61 => W::Rain {
            intensity: Light,
            freezing: false,
        },
        63 => W::Rain {
            intensity: Moderate,
            freezing: false,
        },
        65 => W::Rain {
            intensity: Heavy,
            freezing: false,
        },
        66 => W::Rain {
            intensity: Light,
            freezing: true,
        },
        67 => W::Rain {
            intensity: Heavy,
            freezing: true,
        },
        71 => W::Snow { intensity: Light },
        73 => W::Snow {
            intensity: Moderate,
        },
        75 => W::Snow { intensity: Heavy },
        77 => W::SnowGrains,
        80 => W::RainShowers { intensity: Light },
        81 => W::RainShowers {
            intensity: Moderate,
        },
        82 => W::RainShowers { intensity: Heavy },
        85 => W::SnowShowers { intensity: Light },
        86 => W::SnowShowers { intensity: Heavy },
        95 => W::Thunderstorm { hail: None },
        96 => W::Thunderstorm { hail: Some(Light) },
        99 => W::Thunderstorm { hail: Some(Heavy) },
        other => W::Unknown { code: other },
    }
}

/// Decodes a forecast response into a [`WeatherReport`] for `place`.
pub(crate) fn decode_forecast(
    body: &[u8],
    place: Place,
    fetched_at_unix: i64,
) -> Result<WeatherReport, WeatherError> {
    let r: ForecastResponse = serde_json::from_slice(body).map_err(WeatherError::bad_response)?;

    let c = r.current;
    let temperature_c = c
        .temperature_2m
        .ok_or_else(|| WeatherError::bad_response("current.temperature_2m is missing"))?;
    let wind = c.wind_speed_10m.map(|speed_kmh| Wind {
        speed_kmh,
        gusts_kmh: c.wind_gusts_10m,
        direction_deg: c
            .wind_direction_10m
            .filter(|d| d.is_finite())
            .map(|d| (d.round() as i64).rem_euclid(360) as u16),
    });
    let current = CurrentConditions {
        observed_at: c.time,
        temperature_c,
        apparent_temperature_c: c.apparent_temperature,
        relative_humidity_pct: percent(c.relative_humidity_2m),
        precipitation_mm: c.precipitation,
        wind,
        condition: condition(c.weather_code),
        // Open-Meteo sends 1/0; assume day if it did not say.
        is_day: c.is_day.is_none_or(|d| d >= 0.5),
    };

    let h = r.hourly;
    let hourly = h
        .time
        .iter()
        .enumerate()
        .filter_map(|(i, time)| {
            // An hour without a temperature is not worth a slot.
            Some(HourlyPoint {
                time: time.clone(),
                temperature_c: at(&h.temperature_2m, i)?,
                precipitation_probability_pct: percent(at(&h.precipitation_probability, i)),
                condition: condition(at(&h.weather_code, i)),
            })
        })
        .take(HOURLY_POINTS)
        .collect();

    let d = r.daily;
    let mut daily = Vec::with_capacity(d.time.len().min(DAILY_POINTS));
    for (i, date) in d.time.iter().enumerate().take(DAILY_POINTS) {
        let (Some(high_c), Some(low_c)) =
            (at(&d.temperature_2m_max, i), at(&d.temperature_2m_min, i))
        else {
            continue;
        };
        daily.push(DailyForecast {
            date: date.clone(),
            condition: condition(at(&d.weather_code, i)),
            high_c,
            low_c,
            precipitation_probability_max_pct: percent(at(&d.precipitation_probability_max, i)),
            precipitation_sum_mm: at(&d.precipitation_sum, i),
            sunrise: at(&d.sunrise, i),
            sunset: at(&d.sunset, i),
            uv_index_max: at(&d.uv_index_max, i),
        });
    }

    let mut place = place;
    if place.timezone.is_none() {
        place.timezone = Some(r.timezone.clone());
    }

    Ok(WeatherReport {
        place,
        grid_coordinate: Coordinate {
            latitude: r.latitude,
            longitude: r.longitude,
        },
        elevation_m: r.elevation,
        timezone: r.timezone,
        timezone_abbreviation: r.timezone_abbreviation,
        utc_offset_seconds: r.utc_offset_seconds,
        current,
        hourly,
        daily,
        fetched_at_unix,
        freshness: Freshness::Live,
    })
}

// ---- Geocoding ----

#[derive(Deserialize)]
struct SearchResponse {
    /// Absent (not empty) when nothing matched.
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    id: Option<u64>,
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    country_code: Option<String>,
    admin1: Option<String>,
    timezone: Option<String>,
    population: Option<u64>,
}

pub(crate) fn decode_search(body: &[u8]) -> Result<Vec<Place>, WeatherError> {
    let r: SearchResponse = serde_json::from_slice(body).map_err(WeatherError::bad_response)?;
    Ok(r.results
        .into_iter()
        .filter_map(|s| {
            let coordinate = Coordinate {
                latitude: s.latitude,
                longitude: s.longitude,
            };
            validate_coordinate(coordinate).ok()?;
            Some(Place {
                name: s.name,
                region: s.admin1,
                country: s.country,
                country_code: s.country_code,
                coordinate,
                timezone: s.timezone,
                population: s.population,
                source: PlaceSource::Search { geocoding_id: s.id },
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = include_str!("fixtures/forecast_paris.json");
    const SPARSE: &str = include_str!("fixtures/forecast_sparse.json");
    const SEARCH: &str = include_str!("fixtures/search_paris.json");

    fn paris() -> Place {
        Place {
            name: "Paris".into(),
            region: Some("Île-de-France".into()),
            country: Some("France".into()),
            country_code: Some("FR".into()),
            coordinate: Coordinate {
                latitude: 48.8534,
                longitude: 2.3488,
            },
            timezone: None,
            population: Some(2_138_551),
            source: PlaceSource::Manual,
        }
    }

    #[test]
    fn forecast_url_has_every_field_and_fixed_units() {
        let url = forecast_url(
            "https://api.open-meteo.com/",
            Coordinate {
                latitude: 48.85341,
                longitude: -2.0,
            },
        );
        assert!(url.starts_with(
            "https://api.open-meteo.com/v1/forecast?latitude=48.8534&longitude=-2.0000&"
        ));
        for part in [
            "current=temperature_2m,",
            "hourly=temperature_2m,precipitation_probability,weather_code",
            "daily=weather_code,",
            "timezone=auto",
            "forecast_days=7",
            "forecast_hours=24",
            "wind_speed_unit=kmh",
        ] {
            assert!(url.contains(part), "{part} missing from {url}");
        }
    }

    #[test]
    fn geocoding_url_percent_encodes_the_query() {
        assert_eq!(
            geocoding_url("https://g.example", "São Paulo & co"),
            "https://g.example/v1/search?name=S%C3%A3o%20Paulo%20%26%20co&count=10&language=en&format=json"
        );
    }

    #[test]
    fn queries_are_trimmed_and_bounded() {
        assert_eq!(validate_query("  Paris ").unwrap(), "Paris");
        assert!(validate_query(" P ").is_err());
        assert!(validate_query(&"x".repeat(101)).is_err());
    }

    #[test]
    fn coordinates_are_range_checked() {
        let ok = Coordinate {
            latitude: -90.0,
            longitude: 180.0,
        };
        assert!(validate_coordinate(ok).is_ok());
        for (lat, lon) in [
            (90.1, 0.0),
            (0.0, -180.5),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(validate_coordinate(Coordinate {
                latitude: lat,
                longitude: lon
            })
            .is_err());
        }
    }

    #[test]
    fn decodes_a_full_forecast() {
        let report = decode_forecast(FORECAST.as_bytes(), paris(), 1_793_820_000).unwrap();
        assert_eq!(report.timezone, "Europe/Paris");
        assert_eq!(report.place.timezone.as_deref(), Some("Europe/Paris"));
        assert_eq!(report.timezone_abbreviation.as_deref(), Some("GMT+1"));
        assert_eq!(report.utc_offset_seconds, 3600);
        assert_eq!(report.grid_coordinate.latitude, 48.86);
        assert_eq!(report.freshness, Freshness::Live);

        let c = &report.current;
        assert_eq!(c.observed_at, "2026-11-04T20:30");
        assert_eq!(c.temperature_c, 9.4);
        assert_eq!(c.apparent_temperature_c, Some(6.8));
        assert_eq!(c.relative_humidity_pct, Some(87));
        assert_eq!(
            c.condition,
            WeatherCondition::Drizzle {
                intensity: Intensity::Light,
                freezing: false
            }
        );
        assert!(!c.is_day);
        let wind = c.wind.as_ref().unwrap();
        assert_eq!(wind.speed_kmh, 14.2);
        assert_eq!(wind.gusts_kmh, Some(29.5));
        assert_eq!(wind.direction_deg, Some(0), "360 wraps to 0");

        assert_eq!(report.hourly.len(), 24);
        assert_eq!(report.hourly[0].time, "2026-11-04T20:00");
        assert_eq!(report.hourly[0].precipitation_probability_pct, Some(40));
        assert_eq!(report.hourly[5].condition, WeatherCondition::Overcast);

        assert_eq!(report.daily.len(), 7);
        let today = &report.daily[0];
        assert_eq!(today.date, "2026-11-04");
        assert_eq!((today.high_c, today.low_c), (12.1, 6.3));
        assert_eq!(today.sunrise.as_deref(), Some("2026-11-04T07:50"));
        assert_eq!(
            report.daily[3].condition,
            WeatherCondition::Thunderstorm {
                hail: Some(Intensity::Light)
            }
        );
        assert_eq!(
            report.daily[6].condition,
            WeatherCondition::Unknown { code: 42 }
        );
    }

    #[test]
    fn sparse_forecast_keeps_what_it_can() {
        let report = decode_forecast(SPARSE.as_bytes(), paris(), 0).unwrap();
        let c = &report.current;
        assert_eq!(c.apparent_temperature_c, None);
        assert_eq!(c.relative_humidity_pct, None);
        assert_eq!(c.wind, None);
        assert!(c.is_day, "missing is_day defaults to day");
        assert_eq!(c.condition, WeatherCondition::Unknown { code: u16::MAX });
        // One hour has a null temperature and is dropped.
        assert_eq!(report.hourly.len(), 2);
        assert_eq!(report.hourly[1].precipitation_probability_pct, None);
        // One day has a null high and is dropped; missing arrays are None.
        assert_eq!(report.daily.len(), 1);
        assert_eq!(report.daily[0].sunrise, None);
        assert_eq!(report.daily[0].uv_index_max, None);
        assert_eq!(report.elevation_m, None);
    }

    #[test]
    fn malformed_forecasts_are_bad_responses() {
        for body in [
            &b"not json"[..],
            b"{}",
            br#"{"latitude":1,"longitude":2,"timezone":"UTC","utc_offset_seconds":0,
                "current":{"time":"t"},"hourly":{"time":[],"temperature_2m":[],"weather_code":[]},
                "daily":{"time":[],"weather_code":[],"temperature_2m_max":[],"temperature_2m_min":[]}}"#,
        ] {
            let err = decode_forecast(body, paris(), 0).unwrap_err();
            assert!(matches!(err, WeatherError::BadResponse { .. }), "{err:?}");
        }
    }

    #[test]
    fn every_documented_wmo_code_is_known() {
        for code in [
            0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82,
            85, 86, 95, 96, 99,
        ] {
            assert!(
                !matches!(wmo_condition(code), WeatherCondition::Unknown { .. }),
                "{code}"
            );
        }
        assert_eq!(wmo_condition(4), WeatherCondition::Unknown { code: 4 });
    }

    #[test]
    fn decodes_search_results() {
        let places = decode_search(SEARCH.as_bytes()).unwrap();
        assert_eq!(places.len(), 2, "the out-of-range result is dropped");
        let p = &places[0];
        assert_eq!(p.name, "Paris");
        assert_eq!(p.region.as_deref(), Some("Île-de-France"));
        assert_eq!(p.country_code.as_deref(), Some("FR"));
        assert_eq!(p.population, Some(2_138_551));
        assert_eq!(
            p.source,
            PlaceSource::Search {
                geocoding_id: Some(2_988_507)
            }
        );
        assert_eq!(places[1].region.as_deref(), Some("Texas"));
        assert_eq!(places[1].population, None);
    }

    #[test]
    fn no_matches_is_an_empty_list() {
        assert!(decode_search(br#"{"generationtime_ms":0.5}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn status_errors_carry_the_api_reason() {
        let err = status_error(
            400,
            r#"{"error":true,"reason":"Latitude must be in range of -90 to 90°."}"#.as_bytes(),
        );
        assert_eq!(
            err,
            WeatherError::Server {
                status: 400,
                reason: Some("Latitude must be in range of -90 to 90°.".into())
            }
        );
        assert_eq!(
            status_error(502, b"<html>"),
            WeatherError::Server {
                status: 502,
                reason: None
            }
        );
    }
}
