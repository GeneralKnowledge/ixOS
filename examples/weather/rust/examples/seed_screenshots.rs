//! Writes a weather cache and saved places for simulator screenshots: "My
//! Location" plus four saved cities, each with a different sky (cloudy,
//! showers on the way, sunny, snow, a clear night). Times are relative to now
//! and each place's clock is pinned to a time of day, so every run looks the
//! same. macOS CI puts the files in the simulator app's container and
//! launches the app with `-WeatherOfflineDemo YES` so it shows them without
//! asking for location or touching the network
//! (scripts/weather-screenshots.sh).
//!
//! cargo run --manifest-path rust/Cargo.toml --example seed_screenshots -- <dir>

use std::time::{SystemTime, UNIX_EPOCH};

use forge_ffi::{
    Coordinate, CurrentConditions, DailyForecast, Freshness, HourlyPoint, Intensity, Place,
    PlaceSource, WeatherCondition, WeatherReport, Wind,
};

struct Seed {
    name: &'static str,
    country: &'static str,
    latitude: f64,
    longitude: f64,
    device: bool,
    /// Local time of day the place's clock shows, in minutes.
    local_minutes: i64,
    /// Temperature at mid-afternoon today, and the overnight dip.
    warm: f64,
    dip: f64,
    /// Condition for hour `h` from now.
    hour_condition: fn(usize) -> WeatherCondition,
    /// Condition and rain chance for day `d` from today.
    day: fn(usize) -> (WeatherCondition, Option<u8>),
    gusts_kmh: f64,
}

const RAIN: WeatherCondition = WeatherCondition::Rain {
    intensity: Intensity::Light,
    freezing: false,
};
const SHOWERS: WeatherCondition = WeatherCondition::RainShowers {
    intensity: Intensity::Moderate,
};
const SNOW: WeatherCondition = WeatherCondition::Snow {
    intensity: Intensity::Light,
};

const SEEDS: &[Seed] = &[
    Seed {
        name: "Bathgate",
        country: "United Kingdom",
        latitude: 55.9024,
        longitude: -3.6431,
        device: true,
        local_minutes: 10 * 60 + 12,
        warm: 9.0,
        dip: 4.0,
        hour_condition: |h| match h {
            3 => RAIN,
            _ => WeatherCondition::Overcast,
        },
        day: |d| match d {
            1 | 5 => (RAIN, Some(60)),
            3 => (WeatherCondition::PartlyCloudy, Some(10)),
            _ => (WeatherCondition::Overcast, Some(20)),
        },
        gusts_kmh: 18.0,
    },
    Seed {
        name: "Düsseldorf",
        country: "Germany",
        latitude: 51.2217,
        longitude: 6.7762,
        device: false,
        local_minutes: 13 * 60 + 5,
        warm: 10.0,
        dip: 7.0,
        hour_condition: |h| match h {
            0..=2 => WeatherCondition::PartlyCloudy,
            3..=5 => SHOWERS,
            _ => WeatherCondition::Overcast,
        },
        day: |d| match d {
            0 | 4 => (SHOWERS, Some(85)),
            2 => (WeatherCondition::Clear, Some(0)),
            _ => (WeatherCondition::PartlyCloudy, Some(15)),
        },
        gusts_kmh: 26.0,
    },
    Seed {
        name: "Prague",
        country: "Czechia",
        latitude: 50.0880,
        longitude: 14.4208,
        device: false,
        local_minutes: 14 * 60 + 20,
        warm: 8.0,
        dip: 9.0,
        hour_condition: |_| WeatherCondition::Clear,
        day: |d| match d {
            3 => (RAIN, Some(65)),
            1 | 6 => (WeatherCondition::Overcast, Some(5)),
            _ => (WeatherCondition::MainlyClear, Some(0)),
        },
        gusts_kmh: 16.0,
    },
    Seed {
        name: "Tromsø",
        country: "Norway",
        latitude: 69.6492,
        longitude: 18.9553,
        device: false,
        local_minutes: 11 * 60 + 40,
        warm: -2.0,
        dip: 4.0,
        hour_condition: |h| {
            if h < 8 {
                SNOW
            } else {
                WeatherCondition::Overcast
            }
        },
        day: |d| match d {
            0 | 1 | 7 => (SNOW, Some(80)),
            _ => (WeatherCondition::Overcast, Some(30)),
        },
        gusts_kmh: 35.0,
    },
    Seed {
        name: "Kyoto",
        country: "Japan",
        latitude: 35.0116,
        longitude: 135.7681,
        device: false,
        local_minutes: 22 * 60 + 30,
        warm: 17.0,
        dip: 8.0,
        hour_condition: |h| {
            if h < 10 {
                WeatherCondition::Clear
            } else {
                WeatherCondition::MainlyClear
            }
        },
        day: |d| match d {
            2 => (SHOWERS, Some(70)),
            _ => (WeatherCondition::MainlyClear, Some(5)),
        },
        gusts_kmh: 12.0,
    },
];

