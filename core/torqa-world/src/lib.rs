//! 3D world geometry for Torqa (R16): terrain chunks in a corridor around the route, coloured
//! by land cover, with buildings and trees from OpenStreetMap, rivers, and the road.
//!
//! Coordinates follow Godot: metres with x east, y up and z south (−z is north), relative to
//! the route start for the road and water, and to each chunk's centre for chunk geometry.
//! Triangles wind clockwise seen from their front, Godot's front-face order.

mod buildings;
mod landcover;
mod road;
mod structures;
mod vegetation;
mod water;

use std::collections::{BTreeSet, HashMap};

use torqa_osm::MapData;
use torqa_routes::{ElevationModel, LocalProjection, Route, Surface};
use tracing::{info, warn};

use landcover::LandIndex;
use road::RoadIndex;
pub use vegetation::Trees;

/// Edge length of a terrain chunk.
const CHUNK_SIZE: f64 = 480.0;
/// Distance between terrain vertices.
const GRID: f64 = 16.0;
/// Terrain (and map data) is used up to this far from the route.
pub const CORRIDOR: f64 = 1500.0;
/// Half the road width.
const ROAD_HALF_WIDTH: f64 = 3.0;
/// Terrain within this distance of the road centre is levelled to the road. Larger than half a
/// grid diagonal, so the coarse grid cannot poke through the road between vertices.
const FLAT_INNER: f64 = 12.0;
/// Beyond this distance the terrain is untouched; in between it blends.
const FLAT_OUTER: f64 = 32.0;
/// Levelled terrain sits this far below the road surface to avoid flickering.
const ROAD_SINK: f64 = 0.25;
/// Without terrain data, heights follow the road found within this distance.
const FALLBACK_RADIUS: f64 = 400.0;

/// Triangle geometry ready for a GPU mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    /// Vertex positions.
    pub vertices: Vec<[f32; 3]>,
    /// Unit vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates; for the road `u` is 0–1 across and `v` the distance in metres.
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours (RGB tint, alpha 1 marks water); empty when the mesh has none.
    pub colors: Vec<[f32; 4]>,
    /// Three vertex indices per triangle.
    pub indices: Vec<u32>,
}

/// A square piece of the world.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainChunk {
    /// Centre of the chunk at sea level; the chunk's geometry is relative to it.
    pub center: [f32; 3],
    /// Ground, coloured by land cover.
    pub mesh: MeshData,
    /// Buildings standing in the chunk.
    pub buildings: MeshData,
    /// Trees standing in the chunk.
    pub trees: Trees,
}

/// The generated world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    /// Terrain chunks around the route.
    pub chunks: Vec<TerrainChunk>,
    /// The road along the route.
    pub road: MeshData,
    /// Rivers and streams.
    pub water: MeshData,
    /// Bridges and tunnels.
    pub structures: MeshData,
    /// Terrain samples that had no elevation data and followed the road instead.
    pub fallback_samples: usize,
}

/// Builds the world for `route`, sampling heights from `model` (e.g. the terrain tiles) and
/// placing `map` features. Where the model has no data, the terrain follows the road.
pub async fn generate<M: ElevationModel>(route: &Route, model: &mut M, map: &MapData) -> World {
    let projection = LocalProjection::for_route(route);
    let road = RoadIndex::new(route, &projection);
    let land = LandIndex::new(&map.areas, &projection);
    let buildings = buildings_by_chunk(map, &projection, &road);
    let mut world = World {
        road: road.mesh(ROAD_HALF_WIDTH),
        water: water::ribbons(&map.waterways, &projection, &road, model).await,
        structures: structures::build(&road, &projection, model).await,
        ..World::default()
    };

    for (cx, cn) in chunks_near_route(&road) {
        let heights = HeightGrid::sample(cx, cn, &projection, &road, model, &mut world).await;
        let origin = [
            heights.origin.0 + CHUNK_SIZE / 2.0,
            0.0,
            -(heights.origin.1 + CHUNK_SIZE / 2.0),
        ];
        let mut building_mesh = MeshData::default();
        for (building, footprint) in buildings.get(&(cx, cn)).into_iter().flatten() {
            buildings::add(&mut building_mesh, building, footprint, &heights, origin);
        }
        #[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
        let center = [origin[0] as f32, 0.0, origin[2] as f32];
        world.chunks.push(TerrainChunk {
            center,
            mesh: heights.mesh(&land, origin),
            buildings: building_mesh,
            trees: vegetation::place(heights.origin, CHUNK_SIZE, &heights, &land, &road, origin),
        });
    }
    if world.fallback_samples > 0 {
        warn!(
            samples = world.fallback_samples,
            "terrain data missing in places; terrain follows the road there"
        );
    }
    info!(
        chunks = world.chunks.len(),
        trees = world.chunks.iter().map(|c| c.trees.len()).sum::<usize>(),
        "world generated"
    );
    world
}

