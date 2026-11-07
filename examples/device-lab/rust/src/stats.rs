//! Device Lab's own logic over the framework's live values: rolling
//! histories, measured update rates, peaks, distance travelled and the
//! connectivity log. Pure, no I/O, tested below.

use std::collections::VecDeque;

/// Samples kept for each sparkline: 6 seconds at 20 Hz.
pub const HISTORY: usize = 120;
/// Connectivity changes kept.
pub const CHANGE_LOG: usize = 20;
/// Fixes less accurate than this (metres) are shown but not counted towards
/// the distance travelled.
pub const TRIP_ACCURACY_M: f64 = 50.0;

/// A rolling series of `(timestamp_s, value)`, for a sparkline and the
/// measured update rate.
#[derive(Clone, Debug, Default)]
pub struct Series {
    samples: VecDeque<(f64, f64)>,
    peak: Option<f64>,
    min: Option<f64>,
    count: u64,
}

impl Series {
    pub fn push(&mut self, timestamp_s: f64, value: f64) {
        if self.samples.len() == HISTORY {
            self.samples.pop_front();
        }
        self.samples.push_back((timestamp_s, value));
        self.peak = Some(self.peak.map_or(value, |p| p.max(value)));
        self.min = Some(self.min.map_or(value, |m| m.min(value)));
        self.count += 1;
    }

    pub fn latest(&self) -> Option<f64> {
        self.samples.back().map(|(_, v)| *v)
    }

    /// Highest value since the last reset.
    pub fn peak(&self) -> Option<f64> {
        self.peak
    }

    /// Lowest value since the last reset.
    pub fn min(&self) -> Option<f64> {
        self.min
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// The values, oldest first.
    pub fn values(&self) -> Vec<f64> {
        self.samples.iter().map(|(_, v)| *v).collect()
    }

    /// Updates per second over the last two seconds of samples (0 with
    /// fewer than two).
    pub fn rate_hz(&self) -> f64 {
        let Some(&(last, _)) = self.samples.back() else {
            return 0.0;
        };
        let window: Vec<f64> = self
            .samples
            .iter()
            .rev()
            .map(|(t, _)| *t)
            .take_while(|t| last - t <= 2.0)
            .collect();
        let first = *window.last().unwrap_or(&last);
        if window.len() < 2 || last <= first {
            return 0.0;
        }
        (window.len() - 1) as f64 / (last - first)
    }

    /// Forgets the peak and minimum; keeps the history.
    pub fn reset_extremes(&mut self) {
        self.peak = self.latest();
        self.min = self.latest();
    }
}

/// Degrees the screen is tilted away from lying flat, face up (0) to
/// standing upright (90) to face down (180), from the gravity vector.
pub fn tilt_degrees(gx: f64, gy: f64, gz: f64) -> Option<f64> {
    let g = (gx * gx + gy * gy + gz * gz).sqrt();
    if g < 1e-6 {
        return None;
    }
    Some((-gz / g).clamp(-1.0, 1.0).acos().to_degrees())
}

/// Distance travelled over accurate fixes.
#[derive(Clone, Debug, Default)]
pub struct Trip {
    last: Option<(f64, f64, f64)>,
    pub distance_m: f64,
    pub max_speed_mps: Option<f64>,
    pub fixes: u64,
}

impl Trip {
    /// Adds a fix. Movements smaller than the two fixes' combined accuracy
    /// are GPS noise and do not count, and the last counted point stays put
    /// so slow walking still adds up.
    pub fn add(&mut self, lat: f64, lon: f64, accuracy_m: f64, speed_mps: Option<f64>) {
        self.fixes += 1;
        if let Some(speed) = speed_mps.filter(|s| *s >= 0.0) {
            self.max_speed_mps = Some(self.max_speed_mps.map_or(speed, |m| m.max(speed)));
        }
        if !(0.0..=TRIP_ACCURACY_M).contains(&accuracy_m) {
            return;
        }
        match self.last {
            None => self.last = Some((lat, lon, accuracy_m)),
            Some((plat, plon, pacc)) => {
                let d = forge_services::sensors::distance_m(plat, plon, lat, lon);
                if d > accuracy_m + pacc {
                    self.distance_m += d;
                    self.last = Some((lat, lon, accuracy_m));
                }
            }
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A bounded log, newest first, that skips a value equal to the newest.
#[derive(Clone, Debug)]
pub struct ChangeLog<T> {
    entries: VecDeque<(i64, T)>,
}

impl<T> Default for ChangeLog<T> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }
}

impl<T: Clone> ChangeLog<T> {
    /// Records `value` at `at_ms` unless `same(newest, value)`. Returns
    /// whether it was recorded.
    pub fn record(&mut self, at_ms: i64, value: T, same: impl Fn(&T, &T) -> bool) -> bool {
        if let Some((_, newest)) = self.entries.front() {
            if same(newest, &value) {
                return false;
            }
        }
        if self.entries.len() == CHANGE_LOG {
            self.entries.pop_back();
        }
        self.entries.push_front((at_ms, value));
        true
    }

