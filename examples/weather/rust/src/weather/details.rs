//! The detail cards under the forecast: each value with the one line that
//! explains it ("Low for the rest of the day.", "Next expected is 4 mm on
//! Tue."). Worked out here, from the report alone, so the screen only draws
//! them and the sentences are tested.

use super::astro::{moon_phase, moon_rise_set, next_full_moon};
use super::types::{
    DailyForecast, FeelsLikeDetail, HourlyPoint, HumidityDetail, MoonDetail, PrecipitationDetail,
    PressureDetail, PressureTrend, Quantity, SunDetail, UnitSettings, UvDetail, UvLevel,
    VisibilityDetail, WeatherDetails, WeatherReport, WindDetail,
};
use super::units::format_quantity;

/// A day with at least this much counts as "expected" for the rain card.
const NEXT_RAIN_MM: f64 = 1.0;
/// A change of at least this much over [`TREND_HOURS`] is rising or falling.
const TREND_HPA: f64 = 1.0;
const TREND_HOURS: usize = 3;
/// The pressure dial's range.
const PRESSURE_LOW_HPA: f64 = 960.0;
const PRESSURE_HIGH_HPA: f64 = 1060.0;

/// Everything the detail cards show for `report` at `now_unix`. `now` is
/// the local "YYYY-MM-DDTHH:MM", `today` the local day's forecast.
pub(crate) fn details(
    report: &WeatherReport,
    now_unix: i64,
    now: &str,
    hours: &[&HourlyPoint],
    days: &[DailyForecast],
    today: Option<&DailyForecast>,
    units: UnitSettings,
) -> WeatherDetails {
    let c = &report.current;
    let temp = |celsius: f64| format_quantity(Quantity::Temperature { celsius }, units);
    WeatherDetails {
        feels_like: c.apparent_temperature_c.map(|feels| FeelsLikeDetail {
            celsius: feels,
            note: feels_note(
                feels,
                c.temperature_c,
                c.wind.as_ref().is_some_and(|w| w.speed_kmh > 15.0),
            ),
        }),
        wind: c.wind.as_ref().map(|w| WindDetail {
            speed_kmh: w.speed_kmh,
            gusts_kmh: w.gusts_kmh,
            direction_deg: w.direction_deg,
            direction: w.direction_deg.map(|d| format!("{d}° {}", compass(d))),
            note: match (w.gusts_kmh, w.direction_deg) {
                (Some(g), Some(d)) => format!(
                    "Gusts up to {}, from the {}.",
                    format_quantity(Quantity::Speed { kmh: g }, units),
                    compass_name(d)
                ),
                (Some(g), None) => format!(
                    "Gusts up to {}.",
                    format_quantity(Quantity::Speed { kmh: g }, units)
                ),
                (None, Some(d)) => format!("From the {}.", compass_name(d)),
                (None, None) => "Wind speed now.".into(),
            },
        }),
        humidity: c.relative_humidity_pct.map(|percent| HumidityDetail {
            percent,
            dew_point_c: c.dew_point_c,
            note: match c.dew_point_c {
                Some(dew) => format!("The dew point is {} right now.", temp(dew)),
                None if percent >= 85 => "The air is close to saturated.".into(),
                None => "Relative humidity now.".into(),
            },
        }),
        precipitation: today.and_then(|t| t.precipitation_sum_mm).map(|today_mm| {
            PrecipitationDetail {
                today_mm,
                note: next_rain(days, units),
            }
        }),
        uv: uv(c.uv_index, hours, today, now),
        visibility: c.visibility_m.map(|metres| VisibilityDetail {
            metres,
            note: visibility_note(metres).into(),
        }),
        pressure: c.pressure_msl_hpa.map(|hpa| {
            let trend = pressure_trend(hours);
            PressureDetail {
                hpa,
                trend,
                scale_position: ((hpa - PRESSURE_LOW_HPA) / (PRESSURE_HIGH_HPA - PRESSURE_LOW_HPA))
                    .clamp(0.0, 1.0),
                note: match trend {
                    Some(PressureTrend::Rising) => "Rising over the next 3 hours.".into(),
                    Some(PressureTrend::Falling) => "Falling over the next 3 hours.".into(),
                    Some(PressureTrend::Steady) => "Steady over the next 3 hours.".into(),
                    None => "At sea level.".into(),
                },
            }
        }),
        sun: today.and_then(|t| {
            sun(
                t,
                days.get(1).filter(|d| d.date.as_str() > t.date.as_str()),
                now,
            )
        }),
        moon: moon(report, now_unix),
    }
}

