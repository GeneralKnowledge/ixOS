//! What the screen shows for a report: the sky to paint, a one-sentence
//! summary, the hourly strip with sunrise and sunset, and the days ahead on
//! one temperature scale. Pure functions of a report and a moment, so the
//! host never re-derives forecast logic and the rules are tested here.

use super::types::{
    DailyForecast, HourlyPoint, SkyScene, TimelineEntry, WeatherCondition, WeatherOverview,
    WeatherReport, WindUnit,
};

/// Hours in the hourly strip.
pub(crate) const TIMELINE_HOURS: usize = 24;
/// Chance of precipitation at which an hour counts as wet.
const WET_THRESHOLD_PCT: u8 = 40;
/// With fewer hours than this left in the day, the summary looks at the
/// next [`EVENING_WINDOW_HOURS`] instead.
const MIN_DAY_WINDOW_HOURS: usize = 3;
const EVENING_WINDOW_HOURS: usize = 12;

/// Works out what to show for `report` at `now_unix` (Unix seconds). Hours
/// and days already past at the place are dropped, so a cached report reads
/// correctly. `wind_unit` is how the summary states wind speed.
#[uniffi::export]
pub fn weather_overview(
    report: WeatherReport,
    now_unix: i64,
    wind_unit: WindUnit,
) -> WeatherOverview {
    let now = local_time(now_unix, report.utc_offset_seconds);
    let local_date = now[..10].to_string();
    let current_hour = format!("{}:00", &now[..13]);

    let hours: Vec<&HourlyPoint> = report
        .hourly
        .iter()
        .filter(|p| p.time.as_str() >= current_hour.as_str())
        .take(TIMELINE_HOURS)
        .collect();
    let days: Vec<DailyForecast> = report
        .daily
        .iter()
        .filter(|d| d.date.as_str() >= local_date.as_str())
        .cloned()
        .collect();
    let today = days.first().filter(|d| d.date == local_date).cloned();

    // The current conditions are only "current" if the report is from this
    // hour; otherwise the first hour of the strip is the better guess.
    let observed_this_hour = report.current.observed_at.get(..13) == now.get(..13);
    let (condition, is_day) = match hours.first() {
        Some(first) if !observed_this_hour => (first.condition, first.is_day),
        _ => (report.current.condition, report.current.is_day),
    };

    let range_low_c = days.iter().map(|d| d.low_c).fold(f64::INFINITY, f64::min);
    let range_high_c = days
        .iter()
        .map(|d| d.high_c)
        .fold(f64::NEG_INFINITY, f64::max);
    let (range_low_c, range_high_c) = if days.is_empty() {
        let t = report.current.temperature_c;
        (t, t)
    } else {
        (range_low_c, range_high_c)
    };

    WeatherOverview {
        scene: scene(condition),
        is_day,
        summary: summary(&report, &hours, &local_date, condition, is_day, wind_unit),
        timeline: timeline(&hours, &report.daily, &now),
        local_date,
        today,
        days,
        range_low_c,
        range_high_c,
    }
}

pub(crate) fn scene(condition: WeatherCondition) -> SkyScene {
    use WeatherCondition as W;
    match condition {
        W::Clear | W::MainlyClear => SkyScene::Clear,
        W::PartlyCloudy => SkyScene::PartlyCloudy,
        W::Overcast | W::Unknown { .. } => SkyScene::Cloudy,
        W::Fog { .. } => SkyScene::Fog,
        W::Drizzle { .. } | W::Rain { .. } | W::RainShowers { .. } => SkyScene::Rain,
        W::Snow { .. } | W::SnowGrains | W::SnowShowers { .. } => SkyScene::Snow,
        W::Thunderstorm { .. } => SkyScene::Thunderstorm,
    }
}

/// Coarse weather families the summary talks in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
    Unknown,
}

impl Family {
    fn of(condition: WeatherCondition) -> Self {
        use WeatherCondition as W;
        match condition {
            W::Clear | W::MainlyClear => Self::Clear,
            W::PartlyCloudy => Self::PartlyCloudy,
            W::Overcast => Self::Cloudy,
            W::Fog { .. } => Self::Fog,
            W::Drizzle { .. } => Self::Drizzle,
            W::Rain { .. } | W::RainShowers { .. } => Self::Rain,
            W::Snow { .. } | W::SnowGrains | W::SnowShowers { .. } => Self::Snow,
            W::Thunderstorm { .. } => Self::Storm,
            W::Unknown { .. } => Self::Unknown,
        }
    }

