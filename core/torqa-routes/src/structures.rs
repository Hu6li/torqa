//! Bridges and tunnels along the route.

use torqa_osm::{Structure, StructureKind};

use crate::Surface;
use crate::gpx::RawPoint;

/// A structure of a road the route was put on belongs to the route where its centre line is
/// this close to a route point (the route is smoothed a little off the map's lines).
const MATCH_DISTANCE: f64 = 4.0;
/// ...and runs in the same direction within this angle, so roads passing above or below the
/// route are not mistaken for it.
const MAX_ANGLE_DEGREES: f64 = 30.0;
const METERS_PER_DEGREE: f64 = 111_195.0;

/// What carries the road at each point.
pub(crate) fn surfaces(points: &[RawPoint], structures: &[Structure]) -> Vec<Surface> {
    let candidates: Vec<(&Structure, Bounds)> = structures
        .iter()
        .map(|s| (s, Bounds::of(&s.line)))
        .collect();
    points
        .iter()
        .enumerate()
        .map(|(i, point)| {
            let next = points
                .get(i + 1)
                .or_else(|| i.checked_sub(1).map(|j| &points[j]));
            let Some(neighbour) = next else {
                return Surface::Ground;
            };
            let direction = local(point, neighbour.lat, neighbour.lon);
            candidates
                .iter()
                .filter(|(_, bounds)| bounds.contains(point.lat, point.lon))
                .find(|(structure, _)| carries(structure, point, direction))
                .map_or(Surface::Ground, |(structure, _)| match structure.kind {
                    StructureKind::Bridge => Surface::Bridge,
                    StructureKind::Tunnel => Surface::Tunnel,
                })
        })
        .collect()
}

/// Bridges and tunnels shorter than this many route points are not built: a structure barely
/// touched (a crossing, a corner of it) would become a short tube or deck on the open road.
const MIN_POINTS: usize = 2;
/// With terrain data, a tunnel needs at least this much ground above its road...
const TUNNEL_COVER: f64 = 4.0;
/// ...and a bridge at least this much clearance above the ground below its deck; elsewhere
/// the map's structure is an underpass, a culvert or a gallery and the road stays on the
/// ground, which looks right.
const BRIDGE_CLEARANCE: f64 = 2.0;

/// Turns bridges and tunnels back into ground where they are too short, or where `terrain`
/// elevations (the ground's, before `bridge_elevations`) do not bear them out.
pub(crate) fn keep_real(points: &[RawPoint], surfaces: &mut [Surface], terrain: bool) {
    let mut i = 0;
    while i < points.len() {
        let kind = surfaces[i];
        if kind == Surface::Ground {
            i += 1;
            continue;
        }
        let start = i;
        while i < points.len() && surfaces[i] == kind {
            i += 1;
        }
        let long_enough = i - start >= MIN_POINTS;
        let borne_out = !terrain || ground_bears_out(points, start, i, kind);
        if !long_enough || !borne_out {
            surfaces[start..i].fill(Surface::Ground);
        }
    }
}

/// Whether the ground along points `start..end` lies far enough above (tunnel) or below
/// (bridge) the straight road between the points on either side.
fn ground_bears_out(points: &[RawPoint], start: usize, end: usize, kind: Surface) -> bool {
    // A structure at the route's start or end has no outer end to compare with.
    let (Some(before), Some(after)) = (start.checked_sub(1), (end < points.len()).then_some(end))
    else {
        return true;
    };
    let (Some(from), Some(to)) = (points[before].elevation, points[after].elevation) else {
        return true;
    };
    #[allow(clippy::cast_precision_loss)] // indices are far below 2^52
    let span = (after - before) as f64;
    let deepest = points[start..end]
        .iter()
        .enumerate()
        .filter_map(|(offset, point)| {
            #[allow(clippy::cast_precision_loss)]
            let road = from + (to - from) * (offset + 1) as f64 / span;
            point.elevation.map(|ground| match kind {
                Surface::Tunnel => ground - road,
                _ => road - ground,
            })
        })
        .fold(f64::NEG_INFINITY, f64::max);
    deepest
        >= match kind {
            Surface::Tunnel => TUNNEL_COVER,
            _ => BRIDGE_CLEARANCE,
        }
}

