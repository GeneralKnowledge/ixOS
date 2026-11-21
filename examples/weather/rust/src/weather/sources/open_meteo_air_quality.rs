//! Open-Meteo's air-quality API (https://open-meteo.com/en/docs/air-quality-api),
//! written from its documentation: hourly pollutants and indices, and
//! pollen (Europe only, from the CAMS European model). The data is based on
//! the Copernicus Atmosphere Monitoring Service, credited as both ask.
//!
//! The air-quality forecast reaches up to 7 days, shorter than the weather
//! forecast, and pollen exists only in Europe during the pollen season (4
//! days ahead), so outside either it is "not available here": each hour is
//! decoded as sent and nothing is extended or invented. Values the API
//! leaves `null` stay missing.

use forge_net::Request;
use serde::Deserialize;

use super::open_meteo::status_error;
use super::Source;
use crate::weather::error::WeatherError;
use crate::weather::types::{
    AirQualityHour, Coordinate, Place, PollenHour, PollenReading, PollenSpecies, SourceCredit,
};

pub(crate) const DEFAULT_AIR_QUALITY_BASE: &str = "https://air-quality-api.open-meteo.com";
/// The longest air-quality forecast the API documents.
pub(crate) const AIR_QUALITY_DAYS: u32 = 7;

const AIR_QUALITY_FIELDS: &str =
    "european_aqi,us_aqi,pm2_5,pm10,ozone,nitrogen_dioxide,sulphur_dioxide,carbon_monoxide";
const POLLEN_FIELDS: &str =
    "alder_pollen,birch_pollen,grass_pollen,mugwort_pollen,olive_pollen,ragweed_pollen";

pub(crate) fn credit() -> SourceCredit {
    SourceCredit {
        id: "open-meteo-air-quality".into(),
        name: "Open-Meteo, Copernicus (CAMS)".into(),
        credit: "Air quality by Open-Meteo.com (CC BY 4.0), with Copernicus Atmosphere Monitoring Service information".into(),
        url: "https://open-meteo.com/en/docs/air-quality-api".into(),
    }
}

fn url(base: &str, c: Coordinate, fields: &str) -> String {
    format!(
        "{}/v1/air-quality?latitude={:.4}&longitude={:.4}&hourly={fields}&timezone=auto&forecast_days={AIR_QUALITY_DAYS}&timeformat=iso8601",
        base.trim_end_matches('/'),
        c.latitude,
        c.longitude,
    )
}

#[derive(Deserialize)]
struct Response {
    hourly: Hourly,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<String>,
    #[serde(default)]
    european_aqi: Vec<Option<f64>>,
    #[serde(default)]
    us_aqi: Vec<Option<f64>>,
    #[serde(default)]
    pm2_5: Vec<Option<f64>>,
    #[serde(default)]
    pm10: Vec<Option<f64>>,
    #[serde(default)]
    ozone: Vec<Option<f64>>,
    #[serde(default)]
    nitrogen_dioxide: Vec<Option<f64>>,
    #[serde(default)]
    sulphur_dioxide: Vec<Option<f64>>,
    #[serde(default)]
    carbon_monoxide: Vec<Option<f64>>,
    #[serde(default)]
    alder_pollen: Vec<Option<f64>>,
    #[serde(default)]
    birch_pollen: Vec<Option<f64>>,
    #[serde(default)]
    grass_pollen: Vec<Option<f64>>,
    #[serde(default)]
    mugwort_pollen: Vec<Option<f64>>,
    #[serde(default)]
    olive_pollen: Vec<Option<f64>>,
    #[serde(default)]
    ragweed_pollen: Vec<Option<f64>>,
}

