//! Display units. Reports store metric values; the person's
//! [`UnitSettings`] apply only when showing them, and every value on the
//! page goes through [`format_quantity`], so a unit is never applied in one
//! place and forgotten in another. The choice is saved in its own small
//! file (a forge-app [`Document`]); until the person picks, units follow the
//! region. A file this build can't read is never silently replaced: units
//! follow the region until the person picks again, which sets the damaged
//! file aside (kept, renamed); a file from a newer version is never written
//! over.

use std::path::{Path, PathBuf};

use forge_app::{Document, DocumentError};

use super::app::ForgeWeatherApp;
use super::error::WeatherError;
use super::types::{
    AirQualityScale, DistanceUnit, PrecipitationUnit, PressureUnit, Quantity, TemperatureUnit,
    UnitSettings, WindUnit,
};
use crate::signal::lock;

pub(crate) const UNITS_FILENAME: &str = "forge-weather-units.json";
const UNITS_VERSION: u32 = 1;

const KM_PER_MILE: f64 = 1.609_344;
const KMH_PER_KNOT: f64 = 1.852;
const MM_PER_INCH: f64 = 25.4;
const HPA_PER_INHG: f64 = 33.863_886_666_7;
const METRES_PER_MILE: f64 = 1_609.344;

/// Where the chosen units are.
pub(crate) enum UnitsStore {
    /// No data folder: kept in memory only.
    Memory(Option<UnitSettings>),
    Saved(Document<Option<UnitSettings>>),
    /// The file could not be opened; it is left alone.
    Failed {
        path: PathBuf,
        error: DocumentError,
    },
}

impl UnitsStore {
    fn chosen(&self) -> Option<UnitSettings> {
        match self {
            UnitsStore::Memory(units) => *units,
            UnitsStore::Saved(doc) => *doc.value(),
            UnitsStore::Failed { .. } => None,
        }
    }
}

pub(crate) fn open_units(directory: Option<&Path>) -> UnitsStore {
    let Some(directory) = directory else {
        return UnitsStore::Memory(None);
    };
    let path = directory.join(UNITS_FILENAME);
    match Document::open(&path, UNITS_VERSION) {
        Ok(doc) => UnitsStore::Saved(doc),
        Err(error) => UnitsStore::Failed { path, error },
    }
}

/// The usual units for a region (ISO 3166-1 alpha-2, e.g. "GB"): imperial
/// in the United States, Liberia and Myanmar; miles and mph with otherwise
/// metric units in the United Kingdom; metric elsewhere. The US air quality
/// index in the United States, the European one elsewhere.
#[uniffi::export]
pub fn default_units(region_code: Option<String>) -> UnitSettings {
    let region = region_code.unwrap_or_default().to_ascii_uppercase();
    let metric = UnitSettings {
        temperature: TemperatureUnit::Celsius,
        wind: WindUnit::KilometresPerHour,
        precipitation: PrecipitationUnit::Millimetres,
        pressure: PressureUnit::Hectopascals,
        distance: DistanceUnit::Kilometres,
        air_quality: AirQualityScale::European,
    };
    match region.as_str() {
        "US" | "LR" | "MM" => UnitSettings {
            temperature: TemperatureUnit::Fahrenheit,
            wind: WindUnit::MilesPerHour,
            precipitation: PrecipitationUnit::Inches,
            pressure: PressureUnit::InchesOfMercury,
            distance: DistanceUnit::Miles,
            air_quality: if region == "US" {
                AirQualityScale::UnitedStates
            } else {
                AirQualityScale::European
            },
        },
        "GB" => UnitSettings {
            wind: WindUnit::MilesPerHour,
            distance: DistanceUnit::Miles,
            ..metric
        },
        _ => metric,
    }
}

