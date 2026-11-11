//! What makes a good track: which location fixes to keep, and the totals a
//! trip shows. Pure functions, no storage, so they test on Linux.

use forge_services::sensors::distance_m;

/// A fix worse than this (its horizontal accuracy, in metres) is dropped.
/// Core Location reports 65 m for Wi-Fi fixes and 5 to 20 m outdoors with GPS.
pub const MAX_ACCURACY_M: f64 = 50.0;

/// A fix closer than this to the last kept one adds nothing but noise.
pub const MIN_STEP_M: f64 = 3.0;

/// Faster than this (metres per second, about 290 km/h) is a GPS jump, not
/// travel. FieldKit is for walks, rides and drives, not flights.
pub const MAX_SPEED_MPS: f64 = 80.0;

/// Altitude changes smaller than this are not counted as climbing: GPS
/// altitude wanders by a few metres even when standing still.
pub const CLIMB_STEP_M: f64 = 3.0;

/// One location fix, as Core Location delivers it.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct LocationSample {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above sea level, if the fix has one.
    pub altitude_m: Option<f64>,
    /// The radius of uncertainty, in metres. Negative means invalid.
    pub horizontal_accuracy_m: f64,
    /// When the fix was taken, in milliseconds since 1970.
    pub timestamp_ms: i64,
}

/// A fix kept in a trip's track.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct TrackPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
    pub horizontal_accuracy_m: f64,
    pub timestamp_ms: i64,
}

impl From<LocationSample> for TrackPoint {
    fn from(s: LocationSample) -> Self {
        Self {
            latitude: s.latitude,
            longitude: s.longitude,
            altitude_m: s.altitude_m,
            horizontal_accuracy_m: s.horizontal_accuracy_m,
            timestamp_ms: s.timestamp_ms,
        }
    }
}

/// Why a fix was not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Not a place on Earth (NaN, or latitude beyond ±90).
    Invalid,
    /// Less accurate than [`MAX_ACCURACY_M`], or accuracy unknown.
    Inaccurate,
    /// Not later than the last kept fix (Core Location can repeat one).
    Stale,
    /// Within [`MIN_STEP_M`] of the last kept fix.
    TooClose,
    /// Further from the last kept fix than [`MAX_SPEED_MPS`] allows.
    Jump,
}

/// Decides whether `sample` belongs in a track whose last kept fix is
/// `last`.
pub fn check(last: Option<&TrackPoint>, sample: &LocationSample) -> Result<(), Skip> {
    let valid = sample.latitude.is_finite()
        && sample.longitude.is_finite()
        && (-90.0..=90.0).contains(&sample.latitude)
        && (-180.0..=180.0).contains(&sample.longitude);
    if !valid {
        return Err(Skip::Invalid);
    }
    if !(0.0..=MAX_ACCURACY_M).contains(&sample.horizontal_accuracy_m) {
        return Err(Skip::Inaccurate);
    }
    let Some(last) = last else {
        return Ok(());
    };
    if sample.timestamp_ms <= last.timestamp_ms {
        return Err(Skip::Stale);
    }
    let step = step_m(last, sample);
    if step < MIN_STEP_M {
        return Err(Skip::TooClose);
    }
    let seconds = (sample.timestamp_ms - last.timestamp_ms) as f64 / 1000.0;
    if step / seconds > MAX_SPEED_MPS {
        return Err(Skip::Jump);
    }
    Ok(())
}

fn step_m(last: &TrackPoint, sample: &LocationSample) -> f64 {
    distance_m(
        last.latitude,
        last.longitude,
        sample.latitude,
        sample.longitude,
    )
}

/// A track's running totals. Kept with the trip and updated as points
/// arrive, so a long trip never has to be re-read to show them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub distance_m: f64,
    pub climb_m: f64,
    pub point_count: u32,
    /// The altitude climbing is measured from: the last altitude that
    /// moved by at least [`CLIMB_STEP_M`].
    pub climb_base_m: Option<f64>,
}

impl Totals {
    /// Adds a kept point that follows `last`.
    pub fn add(&mut self, last: Option<&TrackPoint>, point: &TrackPoint) {
        if let Some(last) = last {
            self.distance_m += distance_m(
                last.latitude,
                last.longitude,
                point.latitude,
                point.longitude,
            );
        }
        if let Some(altitude) = point.altitude_m {
            match self.climb_base_m {
                None => self.climb_base_m = Some(altitude),
                Some(base) if (altitude - base).abs() >= CLIMB_STEP_M => {
                    if altitude > base {
                        self.climb_m += altitude - base;
                    }
                    self.climb_base_m = Some(altitude);
                }
                Some(_) => {}
            }
        }
        self.point_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(lat: f64, lon: f64, t_s: i64) -> LocationSample {
        LocationSample {
            latitude: lat,
            longitude: lon,
            altitude_m: None,
            horizontal_accuracy_m: 10.0,
            timestamp_ms: t_s * 1000,
        }
    }

    // 0.0001 degrees of latitude is about 11.1 m.
    const STEP: f64 = 0.0001;

    #[test]
    fn first_good_fix_is_kept() {
        assert_eq!(check(None, &sample(51.5, -0.12, 0)), Ok(()));
    }

    #[test]
    fn bad_fixes_are_skipped() {
        let mut s = sample(f64::NAN, 0.0, 0);
        assert_eq!(check(None, &s), Err(Skip::Invalid));
        s = sample(91.0, 0.0, 0);
        assert_eq!(check(None, &s), Err(Skip::Invalid));
        s = sample(51.5, 0.0, 0);
        s.horizontal_accuracy_m = -1.0;
        assert_eq!(check(None, &s), Err(Skip::Inaccurate));
        s.horizontal_accuracy_m = 65.0;
        assert_eq!(check(None, &s), Err(Skip::Inaccurate));
    }

    #[test]
    fn stale_close_and_jumping_fixes_are_skipped() {
        let last = TrackPoint::from(sample(51.5, -0.12, 10));
        assert_eq!(
            check(Some(&last), &sample(51.5 + STEP, -0.12, 10)),
            Err(Skip::Stale)
        );
        assert_eq!(
            check(Some(&last), &sample(51.50001, -0.12, 11)),
            Err(Skip::TooClose)
        );
        // About 1.1 km in one second.
        assert_eq!(
            check(Some(&last), &sample(51.51, -0.12, 11)),
            Err(Skip::Jump)
        );
        // About 11 m in five seconds: a walk.
        assert_eq!(check(Some(&last), &sample(51.5 + STEP, -0.12, 15)), Ok(()));
    }

    #[test]
    fn totals_add_up_distance_and_climb() {
        let mut totals = Totals::default();
        let mut last: Option<TrackPoint> = None;
        // Up 10 m, down 5, up 1 (ignored: under CLIMB_STEP_M), up 4.
        for (i, alt) in [100.0, 110.0, 105.0, 106.0, 109.0].into_iter().enumerate() {
            let mut s = sample(51.5 + STEP * i as f64, -0.12, i as i64 * 10);
            s.altitude_m = Some(alt);
            let point = TrackPoint::from(s);
            totals.add(last.as_ref(), &point);
            last = Some(point);
        }
        assert_eq!(totals.point_count, 5);
        assert!(
            (totals.distance_m - 44.5).abs() < 0.5,
            "{}",
            totals.distance_m
        );
        assert_eq!(totals.climb_m, 14.0);
    }
}
