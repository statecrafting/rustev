//! Unix seconds as an Aicortex valid-time instant in UTC.

use aicortex_types::{CivilDate, TimePoint, TimeValue, Zone, ZonedTime};
use rahi_types::UnixSeconds;

/// The civil date of `days` since 1970-01-01 (proleptic Gregorian), by the
/// days-to-civil algorithm of H. Hinnant.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        u32::try_from(m).unwrap_or(0),
        u32::try_from(d).unwrap_or(0),
    )
}

/// `at` as an exact UTC instant at second precision, or `None` outside the
/// years Aicortex represents (1 to 9999).
pub(crate) fn instant(at: UnixSeconds) -> Option<TimePoint> {
    let secs = i64::try_from(at.get()).ok()?;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let in_day = secs.rem_euclid(86_400);
    let date = CivilDate::ymd(
        u16::try_from(y).ok()?,
        u8::try_from(m).ok()?,
        u8::try_from(d).ok()?,
    )
    .ok()?;
    let time = ZonedTime::second(
        date,
        u8::try_from(in_day / 3_600).ok()?,
        u8::try_from(in_day % 3_600 / 60).ok()?,
        u8::try_from(in_day % 60).ok()?,
        Zone::offset(0).ok()?,
    )
    .ok()?;
    Some(TimePoint::exact(TimeValue::DateTime(time)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(20_744), (2026, 10, 18));
    }

    #[test]
    fn an_instant_is_utc_at_second_precision() {
        // 2026-10-21T12:34:56Z.
        let at = UnixSeconds::new(1_792_586_096);
        let expected = TimePoint::exact(TimeValue::DateTime(
            ZonedTime::second(
                CivilDate::ymd(2026, 10, 21).unwrap(),
                12,
                34,
                56,
                Zone::offset(0).unwrap(),
            )
            .unwrap(),
        ));
        assert_eq!(instant(at), Some(expected));
    }
}