    fn is_wet(self) -> bool {
        matches!(self, Self::Drizzle | Self::Rain | Self::Snow | Self::Storm)
    }

    /// The noun for wet families ("Rain expected around 15:00.").
    fn noun(self) -> &'static str {
        match self {
            Self::Drizzle => "Drizzle",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::Storm => "Thunderstorms",
            _ => "",
        }
    }

    /// The adjective for dry families ("Cloudy conditions ...").
    fn adjective(self, is_day: bool) -> &'static str {
        match self {
            Self::Clear if is_day => "Sunny",
            Self::Clear => "Clear",
            Self::PartlyCloudy => "Partly cloudy",
            Self::Cloudy => "Cloudy",
            Self::Fog => "Foggy",
            _ => "",
        }
    }
}

/// An hour counts as wet when its condition is and the chance is not low.
fn wet_hour(p: &HourlyPoint) -> bool {
    Family::of(p.condition).is_wet()
        && p.precipitation_probability_pct
            .is_none_or(|pct| pct >= WET_THRESHOLD_PCT)
}

fn hh_mm(time: &str) -> &str {
    time.split('T')
        .nth(1)
        .and_then(|t| t.get(..5))
        .unwrap_or(time)
}

fn summary(
    report: &WeatherReport,
    hours: &[&HourlyPoint],
    local_date: &str,
    condition: WeatherCondition,
    is_day: bool,
    wind_unit: WindUnit,
) -> String {
    let today: Vec<&HourlyPoint> = hours
        .iter()
        .copied()
        .filter(|p| p.time.starts_with(local_date))
        .collect();
    let (window, span) = if today.len() >= MIN_DAY_WINDOW_HOURS {
        (today, "for the rest of the day")
    } else {
        (
            hours.iter().copied().take(EVENING_WINDOW_HOURS).collect(),
            "overnight",
        )
    };

    let now = Family::of(condition);
    let later = window.get(1..).unwrap_or_default();
    let mut sentences = Vec::new();

    if now.is_wet() {
        match later.iter().find(|p| !wet_hour(p)) {
            Some(dry) => sentences.push(format!(
                "{} ending around {}.",
                now.noun(),
                hh_mm(&dry.time)
            )),
            None if !later.is_empty() => {
                sentences.push(format!("{} will continue {span}.", now.noun()))
            }
            None => {}
        }
    } else if let Some(wet) = later.iter().find(|p| wet_hour(p)) {
        let noun = Family::of(wet.condition).noun();
        let chance = wet
            .precipitation_probability_pct
            .map(|p| format!(" ({p}% chance)"))
            .unwrap_or_default();
        sentences.push(format!(
            "{noun} expected around {}{chance}.",
            hh_mm(&wet.time)
        ));
    } else if now != Family::Unknown && !later.is_empty() {
        // A change counts once it holds for two hours in a row.
        let change = later.windows(2).find(|pair| {
            let f = Family::of(pair[0].condition);
            f != now && f != Family::Unknown && Family::of(pair[1].condition) == f
        });
        match change {
            Some(pair) => sentences.push(format!(
                "{} conditions expected around {}.",
                Family::of(pair[0].condition).adjective(pair[0].is_day),
                hh_mm(&pair[0].time)
            )),
            None => sentences.push(format!(
                "{} conditions will continue {span}.",
                now.adjective(is_day)
            )),
        }
    }

    let gusts = window
        .iter()
        .filter_map(|p| p.wind_gusts_kmh)
        .chain(report.current.wind.as_ref().and_then(|w| w.gusts_kmh))
        .filter(|g| g.is_finite())
        .fold(None, |max: Option<f64>, g| {
            Some(max.map_or(g, |m| m.max(g)))
        });
    if let Some(kmh) = gusts {
        let (value, unit) = match wind_unit {
            WindUnit::KilometresPerHour => (kmh, "km/h"),
            WindUnit::MilesPerHour => (kmh / 1.609_344, "mph"),
        };
        let value = value.round() as i64;
        if value > 0 {
            sentences.push(format!("Wind gusts are up to {value} {unit}."));
        }
    }
    sentences.join(" ")
}

