//! Ride metadata (R31, ADR 0002): next to each FIT activity a JSON file with the route name and
//! the ride's summary, so the history lists rides without decoding every FIT file. The FIT file
//! stays the source of truth; a missing or outdated JSON file can be rebuilt from it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use torqa_domain::recording::{PowerCurve, RideAnalysis, RideSummary};
use torqa_domain::units::{BeatsPerMinute, Joules, Kilograms, Meters, MetersPerSecond, Rpm, Watts};

/// The metadata format written by this version; newer files are treated as missing and rebuilt
/// from the FIT file rather than misread.
/// Version 2 added the route key, route time and climb times; version 1 files read without them.
pub const FORMAT_VERSION: u32 = 2;

/// The analysis data written by this version (R61, ADR 0013). It has its own version, so it can
/// grow and be computed again from the FIT file without a new [`FORMAT_VERSION`], which an older
/// Torqa would answer by rebuilding the metadata and losing the ride's name and times. Later
/// versions only add to it, so a newer one still reads as this one.
pub const ANALYSIS_VERSION: u32 = 1;

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
    /// The FTP an FTP test showed (R22), if the ride was one.
    pub ftp_estimate: Option<Watts>,
    /// The rider's body mass on the ride, for power per kilogram (R61); `None` for rides saved
    /// before Torqa kept it.
    pub rider_mass: Option<Kilograms>,
    /// Sped up or moved along in simulation (#53): not the rider's own effort, so it counts
    /// towards no records and no analysis.
    pub simulated: bool,
    /// What the analysis across rides keeps of the ride (R61); `None` until it is computed
    /// from the FIT file, or if an older Torqa computed less than this one keeps.
    pub analysis: Option<RideAnalysis>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ftp_estimate_w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rider_mass_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    simulated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    analysis: Option<AnalysisFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnalysisFile {
    version: u32,
    /// Best power in watts per duration in seconds: one line each in the pretty-printed file.
    #[serde(default)]
    power_curve_w: BTreeMap<u64, f64>,
}

impl From<&RideAnalysis> for AnalysisFile {
    fn from(a: &RideAnalysis) -> Self {
        Self {
            version: ANALYSIS_VERSION,
            power_curve_w: a
                .power_curve
                .0
                .iter()
                .map(|(d, p)| (d.as_secs(), p.0))
                .collect(),
        }
    }
}

