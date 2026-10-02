//! 3D world geometry for Torqa (R16): terrain chunks in a corridor around the route and a road
//! mesh along it.
//!
//! Coordinates follow Godot: metres from the route start with x east, y up and z south
//! (−z is north). Triangles wind clockwise seen from their front, Godot's front-face order.

mod road;

use std::collections::BTreeSet;

use torqa_routes::{ElevationModel, LocalProjection, Route};
use tracing::{info, warn};

use road::RoadIndex;

/// Edge length of a terrain chunk.
const CHUNK_SIZE: f64 = 480.0;
/// Distance between terrain vertices.
const GRID: f64 = 16.0;
/// Terrain is generated up to this far from the route.
const CORRIDOR: f64 = 1500.0;
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
    /// Three vertex indices per triangle.
    pub indices: Vec<u32>,
}

/// A square piece of terrain.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainChunk {
    /// Centre of the chunk (at sea level).
    pub center: [f32; 3],
    /// Geometry in world coordinates.
    pub mesh: MeshData,
}

/// The generated world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    /// Terrain chunks around the route.
    pub chunks: Vec<TerrainChunk>,
    /// The road along the route.
    pub road: MeshData,
    /// Terrain samples that had no elevation data and followed the road instead.
    pub fallback_samples: usize,
}

/// Builds terrain and road for `route`, sampling heights from `model` (e.g. the terrain tiles).
/// Where the model has no data, the terrain follows the road's elevation.
pub async fn generate<M: ElevationModel>(route: &Route, model: &mut M) -> World {
    let projection = LocalProjection::for_route(route);
    let road = RoadIndex::new(route, &projection);
    let mut world = World {
        road: road.mesh(ROAD_HALF_WIDTH),
        ..World::default()
    };

    let chunk_cells = chunks_near_route(&road);
    for (cx, cn) in chunk_cells {
        let chunk = terrain_chunk(cx, cn, &projection, &road, model, &mut world).await;
        world.chunks.push(chunk);
    }
    if world.fallback_samples > 0 {
        warn!(
            samples = world.fallback_samples,
            "terrain data missing in places; terrain follows the road there"
        );
    }
    info!(chunks = world.chunks.len(), "world generated");
    world
}

