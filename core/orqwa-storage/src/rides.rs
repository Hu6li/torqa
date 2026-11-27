//! Ride metadata (R31, ADR 0002): next to each FIT activity a JSON file with the route name and
//! the ride's summary, so the history lists rides without decoding every FIT file. The FIT file
//! stays the source of truth; a missing or outdated JSON file can be rebuilt from it.
//!
//! The times on the rider's own segments (R302) are in a third file, `<ride>.segments.json`
//! (ADR 0044): they cannot be rebuilt from the FIT file, and an ORQWA from before them rewrites
//! the metadata without the fields it does not know, as when it renames a ride.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orqwa_domain::recording::{Histogram, PowerCurve, RideAnalysis, RideSummary};
use orqwa_domain::units::{BeatsPerMinute, Joules, Kilograms, Meters, MetersPerSecond, Rpm, Watts};
use serde::{Deserialize, Serialize};

use crate::segments::SegmentKey;

/// The metadata format written by this version; newer files are treated as missing and rebuilt
/// from the FIT file rather than misread.
/// Version 2 added the route key, route time and climb times; version 1 files read without them.
pub const FORMAT_VERSION: u32 = 2;

/// The analysis data written by this version (R62, ADR 0014). It has its own version, so it can
/// grow and be computed again from the FIT file without a new [`FORMAT_VERSION`], which an older
/// ORQWA would answer by rebuilding the metadata and losing the ride's name and times. Later
/// versions only add to it, so a newer one still reads as this one. Version 1 held the power
/// curve, version 2 added the power and heart-rate histograms (R64).
pub const ANALYSIS_VERSION: u32 = 2;

/// The segment times format written by this version (ADR 0044). Later versions of the same
/// format only add fields, which this one passes over; a file of a newer format is not misread.
pub const SEGMENTS_VERSION: u32 = 1;

