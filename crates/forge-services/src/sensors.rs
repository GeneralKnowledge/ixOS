//! Plain-Rust rules behind the optional device services (motion, location,
//! device status). The UniFFI items are in the `__native_*` macros next to
//! this file; what does not cross the boundary lives here, with tests.

/// Fastest motion update interval accepted: 10 ms (100 Hz), the most the
/// iPhone's motion coprocessor delivers.
pub const MIN_MOTION_INTERVAL_MS: u32 = 10;
/// Slowest motion update interval accepted: 10 seconds.
pub const MAX_MOTION_INTERVAL_MS: u32 = 10_000;

/// Checks a motion update interval.
pub fn validate_motion_interval(interval_ms: u32) -> Result<(), String> {
    if !(MIN_MOTION_INTERVAL_MS..=MAX_MOTION_INTERVAL_MS).contains(&interval_ms) {
        return Err(format!(
            "a motion update interval is {MIN_MOTION_INTERVAL_MS} to {MAX_MOTION_INTERVAL_MS} ms, got {interval_ms}"
        ));
    }
    Ok(())
}

/// Checks a location distance filter (metres; `None` means every update).
pub fn validate_distance_filter(distance_m: Option<f64>) -> Result<(), String> {
    match distance_m {
        Some(d) if !d.is_finite() || d < 0.0 => Err(format!(
            "a distance filter is a positive number of metres, got {d}"
        )),
        _ => Ok(()),
    }
}

/// Mean Earth radius used by [`distance_m`], in metres.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance between two coordinates in degrees, in metres
/// (haversine; within 0.5% of the ellipsoid distance anywhere on Earth).
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().min(1.0).asin()
}

/// Thermal pressure levels, in the order the platform reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thermal {
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// Why heavy work (a model download, an import, on-device inference)
/// should wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocker {
    Thermal,
    LowPowerMode,
    LowBattery,
    LowMemory,
}

/// Battery level below which unplugged devices defer heavy work.
pub const LOW_BATTERY: f32 = 0.2;

/// The default policy for heavy work: wait when the device is hot
/// (serious or critical), in Low Power Mode, below 20% battery and not
/// charging, or when the app has less memory left than the work needs.
/// The first reason found is returned, in that order.
pub fn heavy_work_blocker(
    thermal: Thermal,
    low_power_mode: bool,
    battery_level: Option<f32>,
    on_power: bool,
    memory_available: Option<u64>,
    memory_needed: Option<u64>,
) -> Option<Blocker> {
    if thermal >= Thermal::Serious {
        return Some(Blocker::Thermal);
    }
    if low_power_mode {
        return Some(Blocker::LowPowerMode);
    }
    if !on_power && battery_level.is_some_and(|level| level < LOW_BATTERY) {
        return Some(Blocker::LowBattery);
    }
    if let (Some(available), Some(needed)) = (memory_available, memory_needed) {
        if available < needed {
            return Some(Blocker::LowMemory);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_intervals() {
        assert!(validate_motion_interval(10).is_ok());
        assert!(validate_motion_interval(10_000).is_ok());
        assert!(validate_motion_interval(9).is_err());
        assert!(validate_motion_interval(10_001).is_err());
    }

    #[test]
    fn distance_filters() {
        assert!(validate_distance_filter(None).is_ok());
        assert!(validate_distance_filter(Some(0.0)).is_ok());
        assert!(validate_distance_filter(Some(-1.0)).is_err());
        assert!(validate_distance_filter(Some(f64::NAN)).is_err());
    }

    #[test]
    fn distances() {
        assert_eq!(distance_m(51.5, -0.12, 51.5, -0.12), 0.0);
        // London to Paris is about 344 km.
        let d = distance_m(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((d - 343_500.0).abs() < 2_000.0, "{d}");
        // A quarter of the way round the equator.
        let q = distance_m(0.0, 0.0, 0.0, 90.0);
        assert!((q - EARTH_RADIUS_M * std::f64::consts::FRAC_PI_2).abs() < 1.0);
        // Antipodes do not overflow asin.
        assert!(distance_m(0.0, 0.0, 0.0, 180.0).is_finite());
    }

    #[test]
    fn heavy_work_policy() {
        let ok = heavy_work_blocker(
            Thermal::Fair,
            false,
            Some(0.5),
            false,
            Some(4 << 30),
            Some(1 << 30),
        );
        assert_eq!(ok, None);
        assert_eq!(
            heavy_work_blocker(Thermal::Serious, false, Some(1.0), true, None, None),
            Some(Blocker::Thermal)
        );
        assert_eq!(
            heavy_work_blocker(Thermal::Nominal, true, Some(1.0), true, None, None),
            Some(Blocker::LowPowerMode)
        );
        assert_eq!(
            heavy_work_blocker(Thermal::Nominal, false, Some(0.1), false, None, None),
            Some(Blocker::LowBattery)
        );
        // Charging at 10% is fine; an unknown level is not treated as low.
        assert_eq!(
            heavy_work_blocker(Thermal::Nominal, false, Some(0.1), true, None, None),
            None
        );
        assert_eq!(
            heavy_work_blocker(Thermal::Nominal, false, None, false, None, None),
            None
        );
        assert_eq!(
            heavy_work_blocker(Thermal::Nominal, false, None, false, Some(100), Some(200)),
            Some(Blocker::LowMemory)
        );
    }
}
