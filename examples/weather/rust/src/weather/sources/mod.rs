//! Weather sources. Each kind of data a page shows (the forecast, air
//! quality, pollen) comes from a [`Source`] of its own, asked independently,
//! so one source failing or having nothing for a place never takes the
//! others with it (see `merge.rs`). Every source is written from its
//! provider's own documentation, and its module says which.
//!
//! Today every source is Open-Meteo's; the plan for more is in
//! `examples/weather/docs/breezy-counterpart-plan.md`.

pub(crate) mod open_meteo;
pub(crate) mod open_meteo_air_quality;

use std::time::Duration;

use forge_net::{CancelHandle, HttpClient};

use super::error::WeatherError;
use super::types::{AirQualityHour, Coordinate, Place, PollenHour, SourceCredit, WeatherReport};

/// A provider of one kind of data, `T`.
pub(crate) trait Source<T>: Send + Sync {
    /// The credit shown next to the data.
    fn credit(&self) -> SourceCredit;

    /// The request for `coordinate` (headers and timeout are added by
    /// [`fetch`]).
    fn request(&self, coordinate: Coordinate) -> forge_net::Request;

    /// Decodes a successful response. `Ok(None)` means the source answered
    /// and has no data for this place: not an error, and never filled in.
    fn decode(
        &self,
        body: &[u8],
        place: &Place,
        fetched_at_unix: i64,
    ) -> Result<Option<T>, WeatherError>;

    /// The error for a non-2xx response.
    fn status_error(&self, status: u16, _body: &[u8]) -> WeatherError {
        WeatherError::Server {
            status,
            reason: None,
        }
    }
}

/// The sources a refresh asks, one per kind of data.
pub(crate) struct Sources {
    pub(crate) forecast: Box<dyn Source<WeatherReport>>,
    pub(crate) air_quality: Box<dyn Source<Vec<AirQualityHour>>>,
    pub(crate) pollen: Box<dyn Source<Vec<PollenHour>>>,
}

impl Sources {
    /// Open-Meteo for everything, at the given base URLs.
    pub(crate) fn open_meteo(forecast_base: &str, air_quality_base: &str) -> Self {
        Self {
            forecast: Box::new(open_meteo::OpenMeteoForecast {
                base: forecast_base.to_string(),
            }),
            air_quality: Box::new(open_meteo_air_quality::OpenMeteoAirQuality {
                base: air_quality_base.to_string(),
            }),
            pollen: Box::new(open_meteo_air_quality::OpenMeteoPollen {
                base: air_quality_base.to_string(),
            }),
        }
    }
}

/// Asks `source` for `place`'s data.
pub(crate) fn fetch<T>(
    client: &dyn HttpClient,
    source: &dyn Source<T>,
    place: &Place,
    timeout: Duration,
    cancel: &CancelHandle,
    fetched_at_unix: i64,
) -> Result<Option<T>, WeatherError> {
    let request = source
        .request(place.coordinate)
        .header("Accept", "application/json")
        .timeout(timeout);
    let response = client.send_cancellable(&request, cancel)?;
    if !response.is_success() {
        return Err(source.status_error(response.status, &response.body));
    }
    source.decode(&response.body, place, fetched_at_unix)
}

/// The credit for reports cached before sources were named: they all came
/// from Open-Meteo.
pub(crate) fn open_meteo_credit() -> SourceCredit {
    open_meteo::credit()
}
