//! Opt-in course positions, separate from activities and shared course files (R220).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A rider's last explicitly remembered position on one prepared route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Fingerprint of the prepared route geometry, for invalidating changed routes.
    pub revision: String,
    /// Absolute distance along the course, in metres.
    pub distance_m: f64,
    /// Length of the prepared route, in metres.
    pub length_m: f64,
}

impl Point {
    /// Whether the position is finite and strictly inside this course.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.revision.is_empty()
            && self.distance_m.is_finite()
            && self.length_m.is_finite()
            && self.distance_m > 0.0
            && self.distance_m < self.length_m
    }
}

fn path(data: &Path, rider: &str, route: &str) -> io::Result<PathBuf> {
    if [rider, route].iter().any(|part| {
        let mut components = Path::new(part).components();
        part.contains(['/', '\\'])
            || !matches!(
                (components.next(), components.next()),
                (Some(std::path::Component::Normal(_)), None)
            )
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid continuation identity",
        ));
    }
    Ok(crate::profiles::profile_dir(data, rider)
        .join("continuations")
        .join(format!("{route}.json")))
}

/// Reads one rider's point. Missing files have no point; malformed files report an error.
///
/// # Errors
/// Invalid identifiers, unreadable files or invalid JSON/positions.
pub fn load(data: &Path, rider: &str, route: &str) -> io::Result<Option<Point>> {
    let bytes = match std::fs::read(path(data, rider, route)?) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let point: Point = serde_json::from_slice(&bytes)?;
    if !point.is_valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid continuation position",
        ));
    }
    Ok(Some(point))
}

/// Atomically replaces a point, or removes it without touching activities.
///
/// # Errors
/// Invalid identifiers/positions or a file-system failure.
pub fn save(data: &Path, rider: &str, route: &str, point: Option<&Point>) -> io::Result<()> {
    let path = path(data, rider, route)?;
    let Some(point) = point else {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    };
    if !point.is_valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid continuation position",
        ));
    }
    let bytes = serde_json::to_vec(point)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("json.part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_are_per_rider_and_route_and_reset_does_not_touch_rides() {
        let dir = std::env::temp_dir().join(format!("orqwa-continuation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let point = Point {
            revision: "geometry".into(),
            distance_m: 120.0,
            length_m: 500.0,
        };
        save(&dir, "alice", "route1", Some(&point)).unwrap();
        assert_eq!(load(&dir, "alice", "route1").unwrap(), Some(point.clone()));
        assert_eq!(load(&dir, "bob", "route1").unwrap(), None);
        assert_eq!(load(&dir, "alice", "route2").unwrap(), None);
        let rides = crate::profiles::rides_dir(&dir, "alice");
        std::fs::create_dir_all(&rides).unwrap();
        std::fs::write(rides.join("today.fit"), b"activity").unwrap();
        save(&dir, "alice", "route1", None).unwrap();
        save(&dir, "alice", "route1", None).unwrap();
        assert_eq!(load(&dir, "alice", "route1").unwrap(), None);
        assert!(rides.join("today.fit").exists());
        for distance in [0.0, -1.0, 500.0, f64::NAN, f64::INFINITY] {
            let bad = Point {
                distance_m: distance,
                ..point.clone()
            };
            assert!(save(&dir, "alice", "route1", Some(&bad)).is_err());
        }
        assert!(load(&dir, "../alice", "route1").is_err());
        assert!(load(&dir, "alice", "../route1").is_err());
        save(&dir, "andré", "route1", Some(&point)).unwrap();
        assert_eq!(load(&dir, "andré", "route1").unwrap(), Some(point));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
