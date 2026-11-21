//! The moon's phase, computed: no source is needed. The lit fraction uses
//! the low-precision formula from Jean Meeus, *Astronomical Algorithms*
//! (2nd ed., chapter 48: the phase angle from the moon's mean elongation and
//! the sun's and moon's mean anomalies), good to about one percent, which is
//! plenty for a weather page.

use super::types::{MoonPhase, MoonPhaseName};

/// Julian Day of the Unix epoch.
const UNIX_EPOCH_JD: f64 = 2_440_587.5;
/// Julian Day of J2000.0.
const J2000_JD: f64 = 2_451_545.0;

/// The moon at `unix` (seconds).
pub(crate) fn moon_phase(unix: i64) -> MoonPhase {
    let jd = unix as f64 / 86_400.0 + UNIX_EPOCH_JD;
    let t = (jd - J2000_JD) / 36_525.0;
    let deg = |d: f64| d.rem_euclid(360.0);
    // Mean elongation of the moon, mean anomaly of the sun, of the moon.
    let d = deg(297.850_192_1 + 445_267.111_403_4 * t);
    let m = deg(357.529_109_2 + 35_999.050_290_9 * t);
    let mp = deg(134.963_396_4 + 477_198.867_505_5 * t);
    let r = f64::to_radians;
    // Phase angle (Meeus 48.4).
    let i = 180.0 - d - 6.289 * r(mp).sin() + 2.100 * r(m).sin()
        - 1.274 * r(2.0 * d - mp).sin()
        - 0.658 * r(2.0 * d).sin()
        - 0.214 * r(2.0 * mp).sin()
        - 0.110 * r(d).sin();
    let i = deg(i);
    let lit = (1.0 + r(i).cos()) / 2.0;
    // Waxing while the elongation is under 180°.
    let elongation = deg(180.0 - i);
    let cycle_fraction = elongation / 360.0;
    MoonPhase {
        name: phase_name(cycle_fraction),
        illumination_pct: (lit * 100.0).round().clamp(0.0, 100.0) as u8,
        cycle_fraction,
    }
}

/// Eight equal parts of the cycle, each centred on its phase.
fn phase_name(fraction: f64) -> MoonPhaseName {
    use MoonPhaseName::*;
    const NAMES: [MoonPhaseName; 8] = [
        NewMoon,
        WaxingCrescent,
        FirstQuarter,
        WaxingGibbous,
        FullMoon,
        WaningGibbous,
        LastQuarter,
        WaningCrescent,
    ];
    NAMES[((fraction * 8.0).round() as usize) % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unix seconds for a UTC date and time.
    fn utc(y: i32, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
        // Days from civil (Howard Hinnant's algorithm).
        let y = if mo <= 2 { y - 1 } else { y } as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = mo as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400 + h * 3_600 + mi * 60
    }

    #[test]
    fn known_phases() {
        // Full moon 2024-01-25 17:54 UTC; new moon 2024-01-11 11:57 UTC;
        // first quarter 2024-01-18 03:53 UTC; last quarter 2024-02-02 23:18 UTC.
        let full = moon_phase(utc(2024, 1, 25, 17, 54));
        assert_eq!(full.name, MoonPhaseName::FullMoon);
        assert!(full.illumination_pct >= 99, "{full:?}");
        let new = moon_phase(utc(2024, 1, 11, 11, 57));
        assert_eq!(new.name, MoonPhaseName::NewMoon);
        assert!(new.illumination_pct <= 1, "{new:?}");
        let first = moon_phase(utc(2024, 1, 18, 3, 53));
        assert_eq!(first.name, MoonPhaseName::FirstQuarter);
        assert!((48..=52).contains(&first.illumination_pct), "{first:?}");
        let last = moon_phase(utc(2024, 2, 2, 23, 18));
        assert_eq!(last.name, MoonPhaseName::LastQuarter);
        assert!((48..=52).contains(&last.illumination_pct), "{last:?}");
    }

    #[test]
    fn waxing_and_waning_between() {
        let waxing = moon_phase(utc(2024, 1, 14, 12, 0));
        assert_eq!(waxing.name, MoonPhaseName::WaxingCrescent);
        let waning = moon_phase(utc(2024, 1, 29, 12, 0));
        assert_eq!(waning.name, MoonPhaseName::WaningGibbous);
    }
}
