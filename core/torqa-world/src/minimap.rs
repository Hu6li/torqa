//! A flat map of the corridor for the minimap: land cover, water, roads and buildings as one
//! coloured triangle list in metres east/north of the route start.

use torqa_osm::{LandCover, MapData};
use torqa_routes::LocalProjection;

use crate::CORRIDOR;
use crate::buildings::{signed_area, triangulate};
use crate::road::RoadIndex;

/// Background of the minimap (open land).
pub const BACKGROUND: [f32; 4] = [0.56, 0.68, 0.46, 1.0];
const WATER: [f32; 4] = [0.45, 0.62, 0.86, 1.0];
const BUILDING: [f32; 4] = [0.56, 0.53, 0.50, 1.0];
const MINOR_ROAD: [f32; 4] = [0.97, 0.97, 0.95, 1.0];
const MAJOR_ROAD: [f32; 4] = [1.0, 0.86, 0.55, 1.0];
/// Rings are simplified to this tolerance in metres; finer detail is invisible on the minimap.
const SIMPLIFY: f64 = 2.5;

/// Coloured triangles, three vertices each, in metres east/north of the route start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlatMap {
    /// Triangle corners (east, north).
    pub vertices: Vec<[f32; 2]>,
    /// One colour per vertex.
    pub colors: Vec<[f32; 4]>,
}

pub(crate) fn build(map: &MapData, projection: &LocalProjection, road: &RoadIndex) -> FlatMap {
    let mut flat = FlatMap::default();
    let near = |points: &[(f64, f64)]| {
        points
            .iter()
            .step_by((points.len() / 16).max(1))
            .any(|&(e, n)| road.nearest(e, n, CORRIDOR).is_some())
    };
    let project = |ring: &[(f64, f64)]| -> Vec<(f64, f64)> {
        ring.iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect()
    };

    // Land cover from general to specific, so specific areas paint over general ones.
    let mut areas: Vec<_> = map.areas.iter().collect();
    areas.sort_by_key(|a| layer(a.cover));
    for area in areas {
        let color = cover_color(area.cover);
        for ring in &area.outer {
            let ring = project(ring);
            if near(&ring) {
                fill(&mut flat, &ring, color);
            }
        }
        // Holes are painted with the background, approximating the cut-out.
        for ring in &area.inner {
            let ring = project(ring);
            if near(&ring) {
                fill(&mut flat, &ring, BACKGROUND);
            }
        }
    }
    for waterway in &map.waterways {
        ribbon(
            &mut flat,
            &project(&waterway.line),
            waterway.width.max(4.0),
            WATER,
            road,
        );
    }
    for minor_first in [false, true] {
        for street in map.roads.iter().filter(|r| r.major == minor_first) {
            let (width, color) = if street.major {
                (9.0, MAJOR_ROAD)
            } else {
                (5.0, MINOR_ROAD)
            };
            ribbon(&mut flat, &project(&street.line), width, color, road);
        }
    }
    for building in &map.buildings {
        let outline = project(&building.outline);
        if near(&outline) {
            fill(&mut flat, &outline, BUILDING);
        }
    }
    flat
}

/// Draw order: general land cover first.
fn layer(cover: LandCover) -> u8 {
    match cover {
        LandCover::Meadow => 0,
        LandCover::Farmland => 1,
        LandCover::Residential => 2,
        LandCover::Orchard => 3,
        LandCover::Forest => 4,
        LandCover::Rock => 5,
        LandCover::Water => 6,
    }
}

fn cover_color(cover: LandCover) -> [f32; 4] {
    match cover {
        LandCover::Meadow => [0.60, 0.74, 0.48, 1.0],
        LandCover::Farmland => [0.80, 0.78, 0.58, 1.0],
        LandCover::Residential => [0.78, 0.76, 0.73, 1.0],
        LandCover::Orchard => [0.64, 0.75, 0.47, 1.0],
        LandCover::Forest => [0.33, 0.52, 0.31, 1.0],
        LandCover::Rock => [0.72, 0.70, 0.67, 1.0],
        LandCover::Water => WATER,
    }
}

/// Fills a closed ring.
#[allow(clippy::cast_possible_truncation)] // drawing precision
fn fill(flat: &mut FlatMap, ring: &[(f64, f64)], color: [f32; 4]) {
    let mut points = simplify(ring, SIMPLIFY);
    points.pop(); // closing point
    if points.len() < 3 {
        return;
    }
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    for triangle in triangulate(&points) {
        for index in triangle {
            let (east, north) = points[index as usize];
            flat.vertices.push([east as f32, north as f32]);
            flat.colors.push(color);
        }
    }
}

/// A band `width` wide along the parts of a line within the corridor.
#[allow(clippy::cast_possible_truncation)] // drawing precision
fn ribbon(flat: &mut FlatMap, line: &[(f64, f64)], width: f64, color: [f32; 4], road: &RoadIndex) {
    let half = width / 2.0;
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let middle = (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
        if road.nearest(middle.0, middle.1, CORRIDOR).is_none() {
            continue;
        }
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length < 0.01 {
            continue;
        }
        let (re, rn) = ((b.1 - a.1) / length * half, -(b.0 - a.0) / length * half);
        let corners = [
            (a.0 - re, a.1 - rn),
            (a.0 + re, a.1 + rn),
            (b.0 + re, b.1 + rn),
            (b.0 - re, b.1 - rn),
        ];
        for index in [0, 1, 2, 0, 2, 3] {
            let (east, north) = corners[index];
            flat.vertices.push([east as f32, north as f32]);
            flat.colors.push(color);
        }
    }
}

/// Douglas–Peucker simplification keeping the first and last point.
fn simplify(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let (a, b) = (points[first], points[last]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        let distance = |p: (f64, f64)| {
            if length < 1e-9 {
                (p.0 - a.0).hypot(p.1 - a.1)
            } else {
                ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / length
            }
        };
        let farthest = (first + 1..last)
            .map(|i| (i, distance(points[i])))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((index, d)) = farthest
            && d > tolerance
        {
            keep[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, k)| k.then_some(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplification_keeps_corners_and_drops_noise() {
        let noisy = [
            (0.0, 0.0),
            (5.0, 0.4),
            (10.0, -0.3),
            (20.0, 0.0),
            (20.0, 20.0),
        ];

        assert_eq!(
            simplify(&noisy, 1.0),
            [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)]
        );
    }
}
