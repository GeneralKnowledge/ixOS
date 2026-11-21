//! Putting one report together from several sources. The forecast is the
//! report's base; air quality and pollen are merged in as sections, each
//! with its own state. The rules:
//!
//! * A section that arrived is `Live`, whatever happened to the others.
//! * A source that answered with nothing for the place is
//!   `NotAvailableHere`: no older data is shown in its place.
//! * A section whose fetch failed keeps the data from an earlier refresh,
//!   if there is some within the cache's age limit, as `Cached` with the
//!   problem; otherwise it is `Unavailable` with the problem. Nothing is
//!   filled in or extended.

use super::error::WeatherError;
use super::types::{
    AirQualityHour, AirQualitySection, FetchProblem, PollenHour, PollenSection, SectionState,
    SourceCredit,
};

/// What a section looks like once merged.
struct Merged<T> {
    state: SectionState,
    source: Option<SourceCredit>,
    fetched_at_unix: Option<i64>,
    hours: Vec<T>,
}

/// A section from an earlier refresh.
struct Prior<'a, T> {
    source: Option<&'a SourceCredit>,
    fetched_at_unix: Option<i64>,
    hours: &'a [T],
}

/// The problem to show for a section that could not be fetched. Unlike the
/// forecast, a section never fails the page, so every error has one.
pub(crate) fn section_problem(err: &WeatherError) -> FetchProblem {
    match err {
        WeatherError::Server { status, .. } => FetchProblem::ServerError { status: *status },
        other => other
            .as_fetch_problem()
            .unwrap_or(FetchProblem::BadResponse),
    }
}

fn merge<T: Clone>(
    fetched: Result<Option<Vec<T>>, WeatherError>,
    credit: SourceCredit,
    now_unix: i64,
    prior: Option<Prior<'_, T>>,
    max_age_seconds: u64,
) -> Merged<T> {
    match fetched {
        Ok(Some(hours)) if !hours.is_empty() => Merged {
            state: SectionState::Live,
            source: Some(credit),
            fetched_at_unix: Some(now_unix),
            hours,
        },
        Ok(_) => Merged {
            state: SectionState::NotAvailableHere,
            source: Some(credit),
            fetched_at_unix: Some(now_unix),
            hours: Vec::new(),
        },
        Err(err) => {
            let problem = section_problem(&err);
            let kept = prior.and_then(|p| {
                let at = p.fetched_at_unix?;
                let age = u64::try_from(now_unix.saturating_sub(at)).unwrap_or(0);
                (!p.hours.is_empty() && age <= max_age_seconds).then(|| Merged {
                    state: SectionState::Cached {
                        age_seconds: age,
                        problem: Some(problem.clone()),
                    },
                    source: p.source.cloned(),
                    fetched_at_unix: Some(at),
                    hours: p.hours.to_vec(),
                })
            });
            kept.unwrap_or(Merged {
                state: SectionState::Unavailable {
                    problem: Some(problem),
                },
                source: Some(credit),
                fetched_at_unix: None,
                hours: Vec::new(),
            })
        }
    }
}

pub(crate) fn air_quality(
    fetched: Result<Option<Vec<AirQualityHour>>, WeatherError>,
    credit: SourceCredit,
    now_unix: i64,
    prior: Option<&AirQualitySection>,
    max_age_seconds: u64,
) -> AirQualitySection {
    let prior = prior.map(|p| Prior {
        source: p.source.as_ref(),
        fetched_at_unix: p.fetched_at_unix,
        hours: &p.hours,
    });
    let m = merge(fetched, credit, now_unix, prior, max_age_seconds);
    AirQualitySection {
        state: m.state,
        source: m.source,
        fetched_at_unix: m.fetched_at_unix,
        hours: m.hours,
    }
}

pub(crate) fn pollen(
    fetched: Result<Option<Vec<PollenHour>>, WeatherError>,
    credit: SourceCredit,
    now_unix: i64,
    prior: Option<&PollenSection>,
    max_age_seconds: u64,
) -> PollenSection {
    let prior = prior.map(|p| Prior {
        source: p.source.as_ref(),
        fetched_at_unix: p.fetched_at_unix,
        hours: &p.hours,
    });
    let m = merge(fetched, credit, now_unix, prior, max_age_seconds);
    PollenSection {
        state: m.state,
        source: m.source,
        fetched_at_unix: m.fetched_at_unix,
        hours: m.hours,
    }
}