/// Replaces elevations on bridges and in tunnels by a straight line between their ends.
pub(crate) fn bridge_elevations(points: &mut [RawPoint], surfaces: &[Surface]) {
    let mut i = 0;
    while i < points.len() {
        if surfaces[i] == Surface::Ground {
            i += 1;
            continue;
        }
        let start = i;
        while i < points.len() && surfaces[i] != Surface::Ground {
            i += 1;
        }
        // A section touching the route start or end has no outer end to aim for.
        let (Some(before), Some(after)) = (start.checked_sub(1), (i < points.len()).then_some(i))
        else {
            continue;
        };
        let (Some(from), Some(to)) = (points[before].elevation, points[after].elevation) else {
            continue;
        };
        #[allow(clippy::cast_precision_loss)] // indices are far below 2^52
        let span = (after - before) as f64;
        for (offset, point) in points[start..after].iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let t = (offset + 1) as f64 / span;
            point.elevation = Some(from + (to - from) * t);
        }
    }
}

/// Whether `structure` carries the road at `point`, travelling in `direction`.
fn carries(structure: &Structure, point: &RawPoint, direction: (f64, f64)) -> bool {
    structure.line.windows(2).any(|pair| {
        let a = local(point, pair[0].0, pair[0].1);
        let b = local(point, pair[1].0, pair[1].1);
        let segment = (b.0 - a.0, b.1 - a.1);
        distance_to_segment(a, segment) <= MATCH_DISTANCE && parallel(segment, direction)
    })
}

/// Metres east/north of `origin` (flat approximation, fine over tens of metres).
fn local(origin: &RawPoint, lat: f64, lon: f64) -> (f64, f64) {
    (
        (lon - origin.lon) * METERS_PER_DEGREE * origin.lat.to_radians().cos(),
        (lat - origin.lat) * METERS_PER_DEGREE,
    )
}

/// Distance from the origin to the segment starting at `a`.
fn distance_to_segment(a: (f64, f64), segment: (f64, f64)) -> f64 {
    let length_squared = segment.0 * segment.0 + segment.1 * segment.1;
    let t = if length_squared > 0.0 {
        (-(a.0 * segment.0 + a.1 * segment.1) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + segment.0 * t).hypot(a.1 + segment.1 * t)
}

/// Whether two directions are parallel or opposite within the angle limit.
fn parallel(a: (f64, f64), b: (f64, f64)) -> bool {
    let lengths = a.0.hypot(a.1) * b.0.hypot(b.1);
    if lengths == 0.0 {
        return false;
    }
    let cos = ((a.0 * b.0 + a.1 * b.1) / lengths).abs();
    cos >= MAX_ANGLE_DEGREES.to_radians().cos()
}

/// A latitude/longitude box around a structure, widened by the match distance.
struct Bounds {
    south: f64,
    north: f64,
    west: f64,
    east: f64,
}

impl Bounds {
    fn of(line: &[(f64, f64)]) -> Self {
        let margin = MATCH_DISTANCE * 2.0 / METERS_PER_DEGREE;
        let mut bounds = Self {
            south: f64::INFINITY,
            north: f64::NEG_INFINITY,
            west: f64::INFINITY,
            east: f64::NEG_INFINITY,
        };
        for &(lat, lon) in line {
            bounds.south = bounds.south.min(lat - margin);
            bounds.north = bounds.north.max(lat + margin);
            // Longitude degrees shrink towards the poles; doubling the margin is ample here.
            bounds.west = bounds.west.min(lon - margin * 2.0);
            bounds.east = bounds.east.max(lon + margin * 2.0);
        }
        bounds
    }

    fn contains(&self, lat: f64, lon: f64) -> bool {
        (self.south..=self.north).contains(&lat) && (self.west..=self.east).contains(&lon)
    }
}
