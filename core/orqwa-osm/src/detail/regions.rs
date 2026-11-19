//! The regions a rider downloaded: each region's download (`<id>.osm.pbf`) and its record
//! (`<id>.json`) in the map cache's `geofabrik` directory, its extract under [`VERSION`].

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::extract::{Stage, VERSION, extract, file_name};
use crate::OsmError;

/// The directory of downloads and their records in the map cache.
pub const DOWNLOADS: &str = "geofabrik";

/// A region whose map detail is ready, as its record keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// Geofabrik's name for it, e.g. `switzerland` or `bayern`.
    pub id: String,
    /// Its name to show, e.g. `Switzerland`.
    pub name: String,
    /// Its extract's directory under [`VERSION`], e.g. `switzerland-2026-10-24`.
    pub extract: String,
    /// When its data was current (Unix seconds), if the download says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_time: Option<i64>,
    /// The file Geofabrik served, dated, e.g. `switzerland-261024.osm.pbf`: a newer one is an
    /// update. `None` for a download ORQWA did not make itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What its extract holds, as [`CONTENT`] counts: 0 for extracts made before it did.
    #[serde(default)]
    pub content: u32,
}

/// What extracts hold: 1 the roads' surfaces and roundabouts, 2 also what buildings are
/// (ADR 0041). A region whose extract holds less is not ready, so its download is read again.
/// The directory [`VERSION`] stays: courses carry the cells they used under it, and a cell
/// without buildings reads as one whose buildings say nothing.
pub const CONTENT: u32 = 2;

/// Where region `id`'s download goes in the map cache `osm_dir`.
#[must_use]
pub fn download_path(osm_dir: &Path, id: &str) -> PathBuf {
    osm_dir
        .join(DOWNLOADS)
        .join(format!("{}.osm.pbf", file_name(id)))
}

fn record_path(osm_dir: &Path, id: &str) -> PathBuf {
    osm_dir
        .join(DOWNLOADS)
        .join(format!("{}.json", file_name(id)))
}

/// Where a download of region `id` cut short lies, to go on with.
pub(crate) fn partial_path(osm_dir: &Path, id: &str) -> PathBuf {
    with_suffix(&download_path(osm_dir, id), ".part")
}

/// Beside a download, whole or cut short: the file Geofabrik served, dated.
pub(crate) fn source_path(download: &Path) -> PathBuf {
    with_suffix(download, ".name")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The file Geofabrik served for region `id`'s whole download, if ORQWA downloaded it.
#[must_use]
pub fn download_source(osm_dir: &Path, id: &str) -> Option<String> {
    let download = download_path(osm_dir, id);
    if !download.is_file() {
        return None;
    }
    fs::read_to_string(source_path(&download)).ok()
}

/// A region in the map cache: its download, whole or cut short, and its detail once made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// Geofabrik's name for it; without a record, that of its download's file.
    pub id: String,
    /// Its record, once its detail is ready.
    pub ready: Option<Region>,
    /// Whether its download is whole: ready, or to make the detail of.
    pub downloaded: bool,
    /// The bytes its download, a download cut short and its detail take together.
    pub size: u64,
}

/// The regions in the map cache `osm_dir`, by id: those with a download, whole or cut short,
/// or with detail ready.
#[must_use]
pub fn kept(osm_dir: &Path) -> Vec<Kept> {
    let ready = installed(osm_dir);
    let mut ids: Vec<String> = ready.iter().map(|region| region.id.clone()).collect();
    if let Ok(entries) = fs::read_dir(osm_dir.join(DOWNLOADS)) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name
                .strip_suffix(".osm.pbf")
                .or_else(|| name.strip_suffix(".osm.pbf.part"));
            if let Some(stem) = stem
                && !ids.iter().any(|id| file_name(id) == stem)
            {
                ids.push(stem.to_owned());
            }
        }
    }
    ids.sort();
    ids.into_iter()
        .map(|id| {
            let region = ready.iter().find(|r| r.id == id).cloned();
            let download = download_path(osm_dir, &id);
            let mut size = [download.clone(), partial_path(osm_dir, &id)]
                .iter()
                .filter_map(|path| fs::metadata(path).ok())
                .map(|meta| meta.len())
                .sum();
            if let Some(region) = &region {
                size += dir_size(&osm_dir.join(VERSION).join(&region.extract));
            }
            Kept {
                downloaded: download.is_file(),
                ready: region,
                id,
                size,
            }
        })
        .collect()
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map_or(0, |meta| meta.len()),
            Err(_) => 0,
        })
        .sum()
}

