// Test geometry compares f32 GPU data with exact, small reference values.
#![allow(clippy::cast_possible_truncation, clippy::float_cmp)]

use std::fmt::Write as _;

use torqa_osm::{Area, Building, LandCover, Structure, StructureKind, Waterway};

use super::*;

const METERS_PER_DEGREE: f64 = 111_195.0;

/// Terrain rising 10 % towards the east, 500 m at the route start.
struct EastwardSlope;

impl ElevationModel for EastwardSlope {
    fn elevation(
        &mut self,
        _lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
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

/// Latitude/longitude of a point `east`/`north` metres from the route start.
fn at(east: f64, north: f64) -> (f64, f64) {
    (
        46.0 + north / METERS_PER_DEGREE,
        7.0 + east / (METERS_PER_DEGREE * 46f64.to_radians().cos()),
    )
}

/// A closed square ring around a point.
fn square(east: f64, north: f64, half: f64) -> Vec<(f64, f64)> {
    vec![
        at(east - half, north - half),
        at(east + half, north - half),
        at(east + half, north + half),
        at(east - half, north + half),
        at(east - half, north - half),
    ]
}

/// A 1 km flat road due north at 500 m.
async fn route_north(structures: &[Structure]) -> Route {
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in 0..=100 {
        let (lat, lon) = at(0.0, f64::from(i) * 10.0);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>500</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    Route::from_gpx_with::<EastwardSlope>(&xml, None, structures)
        .await
        .unwrap()
}

async fn world(map: &MapData) -> World {
    generate(
        &route_north(&[]).await,
        &mut EastwardSlope,
        map,
        &mut |_, _| {},
    )
    .await
}

/// All terrain vertices in absolute coordinates.
fn terrain_vertices(world: &World) -> impl Iterator<Item = [f32; 3]> + '_ {
    world.chunks.iter().flat_map(|c| {
        c.mesh
            .vertices
            .iter()
            .map(move |v| [v[0] + c.center[0], v[1], v[2] + c.center[2]])
    })
}

fn triangles(mesh: &MeshData) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|k| mesh.vertices[k as usize]))
}

/// Normal of a triangle by the right-hand rule.
fn face_normal([first, second, third]: [[f32; 3]; 3]) -> [f32; 3] {
    let u = [0, 1, 2].map(|k| second[k] - first[k]);
    let v = [0, 1, 2].map(|k| third[k] - first[k]);
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn assert_valid(mesh: &MeshData) {
    assert_eq!(mesh.vertices.len(), mesh.normals.len());
    assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    assert!(mesh.colors.is_empty() || mesh.colors.len() == mesh.vertices.len());
    assert_eq!(mesh.indices.len() % 3, 0);
    assert!(
        mesh.indices
            .iter()
            .all(|&i| (i as usize) < mesh.vertices.len())
    );
}

#[tokio::test]
async fn terrain_surrounds_the_route() {
    let world = world(&MapData::default()).await;

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
async fn ground_and_road_face_up() {
    let world = world(&MapData::default()).await;

    for mesh in world.chunks.iter().map(|c| &c.mesh).chain([&world.road]) {
        assert_valid(mesh);
        // Godot draws clockwise triangles; seen from above, clockwise means a downward normal
        // by the right-hand rule.
        assert!(triangles(mesh).all(|t| face_normal(t)[1] < 0.0));
        assert!(mesh.normals.iter().all(|n| n[1] > 0.0));
    }
}

#[tokio::test]
async fn terrain_follows_the_model_away_from_the_road() {
    let world = world(&MapData::default()).await;

    let vertex = terrain_vertices(&world)
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
    let world = world(&MapData::default()).await;

    let near_road: Vec<_> = terrain_vertices(&world)
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
async fn ground_under_a_bridge_is_left_alone() {
    // A valley 30 m below the deck.
    struct Valley;
    impl ElevationModel for Valley {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (320.0..680.0).contains(&north) {
                470.0
            } else {
                500.0
            }))
        }
    }
    let bridge = Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    };
    let route = route_north(&[bridge]).await;
    let world = generate(&route, &mut Valley, &MapData::default(), &mut |_, _| {}).await;

    let under_bridge = terrain_vertices(&world)
        .find(|v| v[0].abs() < 9.0 && (v[2] + 500.0).abs() < 9.0)
        .unwrap();
    assert!((under_bridge[1] - 470.0).abs() < 0.01, "{under_bridge:?}");
}

#[tokio::test]
async fn without_terrain_data_the_world_follows_the_road() {
    let world = generate(
        &route_north(&[]).await,
        &mut NoData,
        &MapData::default(),
        &mut |_, _| {},
    )
    .await;

    assert!(world.fallback_samples > 0);
    assert!(terrain_vertices(&world).all(|v| (v[1] - 500.0).abs() < 0.3));
}