fn feels_note(feels: f64, actual: f64, windy: bool) -> String {
    if feels < actual - 1.5 {
        if windy {
            "Wind makes it feel colder."
        } else {
            "It feels colder than the actual temperature."
        }
    } else if feels > actual + 1.5 {
        "It feels warmer than the actual temperature."
    } else {
        "Similar to the actual temperature."
    }
    .into()
}

const POINTS: [&str; 16] = [
    "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW",
    "NNW",
];

/// One of the 16 compass points, e.g. "ENE".
pub(crate) fn compass(degrees: u16) -> &'static str {
    POINTS[((f64::from(degrees % 360) / 22.5).round() as usize) % 16]
}

/// The point spelt out for a sentence: "north-east", "east-north-east".
fn compass_name(degrees: u16) -> String {
    compass(degrees)
        .chars()
        .map(|c| match c {
            'N' => "north",
            'E' => "east",
            'S' => "south",
            _ => "west",
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// "Next expected is 4 mm on Tue." from the days after today.
fn next_rain(days: &[DailyForecast], units: UnitSettings) -> String {
    let ahead = days.get(1..).unwrap_or_default();
    match ahead
        .iter()
        .find(|d| d.precipitation_sum_mm.is_some_and(|mm| mm >= NEXT_RAIN_MM))
    {
        Some(day) => format!(
            "Next expected is {} on {}.",
            format_quantity(
                Quantity::Precipitation {
                    mm: day.precipitation_sum_mm.unwrap_or_default()
                },
                units
            ),
            weekday(&day.date)
        ),
        None if ahead.is_empty() => "Expected today.".into(),
        None => format!("None expected in the next {} days.", ahead.len()),
    }
}

fn uv_level(index: f64) -> (UvLevel, &'static str) {
    match index.round() as i64 {
        ..=2 => (UvLevel::Low, "Low"),
        3..=5 => (UvLevel::Moderate, "Moderate"),
        6..=7 => (UvLevel::High, "High"),
        8..=10 => (UvLevel::VeryHigh, "Very high"),
        _ => (UvLevel::Extreme, "Extreme"),
    }
}

/// UV now and what the rest of the day brings, from the hours left today;
/// today's peak when the source has no hourly UV.
fn uv(
    current: Option<f64>,
    hours: &[&HourlyPoint],
    today: Option<&DailyForecast>,
    now: &str,
) -> Option<UvDetail> {
    let date = now.get(..10).unwrap_or(now);
    let rest: Vec<(&str, f64)> = hours
        .iter()
        .filter(|h| h.time.starts_with(date))
        .filter_map(|h| Some((h.time.as_str(), h.uv_index?)))
        .collect();
    let index = current.or_else(|| rest.first().map(|(_, v)| *v));
    let (index, note) = match index {
        Some(index) => {
            let (level, label) = uv_level(index);
            let peak = rest.iter().copied().fold(None::<(&str, f64)>, |best, h| {
                if best.is_none_or(|b| h.1 > b.1) {
                    Some(h)
                } else {
                    best
                }
            });
            let note = match peak {
                Some((at, v)) if uv_level(v).0 != level => {
                    format!("{} from {}.", uv_level(v).1, &at[11..16.min(at.len())])
                }
                Some(_) => format!("{label} for the rest of the day."),
                None => match today.and_then(|t| t.uv_index_max) {
                    Some(max) => format!("Peak today {}.", max.round()),
                    None => format!("{label} now."),
                },
            };
            (index, note)
        }
        None => {
            let max = today?.uv_index_max?;
            (max, format!("{} at its peak today.", uv_level(max).1))
        }
    };
    let (level, label) = uv_level(index);
    Some(UvDetail {
        index,
        level,
        label: label.into(),
        scale_position: (index / 11.0).clamp(0.0, 1.0),
        note,
    })
}

fn visibility_note(metres: f64) -> &'static str {
    match metres {
        m if m >= 20_000.0 => "Perfectly clear view.",
        m if m >= 10_000.0 => "Clear view.",
        m if m >= 4_000.0 => "Slightly hazy.",
        m if m >= 1_000.0 => "Hazy.",
        _ => "Fog or heavy rain is limiting the view.",
    }
}

/// From this hour to [`TREND_HOURS`] later, when both are in the forecast.
fn pressure_trend(hours: &[&HourlyPoint]) -> Option<PressureTrend> {
    let now = hours.first()?.pressure_msl_hpa?;
    let later = hours.get(TREND_HOURS)?.pressure_msl_hpa?;
    Some(match later - now {
        d if d >= TREND_HPA => PressureTrend::Rising,
        d if d <= -TREND_HPA => PressureTrend::Falling,
        _ => PressureTrend::Steady,
    })
}

/// Minutes after midnight of a local "…THH:MM".
fn minutes(time: &str) -> Option<i64> {
    let hm = time.split('T').nth(1)?;
    let (h, m) = hm.split_once(':')?;
    Some(h.parse::<i64>().ok()? * 60 + m.get(..2)?.parse::<i64>().ok()?)
}

/// Today's sun; after sunset the sunrise shown is tomorrow's.
fn sun(today: &DailyForecast, tomorrow: Option<&DailyForecast>, now: &str) -> Option<SunDetail> {
    let hm = |t: &Option<String>| t.as_deref().and_then(|t| t.get(11..16)).map(str::to_string);
    let (rise, set) = (today.sunrise.as_deref(), today.sunset.as_deref());
    if rise.is_none() && set.is_none() {
        return None;
    }
    let now_min = minutes(now)?;
    let (rise_min, set_min) = (rise.and_then(minutes), set.and_then(minutes));
    let up = matches!((rise_min, set_min), (Some(r), Some(s)) if r <= now_min && now_min < s);
    let progress = match (rise_min, set_min) {
        (Some(r), Some(s)) if up && s > r => Some((now_min - r) as f64 / (s - r) as f64),
        _ => None,
    };
    let next_is_sunset = up || set_min.is_some_and(|s| now_min < s) && rise_min.is_none();
    let length = match (rise_min, set_min) {
        (Some(r), Some(s)) if s > r => {
            format!(" Daylight {} h {} min.", (s - r) / 60, (s - r) % 60)
        }
        _ => String::new(),
    };
    let after_sunset = set_min.is_some_and(|s| now_min >= s);
    let sunrise = match tomorrow {
        Some(t) if after_sunset && t.sunrise.is_some() => hm(&t.sunrise),
        _ => hm(&today.sunrise),
    };
    let other = if next_is_sunset {
        hm(&today.sunrise).map(|t| format!("Sunrise: {t}."))
    } else {
        hm(&today.sunset).map(|t| format!("Sunset: {t}."))
    };
    Some(SunDetail {
        sunrise,
        sunset: hm(&today.sunset),
        next_is_sunset,
        progress,
        note: format!("{}{length}", other.unwrap_or_default())
            .trim()
            .to_string(),
    })
}

fn moon(report: &WeatherReport, now_unix: i64) -> MoonDetail {
    let offset = i64::from(report.utc_offset_seconds);
    let local = now_unix + offset;
    let day_start = local - local.rem_euclid(86_400) - offset;
    let c = report.place.coordinate;
    let (rise, set) = moon_rise_set(day_start, c.latitude, c.longitude);
    let hm = |t: i64| {
        let s = (t + offset).rem_euclid(86_400);
        format!("{:02}:{:02}", s / 3_600, (s % 3_600) / 60)
    };
    let full = next_full_moon(now_unix);
    let days =
        u32::try_from((full + offset).div_euclid(86_400) - local.div_euclid(86_400)).unwrap_or(0);
    MoonDetail {
        phase: moon_phase(now_unix),
        moonrise: rise.map(hm),
        moonset: set.map(hm),
        days_to_full_moon: days,
        note: match days {
            0 => "Full moon today.".into(),
            1 => "Full moon tomorrow.".into(),
            n => format!("Next full moon in {n} days."),
        },
    }
}

/// "Mon" for a "YYYY-MM-DD".
pub(crate) fn weekday(date: &str) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    days_from_date(date)
        .map(|d| NAMES[d.rem_euclid(7) as usize])
        .unwrap_or("")
}

/// Days since 1970-01-01 for a "YYYY-MM-DD…" (Howard Hinnant's algorithm).
pub(crate) fn days_from_date(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The local "YYYY-MM-DDTHH:MM" `hours` earlier or later.
pub(crate) fn shift_hours(time: &str, hours: i64) -> Option<String> {
    let days = days_from_date(time)?;
    let at = days * 1_440 + minutes(time)? + hours * 60;
    let (y, m, d) = forge_runtime::time::civil_from_days(at.div_euclid(1_440));
    let min = at.rem_euclid(1_440);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}",
        min / 60,
        min % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::types::WeatherCondition;

    fn hour(time: &str, uv: Option<f64>, hpa: Option<f64>) -> HourlyPoint {
        HourlyPoint {
            time: time.into(),
            temperature_c: 5.0,
            precipitation_probability_pct: None,
            condition: WeatherCondition::Clear,
            is_day: true,
            wind_gusts_kmh: None,
            pressure_msl_hpa: hpa,
            uv_index: uv,
        }
    }

    fn day(date: &str, mm: Option<f64>) -> DailyForecast {
        DailyForecast {
            date: date.into(),
            condition: WeatherCondition::Clear,
            high_c: 3.0,
            low_c: -4.0,
            precipitation_probability_max_pct: None,
            precipitation_sum_mm: mm,
            sunrise: Some(format!("{date}T08:04")),
            sunset: Some(format!("{date}T15:57")),
            uv_index_max: Some(0.4),
        }
    }

    #[test]
    fn dates_and_weekdays() {
        assert_eq!(weekday("2026-11-21"), "Sat");
        assert_eq!(weekday("2026-11-24"), "Tue");
        assert_eq!(weekday("1970-01-01"), "Thu");
        assert_eq!(
            shift_hours("2026-03-01T01:00", -24).as_deref(),
            Some("2026-02-28T01:00")
        );
        assert_eq!(
            shift_hours("2024-12-31T23:00", 2).as_deref(),
            Some("2025-01-01T01:00")
        );
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0), "N");
        assert_eq!(compass(60), "ENE");
        assert_eq!(compass(349), "N");
        assert_eq!(compass(225), "SW");
        assert_eq!(compass_name(60), "east-north-east");
    }

    #[test]
    fn rain_today_and_the_next_wet_day() {
        let uk = crate::default_units(Some("GB".into()));
        let days = [
            day("2026-11-21", Some(0.0)),
            day("2026-11-22", Some(0.3)),
            day("2026-11-23", None),
            day("2026-11-24", Some(4.2)),
        ];
        assert_eq!(next_rain(&days, uk), "Next expected is 4.2 mm on Tue.");
        let dry = [day("2026-11-21", Some(0.0)), day("2026-11-22", Some(0.0))];
        assert_eq!(next_rain(&dry, uk), "None expected in the next 1 days.");
    }

    #[test]
    fn uv_levels_and_the_rest_of_the_day() {
        assert_eq!(uv_level(2.4).0, UvLevel::Low);
        assert_eq!(uv_level(2.6).0, UvLevel::Moderate);
        assert_eq!(uv_level(7.0).1, "High");
        assert_eq!(uv_level(11.0).0, UvLevel::Extreme);
        let today = day("2026-06-21", None);
        let morning = [
            hour("2026-06-21T09:00", Some(2.0), None),
            hour("2026-06-21T12:00", Some(6.5), None),
            hour("2026-06-22T12:00", Some(9.0), None),
        ];
        let refs: Vec<&HourlyPoint> = morning.iter().collect();
        let u = uv(Some(2.0), &refs, Some(&today), "2026-06-21T09:10").unwrap();
        assert_eq!(u.label, "Low");
        assert_eq!(u.note, "High from 12:00.", "tomorrow's hours don't count");
        let evening = [hour("2026-11-21T14:00", Some(0.0), None)];
        let refs: Vec<&HourlyPoint> = evening.iter().collect();
        let u = uv(Some(0.0), &refs, Some(&today), "2026-11-21T14:05").unwrap();
        assert_eq!(u.note, "Low for the rest of the day.");
        assert_eq!(u.scale_position, 0.0);
    }

    #[test]
    fn pressure_trend_over_three_hours() {
        let falling = [
            hour("T10:00", None, Some(1017.0)),
            hour("T11:00", None, Some(1016.6)),
            hour("T12:00", None, Some(1016.1)),
            hour("T13:00", None, Some(1015.8)),
        ];
        let refs: Vec<&HourlyPoint> = falling.iter().collect();
        assert_eq!(pressure_trend(&refs), Some(PressureTrend::Falling));
        assert_eq!(pressure_trend(&refs[..3]), None, "not far enough ahead");
    }

    #[test]
    fn the_sun_through_the_day() {
        let today = day("2026-11-21", None);
        let s = sun(&today, None, "2026-11-21T14:00").unwrap();
        assert!(s.next_is_sunset);
        assert_eq!(s.sunset.as_deref(), Some("15:57"));
        assert_eq!(s.note, "Sunrise: 08:04. Daylight 7 h 53 min.");
        let p = s.progress.unwrap();
        assert!((0.7..0.8).contains(&p), "{p}");
        let mut tomorrow = day("2026-11-22", None);
        tomorrow.sunrise = Some("2026-11-22T08:06".into());
        let night = sun(&today, Some(&tomorrow), "2026-11-21T20:00").unwrap();
        assert!(!night.next_is_sunset);
        assert_eq!(night.sunrise.as_deref(), Some("08:06"), "tomorrow's");
        assert_eq!(night.progress, None);
        assert!(night.note.starts_with("Sunset: 15:57."));
    }

    #[test]
    fn visibility_sentences() {
        assert_eq!(visibility_note(24_000.0), "Perfectly clear view.");
        assert_eq!(visibility_note(10_600.0), "Clear view.");
        assert_eq!(
            visibility_note(300.0),
            "Fog or heavy rain is limiting the view."
        );
    }
}
