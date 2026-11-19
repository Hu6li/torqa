//! The land beyond the corridor (#75): coarse terrain out to [`HORIZON`] metres from the route,
//! so in the mountains the world never ends at the edge of the detailed ground; the haze takes
//! it to the sky. Heights come from a coarse terrain model (tiles of a low zoom level), colours
//! from the land's shape alone, as the map is only loaded within the corridor: meadow, forest
//! on the slopes below the tree line, and lakes where the ground is dead level (the terrain
//! models measure their surface). The terrain shader makes steep facets rock and high ones
//! snow, as on the detailed ground.
//!
//! The land covers the corridor too (#560): the detailed ground is drawn only near the camera
//! and loaded only around the rider, so farther along the route the coarse land stands in for
//! it, chunk by chunk ([`HorizonParts`]); otherwise the sky showed through where the route runs
//! on into the distance.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use orqwa_routes::{ElevationModel, LocalProjection, Route};

use crate::landcover;
use crate::road::RoadIndex;
use crate::{CHUNK_SIZE, MeshData, chunks_near_route, palette};
use orqwa_osm::LandCover;

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

/// The coarse land around the route, beyond the corridor and under it: its ground (vertex
/// colours as the detailed ground's) and its lakes (for the water material), in route
/// coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Horizon {
    /// Ground.
    pub ground: MeshData,
    /// Lakes.
    pub water: MeshData,
}

/// The land around `route` out to [`HORIZON`], under its detailed ground too, with heights
/// from `model` (a coarse one: the land is drawn in facets of `CELL`). Where `model` has no
/// height the land has a hole, hidden by the haze.
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
                // The detailed ground's cells reach a little beyond `HORIZON` on a short
                // route; their stand-ins must still be there.
                if detailed.contains(&chunk_at(middle))
                    || (middle.0 - east).hypot(middle.1 - north) <= HORIZON
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

/// The land of a [`Horizon`] split by where the detailed ground lies: the part that is never
/// detailed, always drawn, and for each chunk of the detailed ground the land under it, which
/// stands in for the chunk wherever the chunk is not drawn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HorizonParts {
    /// The land beyond the detailed ground.
    pub outside: Horizon,
    /// The land under each chunk of the detailed ground, by its [`chunk_key`].
    pub corridor: BTreeMap<(i32, i32), Horizon>,
}

impl HorizonParts {
    /// Splits `horizon`, made for `route`, by the chunks of the route's detailed ground. A
    /// world kept from before the land covered the corridor has nothing under it.
    #[must_use]
    pub fn new(route: &Route, horizon: &Horizon) -> Self {
        let projection = LocalProjection::for_route(route);
        let detailed = chunks_near_route(&RoadIndex::new(route, &projection));
        let mut parts = Self::default();
        for (mesh, water) in [(&horizon.ground, false), (&horizon.water, true)] {
            for cell in cells(mesh) {
                let key = chunk_key(cell_middle(&cell));
                let part = if detailed.contains(&key) {
                    parts.corridor.entry(key).or_default()
                } else {
                    &mut parts.outside
                };
                let target = if water {
                    &mut part.water
                } else {
                    &mut part.ground
                };
                target.append(cell);
            }
        }
        parts
    }

    /// The land under every chunk not in `drawn`, as one: it fills in for the chunks that are
    /// not loaded, still being built or could not be read.
    #[must_use]
    pub fn fill(&self, drawn: &BTreeSet<(i32, i32)>) -> Horizon {
        let mut fill = Horizon::default();
        for (key, part) in &self.corridor {
            if !drawn.contains(key) {
                fill.ground.append(part.ground.clone());
                fill.water.append(part.water.clone());
            }
        }
        fill
    }
}

/// The chunk of the detailed ground at `center` (in route coordinates, as
/// [`TerrainChunk::center`](crate::TerrainChunk) and the land's vertices): its grid cell, east
/// and north.
#[must_use]
pub fn chunk_key(center: [f32; 3]) -> (i32, i32) {
    chunk_at((f64::from(center[0]), -f64::from(center[2])))
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^31 chunks
fn chunk_at((east, north): (f64, f64)) -> (i32, i32) {
    (
        (east / CHUNK_SIZE).floor() as i32,
        (north / CHUNK_SIZE).floor() as i32,
    )
}

/// The land's cells, each its own four vertices and two triangles as [`horizon`] lays them.
fn cells(mesh: &MeshData) -> impl Iterator<Item = MeshData> + '_ {
    (0..mesh.vertices.len() / 4).map(move |cell| {
        let corners = cell * 4..cell * 4 + 4;
        MeshData {
            vertices: mesh.vertices[corners.clone()].to_vec(),
            normals: mesh.normals[corners.clone()].to_vec(),
            uvs: mesh.uvs[corners.clone()].to_vec(),
            colors: mesh.colors[corners].to_vec(),
            indices: mesh.indices[cell * 6..cell * 6 + 6]
                .iter()
                .map(|i| i - u32::try_from(cell * 4).expect("the horizon fits u32"))
                .collect(),
        }
    })
}

fn cell_middle(cell: &MeshData) -> [f32; 3] {
    let mut middle = [0.0; 3];
    for vertex in &cell.vertices {
        for (sum, value) in middle.iter_mut().zip(vertex) {
            *sum += value / 4.0;
        }
    }
    middle
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell_of(metres: f64) -> i64 {
    (metres / CELL).floor() as i64
}

#[allow(clippy::cast_precision_loss)] // grid indices stay small
fn middle_of(ce: i64, cn: i64) -> (f64, f64) {
    ((ce as f64 + 0.5) * CELL, (cn as f64 + 0.5) * CELL)
}