impl From<AnalysisFile> for RideAnalysis {
    fn from(f: AnalysisFile) -> Self {
        Self {
            power_curve: PowerCurve(
                f.power_curve_w
                    .into_iter()
                    .map(|(s, w)| (Duration::from_secs(s), Watts(w)))
                    .collect(),
            ),
        }
    }
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
            ftp_estimate_w: r.ftp_estimate.map(|w| w.0),
            rider_mass_kg: r.rider_mass.map(|m| m.0),
            simulated: r.simulated,
            analysis: r.analysis.as_ref().map(AnalysisFile::from),
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
            ftp_estimate: f.ftp_estimate_w.map(Watts),
            rider_mass: f.rider_mass_kg.map(Kilograms),
            simulated: f.simulated,
            analysis: f
                .analysis
                .filter(|a| a.version >= ANALYSIS_VERSION)
                .map(RideAnalysis::from),
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

/// Copies the ride stored in `fit` to `to`, e.g. to upload it by hand (R28). The copy goes to
/// a `.part` file beside `to` and is moved into place, so a failed export leaves no partial
/// file, keeps a file it would have replaced, and choosing the ride's own file cannot
/// truncate it.
///
/// # Errors
/// On file system errors.
pub fn export(fit: &Path, to: &Path) -> Result<(), RideError> {
    let mut partial = to.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    if let Err(error) = std::fs::copy(fit, &partial).and_then(|_| std::fs::rename(&partial, to)) {
        let _ = std::fs::remove_file(&partial);
        return Err(error.into());
    }
    Ok(())
}

/// A file name for exporting the ride titled `title` (R50): the title with what Windows,
/// macOS or Linux reject in a file name dropped, and `.fit`. The FIT format has no field
/// for an activity name, so the file name is where it travels.
#[must_use]
pub fn export_file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Windows drops trailing dots; a leading one hides the file on macOS and Linux.
    let stem = words.trim_matches(|c: char| c == '.' || c.is_whitespace());
    format!("{}.fit", if stem.is_empty() { "ride" } else { stem })
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
            ftp_estimate: Some(Watts(245.0)),
            rider_mass: Some(Kilograms(72.5)),
            simulated: false,
            analysis: Some(RideAnalysis {
                power_curve: PowerCurve(vec![
                    (Duration::from_secs(1), Watts(612.5)),
                    (Duration::from_mins(1), Watts(388.0)),
                    (Duration::from_mins(20), Watts(251.3)),
                ]),
            }),
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
        assert_eq!(record.rider_mass, None);
        assert!(!record.simulated);
        assert_eq!(record.analysis, None, "to be computed from the FIT file");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn analysis_from_an_older_torqa_is_computed_again_and_a_newer_one_still_read() {
        let dir = temp_dir("analysis-versions");
        let fit = dir.join("a.fit");
        let json = |version: u32| {
            format!(
                r#"{{"format": 2, "route": "R", "start_unix_s": 1, "elapsed_s": 60.0,
                    "distance_m": 500.0, "elevation_gain_m": 0.0, "avg_speed_mps": 8.0,
                    "max_speed_mps": 9.0, "ftp_w": 200.0, "simulated": true,
                    "analysis": {{"version": {version}, "power_curve_w": {{"5": 700.0}},
                    "added_later": [1, 2]}}}}"#
            )
        };

        std::fs::write(dir.join("a.json"), json(0)).unwrap();
        let older = load(&fit).unwrap();
        std::fs::write(dir.join("a.json"), json(99)).unwrap();
        let newer = load(&fit).unwrap();

        assert!(older.simulated);
        assert_eq!(older.analysis, None);
        let curve = newer.analysis.unwrap().power_curve;
        assert_eq!(curve.at(Duration::from_secs(5)), Some(Watts(700.0)));
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
    fn exports_an_exact_copy_and_replaces_an_older_export() {
        let dir = temp_dir("export");
        let fit = dir.join("torqa-20261003-071500.fit");
        std::fs::write(&fit, b"FIT activity").unwrap();
        let to = dir.join("Morning loop.fit");
        std::fs::write(&to, b"an older export, longer than the ride").unwrap();

        export(&fit, &to).unwrap();

        assert_eq!(std::fs::read(&to).unwrap(), b"FIT activity");
        assert_eq!(std::fs::read(&fit).unwrap(), b"FIT activity");
        assert!(!dir.join("Morning loop.fit.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exporting_onto_the_ride_itself_keeps_it() {
        let dir = temp_dir("export-self");
        let fit = dir.join("a.fit");
        std::fs::write(&fit, b"FIT activity").unwrap();

        export(&fit, &fit).unwrap();

        assert_eq!(std::fs::read(&fit).unwrap(), b"FIT activity");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_export_leaves_nothing_behind() {
        let dir = temp_dir("export-missing");

        assert!(matches!(
            export(&dir.join("gone.fit"), &dir.join("out.fit")),
            Err(RideError::Io(_))
        ));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn export_file_names_keep_the_title_and_are_valid_everywhere() {
        assert_eq!(export_file_name("Morning loop"), "Morning loop.fit");
        assert_eq!(
            export_file_name("Gurtenstrasse · Sat 3 Oct"),
            "Gurtenstrasse · Sat 3 Oct.fit"
        );
        assert_eq!(
            export_file_name("Intervals: 4/4 <hard>?"),
            "Intervals 4 4 hard.fit"
        );
        assert_eq!(export_file_name(" .hidden\tride... "), "hidden ride.fit");
        assert_eq!(export_file_name("??"), "ride.fit");
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