    pub fn entries(&self) -> Vec<(i64, T)> {
        self.entries.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_rate_peak_and_history() {
        let mut s = Series::default();
        assert_eq!(s.rate_hz(), 0.0);
        for i in 0..200 {
            s.push(i as f64 * 0.05, (i % 10) as f64);
        }
        assert_eq!(s.values().len(), HISTORY);
        assert!((s.rate_hz() - 20.0).abs() < 0.01, "{}", s.rate_hz());
        assert_eq!(s.peak(), Some(9.0));
        assert_eq!(s.min(), Some(0.0));
        assert_eq!(s.count(), 200);
        s.reset_extremes();
        assert_eq!(s.peak(), s.latest());
        s.push(100.0, 50.0);
        assert_eq!(s.peak(), Some(50.0));
        // A pause longer than the window: the rate falls to what is left.
        s.push(110.0, 1.0);
        assert_eq!(s.rate_hz(), 0.0);
    }

    #[test]
    fn tilt() {
        assert_eq!(tilt_degrees(0.0, 0.0, -1.0), Some(0.0));
        assert!((tilt_degrees(0.0, -1.0, 0.0).unwrap() - 90.0).abs() < 1e-9);
        assert!((tilt_degrees(0.0, 0.0, 1.0).unwrap() - 180.0).abs() < 1e-9);
        assert_eq!(tilt_degrees(0.0, 0.0, 0.0), None);
    }

    #[test]
    fn trip_ignores_noise_and_inaccurate_fixes() {
        let mut trip = Trip::default();
        trip.add(51.5, -0.12, 5.0, Some(1.0));
        // 3 m of jitter with 5 m accuracy: noise.
        trip.add(51.50002, -0.12, 5.0, Some(0.2));
        assert_eq!(trip.distance_m, 0.0);
        // 1 km away but 500 m accuracy: not counted.
        trip.add(51.509, -0.12, 500.0, None);
        assert_eq!(trip.distance_m, 0.0);
        // About 111 m north, accurately.
        trip.add(51.501, -0.12, 5.0, Some(3.0));
        assert!((trip.distance_m - 111.2).abs() < 1.0, "{}", trip.distance_m);
        assert_eq!(trip.fixes, 4);
        assert_eq!(trip.max_speed_mps, Some(3.0));
        trip.add(51.501, -0.12, 5.0, Some(-1.0));
        assert_eq!(
            trip.max_speed_mps,
            Some(3.0),
            "negative speed means unknown"
        );
        trip.reset();
        assert_eq!(trip.fixes, 0);
    }

    #[test]
    fn change_log_skips_repeats_and_is_bounded() {
        let mut log = ChangeLog::default();
        assert!(log.record(1, "wifi", |a, b| a == b));
        assert!(!log.record(2, "wifi", |a, b| a == b));
        assert!(log.record(3, "cellular", |a, b| a == b));
        assert_eq!(log.entries(), vec![(3, "cellular"), (1, "wifi")]);
        for i in 0..50 {
            log.record(10 + i, if i % 2 == 0 { "a" } else { "b" }, |a, b| a == b);
        }
        assert_eq!(log.entries().len(), CHANGE_LOG);
        assert_eq!(log.entries()[0].0, 59);
    }
}
