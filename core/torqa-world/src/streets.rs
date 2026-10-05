//! The streets, tracks and paths of the map around the route, other than the road ridden,
//! draped on the terrain of each chunk (from its own heights, so they sit exactly on the ground
//! the rider sees). Streets joining or crossing the road run up to it and under it, so
//! junctions look joined; bridges are straight decks between their ends; tunnels stay under
//! the ground.

use torqa_osm::{MapData, RoadClass, StructureKind};
use torqa_routes::{ElevationModel, LocalProjection};

use crate::road::RoadIndex;
use crate::{HeightGrid, MeshData, ROAD_HALF_WIDTH};

/// Distance between the points of a draped street: short enough to follow the ground.
const STEP_M: f64 = 3.0;
/// Bridge decks are this thick at their edges.
const DECK_DEPTH_M: f64 = 0.7;

/// How far a street lies above the ground: bigger roads above smaller ones, so where they
/// overlap at junctions the bigger one shows, and a little more for every street (up to 2 cm)
/// so overlapping ones of a kind never flicker.
fn lift(class: RoadClass, index: usize) -> f64 {
    let base = match class {
        RoadClass::Major => 0.16,
        RoadClass::Street => 0.14,
        RoadClass::Service => 0.12,
        RoadClass::Track => 0.1,
        RoadClass::Path => 0.08,
    };
    #[allow(clippy::cast_precision_loss)] // a small remainder
    let jitter = (index % 20) as f64 * 0.001;
    base + jitter
}

/// A street of the map in metres east/north, with its bounds for quick chunk tests.
pub(crate) struct Street {
    class: RoadClass,
    points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
    /// For bridges: the deck's height at both ends (the ground's there).
    deck: Option<(f64, f64)>,
    index: usize,
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

/// The map's streets around the route, densified, with bridge decks' end heights from `model`.
/// Tunnels are left out: draped on the ground, they would run over the mountain.
pub(crate) async fn lines<M: ElevationModel>(
    map: &MapData,
    projection: &LocalProjection,
    model: &mut M,
) -> Vec<Street> {
    let mut streets = Vec::new();
    for (index, road) in map.roads.iter().enumerate() {
        if road.structure == Some(StructureKind::Tunnel) {
            continue;
        }
        let line: Vec<(f64, f64)> = road
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let points = densify(&line);
        let Some(&first) = points.first() else {
            continue;
        };
        let deck = if road.structure == Some(StructureKind::Bridge) {
            let mut end = async |(lat, lon): (f64, f64)| model.elevation(lat, lon).await.ok();
            let (start, finish) = (road.line[0], road.line[road.line.len() - 1]);
            match (end(start).await, end(finish).await) {
                (Some(a), Some(b)) => Some((a, b)),
                // Without the ground's heights a deck cannot be placed.
                _ => continue,
            }
        } else {
            None
        };
        let (mut min, mut max) = (first, first);
        for &(e, n) in &points {
            min = (min.0.min(e), min.1.min(n));
            max = (max.0.max(e), max.1.max(n));
        }
        streets.push(Street {
            class: road.class,
            points,
            min,
            max,
            deck,
            index,
        });
    }
    streets
}

/// The paved streets and the unpaved tracks and paths within the chunk square `[origin,
/// origin + size]`, relative to `chunk_origin`. Stretches running along the road ridden are
/// left out (it is drawn there already, and they are mostly the same road); streets joining or
/// crossing it run on under it.
pub(crate) fn meshes(
    streets: &[Street],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    road: &RoadIndex,
    chunk_origin: [f64; 3],
) -> (MeshData, MeshData) {
    let (mut paved_mesh, mut unpaved_mesh) = (MeshData::default(), MeshData::default());
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
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
        let total = along(&street.points);
        // The street cut exactly at the chunk's edges, so the neighbour's piece starts where
        // this one ends; points with their distance along the street.
        let mut pieces: Vec<Vec<((f64, f64), f64)>> = Vec::new();
        let mut piece: Vec<((f64, f64), f64)> = Vec::new();
        let mut travelled = 0.0;
        for pair in street.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = (b.0 - a.0).hypot(b.1 - a.1);
            if let Some((t0, t1)) = clip(a, b, low, high) {
                let start = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
                let end = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
                if piece
                    .last()
                    .is_none_or(|&(p, _)| (p.0 - start.0).hypot(p.1 - start.1) > 1e-6)
                {
                    pieces.push(std::mem::take(&mut piece));
                    piece.push((start, travelled + length * t0));
                }
                piece.push((end, travelled + length * t1));
            } else {
                pieces.push(std::mem::take(&mut piece));
            }
            travelled += length;
        }
        pieces.push(piece);
        for piece in pieces {
            let points: Vec<(f64, f64)> = piece.iter().map(|&(p, _)| p).collect();
            let mut run: Vec<((f64, f64), f64)> = Vec::new();
            for (i, &(point, distance)) in piece.iter().enumerate() {
                let direction = local_direction(&points, i);
                let reach = ROAD_HALF_WIDTH + half + 0.5;
                if road.runs_along(point.0, point.1, reach, direction) {
                    ribbon(target, &run, half, street, total, heights, chunk_origin);
                    run.clear();
                } else {
                    run.push((point, distance));
                }
            }
            ribbon(target, &run, half, street, total, heights, chunk_origin);
        }
    }
    (paved_mesh, unpaved_mesh)
}