/// Chunk grid cells (east, north) within the corridor of any part of the route.
fn chunks_near_route(road: &RoadIndex) -> BTreeSet<(i32, i32)> {
    let mut cells = BTreeSet::new();
    let reach = CORRIDOR + CHUNK_SIZE / 2.0 * std::f64::consts::SQRT_2;
    // Sampling the road every half chunk is enough to touch every chunk in reach.
    for (east, north) in road.samples(CHUNK_SIZE / 2.0) {
        let range = |center: f64| {
            let low = ((center - reach) / CHUNK_SIZE).floor();
            let high = ((center + reach) / CHUNK_SIZE).floor();
            #[allow(clippy::cast_possible_truncation)]
            // world coordinates are far below 2^31 chunks
            (low as i32..=high as i32)
        };
        for cx in range(east) {
            for cn in range(north) {
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

#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
async fn terrain_chunk<M: ElevationModel>(
    cx: i32,
    cn: i32,
    projection: &LocalProjection,
    road: &RoadIndex,
    model: &mut M,
    world: &mut World,
) -> TerrainChunk {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cells = (CHUNK_SIZE / GRID).round() as usize;
    let side = cells + 1;
    // Heights with a one-vertex border, so normals at the edges match the neighbours.
    let bordered = side + 2;
    let origin_e = f64::from(cx) * CHUNK_SIZE;
    let origin_n = f64::from(cn) * CHUNK_SIZE;
    let chunk_road_elevation = road
        .nearest(
            origin_e + CHUNK_SIZE / 2.0,
            origin_n + CHUNK_SIZE / 2.0,
            f64::INFINITY,
        )
        .map_or(0.0, |(_, elevation)| elevation);

    let mut heights = vec![0.0; bordered * bordered];
    for j in 0..bordered {
        for i in 0..bordered {
            #[allow(clippy::cast_precision_loss)] // small grid indices
            let (east, north) = (
                origin_e + (i as f64 - 1.0) * GRID,
                origin_n + (j as f64 - 1.0) * GRID,
            );
            let (lat, lon) = projection.unproject(east, north);
            let natural = if let Ok(height) = model.elevation(lat, lon).await {
                height
            } else {
                world.fallback_samples += 1;
                road.nearest(east, north, FALLBACK_RADIUS)
                    .map_or(chunk_road_elevation, |(_, elevation)| elevation)
            };
            heights[j * bordered + i] =
                level_to_road(natural, road.nearest(east, north, FLAT_OUTER));
        }
    }

    let height = |i: usize, j: usize| heights[(j + 1) * bordered + (i + 1)];
    let mut mesh = MeshData::default();
    for j in 0..side {
        for i in 0..side {
            #[allow(clippy::cast_precision_loss)]
            let (east, north) = (origin_e + i as f64 * GRID, origin_n + j as f64 * GRID);
            let h = height(i, j);
            mesh.vertices.push([east as f32, h as f32, -north as f32]);
            // Central differences; (bi, bj) index the bordered grid, so neighbours always exist.
            let (bi, bj) = (i + 1, j + 1);
            let at = |bi: usize, bj: usize| heights[bj * bordered + bi];
            let slope_east = (at(bi + 1, bj) - at(bi - 1, bj)) / (2.0 * GRID);
            let slope_north = (at(bi, bj + 1) - at(bi, bj - 1)) / (2.0 * GRID);
            mesh.normals.push(unit([-slope_east, 1.0, slope_north]));
            mesh.uvs.push([(east / GRID) as f32, (north / GRID) as f32]);
        }
    }
    for j in 0..cells {
        for i in 0..cells {
            let index = |i: usize, j: usize| u32::try_from(j * side + i).expect("chunk fits u32");
            let (sw, se, nw, ne) = (
                index(i, j),
                index(i + 1, j),
                index(i, j + 1),
                index(i + 1, j + 1),
            );
            mesh.indices.extend([sw, nw, ne, sw, ne, se]);
        }
    }

    TerrainChunk {
        center: [
            (origin_e + CHUNK_SIZE / 2.0) as f32,
            0.0,
            -(origin_n + CHUNK_SIZE / 2.0) as f32,
        ],
        mesh,
    }
}

/// Levels terrain to just below the road near it and blends back to the natural height.
fn level_to_road(natural: f64, nearest_road: Option<(f64, f64)>) -> f64 {
    let Some((distance, road_elevation)) = nearest_road else {
        return natural;
    };
    let levelled = road_elevation - ROAD_SINK;
    if distance <= FLAT_INNER {
        return levelled;
    }
    let t = ((distance - FLAT_INNER) / (FLAT_OUTER - FLAT_INNER)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    levelled + (natural - levelled) * smooth
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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// Terrain rising 10 % towards the east, 500 m at the route start.
    struct EastwardSlope;

    impl ElevationModel for EastwardSlope {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * 111_195.0 * 46f64.to_radians().cos();
            std::future::ready(Ok(500.0 + 0.1 * east))
        }
    }

    /// No terrain data at all, as when offline without cached tiles.
    struct NoData;

    impl ElevationModel for NoData {
        fn elevation(
            &mut self,
            _lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Err("offline".to_owned()))
        }
    }

    /// A 1 km flat road due north at 500 m.
    async fn route_north() -> Route {
        let mut xml = String::from("<gpx><trk><trkseg>");
        for i in 0..=10 {
            let lat = 46.0 + f64::from(i) * 100.0 / 111_195.0;
            let _ = write!(xml, r#"<trkpt lat="{lat}" lon="7"><ele>500</ele></trkpt>"#);
        }
        xml.push_str("</trkseg></trk></gpx>");
        Route::from_gpx(&xml, None).await.unwrap()
    }

    fn triangles(mesh: &MeshData) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|k| mesh.vertices[k as usize]))
    }

    /// Normal of a triangle by the right-hand rule; negative y means clockwise seen from above.
    fn face_normal_y([first, second, third]: [[f32; 3]; 3]) -> f32 {
        let edge_1 = [0, 1, 2].map(|k| second[k] - first[k]);
        let edge_2 = [0, 1, 2].map(|k| third[k] - first[k]);
        edge_1[2] * edge_2[0] - edge_1[0] * edge_2[2]
    }

    #[tokio::test]
    async fn terrain_surrounds_the_route() {
        let world = generate(&route_north().await, &mut EastwardSlope).await;

        assert_ne!(world.chunks.len(), 0);
        // Every chunk is within the corridor; the route spans x = 0 and z = 0..−1000.
        for chunk in &world.chunks {
            let [x, _, z] = chunk.center;
            assert!(
                x.abs() < 2000.0 && (-2700.0..1700.0).contains(&z),
                "{x}, {z}"
            );
        }
        assert_eq!(world.fallback_samples, 0);
    }

    #[tokio::test]
    async fn meshes_are_valid_and_face_up() {
        let world = generate(&route_north().await, &mut EastwardSlope).await;

        for mesh in world.chunks.iter().map(|c| &c.mesh).chain([&world.road]) {
            assert_eq!(mesh.vertices.len(), mesh.normals.len());
            assert_eq!(mesh.vertices.len(), mesh.uvs.len());
            assert_eq!(mesh.indices.len() % 3, 0);
            assert!(
                mesh.indices
                    .iter()
                    .all(|&i| (i as usize) < mesh.vertices.len())
            );
            // Godot draws clockwise triangles; seen from above they must be clockwise.
            assert!(triangles(mesh).all(|t| face_normal_y(t) < 0.0));
            assert!(mesh.normals.iter().all(|n| n[1] > 0.0));
        }
    }

    #[tokio::test]
    async fn terrain_follows_the_model_away_from_the_road() {
        let world = generate(&route_north().await, &mut EastwardSlope).await;

        let vertex = world
            .chunks
            .iter()
            .flat_map(|c| &c.mesh.vertices)
            .find(|v| (v[0] - 1000.0).abs() < 9.0 && (v[2] + 500.0).abs() < 9.0)
            .expect("a vertex 1 km east of the route");
        let expected = 500.0 + 0.1 * vertex[0];
        assert!(
            (vertex[1] - expected).abs() < 0.5,
            "{} vs {expected}",
            vertex[1]
        );
    }

    #[tokio::test]
    async fn terrain_is_levelled_just_below_the_road() {
        let world = generate(&route_north().await, &mut EastwardSlope).await;

        let near_road: Vec<_> = world
            .chunks
            .iter()
            .flat_map(|c| &c.mesh.vertices)
            .filter(|v| v[0].abs() <= 10.0 && (-1000.0..0.0).contains(&v[2]))
            .collect();
        assert_ne!(near_road.len(), 0);
        for v in near_road {
            assert!((v[1] - (500.0 - 0.25)).abs() < 0.01, "{v:?}");
        }
        assert!(
            world
                .road
                .vertices
                .iter()
                .all(|v| (v[1] - 500.0).abs() < 0.01)
        );
    }

    #[tokio::test]
    async fn without_terrain_data_the_world_follows_the_road() {
        let world = generate(&route_north().await, &mut NoData).await;

        assert!(world.fallback_samples > 0);
        let all_flat = world
            .chunks
            .iter()
            .flat_map(|c| &c.mesh.vertices)
            .all(|v| (v[1] - 500.0).abs() < 0.3);
        assert!(all_flat);
    }
}
