//! The streets, tracks and paths of the map around the route, other than the road ridden,
//! draped on the terrain of each chunk (from its own heights, so they sit exactly on the ground
//! the rider sees).

use torqa_osm::{MapData, RoadClass};
use torqa_routes::LocalProjection;

use crate::road::RoadIndex;
use crate::{HeightGrid, MeshData, ROAD_HALF_WIDTH};

/// Distance between the points of a draped street: short enough to follow the ground.
const STEP_M: f64 = 3.0;
/// Streets lie this far above the ground, so the terrain does not show through.
const LIFT_M: f64 = 0.12;

/// A street of the map in metres east/north, with its bounds for quick chunk tests.
pub(crate) struct Street {
    class: RoadClass,
    points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
}

/// Width in metres by kind of way.
fn width(class: RoadClass) -> f64 {
    match class {
        RoadClass::Major => 7.0,
        RoadClass::Street => 5.5,
        RoadClass::Service => 3.5,
        RoadClass::Track => 2.8,
        RoadClass::Path => 1.6,
    }
}

/// Whether a way is paved (asphalt) rather than gravel or dirt.
fn paved(class: RoadClass) -> bool {
    matches!(
        class,
        RoadClass::Major | RoadClass::Street | RoadClass::Service
    )
}

/// The map's streets around the route, densified. Bridges and tunnels are left out: draped on
/// the ground, a tunnel would run over the mountain.
pub(crate) fn lines(map: &MapData, projection: &LocalProjection) -> Vec<Street> {
    map.roads
        .iter()
        .filter(|r| r.structure.is_none())
        .filter_map(|road| {
            let line: Vec<(f64, f64)> = road
                .line
                .iter()
                .map(|&(lat, lon)| projection.project(lat, lon))
                .collect();
            let points = densify(&line);
            let (mut min, mut max) = (points.first()?.to_owned(), points.first()?.to_owned());
            for &(e, n) in &points {
                min = (min.0.min(e), min.1.min(n));
                max = (max.0.max(e), max.1.max(n));
            }
            Some(Street {
                class: road.class,
                points,
                min,
                max,
            })
        })
        .collect()
}

/// The paved streets and the unpaved tracks and paths within the chunk square `[origin,
/// origin + size]`, relative to `chunk_origin`, leaving out what runs along or across the road
/// ridden (drawn there already).
pub(crate) fn meshes(
    streets: &[Street],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    road: &RoadIndex,
    chunk_origin: [f64; 3],
) -> (MeshData, MeshData) {
    let (mut paved_mesh, mut unpaved_mesh) = (MeshData::default(), MeshData::default());
    // A step past the edges, so the pieces of neighbouring chunks meet.
    let (low, high) = (
        (origin.0 - STEP_M, origin.1 - STEP_M),
        (origin.0 + size + STEP_M, origin.1 + size + STEP_M),
    );
    for street in streets {
        if street.max.0 < low.0
            || street.min.0 > high.0
            || street.max.1 < low.1
            || street.min.1 > high.1
        {
            continue;
        }
        let half = width(street.class) / 2.0;
        let target = if paved(street.class) {
            &mut paved_mesh
        } else {
            &mut unpaved_mesh
        };
        let mut run: Vec<(f64, f64)> = Vec::new();
        for &(e, n) in &street.points {
            let inside = e >= low.0 && e <= high.0 && n >= low.1 && n <= high.1;
            let on_route = road.nearest(e, n, ROAD_HALF_WIDTH + half + 0.5).is_some();
            if inside && !on_route {
                run.push((e, n));
            } else {
                ribbon(target, &run, half, heights, chunk_origin);
                run.clear();
            }
        }
        ribbon(target, &run, half, heights, chunk_origin);
    }
    (paved_mesh, unpaved_mesh)
}

