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
//! on into the distance. Where the detailed ground ends beside it, the two do not meet: the
//! coarse land lies lower on ridges and higher in hollows. The detailed ground's edges hang
//! down below it ([`HorizonParts::skirt`]), so no crack opens between them.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use orqwa_routes::{ElevationModel, LocalProjection, Route, Surface};

use crate::landcover;
use crate::road::RoadIndex;
use crate::{CHUNK_SIZE, MeshData, TerrainChunk, chunks_near_route, palette};
use orqwa_osm::LandCover;

/// How far from the route the land reaches.
pub const HORIZON: f64 = 12_000.0;
/// Size of its facets: half a chunk, so every one lies wholly in or out of the detailed ground.
const CELL: f64 = CHUNK_SIZE / 2.0;
/// Cells along a chunk's side.
const CHUNK_CELLS: i64 = 2;
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
/// The detailed ground's edges hang this far below the land beside them, and at least this far
/// below themselves: the coarser versions of neighbouring chunks (`GroundLods`) need not meet
/// either.
const SKIRT_BELOW: f64 = 2.0;
const SKIRT_DEPTH: f64 = 3.0;
/// The edges keep out of the ridden road's tunnels: the tube (`structures`) and a margin, this
/// far either side of the line and this high above the road.
const TUNNEL_CLEAR_RADIUS: f64 = 8.0;
const TUNNEL_CLEAR_HEIGHT: f64 = 9.0;
/// A vertex this close to a chunk's side lies on it.
const ON_SIDE: f32 = 0.01;

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
    /// The ridden road's tunnels, each its centre line as `[east, north, road height]`.
    tunnels: Vec<Vec<[f64; 3]>>,
}

impl HorizonParts {
    /// Splits `horizon`, made for `route`, by the chunks of the route's detailed ground. A
    /// world kept from before the land covered the corridor has nothing under it.
    #[must_use]
    pub fn new(route: &Route, horizon: &Horizon) -> Self {
        let projection = LocalProjection::for_route(route);
        let road = RoadIndex::new(route, &projection);
        let detailed = chunks_near_route(&road);
        let mut parts = Self {
            tunnels: road
                .structure_runs()
                .into_iter()
                .filter(|(surface, _)| *surface == Surface::Tunnel)
                .map(|(_, line)| {
                    line.iter()
                        .map(|p| [p.position.0, p.position.1, p.elevation])
                        .collect()
                })
                .collect(),
            ..Self::default()
        };
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

    /// A skirt for `chunk`'s ground, in the chunk's coordinates: from every edge of the ground
    /// on the chunk's sides down to below the land there, facing out, with the edge's normals
    /// and colours. Wherever the chunk is drawn and its neighbour is not, or lies beyond the
    /// corridor, it closes the crack between the ground and the coarse land (#560). It stops
    /// above the ridden road's tunnels, whose tube would show it. Empty for a chunk with no
    /// land under it (a world kept from before).
    ///
    /// # Panics
    /// If the skirt had more than 2³² vertices; a chunk's has a few hundred.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // f32 GPU data
    pub fn skirt(&self, chunk: &TerrainChunk) -> MeshData {
        let key = chunk_key(chunk.center);
        let Some(under) = self.corridor.get(&key) else {
            return MeshData::default();
        };
        let beside = LandBeside::new(under, key);
        let tunnels = TunnelRoofs::new(&self.tunnels, chunk.center);
        let [cx, _, cz] = chunk.center.map(f64::from);
        let mesh = &chunk.mesh;
        let mut skirt = MeshData::default();
        let mut hung: HashSet<(u32, u32)> = HashSet::new();
        for triangle in mesh.indices.as_chunks::<3>().0 {
            for (a, b) in [(0, 1), (1, 2), (2, 0)].map(|(i, j)| (triangle[i], triangle[j])) {
                let (pa, pb) = (mesh.vertices[a as usize], mesh.vertices[b as usize]);
                let Some((outward, along_north)) = side(pa, pb) else {
                    continue;
                };
                if !hung.insert((a.min(b), a.max(b))) {
                    continue;
                }
                let bottom = |p: [f32; 3]| {
                    let (east, north) = (f64::from(p[0]) + cx, -(f64::from(p[2]) + cz));
                    let top = f64::from(p[1]);
                    let mut low = top - SKIRT_DEPTH;
                    if let Some(height) = beside.height(east, north, along_north) {
                        low = low.min(height - SKIRT_BELOW);
                    }
                    low.max(tunnels.roof(east, north)).min(top) as f32
                };
                let (low_a, low_b) = (bottom(pa), bottom(pb));
                if low_a < pa[1] || low_b < pb[1] {
                    // Front faces wind clockwise as seen: with `a` before `b`, the quad below
                    // faces (−dz, dx) across the edge.
                    let (dx, dz) = (pb[0] - pa[0], pb[2] - pa[2]);
                    let ends = if -dz * outward[0] + dx * outward[2] >= 0.0 {
                        [(a, low_a), (b, low_b)]
                    } else {
                        [(b, low_b), (a, low_a)]
                    };
                    hang(&mut skirt, mesh, ends);
                }
            }
        }
        skirt
    }
}

/// Hangs a quad from the edge between `ends` of `ground` (each a vertex and how low it hangs).
fn hang(skirt: &mut MeshData, ground: &MeshData, ends: [(u32, f32); 2]) {
    let base = u32::try_from(skirt.vertices.len()).expect("a skirt fits u32");
    for (k, low) in ends.map(|(k, low)| (k as usize, low)) {
        let p = ground.vertices[k];
        for y in [p[1], low] {
            skirt.vertices.push([p[0], y, p[2]]);
            skirt.normals.push(ground.normals[k]);
            skirt.uvs.push(ground.uvs[k]);
            if let Some(&color) = ground.colors.get(k) {
                skirt.colors.push(color);
            }
        }
    }
    // Top and bottom of the first end, then of the second.
    let (top_a, low_a, top_b, low_b) = (base, base + 1, base + 2, base + 3);
    skirt
        .indices
        .extend([top_a, top_b, low_b, top_a, low_b, low_a]);
}

/// The coarse land along a chunk's sides, which run along its cell lines: there it is straight
/// from corner to corner.
struct LandBeside {
    /// Its corners under the chunk by grid line, east and north; a lake's lie above the
    /// ground's sunk ones.
    corners: HashMap<(i64, i64), f64>,
    /// The chunk's first cell, east and north.
    first: (i64, i64),
}

impl LandBeside {
    fn new(under: &Horizon, key: (i32, i32)) -> Self {
        let mut corners: HashMap<(i64, i64), f64> = HashMap::new();
        for v in under.ground.vertices.iter().chain(&under.water.vertices) {
            let corner = (grid_line(f64::from(v[0])), grid_line(-f64::from(v[2])));
            let height = f64::from(v[1]);
            corners
                .entry(corner)
                .and_modify(|h| *h = h.min(height))
                .or_insert(height);
        }
        let first = (
            i64::from(key.0) * CHUNK_CELLS,
            i64::from(key.1) * CHUNK_CELLS,
        );
        Self { corners, first }
    }