const SUNRISE: &str = "07:40";
const SUNSET: &str = "16:50";

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: seed_screenshots <dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let reports: Vec<WeatherReport> = SEEDS
        .iter()
        .enumerate()
        .map(|(i, seed)| report(seed, now - 60 * (i as i64 + 1)))
        .collect();
    let places: Vec<&Place> = reports
        .iter()
        .map(|r| &r.place)
        .filter(|p| !matches!(p.source, PlaceSource::Device { .. }))
        .collect();

    let cache = serde_json::json!({ "schema_version": 2, "reports": reports });
    let saved = serde_json::json!({ "schema_version": 1, "places": places });
    let dir = std::path::Path::new(&dir);
    std::fs::write(dir.join("forge-weather-cache-v1.json"), cache.to_string()).unwrap();
    std::fs::write(dir.join("forge-weather-places-v1.json"), saved.to_string()).unwrap();
    for r in &reports {
        let o = forge_ffi::weather_overview(r.clone(), now, forge_ffi::WindUnit::MilesPerHour);
        println!(
            "[seed] {}: {:?} day={} {} | {} hours, {} days",
            r.place.name,
            o.scene,
            o.is_day,
            o.summary,
            o.timeline.len(),
            o.days.len()
        );
    }
    println!(
        "[seed] {} reports, {} saved places in {}",
        reports.len(),
        places.len(),
        dir.display()
    );
}

fn report(seed: &Seed, fetched_at: i64) -> WeatherReport {
    // Pin the place's clock: offset = wanted local time - UTC time of day.
    let utc_minutes = fetched_at.rem_euclid(86_400) / 60;
    let mut offset_minutes = seed.local_minutes - utc_minutes;
    if offset_minutes > 14 * 60 {
        offset_minutes -= 24 * 60;
    } else if offset_minutes < -12 * 60 {
        offset_minutes += 24 * 60;
    }
    let local = fetched_at + offset_minutes * 60;
    let local_hour_start = local - local.rem_euclid(3_600);

    let temperature_at = |t: i64| -> f64 {
        // Warmest at 15:00, coolest at 03:00.
        let hour = (t.rem_euclid(86_400) as f64) / 3_600.0;
        let phase = (hour - 15.0) / 24.0 * std::f64::consts::TAU;
        seed.warm - seed.dip * (1.0 - phase.cos()) / 2.0
    };

    let hourly: Vec<HourlyPoint> = (0..48)
        .map(|h| {
            let t = local_hour_start + h as i64 * 3_600;
            let time = iso(t);
            let condition = (seed.hour_condition)(h);
            let wet = matches!(
                condition,
                WeatherCondition::Rain { .. }
                    | WeatherCondition::RainShowers { .. }
                    | WeatherCondition::Snow { .. }
            );
            let clock = &time[11..16];
            HourlyPoint {
                is_day: SUNRISE <= half_past(clock).as_str() && half_past(clock).as_str() < SUNSET,
                time,
                temperature_c: (temperature_at(t) * 10.0).round() / 10.0,
                precipitation_probability_pct: Some(if wet { 70 } else { 5 }),
                condition,
                wind_gusts_kmh: Some(seed.gusts_kmh - (h % 5) as f64),
            }
        })
        .collect();

    let today_start = local - local.rem_euclid(86_400);
    let daily: Vec<DailyForecast> = (0..10)
        .map(|d| {
            let date = &iso(today_start + d as i64 * 86_400)[..10];
            let (condition, chance) = (seed.day)(d);
            let swing = [0.0, 1.0, -1.0, 2.0, -2.0, 0.5, 1.5, -0.5, 1.0, 0.0][d];
            DailyForecast {
                date: date.to_string(),
                condition,
                high_c: seed.warm + swing,
                low_c: seed.warm - seed.dip + swing * 0.6 - 1.0,
                precipitation_probability_max_pct: chance,
                precipitation_sum_mm: Some(if chance.unwrap_or(0) >= 50 { 3.4 } else { 0.0 }),
                sunrise: Some(format!("{date}T{SUNRISE}")),
                sunset: Some(format!("{date}T{SUNSET}")),
                uv_index_max: Some(if seed.warm > 12.0 { 4.0 } else { 1.0 }),
            }
        })
        .collect();

    let first = &hourly[0];
    WeatherReport {
        place: Place {
            name: seed.name.into(),
            region: None,
            country: Some(seed.country.into()),
            country_code: None,
            coordinate: Coordinate {
                latitude: seed.latitude,
                longitude: seed.longitude,
            },
            timezone: None,
            population: None,
            source: if seed.device {
                PlaceSource::Device {
                    horizontal_accuracy_m: Some(65.0),
                }
            } else {
                PlaceSource::Search { geocoding_id: None }
            },
        },
        grid_coordinate: Coordinate {
            latitude: seed.latitude,
            longitude: seed.longitude,
        },
        elevation_m: Some(120.0),
        timezone: "UTC".into(),
        timezone_abbreviation: None,
        utc_offset_seconds: (offset_minutes * 60) as i32,
        current: CurrentConditions {
            observed_at: iso(local),
            temperature_c: (temperature_at(local) * 10.0).round() / 10.0,
            apparent_temperature_c: Some(temperature_at(local) - 3.0),
            relative_humidity_pct: Some(82),
            precipitation_mm: Some(0.0),
            wind: Some(Wind {
                speed_kmh: seed.gusts_kmh * 0.55,
                gusts_kmh: Some(seed.gusts_kmh),
                direction_deg: Some(250),
            }),
            condition: first.condition,
            is_day: first.is_day,
        },
        hourly,
        daily,
        fetched_at_unix: fetched_at,
        freshness: Freshness::Live,
    }
}

fn half_past(clock: &str) -> String {
    format!("{}:30", &clock[..2])
}

/// "YYYY-MM-DDTHH:MM" for seconds since the epoch (already shifted to local).
fn iso(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}
