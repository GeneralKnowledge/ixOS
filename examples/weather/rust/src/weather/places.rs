//! Saved places: the cities the user pages between, after "My Location".
//!
//! The list lives in its own small JSON file next to the cache, written
//! atomically on every change. Unlike the cache it is the user's own data,
//! so a failed write is reported (`Internal`) and the in-memory list is left
//! as it was. The device location is never saved: the host asks the
//! platform for it each launch.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::app::ForgeWeatherApp;
use super::error::WeatherError;
use super::open_meteo;
use super::types::{Place, PlaceSource};
use crate::signal::lock;

pub(crate) const PLACES_FILENAME: &str = "forge-weather-places-v1.json";
const PLACES_SCHEMA_VERSION: u32 = 1;
/// More than anyone pages through.
pub(crate) const MAX_SAVED_PLACES: usize = 20;
/// A place this close to a saved one is the same place.
const SAME_PLACE_KM: f64 = 2.0;

#[derive(Serialize, Deserialize)]
struct PlacesEnvelope {
    schema_version: u32,
    places: Vec<Place>,
}

pub(crate) fn load_places(path: &Path) -> Vec<Place> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PlacesEnvelope>(&bytes).ok())
        .filter(|e| e.schema_version == PLACES_SCHEMA_VERSION)
        .map(|e| e.places)
        .unwrap_or_default()
}

#[uniffi::export]
impl ForgeWeatherApp {
    /// The saved places, in the user's order.
    pub fn saved_places(&self) -> Vec<Place> {
        lock(&self.shared.places).clone()
    }

    /// Adds `place` at the end, unless a saved place is already within 2 km
    /// of it. Returns the new list. Device locations cannot be saved.
    pub fn save_place(&self, place: Place) -> Result<Vec<Place>, WeatherError> {
        open_meteo::validate_coordinate(place.coordinate)?;
        if place.name.trim().is_empty() {
            return Err(WeatherError::invalid("place name is empty"));
        }
        if matches!(place.source, PlaceSource::Device { .. }) {
            return Err(WeatherError::invalid(
                "the device location is not saved; ask the platform for it",
            ));
        }
        self.update_places(|places| {
            if places
                .iter()
                .any(|p| super::app::distance_km(p.coordinate, place.coordinate) <= SAME_PLACE_KM)
            {
                return Ok(());
            }
            if places.len() >= MAX_SAVED_PLACES {
                return Err(WeatherError::invalid(format!(
                    "at most {MAX_SAVED_PLACES} places can be saved"
                )));
            }
            places.push(place);
            Ok(())
        })
    }

    /// Removes the saved place within 2 km of `place`, if any. Returns the
    /// new list.
    pub fn remove_saved_place(&self, place: Place) -> Result<Vec<Place>, WeatherError> {
        self.update_places(|places| {
            places.retain(|p| {
                super::app::distance_km(p.coordinate, place.coordinate) > SAME_PLACE_KM
            });
            Ok(())
        })
    }

    /// Moves the saved place at `from` to `to` (indexes in `saved_places()`).
    /// Returns the new list.
    pub fn move_saved_place(&self, from: u32, to: u32) -> Result<Vec<Place>, WeatherError> {
        self.update_places(|places| {
            let (from, to) = (from as usize, to as usize);
            if from >= places.len() || to >= places.len() {
                return Err(WeatherError::invalid(format!(
                    "move {from} -> {to} is outside 0..{}",
                    places.len()
                )));
            }
            let place = places.remove(from);
            places.insert(to, place);
            Ok(())
        })
    }
}

impl ForgeWeatherApp {
    /// Applies `change` to a copy, writes it, then makes it current.
    fn update_places(
        &self,
        change: impl FnOnce(&mut Vec<Place>) -> Result<(), WeatherError>,
    ) -> Result<Vec<Place>, WeatherError> {
        if self.is_shut_down() {
            return Err(WeatherError::ShutDown);
        }
        let mut places = lock(&self.shared.places);
        let mut updated = places.clone();
        change(&mut updated)?;
        if updated == *places {
            return Ok(updated);
        }
        if let Some(path) = &self.shared.places_path {
            let envelope = PlacesEnvelope {
                schema_version: PLACES_SCHEMA_VERSION,
                places: updated.clone(),
            };
            let json = serde_json::to_vec(&envelope).map_err(|e| WeatherError::Internal {
                reason: format!("could not encode saved places: {e}"),
            })?;
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            forge_storage::write_atomic(path, &json).map_err(|e| WeatherError::Internal {
                reason: format!("could not save places: {e}"),
            })?;
        }
        *places = updated.clone();
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use forge_net::StaticClient;

    use super::*;
    use crate::weather::types::{Coordinate, WeatherConfig};

    fn place(name: &str, latitude: f64, longitude: f64) -> Place {
        Place {
            name: name.into(),
            region: None,
            country: None,
            country_code: None,
            coordinate: Coordinate {
                latitude,
                longitude,
            },
            timezone: None,
            population: None,
            source: PlaceSource::Search { geocoding_id: None },
        }
    }

    fn app(dir: Option<PathBuf>) -> ForgeWeatherApp {
        ForgeWeatherApp::build(Arc::new(StaticClient::new()), dir, WeatherConfig::default())
            .unwrap()
    }

    #[test]
    fn places_are_saved_deduplicated_reordered_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let a = app(Some(dir.path().to_path_buf()));
        assert!(a.saved_places().is_empty());

        a.save_place(place("Düsseldorf", 51.22, 6.78)).unwrap();
        a.save_place(place("Prague", 50.09, 14.42)).unwrap();
        let list = a.save_place(place("Praha", 50.088, 14.421)).unwrap();
        assert_eq!(list.len(), 2, "within 2 km is the same place");

        let list = a.move_saved_place(1, 0).unwrap();
        assert_eq!(list[0].name, "Prague");
        assert!(a.move_saved_place(0, 2).is_err());

        let restored = app(Some(dir.path().to_path_buf()));
        assert_eq!(restored.saved_places(), list);

        let list = restored
            .remove_saved_place(place("x", 51.221, 6.781))
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(app(Some(dir.path().to_path_buf())).saved_places().len(), 1);
    }

    #[test]
    fn device_locations_and_bad_input_are_refused() {
        let a = app(None);
        let mut here = place("My Location", 55.9, -3.64);
        here.source = PlaceSource::Device {
            horizontal_accuracy_m: None,
        };
        assert!(matches!(
            a.save_place(here),
            Err(WeatherError::InvalidInput { .. })
        ));
        assert!(a.save_place(place(" ", 1.0, 1.0)).is_err());
        assert!(a.save_place(place("Nowhere", 91.0, 1.0)).is_err());
        for i in 0..MAX_SAVED_PLACES {
            a.save_place(place("p", i as f64, 0.0)).unwrap();
        }
        assert!(a.save_place(place("one too many", -50.0, 0.0)).is_err());
        a.shutdown();
        assert_eq!(
            a.remove_saved_place(place("p", 0.0, 0.0)),
            Err(WeatherError::ShutDown)
        );
    }

    #[test]
    fn a_failed_write_keeps_the_old_list() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be makes the write fail.
        std::fs::create_dir(dir.path().join(PLACES_FILENAME)).unwrap();
        let a = app(Some(dir.path().to_path_buf()));
        assert!(matches!(
            a.save_place(place("Prague", 50.09, 14.42)),
            Err(WeatherError::Internal { .. })
        ));
        assert!(a.saved_places().is_empty());
    }
}