/// A stored (metric) value as text in `units`: "12°", "18 mph", "2.5 mm",
/// "1013 hPa", "24 km". Not-a-number shows as "–".
#[uniffi::export]
pub fn format_quantity(quantity: Quantity, units: UnitSettings) -> String {
    let finite = match quantity {
        Quantity::Temperature { celsius: v }
        | Quantity::Speed { kmh: v }
        | Quantity::Precipitation { mm: v }
        | Quantity::Pressure { hpa: v }
        | Quantity::Distance { metres: v } => v.is_finite(),
    };
    if !finite {
        return "–".into();
    }
    match quantity {
        Quantity::Temperature { celsius } => {
            let value = match units.temperature {
                TemperatureUnit::Celsius => celsius,
                TemperatureUnit::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
            };
            format!("{}°", whole(value))
        }
        Quantity::Speed { kmh } => {
            let (value, unit) = speed(kmh, units.wind);
            format!("{} {unit}", whole(value))
        }
        Quantity::Precipitation { mm } => match units.precipitation {
            PrecipitationUnit::Millimetres => format!("{:.1} mm", mm),
            PrecipitationUnit::Inches => format!("{:.2} in", mm / MM_PER_INCH),
        },
        Quantity::Pressure { hpa } => match units.pressure {
            PressureUnit::Hectopascals => format!("{} hPa", whole(hpa)),
            PressureUnit::InchesOfMercury => format!("{:.2} inHg", hpa / HPA_PER_INHG),
        },
        Quantity::Distance { metres } => {
            let (value, unit) = match units.distance {
                DistanceUnit::Kilometres => (metres / 1_000.0, "km"),
                DistanceUnit::Miles => (metres / METRES_PER_MILE, "mi"),
            };
            if value < 10.0 {
                format!("{value:.1} {unit}")
            } else {
                format!("{} {unit}", whole(value))
            }
        }
    }
}

/// A speed in `unit`, and the unit's symbol.
pub(crate) fn speed(kmh: f64, unit: WindUnit) -> (f64, &'static str) {
    match unit {
        WindUnit::KilometresPerHour => (kmh, "km/h"),
        WindUnit::MilesPerHour => (kmh / KM_PER_MILE, "mph"),
        WindUnit::MetresPerSecond => (kmh / 3.6, "m/s"),
        WindUnit::Knots => (kmh / KMH_PER_KNOT, "kn"),
    }
}

/// Rounded to a whole number, without "-0".
fn whole(value: f64) -> i64 {
    let rounded = value.round() as i64;
    if rounded == 0 {
        0
    } else {
        rounded
    }
}

#[uniffi::export]
impl ForgeWeatherApp {
    /// The person's units, or the usual ones for `region_code` if they have
    /// not chosen.
    pub fn units(&self, region_code: Option<String>) -> UnitSettings {
        lock(&self.shared.units)
            .chosen()
            .unwrap_or_else(|| default_units(region_code))
    }

