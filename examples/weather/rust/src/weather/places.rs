//! Saved places: the cities the user pages between, after "My Location".
//!
//! They are the person's own data, kept in a [`Document`] (forge-app's
//! saved-document rules): every change edits a copy, saves it, and only
//! then keeps it, so a failed write leaves the list as it was. A file this
//! build can't read is never replaced: the list shows as empty, changes
//! are refused with [`WeatherError::SavedDataUnreadable`] until the person
//! chooses to start a new list (which sets the old file aside and keeps it),
//! and a file from a newer version of the app is never touched at all. The
//! device location is never saved: the host asks the platform for it each
//! launch.

use std::path::{Path, PathBuf};

use forge_app::{Document, DocumentError};
use serde::{Deserialize, Serialize};

use super::app::ForgeWeatherApp;
use super::error::WeatherError;
use super::sources::open_meteo;
use super::types::{Place, PlaceSource, SavedDataProblem};
use crate::signal::lock;

/// The document (`{"schema_version": 1, "data": {"places": [...]}}`).
pub(crate) const PLACES_FILENAME: &str = "forge-weather-saved-places.json";
const PLACES_VERSION: u32 = 1;
/// The file builds before the document wrote
/// (`{"schema_version": 1, "places": [...]}`). Read once into the document
/// and then left where it is.
pub(crate) const LEGACY_PLACES_FILENAME: &str = "forge-weather-places-v1.json";
const LEGACY_PLACES_VERSION: u32 = 1;
/// More than anyone pages through.
pub(crate) const MAX_SAVED_PLACES: usize = 20;
/// A place this close to a saved one is the same place.
const SAME_PLACE_KM: f64 = 2.0;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub(crate) struct SavedPlaces {
    places: Vec<Place>,
}

/// Where the saved places are.
pub(crate) enum PlacesStore {
    /// No data folder (tests and previews): kept in memory only.
    Memory(Vec<Place>),
    Saved(Document<SavedPlaces>),
    /// The file could not be opened. Nothing is written until the person
    /// starts a new list (only allowed when it is unreadable, not newer).
    Failed {
        directory: PathBuf,
        error: DocumentError,
    },
}

impl PlacesStore {
    pub(crate) fn places(&self) -> Vec<Place> {
        match self {
            PlacesStore::Memory(places) => places.clone(),
            PlacesStore::Saved(doc) => doc.value().places.clone(),
            PlacesStore::Failed { .. } => Vec::new(),
        }
    }
}

/// Opens the saved places in `directory`, bringing over the older file the
/// first time.
pub(crate) fn open_places(directory: Option<&Path>) -> PlacesStore {
    let Some(directory) = directory else {
        return PlacesStore::Memory(Vec::new());
    };
    match open_document(directory) {
        Ok(doc) => PlacesStore::Saved(doc),
        Err(error) => PlacesStore::Failed {
            directory: directory.to_path_buf(),
            error,
        },
    }
}

fn open_document(directory: &Path) -> Result<Document<SavedPlaces>, DocumentError> {
    let path = directory.join(PLACES_FILENAME);
    let legacy = directory.join(LEGACY_PLACES_FILENAME);
    if path.exists() || !legacy.exists() {
        return Document::open(path, PLACES_VERSION);
    }
    let places = read_legacy(&legacy)?;
    let mut doc = Document::open(path, PLACES_VERSION)?;
    doc.replace(SavedPlaces { places })?;
    Ok(doc)
}