/// A section read back from the cache: data it holds is `Cached` with its
/// own age (`problem` is why this refresh could not replace it, if it
/// tried). "Not available here" and "unavailable" stay as they were.
pub(crate) fn age_state(
    state: &SectionState,
    has_data: bool,
    fetched_at_unix: Option<i64>,
    now_unix: i64,
    problem: Option<FetchProblem>,
) -> SectionState {
    match (state, fetched_at_unix) {
        (SectionState::Live | SectionState::Cached { .. }, Some(at)) if has_data => {
            SectionState::Cached {
                age_seconds: u64::try_from(now_unix.saturating_sub(at)).unwrap_or(0),
                problem,
            }
        }
        _ => state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit() -> SourceCredit {
        crate::weather::sources::open_meteo_air_quality::credit()
    }

    fn hour(time: &str, aqi: u16) -> AirQualityHour {
        AirQualityHour {
            time: time.into(),
            european_aqi: Some(aqi),
            us_aqi: None,
            pm2_5: None,
            pm10: None,
            ozone: None,
            nitrogen_dioxide: None,
            sulphur_dioxide: None,
            carbon_monoxide: None,
        }
    }

    fn earlier(at: i64) -> AirQualitySection {
        AirQualitySection {
            state: SectionState::Live,
            source: Some(credit()),
            fetched_at_unix: Some(at),
            hours: vec![hour("2026-11-20T07:00", 18)],
        }
    }

    const NOW: i64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    #[test]
    fn arrived_data_is_live() {
        let s = air_quality(
            Ok(Some(vec![hour("2026-11-20T08:00", 30)])),
            credit(),
            NOW,
            Some(&earlier(NOW - 600)),
            DAY,
        );
        assert_eq!(s.state, SectionState::Live);
        assert_eq!(s.fetched_at_unix, Some(NOW));
        assert_eq!(s.hours[0].european_aqi, Some(30));
    }

    #[test]
    fn nothing_for_the_place_is_not_available_and_shows_no_older_data() {
        let s = air_quality(Ok(None), credit(), NOW, Some(&earlier(NOW - 600)), DAY);
        assert_eq!(s.state, SectionState::NotAvailableHere);
        assert!(s.hours.is_empty());
    }

    #[test]
    fn a_failure_keeps_recent_data_with_the_problem() {
        let s = air_quality(
            Err(WeatherError::Timeout),
            credit(),
            NOW,
            Some(&earlier(NOW - 600)),
            DAY,
        );
        assert_eq!(
            s.state,
            SectionState::Cached {
                age_seconds: 600,
                problem: Some(FetchProblem::TimedOut)
            }
        );
        assert_eq!(s.fetched_at_unix, Some(NOW - 600));
        assert_eq!(s.hours.len(), 1);
    }

    #[test]
    fn a_failure_without_usable_data_says_unavailable_and_why() {
        // Too old to keep.
        let s = air_quality(
            Err(WeatherError::Offline {
                reason: "no route".into(),
            }),
            credit(),
            NOW,
            Some(&earlier(NOW - 2 * DAY as i64)),
            DAY,
        );
        assert_eq!(
            s.state,
            SectionState::Unavailable {
                problem: Some(FetchProblem::Offline)
            }
        );
        assert!(s.hours.is_empty());
        // Nothing earlier; a 4xx still gets a problem, since a section
        // never fails the page.
        let s = pollen(
            Err(WeatherError::Server {
                status: 400,
                reason: None,
            }),
            credit(),
            NOW,
            None,
            DAY,
        );
        assert_eq!(
            s.state,
            SectionState::Unavailable {
                problem: Some(FetchProblem::ServerError { status: 400 })
            }
        );
    }

    #[test]
    fn cached_sections_age_from_their_own_fetch() {
        let aged = age_state(&SectionState::Live, true, Some(NOW - 90), NOW, None);
        assert_eq!(
            aged,
            SectionState::Cached {
                age_seconds: 90,
                problem: None
            }
        );
        let here = age_state(&SectionState::NotAvailableHere, false, Some(NOW), NOW, None);
        assert_eq!(here, SectionState::NotAvailableHere);
    }
}
