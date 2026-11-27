//! A rider's own segments of a course (R302): stretches they time, such as a sprint or a
//! favourite climb, kept per rider and route like course positions, apart from activities and
//! the shared course files.
//!
//! Each segment is a file of its own, `profiles/<rider>/segments/<route>/<id>.json`, under an
//! id no other computer will choose (ADR 0044): segments added, changed or removed on two
//! computers before their data directory syncs come together as files, where one list would
//! leave a sync conflict. Each keeps the revision of the prepared route its ends were set on
//! ([`Segment::revision`]): the same course prepared again may lie differently along its
//! distance, so the rider checks the ends before the segment is timed again.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The segment file format written by this version. Later versions only add fields, which
/// this one passes over; a file of a newer format is left out and left alone.
pub const FORMAT_VERSION: u32 = 1;
/// The longest name a segment may have, in characters.
pub const MAX_NAME_CHARS: usize = 60;
/// The most segments one course may have.
pub const MAX_SEGMENTS: usize = 100;
/// The shortest segment, in metres: below a few seconds of riding, a time says nothing.
pub const MIN_LENGTH_M: f64 = 50.0;
/// The longest id, in characters; ORQWA's own are 32.
const MAX_ID_CHARS: usize = 64;

/// Where a segment lies on one revision of the prepared route, to the centimetre: times with
/// the same key are on the same stretch of road and compare. A segment moved, or the course
/// prepared differently, is another key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SegmentKey {
    revision: String,
    start_cm: i64,
    end_cm: i64,
}

impl SegmentKey {
    /// The key of the stretch from `start_m` to `end_m` on the prepared route `revision`.
    #[must_use]
    pub fn new(revision: &str, start_m: f64, end_m: f64) -> Self {
        #[allow(clippy::cast_possible_truncation)] // a course is far shorter than i64 cm
        let cm = |m: f64| (m * 100.0).round() as i64;
        Self {
            revision: revision.to_owned(),
            start_cm: cm(start_m),
            end_cm: cm(end_m),
        }
    }
}

/// Where a segment lay on an earlier revision of the prepared route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// The prepared route's revision.
    pub revision: String,
    /// Where the segment started on it, in metres from the course start.
    pub start_m: f64,
    /// Where it ended.
    pub end_m: f64,
}

/// One stretch of a course the rider times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Its own for good, on every computer: see [`new_id`].
    pub id: String,
    /// The rider's name for it.
    pub name: String,
    /// Where it starts, in metres from the course start.
    pub start_m: f64,
    /// Where it ends, in metres from the course start.
    pub end_m: f64,
    /// The revision of the prepared route (`orqwa_routes::Route::revision`) on which its ends
    /// were set or last checked by the rider.
    pub revision: String,
    /// Where it lay on earlier revisions of the route, as the rider confirmed when the course
    /// came out differently: times there count towards its best.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<Placement>,
    /// When it was marked, in milliseconds since the Unix epoch: the course lists its
    /// segments in that order.
    pub added_ms: u64,
}

impl Segment {
    /// Whether it has a name and lies forward along a course of `length_m`, long enough to time.
    #[must_use]
    pub fn is_valid(&self, length_m: f64) -> bool {
        let name = self.name.trim();
        valid_id(&self.id)
            && !name.is_empty()
            && name.chars().count() <= MAX_NAME_CHARS
            && !name.chars().any(char::is_control)
            && self.start_m.is_finite()
            && self.end_m.is_finite()
            && self.start_m >= 0.0
            && self.end_m <= length_m + 0.01
            && self.end_m - self.start_m >= MIN_LENGTH_M
    }

    /// Where it lies now.
    #[must_use]
    pub fn key(&self) -> SegmentKey {
        SegmentKey::new(&self.revision, self.start_m, self.end_m)
    }

    /// Every stretch whose times count for it: where it lies now and where it lay before.
    pub fn keys(&self) -> impl Iterator<Item = SegmentKey> + '_ {
        std::iter::once(self.key()).chain(
            self.earlier
                .iter()
                .map(|p| SegmentKey::new(&p.revision, p.start_m, p.end_m)),
        )
    }
}