/// The list from a file the older builds wrote. Unreadable or newer files
/// are errors, never an empty list.
fn read_legacy(path: &Path) -> Result<Vec<Place>, DocumentError> {
    #[derive(Deserialize)]
    struct LegacyEnvelope {
        schema_version: u32,
        #[serde(default)]
        places: Option<Vec<Place>>,
    }
    let bytes = std::fs::read(path).map_err(|e| DocumentError::Io {
        reason: format!("read {}: {e}", path.display()),
    })?;
    let envelope: LegacyEnvelope =
        serde_json::from_slice(&bytes).map_err(|e| DocumentError::Unreadable {
            reason: e.to_string(),
        })?;
    if envelope.schema_version > LEGACY_PLACES_VERSION {
        return Err(DocumentError::Newer {
            found: envelope.schema_version,
            supported: LEGACY_PLACES_VERSION,
        });
    }
    envelope.places.ok_or_else(|| DocumentError::Unreadable {
        reason: "no places list".into(),
    })
}

#[uniffi::export]
impl ForgeWeatherApp {
    /// The saved places, in the user's order. Empty when they could not be
    /// read: [`Self::saved_places_problem`] says so.
    pub fn saved_places(&self) -> Vec<Place> {
        lock(&self.shared.places).places()
    }

    /// Why the saved places could not be read, if they could not. Until it
    /// is resolved every change is refused, so nothing overwrites them.
    pub fn saved_places_problem(&self) -> Option<SavedDataProblem> {
        match &*lock(&self.shared.places) {
            PlacesStore::Failed { error, .. } => Some(match error {
                DocumentError::Unreadable { .. } => SavedDataProblem::Unreadable,
                DocumentError::Newer { .. } => SavedDataProblem::FromNewerVersion,
                DocumentError::Io { .. } => SavedDataProblem::CouldNotOpen,
            }),
            _ => None,
        }
    }

