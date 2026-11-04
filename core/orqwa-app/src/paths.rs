//! Platform file locations and file names.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The platform's per-user data directory for ORQWA (rides, routes, settings).
/// On Android the app's private one,
/// for when shared storage is missing ([`android_shared_data_dir`]).
#[must_use]
pub fn data_dir() -> PathBuf {
    if cfg!(target_os = "android") {
        return android_dirs(&std::env::temp_dir()).0;
    }
    let env = |key: &str| std::env::var_os(key).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local/share")))
    };
    let name = if cfg!(target_os = "macos") || cfg!(windows) {
        "ORQWA"
    } else {
        "orqwa"
    };
    let base = base.unwrap_or_else(std::env::temp_dir);
    base.join(name)
}

/// The platform's per-user cache directory for ORQWA: this computer's settings and kept worlds.
#[must_use]
pub fn cache_dir() -> PathBuf {
    if cfg!(target_os = "android") {
        return android_dirs(&std::env::temp_dir()).1;
    }
    let env = |key: &str| std::env::var_os(key).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Caches"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA")
    } else {
        env("XDG_CACHE_HOME").or_else(|| home.map(|h| h.join(".cache")))
    };
    let base = base.unwrap_or_else(std::env::temp_dir);
    base.join("orqwa")
}

/// ORQWA's data and cache directories on Android (ADR 0024), from the app's cache directory that
/// Android gives as the temporary one (`/data/user/0/<app>/cache`): data goes beside it into the
/// app's own `files`, which Android never clears, unlike the cache when space runs short.
fn android_dirs(app_cache: &std::path::Path) -> (PathBuf, PathBuf) {
    let app = app_cache.parent().unwrap_or(app_cache);
    (app.join("files").join("orqwa"), app_cache.join("orqwa"))
}

/// ORQWA's data directory on Android in `shared`, the app's own folder in shared storage
/// (`Android/media/<app>`, R148, ADR 0024) that the APK's plugin names: no app reaches the
/// private `files` folder, but sync apps allowed all files reach this one. What the private
/// folder held is moved there once; while that fails, the private folder stays in use.
#[must_use]
pub fn android_shared_data_dir(shared: &Path) -> PathBuf {
    into_shared(&android_dirs(&std::env::temp_dir()).0, shared)
}

/// The data directory `ORQWA` in `shared`, with `private` moved into it when only `private`
/// holds data. A sync app may have filled `ORQWA` first: then it wins, and `private` is left as
/// it is.
fn into_shared(private: &Path, shared: &Path) -> PathBuf {
    let data = shared.join("ORQWA");
    // Beside the data, not in it: a computer syncing the data has no gallery to hide videos from.
    let hidden = std::fs::create_dir_all(shared).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(shared.join(".nomedia"))
            .map(drop)
    });
    if let Err(error) = hidden {
        tracing::warn!(%error, "course videos may show in the gallery");
    }
    if data.exists() || !private.exists() {
        return data;
    }
    // Shared storage is another file system, so this is a copy: made aside and renamed into
    // place, so that `ORQWA` never holds half the data, and one cut short is made again.
    let moving = shared.join("ORQWA.moving");
    let moved = std::fs::remove_dir_all(&moving)
        .or_else(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(error),
        })
        .and_then(|()| copy_tree(private, &moving))
        .and_then(|()| std::fs::rename(&moving, &data));
    match moved {
        Ok(()) => {
            tracing::info!(to = %data.display(), "moved the data to shared storage");
            if let Err(error) = std::fs::remove_dir_all(private) {
                tracing::warn!(%error, folder = %private.display(), "cannot remove the moved data");
            }
            data
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&moving);
            tracing::warn!(%error, "cannot move the data to shared storage; it stays private");
            private.to_path_buf()
        }
    }
}

