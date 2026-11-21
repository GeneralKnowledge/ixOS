//! The Weather example: forecast, air quality, pollen and place search from
//! independent sources (all Open-Meteo today), with refresh/cancel and an
//! offline cache. See `app.rs` for the concurrency
//! contract and `types.rs` for the FFI payload.

mod app;
mod astro;
mod details;
mod error;
mod indices;
mod location;
mod merge;
mod overview;
mod places;
pub(crate) mod sources;
mod types;
mod units;

pub use app::{ForgeWeatherApp, WeatherOperation};
pub use error::{weather_error_kind, WeatherError};
pub use location::DeviceFix;
pub use overview::weather_overview;
pub use types::*;
pub use units::{default_units, format_quantity};