/// Points along `line` at most `STEP_M` apart.
fn densify(line: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for pair in line.windows(2) {
        let ((e0, n0), (e1, n1)) = (pair[0], pair[1]);
        let length = (e1 - e0).hypot(n1 - n0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short segments
        let steps = (length / STEP_M).ceil().max(1.0) as usize;
        for step in 0..steps {
            #[allow(clippy::cast_precision_loss)]
            let t = step as f64 / steps as f64;
            points.push((e0 + (e1 - e0) * t, n0 + (n1 - n0) * t));
        }
    }
    points.extend(line.last());
    points
}

/// A strip of `half` width along `run`, each edge on the ground below it.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn ribbon(
    mesh: &mut MeshData,
    run: &[(f64, f64)],
    half: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    if run.len() < 2 {
        return;
    }
    let base = u32::try_from(mesh.vertices.len()).expect("streets fit u32");
    let mut distance = 0.0;
    for (i, &(east, north)) in run.iter().enumerate() {
        let (before, after) = (run[i.saturating_sub(1)], run[(i + 1).min(run.len() - 1)]);
        let (de, dn) = (after.0 - before.0, after.1 - before.1);
        let length = de.hypot(dn).max(1e-6);
        // Right of travel is the direction turned clockwise by 90°.
        let (re, rn) = (dn / length, -de / length);
        if i > 0 {
            distance += (east - run[i - 1].0).hypot(north - run[i - 1].1);
        }
        for (side, u) in [(-1.0, 0.0), (1.0, 1.0)] {
            let (e, n) = (east + re * half * side, north + rn * half * side);
            let height = heights.at(e, n) + LIFT_M;
            // The ground's slope here, for shading like the terrain around.
            let slope_e = (heights.at(e + 1.0, n) - heights.at(e - 1.0, n)) / 2.0;
            let slope_n = (heights.at(e, n + 1.0) - heights.at(e, n - 1.0)) / 2.0;
            let normal = normalize([-slope_e, 1.0, slope_n]);
            mesh.vertices.push([
                (e - origin[0]) as f32,
                (height - origin[1]) as f32,
                (-n - origin[2]) as f32,
            ]);
            mesh.normals.push(normal.map(|v| v as f32));
            mesh.uvs.push([u, distance as f32]);
        }
        if i > 0 {
            let at = base + u32::try_from(i * 2).expect("streets fit u32");
            let (left_0, right_0, left_1, right_1) = (at - 2, at - 1, at, at + 1);
            mesh.indices
                .extend([left_0, left_1, right_1, left_0, right_1, right_0]);
        }
    }
}

fn normalize([x, y, z]: [f64; 3]) -> [f64; 3] {
    let length = (x * x + y * y + z * z).sqrt().max(1e-9);
    [x / length, y / length, z / length]
}

/// Street points with their half widths, by index cell.
type StreetCells = std::collections::HashMap<(i64, i64), Vec<(f64, f64, f64)>>;

/// Where the map's streets are, to keep trees and grass off them.
pub(crate) struct Clearance {
    cells: StreetCells,
}

/// Index cell size; larger than half the widest street plus the margins used.
const CLEARANCE_CELL_M: f64 = 10.0;

impl Clearance {
    pub(crate) fn new(streets: &[Street]) -> Self {
        let mut cells = StreetCells::new();
        for street in streets {
            let half = width(street.class) / 2.0;
            for &(e, n) in &street.points {
                cells
                    .entry(clearance_cell(e, n))
                    .or_default()
                    .push((e, n, half));
            }
        }
        Self { cells }
    }

    /// Whether `(east, north)` lies on a street or within `margin` of its edge.
    pub(crate) fn blocked(&self, east: f64, north: f64, margin: f64) -> bool {
        let (ce, cn) = clearance_cell(east, north);
        (ce - 1..=ce + 1).any(|x| {
            (cn - 1..=cn + 1).any(|y| {
                self.cells.get(&(x, y)).is_some_and(|points| {
                    points
                        .iter()
                        // Points lie at most a step apart: allow half a step along.
                        .any(|&(e, n, half)| {
                            (e - east).hypot(n - north) < half + margin + STEP_M / 2.0
                        })
                })
            })
        })
    }
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn clearance_cell(east: f64, north: f64) -> (i64, i64) {
    (
        (east / CLEARANCE_CELL_M).floor() as i64,
        (north / CLEARANCE_CELL_M).floor() as i64,
    )
}
