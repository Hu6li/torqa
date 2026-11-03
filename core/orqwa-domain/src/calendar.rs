//! Shared Gregorian calendar calculations, independent of the host's time zone.

use std::time::{SystemTime, UNIX_EPOCH};

/// The year, month (1–12) and day of the month (1–31) of `day`, in the Gregorian calendar
/// (Howard Hinnant's algorithm).
#[must_use]
pub fn civil_date(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day_of_month = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // Both come out in range by construction: 1–12 and 1–31.
    let (month, day_of_month) = (
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day_of_month).unwrap_or(1),
    );
    (year, month, day_of_month)
}

/// The day of `year`-`month`-`day`, counted from 1970-01-01.
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// UTC calendar year of a file timestamp or clock reading since the Unix epoch.
/// Returns none for an unsupported timestamp rather than inventing an age reference.
#[must_use]
pub fn utc_year(time: SystemTime) -> Option<u16> {
    let days = time.duration_since(UNIX_EPOCH).ok()?.as_secs() / 86_400;
    let day = i64::try_from(days).ok()?;
    u16::try_from(civil_date(day).0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn years_follow_new_year_and_leap_years() {
        let at = |year, month, day| {
            UNIX_EPOCH
                + Duration::from_secs(
                    u64::try_from(days_from_civil(year, month, day) * 86_400).unwrap(),
                )
        };
        assert_eq!(
            utc_year(at(2027, 1, 1) - Duration::from_nanos(1)),
            Some(2026)
        );
        assert_eq!(utc_year(at(2027, 1, 1)), Some(2027));
        assert_eq!(utc_year(at(2028, 2, 29)), Some(2028));
        assert_eq!(utc_year(UNIX_EPOCH - Duration::from_secs(1)), None);
    }
}
