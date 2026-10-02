//! Platform file locations.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// The platform's per-user cache directory for Torqa.
pub(crate) fn cache_dir() -> PathBuf {
    let env = |key: &str| std::env::var_os(key).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Caches"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA")
    } else {
        env("XDG_CACHE_HOME").or_else(|| home.map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("torqa")
}

/// `torqa-YYYYMMDD-HHMMSS.fit` in UTC.
pub(crate) fn activity_file_name(start: SystemTime) -> PathBuf {
    let secs = start
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let time = secs.rem_euclid(86_400);
    PathBuf::from(format!(
        "torqa-{year:04}{month:02}{day:02}-{:02}{:02}{:02}.fit",
        time / 3600,
        time % 3600 / 60,
        time % 60
    ))
}

/// Gregorian date from days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn names_activity_files_by_utc_start() {
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400 + 13 * 3600 + 5 * 60 + 9);

        assert_eq!(
            activity_file_name(leap_day),
            PathBuf::from("torqa-20000229-130509.fit")
        );
        assert_eq!(
            activity_file_name(UNIX_EPOCH),
            PathBuf::from("torqa-19700101-000000.fit")
        );
    }
}
