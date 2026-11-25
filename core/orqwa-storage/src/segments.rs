//! A rider's own segments of a course (R300): stretches they time, such as a sprint or a
//! favourite climb, kept per rider and route like course positions, apart from activities and
//! the shared course files.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The segment file format written by this version; newer files are refused, not misread.
pub const FORMAT_VERSION: u32 = 1;
/// The longest name a segment may have, in characters.
pub const MAX_NAME_CHARS: usize = 60;
/// The most segments one course may have.
pub const MAX_SEGMENTS: usize = 100;
/// The shortest segment, in metres: below a few seconds of riding, a time says nothing.
pub const MIN_LENGTH_M: f64 = 50.0;

/// One stretch of a course the rider times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Unique among the course's segments, so one can be changed or removed.
    pub id: u32,
    /// The rider's name for it.
    pub name: String,
    /// Where it starts, in metres from the course start.
    pub start_m: f64,
    /// Where it ends, in metres from the course start.
    pub end_m: f64,
}

impl Segment {
    /// Whether it has a name and lies forward along a course of `length_m`, long enough to time.
    #[must_use]
    pub fn is_valid(&self, length_m: f64) -> bool {
        let name = self.name.trim();
        !name.is_empty()
            && name.chars().count() <= MAX_NAME_CHARS
            && !name.chars().any(char::is_control)
            && self.start_m.is_finite()
            && self.end_m.is_finite()
            && self.start_m >= 0.0
            && self.end_m <= length_m + 0.01
            && self.end_m - self.start_m >= MIN_LENGTH_M
    }
}

#[derive(Serialize, Deserialize)]
struct SegmentsFile {
    format: u32,
    segments: Vec<Segment>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The rider's segments of the route `route` (its key), in the order they were added; none
/// without a file.
///
/// # Errors
/// Invalid identities, an unreadable file, invalid JSON or a newer format.
pub fn load(data: &Path, rider: &str, route: &str) -> io::Result<Vec<Segment>> {
    let path = crate::continuation::rider_route_file(data, rider, "segments", route)?;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let file: SegmentsFile = serde_json::from_slice(&bytes)?;
    if file.format > FORMAT_VERSION {
        return Err(invalid("segments written by a newer ORQWA"));
    }
    Ok(file.segments)
}

/// Replaces the rider's segments of `route` on a course of `length_m`, written whole or not at
/// all; no segments remove the file.
///
/// # Errors
/// Invalid identities, an invalid or repeated segment, too many, or a file-system failure.
pub fn save(
    data: &Path,
    rider: &str,
    route: &str,
    length_m: f64,
    segments: &[Segment],
) -> io::Result<()> {
    let path = crate::continuation::rider_route_file(data, rider, "segments", route)?;
    if segments.len() > MAX_SEGMENTS {
        return Err(invalid("too many segments"));
    }
    if segments.iter().any(|s| !s.is_valid(length_m)) {
        return Err(invalid("invalid segment"));
    }
    if segments
        .iter()
        .enumerate()
        .any(|(i, s)| segments[..i].iter().any(|other| other.id == s.id))
    {
        return Err(invalid("repeated segment id"));
    }
    if segments.is_empty() {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    let file = SegmentsFile {
        format: FORMAT_VERSION,
        segments: segments.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&file)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("json.part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprint(id: u32, start_m: f64, end_m: f64) -> Segment {
        Segment {
            id,
            name: format!("Sprint {id}"),
            start_m,
            end_m,
        }
    }

    #[test]
    fn segments_are_per_rider_and_route() {
        let dir = std::env::temp_dir().join(format!("orqwa-segments-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let segments = vec![sprint(1, 100.0, 300.0), sprint(2, 2000.0, 5000.0)];
        save(&dir, "alice", "route1", 5000.0, &segments).unwrap();
        assert_eq!(load(&dir, "alice", "route1").unwrap(), segments);
        assert_eq!(load(&dir, "bob", "route1").unwrap(), []);
        assert_eq!(load(&dir, "alice", "route2").unwrap(), []);
        save(&dir, "alice", "route1", 5000.0, &[]).unwrap();
        save(&dir, "alice", "route1", 5000.0, &[]).unwrap();
        assert_eq!(load(&dir, "alice", "route1").unwrap(), []);
        assert!(load(&dir, "../alice", "route1").is_err());
        assert!(load(&dir, "alice", "../route1").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_named_forward_stretches_of_the_course_are_kept() {
        let dir = std::env::temp_dir().join(format!("orqwa-segments-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let unnamed = Segment {
            name: "  ".into(),
            ..sprint(1, 0.0, 200.0)
        };
        let long_name = Segment {
            name: "x".repeat(MAX_NAME_CHARS + 1),
            ..sprint(1, 0.0, 200.0)
        };
        for bad in [
            unnamed,
            long_name,
            sprint(1, 300.0, 100.0),
            sprint(1, 100.0, 120.0),
            sprint(1, -10.0, 200.0),
            sprint(1, 100.0, 1200.0),
            sprint(1, f64::NAN, 200.0),
        ] {
            assert!(save(&dir, "alice", "r", 1000.0, &[bad]).is_err());
        }
        let twice = [sprint(1, 0.0, 200.0), sprint(1, 400.0, 600.0)];
        assert!(save(&dir, "alice", "r", 1000.0, &twice).is_err());
        assert_eq!(load(&dir, "alice", "r").unwrap(), []);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_newer_format_is_refused() {
        let dir = std::env::temp_dir().join(format!("orqwa-segments-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = crate::continuation::rider_route_file(&dir, "alice", "segments", "r").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"format": 2, "segments": []}"#).unwrap();
        assert!(load(&dir, "alice", "r").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