/// Copies the folder `from` with everything in it to `to`.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `orqwa-YYYYMMDD-HHMMSS.fit` in UTC.
#[must_use]
pub fn activity_file_name(start: SystemTime) -> PathBuf {
    let secs = start
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let time = secs.rem_euclid(86_400);
    PathBuf::from(format!(
        "orqwa-{year:04}{month:02}{day:02}-{:02}{:02}{:02}.fit",
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
    fn on_android_data_stays_out_of_the_cache() {
        let (data, cache) = android_dirs(Path::new("/data/user/0/org.orqwa.bike/cache"));

        assert_eq!(
            data,
            PathBuf::from("/data/user/0/org.orqwa.bike/files/orqwa")
        );
        assert_eq!(
            cache,
            PathBuf::from("/data/user/0/org.orqwa.bike/cache/orqwa")
        );
    }

    /// A private data folder as an earlier ORQWA left it on Android, with a ride and a course.
    fn private_data(base: &Path) -> PathBuf {
        let private = base.join("files/orqwa");
        std::fs::create_dir_all(private.join("profiles/rider/rides")).unwrap();
        std::fs::create_dir_all(private.join("courses")).unwrap();
        std::fs::write(private.join("profiles/rider/rides/a.fit"), b"ride").unwrap();
        std::fs::write(private.join("courses/Lake.oqc"), b"course").unwrap();
        private
    }

    #[test]
    fn on_android_the_data_moves_where_sync_apps_reach_it() {
        let base = scratch("shared-move");
        let private = private_data(&base);
        let shared = base.join("media/org.orqwa.bike");

        let data = into_shared(&private, &shared);

        assert_eq!(data, shared.join("ORQWA"));
        assert_eq!(
            std::fs::read(data.join("profiles/rider/rides/a.fit")).unwrap(),
            b"ride"
        );
        assert_eq!(
            std::fs::read(data.join("courses/Lake.oqc")).unwrap(),
            b"course"
        );
        assert!(!private.exists(), "the data is moved, not left twice");
        assert!(!shared.join("ORQWA.moving").exists());
    }

    #[test]
    fn data_a_sync_app_brought_first_wins_and_the_private_data_stays_untouched() {
        let base = scratch("shared-synced");
        let private = private_data(&base);
        let shared = base.join("media/org.orqwa.bike");
        std::fs::create_dir_all(shared.join("ORQWA/profiles/synced")).unwrap();

        assert_eq!(into_shared(&private, &shared), shared.join("ORQWA"));
        assert!(!shared.join("ORQWA/profiles/rider").exists());
        assert!(private.join("profiles/rider/rides/a.fit").exists());
    }

    #[test]
    fn a_move_cut_short_is_made_again_whole() {
        let base = scratch("shared-again");
        let private = private_data(&base);
        let shared = base.join("media/org.orqwa.bike");
        std::fs::create_dir_all(shared.join("ORQWA.moving/profiles")).unwrap();
        std::fs::write(shared.join("ORQWA.moving/half.fit"), b"ha").unwrap();

        let data = into_shared(&private, &shared);

        assert!(data.join("profiles/rider/rides/a.fit").exists());
        assert!(data.join("courses/Lake.oqc").exists());
        assert!(!data.join("half.fit").exists());
    }

    #[cfg(unix)]
    #[test]
    fn data_that_cannot_move_stays_private_and_nothing_half_copied_shows_in_shared_storage() {
        let base = scratch("shared-stuck");
        let private = private_data(&base);
        // A file that cannot be copied stops the move.
        std::os::unix::fs::symlink(base.join("gone"), private.join("courses/gone.mp4")).unwrap();
        let shared = base.join("media/org.orqwa.bike");

        assert_eq!(into_shared(&private, &shared), private);
        assert!(private.join("profiles/rider/rides/a.fit").exists());
        assert!(!shared.join("ORQWA").exists());
        assert!(!shared.join("ORQWA.moving").exists());
    }

    #[test]
    fn a_new_tablet_starts_in_shared_storage_with_videos_kept_out_of_the_gallery() {
        let base = scratch("shared-new");
        let shared = base.join("media/org.orqwa.bike");

        let data = into_shared(&base.join("files/orqwa"), &shared);

        assert_eq!(data, shared.join("ORQWA"));
        assert!(shared.join(".nomedia").is_file());
        assert!(
            !data.join(".nomedia").exists(),
            "computers syncing the data get no Android file"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_activity_files_by_utc_start() {
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400 + 13 * 3600 + 5 * 60 + 9);

        assert_eq!(
            activity_file_name(leap_day),
            PathBuf::from("orqwa-20000229-130509.fit")
        );
        assert_eq!(
            activity_file_name(UNIX_EPOCH),
            PathBuf::from("orqwa-19700101-000000.fit")
        );
    }
}