/// `values[i]` if it is a real, non-negative number.
fn at(values: &[Option<f64>], i: usize) -> Option<f64> {
    values
        .get(i)
        .copied()
        .flatten()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn index(values: &[Option<f64>], i: usize) -> Option<u16> {
    at(values, i).map(|v| v.round().min(f64::from(u16::MAX)) as u16)
}

fn parse(body: &[u8]) -> Result<Hourly, WeatherError> {
    serde_json::from_slice::<Response>(body)
        .map(|r| r.hourly)
        .map_err(WeatherError::bad_response)
}

/// Hours with at least one value. `None` when the API sent none at all:
/// it has no air quality for this place.
pub(crate) fn decode_air_quality(body: &[u8]) -> Result<Option<Vec<AirQualityHour>>, WeatherError> {
    let h = parse(body)?;
    let hours: Vec<AirQualityHour> = h
        .time
        .iter()
        .enumerate()
        .map(|(i, time)| AirQualityHour {
            time: time.clone(),
            european_aqi: index(&h.european_aqi, i),
            us_aqi: index(&h.us_aqi, i),
            pm2_5: at(&h.pm2_5, i),
            pm10: at(&h.pm10, i),
            ozone: at(&h.ozone, i),
            nitrogen_dioxide: at(&h.nitrogen_dioxide, i),
            sulphur_dioxide: at(&h.sulphur_dioxide, i),
            carbon_monoxide: at(&h.carbon_monoxide, i),
        })
        .filter(|a| {
            a.european_aqi.is_some()
                || a.us_aqi.is_some()
                || a.pm2_5.is_some()
                || a.pm10.is_some()
                || a.ozone.is_some()
                || a.nitrogen_dioxide.is_some()
                || a.sulphur_dioxide.is_some()
                || a.carbon_monoxide.is_some()
        })
        .collect();
    Ok((!hours.is_empty()).then_some(hours))
}

/// Hours with at least one species. A species with no value is left out of
/// that hour, never reported as zero. `None` when no species has any value:
/// the place is outside the pollen model.
pub(crate) fn decode_pollen(body: &[u8]) -> Result<Option<Vec<PollenHour>>, WeatherError> {
    let h = parse(body)?;
    let species: [(PollenSpecies, &[Option<f64>]); 6] = [
        (PollenSpecies::Alder, &h.alder_pollen),
        (PollenSpecies::Birch, &h.birch_pollen),
        (PollenSpecies::Grass, &h.grass_pollen),
        (PollenSpecies::Mugwort, &h.mugwort_pollen),
        (PollenSpecies::Olive, &h.olive_pollen),
        (PollenSpecies::Ragweed, &h.ragweed_pollen),
    ];
    let hours: Vec<PollenHour> = h
        .time
        .iter()
        .enumerate()
        .filter_map(|(i, time)| {
            let readings: Vec<PollenReading> = species
                .iter()
                .filter_map(|(species, values)| {
                    at(values, i).map(|grains_per_m3| PollenReading {
                        species: *species,
                        grains_per_m3,
                    })
                })
                .collect();
            (!readings.is_empty()).then(|| PollenHour {
                time: time.clone(),
                readings,
            })
        })
        .collect();
    Ok((!hours.is_empty()).then_some(hours))
}

/// Air quality: pollutants and both indices.
pub(crate) struct OpenMeteoAirQuality {
    pub(crate) base: String,
}

impl Source<Vec<AirQualityHour>> for OpenMeteoAirQuality {
    fn credit(&self) -> SourceCredit {
        credit()
    }

    fn request(&self, coordinate: Coordinate) -> Request {
        Request::get(url(&self.base, coordinate, AIR_QUALITY_FIELDS))
    }

    fn decode(
        &self,
        body: &[u8],
        _place: &Place,
        _fetched_at_unix: i64,
    ) -> Result<Option<Vec<AirQualityHour>>, WeatherError> {
        decode_air_quality(body)
    }

    fn status_error(&self, status: u16, body: &[u8]) -> WeatherError {
        status_error(status, body)
    }
}

/// Pollen, asked separately so that its coverage and failures are its own.
pub(crate) struct OpenMeteoPollen {
    pub(crate) base: String,
}

impl Source<Vec<PollenHour>> for OpenMeteoPollen {
    fn credit(&self) -> SourceCredit {
        credit()
    }

    fn request(&self, coordinate: Coordinate) -> Request {
        Request::get(url(&self.base, coordinate, POLLEN_FIELDS))
    }

    fn decode(
        &self,
        body: &[u8],
        _place: &Place,
        _fetched_at_unix: i64,
    ) -> Result<Option<Vec<PollenHour>>, WeatherError> {
        decode_pollen(body)
    }

    fn status_error(&self, status: u16, body: &[u8]) -> WeatherError {
        status_error(status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Edinburgh, both requests in one fixture (the API answers only what
    /// was asked; the decoders read only their own fields).
    const EDINBURGH: &str = include_str!("../fixtures/air_quality_edinburgh.json");
    /// Somewhere outside the European pollen model: pollen fields all null.
    const NEW_YORK: &str = include_str!("../fixtures/air_quality_new_york.json");

    #[test]
    fn urls_ask_for_the_documented_fields_and_seven_days() {
        let c = Coordinate {
            latitude: 55.95334,
            longitude: -3.18831,
        };
        let aq = url("https://aq.example/", c, AIR_QUALITY_FIELDS);
        assert!(aq.starts_with(
            "https://aq.example/v1/air-quality?latitude=55.9533&longitude=-3.1883&hourly=european_aqi,us_aqi,pm2_5"
        ));
        assert!(aq.contains("forecast_days=7"));
        assert!(aq.contains("timezone=auto"));
        let pollen = url("https://aq.example", c, POLLEN_FIELDS);
        assert!(pollen.contains("hourly=alder_pollen,birch_pollen,grass_pollen,mugwort_pollen,olive_pollen,ragweed_pollen&"));
    }

    #[test]
    fn decodes_air_quality_and_drops_hours_without_values() {
        let hours = decode_air_quality(EDINBURGH.as_bytes()).unwrap().unwrap();
        // 6 hours sent; the last has every value null.
        assert_eq!(hours.len(), 5);
        let first = &hours[0];
        assert_eq!(first.time, "2026-11-20T07:00");
        assert_eq!(first.european_aqi, Some(18));
        assert_eq!(first.us_aqi, Some(21));
        assert_eq!(first.pm2_5, Some(4.6));
        assert_eq!(first.carbon_monoxide, Some(161.0));
        // A null pollutant stays missing; the rest of the hour is kept.
        assert_eq!(hours[1].ozone, None);
        assert_eq!(hours[1].european_aqi, Some(24));
    }

    #[test]
    fn pollen_keeps_only_species_with_values() {
        let hours = decode_pollen(EDINBURGH.as_bytes()).unwrap().unwrap();
        // Hours with no species at all are dropped (the fixture's last two).
        assert_eq!(hours.len(), 4);
        let species: Vec<PollenSpecies> = hours[0].readings.iter().map(|r| r.species).collect();
        // Olive and ragweed are null at Edinburgh: left out, not zero.
        assert_eq!(
            species,
            vec![
                PollenSpecies::Alder,
                PollenSpecies::Birch,
                PollenSpecies::Grass,
                PollenSpecies::Mugwort
            ]
        );
        assert_eq!(hours[0].readings[2].grains_per_m3, 0.0, "a real zero stays");
    }

    #[test]
    fn outside_the_pollen_model_is_not_available_rather_than_zero() {
        assert_eq!(decode_pollen(NEW_YORK.as_bytes()).unwrap(), None);
        // The same place still has air quality.
        assert!(decode_air_quality(NEW_YORK.as_bytes()).unwrap().is_some());
    }

    #[test]
    fn bad_json_is_a_bad_response() {
        assert!(matches!(
            decode_pollen(b"{\"hourly\": 3}"),
            Err(WeatherError::BadResponse { .. })
        ));
    }
}