fn timeline(hours: &[&HourlyPoint], daily: &[DailyForecast], now: &str) -> Vec<TimelineEntry> {
    let Some(last) = hours.last() else {
        return Vec::new();
    };
    let end = format!("{}:59", &last.time[..last.time.len().min(13)]);
    // Sun events still ahead and inside the strip, in time order.
    let mut events: Vec<TimelineEntry> = daily
        .iter()
        .flat_map(|d| {
            [
                d.sunrise
                    .clone()
                    .map(|time| TimelineEntry::Sunrise { time }),
                d.sunset.clone().map(|time| TimelineEntry::Sunset { time }),
            ]
        })
        .flatten()
        .filter(|e| {
            let t = event_time(e);
            t > now && t <= end.as_str()
        })
        .collect();
    events.sort_by(|a, b| event_time(a).cmp(event_time(b)));

    let mut out = Vec::with_capacity(hours.len() + events.len());
    let mut events = events.into_iter().peekable();
    for (i, point) in hours.iter().enumerate() {
        out.push(TimelineEntry::Hour {
            point: (*point).clone(),
            is_now: i == 0,
        });
        let hour = point.time.get(..13).unwrap_or(&point.time);
        while let Some(e) = events.next_if(|e| event_time(e).get(..13) <= Some(hour)) {
            out.push(e);
        }
    }
    out
}

fn event_time(e: &TimelineEntry) -> &str {
    match e {
        TimelineEntry::Sunrise { time } | TimelineEntry::Sunset { time } => time,
        TimelineEntry::Hour { point, .. } => &point.time,
    }
}