/// Buildings with their footprints in metres east/north, by chunk.
type BuildingsByChunk<'a> = HashMap<(i32, i32), Vec<(&'a torqa_osm::Building, Vec<(f64, f64)>)>>;

/// Buildings near the route with their footprints, grouped by the chunk containing their
/// first corner.
fn buildings_by_chunk<'a>(
    map: &'a MapData,
    projection: &LocalProjection,
    road: &RoadIndex,
) -> BuildingsByChunk<'a> {
    let mut by_chunk: HashMap<_, Vec<_>> = HashMap::new();
    for building in &map.buildings {
        let footprint = buildings::footprint(building, projection);
        let Some(&(east, north)) = footprint.first() else {
            continue;
        };
        // Buildings mapped across the road (e.g. bad data) would block it.
        let on_road = footprint
            .iter()
            .any(|&(e, n)| road.nearest(e, n, ROAD_HALF_WIDTH + 1.0).is_some());
        if on_road || road.nearest(east, north, CORRIDOR).is_none() {
            continue;
        }
        by_chunk
            .entry(chunk_of(east, north))
            .or_default()
            .push((building, footprint));
    }
    by_chunk
}

fn chunk_of(east: f64, north: f64) -> (i32, i32) {
    #[allow(clippy::cast_possible_truncation)] // world coordinates are far below 2^31 chunks
    (
        (east / CHUNK_SIZE).floor() as i32,
        (north / CHUNK_SIZE).floor() as i32,
    )
}

/// Chunk grid cells (east, north) within the corridor of any part of the route.
fn chunks_near_route(road: &RoadIndex) -> BTreeSet<(i32, i32)> {
    let mut cells = BTreeSet::new();
    let reach = CORRIDOR + CHUNK_SIZE / 2.0 * std::f64::consts::SQRT_2;
    // Sampling the road every half chunk is enough to touch every chunk in reach.
    for (east, north) in road.samples(CHUNK_SIZE / 2.0) {
        let (low, high) = (
            chunk_of(east - reach, north - reach),
            chunk_of(east + reach, north + reach),
        );
        for cx in low.0..=high.0 {
            for cn in low.1..=high.1 {
                let center_e = (f64::from(cx) + 0.5) * CHUNK_SIZE;
                let center_n = (f64::from(cn) + 0.5) * CHUNK_SIZE;
                if (center_e - east).hypot(center_n - north) <= reach {
                    cells.insert((cx, cn));
                }
            }
        }
    }
    cells
}

/// Terrain heights of one chunk on the vertex grid, with a one-vertex border so normals at
/// the edges match the neighbours.
pub(crate) struct HeightGrid {
    /// South-west corner in metres east/north.
    origin: (f64, f64),
    /// Vertices per side, without the border.
    side: usize,
    heights: Vec<f64>,
}

impl HeightGrid {
    async fn sample<M: ElevationModel>(
        cx: i32,
        cn: i32,
        projection: &LocalProjection,
        road: &RoadIndex,
        model: &mut M,
        world: &mut World,
    ) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let side = (CHUNK_SIZE / GRID).round() as usize + 1;
        let bordered = side + 2;
        let origin = (f64::from(cx) * CHUNK_SIZE, f64::from(cn) * CHUNK_SIZE);
        let chunk_road_elevation = road
            .nearest(
                origin.0 + CHUNK_SIZE / 2.0,
                origin.1 + CHUNK_SIZE / 2.0,
                f64::INFINITY,
            )
            .map_or(0.0, |(_, elevation, _)| elevation);

