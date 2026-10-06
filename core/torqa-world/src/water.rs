//! Water: lakes, ponds and rivers mapped as areas, and streams and rivers mapped as lines, laid
//! on the ground of each chunk just above it (see `drape`). The terrain model measures the
//! water's surface where there is water, so laid on the ground lakes lie level, rivers slope
//! with their course, and neither floats above the land nor sinks below it.

use torqa_osm::{Area, LandCover, Waterway};
use torqa_routes::LocalProjection;

use crate::buildings::{signed_area, triangulate};
use crate::minimap::simplify;
use crate::road::RoadIndex;
use crate::{CORRIDOR, HeightGrid, MeshData, drape};

/// Shores are simplified to this tolerance in metres.
const SHORE_SIMPLIFY: f64 = 3.0;
/// Waterway lines are sampled this often.
const STEP: f64 = 10.0;
/// Water lies this far above the ground: below every street and the road ridden
/// (`streets::lift`, `ROAD_SINK`), so where it meets one it passes under it.
const LIFT: f64 = 0.02;

/// A stream or river of the map near the route, in metres east/north, with its bounds for
/// quick chunk tests.
pub(crate) struct Stream {
    width: f64,
    points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
}

/// The map's streams and rivers, their stretches within the corridor around the road.
pub(crate) fn streams(
    waterways: &[Waterway],
    projection: &LocalProjection,
    road: &RoadIndex,
) -> Vec<Stream> {
    let mut streams = Vec::new();
    let mut add = |points: &mut Vec<(f64, f64)>, width: f64| {
        let taken = std::mem::take(points);
        let Some(&first) = taken.first() else {
            return;
        };
        if taken.len() < 2 {
            return;
        }
        let (mut min, mut max) = (first, first);
        for &(e, n) in &taken {
            min = (min.0.min(e), min.1.min(n));
            max = (max.0.max(e), max.1.max(n));
        }
        streams.push(Stream {
            width,
            points: taken,
            min,
            max,
        });
    };
    for waterway in waterways {
        let line: Vec<(f64, f64)> = waterway
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let mut run = Vec::new();
        for (east, north) in drape::densify(&line, STEP) {
            if road.nearest(east, north, CORRIDOR).is_some() {
                run.push((east, north));
            } else {
                add(&mut run, waterway.width);
            }
        }
        add(&mut run, waterway.width);
    }
    streams
}

/// A lake, pond or river mapped as an area near the route: triangles (metres east/north,
/// clockwise seen from above) with their bounds for quick chunk tests.
pub(crate) struct Pool {
    triangles: Vec<[(f64, f64); 3]>,
    min: (f64, f64),
    max: (f64, f64),
}

/// The map's water areas that reach into the corridor around the road, triangulated.
pub(crate) fn pools(areas: &[Area], projection: &LocalProjection, road: &RoadIndex) -> Vec<Pool> {
    let mut pools = Vec::new();
    for area in areas.iter().filter(|a| a.cover == LandCover::Water) {
        for ring in &area.outer {
            let projected: Vec<(f64, f64)> = ring
                .iter()
                .map(|&(lat, lon)| projection.project(lat, lon))
                .collect();
            let near = projected
                .iter()
                .step_by((projected.len() / 32).max(1))
                .any(|&(e, n)| road.nearest(e, n, CORRIDOR).is_some());
            if !near {
                continue;
            }
            let mut outline = simplify(&projected, SHORE_SIMPLIFY);
            outline.pop(); // closing point
            if outline.len() < 3 {
                continue;
            }
            if signed_area(&outline) < 0.0 {
                outline.reverse();
            }
            // Counter-clockwise outline, so counter-clockwise triangles: turned clockwise.
            let triangles: Vec<[(f64, f64); 3]> = triangulate(&outline)
                .into_iter()
                .map(|[a, b, c]| [c, b, a].map(|k| outline[k as usize]))
                .collect();
            let (mut min, mut max) = (outline[0], outline[0]);
            for &(e, n) in &outline {
                min = (min.0.min(e), min.1.min(n));
                max = (max.0.max(e), max.1.max(n));
            }
            pools.push(Pool {
                triangles,
                min,
                max,
            });
        }
    }
    pools
}

/// The water within the chunk square `[origin, origin + size]`, relative to `chunk_origin`:
/// streams and pools laid on the chunk's ground.
pub(crate) fn mesh(
    streams: &[Stream],
    pools: &[Pool],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    chunk_origin: [f64; 3],
) -> MeshData {
    let mut mesh = stream_mesh(streams, origin, size, heights, chunk_origin);
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
    let outside = |min: (f64, f64), max: (f64, f64)| {
        max.0 < low.0 || min.0 > high.0 || max.1 < low.1 || min.1 > high.1
    };
    for pool in pools.iter().filter(|p| !outside(p.min, p.max)) {
        for triangle in &pool.triangles {
            let (mut min, mut max) = (triangle[0], triangle[0]);
            for &(e, n) in triangle {
                min = (min.0.min(e), min.1.min(n));
                max = (max.0.max(e), max.1.max(n));
            }
            if !outside(min, max) {
                drape::drape_polygon(&mut mesh, triangle, LIFT, heights, chunk_origin, &|_| {
                    [0.0, 0.0]
                });
            }
        }
    }
    mesh
}

/// The streams within the chunk square `[origin, origin + size]`, relative to `chunk_origin`,
/// laid on the chunk's ground (see `drape`): water lies in the land, never floating above it
/// or sunk below it.
fn stream_mesh(
    streams: &[Stream],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    chunk_origin: [f64; 3],
) -> MeshData {
    let mut mesh = MeshData::default();
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
    for stream in streams {
        if stream.max.0 < low.0
            || stream.min.0 > high.0
            || stream.max.1 < low.1
            || stream.min.1 > high.1
        {
            continue;
        }
        for piece in drape::pieces(&stream.points, low, high) {
            drape::drape(
                &mut mesh,
                &piece,
                stream.width / 2.0,
                LIFT,
                heights,
                chunk_origin,
            );
        }
    }
    mesh
}