/// Deletes region `id` from the map cache `osm_dir`: its download, whole or cut short, its
/// record and its detail. Courses prepared with it carry what they used. One already gone is
/// no error.
///
/// # Errors
/// [`OsmError::Cache`] if a file is there but cannot be deleted.
pub fn remove(osm_dir: &Path, id: &str) -> Result<(), OsmError> {
    let record = record_path(osm_dir, id);
    let region: Option<Region> = fs::read(&record)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    // The record first: a region half deleted is never ready.
    let download = download_path(osm_dir, id);
    let partial = partial_path(osm_dir, id);
    for file in [
        record,
        source_path(&download),
        download,
        source_path(&partial),
        partial,
    ] {
        match fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    if let Some(region) = region {
        let extract = osm_dir.join(VERSION).join(file_name(&region.extract));
        if extract.is_dir() {
            fs::remove_dir_all(extract)?;
        }
    }
    Ok(())
}

/// The regions whose map detail is ready in the map cache `osm_dir`, by id; a record whose
/// extract is gone counts for none.
#[must_use]
pub fn installed(osm_dir: &Path) -> Vec<Region> {
    let Ok(entries) = fs::read_dir(osm_dir.join(DOWNLOADS)) else {
        return Vec::new();
    };
    let mut regions: Vec<Region> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| serde_json::from_slice(&fs::read(path).ok()?).ok())
        .filter(|region: &Region| {
            region.content >= CONTENT && osm_dir.join(VERSION).join(&region.extract).is_dir()
        })
        .collect();
    regions.sort_by(|a, b| a.id.cmp(&b.id));
    regions
}

/// The extracts a course opened here reads (R144): those of the regions ready here, and those
/// it `carried` of regions that are not; a region's extract here takes the place of an older
/// one the course carried.
#[must_use]
pub fn for_course(carried: &[String], ready: &[Region]) -> Vec<String> {
    let mut extracts: Vec<String> = ready.iter().map(|region| region.extract.clone()).collect();
    for extract in carried {
        let here = ready
            .iter()
            .any(|region| region_of(extract) == Some(file_name(&region.id).as_str()));
        if !here && !extracts.contains(extract) {
            extracts.push(extract.clone());
        }
    }
    extracts.sort();
    extracts
}

/// The region of an extract's name: all but its date (`-yyyy-mm-dd`) and what it holds
/// (`-c<content>`, if given).
fn region_of(extract: &str) -> Option<&str> {
    let extract = match extract.rsplit_once("-c") {
        Some((before, content))
            if !content.is_empty() && content.bytes().all(|b| b.is_ascii_digit()) =>
        {
            before
        }
        _ => extract,
    };
    let (region, date) = extract.split_at_checked(extract.len().checked_sub(11)?)?;
    let digits = |range: std::ops::Range<usize>| date[range].bytes().all(|b| b.is_ascii_digit());
    let dated = date.len() == 11
        && date.as_bytes()[0] == b'-'
        && date.as_bytes()[5] == b'-'
        && date.as_bytes()[8] == b'-'
        && digits(1..5)
        && digits(6..8)
        && digits(9..11);
    dated.then_some(region)
}

