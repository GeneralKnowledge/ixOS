//! The Weather example: forecast and place search from Open-Meteo, with
//! refresh/cancel and an offline cache. See `app.rs` for the concurrency
//! contract and `types.rs` for the FFI payload.

mod app;
mod error;
mod open_meteo;
mod types;

pub use app::{ForgeWeatherApp, WeatherOperation};
pub use error::WeatherError;
pub use types::*;
