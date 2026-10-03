//! Ride metadata (R31, ADR 0002): next to each FIT activity a JSON file with the route name and
//! the ride's summary, so the history lists rides without decoding every FIT file. The FIT file
//! stays the source of truth; a missing or outdated JSON file can be rebuilt from it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use torqa_domain::recording::RideSummary;
use torqa_domain::units::{BeatsPerMinute, Joules, Meters, MetersPerSecond, Rpm, Watts};

/// The metadata format written by this version; newer files are treated as missing and rebuilt
/// from the FIT file rather than misread.
/// Version 2 added the route key, route time and climb times; version 1 files read without them.
pub const FORMAT_VERSION: u32 = 2;

/// Reading or writing ride metadata failed.
#[derive(Debug, thiserror::Error)]
pub enum RideError {
    /// File system error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid ride metadata.
    #[error("invalid ride metadata: {0}")]
    Json(#[from] serde_json::Error),
    /// Written by a newer Torqa.
    #[error("ride metadata format {0} is newer than this Torqa supports")]
    NewerFormat(u32),
}

/// What the history knows about one ride.
#[derive(Debug, Clone, PartialEq)]
pub struct RideRecord {
    /// The route ridden.
    pub route: String,
    /// When the ride started.
    pub start: SystemTime,
    /// Key figures.
    pub summary: RideSummary,
    /// Fingerprint of the route, to compare rides on the same course; `None` if unknown (e.g.
    /// rebuilt from a FIT file).
    pub route_key: Option<String>,
    /// Time for the whole route, if the rider reached the finish.
    pub route_time: Option<Duration>,
    /// Times on the route's climbs that the rider completed.
    pub climbs: Vec<ClimbTime>,
    /// The name the rider gave the ride (R50); `None` shows the route and date.
    pub name: Option<String>,
}

/// The time on one climb of a ride.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimbTime {
    /// Foot of the climb, from the route start.
    pub start: Meters,
    /// Top of the climb, from the route start.
    pub end: Meters,
    /// Time from foot to top.
    pub elapsed: Duration,
    /// Average power on the climb.
    pub avg_power: Option<Watts>,
}