#[tokio::test]
async fn land_cover_colours_the_ground() {
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(300.0, 500.0, 100.0)],
        inner: vec![],
    };
    let world = world(&MapData {
        areas: vec![forest],
        ..MapData::default()
    })
    .await;

    let color_at = |east: f32, north: f32| {
        world
            .chunks
            .iter()
            .find_map(|c| {
                c.mesh
                    .vertices
                    .iter()
                    .zip(&c.mesh.colors)
                    .find_map(|(v, color)| {
                        let (x, z) = (v[0] + c.center[0], v[2] + c.center[2]);
                        ((x - east).abs() < 9.0 && (z + north).abs() < 9.0).then_some(*color)
                    })
            })
            .unwrap()
    };
    assert_eq!(
        color_at(300.0, 500.0),
        landcover::color(Some(LandCover::Forest))
    );
    assert_eq!(color_at(800.0, 500.0), landcover::color(None));
}

#[tokio::test]
async fn forests_get_trees_but_not_on_the_road() {
    // A forest across the road.
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(0.0, 500.0, 100.0)],
        inner: vec![],
    };
    let world = world(&MapData {
        areas: vec![forest],
        ..MapData::default()
    })
    .await;

    let trees: Vec<[f32; 3]> = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.trees
                .conifers
                .as_chunks::<12>()
                .0
                .iter()
                .chain(c.trees.broadleaves.as_chunks::<12>().0.iter())
                .map(move |t| [t[3] + c.center[0], t[7], t[11] + c.center[2]])
        })
        .collect();
    assert!(trees.len() > 200, "{} trees", trees.len());
    for [x, y, z] in &trees {
        assert!(x.abs() >= 8.0, "tree on the road at {x}");
        assert!(
            (-610.0..=-390.0).contains(z) && x.abs() <= 110.0,
            "tree outside forest"
        );
        // Beyond the ground levelled for the road, trees stand on the natural terrain.
        assert!(
            x.abs() < FLAT_OUTER as f32 || (y - (500.0 + 0.1 * x)).abs() < 1.0,
            "tree not on the ground: {y}"
        );
    }
}

#[tokio::test]
async fn buildings_stand_on_the_ground_with_walls_facing_out() {
    let house = Building {
        id: 42,
        outline: square(60.0, 500.0, 5.0),
        height: None,
        levels: Some(2.0),
    };
    let world = world(&MapData {
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    let (chunk, mesh) = world
        .chunks
        .iter()
        .find_map(|c| (!c.buildings.vertices.is_empty()).then_some((c, &c.buildings)))
        .expect("the house");
    assert_valid(mesh);
    let top = mesh.vertices.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    let bottom = mesh.vertices.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    // Ground at the lowest corner (500 m + 10 % of 55 m east), 2 storeys + 1 m roof slab.
    assert!((top - (505.5 + 7.0)).abs() < 0.3, "top {top}");
    assert!((bottom - (505.5 - 1.0)).abs() < 0.3, "bottom {bottom}");

    let centre = [60.0 - chunk.center[0], -500.0 - chunk.center[2]];
    for triangle in triangles(mesh) {
        let normal = face_normal(triangle);
        if normal[1].abs() > 0.5 {
            assert!(normal[1] < 0.0, "roof must be clockwise seen from above");
            continue;
        }
        // Clockwise seen from outside: the right-hand normal points into the building.
        let middle = [0, 2].map(|k| (triangle[0][k] + triangle[1][k] + triangle[2][k]) / 3.0);
        let outward = [middle[0] - centre[0], middle[1] - centre[1]];
        assert!(normal[0] * outward[0] + normal[2] * outward[1] < 0.0);
    }
}

#[tokio::test]
async fn rivers_become_water_ribbons_near_the_route() {
    let river = Waterway {
        width: 12.0,
        line: vec![at(-3000.0, 500.0), at(3000.0, 500.0)],
    };
    let world = world(&MapData {
        waterways: vec![river],
        ..MapData::default()
    })
    .await;

    assert_valid(&world.water);
    assert_ne!(world.water.vertices.len(), 0);
    assert!(world.water.colors.iter().all(|c| c[3] == 1.0));
    // Only within the corridor.
    assert!(
        world
            .water
            .vertices
            .iter()
            .all(|v| v[0].abs() <= CORRIDOR as f32 + 10.0)
    );
}

/// Every triangle is clockwise seen from the side its normal points to (Godot's front face):
/// the right-hand normal points against the stored vertex normal.
fn assert_faces_follow_normals(mesh: &MeshData) {
    for t in mesh.indices.as_chunks::<3>().0 {
        let triangle = t.map(|k| mesh.vertices[k as usize]);
        let normal = mesh.normals[t[0] as usize];
        let face = face_normal(triangle);
        let dot = face[0] * normal[0] + face[1] * normal[1] + face[2] * normal[2];
        assert!(
            dot < 0.0,
            "triangle {triangle:?} faces away from {normal:?}"
        );
    }
}

#[tokio::test]
async fn bridges_have_a_deck_and_pillars_down_to_the_valley() {
    // A 400 m bridge over a valley: the model is 30 m lower under the bridge.
    struct Valley;
    impl ElevationModel for Valley {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (320.0..680.0).contains(&north) {
                470.0
            } else {
                500.0
            }))
        }
    }
    let bridge = Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    };
    let route = route_north(&[bridge]).await;
    let world = generate(&route, &mut Valley, &MapData::default(), &mut |_, _| {}).await;

    let mesh = &world.structures;
    assert_valid(mesh);
    assert_faces_follow_normals(mesh);
    let lowest = mesh.vertices.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!(
        (lowest - 469.0).abs() < 0.1,
        "pillars reach the valley floor: {lowest}"
    );
    let highest = mesh.vertices.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    assert!(
        (highest - 501.0).abs() < 0.1,
        "parapets 1 m above the road: {highest}"
    );
    // Nothing outside the bridge.
    assert!(
        mesh.vertices
            .iter()
            .all(|v| (-720.0..=-280.0).contains(&v[2]))
    );
}