    /// Saves the person's units. A failed write is an error and changes
    /// nothing. If the saved units could not be read, choosing new ones
    /// sets that file aside (kept) first; a file from a newer version of
    /// the app is never written over, so the change is refused.
    pub fn set_units(&self, units: UnitSettings) -> Result<(), WeatherError> {
        if self.is_shut_down() {
            return Err(WeatherError::ShutDown);
        }
        let mut store = lock(&self.shared.units);
        match &mut *store {
            UnitsStore::Memory(current) => *current = Some(units),
            UnitsStore::Saved(doc) => doc.replace(Some(units))?,
            UnitsStore::Failed { path, error } => {
                if !error.can_start_fresh() {
                    return Err(WeatherError::from(error.clone()));
                }
                let (mut doc, _kept) = Document::start_fresh(
                    path.clone(),
                    UNITS_VERSION,
                    None,
                    forge_runtime::now_ms(),
                )?;
                doc.replace(Some(units))?;
                *store = UnitsStore::Saved(doc);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric() -> UnitSettings {
        default_units(Some("fr".into()))
    }

    #[test]
    fn regions_get_their_usual_units() {
        let us = default_units(Some("US".into()));
        assert_eq!(us.temperature, TemperatureUnit::Fahrenheit);
        assert_eq!(us.air_quality, AirQualityScale::UnitedStates);
        let gb = default_units(Some("gb".into()));
        assert_eq!(gb.temperature, TemperatureUnit::Celsius);
        assert_eq!(gb.wind, WindUnit::MilesPerHour);
        assert_eq!(gb.distance, DistanceUnit::Miles);
        assert_eq!(gb.precipitation, PrecipitationUnit::Millimetres);
        assert_eq!(gb.air_quality, AirQualityScale::European);
        assert_eq!(metric().wind, WindUnit::KilometresPerHour);
        assert_eq!(default_units(None), metric());
    }

    #[test]
    fn every_quantity_converts_and_formats() {
        let us = default_units(Some("US".into()));
        let t = |c| Quantity::Temperature { celsius: c };
        assert_eq!(format_quantity(t(21.6), metric()), "22°");
        assert_eq!(format_quantity(t(-0.4), metric()), "0°", "no minus zero");
        assert_eq!(format_quantity(t(0.0), us), "32°");
        assert_eq!(format_quantity(t(-40.0), us), "-40°");

        let s = Quantity::Speed { kmh: 29.0 };
        assert_eq!(format_quantity(s, metric()), "29 km/h");
        assert_eq!(format_quantity(s, us), "18 mph");
        let ms = UnitSettings {
            wind: WindUnit::MetresPerSecond,
            ..metric()
        };
        assert_eq!(format_quantity(s, ms), "8 m/s");
        let kn = UnitSettings {
            wind: WindUnit::Knots,
            ..metric()
        };
        assert_eq!(format_quantity(s, kn), "16 kn");

        let p = Quantity::Precipitation { mm: 2.54 };
        assert_eq!(format_quantity(p, metric()), "2.5 mm");
        assert_eq!(format_quantity(p, us), "0.10 in");

        let hpa = Quantity::Pressure { hpa: 1013.25 };
        assert_eq!(format_quantity(hpa, metric()), "1013 hPa");
        assert_eq!(format_quantity(hpa, us), "29.92 inHg");

        assert_eq!(
            format_quantity(Quantity::Distance { metres: 24_140.0 }, metric()),
            "24 km"
        );
        assert_eq!(
            format_quantity(Quantity::Distance { metres: 24_140.0 }, us),
            "15 mi"
        );
        assert_eq!(
            format_quantity(Quantity::Distance { metres: 800.0 }, metric()),
            "0.8 km"
        );
        assert_eq!(
            format_quantity(Quantity::Speed { kmh: f64::NAN }, metric()),
            "–"
        );
    }

    #[test]
    fn chosen_units_are_saved_and_survive_a_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        let us = default_units(Some("US".into()));
        {
            let app = ForgeWeatherApp::new(path.clone()).unwrap();
            assert_eq!(app.units(Some("FR".into())), metric(), "region default");
            app.set_units(us).unwrap();
            assert_eq!(app.units(Some("FR".into())), us);
            app.shutdown();
        }
        let app = ForgeWeatherApp::new(path).unwrap();
        assert_eq!(app.units(Some("FR".into())), us, "kept after relaunch");
    }

    #[test]
    fn damaged_or_newer_units_are_never_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(UNITS_FILENAME);
        let path = dir.path().to_str().unwrap().to_string();
        let us = default_units(Some("US".into()));

        // Newer: units follow the region, and the file is never written over.
        let newer = br#"{"schema_version":2,"data":{"temperature":"Kelvin"}}"#;
        std::fs::write(&file, newer).unwrap();
        let app = ForgeWeatherApp::new(path.clone()).unwrap();
        assert_eq!(app.units(Some("FR".into())), metric());
        assert!(matches!(
            app.set_units(us),
            Err(WeatherError::SavedDataNewer { found: 2, .. })
        ));
        assert_eq!(std::fs::read(&file).unwrap(), newer);
        app.shutdown();
        drop(app);

        // Damaged: region units until the person picks; picking keeps the
        // damaged file, set aside.
        std::fs::write(&file, b"garbage").unwrap();
        let app = ForgeWeatherApp::new(path).unwrap();
        assert_eq!(app.units(Some("FR".into())), metric());
        assert_eq!(std::fs::read(&file).unwrap(), b"garbage", "not on open");
        app.set_units(us).unwrap();
        assert_eq!(app.units(Some("FR".into())), us);
        let kept = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("unreadable"))
            .count();
        assert_eq!(kept, 1);
    }
}