/// Makes the map detail of region `id` from its download (see [`download_path`]), the file
/// `source` Geofabrik served if known, and records it as ready under `name`. The extract of
/// an earlier download of the region goes: courses prepared with it carry what they used.
/// Once `stop` is set, it stops, the earlier extract still ready.
///
/// # Errors
/// [`OsmError::Cache`] if the download cannot be read or the detail written,
/// [`OsmError::Invalid`] if it is no OpenStreetMap download this can read,
/// [`OsmError::Stopped`] once stopped.
pub fn install(
    osm_dir: &Path,
    (id, name, source): (&str, &str, Option<&str>),
    progress: &mut dyn FnMut(Stage, u64, u64),
    stop: &AtomicBool,
) -> Result<Region, OsmError> {
    let made = extract(
        &download_path(osm_dir, id),
        &osm_dir.join(VERSION),
        id,
        progress,
        stop,
    )?;
    let record = record_path(osm_dir, id);
    let earlier: Option<Region> = fs::read(&record)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let region = Region {
        id: id.to_owned(),
        name: name.to_owned(),
        extract: made.name,
        data_time: made.data_time,
        source: source.map(ToOwned::to_owned),
        content: CONTENT,
    };
    let json = serde_json::to_vec_pretty(&region).map_err(|e| OsmError::Invalid(e.to_string()))?;
    let aside = record.with_extension("json.part");
    fs::write(&aside, json)?;
    fs::rename(&aside, &record)?;
    if let Some(earlier) = earlier
        && earlier.extract != region.extract
    {
        let old = osm_dir.join(VERSION).join(file_name(&earlier.extract));
        if old.is_dir() {
            fs::remove_dir_all(old)?;
        }
    }
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::super::extract::tests::{TestWay, test_file};
    use super::*;

    fn cache(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-regions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(DOWNLOADS)).unwrap();
        dir
    }

    const SETT: [TestWay; 1] = [TestWay {
        id: 10,
        tags: &[("highway", "residential"), ("surface", "sett")],
        nodes: &[1, 2],
    }];

    #[test]
    fn an_installed_region_is_listed_with_its_extract() {
        let osm = cache("install");
        assert_eq!(installed(&osm), []);
        fs::write(download_path(&osm, "bern"), test_file(&SETT, 1_792_875_600)).unwrap();

        let region = install(
            &osm,
            ("bern", "Bern", None),
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(region.extract, format!("bern-2026-10-24-c{CONTENT}"));
        assert_eq!(installed(&osm), vec![region]);
        fs::remove_dir_all(osm).unwrap();
    }

    #[test]
    fn a_newer_download_replaces_the_extract_of_the_earlier() {
        let osm = cache("update");
        fs::write(download_path(&osm, "bern"), test_file(&SETT, 1_792_875_600)).unwrap();
        install(
            &osm,
            ("bern", "Bern", None),
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        // A day later.
        fs::write(download_path(&osm, "bern"), test_file(&SETT, 1_792_962_000)).unwrap();

        let region = install(
            &osm,
            ("bern", "Bern", None),
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(region.extract, format!("bern-2026-10-25-c{CONTENT}"));
        assert_eq!(installed(&osm), vec![region]);
        let extracts: Vec<_> = fs::read_dir(osm.join(VERSION)).unwrap().collect();
        assert_eq!(extracts.len(), 1);
        fs::remove_dir_all(osm).unwrap();
    }

    #[test]
    fn the_regions_kept_are_listed_with_their_room_ready_or_not_and_deleted_whole() {
        let osm = cache("kept");
        fs::write(download_path(&osm, "bern"), test_file(&SETT, 1_792_875_600)).unwrap();
        let bern = install(
            &osm,
            ("bern", "Bern", Some("bern-261024.osm.pbf")),
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        // A download put there by hand, its detail not made yet, and one cut short.
        fs::write(download_path(&osm, "germany"), [0; 700]).unwrap();
        fs::write(partial_path(&osm, "us/california"), [0; 300]).unwrap();

        let listed = kept(&osm);

        let ids: Vec<_> = listed
            .iter()
            .map(|k| (k.id.as_str(), k.downloaded))
            .collect();
        assert_eq!(
            ids,
            [("bern", true), ("germany", true), ("us-california", false)]
        );
        assert_eq!(listed[0].ready.as_ref(), Some(&bern));
        assert_eq!(
            listed[0].ready.as_ref().unwrap().source.as_deref(),
            Some("bern-261024.osm.pbf")
        );
        let extract = dir_size(&osm.join(VERSION).join(&bern.extract));
        assert!(extract > 0);
        let download = fs::metadata(download_path(&osm, "bern")).unwrap().len();
        assert_eq!(listed[0].size, download + extract);
        assert_eq!((listed[1].ready.is_none(), listed[1].size), (true, 700));
        assert_eq!(listed[2].size, 300);

        remove(&osm, "bern").unwrap();
        remove(&osm, "us/california").unwrap();
        remove(&osm, "bern").expect("one already gone is no error");
        let left: Vec<_> = kept(&osm).into_iter().map(|k| k.id).collect();
        assert_eq!(left, ["germany"]);
        assert!(!osm.join(VERSION).join(&bern.extract).exists());
        fs::remove_dir_all(osm).unwrap();
    }

    #[test]
    fn a_course_opened_reads_the_regions_here_and_its_own_of_the_others() {
        let ready = |id: &str, extract: &str| Region {
            id: id.to_owned(),
            name: id.to_owned(),
            extract: extract.to_owned(),
            data_time: None,
            source: None,
            content: CONTENT,
        };
        let here = [
            ready("switzerland", "switzerland-2026-11-20"),
            ready("us/california", "us-california-2026-11-20"),
        ];
        let carried = [
            "switzerland-2026-10-24".to_owned(),
            "france-2026-10-24".to_owned(),
            "us-california-2026-10-01".to_owned(),
        ];

        assert_eq!(
            for_course(&carried, &here),
            [
                "france-2026-10-24",
                "switzerland-2026-11-20",
                "us-california-2026-11-20"
            ]
        );
        assert_eq!(for_course(&carried, &[]), {
            let mut own = carried.to_vec();
            own.sort();
            own
        });
        assert_eq!(region_of("bern-2026-10-24"), Some("bern"));
        assert_eq!(region_of("bern-2026-10-24-c2"), Some("bern"));
        assert_eq!(
            region_of("us-california-2026-11-20-c12"),
            Some("us-california")
        );
        assert_eq!(region_of("bern-latest"), None);
    }

    #[test]
    fn a_region_without_its_extract_is_not_ready() {
        let osm = cache("gone");
        fs::write(download_path(&osm, "bern"), test_file(&SETT, 1_792_875_600)).unwrap();
        let region = install(
            &osm,
            ("bern", "Bern", None),
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        fs::remove_dir_all(osm.join(VERSION).join(&region.extract)).unwrap();

        assert_eq!(installed(&osm), []);
        fs::remove_dir_all(osm).unwrap();
    }

    #[test]
    fn a_region_whose_extract_holds_less_than_buildings_is_read_again() {
        let osm = std::env::temp_dir().join(format!("orqwa-content-{}", std::process::id()));
        let _ = fs::remove_dir_all(&osm);
        fs::create_dir_all(osm.join(VERSION).join("bern-2026-10-24")).unwrap();
        fs::create_dir_all(osm.join(DOWNLOADS)).unwrap();
        let record = |content: &str| {
            format!(r#"{{"id":"bern","name":"Bern","extract":"bern-2026-10-24"{content}}}"#)
        };
        fs::write(osm.join(DOWNLOADS).join("bern.json"), record("")).unwrap();
        assert_eq!(installed(&osm), [], "made before extracts held buildings");
        fs::write(
            osm.join(DOWNLOADS).join("bern.json"),
            record(&format!(r#","content":{CONTENT}"#)),
        )
        .unwrap();
        assert_eq!(installed(&osm).len(), 1);
        fs::remove_dir_all(&osm).unwrap();
    }

    #[test]
    fn a_course_takes_its_regions_extract_made_again_for_more_content() {
        // The same download, extracted again now that extracts hold what buildings are: a
        // course carrying the extract from before takes the new one when opened (R144).
        let ready = Region {
            id: "bern".to_owned(),
            name: "Bern".to_owned(),
            extract: format!("bern-2026-10-24-c{CONTENT}"),
            data_time: None,
            source: None,
            content: CONTENT,
        };
        let carried = ["bern-2026-10-24".to_owned()];
        assert_eq!(
            for_course(&carried, std::slice::from_ref(&ready)),
            [ready.extract]
        );
    }
}