/// The part of segment `a`–`b` inside the rectangle `low`–`high`, as positions along it
/// (0–1), if any (Liang–Barsky).
fn clip(a: (f64, f64), b: (f64, f64), low: (f64, f64), high: (f64, f64)) -> Option<(f64, f64)> {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-de, a.0 - low.0),
        (de, high.0 - a.0),
        (-dn, a.1 - low.1),
        (dn, high.1 - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
}

/// The length of a line.
fn along(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

/// The unit direction of a line at its point `i`.
fn local_direction(points: &[(f64, f64)], i: usize) -> (f64, f64) {
    let (a, b) = (
        points[i.saturating_sub(1)],
        points[(i + 1).min(points.len() - 1)],
    );
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let length = de.hypot(dn).max(1e-9);
    (de / length, dn / length)
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

/// A strip of `half` width along `run` (points with their distance along the street): on the
/// ground below each edge, or for a bridge a deck straight between its ends' heights, with
/// its sides.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn ribbon(
    mesh: &mut MeshData,
    run: &[((f64, f64), f64)],
    half: f64,
    street: &Street,
    total: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    if run.len() < 2 {
        return;
    }
    let lift = lift(street.class, street.index);
    let base = u32::try_from(mesh.vertices.len()).expect("streets fit u32");
    // Per point: left edge, right edge, and for bridges the bottoms of the deck's sides.
    let per_point = if street.deck.is_some() { 4 } else { 2 };
    for (i, &((east, north), distance)) in run.iter().enumerate() {
        let (before, after) = (
            run[i.saturating_sub(1)].0,
            run[(i + 1).min(run.len() - 1)].0,
        );
        let (de, dn) = (after.0 - before.0, after.1 - before.1);
        let length = de.hypot(dn).max(1e-6);
        // Right of travel is the direction turned clockwise by 90°.
        let (re, rn) = (dn / length, -de / length);
        let deck = street
            .deck
            .map(|(start, end)| start + (end - start) * (distance / total.max(1e-6)) + lift);
        for (side, u) in [(-1.0, 0.0), (1.0, 1.0)] {
            let (e, n) = (east + re * half * side, north + rn * half * side);
            let (height, normal) = if let Some(top) = deck {
                (top, [0.0, 1.0, 0.0])
            } else {
                // The ground's slope here, for shading like the terrain around.
                let slope_e = (heights.at(e + 1.0, n) - heights.at(e - 1.0, n)) / 2.0;
                let slope_n = (heights.at(e, n + 1.0) - heights.at(e, n - 1.0)) / 2.0;
                (heights.at(e, n) + lift, normalize([-slope_e, 1.0, slope_n]))
            };
            mesh.vertices.push([
                (e - origin[0]) as f32,
                (height - origin[1]) as f32,
                (-n - origin[2]) as f32,
            ]);
            mesh.normals.push(normal.map(|v| v as f32));
            mesh.uvs.push([u, distance as f32]);
        }
        if let Some(top) = deck {
            for side in [-1.0, 1.0] {
                let (e, n) = (east + re * half * side, north + rn * half * side);
                mesh.vertices.push([
                    (e - origin[0]) as f32,
                    (top - DECK_DEPTH_M - origin[1]) as f32,
                    (-n - origin[2]) as f32,
                ]);
                mesh.normals
                    .push([(re * side) as f32, 0.0, (-rn * side) as f32]);
                // Dark like the verge.
                mesh.uvs
                    .push([if side < 0.0 { 0.0 } else { 1.0 }, distance as f32]);
            }
        }
        if i > 0 {
            let step = u32::try_from(per_point).expect("small");
            let at = base + u32::try_from(i * per_point).expect("streets fit u32");
            let previous = at - step;
            let (left_0, right_0, left_1, right_1) = (previous, previous + 1, at, at + 1);
            mesh.indices
                .extend([left_0, left_1, right_1, left_0, right_1, right_0]);
            if per_point == 4 {
                // The deck's sides, facing out.
                let (bottom_left_0, bottom_right_0) = (previous + 2, previous + 3);
                let (bottom_left_1, bottom_right_1) = (at + 2, at + 3);
                mesh.indices.extend([
                    left_0,
                    bottom_left_0,
                    bottom_left_1,
                    left_0,
                    bottom_left_1,
                    left_1,
                    right_0,
                    right_1,
                    bottom_right_1,
                    right_0,
                    bottom_right_1,
                    bottom_right_0,
                ]);
            }
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
