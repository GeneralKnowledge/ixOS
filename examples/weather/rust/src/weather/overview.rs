//! What the screen shows for a report: the sky to paint, a one-sentence
//! summary, the hourly strip with sunrise and sunset, and the days ahead on
//! one temperature scale. Pure functions of a report and a moment, so the
//! host never re-derives forecast logic and the rules are tested here.

use super::astro::moon_phase;
use super::details::{details, shift_hours};
use super::indices::{aqi_reading, hour_reading, scale_position};
use super::types::{
    AirQualityDay, AirQualityOverview, AirQualityScale, Coordinate, DailyForecast, HourlyPoint,
    PollenGap, PollenOverview, PollenSpeciesNow, SectionState, SkyScene, SourceCredit,
    TimelineEntry, UnitSettings, WeatherCondition, WeatherOverview, WeatherReport,
};
use super::units::speed;

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
/// correctly; air quality and pollen likewise, each only as far as its own
/// source reaches. `units` is how the summary states wind speed.
#[uniffi::export]
pub fn weather_overview(
    report: WeatherReport,
    now_unix: i64,
    units: UnitSettings,
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
        summary: summary(&report, &hours, &local_date, condition, is_day, units),
        timeline: timeline(&hours, &report.daily, &now),
        air_quality: air_quality(&report, &current_hour, &local_date, units.air_quality),
        pollen: pollen(&report, &current_hour, &local_date),
        moon: moon_phase(now_unix),
        daylight_minutes: today.as_ref().and_then(daylight_minutes),
        credits: credits(&report),
        details: details(
            &report,
            now_unix,
            &now,
            &hours,
            &days,
            today.as_ref(),
            units,
        ),
        local_date,
        today,
        days,
        range_low_c,
        range_high_c,
    }
}

/// Minutes between sunrise and sunset ("…THH:MM" strings on one day).
fn daylight_minutes(day: &DailyForecast) -> Option<u32> {
    let minutes = |t: &str| -> Option<u32> {
        let hm = t.split('T').nth(1)?;
        let (h, m) = hm.split_once(':')?;
        Some(h.parse::<u32>().ok()? * 60 + m.get(..2)?.parse::<u32>().ok()?)
    };
    let (rise, set) = (
        minutes(day.sunrise.as_deref()?)?,
        minutes(day.sunset.as_deref()?)?,
    );
    set.checked_sub(rise)
}

/// Air quality from the current hour on. Days are only those the source
/// covers; nothing is carried past its last hour.
fn air_quality(
    report: &WeatherReport,
    current_hour: &str,
    local_date: &str,
    scale: AirQualityScale,
) -> AirQualityOverview {
    let section = &report.air_quality;
    let ahead: Vec<_> = section
        .hours
        .iter()
        .filter(|h| h.time.as_str() >= current_hour)
        .collect();
    // This hour only: an hour already past is not "now".
    let now = ahead
        .first()
        .filter(|h| h.time.get(..13) == current_hour.get(..13))
        .map(|h| (*h).clone());
    let index = now.as_ref().and_then(|h| hour_reading(h, scale));
    // The same hour yesterday (the source sends the day before too).
    let yesterday = shift_hours(current_hour, -24).and_then(|t| {
        section
            .hours
            .iter()
            .find(|h| h.time.get(..13) == t.get(..13))
            .and_then(|h| hour_reading(h, scale))
    });
    let comparison = index.as_ref().zip(yesterday).map(|(now, then)| {
        let (a, b) = (i32::from(now.value), i32::from(then.value));
        if (a - b).abs() <= (b / 10).max(3) {
            "Similar to yesterday at this time.".to_string()
        } else if a > b {
            format!("Up from {b} yesterday at this time.")
        } else {
            format!("Down from {b} yesterday at this time.")
        }
    });
    let mut days: Vec<AirQualityDay> = Vec::new();
    for hour in section
        .hours
        .iter()
        .filter(|h| h.time.as_str() >= local_date)
    {
        let Some(reading) = hour_reading(hour, scale) else {
            continue;
        };
        let date = hour.time.get(..10).unwrap_or(&hour.time);
        match days.last_mut() {
            Some(day) if day.date == date => {
                if reading.value > day.worst.value {
                    day.worst = reading;
                }
            }
            _ => days.push(AirQualityDay {
                date: date.to_string(),
                worst: aqi_reading(reading.value, scale),
            }),
        }
    }
    AirQualityOverview {
        state: past_end(&section.state, ahead.is_empty()),
        source: section.source.clone(),
        scale_position: index.as_ref().map(|i| scale_position(i.value, scale)),
        comparison,
        index,
        now,
        days,
        covers_until: ahead.last().map(|h| h.time.clone()),
    }
}

