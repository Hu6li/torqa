//! The streets, tracks and paths of the map around the route, other than the road ridden,
//! draped on the terrain of each chunk: cut along the ground's own triangles, so they lie
//! exactly on the ground the rider sees and it never shows through them. Streets joining or
//! crossing the road run up to it and under it, so junctions look joined; bridges are straight
//! decks between their ends; tunnels stay under the ground.

use torqa_osm::{MapData, RoadClass, StructureKind};
use torqa_routes::{ElevationModel, LocalProjection};

use crate::drape;
use crate::railways::{self, Railway};
use crate::road::{Mouth, RoadIndex};
use crate::water::{self, Pool};
use crate::{HeightGrid, MeshData, ROAD_HALF_WIDTH};

/// Distance between the points of a street: short enough to tell where it runs along the road
/// ridden and to keep plants off it.
const STEP_M: f64 = 3.0;

/// How far a street lies above the ground: bigger roads above smaller ones, so where they
/// overlap at junctions the bigger one shows, and a little more for every street (up to 1 cm)
/// so overlapping ones of a kind never flicker. All stay well below the road ridden, which
/// stands `ROAD_SINK` above the ground beside it, so streets joining it run on under it.
fn lift(class: RoadClass, index: usize) -> f64 {
    let base = match class {
        RoadClass::Major => 0.08,
        RoadClass::Street => 0.07,
        RoadClass::Service => 0.06,
        RoadClass::Track => 0.05,
        RoadClass::Path => 0.04,
    };
    #[allow(clippy::cast_precision_loss)] // a small remainder
    let jitter = (index % 10) as f64 * 0.001;
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
        let points = drape::densify(&line, STEP_M);
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
        for piece in drape::pieces(&street.points, low, high) {
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

/// Where the streets meet the road ridden (see [`Mouth`]): the road's edge is road there, not
/// shoulder. Stretches running along the road are no mouths.
pub(crate) fn mouths(streets: &[Street], road: &RoadIndex) -> Vec<Mouth> {
    let mut mouths = Vec::new();
    for street in streets.iter().filter(|s| s.deck.is_none()) {
        let half = width(street.class) / 2.0;
        for (i, &(east, north)) in street.points.iter().enumerate() {
            // Points on the road's surface belong to no side; beside it, a band wider than the
            // points are apart catches every street that reaches the edge.
            let Some((distance, along, side)) = road.locate(east, north, ROAD_HALF_WIDTH + 3.0)
            else {
                continue;
            };
            if distance < ROAD_HALF_WIDTH - 0.5 {
                continue;
            }
            let direction = local_direction(&street.points, i);
            if !road.runs_along(east, north, ROAD_HALF_WIDTH + half + 0.5, direction) {
                mouths.push((along, side, half));
            }
        }
    }
    mouths
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

/// A strip of `half` width along `run` (points with their distance along the street): draped
/// on the ground, or for a bridge a deck straight between its ends' heights, with its sides.
fn ribbon(
    mesh: &mut MeshData,
    run: &[((f64, f64), f64)],
    half: f64,
    street: &Street,
    total: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    if let Some(ends) = street.deck {
        drape::deck(
            mesh,
            run,
            half,
            ends,
            lift(street.class, street.index),
            total,
            heights,
            origin,
        );
    } else {
        drape::drape(
            mesh,
            run,
            half,
            lift(street.class, street.index),
            heights,
            origin,
        );
    }
}

/// Street points with their half widths, by index cell.
type StreetCells = std::collections::HashMap<(i64, i64), Vec<(f64, f64, f64)>>;

/// Where the map's streets, railways and shores are, to keep trees and grass off them.
pub(crate) struct Clearance {
    cells: StreetCells,
}

/// Index cell size; larger than half the widest street plus the margins used.
const CLEARANCE_CELL_M: f64 = 10.0;

impl Clearance {
    pub(crate) fn new(streets: &[Street], railways: &[Railway], pools: &[Pool]) -> Self {
        let mut cells = StreetCells::new();
        let lines = streets
            .iter()
            .map(|s| (&s.points, width(s.class) / 2.0))
            .chain(railways.iter().map(|r| (&r.points, railways::BED_M / 2.0)))
            .chain(pools.iter().map(|p| (&p.shore, water::SHORE_M / 2.0)));
        for (points, half) in lines {
            for &(e, n) in points {
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
