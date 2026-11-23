//! Dates from Unix time without a date crate (app-friction-log A6, A54):
//! what an export format, a "local day" or a daily time across a clock
//! change needs, and nothing else. Formatting for
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

/// Milliseconds in a day of 24 hours.
pub const DAY_MS: i64 = 86_400_000;

/// A change of the device's offset from UTC (a daylight saving clock
/// change), as the host's time zone predicts it. On iOS,
/// `TimeZone.nextDaylightSavingTimeTransition(after:)` lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetChange {
    /// When it happens, Unix milliseconds.
    pub at_ms: i64,
    /// The offset from then on, in minutes east of UTC.
    pub utc_offset_minutes: i32,
}

/// The local day (days since 1970-01-01 on the local calendar) that Unix
/// milliseconds `ms` fall on, `utc_offset_minutes` east of UTC. Midnight
/// belongs to the day it starts.
pub fn local_day(ms: i64, utc_offset_minutes: i32) -> i64 {
    (ms + i64::from(utc_offset_minutes) * 60_000).div_euclid(DAY_MS)
}

/// The Unix milliseconds of `minute_of_day` past local midnight on local
/// `day`, by the offset in force then: `now_offset_minutes` (the offset at
/// `now_ms`), or the last of `changes` after now and before that moment.
///
/// A local time the clocks skip (01:30 when they go forward at 01:00) falls
/// just after the change, never before it; a time they repeat (going back)
/// is the first of the two. With no `changes` every day uses today's
/// offset, which puts a time after a clock change an hour early or late.
pub fn local_time_ms(
    day: i64,
    minute_of_day: u32,
    now_ms: i64,
    now_offset_minutes: i32,
    changes: &[OffsetChange],
) -> i64 {
    let local = day * DAY_MS + i64::from(minute_of_day) * 60_000;
    // The offsets in force from now on, as UTC segments: [from, to) at offset.
    let mut ahead: Vec<OffsetChange> = changes
        .iter()
        .copied()
        .filter(|c| c.at_ms > now_ms)
        .collect();
    ahead.sort_by_key(|c| c.at_ms);
    let mut segments = Vec::with_capacity(ahead.len() + 1);
    let mut from = i64::MIN;
    let mut offset = now_offset_minutes;
    for change in &ahead {
        segments.push((from, change.at_ms, offset));
        from = change.at_ms;
        offset = change.utc_offset_minutes;
    }
    segments.push((from, i64::MAX, offset));
    // Each segment whose offset puts this local time inside it is a real
    // occurrence; a repeated time has two, and the first is the one.
    let first = segments
        .iter()
        .map(|&(from, to, offset)| (from, to, local - i64::from(offset) * 60_000))
        .filter(|&(from, to, utc)| from <= utc && utc < to)
        .map(|(_, _, utc)| utc)
        .min();
    if let Some(utc) = first {
        return utc;
    }
    // A time the clocks skip: by the offset before the skip, which lands it
    // just after the change.
    for pair in segments.windows(2) {
        let (before, after) = (pair[0].2, pair[1].2);
        let at = pair[1].0;
        let gap_start = at + i64::from(before) * 60_000;
        let gap_end = at + i64::from(after) * 60_000;
        if gap_start <= local && local < gap_end {
            return local - i64::from(before) * 60_000;
        }
    }
    local - i64::from(now_offset_minutes) * 60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    // London, 2027: GMT (0) until 28 March 01:00 UTC, then BST (+60) until
    // 31 October 01:00 UTC.
    const SPRING: i64 = 1_806_195_600_000; // 2027-03-28T01:00:00Z
    const AUTUMN: i64 = 1_824_944_400_000; // 2027-10-31T01:00:00Z

    #[test]
    fn the_change_moments_are_what_the_tests_say() {
        assert_eq!(utc_iso8601(SPRING), "2027-03-28T01:00:00Z");
        assert_eq!(utc_iso8601(AUTUMN), "2027-10-31T01:00:00Z");
    }

    #[test]
    fn local_days_turn_at_local_midnight() {
        let day = SPRING.div_euclid(DAY_MS);
        assert_eq!(local_day(day * DAY_MS, 0), day);
        assert_eq!(local_day(day * DAY_MS - 1, 0), day - 1);
        // 23:30 UTC is already the next day an hour east, still today west.
        let late = day * DAY_MS + 23 * 3_600_000 + 30 * 60_000;
        assert_eq!(local_day(late, 60), day + 1);
        assert_eq!(local_day(late, -60), day);
        assert_eq!(local_day(-1, 0), -1);
    }

    #[test]
    fn a_time_after_a_change_keeps_its_local_time() {
        let change = [OffsetChange {
            at_ms: SPRING,
            utc_offset_minutes: 60,
        }];
        let day = SPRING.div_euclid(DAY_MS) + 1; // Monday 29 March
        let now = SPRING - 2 * DAY_MS;
        // 09:00 BST is 08:00 UTC; today's offset alone would say 09:00 UTC.
        let at = local_time_ms(day, 9 * 60, now, 0, &change);
        assert_eq!(utc_iso8601(at), "2027-03-29T08:00:00Z");
        assert_eq!(
            utc_iso8601(local_time_ms(day, 9 * 60, now, 0, &[])),
            "2027-03-29T09:00:00Z"
        );
        // Before the change, nothing moves.
        let saturday = day - 2;
        assert_eq!(
            utc_iso8601(local_time_ms(saturday, 9 * 60, now, 0, &change)),
            "2027-03-27T09:00:00Z"
        );
        // Going back in autumn.
        let back = [OffsetChange {
            at_ms: AUTUMN,
            utc_offset_minutes: 0,
        }];
        let monday = AUTUMN.div_euclid(DAY_MS) + 1;
        let at = local_time_ms(monday, 9 * 60, AUTUMN - DAY_MS, 60, &back);
        assert_eq!(utc_iso8601(at), "2027-11-01T09:00:00Z");
    }

    #[test]
    fn a_repeated_time_is_the_first_even_with_both_changes_listed() {
        let both = [
            OffsetChange {
                at_ms: SPRING,
                utc_offset_minutes: 60,
            },
            OffsetChange {
                at_ms: AUTUMN,
                utc_offset_minutes: 0,
            },
        ];
        let day = AUTUMN.div_euclid(DAY_MS);
        // 01:30 happens at 00:30 UTC (BST) and again at 01:30 UTC (GMT).
        let at = local_time_ms(day, 90, SPRING - DAY_MS, 0, &both);
        assert_eq!(at, AUTUMN - 1_800_000);
        // The spring skip still lands after its change with both listed.
        let spring = local_time_ms(SPRING.div_euclid(DAY_MS), 90, SPRING - DAY_MS, 0, &both);
        assert_eq!(utc_iso8601(spring), "2027-03-28T01:30:00Z");
    }

    #[test]
    fn a_skipped_time_falls_just_after_the_change() {
        let change = [OffsetChange {
            at_ms: SPRING,
            utc_offset_minutes: 60,
        }];
        let day = SPRING.div_euclid(DAY_MS);
        let at = local_time_ms(day, 90, SPRING - DAY_MS, 0, &change); // 01:30
        assert!(at >= SPRING, "{}", utc_iso8601(at));
        assert_eq!(utc_iso8601(at), "2027-03-28T01:30:00Z");
    }

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