/// Pollen from the current hour on, species highest today first.
fn pollen(report: &WeatherReport, current_hour: &str, local_date: &str) -> PollenOverview {
    let section = &report.pollen;
    let ahead: Vec<_> = section
        .hours
        .iter()
        .filter(|h| h.time.as_str() >= current_hour)
        .collect();
    let mut species: Vec<PollenSpeciesNow> = Vec::new();
    if let Some(first) = ahead
        .first()
        .filter(|h| h.time.get(..13) == current_hour.get(..13))
    {
        for reading in &first.readings {
            // The whole day, so a species that peaked this morning stays.
            let today_max = section
                .hours
                .iter()
                .filter(|h| h.time.starts_with(local_date))
                .flat_map(|h| &h.readings)
                .filter(|r| r.species == reading.species)
                .map(|r| r.grains_per_m3)
                .fold(reading.grains_per_m3, f64::max);
            species.push(PollenSpeciesNow {
                species: reading.species,
                now_grains_per_m3: reading.grains_per_m3,
                today_max_grains_per_m3: today_max,
            });
        }
    }
    species.sort_by(|a, b| {
        b.today_max_grains_per_m3
            .total_cmp(&a.today_max_grains_per_m3)
    });
    let had_reading = !species.is_empty();
    // Below half a grain rounds to nothing: not worth a row.
    species.retain(|s| s.today_max_grains_per_m3 >= 0.5);
    let summary = match species.first() {
        Some(top) => format!(
            "{} is highest today, up to {} grains per cubic metre.",
            pollen_name(top.species),
            top.today_max_grains_per_m3.round()
        ),
        None if had_reading => "No pollen in the air right now.".into(),
        None => String::new(),
    };
    let state = past_end(&section.state, ahead.is_empty());
    PollenOverview {
        gap: (state == SectionState::NotAvailableHere).then(|| pollen_gap(report.place.coordinate)),
        state,
        source: section.source.clone(),
        species,
        summary,
        covers_until: ahead.last().map(|h| h.time.clone()),
    }
}

/// The area of the European pollen model (CAMS European regional air
/// quality ensemble: 25° W to 45° E, 30° N to 72° N), which Open-Meteo's
/// pollen comes from ("only available in Europe during pollen season").
fn pollen_gap(c: Coordinate) -> PollenGap {
    let inside = (30.0..=72.0).contains(&c.latitude) && (-25.0..=45.0).contains(&c.longitude);
    if inside {
        PollenGap::NothingForecastNow
    } else {
        PollenGap::OutsideModelArea
    }
}

fn pollen_name(species: crate::weather::types::PollenSpecies) -> &'static str {
    use crate::weather::types::PollenSpecies::*;
    match species {
        Alder => "Alder",
        Birch => "Birch",
        Grass => "Grass",
        Mugwort => "Mugwort",
        Olive => "Olive",
        Ragweed => "Ragweed",
    }
}

/// A section whose hours are all in the past has nothing left to show:
/// it is unavailable (it was fetched, but it does not reach now).
fn past_end(state: &SectionState, nothing_ahead: bool) -> SectionState {
    match state {
        SectionState::Live | SectionState::Cached { .. } if nothing_ahead => {
            SectionState::Unavailable { problem: None }
        }
        other => other.clone(),
    }
}