#[tokio::test]
async fn tunnels_are_tubes_visible_from_inside() {
    let tunnel = Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    };
    let route = route_north(&[tunnel]).await;
    let world = generate(
        &route,
        &mut EastwardSlope,
        &MapData::default(),
        &mut |_, _| {},
    )
    .await;

    let mesh = &world.structures;
    assert_valid(mesh);
    assert_faces_follow_normals(mesh);
    // Inner faces point towards the tunnel axis (x = 0, 500 m + half the radius up).
    let inward = mesh
        .vertices
        .iter()
        .zip(&mesh.normals)
        .filter(|(v, n)| n[0] * v[0] < 0.0 || (n[1] < 0.0 && v[1] > 500.0))
        .count();
    assert!(inward > 0);
    let top = mesh.vertices.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    assert!((top - 505.0).abs() < 0.1, "arch 5 m high: {top}");
}

#[tokio::test]
async fn all_world_meshes_face_their_normals() {
    let house = Building {
        id: 7,
        outline: square(60.0, 500.0, 5.0),
        height: Some(9.0),
        levels: None,
    };
    let world = world(&MapData {
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    assert_faces_follow_normals(&world.road);
    for chunk in &world.chunks {
        assert_faces_follow_normals(&chunk.buildings);
    }
}

#[tokio::test]
async fn ground_never_covers_a_bridge_deck() {
    // Terrain 10 m above the road where the bridge starts (an abutment in a slope).
    struct Bank;
    impl ElevationModel for Bank {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (300.0..400.0).contains(&north) {
                510.0
            } else {
                500.0
            }))
        }
    }
    let bridge = Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    };
    // The route keeps 500 m (file elevations), the bridge spans the bank.
    let route = route_north(&[bridge]).await;
    let world = generate(&route, &mut Bank, &MapData::default(), &mut |_, _| {}).await;

    for v in
        terrain_vertices(&world).filter(|v| v[0].abs() <= 10.0 && (-700.0..-300.0).contains(&v[2]))
    {
        assert!(v[1] <= 500.0 - 0.25 + 0.01, "terrain above the deck: {v:?}");
    }
}

#[tokio::test]
async fn minimap_draws_map_features_near_the_route_only() {
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(300.0, 500.0, 100.0)],
        inner: vec![],
    };
    let far_lake = Area {
        cover: LandCover::Water,
        outer: vec![square(9000.0, 500.0, 100.0)],
        inner: vec![],
    };
    let house = Building {
        id: 1,
        outline: square(60.0, 500.0, 5.0),
        height: None,
        levels: None,
    };
    let world = world(&MapData {
        areas: vec![forest, far_lake],
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    let flat = &world.minimap;
    assert_eq!(flat.vertices.len() % 3, 0);
    assert_eq!(flat.vertices.len(), flat.colors.len());
    // Forest square (2 triangles) and house (2 triangles); the lake is 9 km away.
    assert_eq!(flat.vertices.len(), 12);
    assert!(flat.vertices.iter().all(|v| v[0] < 500.0));
}
