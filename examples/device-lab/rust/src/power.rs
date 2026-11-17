//! Battery drain and heat over time: what an AI Lab run or a big Relay
//! transfer actually costs. Samples come from the device status Device Lab
//! already reads, at most every [`SAMPLE_MS`], whichever tab or hosted app
//! is on screen, as long as Device Lab is in the foreground.

use std::collections::VecDeque;

use crate::native::device_status::{DeviceStatus, ThermalState};
use crate::stats::ChangeLog;

/// How often a sample is kept.
pub const SAMPLE_MS: i64 = 30_000;
/// Samples kept: two hours at one every 30 seconds.
pub const MAX_SAMPLES: usize = 240;
/// A drain rate needs this long on battery, so 1% steps don't swamp it.
pub const MIN_DRAIN_MS: i64 = 10 * 60_000;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    at_ms: i64,
    /// 0 to 100.
    percent: f64,
    on_power: bool,
}

/// One change of the phone's thermal state.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ThermalChange {
    pub at_ms: i64,
    pub state: ThermalState,
}

/// What the Battery & Power section shows about the time since launch.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PowerStats {
    /// Percent an hour since the phone was last unplugged (or since launch),
    /// once it has been on battery for 10 minutes. `None` on power or
    /// before then.
    pub drain_percent_per_hour: Option<f64>,
    /// How long the current stretch on battery has lasted, in minutes.
    pub on_battery_minutes: f64,
    /// Battery level samples, oldest first, for a sparkline (0 to 100).
    pub levels: Vec<f64>,
    /// Thermal state changes, newest first.
    pub thermal_changes: Vec<ThermalChange>,
}

#[derive(Default)]
pub struct PowerLog {
    samples: VecDeque<Sample>,
    thermal: ChangeLog<ThermalState>,
}

impl PowerLog {
    /// Whether a new sample is due at `now_ms`.
    pub fn due(&self, now_ms: i64) -> bool {
        self.samples
            .back()
            .is_none_or(|last| now_ms - last.at_ms >= SAMPLE_MS)
    }

    /// Records a status. The thermal state is logged on every change; the
    /// battery level when a sample is due or the charger was plugged in or
    /// out.
    pub fn record(&mut self, status: &DeviceStatus) {
        let at_ms = status.observed_at_ms;
        self.thermal
            .record(at_ms, status.thermal_state, |a, b| a == b);
        let Some(level) = status.battery_level else {
            return;
        };
        let on_power = status.is_on_power();
        let plug_changed = self.samples.back().is_some_and(|s| s.on_power != on_power);
        if !self.due(at_ms) && !plug_changed {
            return;
        }
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            at_ms,
            percent: f64::from(level) * 100.0,
            on_power,
        });
    }

    pub fn stats(&self) -> PowerStats {
        // The current stretch on battery: the newest samples back to the
        // last one on power.
        let stretch: Vec<&Sample> = self
            .samples
            .iter()
            .rev()
            .take_while(|s| !s.on_power)
            .collect();
        let (first, last) = (stretch.last(), stretch.first());
        let span_ms = match (first, last) {
            (Some(first), Some(last)) => last.at_ms - first.at_ms,
            _ => 0,
        };
        let drain = match (first, last) {
            (Some(first), Some(last)) if span_ms >= MIN_DRAIN_MS => {
                let hours = span_ms as f64 / 3_600_000.0;
                Some(((first.percent - last.percent) / hours).max(0.0))
            }
            _ => None,
        };
        PowerStats {
            drain_percent_per_hour: drain,
            on_battery_minutes: span_ms as f64 / 60_000.0,
            levels: self.samples.iter().map(|s| s.percent).collect(),
            thermal_changes: self
                .thermal
                .entries()
                .into_iter()
                .map(|(at_ms, state)| ThermalChange { at_ms, state })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::device_status::BatteryState;

    fn status(at_min: i64, level: f32, charging: bool, thermal: ThermalState) -> DeviceStatus {
        DeviceStatus {
            battery_level: Some(level),
            battery_state: if charging {
                BatteryState::Charging
            } else {
                BatteryState::Unplugged
            },
            low_power_mode: false,
            thermal_state: thermal,
            storage_total_bytes: None,
            storage_available_bytes: None,
            memory_available_bytes: None,
            memory_footprint_bytes: None,
            uptime_seconds: 0.0,
            screen_brightness: None,
            observed_at_ms: at_min * 60_000,
        }
    }

    #[test]
    fn drain_is_measured_over_the_stretch_on_battery() {
        let mut log = PowerLog::default();
        log.record(&status(0, 0.90, true, ThermalState::Nominal));
        // Unplugged at minute 1, then 6% gone in 30 minutes: 12%/h.
        for minute in 1..=31 {
            let level = 0.90 - 0.002 * (minute - 1) as f32;
            log.record(&status(minute, level, false, ThermalState::Nominal));
        }
        let stats = log.stats();
        assert!((stats.on_battery_minutes - 30.0).abs() < 1e-9);
        let drain = stats.drain_percent_per_hour.expect("30 minutes is enough");
        assert!((drain - 12.0).abs() < 0.1, "{drain}");
    }

    #[test]
    fn no_rate_before_ten_minutes_or_while_charging() {
        let mut log = PowerLog::default();
        log.record(&status(0, 0.5, false, ThermalState::Nominal));
        log.record(&status(5, 0.49, false, ThermalState::Nominal));
        assert_eq!(log.stats().drain_percent_per_hour, None);
        log.record(&status(20, 0.47, true, ThermalState::Nominal));
        assert_eq!(log.stats().drain_percent_per_hour, None);
        assert_eq!(log.stats().on_battery_minutes, 0.0);
    }

    #[test]
    fn samples_are_kept_every_thirty_seconds_and_on_plug_changes() {
        let mut log = PowerLog::default();
        let mut s = status(0, 0.5, false, ThermalState::Nominal);
        log.record(&s);
        s.observed_at_ms = 10_000;
        log.record(&s);
        assert_eq!(log.stats().levels.len(), 1);
        s.battery_state = BatteryState::Charging;
        log.record(&s);
        assert_eq!(log.stats().levels.len(), 2);
        assert!(!log.due(20_000));
        assert!(log.due(40_000));
    }

    #[test]
    fn thermal_changes_are_logged_newest_first() {
        let mut log = PowerLog::default();
        log.record(&status(0, 0.5, false, ThermalState::Nominal));
        log.record(&status(1, 0.5, false, ThermalState::Nominal));
        log.record(&status(2, 0.5, false, ThermalState::Fair));
        let changes = log.stats().thermal_changes;
        assert_eq!(
            changes.iter().map(|c| c.state).collect::<Vec<_>>(),
            vec![ThermalState::Fair, ThermalState::Nominal]
        );
    }
}