impl ClimbTime {
    /// Whether `other` is the same climb, allowing for small shifts when the route is prepared
    /// again by a newer version.
    #[must_use]
    pub fn same_climb(&self, other: &ClimbTime) -> bool {
        const TOLERANCE_M: f64 = 100.0;
        (self.start.0 - other.start.0).abs() < TOLERANCE_M
            && (self.end.0 - other.end.0).abs() < TOLERANCE_M
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ClimbFile {
    start_m: f64,
    end_m: f64,
    time_s: f64,
    avg_power_w: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RideFile {
    format: u32,
    route: String,
    start_unix_s: u64,
    elapsed_s: f64,
    distance_m: f64,
    elevation_gain_m: f64,
    avg_speed_mps: f64,
    max_speed_mps: f64,
    avg_power_w: Option<f64>,
    max_power_w: Option<f64>,
    normalized_power_w: Option<f64>,
    intensity_factor: Option<f64>,
    training_stress: Option<f64>,
    work_j: Option<f64>,
    avg_cadence_rpm: Option<f64>,
    avg_heart_rate_bpm: Option<f64>,
    max_heart_rate_bpm: Option<f64>,
    ftp_w: f64,
    #[serde(default)]
    route_key: Option<String>,
    #[serde(default)]
    route_time_s: Option<f64>,
    #[serde(default)]
    climbs: Vec<ClimbFile>,
    // Added without a format bump: older readers ignore it, newer ones default it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl From<&RideRecord> for RideFile {
    fn from(r: &RideRecord) -> Self {
        let s = &r.summary;
        Self {
            format: FORMAT_VERSION,
            route: r.route.clone(),
            start_unix_s: r
                .start
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            elapsed_s: s.elapsed.as_secs_f64(),
            distance_m: s.distance.0,
            elevation_gain_m: s.elevation_gain.0,
            avg_speed_mps: s.avg_speed.0,
            max_speed_mps: s.max_speed.0,
            avg_power_w: s.avg_power.map(|v| v.0),
            max_power_w: s.max_power.map(|v| v.0),
            normalized_power_w: s.normalized_power.map(|v| v.0),
            intensity_factor: s.intensity_factor,
            training_stress: s.training_stress,
            work_j: s.work.map(|v| v.0),
            avg_cadence_rpm: s.avg_cadence.map(|v| v.0),
            avg_heart_rate_bpm: s.avg_heart_rate.map(|v| v.0),
            max_heart_rate_bpm: s.max_heart_rate.map(|v| v.0),
            ftp_w: s.ftp.0,
            route_key: r.route_key.clone(),
            name: r.name.clone(),
            route_time_s: r.route_time.map(|t| t.as_secs_f64()),
            climbs: r
                .climbs
                .iter()
                .map(|c| ClimbFile {
                    start_m: c.start.0,
                    end_m: c.end.0,
                    time_s: c.elapsed.as_secs_f64(),
                    avg_power_w: c.avg_power.map(|p| p.0),
                })
                .collect(),
        }
    }
}

impl From<RideFile> for RideRecord {
    fn from(f: RideFile) -> Self {
        Self {
            route: f.route,
            start: UNIX_EPOCH + Duration::from_secs(f.start_unix_s),
            summary: RideSummary {
                elapsed: Duration::from_secs_f64(f.elapsed_s.max(0.0)),
                distance: Meters(f.distance_m),
                elevation_gain: Meters(f.elevation_gain_m),
                avg_speed: MetersPerSecond(f.avg_speed_mps),
                max_speed: MetersPerSecond(f.max_speed_mps),
                avg_power: f.avg_power_w.map(Watts),
                max_power: f.max_power_w.map(Watts),
                normalized_power: f.normalized_power_w.map(Watts),
                intensity_factor: f.intensity_factor,
                training_stress: f.training_stress,
                work: f.work_j.map(Joules),
                avg_cadence: f.avg_cadence_rpm.map(Rpm),
                avg_heart_rate: f.avg_heart_rate_bpm.map(BeatsPerMinute),
                max_heart_rate: f.max_heart_rate_bpm.map(BeatsPerMinute),
                ftp: Watts(f.ftp_w),
            },
            route_key: f.route_key,
            name: f.name,
            route_time: f.route_time_s.map(|t| Duration::from_secs_f64(t.max(0.0))),
            climbs: f
                .climbs
                .into_iter()
                .map(|c| ClimbTime {
                    start: Meters(c.start_m),
                    end: Meters(c.end_m),
                    elapsed: Duration::from_secs_f64(c.time_s.max(0.0)),
                    avg_power: c.avg_power_w.map(Watts),
                })
                .collect(),
        }
    }
}

/// The metadata file belonging to a FIT file: same name, `.json`.
#[must_use]
pub fn metadata_path(fit: &Path) -> PathBuf {
    fit.with_extension("json")
}

/// Writes the metadata of the ride stored in `fit`, atomically.
///
/// # Errors
/// On file system errors.
pub fn save(fit: &Path, record: &RideRecord) -> Result<(), RideError> {
    let path = metadata_path(fit);
    let partial = path.with_extension("json.part");
    std::fs::write(
        &partial,
        serde_json::to_vec_pretty(&RideFile::from(record))?,
    )?;
    std::fs::rename(&partial, &path)?;
    Ok(())
}

/// Reads the metadata of the ride stored in `fit`.
///
/// # Errors
/// If it is missing, malformed or from a newer Torqa.
pub fn load(fit: &Path) -> Result<RideRecord, RideError> {
    let text = std::fs::read_to_string(metadata_path(fit))?;
    let format = serde_json::from_str::<serde_json::Value>(&text)?["format"]
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .unwrap_or(0);
    if format > FORMAT_VERSION {
        return Err(RideError::NewerFormat(format));
    }
    Ok(serde_json::from_str::<RideFile>(&text)?.into())
}

/// The FIT files in `dir`, newest first (file names start with the UTC start time).
#[must_use]
pub fn fit_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fit")))
        .collect();
    files.sort_by(|a, b| b.cmp(a));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("torqa-rides-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record() -> RideRecord {
        RideRecord {
            route: "Gurten".to_owned(),
            start: UNIX_EPOCH + Duration::from_secs(1_790_000_000),
            summary: RideSummary {
                elapsed: Duration::from_mins(30),
                distance: Meters(12_000.0),
                avg_power: Some(Watts(210.0)),
                normalized_power: Some(Watts(225.0)),
                training_stress: Some(40.5),
                avg_heart_rate: None,
                ftp: Watts(250.0),
                ..RideSummary::default()
            },
            route_key: Some("0123456789abcdef".to_owned()),
            route_time: Some(Duration::from_secs(1795)),
            climbs: vec![ClimbTime {
                start: Meters(1000.0),
                end: Meters(3400.0),
                elapsed: Duration::from_secs(700),
                avg_power: Some(Watts(260.0)),
            }],
            name: Some("Morning loop".to_owned()),
        }
    }

    #[test]
    fn metadata_round_trips_next_to_the_fit_file() {
        let dir = temp_dir("roundtrip");
        let fit = dir.join("torqa-20260930-071500.fit");

        save(&fit, &record()).unwrap();

        assert!(dir.join("torqa-20260930-071500.json").exists());
        assert_eq!(load(&fit).unwrap(), record());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_1_metadata_still_loads() {
        let dir = temp_dir("v1");
        let fit = dir.join("a.fit");
        std::fs::write(
            dir.join("a.json"),
            r#"{"format": 1, "route": "Old", "start_unix_s": 1, "elapsed_s": 60.0,
                "distance_m": 500.0, "elevation_gain_m": 0.0, "avg_speed_mps": 8.0,
                "max_speed_mps": 9.0, "ftp_w": 200.0}"#,
        )
        .unwrap();

        let record = load(&fit).unwrap();

        assert_eq!(record.route, "Old");
        assert_eq!(record.route_key, None);
        assert_eq!(record.climbs, []);
        assert_eq!(record.name, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn climbs_match_despite_small_shifts() {
        let climb = record().climbs[0];
        let shifted = ClimbTime {
            start: Meters(1040.0),
            end: Meters(3380.0),
            ..climb
        };
        let other = ClimbTime {
            start: Meters(5000.0),
            end: Meters(6000.0),
            ..climb
        };

        assert!(climb.same_climb(&shifted));
        assert!(!climb.same_climb(&other));
    }

    #[test]
    fn metadata_from_a_newer_torqa_is_not_misread() {
        let dir = temp_dir("newer");
        let fit = dir.join("a.fit");
        std::fs::write(dir.join("a.json"), r#"{"format": 99, "route": "x"}"#).unwrap();

        assert!(matches!(load(&fit), Err(RideError::NewerFormat(99))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_fit_files_newest_first() {
        let dir = temp_dir("list");
        for name in [
            "torqa-20260101-080000.fit",
            "torqa-20261001-080000.fit",
            "notes.txt",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let names: Vec<String> = fit_files(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            names,
            ["torqa-20261001-080000.fit", "torqa-20260101-080000.fit"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