/// Reading or writing ride metadata failed.
#[derive(Debug, thiserror::Error)]
pub enum RideError {
    /// File system error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid ride metadata.
    #[error("invalid ride metadata: {0}")]
    Json(#[from] serde_json::Error),
    /// Written by a newer ORQWA.
    #[error("ride metadata format {0} is newer than this ORQWA supports")]
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
    /// Times on the rider's own segments of the route that they completed (R302).
    pub segments: Vec<SegmentTime>,
    /// The name the rider gave the ride (R50); `None` shows the route and date.
    pub name: Option<String>,
    /// The FTP an FTP test showed (R22), if the ride was one.
    pub ftp_estimate: Option<Watts>,
    /// The rider's body mass on the ride, for power per kilogram (R62); `None` for rides saved
    /// before ORQWA kept it.
    pub rider_mass: Option<Kilograms>,
    /// Sped up or moved along in simulation (#53): not the rider's own effort, so it counts
    /// towards no records and no analysis.
    pub simulated: bool,
    /// Drafting or keep together was enabled; speed records are excluded, measured power is real.
    pub assisted: bool,
    /// What the analysis across rides keeps of the ride (R62); `None` until it is computed
    /// from the FIT file, or if an older ORQWA computed less than this one keeps.
    pub analysis: Option<RideAnalysis>,
    /// Where the ride was uploaded (R29), by platform id.
    pub uploads: BTreeMap<String, Upload>,
}

/// A ride's activity on a training platform (R29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    /// The platform's id for it.
    pub id: String,
    /// Its page on the platform.
    pub url: String,
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

/// The time on one of the rider's segments of a ride (R302).
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentTime {
    /// The segment's id ([`crate::segments::Segment::id`]).
    pub id: String,
    /// The segment's name when it was ridden.
    pub name: String,
    /// The revision of the prepared route ridden (`orqwa_routes::Route::revision`).
    pub revision: String,
    /// Its start, from the route start.
    pub start: Meters,
    /// Its end, from the route start.
    pub end: Meters,
    /// Time from start to end.
    pub elapsed: Duration,
    /// Average power on it.
    pub avg_power: Option<Watts>,
}

impl SegmentTime {
    /// The stretch it was ridden on: times with the same key compare, whatever the segment is
    /// called now.
    #[must_use]
    pub fn key(&self) -> SegmentKey {
        SegmentKey::new(&self.revision, self.start.0, self.end.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SegmentFile {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    revision: String,
    start_m: f64,
    end_m: f64,
    time_s: f64,
    avg_power_w: Option<f64>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    uploads: BTreeMap<String, Upload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rider_mass_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    simulated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    assisted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    analysis: Option<AnalysisFile>,
}

/// `<ride>.segments.json`.
#[derive(Debug, Serialize, Deserialize)]
struct SegmentsFile {
    format: u32,
    segments: Vec<SegmentFile>,
}

/// What the history index keeps of a ride: both its files' contents in one text.
#[derive(Debug, Serialize, Deserialize)]
struct IndexedRide {
    metadata: RideFile,
    #[serde(default)]
    segments: Vec<SegmentFile>,
}

impl From<&SegmentTime> for SegmentFile {
    fn from(s: &SegmentTime) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            revision: s.revision.clone(),
            start_m: s.start.0,
            end_m: s.end.0,
            time_s: s.elapsed.as_secs_f64(),
            avg_power_w: s.avg_power.map(|p| p.0),
        }
    }
}

impl From<SegmentFile> for SegmentTime {
    fn from(s: SegmentFile) -> Self {
        Self {
            id: s.id,
            name: s.name,
            revision: s.revision,
            start: Meters(s.start_m),
            end: Meters(s.end_m),
            elapsed: Duration::from_secs_f64(s.time_s.max(0.0)),
            avg_power: s.avg_power_w.map(Watts),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AnalysisFile {
    version: u32,
    /// Best power in watts per duration in seconds: one line each in the pretty-printed file.
    #[serde(default)]
    power_curve_w: BTreeMap<u64, f64>,
    /// Seconds per whole watt.
    #[serde(default)]
    power_histogram_s: BTreeMap<u32, f64>,
    /// Seconds per whole beat per minute.
    #[serde(default)]
    heart_rate_histogram_s: BTreeMap<u32, f64>,
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
            power_histogram_s: a.power_histogram.0.clone(),
            heart_rate_histogram_s: a.heart_rate_histogram.0.clone(),
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
            power_histogram: Histogram(f.power_histogram_s),
            heart_rate_histogram: Histogram(f.heart_rate_histogram_s),
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
            assisted: r.assisted,
            analysis: r.analysis.as_ref().map(AnalysisFile::from),
            uploads: r.uploads.clone(),
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
            assisted: f.assisted,
            analysis: f
                .analysis
                .filter(|a| a.version >= ANALYSIS_VERSION)
                .map(RideAnalysis::from),
            uploads: f.uploads,
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
            // In their own file: see `load`.
            segments: Vec::new(),
        }
    }
}

/// The metadata file belonging to a FIT file: same name, `.json`.
#[must_use]
pub fn metadata_path(fit: &Path) -> PathBuf {
    fit.with_extension("json")
}

/// The segment times file belonging to a FIT file: same name, `.segments.json`. Releases before
/// it list rides by their FIT files and read only the metadata beside them, so they leave it be.
#[must_use]
pub fn segments_path(fit: &Path) -> PathBuf {
    fit.with_extension("segments.json")
}

/// Writes `bytes` to `path` through a `.part` file beside it, so a reader or a sync client
/// never sees half of it.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), RideError> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)?;
    Ok(())
}

/// Writes the metadata of the ride stored in `fit`, atomically. Its segment times are not
/// part of it: [`save_segments`] writes them, once, when the ride is saved.
///
/// # Errors
/// On file system errors.
pub fn save(fit: &Path, record: &RideRecord) -> Result<(), RideError> {
    write_atomically(
        &metadata_path(fit),
        &serde_json::to_vec_pretty(&RideFile::from(record))?,
    )
}

/// Writes the times on the rider's segments of the ride stored in `fit`, atomically; none
/// writes no file.
///
/// # Errors
/// On file system errors.
pub fn save_segments(fit: &Path, times: &[SegmentTime]) -> Result<(), RideError> {
    if times.is_empty() {
        return Ok(());
    }
    let file = SegmentsFile {
        format: SEGMENTS_VERSION,
        segments: times.iter().map(SegmentFile::from).collect(),
    };
    write_atomically(&segments_path(fit), &serde_json::to_vec_pretty(&file)?)
}

/// The segment times of the ride stored in `fit`: none without the file.
///
/// # Errors
/// If the file is unreadable, malformed or from a newer ORQWA.
pub fn load_segments(fit: &Path) -> Result<Vec<SegmentTime>, RideError> {
    let text = match std::fs::read_to_string(segments_path(fit)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let format = format_of(&text)?;
    if format > SEGMENTS_VERSION {
        return Err(RideError::NewerFormat(format));
    }
    let file: SegmentsFile = serde_json::from_str(&text)?;
    Ok(file.segments.into_iter().map(SegmentTime::from).collect())
}

/// Reads the metadata of the ride stored in `fit`, with its segment times. Times that cannot
/// be read (from a newer ORQWA, say) are left out rather than costing the ride.
///
/// # Errors
/// If the metadata is missing, malformed or from a newer ORQWA.
pub fn load(fit: &Path) -> Result<RideRecord, RideError> {
    let mut record = metadata_from_json(&std::fs::read_to_string(metadata_path(fit))?)?;
    record.segments = load_segments(fit).unwrap_or_default();
    Ok(record)
}

/// The ride with its segment times as one text, for the history index to keep (R79).
///
/// # Errors
/// If it cannot be written as JSON.
pub fn to_json(record: &RideRecord) -> Result<String, RideError> {
    Ok(serde_json::to_string(&IndexedRide {
        metadata: RideFile::from(record),
        segments: record.segments.iter().map(SegmentFile::from).collect(),
    })?)
}

/// A ride from the text [`to_json`] made of it.
///
/// # Errors
/// If it is malformed or from a newer ORQWA.
pub fn from_json(text: &str) -> Result<RideRecord, RideError> {
    let indexed: IndexedRide = serde_json::from_str(text)?;
    if indexed.metadata.format > FORMAT_VERSION {
        return Err(RideError::NewerFormat(indexed.metadata.format));
    }
    let mut record = RideRecord::from(indexed.metadata);
    record.segments = indexed
        .segments
        .into_iter()
        .map(SegmentTime::from)
        .collect();
    Ok(record)
}

fn format_of(text: &str) -> Result<u32, RideError> {
    Ok(serde_json::from_str::<serde_json::Value>(text)?["format"]
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .unwrap_or(0))
}

/// A ride's metadata from the text of its file.
fn metadata_from_json(text: &str) -> Result<RideRecord, RideError> {
    let format = format_of(text)?;
    if format > FORMAT_VERSION {
        return Err(RideError::NewerFormat(format));
    }
    Ok(serde_json::from_str::<RideFile>(text)?.into())
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

/// The FIT files in `dir`, by name. Their order says nothing of when they were ridden: see
/// [`newest_first`].
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
    files.sort();
    files
}

/// Orders `rides` newest first by their recorded start, including imported file names (R139).
pub fn newest_first(rides: &mut [(PathBuf, RideRecord)]) {
    rides.sort_by(|(a_fit, a), (b_fit, b)| b.start.cmp(&a.start).then_with(|| b_fit.cmp(a_fit)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-rides-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn assisted_metadata_round_trips_without_excluding_measured_training_data() {
        let mut ride = record();
        ride.assisted = true;
        let encoded = serde_json::to_string(&RideFile::from(&ride)).unwrap();
        let restored = RideRecord::from(serde_json::from_str::<RideFile>(&encoded).unwrap());
        assert!(restored.assisted);
        assert!(!restored.simulated);
        assert_eq!(restored.summary.avg_power, ride.summary.avg_power);
        let mut old = serde_json::from_str::<serde_json::Value>(&encoded).unwrap();
        old.as_object_mut().unwrap().remove("assisted");
        let legacy = RideRecord::from(serde_json::from_value::<RideFile>(old).unwrap());
        assert!(!legacy.assisted);
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
            segments: vec![SegmentTime {
                id: "0f3a".to_owned(),
                name: "Wabern sprint".to_owned(),
                revision: "9e1c".to_owned(),
                start: Meters(200.0),
                end: Meters(500.0),
                elapsed: Duration::from_secs(21),
                avg_power: Some(Watts(780.0)),
            }],
            name: Some("Morning loop".to_owned()),
            ftp_estimate: Some(Watts(245.0)),
            rider_mass: Some(Kilograms(72.5)),
            simulated: false,
            assisted: false,
            analysis: Some(RideAnalysis {
                power_curve: PowerCurve(vec![
                    (Duration::from_secs(1), Watts(612.5)),
                    (Duration::from_mins(1), Watts(388.0)),
                    (Duration::from_mins(20), Watts(251.3)),
                ]),
                power_histogram: Histogram(BTreeMap::from([(150, 600.0), (250, 1200.0)])),
                heart_rate_histogram: Histogram(BTreeMap::from([(141, 1800.0)])),
            }),
            uploads: BTreeMap::from([(
                "intervals".to_owned(),
                Upload {
                    id: "i777".to_owned(),
                    url: "https://intervals.icu/activities/i777".to_owned(),
                },
            )]),
        }
    }

    #[test]
    fn metadata_round_trips_next_to_the_fit_file() {
        let dir = temp_dir("roundtrip");
        let fit = dir.join("orqwa-20260930-071500.fit");

        save(&fit, &record()).unwrap();
        save_segments(&fit, &record().segments).unwrap();

        assert!(dir.join("orqwa-20260930-071500.json").exists());
        assert!(dir.join("orqwa-20260930-071500.segments.json").exists());
        assert_eq!(load(&fit).unwrap(), record());
        let indexed = from_json(&to_json(&record()).unwrap()).unwrap();
        assert_eq!(indexed, record(), "the index keeps the times too");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn segment_times_outlast_an_orqwa_that_does_not_know_them() {
        let dir = temp_dir("older");
        let fit = dir.join("orqwa-20260930-071500.fit");
        save(&fit, &record()).unwrap();
        save_segments(&fit, &record().segments).unwrap();

        // An older release renames the ride: it writes the metadata afresh from the fields it
        // knows, none of them the times.
        let metadata: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(metadata_path(&fit)).unwrap()).unwrap();
        assert!(
            metadata.get("segments").is_none(),
            "nothing of them to lose there"
        );
        let older = RideRecord {
            name: Some("Renamed elsewhere".to_owned()),
            segments: Vec::new(),
            ..record()
        };
        save(&fit, &older).unwrap();

        let loaded = load(&fit).unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Renamed elsewhere"));
        assert_eq!(loaded.segments, record().segments);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn no_segment_times_make_no_file_and_newer_ones_are_not_misread() {
        let dir = temp_dir("segment-files");
        let fit = dir.join("a.fit");
        save(&fit, &record()).unwrap();
        save_segments(&fit, &[]).unwrap();
        assert!(!segments_path(&fit).exists());
        assert_eq!(load(&fit).unwrap().segments, []);

        std::fs::write(segments_path(&fit), r#"{"format": 99, "laps": []}"#).unwrap();
        assert!(matches!(
            load_segments(&fit),
            Err(RideError::NewerFormat(99))
        ));
        let ride = load(&fit).unwrap();
        assert_eq!(ride.segments, [], "the ride without them");
        assert_eq!(ride.route, "Gurten");
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
        assert_eq!(record.segments, []);
        assert_eq!(record.name, None);
        assert_eq!(record.uploads, BTreeMap::new());
        assert_eq!(record.rider_mass, None);
        assert!(!record.simulated);
        assert_eq!(record.analysis, None, "to be computed from the FIT file");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn analysis_from_an_older_orqwa_is_computed_again_and_a_newer_one_still_read() {
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

        std::fs::write(dir.join("a.json"), json(ANALYSIS_VERSION - 1)).unwrap();
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
    fn a_segment_time_compares_by_its_route_revision_and_ends_not_its_name() {
        let sprint = record().segments[0].clone();
        let renamed = SegmentTime {
            name: "Village sprint".to_owned(),
            ..sprint.clone()
        };
        let moved = SegmentTime {
            end: Meters(520.0),
            ..sprint.clone()
        };

        assert_eq!(sprint.key(), renamed.key());
        assert_ne!(sprint.key(), moved.key());
        let rebuilt = SegmentTime {
            revision: "77aa".to_owned(),
            ..sprint.clone()
        };
        assert_ne!(
            sprint.key(),
            rebuilt.key(),
            "the course prepared differently"
        );
    }

    #[test]
    fn metadata_from_a_newer_orqwa_is_not_misread() {
        let dir = temp_dir("newer");
        let fit = dir.join("a.fit");
        std::fs::write(dir.join("a.json"), r#"{"format": 99, "route": "x"}"#).unwrap();

        assert!(matches!(load(&fit), Err(RideError::NewerFormat(99))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exports_an_exact_copy_and_replaces_an_older_export() {
        let dir = temp_dir("export");
        let fit = dir.join("orqwa-20261003-071500.fit");
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
    fn lists_the_fit_files_alone() {
        let dir = temp_dir("list");
        for name in ["orqwa-20261001-080000.fit", "Imported.FIT", "notes.txt"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let mut names: Vec<String> = fit_files(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();

        assert_eq!(names, ["Imported.FIT", "orqwa-20261001-080000.fit"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imported_and_recorded_rides_come_newest_first() {
        let ride = |name: &str, secs: u64| {
            let record = RideRecord {
                start: UNIX_EPOCH + Duration::from_secs(secs),
                ..record()
            };
            (PathBuf::from(name), record)
        };
        // File-name order differs from the recorded start order.
        let mut rides = vec![
            ride("orqwa-20261019-071500.fit", 1_792_394_100),
            ride("zz-imported-20261012-071500.fit", 1_791_789_300),
            ride("orqwa-20261024-071500.fit", 1_792_826_100),
        ];

        newest_first(&mut rides);

        let names: Vec<&str> = rides.iter().map(|(fit, _)| fit.to_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "orqwa-20261024-071500.fit",
                "orqwa-20261019-071500.fit",
                "zz-imported-20261012-071500.fit",
            ]
        );
    }
}