/// "YYYY-MM-DDTHH:MM" at `utc_offset_seconds` from UTC.
pub(crate) fn local_time(unix: i64, utc_offset_seconds: i32) -> String {
    let t = unix + i64::from(utc_offset_seconds);
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian
/// (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::open_meteo::decode_forecast;
    use crate::weather::types::{Coordinate, Intensity, Place, PlaceSource};

    const FORECAST: &str = include_str!("fixtures/forecast_paris.json");
    /// 2026-11-04T20:30 in Paris (UTC+1).
    const PARIS_EVENING: i64 = 1_793_820_600;

    fn report() -> WeatherReport {
        let place = Place {
            name: "Paris".into(),
            region: None,
            country: None,
            country_code: None,
            coordinate: Coordinate {
                latitude: 48.85,
                longitude: 2.35,
            },
            timezone: None,
            population: None,
            source: PlaceSource::Manual,
        };
        decode_forecast(FORECAST.as_bytes(), place, PARIS_EVENING).unwrap()
    }

    fn hour(time: &str, condition: WeatherCondition, pct: Option<u8>) -> HourlyPoint {
        HourlyPoint {
            time: time.into(),
            temperature_c: 10.0,
            precipitation_probability_pct: pct,
            condition,
            is_day: true,
            wind_gusts_kmh: None,
        }
    }

    #[test]
    fn local_time_handles_offsets_and_dates() {
        assert_eq!(local_time(0, 0), "1970-01-01T00:00");
        assert_eq!(local_time(PARIS_EVENING, 3_600), "2026-11-04T20:30");
        assert_eq!(local_time(PARIS_EVENING, -10 * 3_600), "2026-11-04T09:30");
        assert_eq!(local_time(951_825_600, 0), "2000-02-29T12:00");
        assert_eq!(local_time(-1, 0), "1969-12-31T23:59");
    }

    #[test]
    fn hours_get_day_and_night_from_sunrise_and_sunset() {
        let r = report();
        let by_time = |t: &str| r.hourly.iter().find(|p| p.time == t).unwrap();
        assert!(!by_time("2026-11-04T20:00").is_day);
        assert!(!by_time("2026-11-05T07:00").is_day, "sunrise is 07:51");
        assert!(by_time("2026-11-05T08:00").is_day);
        assert!(by_time("2026-11-05T16:00").is_day);
        assert!(!by_time("2026-11-05T18:00").is_day);
    }

    #[test]
    fn overview_of_a_live_report() {
        let o = weather_overview(report(), PARIS_EVENING, WindUnit::KilometresPerHour);
        assert_eq!(o.local_date, "2026-11-04");
        assert_eq!(o.scene, SkyScene::Rain);
        assert!(!o.is_day);
        assert_eq!(o.today.as_ref().unwrap().date, "2026-11-04");
        assert_eq!(o.days.len(), 7);
        assert!(o.range_low_c <= o.days.iter().map(|d| d.low_c).fold(f64::MAX, f64::min));
        assert!(o.range_high_c >= o.days[0].high_c);

        let hours: Vec<_> = o
            .timeline
            .iter()
            .filter_map(|e| match e {
                TimelineEntry::Hour { point, is_now } => Some((point.time.as_str(), *is_now)),
                _ => None,
            })
            .collect();
        assert_eq!(hours.len(), 24);
        assert_eq!(hours[0], ("2026-11-04T20:00", true));
        assert!(!hours[1].1);
        // Tomorrow's sunrise sits right after the 07:00 slot.
        let sunrise = o
            .timeline
            .iter()
            .position(|e| matches!(e, TimelineEntry::Sunrise { .. }))
            .unwrap();
        match &o.timeline[sunrise - 1] {
            TimelineEntry::Hour { point, .. } => assert_eq!(point.time, "2026-11-05T07:00"),
            other => panic!("{other:?}"),
        }
        assert!(o.summary.contains("Wind gusts are up to"), "{}", o.summary);
    }

    #[test]
    fn a_cached_report_drops_what_is_past() {
        // The next afternoon: 18 hours later.
        let later = PARIS_EVENING + 18 * 3_600;
        let o = weather_overview(report(), later, WindUnit::KilometresPerHour);
        assert_eq!(o.local_date, "2026-11-05");
        assert_eq!(o.days[0].date, "2026-11-05");
        assert_eq!(o.days.len(), 6);
        match &o.timeline[0] {
            TimelineEntry::Hour { point, is_now } => {
                assert_eq!(point.time, "2026-11-05T14:00");
                assert!(is_now);
            }
            other => panic!("{other:?}"),
        }
        // A report from days ago has nothing left for the strip.
        let o = weather_overview(report(), later + 3 * 86_400, WindUnit::KilometresPerHour);
        assert!(o.timeline.is_empty());
    }

    #[test]
    fn summary_sentences() {
        let mut r = report();
        r.current.observed_at = "2026-11-05T09:00".into();
        r.current.condition = WeatherCondition::Overcast;
        r.current.wind = None;
        let nine_am = PARIS_EVENING + (12 * 60 + 30) * 60;
        let day = |conds: &[(WeatherCondition, Option<u8>)]| -> Vec<HourlyPoint> {
            conds
                .iter()
                .enumerate()
                .map(|(i, (c, p))| hour(&format!("2026-11-05T{:02}:00", 9 + i), *c, *p))
                .collect()
        };
        let o = |r: &WeatherReport| weather_overview(r.clone(), nine_am, WindUnit::MilesPerHour);
        let rain = WeatherCondition::Rain {
            intensity: Intensity::Moderate,
            freezing: false,
        };
        use WeatherCondition::{Clear, Overcast};

        r.hourly = day(&[(Overcast, None); 6]);
        assert_eq!(
            o(&r).summary,
            "Cloudy conditions will continue for the rest of the day."
        );

        r.hourly = day(&[
            (Overcast, None),
            (Overcast, None),
            (rain, Some(70)),
            (rain, Some(80)),
        ]);
        assert_eq!(o(&r).summary, "Rain expected around 11:00 (70% chance).");

        // A low chance of rain is not worth announcing.
        r.hourly = day(&[
            (Overcast, None),
            (rain, Some(10)),
            (Overcast, None),
            (Overcast, None),
        ]);
        assert_eq!(
            o(&r).summary,
            "Cloudy conditions will continue for the rest of the day."
        );

        r.hourly = day(&[
            (Overcast, None),
            (Overcast, None),
            (Clear, None),
            (Clear, None),
        ]);
        assert_eq!(o(&r).summary, "Sunny conditions expected around 11:00.");

        r.current.condition = rain;
        r.hourly = day(&[(rain, Some(90)), (rain, Some(80)), (Overcast, Some(20))]);
        assert_eq!(o(&r).summary, "Rain ending around 11:00.");

        r.hourly = day(&[(rain, Some(90)); 4]);
        for p in &mut r.hourly {
            p.wind_gusts_kmh = Some(32.0);
        }
        assert_eq!(
            o(&r).summary,
            "Rain will continue for the rest of the day. Wind gusts are up to 20 mph."
        );
    }

    #[test]
    fn scenes_cover_every_condition() {
        assert_eq!(scene(WeatherCondition::MainlyClear), SkyScene::Clear);
        assert_eq!(
            scene(WeatherCondition::SnowShowers {
                intensity: Intensity::Light
            }),
            SkyScene::Snow
        );
        assert_eq!(
            scene(WeatherCondition::Thunderstorm { hail: None }),
            SkyScene::Thunderstorm
        );
        assert_eq!(
            scene(WeatherCondition::Unknown { code: 4 }),
            SkyScene::Cloudy
        );
    }
}
