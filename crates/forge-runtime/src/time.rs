//! Dates from Unix time without a date crate (app-friction-log A6): what an
//! export format or a "local day" needs, and nothing else. Formatting for
//! people stays in Swift (`Date.FormatStyle`), which knows the locale.

/// Days since 1970-01-01 to a proleptic Gregorian `(year, month, day)`
/// (Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Unix milliseconds as UTC ISO 8601 (`2026-11-11T17:33:23Z`), as GPX,
/// GeoJSON, Atom and most APIs want it. Milliseconds are dropped.
pub fn utc_iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_772), (2026, 11, 15));
    }

    #[test]
    fn iso8601_in_utc() {
        assert_eq!(utc_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_iso8601(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(utc_iso8601(1_794_418_403_999), "2026-11-11T17:33:23Z");
        assert_eq!(utc_iso8601(-1_000), "1969-12-31T23:59:59Z");
    }
}
