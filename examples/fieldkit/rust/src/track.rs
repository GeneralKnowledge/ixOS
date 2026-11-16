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

/// The most points the elevation chart is given. A day's walk can keep
/// thousands of fixes; a phone screen shows a few hundred at most.
pub const MAX_PROFILE_POINTS: usize = 300;

/// A split shorter than this at the end of a track is left out: its pace
/// would be noise.
pub const MIN_LAST_SPLIT_M: f64 = 100.0;

/// One point of a track's elevation profile.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct ElevationPoint {
    /// How far along the track, in metres.
    pub distance_m: f64,
    pub altitude_m: f64,
}

/// One kilometre of a track (the last may be shorter) and how long it took.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct PaceSplit {
    /// 1 for the first kilometre.
    pub number: u32,
    /// The split's length: 1000 m, or less for the last.
    pub distance_m: f64,
    pub seconds: f64,
    /// Seconds per kilometre, so a short last split compares with the rest.
    pub seconds_per_km: f64,
}

/// What a trip's charts show: altitude over distance, and pace per
/// kilometre. Both are empty when the track has nothing to show.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub elevation: Vec<ElevationPoint>,
    pub splits: Vec<PaceSplit>,
}

/// The profile of a kept track, in order.
pub fn profile(track: &[TrackPoint]) -> Profile {
    let mut elevation = Vec::new();
    let mut splits = Vec::new();
    let mut along = 0.0;
    let mut split_start_m = 0.0;
    let mut split_start_ms = track.first().map(|p| p.timestamp_ms);
    for (i, point) in track.iter().enumerate() {
        if i > 0 {
            let last = &track[i - 1];
            let step = distance_m(
                last.latitude,
                last.longitude,
                point.latitude,
                point.longitude,
            );
            // Where along this step each kilometre was crossed, assuming a
            // steady speed between the two fixes.
            while along + step >= split_start_m + 1000.0 {
                let into = split_start_m + 1000.0 - along;
                let fraction = if step > 0.0 { into / step } else { 1.0 };
                let at_ms = last.timestamp_ms as f64
                    + fraction * (point.timestamp_ms - last.timestamp_ms) as f64;
                let start = split_start_ms.unwrap_or(last.timestamp_ms) as f64;
                splits.push(split(splits.len(), 1000.0, (at_ms - start) / 1000.0));
                split_start_m += 1000.0;
                split_start_ms = Some(at_ms.round() as i64);
            }
            along += step;
        }
        if let Some(altitude) = point.altitude_m {
            elevation.push(ElevationPoint {
                distance_m: along,
                altitude_m: altitude,
            });
        }
    }
    let rest = along - split_start_m;
    if let (Some(last), Some(start)) = (track.last(), split_start_ms) {
        if rest >= MIN_LAST_SPLIT_M {
            splits.push(split(
                splits.len(),
                rest,
                (last.timestamp_ms - start) as f64 / 1000.0,
            ));
        }
    }
    // One altitude is not a profile.
    if elevation.len() < 2 {
        elevation.clear();
    }
    Profile {
        elevation: thin(elevation, MAX_PROFILE_POINTS),
        splits,
    }
}

fn split(index: usize, distance_m: f64, seconds: f64) -> PaceSplit {
    PaceSplit {
        number: index as u32 + 1,
        distance_m,
        seconds,
        seconds_per_km: seconds * 1000.0 / distance_m,
    }
}

/// At most `max` points, evenly spread, always keeping the first and last.
fn thin(points: Vec<ElevationPoint>, max: usize) -> Vec<ElevationPoint> {
    if points.len() <= max || max < 2 {
        return points;
    }
    let last = points.len() - 1;
    (0..max).map(|i| points[i * last / (max - 1)]).collect()
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

    /// A straight walk north: `n` fixes `STEP` apart, `secs` seconds apart.
    fn walk(n: usize, secs: i64, altitude: impl Fn(usize) -> Option<f64>) -> Vec<TrackPoint> {
        (0..n)
            .map(|i| {
                let mut s = sample(51.5 + STEP * i as f64, -0.12, i as i64 * secs);
                s.altitude_m = altitude(i);
                TrackPoint::from(s)
            })
            .collect()
    }

    #[test]
    fn a_profile_has_altitude_over_distance_and_a_pace_per_kilometre() {
        // 300 fixes about 11.1 m apart (about 3.3 km), 10 s apart: about
        // 15 minutes a kilometre.
        let track = walk(300, 10, |i| Some(100.0 + i as f64 / 10.0));
        let p = profile(&track);
        assert_eq!(p.elevation.len(), 300);
        assert_eq!(p.elevation[0].distance_m, 0.0);
        assert!((p.elevation[299].distance_m - 3324.0).abs() < 10.0);
        assert_eq!(p.elevation[299].altitude_m, 129.9);
        // Three whole kilometres and a last split of about 320 m.
        assert_eq!(p.splits.len(), 4);
        assert_eq!(
            p.splits.iter().map(|s| s.number).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        for s in &p.splits[..3] {
            assert_eq!(s.distance_m, 1000.0);
            assert!((s.seconds_per_km - 900.0).abs() < 2.0, "{s:?}");
        }
        let last = p.splits[3];
        assert!((last.distance_m - 324.0).abs() < 10.0, "{last:?}");
        assert!((last.seconds_per_km - 900.0).abs() < 10.0, "{last:?}");
        // The splits add up to the whole walk.
        let total: f64 = p.splits.iter().map(|s| s.seconds).sum();
        assert!((total - 2990.0).abs() < 1.0, "{total}");
    }

    #[test]
    fn a_short_or_flat_track_has_nothing_to_chart() {
        assert_eq!(profile(&[]), Profile::default());
        // 50 m without altitude: no elevation, and too short for a split.
        let p = profile(&walk(5, 10, |_| None));
        assert!(p.elevation.is_empty());
        assert!(p.splits.is_empty());
        // One altitude is not a profile.
        let p = profile(&walk(5, 10, |i| (i == 2).then_some(100.0)));
        assert!(p.elevation.is_empty());
    }

    #[test]
    fn a_long_track_is_thinned_for_the_chart() {
        let track = walk(5000, 5, |i| Some(i as f64));
        let p = profile(&track);
        assert_eq!(p.elevation.len(), MAX_PROFILE_POINTS);
        assert_eq!(p.elevation[0].altitude_m, 0.0);
        assert_eq!(p.elevation[MAX_PROFILE_POINTS - 1].altitude_m, 4999.0);
        // About 55.6 km: 55 whole kilometres and a last split of about 600 m.
        assert_eq!(p.splits.len(), 56);
    }
}
