//! The land beyond the corridor (#75): coarse terrain out to [`HORIZON`] metres from the route,
//! so in the mountains the world never ends at the edge of the detailed ground; the haze takes
//! it to the sky. Heights come from a coarse terrain model (tiles of a low zoom level), colours
//! from the land's shape alone, as the map is only loaded within the corridor: meadow, forest
//! on the slopes below the tree line, and lakes where the ground is dead level (the terrain
//! models measure their surface). The terrain shader makes steep facets rock and high ones
//! snow, as on the detailed ground.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};

use torqa_routes::{ElevationModel, LocalProjection, Route};

use crate::landcover;
use crate::road::RoadIndex;
use crate::{CHUNK_SIZE, MeshData, chunks_near_route, palette};
use torqa_osm::LandCover;

/// How far from the route the land reaches.
pub const HORIZON: f64 = 12_000.0;
/// Size of its facets: half a chunk, so every one lies wholly in or out of the detailed ground.
const CELL: f64 = CHUNK_SIZE / 2.0;
/// The land lies this much lower than measured, so where it meets the detailed ground it never
/// rises above it; at its distance no one sees the difference.
const SINK: f64 = 2.0;
/// A facet whose corners differ by less than this is water (a lake's measured surface).
const LEVEL: f64 = 0.05;
/// Slopes below the tree line and steeper than this (rise per metre) are forest.
const FOREST_SLOPE: f64 = 0.15;
const TREE_LINE: f64 = 1800.0;
/// Water lies this far above the lake's measured surface, as on the detailed ground.
const WATER_LIFT: f64 = 0.02;

/// The land beyond the corridor: its ground (vertex colours as the detailed ground's) and its
/// lakes (for the water material), in route coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Horizon {
    /// Ground.
    pub ground: MeshData,
    /// Lakes.
    pub water: MeshData,
}

/// The land beyond the detailed ground of `route` out to [`HORIZON`], with heights from
/// `model` (a coarse one: the land is drawn in facets of `CELL`). Where `model` has no height
/// the land has a hole, hidden by the haze.
///
/// # Panics
/// If the land had more than 2³² vertices; out to [`HORIZON`] it has a few hundred thousand.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data; the grid is small
pub async fn horizon<M: ElevationModel>(route: &Route, model: &mut M) -> Horizon {
    let projection = LocalProjection::for_route(route);
    let road = RoadIndex::new(route, &projection);
    let detailed: BTreeSet<(i32, i32)> = chunks_near_route(&road);
    // Every cell within reach of the route: the road sampled every few cells is enough.
    let reach = HORIZON + CELL;
    let mut cells: BTreeSet<(i64, i64)> = BTreeSet::new();
    for (east, north) in road.samples(CELL * 4.0) {
        let (low_e, low_n) = (cell_of(east - reach), cell_of(north - reach));
        let (high_e, high_n) = (cell_of(east + reach), cell_of(north + reach));
        for ce in low_e..=high_e {
            for cn in low_n..=high_n {
                let middle = middle_of(ce, cn);
                let chunk = (
                    (middle.0 / CHUNK_SIZE).floor() as i32,
                    (middle.1 / CHUNK_SIZE).floor() as i32,
                );
                if !detailed.contains(&chunk)
                    && (middle.0 - east).hypot(middle.1 - north) <= HORIZON
                {
                    cells.insert((ce, cn));
                }
            }
        }
    }
    let mut heights: HashMap<(i64, i64), Option<f64>> = HashMap::new();
    for &(ce, cn) in &cells {
        for corner in [(ce, cn), (ce + 1, cn), (ce, cn + 1), (ce + 1, cn + 1)] {
            if let Entry::Vacant(slot) = heights.entry(corner) {
                #[allow(clippy::cast_precision_loss)] // grid indices stay small
                let (lat, lon) =
                    projection.unproject(corner.0 as f64 * CELL, corner.1 as f64 * CELL);
                slot.insert(model.elevation(lat, lon).await.ok());
            }
        }
    }

    let mut horizon = Horizon::default();
    let lake = palette::srgb("water.deep", 1.0);
    for &(ce, cn) in &cells {
        let corners = [(ce, cn), (ce + 1, cn), (ce, cn + 1), (ce + 1, cn + 1)];
        let Some([sw, se, nw, ne]) = corners
            .iter()
            .map(|c| heights.get(c).copied().flatten())
            .collect::<Option<Vec<f64>>>()
            .and_then(|h| <[f64; 4]>::try_from(h).ok())
        else {
            continue;
        };
        let (low, high) = (sw.min(se).min(nw).min(ne), sw.max(se).max(nw).max(ne));
        let water = high - low < LEVEL;
        let slope = (high - low) / CELL;
        let (mesh, lift, color) = if water {
            (&mut horizon.water, WATER_LIFT, lake)
        } else {
            let forest = slope > FOREST_SLOPE && high < TREE_LINE;
            let cover = forest.then_some(LandCover::Forest);
            (&mut horizon.ground, -SINK, landcover::color(cover))
        };
        #[allow(clippy::cast_precision_loss)] // grid indices stay small
        let at = |(e, n): (i64, i64), height: f64| {
            [
                (e as f64 * CELL) as f32,
                (height + lift) as f32,
                (-(n as f64) * CELL) as f32,
            ]
        };
        let base = u32::try_from(mesh.vertices.len()).expect("the horizon fits u32");
        for (corner, height) in corners.iter().zip([sw, se, nw, ne]) {
            mesh.vertices.push(at(*corner, height));
            mesh.normals.push([0.0, 1.0, 0.0]);
            mesh.uvs.push([0.0, 0.0]);
            mesh.colors.push(color);
        }
        // Split along the south-west to north-east diagonal, clockwise seen from above, as
        // the detailed ground.
        let (sw, se, nw, ne) = (base, base + 1, base + 2, base + 3);
        mesh.indices.extend([sw, nw, ne, sw, ne, se]);
    }
    horizon
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell_of(metres: f64) -> i64 {
    (metres / CELL).floor() as i64
}

#[allow(clippy::cast_precision_loss)] // grid indices stay small
fn middle_of(ce: i64, cn: i64) -> (f64, f64) {
    ((ce as f64 + 0.5) * CELL, (cn as f64 + 0.5) * CELL)
}
