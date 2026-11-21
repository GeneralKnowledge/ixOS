//! The moon's phase, computed: no source is needed. The lit fraction uses
//! the low-precision formula from Jean Meeus, *Astronomical Algorithms*
//! (2nd ed., chapter 48: the phase angle from the moon's mean elongation and
//! the sun's and moon's mean anomalies), good to about one percent, which is
//! plenty for a weather page.
//!
//! Moonrise and moonset use the Astronomical Almanac's low-precision lunar
//! formulae (section D, "Low-precision formulas for the Moon's
//! coordinates": longitude to about 0.3°, latitude 0.2°, parallax 0.003°),
//! the standard altitude for the Moon from Meeus ch. 15
//! (h0 = 0.7275 π − 0°34′), and a scan of the local day in ten-minute
//! steps, so the times are good to a few minutes.

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

/// Days from `unix` to the next full moon (the next time the cycle passes
/// half way), found by stepping an hour at a time and then halving.
/// Returns Unix seconds of that moment.
pub(crate) fn next_full_moon(unix: i64) -> i64 {
    let past_full = |t: i64| moon_phase(t).cycle_fraction >= 0.5;
    let mut before = unix;
    // Already past half way: first get back below it (the next new moon).
    while past_full(before) {
        before += 3_600;
    }
    let mut after = before;
    while !past_full(after) {
        before = after;
        after += 3_600;
    }
    while after - before > 60 {
        let mid = before + (after - before) / 2;
        if past_full(mid) {
            after = mid;
        } else {
            before = mid;
        }
    }
    after
}

/// The Moon's geocentric right ascension and declination (degrees) and
/// horizontal parallax (degrees) at Julian Day `jd`.
fn moon_position(jd: f64) -> (f64, f64, f64) {
    let t = (jd - J2000_JD) / 36_525.0;
    let s = |a: f64, b: f64| (a + b * t).to_radians().sin();
    let c = |a: f64, b: f64| (a + b * t).to_radians().cos();
    let lambda = 218.32 + 481_267.881 * t + 6.29 * s(135.0, 477_198.87)
        - 1.27 * s(259.3, -413_335.36)
        + 0.66 * s(235.7, 890_534.22)
        + 0.21 * s(269.9, 954_397.74)
        - 0.19 * s(357.5, 35_999.05)
        - 0.11 * s(186.5, 966_404.03);
    let beta = 5.13 * s(93.3, 483_202.02) + 0.28 * s(228.2, 960_400.89)
        - 0.28 * s(318.3, 6_003.15)
        - 0.17 * s(217.6, -407_332.21);
    let parallax = 0.9508
        + 0.0518 * c(135.0, 477_198.87)
        + 0.0095 * c(259.3, -413_335.36)
        + 0.0078 * c(235.7, 890_534.22)
        + 0.0028 * c(269.9, 954_397.74);
    let eps = (23.439_3 - 0.013_0 * t).to_radians();
    let (l, b) = (lambda.to_radians(), beta.to_radians());
    let ra = (l.sin() * eps.cos() - b.tan() * eps.sin())
        .atan2(l.cos())
        .to_degrees();
    let dec = (b.sin() * eps.cos() + b.cos() * eps.sin() * l.sin())
        .asin()
        .to_degrees();
    (ra, dec, parallax)
}

/// How far the Moon's centre is above the altitude at which it rises or
/// sets, in degrees, at `unix` for an observer at `latitude`, `longitude`.
fn moon_above_horizon(unix: i64, latitude: f64, longitude: f64) -> f64 {
    let jd = unix as f64 / 86_400.0 + UNIX_EPOCH_JD;
    let (ra, dec, parallax) = moon_position(jd);
    // Greenwich mean sidereal time, degrees (Meeus 12.4).
    let gmst = 280.460_618_37 + 360.985_647_366_29 * (jd - J2000_JD);
    let hour_angle = (gmst + longitude - ra).to_radians();
    let (phi, delta) = (latitude.to_radians(), dec.to_radians());
    let altitude = (phi.sin() * delta.sin() + phi.cos() * delta.cos() * hour_angle.cos())
        .asin()
        .to_degrees();
    altitude - (0.7275 * parallax - 34.0 / 60.0)
}

/// Moonrise and moonset in the local day starting at `day_start_unix`
/// (Unix seconds of local midnight), as Unix seconds. Either is `None` on
/// a day the Moon doesn't rise or set (it happens about once a month).
pub(crate) fn moon_rise_set(
    day_start_unix: i64,
    latitude: f64,
    longitude: f64,
) -> (Option<i64>, Option<i64>) {
    const STEP: i64 = 600;
    let (mut rise, mut set) = (None, None);
    let mut t = day_start_unix;
    let mut h = moon_above_horizon(t, latitude, longitude);
    while t < day_start_unix + 86_400 {
        let next = t + STEP;
        let hn = moon_above_horizon(next, latitude, longitude);
        if h.signum() != hn.signum() {
            // Straight-line crossing within the step.
            let at = t + ((h / (h - hn)) * STEP as f64).round() as i64;
            if h < 0.0 && rise.is_none() {
                rise = Some(at);
            } else if h >= 0.0 && set.is_none() {
                set = Some(at);
            }
        }
        t = next;
        h = hn;
    }
    (rise, set)
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
    fn the_next_full_moon() {
        // Full moon 2024-01-25 17:54 UTC, then 2024-02-24 12:30 UTC.
        let found = next_full_moon(utc(2024, 1, 14, 12, 0));
        assert!(
            (found - utc(2024, 1, 25, 17, 54)).abs() < 3 * 3_600,
            "{found}"
        );
        let found = next_full_moon(utc(2024, 1, 26, 0, 0));
        assert!(
            (found - utc(2024, 2, 24, 12, 30)).abs() < 3 * 3_600,
            "{found}"
        );
    }

    #[test]
    fn moonrise_and_moonset() {
        // Bathgate, Scotland, 2026-11-21 (UTC = local time): moonrise about
        // 14:07, as the phone's own Weather app showed that afternoon.
        let (rise, set) = moon_rise_set(utc(2026, 11, 21, 0, 0), 55.902, -3.643);
        let rise = rise.expect("the moon rises that day");
        assert!(
            (rise - utc(2026, 11, 21, 14, 7)).abs() <= 10 * 60,
            "rise {rise}"
        );
        let set = set.expect("and sets");
        // A waxing gibbous moon sets in the early morning.
        assert!(set < utc(2026, 11, 21, 9, 0), "set {set}");
        // And the next full moon is three days on, as the phone said.
        let full = next_full_moon(utc(2026, 11, 21, 14, 0));
        let days = (full - utc(2026, 11, 21, 14, 0)) as f64 / 86_400.0;
        assert!((2.5..3.6).contains(&days), "{days}");
    }

    #[test]
    fn waxing_and_waning_between() {
        let waxing = moon_phase(utc(2024, 1, 14, 12, 0));
        assert_eq!(waxing.name, MoonPhaseName::WaxingCrescent);
        let waning = moon_phase(utc(2024, 1, 29, 12, 0));
        assert_eq!(waning.name, MoonPhaseName::WaningGibbous);
    }
}