    /// The land's height at `east`, `north` on a side of the chunk running north or east.
    fn height(&self, east: f64, north: f64, along_north: bool) -> Option<f64> {
        let (line, along, first) = if along_north {
            (grid_line(east), north, self.first.1)
        } else {
            (grid_line(north), east, self.first.0)
        };
        #[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
        let cell = ((along / CELL).floor() as i64).clamp(first, first + CHUNK_CELLS - 1);
        #[allow(clippy::cast_precision_loss)] // grid indices stay small
        let t = (along / CELL - cell as f64).clamp(0.0, 1.0);
        let corner = |k: i64| {
            self.corners
                .get(&if along_north { (line, k) } else { (k, line) })
                .copied()
        };
        Some(corner(cell)? * (1.0 - t) + corner(cell + 1)? * t)
    }
}

/// The ridden road's tunnels near a chunk, as segments of `[east, north, road height]`.
struct TunnelRoofs(Vec<[[f64; 3]; 2]>);

impl TunnelRoofs {
    fn new(tunnels: &[Vec<[f64; 3]>], center: [f32; 3]) -> Self {
        let (east, north) = (f64::from(center[0]), -f64::from(center[2]));
        let reach = CHUNK_SIZE / 2.0 + TUNNEL_CLEAR_RADIUS;
        Self(
            tunnels
                .iter()
                .flat_map(|line| line.windows(2).map(|w| [w[0], w[1]]))
                .filter(|[a, b]| {
                    a[0].min(b[0]) <= east + reach
                        && a[0].max(b[0]) >= east - reach
                        && a[1].min(b[1]) <= north + reach
                        && a[1].max(b[1]) >= north - reach
                })
                .collect(),
        )
    }

    /// The lowest a skirt may reach at `east`, `north`: above the tube of a tunnel there, or
    /// anywhere.
    fn roof(&self, east: f64, north: f64) -> f64 {
        self.0
            .iter()
            .filter_map(|[a, b]| {
                let (de, dn) = (b[0] - a[0], b[1] - a[1]);
                let length = de * de + dn * dn;
                let t = if length > 0.0 {
                    (((east - a[0]) * de + (north - a[1]) * dn) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let off = (east - a[0] - t * de).hypot(north - a[1] - t * dn);
                (off <= TUNNEL_CLEAR_RADIUS).then(|| a[2] + t * (b[2] - a[2]) + TUNNEL_CLEAR_HEIGHT)
            })
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The side of a chunk on which the edge from `a` to `b` (in its coordinates) lies: the
/// direction out of the chunk, and whether the side runs north; `None` for an edge inside it,
/// or standing upright on its side.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn side(a: [f32; 3], b: [f32; 3]) -> Option<([f32; 3], bool)> {
    let half = (CHUNK_SIZE / 2.0) as f32;
    let on = |value: f32, edge: f32| (value - edge).abs() < ON_SIDE;
    if on(a[0], b[0]) && on(a[2], b[2]) {
        return None;
    }
    [
        (0, half, [1.0, 0.0, 0.0]),
        (0, -half, [-1.0, 0.0, 0.0]),
        (2, half, [0.0, 0.0, 1.0]),
        (2, -half, [0.0, 0.0, -1.0]),
    ]
    .into_iter()
    .find(|&(axis, edge, _)| on(a[axis], edge) && on(b[axis], edge))
    .map(|(axis, _, outward)| (outward, axis == 0))
}

/// The land's grid line nearest `metres`.
#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn grid_line(metres: f64) -> i64 {
    (metres / CELL).round() as i64
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