    /// After the person chose to start a new list because the old one
    /// can't be read: sets the unreadable file aside (kept next to it,
    /// renamed) and starts an empty list. Refused for a file from a newer
    /// version, which is never touched. Returns the new (empty) list.
    pub fn start_new_saved_places(&self) -> Result<Vec<Place>, WeatherError> {
        if self.is_shut_down() {
            return Err(WeatherError::ShutDown);
        }
        let mut store = lock(&self.shared.places);
        let PlacesStore::Failed { directory, error } = &*store else {
            return Ok(store.places());
        };
        if !error.can_start_fresh() {
            return Err(WeatherError::from(error.clone()));
        }
        let now_ms = forge_runtime::now_ms();
        // Whichever file failed: the document, or the older file before it.
        let legacy = directory.join(LEGACY_PLACES_FILENAME);
        if !directory.join(PLACES_FILENAME).exists() && legacy.exists() {
            forge_storage::set_aside(&legacy, now_ms).map_err(|e| WeatherError::Internal {
                reason: format!("could not set the old places aside: {e}"),
            })?;
        }
        let (doc, _kept) = Document::start_fresh(
            directory.join(PLACES_FILENAME),
            PLACES_VERSION,
            None,
            now_ms,
        )?;
        *store = PlacesStore::Saved(doc);
        Ok(Vec::new())
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
    /// Refused while the saved places can't be read.
    fn update_places(
        &self,
        change: impl FnOnce(&mut Vec<Place>) -> Result<(), WeatherError>,
    ) -> Result<Vec<Place>, WeatherError> {
        if self.is_shut_down() {
            return Err(WeatherError::ShutDown);
        }
        let mut store = lock(&self.shared.places);
        match &mut *store {
            PlacesStore::Memory(places) => {
                let mut updated = places.clone();
                change(&mut updated)?;
                *places = updated.clone();
                Ok(updated)
            }
            PlacesStore::Saved(doc) => {
                let mut updated = doc.value().places.clone();
                change(&mut updated)?;
                if updated == doc.value().places {
                    return Ok(updated);
                }
                doc.replace(SavedPlaces {
                    places: updated.clone(),
                })?;
                Ok(updated)
            }
            PlacesStore::Failed { error, .. } => Err(WeatherError::from(error.clone())),
        }
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

        // A relaunch: the first core lets go of the folder first.
        drop(a);
        let restored = app(Some(dir.path().to_path_buf()));
        assert_eq!(restored.saved_places(), list);

        let list = restored
            .remove_saved_place(place("x", 51.221, 6.781))
            .unwrap();
        assert_eq!(list.len(), 1);
        drop(restored);
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
        let a = app(Some(dir.path().to_path_buf()));
        a.save_place(place("Prague", 50.09, 14.42)).unwrap();
        // A directory where the file should be makes the next write fail.
        let path = dir.path().join(PLACES_FILENAME);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            a.save_place(place("Paris", 48.85, 2.35)),
            Err(WeatherError::Internal { .. })
        ));
        assert_eq!(a.saved_places().len(), 1, "the list is as it was");
    }

    #[test]
    fn an_unreadable_file_is_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PLACES_FILENAME);
        std::fs::write(&path, b"{ not json").unwrap();
        let a = app(Some(dir.path().to_path_buf()));
        assert!(a.saved_places().is_empty());
        assert_eq!(a.saved_places_problem(), Some(SavedDataProblem::Unreadable));
        assert!(matches!(
            a.save_place(place("Prague", 50.09, 14.42)),
            Err(WeatherError::SavedDataUnreadable { .. })
        ));
        assert!(a.remove_saved_place(place("x", 1.0, 1.0)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json", "untouched");

        // Only when the person chooses to: set aside (kept), then a new list.
        assert_eq!(a.start_new_saved_places().unwrap(), Vec::<Place>::new());
        assert_eq!(a.saved_places_problem(), None);
        a.save_place(place("Prague", 50.09, 14.42)).unwrap();
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("unreadable"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path()).unwrap(), b"{ not json");
    }

    #[test]
    fn a_file_from_a_newer_version_is_never_touched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PLACES_FILENAME);
        let newer = br#"{"schema_version":2,"data":{"places":[],"groups":[]}}"#;
        std::fs::write(&path, newer).unwrap();
        let a = app(Some(dir.path().to_path_buf()));
        assert_eq!(
            a.saved_places_problem(),
            Some(SavedDataProblem::FromNewerVersion)
        );
        assert!(matches!(
            a.save_place(place("Prague", 50.09, 14.42)),
            Err(WeatherError::SavedDataNewer {
                found: 2,
                supported: 1
            })
        ));
        assert!(a.start_new_saved_places().is_err(), "not even on request");
        assert_eq!(std::fs::read(&path).unwrap(), newer);
        assert!(
            !std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().contains("unreadable")),
            "nothing set aside"
        );
    }

    #[test]
    fn places_saved_by_older_builds_are_brought_over() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_PLACES_FILENAME);
        let old = serde_json::json!({
            "schema_version": 1,
            "places": [place("Prague", 50.09, 14.42)],
        });
        std::fs::write(&legacy, serde_json::to_vec(&old).unwrap()).unwrap();
        let a = app(Some(dir.path().to_path_buf()));
        assert_eq!(a.saved_places()[0].name, "Prague");
        assert!(legacy.exists(), "the old file is left where it was");
        a.save_place(place("Paris", 48.85, 2.35)).unwrap();
        drop(a);
        assert_eq!(app(Some(dir.path().to_path_buf())).saved_places().len(), 2);
    }

    #[test]
    fn a_damaged_or_newer_older_file_is_reported_not_emptied() {
        for (contents, problem) in [
            (
                &br#"{"schema_version":1,"plac"#[..],
                SavedDataProblem::Unreadable,
            ),
            (
                &br#"{"schema_version":7,"places":[]}"#[..],
                SavedDataProblem::FromNewerVersion,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let legacy = dir.path().join(LEGACY_PLACES_FILENAME);
            std::fs::write(&legacy, contents).unwrap();
            let a = app(Some(dir.path().to_path_buf()));
            assert_eq!(a.saved_places_problem(), Some(problem));
            assert!(a.save_place(place("Prague", 50.09, 14.42)).is_err());
            assert!(!dir.path().join(PLACES_FILENAME).exists());
            assert_eq!(std::fs::read(&legacy).unwrap(), contents);
        }
    }
}