#[derive(Serialize, Deserialize)]
struct SegmentFile {
    format: u32,
    #[serde(flatten)]
    segment: Segment,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A new segment id: 128 random bits as hexadecimal, from the randomly keyed hasher the
/// standard library seeds from the operating system, so two computers do not pick the same.
#[must_use]
pub fn new_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let half = || {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(now);
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

/// Milliseconds since the Unix epoch, for [`Segment::added_ms`].
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn dir(data: &Path, rider: &str, route: &str) -> io::Result<PathBuf> {
    crate::continuation::rider_route_dir(data, rider, "segments", route)
}

/// The segment files in `dir` with what they hold, by file name; files that are not segments
/// this ORQWA can read (from a newer one, half-written, something else) left out.
fn files(dir: &Path) -> io::Result<Vec<(PathBuf, Segment)>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    Ok(paths
        .into_iter()
        .filter_map(|path| {
            let file: SegmentFile = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            (file.format <= FORMAT_VERSION && valid_id(&file.segment.id))
                .then_some((path, file.segment))
        })
        .collect())
}

/// The rider's segments of the route `route` (its key), in the order they were added; none
/// without any. A segment a sync client left twice (a conflict copy beside its file) is listed
/// once, as its own file has it.
///
/// # Errors
/// Invalid identities or an unreadable directory.
pub fn load(data: &Path, rider: &str, route: &str) -> io::Result<Vec<Segment>> {
    let mut by_id: HashMap<String, (bool, Segment)> = HashMap::new();
    for (path, segment) in files(&dir(data, rider, route)?)? {
        let own = path.file_stem().is_some_and(|s| *s == *segment.id);
        match by_id.get(&segment.id) {
            Some((true, _)) => {}
            Some((false, _)) if !own => {}
            _ => {
                by_id.insert(segment.id.clone(), (own, segment));
            }
        }
    }
    let mut segments: Vec<Segment> = by_id.into_values().map(|(_, s)| s).collect();
    segments.sort_by(|a, b| a.added_ms.cmp(&b.added_ms).then_with(|| a.id.cmp(&b.id)));
    Ok(segments)
}

/// Writes `segment` of `route`, on a course of `length_m`, as its own file: whole or not at
/// all, leaving the course's other segments be.
///
/// # Errors
/// Invalid identities, an invalid segment, one too many, or a file-system failure.
pub fn save(
    data: &Path,
    rider: &str,
    route: &str,
    length_m: f64,
    segment: &Segment,
) -> io::Result<()> {
    if !segment.is_valid(length_m) {
        return Err(invalid("invalid segment"));
    }
    let dir = dir(data, rider, route)?;
    let path = dir.join(format!("{}.json", segment.id));
    if !path.exists() && load(data, rider, route)?.len() >= MAX_SEGMENTS {
        return Err(invalid("too many segments"));
    }
    let file = SegmentFile {
        format: FORMAT_VERSION,
        segment: segment.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&file)?;
    std::fs::create_dir_all(&dir)?;
    let partial = path.with_extension("json.part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })
}

/// Removes the segment `id` of `route`, with any copy of it a sync client left.
///
/// # Errors
/// Invalid identities or a file-system failure.
pub fn remove(data: &Path, rider: &str, route: &str, id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid segment id",
        ));
    }
    let dir = dir(data, rider, route)?;
    let own = dir.join(format!("{id}.json"));
    for (path, segment) in files(&dir)? {
        if segment.id == id && path != own {
            std::fs::remove_file(path)?;
        }
    }
    match std::fs::remove_file(own) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orqwa-segments-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sprint(id: &str, start_m: f64, end_m: f64) -> Segment {
        Segment {
            id: id.to_owned(),
            name: format!("Sprint {id}"),
            start_m,
            end_m,
            revision: "r1".to_owned(),
            earlier: Vec::new(),
            added_ms: 1_800_000_000_000,
        }
    }

    #[test]
    fn segments_are_per_rider_and_route_one_file_each() {
        let dir = temp_dir("files");
        let first = Segment {
            added_ms: 2,
            ..sprint("b2", 100.0, 300.0)
        };
        let second = Segment {
            added_ms: 1,
            ..sprint("a1", 2000.0, 5000.0)
        };
        save(&dir, "alice", "route1", 5000.0, &first).unwrap();
        save(&dir, "alice", "route1", 5000.0, &second).unwrap();
        assert_eq!(
            load(&dir, "alice", "route1").unwrap(),
            [second.clone(), first.clone()],
            "in the order they were added"
        );
        assert!(dir.join("profiles/alice/segments/route1/b2.json").exists());
        assert_eq!(load(&dir, "bob", "route1").unwrap(), []);
        assert_eq!(load(&dir, "alice", "route2").unwrap(), []);

        remove(&dir, "alice", "route1", "a1").unwrap();
        remove(&dir, "alice", "route1", "a1").unwrap();
        assert_eq!(load(&dir, "alice", "route1").unwrap(), [first]);
        assert!(load(&dir, "../alice", "route1").is_err());
        assert!(load(&dir, "alice", "../route1").is_err());
        assert!(remove(&dir, "alice", "route1", "../b2").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changes_on_two_computers_come_together_through_a_sync() {
        let here = temp_dir("here");
        let there = temp_dir("there");
        let both = sprint("shared", 100.0, 300.0);
        for dir in [&here, &there] {
            save(dir, "alice", "r", 5000.0, &both).unwrap();
        }
        // Here a segment is added; there the shared one is renamed.
        let added = Segment {
            id: new_id(),
            added_ms: 1_800_000_000_001,
            ..sprint("x", 1000.0, 1500.0)
        };
        save(&here, "alice", "r", 5000.0, &added).unwrap();
        let renamed = Segment {
            name: "Village sprint".to_owned(),
            ..both.clone()
        };
        save(&there, "alice", "r", 5000.0, &renamed).unwrap();

        // A sync copies each changed file across.
        let folder = "profiles/alice/segments/r";
        std::fs::copy(
            here.join(folder).join(format!("{}.json", added.id)),
            there.join(folder).join(format!("{}.json", added.id)),
        )
        .unwrap();
        std::fs::copy(
            there.join(folder).join("shared.json"),
            here.join(folder).join("shared.json"),
        )
        .unwrap();
        for dir in [&here, &there] {
            assert_eq!(
                load(dir, "alice", "r").unwrap(),
                [renamed.clone(), added.clone()]
            );
        }

        // Both change the same one: the sync keeps a conflict copy beside its file.
        std::fs::write(
            here.join(folder).join("shared (conflicted copy).json"),
            serde_json::to_vec(&SegmentFile {
                format: FORMAT_VERSION,
                segment: both,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            load(&here, "alice", "r").unwrap(),
            [renamed, added.clone()],
            "listed once, as its own file has it"
        );
        remove(&here, "alice", "r", "shared").unwrap();
        assert_eq!(load(&here, "alice", "r").unwrap(), [added], "copy and all");
        std::fs::remove_dir_all(here).unwrap();
        std::fs::remove_dir_all(there).unwrap();
    }

    #[test]
    fn ids_differ_and_suit_file_names() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(valid_id(&a));
    }

    #[test]
    fn only_named_forward_stretches_of_the_course_are_kept() {
        let dir = temp_dir("bad");
        let unnamed = Segment {
            name: "  ".into(),
            ..sprint("a", 0.0, 200.0)
        };
        let long_name = Segment {
            name: "x".repeat(MAX_NAME_CHARS + 1),
            ..sprint("a", 0.0, 200.0)
        };
        for bad in [
            unnamed,
            long_name,
            sprint("a", 300.0, 100.0),
            sprint("a", 100.0, 120.0),
            sprint("a", -10.0, 200.0),
            sprint("a", 100.0, 1200.0),
            sprint("a", f64::NAN, 200.0),
            sprint("a/b", 0.0, 200.0),
            sprint("", 0.0, 200.0),
        ] {
            assert!(save(&dir, "alice", "r", 1000.0, &bad).is_err(), "{bad:?}");
        }
        assert_eq!(load(&dir, "alice", "r").unwrap(), []);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_course_keeps_at_most_its_share_of_segments() {
        let dir = temp_dir("many");
        for i in 0..MAX_SEGMENTS {
            save(
                &dir,
                "alice",
                "r",
                1000.0,
                &sprint(&format!("s{i}"), 0.0, 200.0),
            )
            .unwrap();
        }
        assert!(save(&dir, "alice", "r", 1000.0, &sprint("one", 0.0, 200.0)).is_err());
        let renamed = Segment {
            name: "Still fits".to_owned(),
            ..sprint("s0", 0.0, 200.0)
        };
        save(&dir, "alice", "r", 1000.0, &renamed).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_newer_format_is_left_out_and_left_alone() {
        let dir = temp_dir("newer");
        let folder = dir.join("profiles/alice/segments/r");
        std::fs::create_dir_all(&folder).unwrap();
        let newer = br#"{"format": 2, "id": "n", "name": "x", "start_m": 0, "end_m": 100,
            "revision": "r1", "added_ms": 0}"#;
        std::fs::write(folder.join("n.json"), newer).unwrap();
        save(&dir, "alice", "r", 1000.0, &sprint("a", 0.0, 200.0)).unwrap();

        assert_eq!(load(&dir, "alice", "r").unwrap(), [sprint("a", 0.0, 200.0)]);
        assert_eq!(std::fs::read(folder.join("n.json")).unwrap(), newer);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_segment_counts_where_it_lies_and_lay() {
        let mut segment = sprint("a", 100.0, 300.0);
        assert_eq!(segment.key(), SegmentKey::new("r1", 100.0, 300.0));
        assert_eq!(segment.key(), SegmentKey::new("r1", 100.001, 299.999));
        assert_ne!(segment.key(), SegmentKey::new("r2", 100.0, 300.0));
        assert_ne!(segment.key(), SegmentKey::new("r1", 100.0, 301.0));

        segment.earlier.push(Placement {
            revision: "r0".to_owned(),
            start_m: 98.0,
            end_m: 297.5,
        });
        let keys: Vec<SegmentKey> = segment.keys().collect();
        assert_eq!(
            keys,
            [
                SegmentKey::new("r1", 100.0, 300.0),
                SegmentKey::new("r0", 98.0, 297.5)
            ]
        );
    }
}