        let mut heights = vec![0.0; bordered * bordered];
        for j in 0..bordered {
            for i in 0..bordered {
                #[allow(clippy::cast_precision_loss)] // small grid indices
                let (east, north) = (
                    origin.0 + (i as f64 - 1.0) * GRID,
                    origin.1 + (j as f64 - 1.0) * GRID,
                );
                let (lat, lon) = projection.unproject(east, north);
                let natural = if let Ok(height) = model.elevation(lat, lon).await {
                    height
                } else {
                    world.fallback_samples += 1;
                    road.nearest(east, north, FALLBACK_RADIUS)
                        .map_or(chunk_road_elevation, |(_, elevation, _)| elevation)
                };
                heights[j * bordered + i] =
                    level_to_road(natural, road.nearest(east, north, FLAT_OUTER));
            }
        }
        Self {
            origin,
            side,
            heights,
        }
    }

    /// Height at a grid vertex; `i`/`j` may be −1 or `side` (the border).
    fn vertex(&self, i: isize, j: isize) -> f64 {
        let bordered = self.side + 2;
        let clamp = |k: isize| usize::try_from(k + 1).unwrap_or(0).min(bordered - 1);
        self.heights[clamp(j) * bordered + clamp(i)]
    }

    /// Height at any point of (or slightly around) the chunk, interpolated like the mesh.
    pub(crate) fn at(&self, east: f64, north: f64) -> f64 {
        let u = (east - self.origin.0) / GRID;
        let v = (north - self.origin.1) / GRID;
        let (i, j) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - j);
        #[allow(clippy::cast_possible_truncation)] // clamped to the small grid by `vertex`
        let (i, j) = (i as isize, j as isize);
        let bottom = self.vertex(i, j) * (1.0 - fu) + self.vertex(i + 1, j) * fu;
        let top = self.vertex(i, j + 1) * (1.0 - fu) + self.vertex(i + 1, j + 1) * fu;
        bottom * (1.0 - fv) + top * fv
    }

    /// The ground mesh relative to `origin`, coloured by land cover.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // f32 GPU data; small grid
    fn mesh(&self, land: &LandIndex, origin: [f64; 3]) -> MeshData {
        let side = self.side;
        let mut mesh = MeshData::default();
        for j in 0..side {
            for i in 0..side {
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (
                    self.origin.0 + i as f64 * GRID,
                    self.origin.1 + j as f64 * GRID,
                );
                let (ii, jj) = (i as isize, j as isize);
                let h = self.vertex(ii, jj);
                mesh.vertices.push([
                    (east - origin[0]) as f32,
                    (h - origin[1]) as f32,
                    (-north - origin[2]) as f32,
                ]);
                // Central differences; the border ring provides the outer neighbours.
                let slope_east = (self.vertex(ii + 1, jj) - self.vertex(ii - 1, jj)) / (2.0 * GRID);
                let slope_north =
                    (self.vertex(ii, jj + 1) - self.vertex(ii, jj - 1)) / (2.0 * GRID);
                mesh.normals.push(unit([-slope_east, 1.0, slope_north]));
                mesh.uvs.push([(east / GRID) as f32, (north / GRID) as f32]);
                mesh.colors
                    .push(landcover::color(land.cover_at(east, north)));
            }
        }
        let cells = side - 1;
        for j in 0..cells {
            for i in 0..cells {
                let index =
                    |i: usize, j: usize| u32::try_from(j * side + i).expect("chunk fits u32");
                let (sw, se, nw, ne) = (
                    index(i, j),
                    index(i + 1, j),
                    index(i, j + 1),
                    index(i + 1, j + 1),
                );
                mesh.indices.extend([sw, nw, ne, sw, ne, se]);
            }
        }
        mesh
    }
}

/// Levels terrain to just below the road near it and blends back to the natural height.
/// Under bridges the ground is only lowered (the valley stays open, but nothing may cover the
/// deck); above tunnels it is left alone.
fn level_to_road(natural: f64, nearest_road: Option<(f64, f64, Surface)>) -> f64 {
    let Some((distance, road_elevation, surface)) = nearest_road else {
        return natural;
    };
    let levelled = road_elevation - ROAD_SINK;
    let target = match surface {
        Surface::Ground => levelled,
        Surface::Bridge => natural.min(levelled),
        Surface::Tunnel => return natural,
    };
    if distance <= FLAT_INNER {
        return target;
    }
    let t = ((distance - FLAT_INNER) / (FLAT_OUTER - FLAT_INNER)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    target + (natural - target) * smooth
}

#[allow(clippy::cast_possible_truncation)]
fn unit(v: [f64; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [
        (v[0] / length) as f32,
        (v[1] / length) as f32,
        (v[2] / length) as f32,
    ]
}

/// A deterministic pseudo-random number in `[0, 1)` from a seed (`SplitMix64`), so the world
/// looks the same every time a route is loaded.
pub(crate) fn hash(seed: i64) -> f64 {
    #[allow(clippy::cast_sign_loss)] // bit reinterpretation
    let mut z = (seed as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    #[allow(clippy::cast_precision_loss)] // 53 significant bits are plenty
    let value = (z >> 11) as f64 / (1u64 << 53) as f64;
    value
}

#[cfg(test)]
mod tests;