/// The forecast's source first, then each other source once.
fn credits(report: &WeatherReport) -> Vec<SourceCredit> {
    let mut out = vec![report.forecast_source.clone()];
    for credit in [&report.air_quality.source, &report.pollen.source]
        .into_iter()
        .flatten()
    {
        if !out.iter().any(|c| c.id == credit.id) {
            out.push(credit.clone());
        }
    }
    out
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
    units: UnitSettings,
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
        let (value, unit) = speed(kmh, units.wind);
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
    let (y, m, d) = forge_runtime::time::civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::sources::open_meteo::decode_forecast;
    use crate::weather::types::{AirQualityScale, Coordinate, Intensity, Place, PlaceSource};

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
            pressure_msl_hpa: None,
            uv_index: None,
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
        let o = weather_overview(
            report(),
            PARIS_EVENING,
            crate::default_units(Some("FR".into())),
        );
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
        let o = weather_overview(report(), later, crate::default_units(Some("FR".into())));
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
        let o = weather_overview(
            report(),
            later + 3 * 86_400,
            crate::default_units(Some("FR".into())),
        );
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
        let o = |r: &WeatherReport| {
            weather_overview(r.clone(), nine_am, crate::default_units(Some("GB".into())))
        };
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

    fn air_hour(time: &str, eu: u16, us: u16) -> crate::AirQualityHour {
        crate::AirQualityHour {
            time: time.into(),
            european_aqi: Some(eu),
            us_aqi: Some(us),
            pm2_5: Some(5.0),
            pm10: None,
            ozone: None,
            nitrogen_dioxide: None,
            sulphur_dioxide: None,
            carbon_monoxide: None,
        }
    }

    fn with_sections() -> WeatherReport {
        use crate::{AirQualitySection, PollenHour, PollenReading, PollenSection, PollenSpecies};
        let credit = crate::weather::sources::open_meteo_air_quality::credit();
        let mut r = report();
        r.air_quality = AirQualitySection {
            state: SectionState::Live,
            source: Some(credit.clone()),
            fetched_at_unix: Some(PARIS_EVENING),
            hours: vec![
                air_hour("2026-11-04T19:00", 70, 150), // past: dropped
                air_hour("2026-11-04T20:00", 25, 40),
                air_hour("2026-11-04T23:00", 45, 60),
                air_hour("2026-11-05T03:00", 12, 20),
            ],
        };
        let pollen = |time: &str, birch: f64, grass: f64| PollenHour {
            time: time.into(),
            readings: vec![
                PollenReading {
                    species: PollenSpecies::Birch,
                    grains_per_m3: birch,
                },
                PollenReading {
                    species: PollenSpecies::Grass,
                    grains_per_m3: grass,
                },
            ],
        };
        r.pollen = PollenSection {
            state: SectionState::Live,
            source: Some(credit),
            fetched_at_unix: Some(PARIS_EVENING),
            hours: vec![
                pollen("2026-11-04T20:00", 1.0, 3.0),
                pollen("2026-11-04T22:00", 6.0, 2.0),
                pollen("2026-11-05T09:00", 40.0, 0.0), // tomorrow: not today's max
            ],
        };
        r
    }

    #[test]
    fn air_quality_reads_from_this_hour_and_stops_where_the_source_stops() {
        let eu = crate::default_units(Some("FR".into()));
        let o = weather_overview(with_sections(), PARIS_EVENING, eu);
        let a = &o.air_quality;
        assert_eq!(a.state, SectionState::Live);
        let index = a.index.as_ref().unwrap();
        assert_eq!((index.value, index.label.as_str()), (25, "Fair"));
        assert_eq!(a.now.as_ref().unwrap().time, "2026-11-04T20:00");
        // Days from today; today's worst ignores nothing still in the
        // report, tomorrow only has what the source sent.
        assert_eq!(a.days.len(), 2);
        assert_eq!(a.days[0].worst.value, 70);
        assert_eq!(a.days[1].worst.value, 12);
        assert_eq!(a.covers_until.as_deref(), Some("2026-11-05T03:00"));

        let us = crate::default_units(Some("US".into()));
        let o = weather_overview(with_sections(), PARIS_EVENING, us);
        assert_eq!(o.air_quality.index.unwrap().label, "Good");
    }

    #[test]
    fn air_quality_has_a_marker_and_compares_with_yesterday() {
        let eu = crate::default_units(Some("FR".into()));
        let mut r = with_sections();
        let o = weather_overview(r.clone(), PARIS_EVENING, eu);
        // 25 is a quarter of the way into the second of six bands.
        let p = o.air_quality.scale_position.unwrap();
        assert!((p - (1.25 / 6.0)).abs() < 1e-9, "{p}");
        assert_eq!(o.air_quality.comparison, None, "no reading from yesterday");
        r.air_quality
            .hours
            .insert(0, air_hour("2026-11-03T20:00", 24, 40));
        let o = weather_overview(r.clone(), PARIS_EVENING, eu);
        assert_eq!(
            o.air_quality.comparison.as_deref(),
            Some("Similar to yesterday at this time.")
        );
        r.air_quality.hours[0] = air_hour("2026-11-03T20:00", 60, 40);
        let o = weather_overview(r, PARIS_EVENING, eu);
        assert_eq!(
            o.air_quality.comparison.as_deref(),
            Some("Down from 60 yesterday at this time.")
        );
        assert_eq!(
            crate::weather::indices::scale_position(400, AirQualityScale::European),
            1.0
        );
    }

    #[test]
    fn pollen_says_what_is_highest_or_that_there_is_none() {
        let o = weather_overview(with_sections(), PARIS_EVENING, crate::default_units(None));
        assert_eq!(
            o.pollen.summary,
            "Birch is highest today, up to 6 grains per cubic metre."
        );
        let mut r = with_sections();
        for h in &mut r.pollen.hours {
            for reading in &mut h.readings {
                reading.grains_per_m3 = 0.0;
            }
        }
        let o = weather_overview(r, PARIS_EVENING, crate::default_units(None));
        assert!(o.pollen.species.is_empty(), "no rows of zeros");
        assert_eq!(o.pollen.summary, "No pollen in the air right now.");
        assert_eq!(o.pollen.state, SectionState::Live);
    }

    #[test]
    fn detail_cards_from_a_live_report() {
        let uk = crate::default_units(Some("GB".into()));
        let o = weather_overview(report(), PARIS_EVENING, uk);
        let d = &o.details;
        assert!(d.feels_like.is_some());
        let wind = d.wind.as_ref().unwrap();
        assert!(wind.direction.as_ref().unwrap().contains('°'));
        assert!(wind.note.contains("mph"), "{}", wind.note);
        let pressure = d.pressure.as_ref().unwrap();
        // The fixture falls 0.1 hPa an hour: steady over three hours.
        assert_eq!(pressure.trend, Some(crate::PressureTrend::Steady));
        assert!(d.visibility.is_some());
        let sun = d.sun.as_ref().unwrap();
        assert!(!sun.next_is_sunset, "20:30 in November is after sunset");
        assert_eq!(sun.progress, None);
        assert!(d.moon.note.contains("full moon") || d.moon.note.contains("Full moon"));
    }

    #[test]
    fn no_hour_for_now_means_no_index_rather_than_a_neighbour() {
        let mut r = with_sections();
        r.air_quality.hours.remove(1); // no 20:00
        let o = weather_overview(r, PARIS_EVENING, crate::default_units(None));
        assert_eq!(o.air_quality.index, None);
        assert_eq!(o.air_quality.now, None);
    }

    #[test]
    fn pollen_lists_species_by_todays_peak() {
        let o = weather_overview(with_sections(), PARIS_EVENING, crate::default_units(None));
        let p = &o.pollen;
        assert_eq!(p.species.len(), 2);
        assert_eq!(p.species[0].species, crate::PollenSpecies::Birch);
        assert_eq!(p.species[0].now_grains_per_m3, 1.0);
        assert_eq!(p.species[0].today_max_grains_per_m3, 6.0);
        assert_eq!(p.species[1].today_max_grains_per_m3, 3.0);
        assert_eq!(p.covers_until.as_deref(), Some("2026-11-05T09:00"));
    }

    #[test]
    fn sections_that_no_longer_reach_now_are_unavailable_and_not_here_stays() {
        let mut r = with_sections();
        r.pollen.state = SectionState::NotAvailableHere;
        r.pollen.hours.clear();
        // Two days later every air quality hour is past.
        let o = weather_overview(r, PARIS_EVENING + 2 * 86_400, crate::default_units(None));
        assert_eq!(
            o.air_quality.state,
            SectionState::Unavailable { problem: None }
        );
        assert_eq!(o.air_quality.index, None);
        assert!(o.air_quality.days.is_empty());
        assert_eq!(o.pollen.state, SectionState::NotAvailableHere);
        assert!(o.pollen.species.is_empty());
        // Paris is inside the pollen model's area: nothing forecast now.
        assert_eq!(o.pollen.gap, Some(PollenGap::NothingForecastNow));
    }

    #[test]
    fn no_pollen_outside_the_model_area_says_so() {
        let mut r = with_sections();
        r.pollen.state = SectionState::NotAvailableHere;
        r.pollen.hours.clear();
        r.place.coordinate = Coordinate {
            latitude: 40.71,
            longitude: -74.01,
        };
        let o = weather_overview(r, PARIS_EVENING, crate::default_units(None));
        assert_eq!(o.pollen.gap, Some(PollenGap::OutsideModelArea));
        // With data there is no gap.
        let o = weather_overview(with_sections(), PARIS_EVENING, crate::default_units(None));
        assert_eq!(o.pollen.gap, None);
    }

    #[test]
    fn credits_moon_and_daylight() {
        let o = weather_overview(with_sections(), PARIS_EVENING, crate::default_units(None));
        let ids: Vec<&str> = o.credits.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["open-meteo", "open-meteo-air-quality"]);
        // Sunrise 07:50; the fixture's sunset is checked against this.
        let today = o.today.as_ref().unwrap();
        let expected = {
            let m = |t: &str| {
                let hm = t.split('T').nth(1).unwrap();
                hm[..2].parse::<u32>().unwrap() * 60 + hm[3..5].parse::<u32>().unwrap()
            };
            m(today.sunset.as_deref().unwrap()) - m(today.sunrise.as_deref().unwrap())
        };
        assert_eq!(o.daylight_minutes, Some(expected));
        assert!(o.moon.illumination_pct <= 100);
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
