// Test geometry compares f32 GPU data with exact, small reference values, reads small
// non-negative style codes from colours, and turns small counts into densities.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_cmp
)]

use std::fmt::Write as _;

use orqwa_osm::{Area, Building, LandCover, Structure, StructureKind, Waterway};

use super::*;

#[path = "urban_tests.rs"]
mod urban;

#[path = "water_tests.rs"]
mod water;

#[path = "earthworks_tests.rs"]
mod earthworks;

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

#[tokio::test]
async fn regional_rides_place_houses_and_only_mapped_castles_offline() {
    for (lat, lon, expected_house, expected_castle) in [
        (35.01, 135.77, "japanese_house", "japanese_castle"),
        (37.57, 126.98, "japanese_house", "japanese_castle"),
        (26.21, 127.68, "tropical", "japanese_castle"),
        (59.33, 18.07, "nordic_house", "nordic_castle"),
        (48.86, 2.35, "french_house", "french_castle"),
        (46.95, 7.45, "house", "castle"),
    ] {
        let xml = format!(
            "<gpx><trk><trkseg><trkpt lat='{lat}' lon='{lon}'><ele>400</ele></trkpt>\
             <trkpt lat='{}' lon='{lon}'><ele>400</ele></trkpt></trkseg></trk></gpx>",
            lat + 0.009
        );
        let route = Route::from_gpx(&xml, None).await.unwrap();
        let projection = LocalProjection::for_route(&route);
        let building = |id, east, north, length: f64, width: f64| Building {
            id,
            outline: [
                (-1.0, -1.0),
                (1.0, -1.0),
                (1.0, 1.0),
                (-1.0, 1.0),
                (-1.0, -1.0),
            ]
            .map(|(x, y)| projection.unproject(east + x * length / 2.0, north + y * width / 2.0))
            .to_vec(),
            height: None,
            levels: Some(2.0),
            color: None,
        };
        let map = MapData {
            buildings: vec![
                building(1, 40.0, 200.0, 12.0, 9.0),
                building(2, 60.0, 600.0, 22.0, 16.0),
            ],
            castles: vec![projection.unproject(60.0, 600.0)],
            ..MapData::default()
        };
        let world = generate(&route, &mut NoData, &map, &mut |_, _| {}).await;
        let names: Vec<&str> = world
            .chunks
            .iter()
            .flat_map(|c| &c.modelled)
            .flat_map(|cell| cell.models.keys().map(String::as_str))
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with(expected_house)),
            "{lat}: {names:?}"
        );
        assert!(
            names.iter().any(|n| n.starts_with(expected_castle)),
            "{lat}: {names:?}"
        );
        assert_eq!(names.len(), 2, "only the marked building becomes a castle");
    }
}

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
    rectangle(east, north, half, half)
}

/// A closed rectangular ring around a point, `half_east` by `half_north` from it.
fn rectangle(east: f64, north: f64, half_east: f64, half_north: f64) -> Vec<(f64, f64)> {
    vec![
        at(east - half_east, north - half_north),
        at(east + half_east, north - half_north),
        at(east + half_east, north + half_north),
        at(east - half_east, north + half_north),
        at(east - half_east, north - half_north),
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
    // The road ridden, with the structures on it.
    let mut roads = Vec::new();
    let mut from = at(0.0, -20.0);
    for structure in structures {
        roads.push(orqwa_osm::Road {
            class: orqwa_osm::RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![from, structure.line[0]],
            structure: None,
        });
        roads.push(orqwa_osm::Road {
            class: orqwa_osm::RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: structure.line.clone(),
            structure: Some(structure.kind),
        });
        from = structure.line[structure.line.len() - 1];
    }
    roads.push(orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![from, at(0.0, 1020.0)],
        structure: None,
    });
    let map = MapData {
        roads,
        ..MapData::default()
    };
    Route::from_gpx_with::<EastwardSlope>(&xml, None, &map)
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

#[tokio::test]
async fn ridden_cycleways_and_surrounding_highways_keep_their_own_widths_and_surfaces() {
    use orqwa_osm::{RoadDetails, RoadKind, RoadSurface};
    let mut cycleway = road_north();
    cycleway.class = RoadClass::Path;
    cycleway.details = RoadDetails {
        kind: RoadKind::Cycleway,
        surface: RoadSurface::Paved,
        ..RoadDetails::default()
    };
    let mut highway = road_north();
    highway.class = RoadClass::Major;
    highway.details = RoadDetails {
        kind: RoadKind::Motorway,
        surface: RoadSurface::Paved,
        oneway: true,
        ..RoadDetails::default()
    };
    highway.line = vec![at(50.0, 100.0), at(50.0, 900.0)];
    let mut unpaved = road_north();
    unpaved.details.surface = RoadSurface::Unpaved;
    unpaved.line = vec![at(-50.0, 100.0), at(-50.0, 900.0)];
    let world = world(&MapData {
        roads: vec![cycleway, highway, unpaved],
        ..MapData::default()
    })
    .await;
    let top: Vec<_> = world
        .road
        .vertices
        .iter()
        .zip(&world.road.uvs)
        .filter(|(_, uv)| uv[0] == 0.0 || uv[0] == 1.0)
        .collect();
    assert!(
        top.iter().all(|(v, _)| v[0].abs() < 1.3),
        "a ridden cycleway stays narrow"
    );
    assert!(
        world.road.colors.iter().all(|c| c[0] == 0.0 && c[2] == 0.0),
        "paved cycleway, no invented paint"
    );
    for chunk in &world.chunks {
        for mesh in [&chunk.streets, &chunk.tracks] {
            assert_eq!(mesh.vertices.len(), mesh.colors.len());
        }
    }
    let highway_widths: Vec<_> = world
        .chunks
        .iter()
        .flat_map(|c| &c.streets.colors)
        .filter(|c| c[2] == 0.5)
        .map(|c| c[1] * 32.0)
        .collect();
    assert!(!highway_widths.is_empty() && highway_widths.iter().all(|&w| w > 9.0));
    assert!(
        world
            .chunks
            .iter()
            .flat_map(|c| &c.tracks.colors)
            .any(|c| c[0] == 0.125 && c[2] == 0.0),
        "explicit unpaved local road uses gravel"
    );
}

#[tokio::test]
async fn road_appearance_matching_ignores_crossings_parallel_neighbours_and_other_levels() {
    use orqwa_osm::{RoadDetails, RoadKind, RoadSurface};
    let route = route_north(&[]).await;
    let projection = LocalProjection::for_route(&route);
    let mut path = road_north();
    path.details = RoadDetails {
        kind: RoadKind::Cycleway,
        surface: RoadSurface::Paved,
        ..RoadDetails::default()
    };
    let mut crossing = road_north();
    crossing.details.kind = RoadKind::Motorway;
    crossing.line = vec![at(-100.0, 500.0), at(100.0, 500.0)];
    let mut upper = path.clone();
    upper.structure = Some(StructureKind::Bridge);
    upper.details.kind = RoadKind::Motorway;
    let mut neighbour = path.clone();
    neighbour.details.kind = RoadKind::Primary;
    neighbour.line = vec![at(5.0, 0.0), at(5.0, 1000.0)];
    let index = appearance::Index::new(
        &MapData {
            roads: vec![crossing, upper, neighbour, path],
            ..MapData::default()
        },
        &projection,
    );
    let point = projection.project(at(0.0, 500.0).0, at(0.0, 500.0).1);
    assert!(index.at(point, (0.0, 1.0), Surface::Ground).0.width < 3.0);
    assert!(index.at(point, (0.0, 1.0), Surface::Bridge).0.width > 9.0);
    assert!(
        (index
            .at((1000.0, 1000.0), (0.0, 1.0), Surface::Ground)
            .0
            .width
            - 6.0)
            .abs()
            < 0.01,
        "unmapped route has a stable fallback"
    );
}

/// Offline, controlled scene rendered by `app/tools/render_roads.gd`. These are generated
/// meshes from the real appearance policy, rather than cross-sections redrawn in GDScript.
#[tokio::test]
#[ignore = "writes screenshots/road-profiles.json for visual review"]
#[allow(clippy::too_many_lines)] // one complete table of review profiles and their mesh export
async fn road_profiles_render_fixture() {
    use orqwa_osm::{Road, RoadDetails, RoadKind, RoadSurface};
    let specs = [
        (
            "Ridden cycleway",
            RoadClass::Path,
            RoadKind::Cycleway,
            RoadSurface::Paved,
            false,
        ),
        (
            "Motorway carriageway",
            RoadClass::Major,
            RoadKind::Motorway,
            RoadSurface::Paved,
            false,
        ),
        (
            "Motorway ramp",
            RoadClass::Major,
            RoadKind::Motorway,
            RoadSurface::Paved,
            true,
        ),
        (
            "Primary road",
            RoadClass::Major,
            RoadKind::Primary,
            RoadSurface::Paved,
            false,
        ),
        (
            "Local street",
            RoadClass::Street,
            RoadKind::Local,
            RoadSurface::Paved,
            false,
        ),
        (
            "Service road",
            RoadClass::Service,
            RoadKind::Service,
            RoadSurface::Paved,
            false,
        ),
        (
            "Unpaved track",
            RoadClass::Track,
            RoadKind::Track,
            RoadSurface::Unpaved,
            false,
        ),
        (
            "Unpaved path",
            RoadClass::Path,
            RoadKind::Path,
            RoadSurface::Unpaved,
            false,
        ),
    ];
    let roads = specs
        .iter()
        .enumerate()
        .map(|(i, &(_, class, kind, surface, ramp))| {
            let east = i as f64 * 19.0;
            Road {
                class,
                details: RoadDetails {
                    kind,
                    surface,
                    ramp,
                    oneway: kind == RoadKind::Motorway,
                    roundabout: false,
                },
                line: vec![at(east, -20.0), at(east, 1020.0)],
                structure: None,
            }
        })
        .collect();
    let map = MapData {
        roads,
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let arrays = |mesh: &MeshData, center: [f32; 3], shoulders| {
        serde_json::json!({
            "vertices": mesh.vertices.iter().map(|v| [v[0]+center[0], v[1]+center[1]-500.0, v[2]+center[2]]).collect::<Vec<_>>(),
            "normals": mesh.normals, "uvs": mesh.uvs, "colors": mesh.colors, "indices": mesh.indices, "shoulders": shoulders,
        })
    };
    let mut meshes = vec![arrays(&world.road, [0.0; 3], true)];
    for chunk in &world.chunks {
        for mesh in [&chunk.streets, &chunk.tracks] {
            if !mesh.vertices.is_empty() {
                meshes.push(arrays(mesh, chunk.center, false));
            }
        }
    }
    let labels: Vec<_> = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| serde_json::json!({ "text": spec.0, "east": i as f64 * 19.0 }))
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../screenshots/road-profiles.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({"meshes":meshes, "labels": labels})).unwrap(),
    )
    .unwrap();
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
async fn streets_lie_on_the_ground_however_it_folds() {
    use orqwa_osm::{Road, RoadClass};

    // A wide street crossing the route on the hillside: through the cutting into it, over the
    // cutting's sharp top edge and on up the natural slope.
    let world = world(&MapData {
        roads: vec![Road {
            class: RoadClass::Major,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(-150.0, 300.0), at(150.0, 300.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;

    let mut checked = 0;
    for step in 0..1200 {
        #[allow(clippy::cast_precision_loss)] // small steps
        let x = -150.0 + step as f32 * 0.25;
        for offset in [-3.3_f32, -1.5, 0.0, 1.5, 3.3] {
            let z = -300.0 + offset;
            let (Some(ground), Some(street)) = (ground_at(&world, x, z), street_at(&world, x, z))
            else {
                continue;
            };
            assert!(
                street > ground,
                "the ground through the street at {x}, {z}: {ground} over {street}"
            );
            checked += 1;
        }
    }
    assert!(checked > 5000, "{checked} points checked");
}

#[tokio::test]
async fn where_streets_join_the_road_its_edge_is_road_not_shoulder() {
    use orqwa_osm::{Road, RoadClass};

    // A street joining from the west at 300 m, none from the east.
    let world = world(&MapData {
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(-150.0, 300.0), at(0.0, 300.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;

    let road = &world.road;
    let rings = road.uvs.as_chunks::<6>().0;
    let bevels = |near: f32| -> Vec<(f32, f32)> {
        rings
            .iter()
            .filter(|ring| (ring[1][1] - near).abs() < 1.0)
            .map(|ring| (ring[1][0], ring[4][0]))
            .collect()
    };
    for (left, right) in bevels(300.0) {
        // Road (u within 0–1) on the left where the street joins, shoulder on the right.
        assert!(
            (0.0..=1.0).contains(&left),
            "left bevel {left} at the junction"
        );
        assert!(right > 1.0, "right bevel {right} at the junction");
    }
    for (left, right) in bevels(600.0) {
        assert!(
            left < 0.0 && right > 1.0,
            "bevels {left}, {right} away from junctions"
        );
    }
}

#[tokio::test]
async fn streets_meet_with_rounded_kerbs_and_end_round() {
    use orqwa_osm::{Road, RoadClass};

    let street = |points: &[(f64, f64)]| Road {
        class: RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: points.iter().map(|&(e, n)| at(e, n)).collect(),
        structure: None,
    };
    // A street east–west at 300 m north, and one from the south ending on it at 160 m east
    // (sharing its point there, as the map's streets do); 5.5 m wide, away from the route.
    let world = world(&MapData {
        roads: vec![
            street(&[(60.0, 300.0), (160.0, 300.0), (260.0, 300.0)]),
            street(&[(160.0, 200.0), (160.0, 300.0)]),
        ],
        ..MapData::default()
    })
    .await;
    let paved = |east: f64, north: f64| street_at(&world, east as f32, -north as f32).is_some();

    // Both corners of the T have a kerb: the corner by the streets' edges (2.75 m out) is
    // paved, the grass beyond the kerb's curve is not.
    for side in [-1.0, 1.0] {
        let edge = 160.0 + side * 2.75;
        assert!(
            paved(edge + side * 0.5, 300.0 - 2.75 - 0.5),
            "corner {side}"
        );
        assert!(
            !paved(edge + side * 2.0, 300.0 - 2.75 - 2.0),
            "beyond the kerb {side}"
        );
    }
    // None across the street, where nothing joins.
    assert!(!paved(160.0 + 3.25, 300.0 + 3.25));
    // The dead end at the south is round, not square.
    assert!(paved(160.0, 200.0 - 1.5));
    assert!(!paved(160.0 + 2.5, 200.0 - 2.0));
}

#[tokio::test]
async fn streets_meeting_the_road_ridden_get_kerbs_at_its_edge() {
    use orqwa_osm::{Road, RoadClass};

    let street = |points: &[(f64, f64)]| Road {
        class: RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: points.iter().map(|&(e, n)| at(e, n)).collect(),
        structure: None,
    };
    // The road ridden as the map has it, and a street from the west ending on it at 600 m.
    let world = world(&MapData {
        roads: vec![
            street(&[(0.0, -20.0), (0.0, 600.0), (0.0, 1020.0)]),
            street(&[(-150.0, 600.0), (0.0, 600.0)]),
        ],
        ..MapData::default()
    })
    .await;
    let paved = |east: f64, north: f64| street_at(&world, east as f32, -north as f32).is_some();

    // The road is 6 m wide: its corners with the street, off its edge, are paved.
    for side in [-1.0, 1.0] {
        assert!(
            paved(-3.0 - 0.6, 600.0 + side * (2.75 + 0.6)),
            "corner {side}"
        );
        assert!(
            !paved(-3.0 - 2.5, 600.0 + side * (2.75 + 2.5)),
            "beyond {side}"
        );
    }
    // The other side of the road, where nothing joins, keeps its plain edge.
    assert!(!paved(3.6, 600.0 + 3.35));
}

#[tokio::test]
async fn streets_on_a_hillside_are_level_across_the_ground_shaped_around_them() {
    use orqwa_osm::{Road, RoadClass};

    // #116: a street along the hillside, which rises 10 % eastwards. It lay tilted with the
    // slope, its uphill edge half a metre above its downhill one.
    let world = world(&MapData {
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(200.0, 100.0), at(200.0, 900.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;

    // The natural ground at the street's centre line.
    let level = 500.0 + 0.1 * 200.0;
    for north in (200..=800).step_by(50) {
        let z = -(north as f32);
        let edge = |x: f32| street_at(&world, x, z).expect("the street");
        let (west, east) = (edge(200.0 - 2.5), edge(200.0 + 2.5));
        assert!(
            (west - east).abs() < 0.02,
            "a tilted street at {north}: {west} west, {east} east"
        );
        assert!((west - level).abs() < 0.15, "the street at {west}");
        // The ground beside it level with it, then back on the natural slope a little way out:
        // cut into the hillside above, banked down below.
        for x in [200.0 - 5.0, 200.0 + 5.0] {
            let ground = ground_at(&world, x, z).expect("ground");
            assert!(
                (ground - level).abs() < 0.05,
                "the ground beside the street at {x}, {north}: {ground}"
            );
        }
        for x in [200.0 - 25.0, 200.0 + 25.0] {
            let ground = ground_at(&world, x, z).expect("ground");
            let natural = 500.0 + 0.1 * x;
            assert!(
                (ground - natural).abs() < 0.05,
                "the ground away from the street at {x}, {north}: {ground}, not {natural}"
            );
        }
    }
}

/// The railways' bed along its middle, as the mesh has it: (x, height, z) per point.
fn rail_bed(world: &World) -> Vec<[f32; 3]> {
    world
        .railways
        .vertices
        .as_chunks::<6>()
        .0
        .iter()
        .map(|ring| [0, 1, 2].map(|k| f32::midpoint(ring[2][k], ring[3][k])))
        .collect()
}

/// A railway of the map through `points` (metres east/north).
fn railway(points: &[(f64, f64)], structure: Option<StructureKind>) -> orqwa_osm::Railway {
    orqwa_osm::Railway {
        line: points.iter().map(|&(e, n)| at(e, n)).collect(),
        structure,
        funicular: false,
    }
}

#[tokio::test]
async fn railways_run_smoothly_level_across_and_keep_the_forest_off() {
    // Bumpy land rising eastwards; a line due north across it, through a forest.
    struct Bumpy;
    impl ElevationModel for Bumpy {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(500.0 + 0.1 * east + 3.0 * (north / 25.0).sin()))
        }
    }
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(300.0, 500.0, 120.0)],
        inner: vec![],
    };
    let map = MapData {
        areas: vec![forest],
        railways: vec![railway(&[(300.0, 100.0), (300.0, 900.0)], None)],
        ..MapData::default()
    };
    let world = generate(&route_north(&[]).await, &mut Bumpy, &map, &mut |_, _| {}).await;

    let bed = rail_bed(&world);
    assert!(bed.len() > 100, "{} points of track", bed.len());
    // Gentle grades, no bumps: the terrain's 3 m waves are gone.
    for pair in bed.windows(2) {
        let run = (pair[1][2] - pair[0][2]).abs().max(1.0);
        let grade = (pair[1][1] - pair[0][1]).abs() / run;
        assert!(grade < 0.041, "a {grade} grade at {:?}", pair[0]);
    }
    // Level across, the ground shaped just below the bed beside it.
    for ring in world.railways.vertices.as_chunks::<6>().0 {
        assert!(
            (ring[2][1] - ring[3][1]).abs() < 0.01,
            "a tilted bed at {:?}",
            ring[2]
        );
    }
    for point in bed.iter().step_by(10) {
        for side in [-2.5_f32, 2.5] {
            let ground = ground_at(&world, point[0] + side, point[2]).expect("ground");
            assert!(
                ground < point[1] && point[1] - ground < 0.5,
                "ground at {ground} beside the bed at {point:?}"
            );
        }
    }
    // The forest grows beside the line, never on it.
    let trees = plants_of(&world, &["conifer", "broadleaf"]);
    assert!(
        trees
            .iter()
            .filter(|t| (t[0] - 300.0).abs() < 100.0)
            .count()
            > 50
    );
    for [x, _, z] in &trees {
        assert!((x - 300.0).abs() > 1.6, "a tree on the railway at {x}, {z}");
    }
}

#[tokio::test]
async fn the_land_reaches_from_the_detailed_ground_to_the_horizon() {
    // Land rising eastwards, and beyond 6 km east a lake whose surface the model reads at 600 m.
    struct Far;
    impl ElevationModel for Far {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(if east > 6000.0 {
                600.0
            } else {
                500.0 + 0.01 * east.abs()
            }))
        }
    }
    let route = route_north(&[]).await;
    let world = generate(&route, &mut Far, &MapData::default(), &mut |_, _| {}).await;
    let land = horizon(&route, &mut Far).await;

    assert_valid(&land.ground);
    assert_valid(&land.water);
    let detailed = |x: f32, z: f32| {
        world.chunks.iter().any(|c| {
            (x - c.center[0]).abs() < CHUNK_SIZE as f32 / 2.0
                && (z - c.center[2]).abs() < CHUNK_SIZE as f32 / 2.0
        })
    };
    for cell in land.ground.vertices.as_chunks::<4>().0 {
        let middle = [0, 2].map(|k| cell.iter().map(|v| v[k]).sum::<f32>() / 4.0);
        assert!(
            !detailed(middle[0], middle[1]),
            "land over the detailed ground at {middle:?}"
        );
    }
    // Out to the horizon, west and east, at the land's height (a little lower).
    let reach = land
        .ground
        .vertices
        .iter()
        .map(|v| v[0])
        .fold(0.0_f32, f32::min);
    assert!(
        reach < -(HORIZON as f32) + 1000.0,
        "the land ends at {reach} m"
    );
    for v in &land.ground.vertices {
        let measured = 500.0 + 0.01 * v[0].abs();
        // On the shore the model may read the lake already.
        let shore = (v[0] - 6000.0).abs() < 1.0 && (v[1] - 598.0).abs() < 0.5;
        assert!(
            shore || (v[1] - (measured - 2.0)).abs() < 0.5,
            "land at {} at {}",
            v[1],
            v[0]
        );
    }
    // The lake far east is water, at its level.
    assert_ne!(land.water.vertices.len(), 0);
    assert!(
        land.water
            .vertices
            .iter()
            .all(|v| v[0] >= 6000.0 && (v[1] - 600.02).abs() < 0.01)
    );
    assert!(land.ground.vertices.iter().all(|v| v[0] <= 6240.0));
}

#[tokio::test]
async fn roundabouts_get_a_raised_island_with_a_kerb() {
    use orqwa_osm::{Road, RoadClass};

    // A roundabout of 20 m radius east of the route; a long thin loop, a road round a square
    // block and a footpath round a pond (no roundabouts).
    let ring: Vec<(f64, f64)> = (0..=24)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k) / 24.0;
            at(200.0 + 20.0 * angle.cos(), 500.0 + 20.0 * angle.sin())
        })
        .collect();
    let loop_ = vec![
        at(150.0, 800.0),
        at(250.0, 800.0),
        at(250.0, 806.0),
        at(200.0, 807.0),
        at(150.0, 806.0),
        at(150.0, 800.0),
    ];
    // The block's road has a point every 10 m, as mapped roads do.
    let block: Vec<(f64, f64)> = (0..=20)
        .map(|k| {
            let (side, step) = (k / 5, f64::from(k % 5) * 10.0);
            let (east, north) = match side {
                0 => (-25.0 + step, -25.0),
                1 => (25.0, -25.0 + step),
                2 => (25.0 - step, 25.0),
                3 => (-25.0, 25.0 - step),
                _ => (-25.0, -25.0),
            };
            at(200.0 + east, 300.0 + north)
        })
        .collect();
    let path: Vec<(f64, f64)> = (0..=24)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k) / 24.0;
            at(200.0 + 15.0 * angle.cos(), 650.0 + 15.0 * angle.sin())
        })
        .collect();
    let world = world(&MapData {
        roads: [
            (RoadClass::Street, ring),
            (RoadClass::Street, loop_),
            (RoadClass::Service, block),
            (RoadClass::Path, path),
        ]
        .into_iter()
        .map(|(class, line)| Road {
            class,
            details: orqwa_osm::RoadDetails::default(),
            line,
            structure: None,
        })
        .collect(),
        ..MapData::default()
    })
    .await;
    // How far the topmost ground lies over the natural slope (0.1 m per metre east).
    let lift = |x: f32, z: f32| top_of_ground(&world, x, z).map(|top| top - (500.0 + 0.1 * x));

    // Inside the ring, the island stands 20 cm up from the ground (which the ring levels
    // beside it, #116); on the ring and outside, the ground is as it was.
    for (x, z) in [
        (200.0, -500.0),
        (210.0, -505.0),
        (190.0, -492.0),
        (214.0, -500.0),
    ] {
        let top = top_of_ground(&world, x, z).expect("island");
        let up = top - ground_at(&world, x, z).expect("ground");
        assert!((up - 0.2).abs() < 0.02, "island at {up} m at {x}, {z}");
    }
    assert!(lift(200.0, -500.0).is_some_and(|up| (up - 0.2).abs() < 0.02));
    for (x, z) in [
        (220.0, -500.0),
        (230.0, -500.0),
        (200.0, -525.0),
        (200.0, -803.0),
        (200.0, -300.0),
        (210.0, -290.0),
        (200.0, -650.0),
    ] {
        let up = lift(x, z).expect("ground");
        assert!(up.abs() < 0.02, "ground raised by {up} m at {x}, {z}");
    }
    // Its kerb: upright faces all round the island.
    let kerb = world
        .chunks
        .iter()
        .flat_map(|c| triangles(&c.mesh).map(move |t| (t, c.center)))
        .filter(|(t, centre)| {
            let normal = face_normal(*t);
            let middle = [0, 2].map(|k| t.iter().map(|v| v[k]).sum::<f32>() / 3.0);
            let (x, z) = (middle[0] + centre[0], middle[1] + centre[2]);
            normal[1].abs() < 0.01 && ((x - 200.0).hypot(z + 500.0) - 16.6).abs() < 1.0
        })
        .count();
    assert!(kerb >= 40, "{kerb} kerb faces");
}

#[tokio::test]
async fn railways_tunnel_through_hills_rather_than_climb_them() {
    // A hill 40 m high in the line's way.
    struct Hill;
    impl ElevationModel for Hill {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(500.0 + (40.0 - 0.3 * (north - 500.0).abs()).max(0.0)))
        }
    }
    let map = MapData {
        railways: vec![railway(&[(300.0, 100.0), (300.0, 900.0)], None)],
        ..MapData::default()
    };
    let world = generate(&route_north(&[]).await, &mut Hill, &map, &mut |_, _| {}).await;

    // The track stays low...
    let top = rail_bed(&world)
        .iter()
        .map(|p| p[1])
        .fold(f32::MIN, f32::max);
    assert!(top < 520.0, "the track climbs to {top}");
    // ...under the hill, which stays as it is, in a tunnel.
    let hill = ground_at(&world, 300.0, -500.0).expect("ground");
    assert!(hill > 535.0, "the hill cut down to {hill}");
    let tube = world
        .structures
        .vertices
        .iter()
        .filter(|v| (v[0] - 300.0).abs() < 6.0 && (v[2] + 500.0).abs() < 20.0)
        .count();
    assert!(tube > 10, "no tunnel under the hill");
    // The map has no tunnel here: the ground's shape found it, and it stays in the hill as it
    // is, without a portal ring facing out along the line at either end (#135, #155).
    let rings = world
        .structures
        .vertices
        .iter()
        .zip(&world.structures.normals)
        .filter(|(v, n)| (v[0] - 300.0).abs() < 6.0 && n[2].abs() > 0.99)
        .count();
    assert_eq!(rings, 0, "a portal ring on a tunnel the map does not have");
}

#[tokio::test]
async fn railway_tunnels_under_the_road_stay_below_it() {
    // Flat land 3 m below the road ridden (500 m), which crosses it on an embankment, and a line
    // passing under it at 500 m in a tunnel 240 m long: its 4.5 m arch would show through the
    // road and the embankment (#138).
    struct Low;
    impl ElevationModel for Low {
        fn elevation(
            &mut self,
            _lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Ok(497.0))
        }
    }
    let map = MapData {
        railways: vec![
            railway(&[(-300.0, 500.0), (-120.0, 500.0)], None),
            railway(
                &[(-120.0, 500.0), (120.0, 500.0)],
                Some(StructureKind::Tunnel),
            ),
            railway(&[(120.0, 500.0), (300.0, 500.0)], None),
        ],
        ..MapData::default()
    };
    let world = generate(&route_north(&[]).await, &mut Low, &map, &mut |_, _| {}).await;

    let cover = ground_at(&world, 1.0, -500.0).expect("ground cover above the railway tube");
    assert!(
        cover > 499.0,
        "clipping the tunnel must retain its compressed roof's ground cover"
    );

    let tube: Vec<_> = world
        .structures
        .vertices
        .iter()
        .filter(|v| (v[2] + 500.0).abs() < 8.0)
        .collect();
    // Nothing of it on or beside the road...
    let through = tube
        .iter()
        .filter(|v| v[0].abs() < 8.0)
        .map(|v| v[1])
        .fold(f32::MIN, f32::max);
    assert!(
        through < 500.0 - 0.1,
        "the tunnel reaches {through} at the road"
    );
    // ...while away from it the arch keeps its height.
    let arch = tube
        .iter()
        .filter(|v| v[0].abs() > 80.0)
        .map(|v| v[1])
        .fold(f32::MIN, f32::max);
    assert!(
        arch > 497.0 + 4.0,
        "the arch only reaches {arch} away from the road"
    );
}

#[tokio::test]
async fn a_railway_bridge_mapped_on_its_own_is_kept() {
    // #116: a bridge between switches, joining no line of its own; with no track on the
    // ground to take its height from, it was left out. Its ends stand on the hillside, here
    // at 520 m.
    let world = world(&MapData {
        railways: vec![railway(
            &[(200.0, 300.0), (200.0, 500.0)],
            Some(StructureKind::Bridge),
        )],
        ..MapData::default()
    })
    .await;

    let bed = rail_bed(&world);
    assert!(bed.len() > 10, "{} points of track", bed.len());
    for point in &bed {
        assert!((point[1] - 520.0).abs() < 0.1, "the track at {point:?}");
    }
}

#[tokio::test]
async fn a_railway_tunnel_on_its_own_is_left_out() {
    // #116: a tunnel joining no line of its own tells nothing about where its ends lie: cut
    // at a tile border, an end is deep in the hill (the Gotthard tunnel above Andermatt), and
    // a track laid from there to the ground would hang in the air across the valley. It is
    // left out, as it was.
    let world = world(&MapData {
        railways: vec![railway(
            &[(200.0, 300.0), (200.0, 500.0)],
            Some(StructureKind::Tunnel),
        )],
        ..MapData::default()
    })
    .await;

    assert!(rail_bed(&world).is_empty(), "the tunnel is laid out");
}

#[tokio::test]
async fn railway_bridges_clear_the_road_and_meet_their_track() {
    // A line crossing the route at 500 m, on a bridge 60 m long over it.
    let map = MapData {
        railways: vec![
            railway(&[(-300.0, 500.0), (-30.0, 500.0)], None),
            railway(
                &[(-30.0, 500.0), (30.0, 500.0)],
                Some(StructureKind::Bridge),
            ),
            railway(&[(30.0, 500.0), (300.0, 500.0)], None),
        ],
        ..MapData::default()
    };
    let world = world(&map).await;

    let bed = rail_bed(&world);
    let at_x = |x: f32| {
        bed.iter()
            .min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
            .map(|p| p[1])
            .expect("track")
    };
    // Over the road (500 m), high enough to pass under.
    assert!(
        at_x(0.0) > 500.0 + 5.5,
        "the bridge at {} over the road",
        at_x(0.0)
    );
    // One line: no step anywhere, the approaches ramping up gently.
    for pair in bed.windows(2) {
        let run = (pair[1][0] - pair[0][0]).abs().max(1.0);
        let grade = (pair[1][1] - pair[0][1]).abs() / run;
        assert!(grade < 0.041, "a {grade} step at {:?}", pair[0]);
    }
    // The bridge is a structure, standing on the ground beside the road.
    let deck = world
        .structures
        .vertices
        .iter()
        .filter(|v| v[0].abs() < 30.0 && (v[2] + 500.0).abs() < 5.0)
        .count();
    assert!(deck > 10, "no bridge over the road");
    assert!(
        world
            .structures
            .vertices
            .iter()
            .filter(|v| v[0].abs() < 25.0)
            .all(|v| (v[2] + 500.0).abs() < 2.5),
        "a single-track bridge fits its ballast and a maintenance walkway"
    );
    assert_valid(&world.structures);
    assert_faces_follow_normals(&world.structures);
}

/// The triangles of a chunked mesh in absolute coordinates.
fn chunk_triangles<'a>(
    world: &'a World,
    mesh: impl Fn(&'a TerrainChunk) -> &'a MeshData + 'a,
) -> impl Iterator<Item = [[f32; 3]; 3]> + 'a {
    world.chunks.iter().flat_map(move |chunk| {
        triangles(mesh(chunk))
            .map(|t| t.map(|v| [v[0] + chunk.center[0], v[1], v[2] + chunk.center[2]]))
    })
}

#[tokio::test]
async fn bridges_over_the_road_keep_it_clear() {
    // The road in a valley, shallow but for a deep stretch at 800 m; crossing it, a short, low
    // railway bridge at 300 m whose arches' pier would stand on it, a long one at 600 m whose
    // pier would, and at 800 m a street bridge whose pier would (#98).
    struct Valley;
    impl ElevationModel for Valley {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            let slope = if (north - 800.0).abs() < 60.0 {
                0.5
            } else {
                0.15
            };
            std::future::ready(Ok(500.0 + slope * east.abs()))
        }
    }
    let line = |from: f64, to: f64, north: f64| {
        vec![
            railway(&[(-300.0, north), (from, north)], None),
            railway(&[(from, north), (to, north)], Some(StructureKind::Bridge)),
            railway(&[(to, north), (300.0, north)], None),
        ]
    };
    let map = MapData {
        railways: [line(-13.0, 27.0, 300.0), line(-75.0, 45.0, 600.0)].concat(),
        roads: vec![orqwa_osm::Road {
            class: orqwa_osm::RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(-30.0, 800.0), at(20.0, 800.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route_north(&[]).await, &mut Valley, &map, &mut |_, _| {}).await;

    // Nothing reaches down onto the road: no pier, wall or arch.
    let on_road = |t: &[[f32; 3]; 3]| {
        let middle = (t[0][0] + t[1][0] + t[2][0]) / 3.0;
        middle.abs() < ROAD_HALF_WIDTH as f32 && t.iter().any(|v| v[1] < 502.0)
    };
    let structures: Vec<_> = triangles(&world.structures).collect();
    for north in [300.0_f32, 600.0] {
        let bridge: Vec<_> = structures
            .iter()
            .filter(|t| (t[0][2] + north).abs() < 10.0)
            .collect();
        assert!(bridge.len() > 20, "no bridge at {north} m");
        assert!(
            !bridge.iter().any(|t| on_road(t)),
            "the bridge at {north} m stands on the road"
        );
        // It still stands on piers either side.
        assert!(
            bridge.iter().any(|t| t.iter().any(|v| v[1] < 502.0)),
            "the bridge at {north} m has no piers"
        );
    }
    let street_piers: Vec<_> = chunk_triangles(&world, |c| &c.streets)
        .filter(|t| (t[0][2] + 800.0).abs() < 10.0 && t.iter().any(|v| v[1] < 502.0))
        .collect();
    assert!(!street_piers.is_empty(), "the street bridge has no piers");
    assert!(
        !street_piers.iter().any(on_road),
        "the street bridge stands on the road"
    );
}

#[tokio::test]
async fn parallel_tracks_enter_a_tunnel_through_one_portal() {
    // Two tracks 4.5 m apart under a hill whose edge runs slantwise across them, so the hill
    // covers the eastern track 45 m later than the western one. Both tubes must begin where
    // the hill covers both: the stretch the eastern track would open onto the ground beside
    // the western track's tube would bare it (#135, #99).
    struct Slant;
    impl ElevationModel for Slant {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            let edge = 400.0 + (east - 300.0) * 10.0;
            std::future::ready(Ok(if north > edge { 540.0 } else { 500.0 }))
        }
    }
    let map = MapData {
        railways: vec![
            railway(
                &[(300.0, 300.0), (300.0, 700.0)],
                Some(StructureKind::Tunnel),
            ),
            railway(
                &[(304.5, 300.0), (304.5, 700.0)],
                Some(StructureKind::Tunnel),
            ),
            railway(&[(300.0, 100.0), (300.0, 300.0)], None),
            railway(&[(304.5, 100.0), (304.5, 300.0)], None),
            railway(&[(300.0, 700.0), (300.0, 900.0)], None),
            railway(&[(304.5, 700.0), (304.5, 900.0)], None),
        ],
        ..MapData::default()
    };
    let world = generate(&route_north(&[]).await, &mut Slant, &map, &mut |_, _| {}).await;

    let inside = palette::srgb("structure.tunnel", 0.0);
    let entrance = |x: f32| {
        world
            .structures
            .vertices
            .iter()
            .zip(&world.structures.colors)
            .filter(|(v, colour)| **colour == inside && (v[0] - x).abs() < 2.0)
            .map(|(v, _)| -v[2])
            .fold(f32::MAX, f32::min)
    };
    let (west, east) = (entrance(300.0), entrance(304.5));
    assert!(west < 1e6 && east < 1e6, "no tubes: {west} {east}");
    assert!(
        (west - east).abs() < 3.0,
        "the tubes begin apart: west at {west} m, east at {east} m"
    );
    assert!(
        east > 420.0,
        "the eastern tube begins at {east} m, before the hill covers it"
    );
}

#[tokio::test]
async fn parallel_tracks_share_one_tunnel_and_one_bridge() {
    // Two tracks 4.5 m apart, mapped each on its own, through a hill at 300 m and over a
    // bridge across a valley at 700 m (#99).
    struct HillAndValley;
    impl ElevationModel for HillAndValley {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            let hill = (40.0 - 0.3 * (north - 300.0).abs()).max(0.0);
            let valley = (15.0 - 0.3 * (north - 700.0).abs()).max(0.0);
            std::future::ready(Ok(500.0 + hill - valley))
        }
    }
    let track = |east: f64| {
        vec![
            railway(&[(east, 0.0), (east, 650.0)], None),
            railway(&[(east, 650.0), (east, 750.0)], Some(StructureKind::Bridge)),
            railway(&[(east, 750.0), (east, 1000.0)], None),
        ]
    };
    let map = MapData {
        railways: [track(300.0), track(304.5)].concat(),
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut HillAndValley,
        &map,
        &mut |_, _| {},
    )
    .await;

    // One height: the tracks lie level with each other.
    let bed = rail_bed(&world);
    let height = |east: f32, north: f32| {
        bed.iter()
            .filter(|p| (p[0] - east).abs() < 1.0)
            .min_by(|a, b| (a[2] + north).abs().total_cmp(&(b[2] + north).abs()))
            .map(|p| p[1])
            .expect("track")
    };
    for k in 1..20 {
        let north = k as f32 * 50.0;
        let (a, b) = (height(300.0, north), height(304.5, north));
        assert!(
            (a - b).abs() < 0.05,
            "at {north} m the tracks lie at {a} and {b}"
        );
    }
    // A tunnel and a bridge, neither standing on a track: no wall or parapet between them
    // (the tunnel's floor, level under both, is no wall).
    let structures: Vec<_> = triangles(&world.structures).collect();
    for north in [300.0_f32, 700.0] {
        let here: Vec<_> = structures
            .iter()
            .filter(|t| (t[0][2] + north).abs() < 20.0)
            .collect();
        assert!(here.len() > 20, "no structure at {north} m");
        let track = height(300.0, north);
        let on_a_track = here.iter().any(|t| {
            let (u, v) = (
                [0, 1, 2].map(|k| t[1][k] - t[0][k]),
                [0, 1, 2].map(|k| t[2][k] - t[0][k]),
            );
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
            if length > 0.0 && (normal[1] / length).abs() > 0.9 {
                return false;
            }
            let middle = [0, 1, 2].map(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
            let beside = |east: f32| (middle[0] - east).abs() < railways::BED_M as f32 / 2.0 + 0.3;
            (beside(300.0) || beside(304.5)) && (track - 0.5..track + 3.0).contains(&middle[1])
        });
        assert!(!on_a_track, "the structure at {north} m stands on a track");
    }
}

#[tokio::test]
async fn the_road_bevels_gently_down_to_a_level_verge() {
    let world = world(&MapData::default()).await;

    let near_road: Vec<_> = terrain_vertices(&world)
        .filter(|v| v[0].abs() <= 6.5 && (-1000.0..0.0).contains(&v[2]))
        .collect();
    assert!(near_road.len() > 100);
    for v in near_road {
        assert!((v[1] - (500.0 - ROAD_SINK as f32)).abs() < 0.01, "{v:?}");
        // Just below the road: a low step, never a wall.
        assert!(500.0 - v[1] < 0.16, "{v:?}");
    }
    // The road's surface is at the route's 500 m; from its edges a bevel runs down, gentler
    // than 45°, to below the verge.
    for ring in world.road.vertices.as_chunks::<6>().0 {
        let (bevel, edge) = (ring[1], ring[2]);
        assert!((ring[2][1] - 500.0).abs() < 0.01 && (ring[3][1] - 500.0).abs() < 0.01);
        let drop = edge[1] - bevel[1];
        let out = (edge[0] - bevel[0]).hypot(edge[2] - bevel[2]);
        assert!(
            drop > ROAD_SINK as f32,
            "the bevel ends above the verge: {drop}"
        );
        assert!(
            drop < out,
            "a bevel steeper than 45°: {drop} m over {out} m"
        );
    }
}

/// The height of the topmost ground at (`x`, `z`): roundabouts' islands lie over the plain
/// ground.
fn top_of_ground(world: &World, x: f32, z: f32) -> Option<f32> {
    let chunk = world.chunks.iter().find(|c| {
        (x - c.center[0]).abs() <= CHUNK_SIZE as f32 / 2.0
            && (z - c.center[2]).abs() <= CHUNK_SIZE as f32 / 2.0
    })?;
    let (px, pz) = (x - chunk.center[0], z - chunk.center[2]);
    triangles(&chunk.mesh)
        .filter_map(|[a, b, c]| {
            let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if det.abs() < 1e-9 {
                return None;
            }
            let wa = ((b[2] - c[2]) * (px - c[0]) + (c[0] - b[0]) * (pz - c[2])) / det;
            let wb = ((c[2] - a[2]) * (px - c[0]) + (a[0] - c[0]) * (pz - c[2])) / det;
            let wc = 1.0 - wa - wb;
            (wa >= -1e-4 && wb >= -1e-4 && wc >= -1e-4).then(|| wa * a[1] + wb * b[1] + wc * c[1])
        })
        .reduce(f32::max)
}

/// The terrain's height at (`x`, `z`) as its mesh has it.
fn ground_at(world: &World, x: f32, z: f32) -> Option<f32> {
    height_on(world, |c| &c.mesh, x, z)
}

/// The height at (`x`, `z`) of the other streets, as their meshes have it.
fn street_at(world: &World, x: f32, z: f32) -> Option<f32> {
    height_on(world, |c| &c.streets, x, z)
}

/// The height at (`x`, `z`) of a chunk mesh `pick` chooses.
fn height_on(
    world: &World,
    pick: fn(&crate::TerrainChunk) -> &MeshData,
    x: f32,
    z: f32,
) -> Option<f32> {
    let chunk = world.chunks.iter().find(|c| {
        (x - c.center[0]).abs() <= CHUNK_SIZE as f32 / 2.0
            && (z - c.center[2]).abs() <= CHUNK_SIZE as f32 / 2.0
    })?;
    let (px, pz) = (x - chunk.center[0], z - chunk.center[2]);
    triangles(pick(chunk)).find_map(|t| triangle_height(t, px, pz))
}

/// Height where a vertical ray through (`x`, `z`) meets a triangle.
fn triangle_height([a, b, c]: [[f32; 3]; 3], point_x: f32, point_z: f32) -> Option<f32> {
    let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if det.abs() < 1e-9 {
        return None;
    }
    let wa = ((b[2] - c[2]) * (point_x - c[0]) + (c[0] - b[0]) * (point_z - c[2])) / det;
    let wb = ((c[2] - a[2]) * (point_x - c[0]) + (a[0] - c[0]) * (point_z - c[2])) / det;
    let wc = 1.0 - wa - wb;
    (wa >= -1e-4 && wb >= -1e-4 && wc >= -1e-4).then(|| wa * a[1] + wb * b[1] + wc * c[1])
}

#[tokio::test]
async fn riders_ride_on_the_road_drawn_through_bends() {
    // North, a hairpin of 15 m radius, back south: the road is a smooth curve there.
    let mut xml = String::from("<gpx><trk><trkseg>");
    let mut points: Vec<(f64, f64)> = (0..=10).map(|i| (0.0, f64::from(i) * 10.0)).collect();
    points.extend((1..12).map(|k| {
        let angle = std::f64::consts::PI * f64::from(k) / 12.0;
        (15.0 - 15.0 * angle.cos(), 100.0 + 15.0 * angle.sin())
    }));
    points.extend((0..=10).rev().map(|i| (30.0, f64::from(i) * 10.0)));
    for &(east, north) in &points {
        let (lat, lon) = at(east, north);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>500</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let route = Route::from_gpx(&xml, None).await.unwrap();
    let projection = LocalProjection::for_route(&route);
    let road = road::RoadIndex::new(&route, &projection);

    let length = route.length().0;
    let mut along = route.length();
    along.0 = 0.0;
    while along.0 < length {
        let rider = route.position(along);
        let (east, north) = projection.project(rider.lat, rider.lon);
        let (off, _, _) = road.nearest(east, north, 5.0).expect("the road nearby");
        // The road is drawn in straight pieces of a few metres along the curve.
        assert!(off < 0.05, "{off} m off the road's middle at {} m", along.0);
        along.0 += 0.5;
    }
}

#[tokio::test]
async fn turning_back_the_same_way_the_road_ends_round() {
    // 100 m north and straight back (#101).
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in (0..=10).chain((0..10).rev()) {
        let (lat, lon) = at(0.0, f64::from(i) * 10.0);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>500</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let route = Route::from_gpx(&xml, None).await.unwrap();
    let projection = LocalProjection::for_route(&route);
    let mesh = road::RoadIndex::new(&route, &projection).mesh(ROAD_HALF_WIDTH, &[]);
    assert_valid(&mesh);

    // The road's surface reaches round the turning point as far as its half width, rather than
    // to a point straight ahead.
    let surface: Vec<[[f32; 3]; 3]> = triangles(&mesh)
        .filter(|t| t.iter().all(|v| (v[1] - 500.0).abs() < 0.01))
        .collect();
    let layers = |east: f32, north: f32| {
        surface
            .iter()
            .filter(|t| {
                let side = |a: [f32; 3], b: [f32; 3]| {
                    (b[0] - a[0]) * (-north - a[2]) - (b[2] - a[2]) * (east - a[0])
                };
                let sides = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
                sides.iter().all(|&s| s >= 0.0) || sides.iter().all(|&s| s <= 0.0)
            })
            .count()
    };
    let reach = 0.8 * ROAD_HALF_WIDTH as f32;
    for degrees in (-80..=80).step_by(10) {
        let angle = (degrees as f32).to_radians();
        let (east, north) = (reach * angle.sin(), 100.0 + reach * angle.cos());
        assert!(layers(east, north) > 0, "no road at {east:.1}, {north:.1}");
    }
    // Ridden twice, the road is drawn once: its markings are not doubled.
    assert_eq!(layers(1.1, 50.3), 1);
}

#[tokio::test]
async fn no_ground_covers_the_road_on_a_hillside_with_a_hairpin() {
    // Terrain rising 30 % to the east; the road climbs north along it, turns in a hairpin of
    // 15 m radius and comes back 30 m further up the slope, cut into the hillside.
    struct Hillside;
    impl ElevationModel for Hillside {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(500.0 + 0.3 * east))
        }
    }
    let mut points = Vec::new();
    for i in 0..=30 {
        points.push((0.0, f64::from(i) * 10.0));
    }
    for k in 1..12 {
        let angle = std::f64::consts::PI * f64::from(k) / 12.0;
        points.push((15.0 - 15.0 * angle.cos(), 300.0 + 15.0 * angle.sin()));
    }
    for i in (0..=30).rev() {
        points.push((30.0, f64::from(i) * 10.0));
    }
    let mut xml = String::from("<gpx><trk><trkseg>");
    for &(east, north) in &points {
        let (lat, lon) = at(east, north);
        let elevation = 500.0 + 0.3 * east;
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{elevation}</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let route = Route::from_gpx_with::<Hillside>(&xml, None, &MapData::default())
        .await
        .unwrap();
    let world = generate(&route, &mut Hillside, &MapData::default(), &mut |_, _| {}).await;

    // Every edge of the road, and its middle, lies above the ground there.
    let road = &world.road.vertices;
    let mut checked = 0;
    for section in road.as_chunks::<6>().0 {
        let (left, right) = (section[2], section[3]);
        let middle = [0, 1, 2].map(|k| f32::midpoint(left[k], right[k]));
        for point in [left, middle, right] {
            let ground = ground_at(&world, point[0], point[2]).expect("ground under the road");
            assert!(
                ground <= point[1] + 0.01,
                "ground {ground} over the road at {point:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 300);
    // The hillside above the upper leg is natural again further up.
    let up = ground_at(&world, 90.0, -150.0).unwrap();
    assert!((up - (500.0 + 0.3 * 90.0)).abs() < 0.5, "{up}");
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
    // A forest across the road; open meadow around it.
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

    let trees = plants_of(&world, &["conifer", "broadleaf"]);
    let in_forest = |[x, _, z]: &[f32; 3]| (-610.0..=-390.0).contains(z) && x.abs() <= 110.0;
    let forest_trees = trees.iter().filter(|t| in_forest(t)).count();
    // At least one trunk per 25 m² makes the crowns read as continuous woodland (R103).
    assert!(forest_trees > 1600, "{forest_trees} trees in the forest");
    // Outside it, in the meadows within 300 m of the road, only now and then a tree on its own.
    let lone = (trees.len() - forest_trees) as f64;
    let forest_density = forest_trees as f64 / (200.0 * 200.0);
    let meadow_density = lone / (1600.0 * 600.0 - 200.0 * 200.0);
    assert!(
        meadow_density < forest_density / 20.0,
        "{lone} trees outside the forest"
    );
    for [x, y, z] in &trees {
        // Past the road's ends the open ground is free.
        let beside = (0.0..=1000.0).contains(&-z);
        assert!(!beside || x.abs() >= 8.0, "tree on the road at {x}");
        // Beyond the ground levelled for the road, trees stand on the natural terrain.
        assert!(
            x.abs() < LEVEL_REACH as f32 || (y - (500.0 + 0.1 * x)).abs() < 1.0,
            "tree not on the ground: {y}"
        );
    }
    let colours: BTreeSet<[u32; 3]> = world
        .chunks
        .iter()
        .flat_map(|c| c.trees.models.values())
        .flat_map(|buffer| buffer.as_chunks::<16>().0.iter())
        .map(|plant| [plant[12], plant[13], plant[14]].map(f32::to_bits))
        .collect();
    assert!(colours.len() > 3, "trees vary in colour: {colours:?}");
}

#[tokio::test]
async fn distant_forests_still_have_a_canopy_and_keep_their_clearings() {
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(550.0, 500.0, 100.0)],
        inner: vec![square(550.0, 500.0, 30.0)],
    };
    let world = world(&MapData {
        areas: vec![forest],
        ..MapData::default()
    })
    .await;
    let trees = plants_of(&world, &["conifer", "broadleaf"]);
    let distant: Vec<_> = trees
        .iter()
        .filter(|[x, _, z]| (450.0..650.0).contains(x) && (-600.0..-400.0).contains(z))
        .collect();
    // Even beyond the detailed roadside band, at least one trunk per 150 m² (R103).
    assert!(distant.len() > 240, "{} distant trees", distant.len());
    assert!(
        distant
            .iter()
            .all(|[x, _, z]| { !(520.0..580.0).contains(x) || !(-530.0..-470.0).contains(z) })
    );
}

#[tokio::test]
async fn mapped_forests_stay_wooded_on_steep_hillsides() {
    struct SteepSlope;
    impl ElevationModel for SteepSlope {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(500.0 + east))
        }
    }
    let map = MapData {
        areas: vec![Area {
            cover: LandCover::Forest,
            outer: vec![square(150.0, 500.0, 100.0)],
            inner: vec![],
        }],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut SteepSlope,
        &map,
        &mut |_, _| {},
    )
    .await;
    let trees = plants_of(&world, &["conifer", "broadleaf"]);
    let wooded_slope = trees
        .iter()
        .filter(|[x, _, z]| (50.0..250.0).contains(x) && (-600.0..-400.0).contains(z))
        .count();
    assert!(
        wooded_slope > 1600,
        "{wooded_slope} trees on the steep forest"
    );
}

#[tokio::test]
async fn ferns_grow_on_the_nearby_forest_floor_and_keep_clear_of_obstacles() {
    let map = MapData {
        areas: vec![
            Area {
                cover: LandCover::Forest,
                outer: vec![square(0.0, 500.0, 100.0)],
                inner: vec![square(30.0, 530.0, 15.0)],
            },
            Area {
                cover: LandCover::Water,
                outer: vec![square(-30.0, 470.0, 15.0)],
                inner: vec![],
            },
            Area {
                cover: LandCover::Forest,
                outer: vec![square(550.0, 500.0, 100.0)],
                inner: vec![],
            },
        ],
        buildings: vec![Building {
            id: 104,
            outline: rectangle(30.0, 470.0, 8.0, 8.0),
            height: None,
            levels: Some(1.0),
            color: None,
        }],
        roads: vec![orqwa_osm::Road {
            class: orqwa_osm::RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(-100.0, 450.0), at(100.0, 450.0)],
            structure: None,
        }],
        railways: vec![railway(&[(-100.0, 550.0), (100.0, 550.0)], None)],
        ..MapData::default()
    };
    let world = world(&map).await;
    let ferns = plants_of(&world, &["fern"]);
    assert!(ferns.len() > 10, "{} woodland ferns", ferns.len());
    for [x, y, z] in &ferns {
        assert!(
            (4.5..60.0).contains(&x.abs()),
            "fern too far away or on the road: {x}"
        );
        assert!((-600.0..-400.0).contains(z), "fern outside the woods: {z}");
        assert!(
            (x - 30.0).abs() >= 15.0 || (z + 530.0).abs() >= 15.0,
            "fern in a clearing"
        );
        assert!(
            (x + 30.0).abs() >= 15.0 || (z + 470.0).abs() >= 15.0,
            "fern in water"
        );
        assert!(
            (x - 30.0).abs() >= 8.0 || (z + 470.0).abs() >= 8.0,
            "fern in a building"
        );
        assert!((z + 450.0).abs() > 3.0, "fern on a street");
        assert!((z + 550.0).abs() > 1.6, "fern on a railway");
        let ground = ground_at(&world, *x, *z).expect("ground below the fern");
        assert!(
            (y - ground).abs() < 1.0,
            "fern at {x}, {z}: base {y}, ground {ground}"
        );
    }
    for name in ["fern_spreading", "fern_upright"] {
        assert!(
            world
                .chunks
                .iter()
                .any(|chunk| chunk.trees.models.contains_key(name)),
            "no {name}"
        );
    }
}

#[tokio::test]
async fn rocks_lie_on_rocky_ground_off_the_road_and_out_of_buildings() {
    // Scree beside the road, a house on it; the gentle slope elsewhere has no rocks.
    let scree = Area {
        cover: LandCover::Rock,
        outer: vec![square(60.0, 400.0, 50.0)],
        inner: vec![],
    };
    let house = Building {
        id: 7,
        outline: rectangle(60.0, 400.0, 6.0, 5.0),
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let world = world(&MapData {
        areas: vec![scree],
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    let rocks = plants_of(&world, &["rock"]);
    assert!(rocks.len() > 10, "{} rocks", rocks.len());
    for [x, _, z] in &rocks {
        assert!(
            (5.0..=115.0).contains(x) && (-455.0..=-345.0).contains(z),
            "rock off the scree at {x}, {z}"
        );
        assert!(
            (x - 60.0).hypot(z + 400.0) > 7.0,
            "rock in the house at {x}, {z}"
        );
    }
    for [x, _, z] in plants_of(&world, &["bush", "broadleaf", "conifer"]) {
        let beside = (0.0..=1000.0).contains(&-z);
        assert!(!beside || x.abs() >= 5.0, "plant on the road at {x}, {z}");
    }
}

/// Positions (absolute) of the plants of the given model kinds.
fn plants_of(world: &World, kinds: &[&str]) -> Vec<[f32; 3]> {
    world
        .chunks
        .iter()
        .flat_map(|c| {
            c.trees
                .models
                .iter()
                .filter(|(name, _)| {
                    vegetation::kind_of(name).is_some_and(|kind| kinds.contains(&kind))
                })
                .flat_map(move |(_, buffer)| {
                    buffer
                        .as_chunks::<16>()
                        .0
                        .iter()
                        .map(move |t| [t[3] + c.center[0], t[7], t[11] + c.center[2]])
                })
        })
        .collect()
}

#[tokio::test]
async fn grass_and_flowers_line_the_road_but_not_lakes_or_the_road_itself() {
    // A pond beside the road at 300 m, a forest beside it at 700 m; open ground elsewhere.
    let pond = Area {
        cover: LandCover::Water,
        outer: vec![square(15.0, 300.0, 8.0)],
        inner: vec![],
    };
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(15.0, 700.0, 60.0)],
        inner: vec![],
    };
    let world = world(&MapData {
        areas: vec![pond, forest],
        ..MapData::default()
    })
    .await;
    let positions = |pick: fn(&crate::Trees) -> &Vec<f32>| -> Vec<[f32; 2]> {
        world
            .chunks
            .iter()
            .flat_map(|c| {
                pick(&c.trees)
                    .as_chunks::<12>()
                    .0
                    .iter()
                    .map(move |t| [t[3] + c.center[0], t[11] + c.center[2]])
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let grass = positions(|t| &t.grass);
    let flowers = positions(|t| &t.flowers);

    assert!(grass.len() > 1000, "{} tufts", grass.len());
    assert_ne!(flowers.len(), 0);
    for [x, z] in grass.iter().chain(&flowers) {
        // Beside the road (it runs north for 1 km); past its ends the distance is to its end.
        let beside = (0.0..=1000.0).contains(&-z);
        assert!(x.abs() <= 30.0, "plant {x} m from the road");
        assert!(!beside || x.abs() >= 3.6, "plant on the road at {x}, {z}");
        let in_pond = (x - 15.0).abs() < 8.0 && (z + 300.0).abs() < 8.0;
        assert!(!in_pond, "grass in the pond at {x}, {z}");
    }
    // The forest floor is sparse next to open ground of the same size.
    let count = |north: f32| {
        grass
            .iter()
            .filter(|[x, z]| *x > 3.6 && *x < 30.0 && (z + north).abs() < 50.0)
            .count()
    };
    assert!(
        count(700.0) * 2 < count(500.0),
        "{} vs {}",
        count(700.0),
        count(500.0)
    );
}

#[tokio::test]
async fn shallow_intersections_reach_the_road_in_both_directions_and_across_chunks() {
    let mut roads = vec![road_north()];
    for (side, join) in [(-1.0, 480.0), (1.0, 760.0)] {
        let mut branch = road_north();
        branch.details.kind = orqwa_osm::RoadKind::Local;
        branch.line = vec![at(0.0, join), at(side * 60.0, join + side * 150.0)];
        if side < 0.0 {
            branch.line.reverse();
        }
        roads.push(branch);
    }
    let world = world(&MapData {
        roads,
        ..MapData::default()
    })
    .await;

    // Twenty-two-degree branches used to disappear within the sum of the road widths.
    // Sample the actual triangles, including both sides of the 480 m chunk boundary.
    let length = 1.0_f32.hypot(2.5);
    for (side, join) in [(-1.0, 480.0), (1.0, 760.0)] {
        for step in 0..=100 {
            let off = step as f32 * 0.25;
            for across in [-1.8, 0.0, 1.8] {
                let x = side * off + across * 2.5 / length;
                let north = join + side * off * 2.5 - across / length;
                if x.abs() <= 3.05 {
                    continue;
                }
                let street = street_at(&world, x, -north)
                    .unwrap_or_else(|| panic!("junction gap at {x}, {north}"));
                let ground = ground_at(&world, x, -north).expect("ground below the street");
                assert!(street > ground, "ground through junction at {x}, {north}");
            }
        }
        let north = join + side * 2.9 * 2.5;
        assert!(
            street_at(&world, side * 2.9, -north).is_some(),
            "the joining segment extends under the ridden road"
        );
        assert!(
            world.road.uvs.as_chunks::<6>().0.iter().any(|ring| {
                let bevel = if side < 0.0 { ring[1] } else { ring[4] };
                (bevel[1] - (join + side * 8.0)).abs() < 1.0 && (0.0..=1.0).contains(&bevel[0])
            }),
            "the gravel shoulder must open at a shallow junction"
        );
    }
}

#[tokio::test]
async fn an_offset_map_centre_line_does_not_paint_a_second_road_down_the_bank() {
    struct Cliff;
    impl ElevationModel for Cliff {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46_f64.to_radians().cos();
            std::future::ready(Ok(500.0 - 3.0 * east))
        }
    }
    let route = route_north(&[]).await;
    for (offset, split) in [(0.4, false), (2.0, true)] {
        let mut mapped = road_north();
        mapped.class = RoadClass::Major;
        mapped.line = vec![at(offset, 100.0), at(offset, 900.0)];
        let mut roads = vec![mapped.clone()];
        if split {
            // Tile pieces become one street chain, but retain both original feature IDs.
            roads[0].line[1] = at(offset, 480.0);
            mapped.line[0] = at(offset, 480.0);
            roads.push(mapped);
        }
        let world = generate(
            &route,
            &mut Cliff,
            &MapData {
                roads,
                ..MapData::default()
            },
            &mut |_, _| {},
        )
        .await;
        assert!(
            world.chunks.iter().all(|c| c.streets.indices.is_empty()),
            "the same marked carriageway must not also drape down its bank at offset {offset}"
        );
    }
}

#[tokio::test]
async fn a_narrow_ridden_cycleway_does_not_claim_a_wider_neighbouring_highway() {
    let mut cycleway = road_north();
    cycleway.class = RoadClass::Path;
    cycleway.details.kind = orqwa_osm::RoadKind::Cycleway;
    cycleway.details.surface = orqwa_osm::RoadSurface::Paved;
    let mut highway = road_north();
    highway.class = RoadClass::Major;
    highway.line = vec![at(2.0, 100.0), at(2.0, 900.0)];
    let world = world(&MapData {
        roads: vec![cycleway, highway],
        ..MapData::default()
    })
    .await;
    for north in [200.0, 479.75, 480.25, 700.0] {
        assert!(
            street_at(&world, 4.5, -north).is_some(),
            "the wider highway remains beside a narrow ridden cycleway"
        );
    }
}

#[tokio::test]
async fn a_nearby_parallel_cycleway_keeps_its_visible_surface() {
    let mut cycleway = road_north();
    cycleway.class = RoadClass::Path;
    cycleway.details.kind = orqwa_osm::RoadKind::Cycleway;
    cycleway.details.surface = orqwa_osm::RoadSurface::Paved;
    cycleway.line = vec![at(4.0, 100.0), at(4.0, 900.0)];
    let world = world(&MapData {
        roads: vec![road_north(), cycleway],
        ..MapData::default()
    })
    .await;
    for north in [200.0, 479.75, 480.25, 700.0] {
        for east in [3.5, 4.0, 4.75] {
            assert!(
                street_at(&world, east, -north).is_some(),
                "parallel cycleway removed at {east}, {north}"
            );
        }
    }
}

#[tokio::test]
async fn other_streets_join_the_road_ridden_and_lie_on_the_ground() {
    use orqwa_osm::{Road, RoadClass, StructureKind};

    let way = |class, from: (f64, f64), to: (f64, f64), structure| Road {
        class,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![at(from.0, from.1), at(to.0, to.1)],
        structure,
    };
    let world = world(&MapData {
        roads: vec![
            // The road ridden itself, as the map has it.
            way(RoadClass::Street, (0.0, 0.0), (0.0, 1000.0), None),
            // A side street crossing the route, a farm track beside it, a tunnel, a bridge.
            way(RoadClass::Street, (-200.0, 300.0), (200.0, 300.0), None),
            way(RoadClass::Track, (60.0, 100.0), (60.0, 600.0), None),
            way(
                RoadClass::Major,
                (-300.0, 800.0),
                (300.0, 800.0),
                Some(StructureKind::Tunnel),
            ),
            way(
                RoadClass::Street,
                (100.0, 900.0),
                (300.0, 900.0),
                Some(StructureKind::Bridge),
            ),
        ],
        ..MapData::default()
    })
    .await;
    let steel = palette::srgb("structure.steel", 0.0);
    let vertices = |pick: fn(&crate::TerrainChunk) -> &MeshData| -> Vec<[f32; 3]> {
        world
            .chunks
            .iter()
            .flat_map(|c| {
                let mesh = pick(c);
                mesh.vertices
                    .iter()
                    .enumerate()
                    // The road and slab follow the ground; safety rails stand above them.
                    .filter(move |(i, _)| mesh.colors.get(*i).is_none_or(|color| *color != steel))
                    .map(move |(_, v)| [v[0] + c.center[0], v[1] + c.center[1], v[2] + c.center[2]])
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let streets = vertices(|c| &c.streets);
    let tracks = vertices(|c| &c.tracks);

    assert!(streets.len() > 100, "{} street vertices", streets.len());
    assert!(tracks.len() > 100, "{} track vertices", tracks.len());
    let mut under_the_road = 0;
    for [x, y, z] in &streets {
        // The crossing street and the bridge only: not the road ridden again, not the tunnel.
        let crossing = (z + 300.0).abs() < 3.0;
        let bridge = (z + 900.0).abs() < 3.0 && (99.0..=301.0).contains(x);
        assert!(crossing || bridge, "street vertex at {x}, {z}");
        if crossing && x.abs() < 3.0 {
            // It runs on under the road, below its surface, so the junction is joined.
            under_the_road += 1;
            assert!(
                *y < 500.0 - 0.05,
                "street over the road ridden: {y} at {x}, {z}"
            );
        }
        // On the ground, which rises 0.1 m per metre east beyond the shaped ground; the
        // bridge's deck runs straight between its ends, here on the same slope.
        if x.abs() > LEVEL_REACH as f32 {
            assert!(
                (y - (500.0 + 0.1 * x)).abs() < 0.8,
                "street off the ground: {y} at {x}"
            );
        }
    }
    assert!(
        under_the_road > 0,
        "the crossing street stops short of the road"
    );
    for [x, _, _] in &tracks {
        assert!((x - 60.0).abs() < 2.0);
    }
    // Nothing grows on them.
    for chunk in &world.chunks {
        for tuft in chunk.trees.grass.as_chunks::<12>().0 {
            let (x, z) = (tuft[3] + chunk.center[0], tuft[11] + chunk.center[2]);
            assert!(
                (z + 300.0).abs() > 2.75 || x.abs() < 5.5,
                "grass on the street at {x}"
            );
            assert!(
                (x - 60.0).abs() > 1.4 || !(100.0..=600.0).contains(&-z),
                "grass on the track"
            );
        }
    }
}

/// One building surface: corners in absolute coordinates (x east, y up, z south), the stored
/// normal and the vertex colour, whose alpha is the shader style.
struct Face {
    corners: [[f32; 3]; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

impl Face {
    fn middle(&self) -> [f32; 3] {
        [0, 1, 2].map(|k| self.corners.iter().map(|c| c[k]).sum::<f32>() / 3.0)
    }

    fn style(&self) -> u8 {
        buildings::Style::code(self.color[3])
    }
}

/// The building shells of a world within `radius` metres of (east, north), modelled buildings'
/// included (their shells are drawn in the distance), checking on the way that every mesh is
/// valid and faces its normals.
fn building_faces(world: &World, (east, north): (f32, f32), radius: f32) -> Vec<Face> {
    let mut faces = Vec::new();
    for chunk in &world.chunks {
        let shells =
            std::iter::once(&chunk.buildings).chain(chunk.modelled.iter().map(|c| &c.shells));
        for mesh in shells {
            faces.extend(shell_faces(chunk, mesh, (east, north), radius));
        }
    }
    faces
}

fn shell_faces(
    chunk: &TerrainChunk,
    mesh: &MeshData,
    (east, north): (f32, f32),
    radius: f32,
) -> Vec<Face> {
    assert_valid(mesh);
    assert_faces_follow_normals(mesh);
    let mut faces = Vec::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        let corners = t.map(|k| {
            let v = mesh.vertices[k as usize];
            [v[0] + chunk.center[0], v[1], v[2] + chunk.center[2]]
        });
        let face = Face {
            corners,
            normal: mesh.normals[t[0] as usize],
            color: mesh.colors[t[0] as usize],
        };
        let middle = face.middle();
        if (middle[0] - east).hypot(-middle[2] - north) < radius {
            faces.push(face);
        }
    }
    faces
}

/// A model instance read back from a `MultiMesh` buffer, in absolute coordinates.
struct Placed {
    model: String,
    origin: [f32; 3],
    /// The model's x axis (its length, scaled) and z axis (its width, scaled).
    x: [f32; 3],
    z: [f32; 3],
    plaster: [f32; 4],
}

fn placed(world: &World) -> Vec<Placed> {
    let mut all = Vec::new();
    for chunk in &world.chunks {
        for cell in &chunk.modelled {
            assert_valid(&cell.shells);
            for (model, buffer) in &cell.models {
                assert_eq!(buffer.len() % 20, 0);
                for b in buffer.as_chunks::<20>().0 {
                    all.push(Placed {
                        model: model.clone(),
                        origin: [
                            b[3] + chunk.center[0],
                            b[7] + chunk.center[1],
                            b[11] + chunk.center[2],
                        ],
                        x: [b[0], b[4], b[8]],
                        z: [b[2], b[6], b[10]],
                        plaster: [b[12], b[13], b[14], b[15]],
                    });
                }
            }
        }
    }
    all
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn highest(faces: &[Face]) -> f32 {
    faces
        .iter()
        .flat_map(|f| f.corners)
        .map(|c| c[1])
        .fold(f32::MIN, f32::max)
}

fn lowest(faces: &[Face]) -> f32 {
    faces
        .iter()
        .flat_map(|f| f.corners)
        .map(|c| c[1])
        .fold(f32::MAX, f32::min)
}

/// Ground of the eastward slope at a point `east` metres from the road.
fn slope_at(east: f32) -> f32 {
    500.0 + 0.1 * east
}

#[tokio::test]
async fn buildings_drawn_into_the_road_are_left_out() {
    // A square house turned 45° to the road, its corner towards it clipped in the map: the
    // footprint keeps 4.2 m off the centre line, but the house is drawn on the whole square,
    // its corner 2.5 m off, on the road (#138). The same house further off stays.
    let diamond = |east: f64, clip: f64| {
        let reach = clip - east + 6.0;
        let points = [
            (clip, 500.0 - reach),
            (clip, 500.0 + reach),
            (east, 506.0),
            (east + 6.0, 500.0),
            (east, 494.0),
            (clip, 500.0 - reach),
        ];
        points.iter().map(|&(e, n)| at(e, n)).collect::<Vec<_>>()
    };
    let house = |id, outline| Building {
        id,
        outline,
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![house(1, diamond(8.5, 4.2)), house(2, diamond(30.0, 25.7))],
        ..MapData::default()
    })
    .await;

    let faces = |east: f32| building_faces(&world, (east, 500.0), 7.0).len();
    assert_eq!(faces(8.5), 0, "a house drawn on the road");
    assert!(faces(30.0) > 0, "the house off the road is gone");
}

#[tokio::test]
async fn buildings_stand_on_the_ground_with_walls_facing_out() {
    // A 10 × 10 m house of two storeys on ground rising eastwards.
    let house = Building {
        id: 42,
        outline: square(60.0, 500.0, 5.0),
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    let faces = building_faces(&world, (60.0, 500.0), 30.0);
    // From below the lowest corner, 55 m east, to a pitched roof (perhaps with a chimney)
    // over two storeys of walls from the highest corner, 65 m east.
    let eaves = slope_at(65.0) + 2.0 * 3.0 + 0.4;
    let top = highest(&faces);
    assert!((eaves + 3.0..eaves + 6.0).contains(&top), "top {top}");
    assert!((lowest(&faces) - (slope_at(55.0) - 1.0)).abs() < 0.01);

    // Upright faces along the outline (walls, gables, windows, roof edges) face out; those
    // inside it belong to the chimney.
    let mut outside = 0;
    for face in faces.iter().filter(|f| f.normal[1].abs() < 0.1) {
        let middle = face.middle();
        let offset = [middle[0] - 60.0, middle[2] + 500.0];
        if offset[0].abs().max(offset[1].abs()) < 4.9 {
            continue;
        }
        outside += 1;
        let facing = face.normal[0] * offset[0] + face.normal[2] * offset[1];
        assert!(facing > 0.0, "face at {middle:?} faces in");
    }
    assert!(outside >= 8);
}

#[tokio::test]
async fn churches_get_a_tower_and_chapels_a_turret() {
    // A 14 × 30 m church and a 7 × 12 m chapel, with their church points inside.
    let church = Building {
        id: 3,
        outline: rectangle(80.0, 300.0, 7.0, 15.0),
        height: None,
        levels: None,
        color: None,
    };
    let chapel = Building {
        id: 4,
        outline: rectangle(80.0, 700.0, 3.5, 6.0),
        ..church.clone()
    };
    let world = world(&MapData {
        buildings: vec![church, chapel],
        churches: vec![at(80.0, 302.0), at(80.0, 701.0)],
        ..MapData::default()
    })
    .await;

    // The tower rises far above the nave; its top is no wider than a tower.
    let church = building_faces(&world, (80.0, 300.0), 40.0);
    let top = highest(&church);
    assert!(top > slope_at(73.0) + 25.0, "top {top}");
    let tower_top: Vec<[f32; 3]> = church
        .iter()
        .flat_map(|f| f.corners)
        .filter(|c| c[1] > slope_at(73.0) + 20.0)
        .collect();
    let width = |k: usize| {
        let values = tower_top.iter().map(|c| c[k]);
        values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min)
    };
    assert!(
        width(0) < 9.0 && width(2) < 9.0,
        "{} × {}",
        width(0),
        width(2)
    );
    assert!(church.iter().any(|f| f.style() == 4), "church windows");

    // The chapel's highest 3 m are a turret on its ridge.
    let chapel = building_faces(&world, (80.0, 700.0), 20.0);
    let top = highest(&chapel);
    let turret: Vec<[f32; 3]> = chapel
        .iter()
        .flat_map(|f| f.corners)
        .filter(|c| c[1] > top - 3.0)
        .collect();
    assert!(turret.iter().all(|c| (c[0] - 80.0).abs() < 1.0));
    assert!(top < slope_at(76.5) + 20.0, "top {top}");
}

#[tokio::test]
async fn walls_show_whole_windows_only() {
    // A block of odd size: no wall is a whole number of window spacings long.
    let block = Building {
        id: 7,
        outline: rectangle(80.0, 500.0, 11.85, 5.65),
        height: Some(15.0),
        levels: None,
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![block],
        ..MapData::default()
    })
    .await;

    let mut walls = 0;
    for chunk in &world.chunks {
        let shells =
            std::iter::once(&chunk.buildings).chain(chunk.modelled.iter().map(|c| &c.shells));
        for mesh in shells {
            for (k, color) in mesh.colors.iter().enumerate() {
                // Windowed walls: plaster and timber every 3.2 m, churches every 4.5 m.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // style codes
                let spacing = match buildings::Style::code(color[3]) {
                    0 | 2 => 3.2,
                    4 => 4.5,
                    _ => continue,
                };
                if mesh.normals[k][1].abs() > 0.01 {
                    continue;
                }
                // Each wall spans whole windows: its ends fall on the grid.
                let windows = mesh.uvs[k][0] / spacing;
                assert!(
                    (windows - windows.round()).abs() < 1e-3,
                    "a wall ending {windows} windows along"
                );
                walls += 1;
            }
        }
    }
    assert!(walls > 8, "{walls} wall corners checked");
}

#[tokio::test]
async fn blocks_have_a_cornice_and_stacked_balconies() {
    // Six blocks of six storeys, 30 m long east-west and 12 m deep.
    let middles: Vec<(f64, f64)> = (0..6)
        .map(|k| {
            (
                80.0 + 70.0 * f64::from(k % 3),
                300.0 + 200.0 * f64::from(k / 3),
            )
        })
        .collect();
    let blocks = middles
        .iter()
        .zip(20..)
        .map(|(&(east, north), id)| Building {
            id,
            outline: rectangle(east, north, 15.0, 6.0),
            height: Some(18.0),
            levels: None,
            color: None,
        })
        .collect();
    let world = world(&MapData {
        buildings: blocks,
        ..MapData::default()
    })
    .await;

    let mut with_balconies = 0;
    for &(east, north) in &middles {
        #[allow(clippy::cast_possible_truncation)] // small coordinates
        let (east, north) = (east as f32, north as f32);
        let faces = building_faces(&world, (east, north), 25.0);
        let off_north = |f: &Face| (-f.middle()[2] - north).abs() - 6.0;
        let off_east = |f: &Face| (f.middle()[0] - east).abs() - 15.0;
        // A cornice stands a quarter metre out of every wall (string courses and balconies
        // stand out less and more).
        let cornice = faces.iter().filter(|f| {
            f.normal[1].abs() < 0.01 && (off_north(f).max(off_east(f)) - 0.25).abs() < 0.05
        });
        assert!(cornice.count() >= 8, "no cornice round the block at {east}");
        // Balconies, if any: level tops 1.2 m out of the long walls, one per storey above the
        // ground floor, 3 m apart.
        let mut tops: Vec<f32> = faces
            .iter()
            .filter(|f| f.normal[1] > 0.99 && off_north(f) > 0.5 && off_east(f) < 0.0)
            .map(|f| f.middle()[1])
            .collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.1);
        if tops.is_empty() {
            continue;
        }
        with_balconies += 1;
        assert_eq!(tops.len(), 5, "balconies at {tops:?}");
        assert!(
            tops.windows(2).all(|w| (w[1] - w[0] - 3.0).abs() < 0.01),
            "{tops:?}"
        );
    }
    assert!(
        with_balconies >= 2,
        "{with_balconies} of six blocks have balconies"
    );
}

#[tokio::test]
async fn tall_blocks_have_flat_roofs_behind_a_parapet() {
    let block = Building {
        id: 5,
        outline: rectangle(80.0, 500.0, 10.0, 15.0),
        height: Some(18.0),
        levels: None,
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![block],
        ..MapData::default()
    })
    .await;

    let faces = building_faces(&world, (80.0, 500.0), 30.0);
    // Six storeys of 3 m make the mapped 18 m above the highest corner; a parapet capped by
    // its cornice, and perhaps a stair housing, top them.
    let eaves = slope_at(90.0) + 6.0 * 3.0 + 0.4;
    let top = highest(&faces);
    assert!(
        (top - (eaves + 0.88)).abs() < 0.01 || (top - (eaves + 2.6)).abs() < 0.01,
        "top {top}"
    );
    // Nothing slopes: faces are upright or level.
    assert!(
        faces
            .iter()
            .all(|f| f.normal[1].abs() < 0.01 || f.normal[1].abs() > 0.99)
    );
}

#[tokio::test]
async fn halls_on_industrial_land_are_low_and_clad_in_metal() {
    let estate = Area {
        cover: LandCover::Industrial,
        outer: vec![square(150.0, 500.0, 100.0)],
        inner: vec![],
    };
    let hall = Building {
        id: 6,
        outline: rectangle(150.0, 500.0, 30.0, 15.0),
        height: None,
        levels: None,
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![hall],
        areas: vec![estate],
        ..MapData::default()
    })
    .await;

    let faces = building_faces(&world, (150.0, 500.0), 50.0);
    let height = highest(&faces) - slope_at(180.0);
    assert!((6.0..=12.5).contains(&height), "{height} m");
    let walls = faces.iter().filter(|f| f.normal[1].abs() < 0.1);
    assert!(walls.clone().any(|f| f.style() == 5), "metal cladding");
    assert!(walls.clone().all(|f| f.style() != 0), "no house windows");
}

#[tokio::test]
async fn offices_hotels_and_public_buildings_come_from_the_map() {
    // Long sides north–south, so the eastward slope falls little across them.
    let building = |id: i64, east: f64, north: f64, half_east: f64, half_north: f64| Building {
        id,
        outline: rectangle(east, north, half_east, half_north),
        height: None,
        levels: None,
        color: None,
    };
    let land = |cover: LandCover, east: f64, north: f64| Area {
        cover,
        outer: vec![square(east, north, 45.0)],
        inner: vec![],
    };
    let world = world(&MapData {
        buildings: vec![
            // On commercial land.
            building(1, 200.0, 300.0, 8.0, 15.0),
            // A hotel and a town hall marked by their points.
            building(2, -150.0, 450.0, 6.5, 12.0),
            building(3, -150.0, 600.0, 7.0, 13.0),
            // On a school's grounds.
            building(4, 200.0, 700.0, 8.0, 19.0),
            // A guest house in a family house stays a house.
            building(5, -150.0, 800.0, 5.0, 6.0),
        ],
        areas: vec![
            land(LandCover::Commercial, 200.0, 300.0),
            land(LandCover::Public, 200.0, 700.0),
        ],
        hotels: vec![at(-150.0, 450.0), at(-151.0, 805.0)],
        public: vec![at(-152.0, 600.0)],
        ..MapData::default()
    })
    .await;

    let placed = placed(&world);
    let model_at = |east: f32, north: f32| {
        placed
            .iter()
            .find(|p| (p.origin[0] - east).hypot(-p.origin[2] - north) < 3.0)
            .map(|p| p.model.clone())
            .unwrap_or_default()
    };
    for (east, north, kind) in [
        (200.0, 300.0, "office"),
        (-150.0, 450.0, "hotel"),
        (-150.0, 600.0, "public"),
        (200.0, 700.0, "public"),
        (-150.0, 800.0, "house"),
    ] {
        let model = model_at(east, north);
        assert!(
            model.starts_with(kind),
            "{model} at {east}, {north}, not {kind}"
        );
    }
}

#[tokio::test]
async fn chalets_in_the_mountains_have_timber_walls_and_deep_eaves() {
    struct Mountains;
    impl ElevationModel for Mountains {
        fn elevation(
            &mut self,
            _lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Ok(1300.0))
        }
    }
    // A 12 × 10 m house at 1300 m.
    let house = Building {
        id: 9,
        outline: rectangle(80.0, 500.0, 6.0, 5.0),
        height: None,
        levels: None,
        color: None,
    };
    let map = MapData {
        buildings: vec![house],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Mountains,
        &map,
        &mut |_, _| {},
    )
    .await;

    let faces = building_faces(&world, (80.0, 500.0), 30.0);
    // Timber above a plastered ground floor.
    assert!(faces.iter().any(|f| f.style() == 2), "timber");
    assert!(faces.iter().any(|f| f.style() == 0), "plaster");
    // A shallow roof reaching well beyond the walls all round.
    let sloped = faces
        .iter()
        .filter(|f| (0.1..0.99).contains(&f.normal[1].abs()));
    assert!(sloped.clone().count() >= 4);
    assert!(sloped.clone().all(|f| f.normal[1].abs() > 0.85));
    let corners: Vec<[f32; 3]> = faces.iter().flat_map(|f| f.corners).collect();
    let reach = |k: usize, centre: f32| {
        corners
            .iter()
            .map(|c| (c[k] - centre).abs())
            .fold(f32::MIN, f32::max)
    };
    assert!(reach(0, 80.0) > 6.0 + 1.1 && reach(2, -500.0) > 5.0 + 1.1);
}

#[tokio::test]
async fn roofs_of_irregular_outlines_stay_over_them() {
    // An L-shaped farmhouse: 20 m arms, 10 m wide.
    let corners = [
        (60.0, 400.0),
        (80.0, 400.0),
        (80.0, 410.0),
        (70.0, 410.0),
        (70.0, 420.0),
        (60.0, 420.0),
        (60.0, 400.0),
    ];
    let house = Building {
        id: 11,
        outline: corners.iter().map(|&(e, n)| at(e, n)).collect(),
        height: None,
        levels: None,
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    let faces = building_faces(&world, (70.0, 410.0), 30.0);
    assert!(
        faces.iter().any(|f| (0.1..0.99).contains(&f.normal[1])),
        "a pitched roof"
    );
    // Nothing reaches beyond the walls by more than the eaves' overhang (at most 1.2 m), so
    // nothing covers the notch.
    for c in faces.iter().flat_map(|f| f.corners) {
        let (east, north) = (c[0], -c[2]);
        let beyond = |value: f32, low: f32, high: f32| (low - value).max(value - high).max(0.0);
        let to_arm =
            |e: (f32, f32), n: (f32, f32)| beyond(east, e.0, e.1).max(beyond(north, n.0, n.1));
        let outside =
            to_arm((60.0, 80.0), (400.0, 410.0)).min(to_arm((60.0, 70.0), (400.0, 420.0)));
        assert!(outside < 1.25, "{c:?} is {outside} m out");
    }
}

#[tokio::test]
async fn shops_get_a_front_and_an_awning_onto_their_street() {
    use orqwa_osm::{Road, RoadClass};

    // Two 12 × 9 m houses along a street to their south; a bakery in the western one.
    let house = |id: i64, east: f64| Building {
        id,
        outline: rectangle(east, 500.0, 6.0, 4.5),
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![house(31, 80.0), house(32, 120.0)],
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(40.0, 489.0), at(160.0, 489.0)],
            structure: None,
        }],
        shops: vec![at(81.0, 501.0)],
        ..MapData::default()
    })
    .await;

    // Drawn by the world itself, so the front shows up close too.
    assert!(
        placed(&world)
            .iter()
            .all(|m| (m.origin[0] - 80.0).abs() > 1.0),
        "the shop became a model"
    );
    let shop = building_faces(&world, (80.0, 500.0), 9.0);
    let fronts: Vec<&Face> = shop.iter().filter(|f| f.style() == 10).collect();
    assert!(!fronts.is_empty(), "no shop front");
    let ground = slope_at(86.0);
    for front in &fronts {
        // On the ground floor of the south wall, facing the street (Godot's +z).
        assert!(front.normal[2] > 0.99, "a front facing {:?}", front.normal);
        assert!(
            (-front.middle()[2] - 495.5).abs() < 0.2,
            "a front at {:?}",
            front.middle()
        );
        assert!(front.corners.iter().all(|c| c[1] < ground + 3.0));
    }
    // The awning reaches out over the pavement, above the front.
    let awnings = palette::list("buildings.awnings");
    let awning: Vec<&Face> = shop
        .iter()
        .filter(|f| awnings.contains(&[f.color[0], f.color[1], f.color[2]]))
        .collect();
    assert!(awning.len() >= 4, "{} awning faces", awning.len());
    let reach = awning
        .iter()
        .flat_map(|f| f.corners)
        .map(|c| -c[2])
        .fold(f32::MAX, f32::min);
    assert!(
        (495.5 - reach - 1.4).abs() < 0.15,
        "the awning reaches to {reach}"
    );
    // The neighbour has no shop.
    let neighbour = building_faces(&world, (120.0, 500.0), 9.0);
    assert!(neighbour.iter().all(|f| f.style() != 10));
}

#[tokio::test]
async fn roads_stay_clear_of_buildings() {
    use orqwa_osm::{Road, RoadClass};

    // A 60 × 20 m block across the road ridden (its corners far from it), a 40 × 12 m block
    // a street runs through, and a house beside each.
    let building = |id: i64, outline| Building {
        id,
        outline,
        height: None,
        levels: Some(3.0),
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![
            building(41, rectangle(0.0, 300.0, 30.0, 10.0)),
            building(42, rectangle(20.0, 400.0, 6.0, 4.5)),
            building(43, rectangle(100.0, 600.0, 20.0, 6.0)),
            building(44, rectangle(100.0, 615.0, 6.0, 4.0)),
        ],
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(40.0, 600.0), at(200.0, 600.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;

    let models = placed(&world);
    let stands = |(east, north): (f32, f32), radius: f32| {
        !building_faces(&world, (east, north), radius).is_empty()
            || models
                .iter()
                .any(|m| (m.origin[0] - east).hypot(-m.origin[2] - north) < radius)
    };
    assert!(!stands((0.0, 300.0), 25.0), "a block stands on the road");
    assert!(!stands((100.0, 600.0), 8.0), "a block stands on the street");
    assert!(stands((20.0, 400.0), 5.0), "the house by the road is gone");
    assert!(
        stands((100.0, 615.0), 3.0),
        "the house by the street is gone"
    );
}

#[tokio::test]
async fn rectangular_houses_become_models_with_shells_for_the_distance() {
    // A 12 × 9 m house with its long side north–south, and an L-shaped farmhouse.
    let house = Building {
        id: 21,
        outline: rectangle(60.0, 500.0, 4.5, 6.0),
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let l_shape = [
        (60.0, 700.0),
        (80.0, 700.0),
        (80.0, 710.0),
        (70.0, 710.0),
        (70.0, 720.0),
        (60.0, 720.0),
        (60.0, 700.0),
    ];
    let farmhouse = Building {
        id: 22,
        outline: l_shape.iter().map(|&(e, n)| at(e, n)).collect(),
        ..house.clone()
    };
    let world = world(&MapData {
        buildings: vec![house, farmhouse],
        ..MapData::default()
    })
    .await;

    let models = placed(&world);
    assert_eq!(models.len(), 1, "only the rectangular house");
    let house = &models[0];
    assert!(house.model.starts_with("house_"), "{}", house.model);
    // Standing on its footprint's centre, at the highest ground (65 m east), stretched by no
    // more than a quarter, its length along the outline's.
    assert!((house.origin[0] - 60.0).abs() < 0.01 && (house.origin[2] + 500.0).abs() < 0.01);
    assert!(
        (house.origin[1] - slope_at(64.5)).abs() < 0.01,
        "{}",
        house.origin[1]
    );
    assert!(
        house.x[0].abs() < 0.01 && house.z[2].abs() < 0.01,
        "length runs north"
    );
    let (along, across) = (length(house.x), length(house.z));
    assert!((0.8..=1.25).contains(&along) && (0.8..=1.25).contains(&across));
    assert!(house.plaster[3] == 1.0 && house.plaster[..3].iter().all(|&c| c > 0.3));
    // Both still have shells: the house's for the distance, the farmhouse's always.
    assert!(!building_faces(&world, (60.0, 500.0), 12.0).is_empty());
    assert!(!building_faces(&world, (70.0, 710.0), 15.0).is_empty());
    let unmodelled: usize = world
        .chunks
        .iter()
        .map(|c| c.buildings.vertices.len())
        .sum();
    assert!(unmodelled > 0, "the farmhouse is drawn as its shell");
}

/// Terrain sloping down eastwards by `grade`, 1300 m at the route start: mountains.
struct Valley {
    grade: f64,
}

impl ElevationModel for Valley {
    fn elevation(
        &mut self,
        _lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
        std::future::ready(Ok(1300.0 - self.grade * east))
    }
}

#[tokio::test]
async fn chalets_show_their_front_to_the_valley_and_churches_their_choir_to_the_east() {
    // A chalet with its long side east–west on ground falling eastwards, and a church
    // likewise, each the other way round in the map.
    let chalet = Building {
        id: 31,
        outline: rectangle(80.0, 400.0, 6.0, 4.75),
        height: None,
        levels: None,
        color: None,
    };
    let church = Building {
        id: 32,
        outline: [
            (-95.0, 600.0),
            (-95.0, 610.5),
            (-67.0, 610.5),
            (-67.0, 600.0),
            (-95.0, 600.0),
        ]
        .iter()
        .map(|&(e, n)| at(e, n))
        .collect(),
        ..chalet.clone()
    };
    let map = MapData {
        buildings: vec![chalet, church],
        churches: vec![at(-80.0, 605.0)],
        ..MapData::default()
    };
    let route = route_north(&[]).await;
    let world = generate(&route, &mut Valley { grade: 0.05 }, &map, &mut |_, _| {}).await;

    let models = placed(&world);
    let chalet = models
        .iter()
        .find(|p| p.model.starts_with("chalet"))
        .expect("a chalet model");
    assert!(
        chalet.x[0] > 0.0,
        "the balconies face east, down the valley"
    );
    let church = models
        .iter()
        .find(|p| p.model.starts_with("church"))
        .expect("a church model");
    assert!(church.x[0] > 0.0, "the choir is in the east");
}

#[tokio::test]
async fn houses_on_steep_slopes_keep_their_shells() {
    // A 30 % slope: the downhill side of a 12 m house lies 3.6 m lower, more than a model's
    // basement walls reach.
    let house = Building {
        id: 41,
        outline: rectangle(80.0, 400.0, 6.0, 4.5),
        height: None,
        levels: Some(2.0),
        color: None,
    };
    let map = MapData {
        buildings: vec![house],
        ..MapData::default()
    };
    let route = route_north(&[]).await;
    let world = generate(&route, &mut Valley { grade: 0.3 }, &map, &mut |_, _| {}).await;

    assert!(placed(&world).is_empty());
    assert!(!building_faces(&world, (80.0, 400.0), 12.0).is_empty());
}

#[tokio::test]
async fn streams_run_in_channels_and_pass_under_roads_and_streets() {
    use orqwa_osm::{Road, RoadClass};

    // A river crossing the route at 500 m, and a street crossing the river east of the route.
    let world = world(&MapData {
        waterways: vec![Waterway {
            width: 12.0,
            line: vec![at(-3000.0, 500.0), at(3000.0, 500.0)],
        }],
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(100.0, 300.0), at(100.0, 700.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;
    let water_at = |x: f32, z: f32| height_on(&world, |c| &c.water, x, z);

    for chunk in &world.chunks {
        assert_valid(&chunk.water);
        // Only within the corridor.
        assert!(
            chunk
                .water
                .vertices
                .iter()
                .all(|v| (v[0] + chunk.center[0]).abs() <= CORRIDOR as f32 + 10.0)
        );
    }
    let mut checked = 0;
    for step in -1400..=1400 {
        #[allow(clippy::cast_precision_loss)] // small steps
        let x = step as f32;
        for z in [-505.0, -500.0, -495.0] {
            let (Some(water), Some(ground)) = (water_at(x, z), ground_at(&world, x, z)) else {
                continue;
            };
            // In a channel (#93): over its bed, half a metre below the land beside it (rising
            // 0.1 m per metre east), never floating over the land.
            let land = 500.0 + 0.1 * x;
            // Where the road and the street cross on the ground, the stream runs through a
            // culvert, under the ground.
            let culvert = x.abs() < 12.0 || (x - 100.0).abs() < 9.0;
            assert!(
                culvert || water > ground,
                "water {water} under ground {ground} at {x}, {z}"
            );
            // Away from the road and the street, which level the land across them (#116).
            if x.abs() > LEVEL_REACH as f32 && (x - 100.0).abs() > 20.0 {
                assert!(
                    (water - (land - 0.48)).abs() < 0.05,
                    "water {water} by land at {land} at {x}, {z}"
                );
            }
            if x.abs() < ROAD_HALF_WIDTH as f32 {
                assert!(
                    water < 500.0 - 0.05,
                    "water over the road ridden: {water} at {x}, {z}"
                );
            }
            if let Some(street) = street_at(&world, x, z) {
                assert!(
                    water < street,
                    "water over the street: {water} over {street} at {x}, {z}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 6000, "{checked} points checked");
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
async fn bridge_piers_reach_the_ground_mesh_instead_of_an_unsampled_dem_ridge() {
    struct NarrowRidge;
    impl ElevationModel for NarrowRidge {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (north - 306.0).abs() < 0.4 {
                480.0
            } else {
                430.0
            }))
        }
    }
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let world = generate(
        &route,
        &mut NarrowRidge,
        &MapData::default(),
        &mut |_, _| {},
    )
    .await;
    let bottom = world
        .structures
        .vertices
        .iter()
        .filter(|v| v[0].abs() < 1.5 && (v[2] + 305.0).abs() < 1.5)
        .map(|v| v[1])
        .fold(f32::MAX, f32::min);
    let ground = ground_at(&world, 0.0, -305.0).expect("drawn ground beneath the pier");
    assert!(
        bottom < ground,
        "pier bottom {bottom} floats over drawn terrain at {ground}"
    );
}

#[tokio::test]
async fn a_pier_footing_reaches_the_downhill_side_of_steep_terrain() {
    struct SteepBank;
    impl ElevationModel for SteepBank {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46_f64.to_radians().cos();
            std::future::ready(Ok(430.0 + 20.0 * east))
        }
    }
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let world = generate(&route, &mut SteepBank, &MapData::default(), &mut |_, _| {}).await;
    let bottom = world
        .structures
        .vertices
        .iter()
        .filter(|v| v[0] < -1.0 && v[0] > -1.5 && (v[2] + 305.0).abs() < 1.5)
        .map(|v| v[1])
        .fold(f32::MAX, f32::min);
    let downhill = ground_at(&world, -1.1, -305.0).expect("drawn downhill bank");
    assert!(
        bottom < downhill,
        "pier bottom {bottom} floats above downhill ground at {downhill}"
    );
}

#[tokio::test]
async fn a_ridden_bridge_has_no_second_deck_from_the_map() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            // GPX and vector-tile centre lines need not coincide exactly. Both describe
            // the same bridge; raw end elevations must not produce a second deck below it.
            line: vec![at(0.4, 300.0), at(0.4, 700.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(480.0), &map, &mut |_, _| {}).await;
    let duplicate = world
        .chunks
        .iter()
        .flat_map(|chunk| {
            chunk
                .streets
                .vertices
                .iter()
                .map(move |v| [v[0] + chunk.center[0], v[1], v[2] + chunk.center[2]])
        })
        .filter(|v| v[0].abs() < 5.0 && (-680.0..-320.0).contains(&v[2]))
        .count();
    assert_eq!(
        duplicate, 0,
        "a mapped bridge ridden on the course is built only once"
    );
}

#[tokio::test]
async fn a_mapped_bridge_on_a_ground_route_does_not_add_a_raised_second_surface() {
    let route = route_north(&[]).await;
    for length in [13.0, 40.0] {
        let map = MapData {
            roads: vec![orqwa_osm::Road {
                class: RoadClass::Major,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.4, 500.0), at(0.4, 500.0 + length)],
                structure: Some(StructureKind::Bridge),
            }],
            ..MapData::default()
        };
        let world = generate(&route, &mut Level(501.5), &map, &mut |_, _| {}).await;
        assert!(
            world.chunks.iter().all(|c| c.streets.vertices.is_empty()),
            "a complete aligned mapped feature must not add a raised deck over a Ground route"
        );
    }
}

#[tokio::test]
async fn a_surface_bridge_above_a_ridden_tunnel_retains_its_deck() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Major,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(0.4, 400.0), at(0.4, 600.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(520.0), &map, &mut |_, _| {}).await;
    assert!(
        world.chunks.iter().any(|c| !c.streets.vertices.is_empty()),
        "the underground route must not claim a separate surface bridge"
    );
}

#[tokio::test]
async fn a_bridge_tag_boundary_does_not_create_an_extra_approach_deck() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Major,
            details: orqwa_osm::RoadDetails::default(),
            // The mapped end is just before the resampled route changes to Bridge.
            // Point-wise ownership leaves a raised slab and railing across the approach.
            line: vec![at(0.4, 289.0), at(0.4, 700.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(480.0), &map, &mut |_, _| {}).await;
    assert!(
        world.chunks.iter().all(|c| c.streets.vertices.is_empty()),
        "one mapped bridge following the ridden centre line must have one complete owner"
    );
}

#[tokio::test]
async fn a_bridge_extending_beyond_the_ridden_route_keeps_its_separate_section() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Major,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(0.4, 300.0), at(0.4, 1100.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(480.0), &map, &mut |_, _| {}).await;
    assert!(
        world.chunks.iter().any(|c| c
            .streets
            .vertices
            .iter()
            .any(|v| v[2] + c.center[2] < -1040.0)),
        "a mapped bridge continuing beyond the route retains its own deck"
    );
}

#[tokio::test]
async fn separate_parallel_and_crossing_bridges_are_not_claimed_by_the_ridden_bridge() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    for (line, target) in [
        (vec![at(12.0, 300.0), at(12.0, 700.0)], (12.0, -500.0)),
        (vec![at(-60.0, 500.0), at(60.0, 500.0)], (30.0, -500.0)),
    ] {
        let map = MapData {
            roads: vec![orqwa_osm::Road {
                class: RoadClass::Street,
                details: orqwa_osm::RoadDetails::default(),
                line,
                structure: Some(StructureKind::Bridge),
            }],
            ..MapData::default()
        };
        let world = generate(&route, &mut Level(480.0), &map, &mut |_, _| {}).await;
        assert!(
            world
                .chunks
                .iter()
                .any(|chunk| chunk.streets.vertices.iter().any(|v| {
                    (v[0] + chunk.center[0] - target.0).abs() < 4.0
                        && (v[2] + chunk.center[2] - target.1).abs() < 4.0
                })),
            "a separate bridge at {target:?} must remain visible"
        );
    }
}

#[tokio::test]
async fn non_ridden_bridges_are_solid_from_below() {
    use orqwa_osm::{Road, RoadClass};

    // R121: sloping bridges across the ridden road, with a gentle bend and several chunks.
    // Motorways use Major; tracks and paths use the separate unpaved mesh.
    let classes = [
        RoadClass::Major,
        RoadClass::Street,
        RoadClass::Service,
        RoadClass::Track,
        RoadClass::Path,
    ];
    let map = MapData {
        // Shore gravel shares the track mesh with the coloured railings: combining them
        // must preserve complete vertex arrays as well as the bridge geometry.
        areas: vec![Area {
            cover: LandCover::Water,
            outer: vec![square(250.0, 820.0, 35.0)],
            inner: vec![],
        }],
        roads: classes
            .iter()
            .enumerate()
            .map(|(i, &class)| {
                let north = 100.0 + i as f64 * 180.0;
                Road {
                    class,
                    line: vec![at(-600.0, north), at(90.0, north + 20.0), at(600.0, north)],
                    structure: Some(StructureKind::Bridge),
                    details: orqwa_osm::RoadDetails::default(),
                }
            })
            .collect(),
        ..MapData::default()
    };
    let world = world(&map).await;
    for chunk in &world.chunks {
        for mesh in [&chunk.streets, &chunk.tracks] {
            assert_valid(mesh);
            assert_faces_follow_normals(mesh);
            assert!(
                mesh.colors
                    .iter()
                    .zip(&mesh.normals)
                    .all(|(color, normal)| {
                        normal[1] < 0.99 || color[3] == 0.0 || color[0] <= 6.0 / 8.0
                    }),
                "bridge wearing surfaces retain road-profile metadata, not default white"
            );
        }
    }
    for (i, class) in classes.into_iter().enumerate() {
        let north = 100.0 + i as f32 * 180.0;
        let pick: fn(&TerrainChunk) -> &MeshData =
            if matches!(class, RoadClass::Track | RoadClass::Path) {
                |c: &TerrainChunk| &c.tracks
            } else {
                |c: &TerrainChunk| &c.streets
            };
        let faces: Vec<_> = chunk_triangles(&world, pick).collect();
        // Include both sides of chunk boundaries, the bend, and both edges of the deck.
        for east in [
            -500.0_f32, -480.1, -479.9, -240.1, -239.9, -0.1, 0.1, 90.0, 239.9, 240.1, 479.9,
            480.1, 500.0,
        ] {
            for across in [-0.6_f32, 0.0, 0.6] {
                let bend = if east < 90.0 {
                    (east + 600.0) / 690.0
                } else {
                    (600.0 - east) / 510.0
                };
                let z = -(north + 20.0 * bend) + across;
                let top = faces
                    .iter()
                    .find_map(|&t| {
                        (face_normal(t)[1] < -0.01)
                            .then(|| triangle_height(t, east, z))
                            .flatten()
                    })
                    .unwrap_or_else(|| panic!("no {class:?} bridge top at {east}, {z}"));
                let underside = faces.iter().find_map(|&t| {
                    (face_normal(t)[1] > 0.01)
                        .then(|| triangle_height(t, east, z))
                        .flatten()
                });
                let bottom =
                    underside.unwrap_or_else(|| panic!("no {class:?} underside at {east}, {z}"));
                assert!(
                    (top - bottom - 0.7).abs() < 0.01,
                    "{class:?} deck thickness at {east}, {z}: top {top}, bottom {bottom}"
                );
            }
        }
    }
}

#[tokio::test]
async fn bridge_junctions_meet_the_road_without_a_step_or_rail_across_it() {
    for (terrain, joins) in [(501.5, true), (510.0, false)] {
        let map = MapData {
            roads: vec![orqwa_osm::Road {
                class: RoadClass::Street,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.0, 500.0), at(80.0, 500.0)],
                structure: Some(StructureKind::Bridge),
            }],
            ..MapData::default()
        };
        let world = generate(
            &route_north(&[]).await,
            &mut Level(terrain),
            &map,
            &mut |_, _| {},
        )
        .await;
        let vertices: Vec<_> = world
            .chunks
            .iter()
            .flat_map(|c| {
                c.streets
                    .vertices
                    .iter()
                    .map(move |v| [v[0] + c.center[0], v[1], v[2] + c.center[2]])
            })
            .filter(|v| v[0].abs() < 2.5 && (v[2] + 500.0).abs() < 5.0)
            .collect();
        assert!(!vertices.is_empty(), "the connecting deck still exists");
        if joins {
            let deck = street_at(&world, 30.0, -500.0).expect("connected deck");
            let ground = ground_at(&world, 30.0, -500.0).expect("approach bank");
            assert!(
                ground < deck,
                "the adjusted deck must not disappear under its bank: {ground} >= {deck}"
            );
            assert!(
                vertices.iter().all(|v| v[1] <= 500.1),
                "deck or protection blocks the junction: {vertices:?}"
            );
        } else {
            assert!(
                vertices.iter().any(|v| v[1] > 511.0),
                "a grade-separated overpass keeps its height and protection"
            );
        }
        assert!(
            world.chunks.iter().any(|c| c
                .streets
                .vertices
                .iter()
                .any(|v| v[0] + c.center[0] > 30.0 && v[1] > 501.0)),
            "protection remains away from the junction"
        );
    }
}

#[tokio::test]
async fn a_side_street_has_an_opening_in_bridge_protection_only_where_it_joins() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(0.0, 500.0), at(80.0, 500.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    for (terrain, joins) in [(500.0, true), (480.0, false)] {
        let world = generate(&route, &mut Level(terrain), &map, &mut |_, _| {}).await;
        let raised = |side: f32| {
            world
                .structures
                .vertices
                .iter()
                .any(|v| v[0] * side > 3.0 && (v[2] + 500.0).abs() < 2.0 && v[1] > 500.1)
        };
        assert_eq!(
            raised(1.0),
            !joins,
            "the joining side opens only at the deck's level"
        );
        assert!(raised(-1.0), "the opposite edge keeps its protection");
    }
}

#[tokio::test]
async fn bridges_of_other_streets_stand_on_piers() {
    use orqwa_osm::{Road, RoadClass};

    // A street bridge east of the route over a valley 20 m deep.
    struct Valley;
    impl ElevationModel for Valley {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(if (150.0..250.0).contains(&east) {
                480.0
            } else {
                500.0
            }))
        }
    }
    let route = route_north(&[]).await;
    let map = MapData {
        roads: vec![Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(100.0, 600.0), at(300.0, 600.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Valley, &map, &mut |_, _| {}).await;

    let mut piers = 0;
    for chunk in &world.chunks {
        assert_valid(&chunk.streets);
        assert_faces_follow_normals(&chunk.streets);
        for v in &chunk.streets.vertices {
            let (x, y) = (v[0] + chunk.center[0], v[1]);
            if y < 495.0 {
                // Only in the valley (its sides slope across a cell of the ground), down to
                // its floor.
                assert!(
                    (134.0..=266.0).contains(&x),
                    "a pier outside the valley at {x}"
                );
                assert!(y > 479.0, "a pier under the ground: {y}");
                if y < 480.0 {
                    piers += 1;
                }
            }
        }
    }
    // Five piers, 20 m apart, four corners each at the bottom.
    assert!(piers >= 16, "{piers} pier corners on the valley floor");
}

#[tokio::test]
async fn a_mapped_bridge_pier_reaches_the_downhill_edge_of_its_footing() {
    struct SlopingValley;
    impl ElevationModel for SlopingValley {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46_f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (150.0..250.0).contains(&east) {
                480.0 + 20.0 * (north - 600.0)
            } else {
                500.0
            }))
        }
    }
    let route = route_north(&[]).await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(100.0, 600.0), at(300.0, 600.0)],
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut SlopingValley, &map, &mut |_, _| {}).await;
    let bottom = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.streets
                .vertices
                .iter()
                .map(move |v| [v[0] + c.center[0], v[1], v[2] + c.center[2]])
        })
        .filter(|v| (v[0] - 210.0).abs() < 1.0 && (v[2] + 600.0).abs() < 2.0)
        .min_by(|a, b| a[1].total_cmp(&b[1]))
        .expect("mapped pier footing");
    let downhill = ground_at(&world, bottom[0], bottom[2]).expect("downhill footing edge");
    assert!(
        bottom[1] < downhill,
        "mapped pier at {bottom:?} hangs over downhill terrain at {downhill}"
    );
}

#[tokio::test]
async fn a_bridge_cut_by_a_tile_border_is_one_deck_between_its_ends() {
    use orqwa_osm::{Road, RoadClass};

    // #116, #117: a street bridge over a valley 20 m deep, cut in the middle by a tile border
    // as the map tiles have it (the two pieces' ends a few decimetres apart). Each piece got a
    // deck down to the valley floor at the cut.
    struct Ravine;
    impl ElevationModel for Ravine {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(if (150.0..250.0).contains(&east) {
                480.0
            } else {
                500.0
            }))
        }
    }
    let piece = |from: (f64, f64), to: (f64, f64)| Road {
        class: RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![at(from.0, from.1), at(to.0, to.1)],
        structure: Some(StructureKind::Bridge),
    };
    let route = route_north(&[]).await;
    let map = MapData {
        roads: vec![
            piece((100.0, 600.0), (200.0, 600.0)),
            // Drawn the other way round.
            piece((300.0, 600.0), (200.0, 600.3)),
        ],
        ..MapData::default()
    };
    let world = generate(&route, &mut Ravine, &map, &mut |_, _| {}).await;

    let mut deck = 0;
    for chunk in &world.chunks {
        for (v, normal) in chunk.streets.vertices.iter().zip(&chunk.streets.normals) {
            if normal[1] < 0.5 {
                continue;
            }
            let (x, y, z) = (v[0] + chunk.center[0], v[1], v[2] + chunk.center[2]);
            if !(160.0..=240.0).contains(&x) || (z + 600.0).abs() > 5.0 {
                continue;
            }
            // The deck and bearing caps stay at the bridge's end height across the tile
            // join. Vertical supports and railings have their own exposed lower faces.
            assert!(!(481.0..499.0).contains(&y), "the deck dips to {y} at {x}");
            if y > 499.9 {
                deck += 1;
            }
        }
    }
    assert!(deck > 10, "{deck} deck vertices over the valley");
}

#[test]
fn pieces_of_a_way_join_end_to_end_but_not_at_junctions() {
    // #116: a way cut by tile borders, its pieces' ends a few decimetres apart where the tiles
    // cut it, one piece drawn the other way round; at its end two others meet it.
    let west = [(0.0, 0.0), (100.0, 0.0)];
    let middle = [(200.0, 0.0), (100.2, 0.3)];
    let east = [(200.0, 0.0), (300.0, 0.0)];
    let branch = [(300.0, 0.0), (300.0, 100.0)];
    let beyond = [(300.0, 0.0), (400.0, 0.0)];
    let lines: Vec<&[(f64, f64)]> = [&west, &middle, &east, &branch, &beyond]
        .into_iter()
        .map(<[(f64, f64); 2]>::as_slice)
        .collect();

    assert_eq!(
        crate::chains::chains(&lines, &|_, _| true),
        vec![
            vec![(0, false), (1, true), (2, false)],
            vec![(3, false)],
            vec![(4, false)],
        ]
    );
    // Pieces that do not go together (another kind of way) stay apart.
    assert_eq!(
        crate::chains::chains(&lines[..3], &|first, second| first + second != 1),
        vec![vec![(0, false)], vec![(2, true), (1, false)]]
    );
}

#[tokio::test]
async fn short_low_bridges_are_stone_arches() {
    // A 40 m bridge over a gully 8 m deep.
    struct Gully;
    impl ElevationModel for Gully {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (305.0..335.0).contains(&north) {
                492.0
            } else {
                500.0
            }))
        }
    }
    let bridge = Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 340.0)],
    };
    let route = route_north(&[bridge]).await;
    let world = generate(&route, &mut Gully, &MapData::default(), &mut |_, _| {}).await;

    let mesh = &world.structures;
    assert_valid(mesh);
    assert_faces_follow_normals(mesh);
    let stone = palette::srgb("structure.stone", 0.0);
    assert!(mesh.colors.iter().all(|c| *c == stone), "built of stone");
    // Vaults: faces turned down into the openings, curving from their springing up to just
    // under the deck.
    let vault: Vec<f32> = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| mesh.normals[t[0] as usize][1] < -0.1)
        .map(|t| t.iter().map(|&k| mesh.vertices[k as usize][1]).sum::<f32>() / 3.0)
        .filter(|&y| y < 500.0 - 1.2 - 0.1)
        .collect();
    assert!(vault.len() > 10, "{} vault faces", vault.len());
    let (low, high) = vault
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
    assert!(high - low > 2.0, "the vault curves: {low} to {high}");
    assert!(high < 500.0 - 1.2 - 0.3, "crowns under the deck: {high}");
    // The piers and walls reach down into the gully.
    let lowest = mesh.vertices.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!(
        (lowest - 489.5).abs() < 0.1,
        "below the gully's floor: {lowest}"
    );
}

#[tokio::test]
async fn bridge_bearing_walls_meet_the_drawn_ground() {
    // The source DEM has a sharp bank just inside the span. Rendered terrain interpolates
    // grid samples: a footing based only on the nearest raw centre sample can hang above it.
    struct Bank;
    impl ElevationModel for Bank {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (301.0..699.0).contains(&north) {
                470.0
            } else {
                500.0
            }))
        }
    }
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let world = generate(&route, &mut Bank, &MapData::default(), &mut |_, _| {}).await;
    let mut corners = 0;
    for (vertex, normal) in world
        .structures
        .vertices
        .iter()
        .zip(&world.structures.normals)
    {
        if normal[1] > -0.99 || vertex[1] > 498.0 {
            continue;
        }
        // End walls span the deck; intermediate columns are narrower.
        if vertex[0].abs() < 3.5 {
            continue;
        }
        let ground = ground_at(&world, vertex[0], vertex[2]).expect("ground under bearing wall");
        assert!(
            vertex[1] <= ground + 0.1,
            "foundation {vertex:?} above ground {ground}"
        );
        corners += 1;
    }
    assert!(
        corners >= 8,
        "both bearing walls have buried foundation corners"
    );
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
        (lowest - 467.5).abs() < 0.1,
        "pillars reach below the valley floor: {lowest}"
    );
    let highest = mesh.vertices.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    assert!(
        (501.0..=501.2).contains(&highest),
        "open safety rails at human height: {highest}"
    );
    let steel = palette::srgb("structure.steel", 0.0);
    assert!(
        mesh.vertices
            .iter()
            .zip(&mesh.colors)
            .any(|(v, c)| *c == steel && v[1] > 501.0),
        "metal protection follows the deck edges"
    );
    assert!(
        mesh.vertices
            .iter()
            .zip(&mesh.colors)
            .all(|(v, c)| *c == steel || v[1] < 500.3),
        "concrete kerbs stay low so open railings do not read as solid walls"
    );
    for (vertex, uv) in world.road.vertices.iter().zip(&world.road.uvs) {
        if (320.0..680.0).contains(&uv[1]) {
            assert!(
                vertex[0].abs() < 3.3,
                "wearing course fits inside the deck: {vertex:?}"
            );
            assert!(
                (0.0..=1.0).contains(&uv[0]),
                "no gravel or grass skirt hangs off the bridge"
            );
        }
    }
    // Nothing outside the bridge.
    assert!(
        mesh.vertices
            .iter()
            .all(|v| (-720.0..=-280.0).contains(&v[2]))
    );
}

#[tokio::test]
async fn a_mapped_road_does_not_climb_over_the_ridden_tunnel() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            // Approach pieces can overlap the mapped tunnel and differ slightly from
            // its smoothed GPX line. Draping them onto the tunnel hill blocks its mouth.
            line: vec![at(0.4, 0.0), at(0.4, 1000.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let over_tunnel = world.chunks.iter().any(|chunk| {
        chunk.streets.vertices.iter().any(|v| {
            let (x, z) = (v[0] + chunk.center[0], v[2] + chunk.center[2]);
            x.abs() < 3.5 && (-690.0..-310.0).contains(&z) && v[1] > 500.5
        })
    });
    assert!(
        !over_tunnel,
        "the ridden tunnel must not have a second road draped over its roof"
    );
}

#[tokio::test]
async fn mapped_approach_paving_does_not_close_tunnel_portals() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: [(0.0, 300.0), (700.0, 1000.0)]
            .into_iter()
            .map(|(start, end)| orqwa_osm::Road {
                class: RoadClass::Major,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.4, start), at(0.4, end)],
                structure: None,
            })
            .collect(),
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let inside = palette::srgb("structure.tunnel", 0.0);
    let tube: Vec<_> = world
        .structures
        .vertices
        .iter()
        .zip(&world.structures.colors)
        .filter(|(_, color)| **color == inside)
        .map(|(v, _)| -v[2])
        .collect();
    let entrance = tube.iter().copied().fold(f32::MAX, f32::min);
    let exit = tube.iter().copied().fold(f32::MIN, f32::max);
    for portal in [entrance, exit] {
        for x in [-2.0, 0.0, 2.0] {
            for step in -12..=12 {
                let north = portal + step as f32 * 0.5;
                if let Some(paving) = street_at(&world, x, -north) {
                    assert!(
                        !(500.3..504.0).contains(&paving),
                        "paving at {paving} blocks the portal at {portal}, x={x}, north={north}: {:?}",
                        chunk_triangles(&world, |c| &c.streets)
                            .find(|&t| triangle_height(t, x, -north).is_some())
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn a_surface_road_crossing_over_a_tunnel_remains_visible() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(-80.0, 500.0), at(80.0, 500.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(550.0), &map, &mut |_, _| {}).await;
    assert!(
        world
            .chunks
            .iter()
            .any(|chunk| chunk.streets.vertices.iter().any(|v| {
                (v[0] + chunk.center[0]).abs() < 5.0
                    && (v[2] + chunk.center[2] + 500.0).abs() < 5.0
                    && v[1] > 549.0
            })),
        "a distinct road crossing above the tunnel keeps its surface"
    );
}

#[tokio::test]
async fn terrain_and_paving_stay_out_of_the_complete_tunnel_interior() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    // A nearby lower line cuts a bank into the tunnel hillside, well beyond either mouth.
    let map = MapData {
        railways: vec![railway(&[(8.0, 0.0), (8.0, 1000.0)], None)],
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(8.0, 400.0), at(8.0, 600.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let surfaces: [fn(&TerrainChunk) -> &MeshData; 4] =
        [|c| &c.mesh, |c| &c.streets, |c| &c.tracks, |c| &c.urban];
    // Inspect every intersection: a valid roof must not hide a second surface inside it.
    let surrounding: Vec<_> = surfaces
        .into_iter()
        .flat_map(|surface| chunk_triangles(&world, surface))
        .collect();
    for north in (450..550).step_by(3) {
        for x in [-3.0_f32, 0.0, 3.0, 4.0] {
            let ceiling = 500.0 + (25.0 - x * x).sqrt() - 0.2;
            for height in surrounding
                .iter()
                .filter_map(|&triangle| triangle_height(triangle, x, -(north as f32)))
            {
                assert!(
                    height <= 499.85 || height >= ceiling,
                    "surrounding surface at {height} intrudes into the tunnel at {x}, {north}"
                );
            }
        }
    }
}

#[tokio::test]
async fn a_complete_aligned_ground_approach_does_not_paint_the_tunnel_roof() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Major,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(0.4, 260.0), at(0.4, 320.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(550.0), &map, &mut |_, _| {}).await;
    assert!(
        world.chunks.iter().all(|c| c.streets.vertices.is_empty()),
        "the route owns this complete approach; no duplicate paving may climb the portal hill"
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
    // The arch of the tube itself: the ring round its opening stands a little proud of it.
    let inside = palette::srgb("structure.tunnel", 0.0);
    let top = mesh
        .vertices
        .iter()
        .zip(&mesh.colors)
        .filter(|(_, colour)| **colour == inside)
        .map(|(v, _)| v[1])
        .fold(f32::MIN, f32::max);
    assert!((top - 505.0).abs() < 0.1, "arch 5 m high: {top}");
    // A floor between the arch's feet, just under the road, facing up: the strip between the
    // road's edge and the wall must not open onto the void (#135).
    let floor = mesh
        .vertices
        .iter()
        .zip(&mesh.normals)
        .filter(|(v, n)| n[1] > 0.99 && (v[1] - (500.0 - 0.15)).abs() < 0.05 && v[0].abs() > 4.9)
        .count();
    assert!(floor > 10, "no floor under the tube's walls");
}

#[tokio::test]
async fn tunnels_enter_the_hill_through_a_portal_left_open() {
    // A ridge 60 m high across the road, its flanks rising 0.8 m a metre, and a tunnel mapped
    // from 300 m to 700 m: the ground lies over the 5 m tube from about 432 m to 568 m only.
    // The tube must not stand in the open in front of the hill, nor the hill close its mouth
    // (#135).
    struct Ridge;
    impl ElevationModel for Ridge {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Ok(ridge((lat - 46.0) * METERS_PER_DEGREE)))
        }
    }
    fn ridge(north: f64) -> f64 {
        500.0 + (60.0 - 0.8 * (north - 500.0).abs()).max(0.0)
    }
    let tunnel = Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    };
    let route = route_north(&[tunnel]).await;
    let world = generate(&route, &mut Ridge, &MapData::default(), &mut |_, _| {}).await;

    let mesh = &world.structures;
    assert_valid(mesh);
    assert_faces_follow_normals(mesh);
    let inside = palette::srgb("structure.tunnel", 0.0);
    let tube: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .zip(&mesh.colors)
        .filter(|(_, colour)| **colour == inside)
        .map(|(v, _)| *v)
        .collect();
    // The tube runs through the hill...
    let under_the_top = tube.iter().filter(|v| (v[2] + 500.0).abs() < 20.0).count();
    assert!(under_the_top > 10, "no tube under the hill");
    // ...from where the ground first lies over its crown to where it last does, nowhere in
    // the open.
    let (entrance, exit) = tube.iter().fold((f32::MAX, f32::MIN), |(low, high), v| {
        (low.min(-v[2]), high.max(-v[2]))
    });
    assert!(
        (420.0..445.0).contains(&entrance),
        "the tube begins at {entrance} m"
    );
    assert!((555.0..580.0).contains(&exit), "the tube ends at {exit} m");
    for v in &tube {
        let hill = ridge(f64::from(-v[2]));
        assert!(
            hill > 504.5,
            "the tube stands in the open at {v:?}, the hill at {hill}"
        );
    }

    // At either end a ring faces out along the road, over the opening and beside it.
    for (portal, facing) in [(entrance, 1.0_f32), (exit, -1.0)] {
        let face: Vec<[f32; 3]> = mesh
            .vertices
            .iter()
            .zip(&mesh.normals)
            .filter(|(v, n)| n[2] * facing > 0.99 && (v[2] + portal).abs() < 2.0)
            .map(|(v, _)| *v)
            .collect();
        assert!(
            face.iter().any(|v| v[0].abs() < 1.0 && v[1] > 505.5),
            "no ring over the opening at {portal} m"
        );
        assert!(
            face.iter().any(|v| v[0] < -5.5) && face.iter().any(|v| v[0] > 5.5),
            "no ring beside the opening at {portal} m"
        );
    }

    // The opening is open: before it the cutting lies at the road, and no ground reaches into
    // it just before the ring or behind it.
    for (portal, inward) in [(entrance, 1.0_f32), (exit, -1.0)] {
        for x in [-3.0_f32, 0.0, 3.0] {
            let before =
                ground_at(&world, x, -(portal - inward * 3.0)).expect("ground before the portal");
            assert!(
                before < 500.5,
                "the ground at {before} before the portal at {portal} m"
            );
            for step in -12..=12 {
                let north = portal + inward * step as f32 * 0.5;
                if let Some(ground) = ground_at(&world, x, -north) {
                    assert!(
                        !(500.3..504.0).contains(&ground),
                        "ground at {ground} in the opening at {x}, {north} m"
                    );
                }
            }
        }
    }

    // The hill over the tunnel stays as it is, right behind the portals too.
    for north in [entrance + 8.0, 470.0, exit - 8.0] {
        let ground = ground_at(&world, 0.0, -north).expect("ground over the tunnel");
        let hill = ridge(f64::from(north)) as f32;
        assert!(
            (ground - hill).abs() < 0.5,
            "the hill at {north} m cut to {ground}, not {hill}"
        );
    }
}

#[tokio::test]
async fn a_tunnel_the_terrain_never_covers_gets_a_hill_heaped_over_it() {
    // A tunnel mapped from 300 m to 700 m on ground the terrain has flat (a gallery on a
    // cliff the samples smooth away): the tube must not lie bare on the hillside. The portals
    // stand at the mapped ends and the ground is heaped over the tube between them (#135).
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
    let inside = palette::srgb("structure.tunnel", 0.0);
    let tube: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .zip(&mesh.colors)
        .filter(|(_, colour)| **colour == inside)
        .map(|(v, _)| *v)
        .collect();
    assert!(!tube.is_empty(), "no tube");
    let (entrance, exit) = tube.iter().fold((f32::MAX, f32::MIN), |(low, high), v| {
        (low.min(-v[2]), high.max(-v[2]))
    });
    // The mapped ends, on the road's own points 10 m apart.
    assert!(
        (285.0..306.0).contains(&entrance),
        "the tube begins at {entrance} m"
    );
    assert!((694.0..715.0).contains(&exit), "the tube ends at {exit} m");
    // Between the portals the ground lies over all of the tube...
    for v in tube
        .iter()
        .filter(|v| -v[2] > entrance + 8.0 && -v[2] < exit - 8.0)
    {
        let ground = ground_at(&world, v[0], v[2])
            .unwrap_or_else(|| panic!("no ground over the tube at {v:?}"));
        assert!(
            ground >= v[1] - 0.05,
            "the tube stands in the open at {v:?}: the ground there is at {ground}"
        );
    }
    // ...before the portal the line runs on the ground at the road's level...
    let before = ground_at(&world, 0.0, -(entrance - 6.0)).expect("ground before the portal");
    assert!(
        before < 500.5,
        "the ground before the portal lies at {before}"
    );
    // ...and a ring faces out over the opening.
    let face: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .zip(&mesh.normals)
        .filter(|(v, n)| n[2] > 0.99 && (v[2] + entrance).abs() < 2.0)
        .map(|(v, _)| *v)
        .collect();
    assert!(
        face.iter().any(|v| v[0].abs() < 1.0 && v[1] > 505.5),
        "no ring over the opening at {entrance} m"
    );
}

#[tokio::test]
async fn portals_stand_where_the_hill_as_drawn_covers_the_tube_for_good() {
    // A hill rising in a sheer step at 450 m to 12 m over the road, dropping back to the road's
    // level in a notch from 470 m to 486 m, and rising for good beyond; the tunnel is mapped
    // from 400 m to 700 m. The terrain tiles have the step, but the ground is drawn from samples
    // 16 m apart and ramps up to it, lower than the tiles have it; and the notch would bare a
    // tube entering at the step. The portal must stand where the drawn hill covers the tube and
    // keeps covering it, with nothing of the tube in the open behind it (#135, #149).
    struct Step;
    impl ElevationModel for Step {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Ok(hill((lat - 46.0) * METERS_PER_DEGREE)))
        }
    }
    fn hill(north: f64) -> f64 {
        if north < 450.0 || (470.0..486.0).contains(&north) {
            500.0
        } else {
            512.0
        }
    }
    let tunnel = Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 400.0), at(0.0, 700.0)],
    };
    let route = route_north(&[tunnel]).await;
    let world = generate(&route, &mut Step, &MapData::default(), &mut |_, _| {}).await;

    let mesh = &world.structures;
    assert_valid(mesh);
    let inside = palette::srgb("structure.tunnel", 0.0);
    let tube: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .zip(&mesh.colors)
        .filter(|(_, colour)| **colour == inside)
        .map(|(v, _)| *v)
        .collect();
    assert!(!tube.is_empty(), "no tube");
    // The tube begins beyond the notch, not at the step...
    let entrance = tube.iter().map(|v| -v[2]).fold(f32::MAX, f32::min);
    assert!(
        (486.0..520.0).contains(&entrance),
        "the tube begins at {entrance} m"
    );
    // ...and between its portals (in whose planes the ground is cut open) the drawn ground
    // lies over all of it.
    let exit = tube.iter().map(|v| -v[2]).fold(f32::MIN, f32::max);
    for v in tube
        .iter()
        .filter(|v| -v[2] > entrance + 2.0 && -v[2] < exit - 2.0)
    {
        let ground = ground_at(&world, v[0], v[2])
            .unwrap_or_else(|| panic!("no ground over the tube at {v:?}"));
        assert!(
            ground >= v[1] - 0.05,
            "the tube stands in the open at {v:?}: the ground there is at {ground}"
        );
    }
    // Before the portal the line runs in a cutting at the road's level.
    let before = ground_at(&world, 0.0, -(entrance - 3.0)).expect("ground before the portal");
    assert!(
        before < 500.5,
        "the ground before the portal lies at {before}"
    );
}

#[tokio::test]
async fn all_world_meshes_face_their_normals() {
    let house = Building {
        id: 7,
        outline: square(60.0, 500.0, 5.0),
        height: Some(9.0),
        levels: None,
        color: None,
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

    // Under the deck and its verge the ground stays below it; the bank rises only beside it.
    let under: Vec<_> = terrain_vertices(&world)
        .filter(|v| v[0].abs() <= 6.3 && (-700.0..-300.0).contains(&v[2]))
        .collect();
    assert!(under.len() > 50);
    for v in under {
        assert!(
            v[1] <= 500.0 - ROAD_SINK as f32 + 0.01,
            "terrain above the deck: {v:?}"
        );
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
        color: None,
    };
    let map = MapData {
        areas: vec![forest, far_lake],
        buildings: vec![house],
        ..MapData::default()
    };

    let flat = crate::minimap(&route_north(&[]).await, &map);

    assert_eq!(flat.vertices.len() % 3, 0);
    assert_eq!(flat.vertices.len(), flat.features.len());
    // Forest square (2 triangles) and house (2 triangles); the lake is 9 km away.
    assert_eq!(flat.vertices.len(), 12);
    assert!(flat.vertices.iter().all(|v| v[0] < 500.0));
}

#[tokio::test]
async fn the_minimaps_corridor_leaves_open_land_to_the_whole_lands_map() {
    // #236: meadows and fields cut off at the corridor's edge covered the land beyond in blocks.
    let area = |cover| Area {
        cover,
        outer: vec![square(300.0, 500.0, 100.0)],
        inner: vec![],
    };
    let map = MapData {
        areas: vec![
            area(LandCover::Meadow),
            area(LandCover::Farmland),
            area(LandCover::Public),
            area(LandCover::Forest),
        ],
        ..MapData::default()
    };

    let flat = crate::minimap(&route_north(&[]).await, &map);

    assert_eq!(flat.features, [MapFeature::Forest; 6]);
}

#[tokio::test]
async fn lakes_are_edged_by_a_band_of_gravel_the_forest_keeps_off() {
    // A pond 100 m square east of the route, in a forest.
    let pond = Area {
        cover: LandCover::Water,
        outer: vec![square(300.0, 500.0, 50.0)],
        inner: vec![],
    };
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![square(300.0, 500.0, 120.0)],
        inner: vec![],
    };
    let world = world(&MapData {
        areas: vec![forest, pond],
        // Both shores and tracks append to the same mesh in these chunks.
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Track,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at(100.0, 400.0), at(100.0, 600.0)],
            structure: None,
        }],
        ..MapData::default()
    })
    .await;
    let band_at = |x: f32, z: f32| height_on(&world, |c| &c.tracks, x, z);
    for chunk in &world.chunks {
        assert_eq!(
            chunk.tracks.colors.len(),
            chunk.tracks.vertices.len(),
            "combined gravel roads and shores need material data for every vertex"
        );
    }

    // All round, just outside the shore, on the ground; none out in the water.
    let mut checked = 0;
    for step in 0..=40 {
        #[allow(clippy::cast_precision_loss)] // small steps
        let along = 255.0 + step as f32 * 2.25;
        for (x, z) in [
            (along, -448.5),
            (along, -551.5),
            (248.5, -(along + 200.0)),
            (351.5, -(along + 200.0)),
        ] {
            let band = band_at(x, z).unwrap_or_else(|| panic!("no shore at {x}, {z}"));
            let ground = ground_at(&world, x, z).expect("ground");
            assert!(
                band > ground && band - ground < 0.03,
                "{band} on {ground} at {x}, {z}"
            );
            checked += 1;
        }
    }
    assert!(checked > 150);
    assert!(band_at(300.0, -500.0).is_none() && band_at(300.0, -453.0).is_none());
    // Trees grow up to the band, not on it.
    let trees = plants_of(&world, &["conifer", "broadleaf"]);
    assert!(trees.len() > 50);
    for [x, _, z] in &trees {
        let outside = (x - 300.0).abs().max((z + 500.0).abs()) - 50.0;
        assert!(
            outside > 3.0 || outside < 0.0,
            "a tree on the shore at {x}, {z}"
        );
    }
}

#[tokio::test]
async fn streams_cut_natural_channels_and_run_on_under_bridges() {
    // A stream 4 m wide crossing the route at 500 m, where the route crosses on a bridge.
    let bridge = Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 480.0), at(0.0, 520.0)],
    };
    let route = route_north(&[bridge]).await;
    let map = MapData {
        waterways: vec![Waterway {
            width: 4.0,
            line: vec![at(-400.0, 500.0), at(400.0, 500.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut EastwardSlope, &map, &mut |_, _| {}).await;

    // Across the stream, 200 m east of the route: the bed deepest in the middle, the banks
    // rising to the land without a wall, the land untouched a few metres out.
    let x = 200.0_f32;
    let land = 500.0 + 0.1 * x;
    let water = height_on(&world, |c| &c.water, x, -500.0).expect("water");
    let profile: Vec<(f32, f32)> = (0..=24)
        .map(|k| {
            #[allow(clippy::cast_precision_loss)] // small steps
            let off = k as f32 * 0.5;
            (
                off,
                ground_at(&world, x, -500.0 - off).expect("ground") - land,
            )
        })
        .collect();
    assert!(
        water - land < -0.4,
        "the water {} m below the land",
        water - land
    );
    assert!(
        profile[0].1 < water - land - 0.2,
        "the bed {} m below the land, the water {}",
        profile[0].1,
        water - land
    );
    for pair in profile.windows(2) {
        let ((a, low), (b, high)) = (pair[0], pair[1]);
        assert!(
            high >= low - 0.01,
            "the bank falls again at {b} m: {profile:?}"
        );
        assert!((high - low) / (b - a) < 1.0, "a wall at {b} m: {profile:?}");
    }
    let (out, top) = profile[profile.len() - 1];
    assert!(top.abs() < 0.02, "the land {top} m off at {out} m");
    // Under the bridge the channel runs on: the ground there lies below the ground under the
    // bridge away from the stream.
    let under = ground_at(&world, 1.0, -500.0).expect("ground under the bridge");
    let beside = ground_at(&world, 1.0, -488.0).expect("ground under the bridge");
    assert!(
        under < beside - 0.5,
        "no channel under the bridge: {under} by {beside}"
    );
}

#[tokio::test]
async fn lakes_lie_level_in_the_land() {
    // A lake east of the road whose surface the terrain model reports at 429 m.
    struct Lake;
    impl ElevationModel for Lake {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(if east > 50.0 { 429.0 } else { 440.0 }))
        }
    }
    let lake = Area {
        cover: LandCover::Water,
        outer: vec![square(400.0, 500.0, 300.0)],
        inner: vec![],
    };
    let route = route_north(&[]).await;
    let map = MapData {
        areas: vec![lake],
        ..MapData::default()
    };
    let world = generate(&route, &mut Lake, &map, &mut |_, _| {}).await;

    for chunk in &world.chunks {
        assert_valid(&chunk.water);
        assert_faces_follow_normals(&chunk.water);
    }
    // All over the lake, level half a metre below its shore (#93), over its bed.
    for x in (110..=690).step_by(20) {
        for north in (210..=790).step_by(20) {
            #[allow(clippy::cast_precision_loss)] // small numbers
            let (x, z) = (x as f32, -(north as f32));
            let water = height_on(&world, |c| &c.water, x, z).expect("water over the lake");
            assert!(
                (water - 428.52).abs() < 0.01,
                "water at {water} at {x}, {z}"
            );
            let bed = ground_at(&world, x, z).expect("a bed");
            assert!(bed < water - 0.2, "the bed at {bed} under water at {water}");
        }
    }
}

#[tokio::test]
async fn rivers_slope_with_their_course_in_their_channel() {
    // A river 40 m wide running east, where the land rises 0.1 m per metre: its surface,
    // which the terrain model measures, falls 130 m along it.
    let river = Area {
        cover: LandCover::Water,
        outer: vec![vec![
            at(100.0, 480.0),
            at(1400.0, 480.0),
            at(1400.0, 520.0),
            at(100.0, 520.0),
            at(100.0, 480.0),
        ]],
        inner: vec![],
    };
    let world = world(&MapData {
        areas: vec![river],
        ..MapData::default()
    })
    .await;

    let mut checked = 0;
    for x in (110..=1390).step_by(5) {
        #[allow(clippy::cast_precision_loss)] // small numbers
        let x = x as f32;
        for z in [-485.0, -500.0, -515.0] {
            let (Some(water), Some(ground)) = (
                height_on(&world, |c| &c.water, x, z),
                ground_at(&world, x, z),
            ) else {
                continue;
            };
            // In its channel (#93): over its bed, half a metre below the land beside it.
            let land = 500.0 + 0.1 * x;
            assert!(
                water > ground,
                "water at {water} under ground {ground} at {x}, {z}"
            );
            assert!(
                (water - (land - 0.48)).abs() < 0.05,
                "water at {water} by land at {land} at {x}, {z}"
            );
            checked += 1;
        }
    }
    assert!(checked > 500, "{checked} points checked");
    let (low, high) = (
        height_on(&world, |c| &c.water, 150.0, -500.0).expect("water"),
        height_on(&world, |c| &c.water, 1350.0, -500.0).expect("water"),
    );
    assert!(high - low > 100.0, "the river runs level: {low} to {high}");
}

#[tokio::test]
async fn mapped_building_colours_turn_into_palette_colours() {
    // A house mapped in dark grey: it keeps a wall colour of the light pastel palette.
    let house = Building {
        id: 5,
        outline: square(60.0, 500.0, 5.0),
        height: None,
        levels: Some(2.0),
        color: Some([0.15, 0.15, 0.17]),
    };
    let world = world(&MapData {
        buildings: vec![house],
        ..MapData::default()
    })
    .await;

    // Plastered walls carry style code 0 in their alpha.
    let walls: Vec<[f32; 4]> = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.modelled
                .iter()
                .flat_map(|cell| cell.shells.colors.iter())
                .chain(c.buildings.colors.iter())
        })
        .filter(|colour| colour[3] == 0.0)
        .copied()
        .collect();
    assert_ne!(walls.len(), 0, "no plastered walls");
    for [r, g, b, _] in walls {
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        assert!(luminance > 0.6, "dark wall {r} {g} {b}");
    }
}

/// Chunks are built on all cores but come out as one by one: one per corridor cell, in the
/// cells' order, with progress counting up to all of them.
#[tokio::test]
async fn chunks_come_in_corridor_order_with_progress() {
    let route = route_north(&[]).await;
    let mut reports = Vec::new();
    let world = generate(
        &route,
        &mut EastwardSlope,
        &MapData::default(),
        &mut |done, total| reports.push((done, total)),
    )
    .await;

    let projection = LocalProjection::for_route(&route);
    let cells = chunks_near_route(&RoadIndex::new(&route, &projection));
    let centers: Vec<(f32, f32)> = cells
        .iter()
        .map(|&(cx, cn)| {
            (
                ((f64::from(cx) + 0.5) * CHUNK_SIZE) as f32,
                (-(f64::from(cn) + 0.5) * CHUNK_SIZE) as f32,
            )
        })
        .collect();
    let built: Vec<(f32, f32)> = world
        .chunks
        .iter()
        .map(|c| (c.center[0], c.center[2]))
        .collect();
    assert_eq!(built, centers);
    let total = cells.len();
    assert_eq!(reports.first(), Some(&(0, total)));
    assert_eq!(reports.last(), Some(&(total, total)));
    assert!(reports.windows(2).all(|pair| pair[0].0 <= pair[1].0));
}

#[test]
fn parallel_work_keeps_the_order_of_its_jobs() {
    let jobs: Vec<u64> = (0..100).collect();
    // Uneven work, so the threads finish out of order.
    let squares = in_parallel(&jobs, 8, |&n| {
        std::thread::sleep(std::time::Duration::from_micros(n * 37 % 11 * 100));
        n * n
    });
    assert_eq!(squares, jobs.iter().map(|n| n * n).collect::<Vec<_>>());
    assert_eq!(in_parallel(&[] as &[u64], 8, |&n| n), Vec::<u64>::new());
}

#[test]
#[should_panic(expected = "bad chunk")]
fn a_panic_in_parallel_work_is_passed_on() {
    in_parallel(&[1, 2, 3], 2, |&n| {
        assert!(n != 2, "bad chunk");
        n
    });
}

/// A closed eight-sided ring (a round tower) of radius `radius` around a point.
fn round(east: f64, north: f64, radius: f64) -> Vec<(f64, f64)> {
    (0..=8)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k % 8) / 8.0;
            at(east + radius * angle.cos(), north + radius * angle.sin())
        })
        .collect()
}

#[tokio::test]
async fn castles_and_lighthouses_from_the_map_get_their_models() {
    // A 22 × 16 m castle with its point inside, a round lighthouse with its point beside it,
    // and a lighthouse the map has as a point only.
    let castle = Building {
        id: 61,
        outline: rectangle(80.0, 300.0, 11.0, 8.0),
        height: None,
        levels: None,
        color: None,
    };
    let tower = Building {
        id: 62,
        outline: round(80.0, 700.0, 3.0),
        ..castle.clone()
    };
    let world = world(&MapData {
        buildings: vec![castle, tower],
        castles: vec![at(80.0, 302.0)],
        lighthouses: vec![at(80.0, 704.0), at(-60.0, 500.0)],
        ..MapData::default()
    })
    .await;

    let models = placed(&world);
    let near = |model: &Placed, (east, north): (f32, f32)| {
        (model.origin[0] - east).hypot(model.origin[2] + north) < 1.0
    };
    let castles: Vec<&Placed> = models
        .iter()
        .filter(|m| m.model.starts_with("castle_"))
        .collect();
    assert_eq!(castles.len(), 1, "one castle");
    assert!(near(castles[0], (80.0, 300.0)));
    let lighthouses: Vec<&Placed> = models
        .iter()
        .filter(|m| m.model.starts_with("lighthouse_"))
        .collect();
    assert_eq!(lighthouses.len(), 2, "two lighthouses");
    assert!(lighthouses.iter().any(|m| near(m, (80.0, 700.0))));
    assert!(
        lighthouses.iter().any(|m| near(m, (-60.0, 500.0))),
        "the lighthouse without an outline stands on its own"
    );
    // Round towers are not stretched out of round.
    for lighthouse in &lighthouses {
        assert!((length(lighthouse.x) - length(lighthouse.z)).abs() < 0.05);
    }
    // Far off, the castle's shell rises to towers and a roof well above its walls, and the
    // lone lighthouse's to its lantern.
    let castle = building_faces(&world, (80.0, 300.0), 30.0);
    assert!(
        highest(&castle) > slope_at(91.0) + 18.0,
        "{}",
        highest(&castle)
    );
    let lighthouse = building_faces(&world, (-60.0, 500.0), 10.0);
    assert!(
        highest(&lighthouse) > slope_at(-57.0) + 12.0,
        "{}",
        highest(&lighthouse)
    );
}

/// Level ground 20 m above the sea, anywhere: a coastal plain.
struct Lowland;

impl ElevationModel for Lowland {
    fn elevation(
        &mut self,
        _lat: f64,
        _lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        std::future::ready(Ok(20.0))
    }
}

/// Latitude/longitude of a point `east`/`north` metres from `origin`.
fn at_from(origin: (f64, f64), east: f64, north: f64) -> (f64, f64) {
    (
        origin.0 + north / METERS_PER_DEGREE,
        origin.1 + east / (METERS_PER_DEGREE * origin.0.to_radians().cos()),
    )
}

/// The world around a 1 km road due north from `origin` on a coastal plain, with a forest
/// east of the road and a row of houses west of it.
async fn forest_and_houses(origin: (f64, f64)) -> World {
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in 0..=100 {
        let (lat, lon) = at_from(origin, 0.0, f64::from(i) * 10.0);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>20</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let road = MapData {
        roads: vec![orqwa_osm::Road {
            class: orqwa_osm::RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: vec![at_from(origin, 0.0, -20.0), at_from(origin, 0.0, 1020.0)],
            structure: None,
        }],
        ..MapData::default()
    };
    let route = Route::from_gpx_with::<Lowland>(&xml, None, &road)
        .await
        .unwrap();
    let ring = |east: f64, north: f64, half_east: f64, half_north: f64| -> Vec<(f64, f64)> {
        [
            (-1.0, -1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
        ]
        .iter()
        .map(|&(e, n)| at_from(origin, east + e * half_east, north + n * half_north))
        .collect()
    };
    let forest = Area {
        cover: LandCover::Forest,
        outer: vec![ring(110.0, 500.0, 90.0, 90.0)],
        inner: vec![],
    };
    let houses = (0..6_i32)
        .map(|k| Building {
            id: 70 + i64::from(k),
            outline: ring(-40.0, 200.0 + 80.0 * f64::from(k), 6.0, 4.75),
            height: None,
            levels: None,
            color: None,
        })
        .collect();
    let map = MapData {
        areas: vec![forest],
        buildings: houses,
        ..MapData::default()
    };
    generate(&route, &mut Lowland, &map, &mut |_, _| {}).await
}

#[tokio::test]
async fn the_subtropics_grow_palms_and_build_houses_for_the_heat() {
    // The same forest and houses on Ishigaki (24.3° N) and on the Swiss plateau.
    let ishigaki = forest_and_houses((24.34, 124.16)).await;
    let plateau = forest_and_houses((46.95, 7.44)).await;

    // Palms among the broadleaf trees, banana plants and tropical shrubs, but no conifers.
    let count = |world: &World, kinds: &[&str]| plants_of(world, kinds).len();
    assert!(count(&ishigaki, &["palm"]) > 0, "no palms");
    assert!(count(&ishigaki, &["broadleaf"]) > 0, "no broadleaf trees");
    assert_eq!(count(&ishigaki, &["conifer", "bush"]), 0);
    // At home conifers and bushes as before, nothing tropical.
    assert!(count(&plateau, &["conifer"]) > 0, "no conifers");
    assert_eq!(count(&plateau, &["palm", "banana", "tropical_bush"]), 0);

    // Houses with flat roofs or low red-tiled ones on Ishigaki, the usual houses at home.
    let models =
        |world: &World| -> Vec<String> { placed(world).into_iter().map(|m| m.model).collect() };
    let tropical = models(&ishigaki);
    assert_eq!(tropical.len(), 6, "{tropical:?}");
    assert!(
        tropical.iter().all(|m| m.starts_with("tropical_")),
        "{tropical:?}"
    );
    let temperate = models(&plateau);
    assert_eq!(temperate.len(), 6, "{temperate:?}");
    assert!(
        temperate.iter().all(|m| m.starts_with("house_")),
        "{temperate:?}"
    );
}

#[tokio::test]
async fn a_castle_with_a_free_outline_gets_towers_windows_and_a_roof() {
    // A Schloss grown over centuries, as the map draws it: an L of 30 × 28 m, no rectangle
    // and so no model. It must not be a bare block: towers at its corners, windows in its
    // walls, and a roof over its keep rising above the walls (#137, #147).
    let outline: Vec<(f64, f64)> = [
        (0.0, 0.0),
        (30.0, 0.0),
        (30.0, 12.0),
        (12.0, 12.0),
        (12.0, 28.0),
        (0.0, 28.0),
    ]
    .into_iter()
    .map(|(east, north)| at(80.0 + east, 500.0 + north))
    .collect();
    let castle = Building {
        id: 63,
        outline,
        height: None,
        levels: None,
        color: None,
    };
    let world = world(&MapData {
        buildings: vec![castle],
        castles: vec![at(86.0, 506.0)],
        ..MapData::default()
    })
    .await;

    let faces = building_faces(&world, (95.0, 514.0), 40.0);
    assert!(!faces.is_empty(), "no castle");
    let ground = 500.0 + 0.1 * 95.0;
    let top = faces
        .iter()
        .flat_map(|f| f.corners.iter().map(|c| c[1]))
        .fold(f32::MIN, f32::max);
    assert!(
        f64::from(top) > ground + 12.0,
        "nothing rises over the walls: the castle tops out at {top}"
    );
    let styled =
        |style: buildings::Style| faces.iter().filter(|f| f.style() == style as u8).count();
    assert!(
        styled(buildings::Style::Plaster) > 0,
        "no windows in the walls"
    );
    assert!(styled(buildings::Style::Tiles) > 0, "no roof");
    // Round towers: wall faces well outside the L's outline, at its outer corners.
    let outside = faces
        .iter()
        .filter(|f| f.style() == buildings::Style::Plaster as u8)
        .map(Face::middle)
        .filter(|m| m[0] < 79.0 || m[0] > 111.0 || -m[2] < 499.0 || -m[2] > 529.0)
        .count();
    assert!(outside > 0, "no towers at the corners");
}

/// Terrain rising 60 % towards the east across the road: the ground falls away steeply on its
/// west side (left of travel, going north), 500 m at the road.
struct SteepHillside;

impl ElevationModel for SteepHillside {
    fn elevation(
        &mut self,
        _lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
        std::future::ready(Ok(500.0 + 0.6 * east))
    }
}

/// Level ground at `0` metres.
struct Level(f64);

impl ElevationModel for Level {
    fn elevation(
        &mut self,
        _lat: f64,
        _lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        std::future::ready(Ok(self.0))
    }
}

/// A 1 km road due north, level at `elevation` metres, as a plain street of the map, with the
/// given side streets and areas around it.
async fn route_north_at(elevation: f64, map: &MapData) -> Route {
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in 0..=100 {
        let (lat, lon) = at(0.0, f64::from(i) * 10.0);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{elevation}</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    Route::from_gpx_with::<Level>(&xml, None, map)
        .await
        .unwrap()
}

/// The six-metre tertiary road ridden by [`route_north_at`]. Its explicit class keeps
/// roadside and junction checks independent of unknown-width fallback choices.
fn road_north() -> orqwa_osm::Road {
    orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Street,
        details: orqwa_osm::RoadDetails {
            kind: orqwa_osm::RoadKind::Tertiary,
            ..orqwa_osm::RoadDetails::default()
        },
        line: vec![at(0.0, -20.0), at(0.0, 1020.0)],
        structure: None,
    }
}

/// Positions (absolute, metres east, height, metres north) of the props of model `name`.
fn props_of(world: &World, name: &str) -> Vec<[f64; 3]> {
    world
        .chunks
        .iter()
        .flat_map(|c| {
            c.props
                .models
                .get(name)
                .into_iter()
                .flat_map(move |buffer| {
                    buffer.as_chunks::<12>().0.iter().map(move |t| {
                        [
                            f64::from(t[3] + c.center[0]),
                            f64::from(t[7]),
                            -f64::from(t[11] + c.center[2]),
                        ]
                    })
                })
        })
        .collect()
}

/// Props of `name` along the open middle of the 1 km road (away from its ends).
fn along_middle(props: &[[f64; 3]]) -> Vec<[f64; 3]> {
    props
        .iter()
        .copied()
        .filter(|p| (100.0..900.0).contains(&p[2]))
        .collect()
}

#[tokio::test]
async fn guard_rails_line_the_side_where_the_ground_falls_away() {
    let map = MapData {
        roads: vec![road_north()],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut SteepHillside, &map, &mut |_, _| {}).await;

    let rails = along_middle(&props_of(&world, "guard_rail"));
    assert!(
        rails.iter().all(|p| p[0] < 0.0),
        "rails only on the falling (west) side: {:?}",
        rails.iter().find(|p| p[0] >= 0.0)
    );
    // One span every 4 m, unbroken: 800 m of falling ground.
    assert!(rails.len() >= 190, "{} spans", rails.len());
    let mut north: Vec<f64> = rails.iter().map(|p| p[2]).collect();
    north.sort_by(f64::total_cmp);
    assert!(
        north.windows(2).all(|w| w[1] - w[0] < 4.5),
        "no gaps in the run"
    );
    // Beside the road's edge, standing on the ground there.
    for p in &rails {
        assert!((3.2..4.2).contains(&-p[0]), "{p:?}");
    }
}

#[tokio::test]
async fn a_lower_parallel_railway_does_not_pull_the_road_verge_and_protection_down() {
    struct Cliff;
    impl ElevationModel for Cliff {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            std::future::ready(Ok(500.0 + 2.0 * east))
        }
    }
    let map = MapData {
        roads: vec![road_north()],
        railways: vec![railway(&[(-20.0, 0.0), (-20.0, 1000.0)], None)],
        ..MapData::default()
    };
    // The road lies on a chunk boundary: protection stands in the neighbouring chunk,
    // whose fine terrain must be used instead of extrapolating the owner's coarse border.
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Cliff, &map, &mut |_, _| {}).await;
    let rails = along_middle(&props_of(&world, "guard_rail"));
    assert!(
        rails.len() >= 190,
        "the cliff still needs continuous protection"
    );
    for p in &rails {
        assert!(p[1] >= 499.5, "guard rail below the ridden road: {p:?}");
        let ground = ground_at(&world, p[0] as f32, -p[2] as f32).expect("verge under the rail");
        assert!(
            (f64::from(ground) - p[1]).abs() < 0.05,
            "protection stays grounded"
        );
    }
    assert!(
        rail_bed(&world).iter().all(|v| v[1] < 470.0),
        "the separate lower railway keeps its elevation"
    );
}

#[tokio::test]
async fn guardrails_stay_at_the_upper_carriageway_beside_a_lower_hairpin_arm() {
    let mut points = Vec::new();
    for i in 0..=100 {
        points.push((0.0, f64::from(i) * 10.0));
    }
    for k in 1..12 {
        let angle = std::f64::consts::PI * f64::from(k) / 12.0;
        points.push((8.0 - 8.0 * angle.cos(), 1000.0 + 8.0 * angle.sin()));
    }
    for i in (0..=100).rev() {
        points.push((16.0, f64::from(i) * 10.0));
    }
    let mut xml = String::from("<gpx><trk><trkseg>");
    for (east, north) in points {
        let (lat, lon) = at(east, north);
        let elevation = 500.0 - east * 1.7;
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{elevation}</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let route = Route::from_gpx_with::<Level>(&xml, None, &MapData::default())
        .await
        .unwrap();
    let world = generate(
        &route,
        &mut Level(440.0),
        &MapData::default(),
        &mut |_, _| {},
    )
    .await;
    let upper: Vec<_> = along_middle(&props_of(&world, "guard_rail"))
        .into_iter()
        .filter(|p| p[0].abs() < 5.0)
        .collect();
    assert!(upper.len() > 100, "the upper arm still needs protection");
    for p in upper {
        assert!(
            p[1] > 499.5,
            "rail follows the cut bank below its road: {p:?}"
        );
        assert!(
            (3.45..3.75).contains(&p[0].abs()),
            "post stays beside the road edge: {p:?}"
        );
    }
    for section in world.road.vertices.as_chunks::<6>().0 {
        for p in [section[2], section[3]] {
            let ground = ground_at(&world, p[0], p[2]).expect("terrain under each carriageway");
            assert!(ground <= p[1] + 0.01, "the lower arm remains clear");
        }
    }
}

#[tokio::test]
async fn level_ground_gets_no_guard_rails() {
    let map = MapData {
        roads: vec![road_north()],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    for name in ["guard_rail", "guard_rail_start", "guard_rail_end"] {
        assert!(props_of(&world, name).is_empty(), "{name}");
    }
}

#[tokio::test]
async fn guard_rail_spans_meet_on_both_edges_of_a_tight_bend() {
    let mut points: Vec<_> = (0..=10)
        .map(|i| (0.0, f64::from(i) * 10.0, 500.0))
        .collect();
    points.extend((1..=24).map(|i| {
        let angle = std::f64::consts::PI * f64::from(i) / 24.0;
        (12.0 - 12.0 * angle.cos(), 100.0 + 12.0 * angle.sin(), 500.0)
    }));
    points.extend((0..10).rev().map(|i| (24.0, f64::from(i) * 10.0, 500.0)));
    let mut map = MapData::default();
    let route = route_through(&points, &mut map).await;
    let world = generate(&route, &mut Level(475.0), &map, &mut |_, _| {}).await;
    let ends: Vec<[f64; 3]> = world
        .chunks
        .iter()
        .flat_map(|chunk| {
            chunk
                .props
                .models
                .get("guard_rail")
                .into_iter()
                .flat_map(|buffer| {
                    buffer.as_chunks::<12>().0.iter().flat_map(|t| {
                        [-2.0, 2.0].map(|z| {
                            [
                                f64::from(t[3] + chunk.center[0] + t[2] * z),
                                f64::from(t[7] + chunk.center[1] + t[6] * z),
                                -f64::from(t[11] + chunk.center[2] + t[10] * z),
                            ]
                        })
                    })
                })
        })
        .collect();
    let mut checked = [0, 0];
    for (i, end) in ends.iter().enumerate().filter(|(_, p)| p[2] > 104.0) {
        let radius = (end[0] - 12.0).hypot(end[2] - 100.0);
        assert!(
            (radius - 12.0).abs() >= 2.85,
            "rail stays outside the carriageway on the bend: {end:?}"
        );
        checked[usize::from(radius > 12.0)] += 1;
        let gap = ends
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, other)| {
                ((end[0] - other[0]).powi(2)
                    + (end[1] - other[1]).powi(2)
                    + (end[2] - other[2]).powi(2))
                .sqrt()
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            gap < 0.03,
            "adjacent blades meet on inner and outer bends: {gap} m at {end:?}"
        );
    }
    assert!(
        checked.into_iter().all(|n| n >= 4),
        "both edges protected: {checked:?}"
    );
}

#[tokio::test]
async fn curved_bridge_protection_leaves_the_wearing_surface_open() {
    let points = [
        (0.0, 0.0, 500.0),
        (0.0, 80.0, 500.0),
        (0.0, 100.0, 502.0),
        (2.0, 108.0, 503.0),
        (12.0, 112.0, 504.0),
        (20.0, 108.0, 505.0),
        (24.0, 100.0, 506.0),
        (24.0, 80.0, 506.0),
        (24.0, 0.0, 506.0),
    ];
    let mut map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: points[1..8].iter().map(|&(e, n, _)| at(e, n)).collect(),
            structure: Some(StructureKind::Bridge),
        }],
        ..MapData::default()
    };
    let mut xml = String::from("<gpx><trk><trkseg>");
    for &(east, north, elevation) in &points {
        let (lat, lon) = at(east, north);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{elevation}</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    for pair in [points[..2].to_vec(), points[7..].to_vec()] {
        map.roads.push(orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: pair.iter().map(|&(e, n, _)| at(e, n)).collect(),
            structure: None,
        });
    }
    let route = Route::from_gpx_with::<Level>(&xml, None, &map)
        .await
        .unwrap();
    let world = generate(&route, &mut Level(475.0), &map, &mut |_, _| {}).await;
    let pavement: Vec<_> = world
        .road
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|tri| {
            tri.iter()
                .all(|&i| matches!(i as usize % road::RING, 2 | 3))
        })
        .map(|tri| tri.map(|i| world.road.vertices[i as usize]))
        .collect();
    let steel = palette::srgb("structure.steel", 0.0);
    let mut checked = 0;
    for (&v, &color) in world
        .structures
        .vertices
        .iter()
        .zip(&world.structures.colors)
    {
        if color != steel {
            continue;
        }
        checked += 1;
        assert!(
            pavement
                .iter()
                .all(|&t| triangle_height(t, v[0], v[2]).is_none()),
            "curved railings and upright posts stay outside the carriageway: {v:?}"
        );
    }
    assert!(checked > 100, "curved bridge has open steel protection");
    for t in pavement {
        let middle = [0, 1, 2].map(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
        for face in triangles(&world.structures) {
            if let Some(height) = triangle_height(face, middle[0], middle[2]) {
                assert!(
                    height < middle[1] - 0.005,
                    "bridge concrete must not cover curved asphalt: {height} at {middle:?}"
                );
            }
        }
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // compare the ground junction and elevated crossing together
async fn bridge_tags_through_a_roundabout_do_not_fence_off_its_interior() {
    let ring = (0..=48)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from(i) / 48.0;
            at(20.0 + 20.0 * angle.cos(), 480.0 + 20.0 * angle.sin())
        })
        .collect();
    let map = MapData {
        roads: vec![orqwa_osm::Road {
            class: RoadClass::Street,
            details: orqwa_osm::RoadDetails::default(),
            line: ring,
            structure: None,
        }],
        ..MapData::default()
    };
    let route = route_north(&[]).await;
    let projection = LocalProjection::for_route(&route);
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let islands = roundabouts::islands(&map, &projection);
    assert_eq!(islands.len(), 1);
    let rails = road::RoadIndex::from_lines(&[]);
    let streets = streets::lines(&map, &projection, &mut Level(500.0)).await;
    let ridden = RoadIndex::new(&route, &projection);
    let levels = streets::Levels::new(
        &streets,
        &appearance::Index::new(&map, &projection),
        &ridden,
        &rails,
    );
    let ground = foundations::Ground::of(&world.chunks);
    let mut line: Vec<_> = (0..=40)
        .map(|i| (0.0, 400.0 + f64::from(i) * 2.0))
        .collect();
    line.extend((1..=48).map(|i| {
        let angle = std::f64::consts::PI * f64::from(i) / 48.0;
        (20.0 - 20.0 * angle.cos(), 480.0 + 20.0 * angle.sin())
    }));
    line.extend((0..40).rev().map(|i| (40.0, 400.0 + f64::from(i) * 2.0)));
    for height in [500.0, 508.0] {
        let mut distance = 0.0;
        let points = line
            .iter()
            .enumerate()
            .map(|(i, &point)| {
                if i > 0 {
                    distance += (point.0 - line[i - 1].0).hypot(point.1 - line[i - 1].1);
                }
                road::Centre {
                    position: point,
                    elevation: height,
                    distance,
                    surface: Surface::Bridge,
                    mapped: true,
                }
            })
            .collect();
        let road = road::RoadIndex::from_lines(&[points]);
        let below = structures::Below::new(&road, &rails, &streets, &islands);
        let shapers = Shapers {
            road: &road,
            rails: &rails,
            streets: &levels,
            portals: &[],
            hollows: &[],
            hills: true,
        };
        let mesh = structures::build_all(
            (&road, &rails),
            &below,
            &shapers,
            &ground,
            &projection,
            &mut Level(500.0),
        )
        .await;
        let mut inside = 0;
        let mut outside = 0;
        let steel = palette::srgb("structure.steel", 0.0);
        for (&v, &c) in mesh.vertices.iter().zip(&mesh.colors) {
            if c != steel {
                continue;
            }
            let radius = (v[0] - 20.0).hypot(-v[2] - 480.0);
            if radius < 21.0 {
                inside += 1;
            } else if radius > 24.0 {
                outside += 1;
            }
        }
        assert!(
            outside > 100,
            "bridge outside the roundabout stays protected"
        );
        if height == 500.0 {
            assert_eq!(
                inside, 0,
                "no fence follows the GPX inside the at-grade roundabout"
            );
        } else {
            assert!(inside > 100, "a separate overpass keeps both railings");
        }
    }
}

#[tokio::test]
async fn riding_back_over_a_bridge_does_not_add_a_second_set_of_fences() {
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in (0..=100).chain((0..100).rev()) {
        let (lat, lon) = at(0.0, f64::from(i) * 10.0);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>500</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let map = MapData {
        roads: vec![
            orqwa_osm::Road {
                class: RoadClass::Street,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.0, 0.0), at(0.0, 300.0)],
                structure: None,
            },
            orqwa_osm::Road {
                class: RoadClass::Street,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.0, 300.0), at(0.0, 700.0)],
                structure: Some(StructureKind::Bridge),
            },
            orqwa_osm::Road {
                class: RoadClass::Street,
                details: orqwa_osm::RoadDetails::default(),
                line: vec![at(0.0, 700.0), at(0.0, 1000.0)],
                structure: None,
            },
        ],
        ..MapData::default()
    };
    let route = Route::from_gpx_with::<Level>(&xml, None, &map)
        .await
        .unwrap();
    let projection = LocalProjection::for_route(&route);
    let road = road::RoadIndex::new(&route, &projection);
    let bridges: Vec<_> = road
        .structure_runs()
        .into_iter()
        .filter(|(s, _)| *s == Surface::Bridge)
        .collect();
    assert_eq!(
        bridges.len(),
        1,
        "one physical bridge, ridden twice, has one deck and one set of barriers"
    );
    assert!(
        bridges[0].1.len() > 100,
        "the complete first bridge remains"
    );
}

#[tokio::test]
async fn bridge_fences_cannot_cut_a_parallel_road_joining_the_carriageway() {
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let side = orqwa_osm::Road {
        class: RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![
            at(7.0, 450.0),
            at(4.0, 480.0),
            at(4.0, 520.0),
            at(10.0, 550.0),
        ],
        structure: None,
    };
    let world = generate(
        &route,
        &mut Level(500.0),
        &MapData {
            roads: vec![side],
            ..MapData::default()
        },
        &mut |_, _| {},
    )
    .await;
    let steel = palette::srgb("structure.steel", 0.0);
    let rails: Vec<_> = world
        .structures
        .indices
        .iter()
        .map(|&i| i as usize)
        .filter(|&i| world.structures.colors[i] == steel)
        .map(|i| world.structures.vertices[i])
        .collect();
    assert!(
        rails
            .iter()
            .any(|v| v[0] < -3.0 && (v[2] + 500.0).abs() < 10.0),
        "opposite bridge edge remains protected"
    );
    assert!(
        rails
            .iter()
            .any(|v| v[0] > 3.0 && (v[2] + 400.0).abs() < 10.0),
        "protection remains outside the junction"
    );
    assert!(
        rails
            .iter()
            .all(|v| !(v[0] > 1.25 && v[0] < 6.75 && (v[2] + 500.0).abs() < 15.0)),
        "no beam, post or kerb may cut the joining carriageway"
    );
    assert!(
        world.structures.indices.iter().all(|&i| {
            let v = world.structures.vertices[i as usize];
            !(v[0] > 1.25 && v[0] < 6.75 && (v[2] + 500.0).abs() < 15.0 && v[1] > 500.05)
        }),
        "concrete kerbs and parapets must also leave the carriageway open"
    );
}

#[tokio::test]
async fn mapped_bridge_fences_leave_parallel_joining_pavement_open() {
    let street = |line: Vec<(f64, f64)>, structure| orqwa_osm::Road {
        class: RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: line.into_iter().map(|(e, n)| at(e, n)).collect(),
        structure,
    };
    let map = MapData {
        roads: vec![
            street(
                vec![(0.0, 500.0), (80.0, 500.0)],
                Some(StructureKind::Bridge),
            ),
            street(vec![(15.0, 503.0), (55.0, 503.0)], None),
        ],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Level(500.0),
        &map,
        &mut |_, _| {},
    )
    .await;
    let steel = palette::srgb("structure.steel", 0.0);
    let rails: Vec<_> = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.streets
                .indices
                .iter()
                .map(|&i| i as usize)
                .filter(|&i| c.streets.colors[i] == steel)
                .map(|i| {
                    [
                        c.streets.vertices[i][0] + c.center[0],
                        c.streets.vertices[i][1],
                        c.streets.vertices[i][2] + c.center[2],
                    ]
                })
        })
        .collect();
    assert!(
        rails
            .iter()
            .any(|v| v[0] > 25.0 && v[0] < 45.0 && v[2] > -500.0),
        "the opposite edge remains protected"
    );
    assert!(
        rails
            .iter()
            .all(|v| !(v[0] > 20.0 && v[0] < 50.0 && v[2] < -500.25 && v[2] > -505.75)),
        "mapped bridge barriers cannot divide the actual joining pavement"
    );
}

#[tokio::test]
async fn roadside_fence_models_cannot_span_the_carriageway() {
    let mut world = generate(
        &route_north(&[]).await,
        &mut Level(500.0),
        &MapData::default(),
        &mut |_, _| {},
    )
    .await;
    let pavement =
        pavement::Pavement::new(&world, world.chunks.iter().map(|c| c.tracks.indices.len()));
    let chunk = world
        .chunks
        .iter_mut()
        .find(|c| c.center[0].abs() <= 240.0 && (c.center[2] + 500.0).abs() <= 240.0)
        .unwrap();
    let center = chunk.center;
    let mut buffer = Vec::new();
    for (east, height) in [(0.0, 499.85), (8.0, 499.85), (0.0, 506.0)] {
        buffer.extend([
            0.0,
            0.0,
            2.0,
            east - center[0],
            0.0,
            1.0,
            0.0,
            height,
            1.0,
            0.0,
            0.0,
            -500.0 - center[2],
        ]);
    }
    for name in ["guard_rail", "fence"] {
        chunk.props.models.insert(name.into(), buffer.clone());
    }
    for chunk in &mut world.chunks {
        props::clear_fences(chunk, &pavement);
    }
    let chunk = world.chunks.iter().find(|c| c.center == center).unwrap();
    for name in ["guard_rail", "fence"] {
        let remaining = &chunk.props.models[name];
        assert_eq!(
            remaining.len(),
            24,
            "remove the crossing span while retaining the off-road and higher spans"
        );
        assert!(
            remaining
                .as_chunks::<12>()
                .0
                .iter()
                .all(|t| t[3] + center[0] > 7.0 || t[7] > 505.0)
        );
    }
}

#[tokio::test]
async fn delineators_mark_country_roads_but_not_towns() {
    // A village on the road's second half.
    let village = Area {
        cover: LandCover::Residential,
        outer: vec![rectangle(0.0, 750.0, 200.0, 250.0)],
        inner: vec![],
    };
    let map = MapData {
        roads: vec![road_north()],
        areas: vec![village],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let posts = props_of(&world, "delineator");
    let country: Vec<[f64; 3]> = posts
        .iter()
        .copied()
        .filter(|p| (100.0..450.0).contains(&p[2]))
        .collect();
    // Every 50 m, both sides.
    let (west, east): (Vec<[f64; 3]>, Vec<[f64; 3]>) = country.iter().partition(|p| p[0] < 0.0);
    assert!((6..=8).contains(&west.len()), "{} west", west.len());
    assert_eq!(west.len(), east.len());
    let mut north: Vec<f64> = east.iter().map(|p| p[2]).collect();
    north.sort_by(f64::total_cmp);
    assert!(north.windows(2).all(|w| (w[1] - w[0] - 50.0).abs() < 1.0));
    assert!(
        posts.iter().all(|p| !(520.0..980.0).contains(&p[2])),
        "none in the village"
    );
    assert!(props_of(&world, "snow_pole").is_empty(), "too low for snow");
}

#[tokio::test]
async fn snow_poles_replace_delineators_on_mountain_roads() {
    let map = MapData {
        roads: vec![road_north()],
        ..MapData::default()
    };
    let route = route_north_at(1800.0, &map).await;
    let world = generate(&route, &mut Level(1800.0), &map, &mut |_, _| {}).await;

    let posts = props_of(&world, "delineator");
    assert!(posts.is_empty(), "no delineators up high: {posts:?}");
    let poles = along_middle(&props_of(&world, "snow_pole"));
    // Every 25 m on both sides of 800 m.
    assert!((60..=68).contains(&poles.len()), "{} poles", poles.len());
    assert!(poles.iter().all(|p| (4.0..4.5).contains(&p[0].abs())));
}

#[tokio::test]
async fn nothing_stands_where_a_street_joins_the_road() {
    let side_street = orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![at(0.0, 300.0), at(200.0, 300.0)],
        structure: None,
    };
    let map = MapData {
        roads: vec![road_north(), side_street],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let posts = props_of(&world, "delineator");
    assert!(
        !posts
            .iter()
            .any(|p| p[0] > 0.0 && (p[2] - 300.0).abs() < 8.0),
        "the mouth is clear"
    );
    assert!(
        posts
            .iter()
            .any(|p| p[0] < 0.0 && (p[2] - 300.0).abs() < 1.0),
        "the other side keeps its post"
    );
}

#[tokio::test]
async fn towns_get_lamps_benches_and_shelters_with_room_to_stand() {
    // A village over the road's middle, a house beside the road in it and a side street.
    let village = Area {
        cover: LandCover::Residential,
        outer: vec![rectangle(0.0, 500.0, 200.0, 400.0)],
        inner: vec![],
    };
    let house = Building {
        id: 7,
        outline: rectangle(9.0, 420.0, 4.0, 6.0),
        height: None,
        levels: None,
        color: None,
    };
    let side_street = orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![at(0.0, 600.0), at(-200.0, 600.0)],
        structure: None,
    };
    let map = MapData {
        roads: vec![road_north(), side_street],
        areas: vec![village],
        buildings: vec![house],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let lamps = props_of(&world, "street_lamp");
    let in_town = |p: &&[f64; 3]| (150.0..850.0).contains(&p[2]);
    let (east, west): (Vec<[f64; 3]>, Vec<[f64; 3]>) =
        lamps.iter().filter(in_town).partition(|p| p[0] > 0.0);
    // Every 60 m on each side, staggered, through 700 m of village.
    assert!((9..=13).contains(&east.len()), "{} east", east.len());
    assert!((9..=13).contains(&west.len()), "{} west", west.len());
    assert!(
        lamps.iter().all(|p| (100.0..900.0).contains(&p[2])),
        "no lamps outside the village"
    );
    for p in lamps
        .iter()
        .chain(&props_of(&world, "bench"))
        .chain(&props_of(&world, "bus_shelter"))
    {
        assert!(
            !((p[0] - 9.0).abs() < 5.5 && (p[2] - 420.0).abs() < 7.5),
            "nothing inside the house: {p:?}"
        );
        assert!(
            !(p[0] < 0.0 && (p[2] - 600.0).abs() < 4.0),
            "nothing in the side street's mouth: {p:?}"
        );
    }
    let shelters = props_of(&world, "bus_shelter");
    assert!(
        (1..=2).contains(&shelters.len()) && shelters.iter().all(|p| p[0] > 0.0),
        "a shelter every 400 m on the right: {shelters:?}"
    );
    let benches = props_of(&world, "bench");
    assert!(
        (4..=7).contains(&benches.len()),
        "regular seating along 800 m of town: {benches:?}"
    );
    assert!(benches.iter().any(|p| p[0] < 0.0) && benches.iter().any(|p| p[0] > 0.0));
    for [east, height, north] in &benches {
        let sidewalk = height_on(&world, |chunk| &chunk.urban, *east as f32, -*north as f32)
            .expect("bench on a drawn sidewalk");
        assert!((height - f64::from(sidewalk)).abs() < 0.01);
        assert!(east.abs() > 4.4, "leave the inner sidewalk open");
    }
    assert_eq!(
        benches.len(),
        props_of(&world, "bin").len(),
        "a bin beside every bench"
    );
    assert!(
        props_of(&world, "delineator").iter().all(|p| !in_town(&p)),
        "town props, not country posts"
    );
}

#[tokio::test]
async fn the_countryside_gets_fences_bales_and_crosses_but_not_towns() {
    // Fields east of the road, meadow west, a village at the north end, lanes joining.
    let fields = Area {
        cover: LandCover::Farmland,
        outer: vec![rectangle(150.0, 400.0, 145.0, 400.0)],
        inner: vec![],
    };
    let meadow = Area {
        cover: LandCover::Meadow,
        outer: vec![rectangle(-150.0, 400.0, 145.0, 400.0)],
        inner: vec![],
    };
    let village = Area {
        cover: LandCover::Residential,
        outer: vec![rectangle(0.0, 950.0, 300.0, 150.0)],
        inner: vec![],
    };
    let mut roads = vec![road_north()];
    roads.extend((1..8).map(|k| orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Track,
        details: orqwa_osm::RoadDetails::default(),
        line: vec![
            at(0.0, 100.0 * f64::from(k)),
            at(-150.0, 100.0 * f64::from(k)),
        ],
        structure: None,
    }));
    let map = MapData {
        roads,
        areas: vec![fields, meadow, village],
        ..MapData::default()
    };
    let route = route_north_at(500.0, &map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let fences = props_of(&world, "fence");
    assert!(fences.len() > 40, "{} fence spans", fences.len());
    assert!(
        fences.iter().all(|p| p[2] < 800.0),
        "no pasture fences in the village"
    );
    for p in &fences {
        assert!((5.5..6.5).contains(&p[0].abs()), "beside the road: {p:?}");
    }
    let bales: Vec<[f64; 3]> = props_of(&world, "hay_bale")
        .into_iter()
        .chain(props_of(&world, "hay_bale_wrapped"))
        .collect();
    assert!(!bales.is_empty(), "bales in the fields");
    assert!(
        bales.iter().all(|p| p[0] > 10.0 && p[2] < 800.0),
        "only in the fields, off the road: {bales:?}"
    );
    let crosses = props_of(&world, "wayside_cross");
    assert!(crosses.len() <= 7, "{crosses:?}");
    assert!(crosses.iter().all(|p| p[0] < 0.0), "on the lanes' side");
}

/// A route through `points` (metres east, north, elevation), its own way on the map.
async fn route_through(points: &[(f64, f64, f64)], map: &mut MapData) -> Route {
    let mut xml = String::from("<gpx><trk><trkseg>");
    for &(east, north, elevation) in points {
        let (lat, lon) = at(east, north);
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{elevation}</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    map.roads.push(orqwa_osm::Road {
        class: orqwa_osm::RoadClass::Street,
        details: orqwa_osm::RoadDetails::default(),
        line: points.iter().map(|&(e, n, _)| at(e, n)).collect(),
        structure: None,
    });
    Route::from_gpx_with::<Level>(&xml, None, map)
        .await
        .unwrap()
}

/// The labels on the signs, with where they stand (absolute, metres east, height, north).
fn labels_of(world: &World) -> Vec<(String, [f64; 3])> {
    world
        .chunks
        .iter()
        .flat_map(|c| {
            c.props.labels.iter().map(move |label| {
                let t = label.transform;
                (
                    label.text.clone(),
                    [
                        f64::from(t[3] + c.center[0]),
                        f64::from(t[7]),
                        -f64::from(t[11] + c.center[2]),
                    ],
                )
            })
        })
        .collect()
}

#[tokio::test]
async fn a_village_is_signed_with_its_name_where_the_road_enters_and_leaves() {
    let village = Area {
        cover: LandCover::Residential,
        outer: vec![rectangle(0.0, 500.0, 200.0, 150.0)],
        inner: vec![],
    };
    let mut map = MapData {
        areas: vec![village],
        places: vec![orqwa_osm::Place {
            name: "Wabern".to_owned(),
            latin: "Wabern".to_owned(),
            kind: orqwa_osm::PlaceKind::Village,
            rank: 1,
            position: at(30.0, 520.0),
        }],
        ..MapData::default()
    };
    let points: Vec<(f64, f64, f64)> = (0..=100)
        .map(|i| (0.0, f64::from(i) * 10.0, 500.0))
        .collect();
    let route = route_through(&points, &mut map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let entry = props_of(&world, "sign_place");
    let end = props_of(&world, "sign_place_end");
    assert_eq!(entry.len(), 1, "{entry:?}");
    assert_eq!(end.len(), 1, "{end:?}");
    assert!(
        (340.0..370.0).contains(&entry[0][2]),
        "at the village's edge: {entry:?}"
    );
    assert!(
        (640.0..670.0).contains(&end[0][2]),
        "where it ends: {end:?}"
    );
    assert!(
        entry[0][0] > 0.0 && end[0][0] > 0.0,
        "on the right of travel"
    );
    let labels = labels_of(&world);
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(
        labels.iter().all(|(text, _)| text == "Wabern"),
        "{labels:?}"
    );
    // The ground beside the road lies a little below it (`ROAD_SINK`).
    assert!(
        labels
            .iter()
            .all(|(_, p)| (1.6..2.0).contains(&(p[1] - 500.0))),
        "on the signs' faces: {labels:?}"
    );
}

#[tokio::test]
async fn hairpins_and_steep_stretches_are_warned_of() {
    // North 600 m climbing at 10 %, a hairpin to the left (radius 20 m), back south level.
    let mut points: Vec<(f64, f64, f64)> = (0..=60)
        .map(|i| (0.0, f64::from(i) * 10.0, 500.0 + f64::from(i)))
        .collect();
    for k in 1..12 {
        let a = std::f64::consts::PI * f64::from(k) / 12.0;
        points.push((-20.0 + 20.0 * a.cos(), 600.0 + 20.0 * a.sin(), 560.0));
    }
    points.extend((0..=60).map(|i| (-40.0, 600.0 - f64::from(i) * 10.0, 560.0)));
    let mut map = MapData::default();
    let route = route_through(&points, &mut map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let bends = props_of(&world, "sign_bend_left");
    assert_eq!(bends.len(), 1, "one sign before the hairpin: {bends:?}");
    assert!(
        (480.0..560.0).contains(&bends[0][2]) && bends[0][0] > 0.0,
        "on the right, before the bend: {bends:?}"
    );
    let wrong = props_of(&world, "sign_bend_right");
    assert!(wrong.is_empty(), "no right-hand bend: {wrong:?}");
    let grades = props_of(&world, "sign_grade");
    assert_eq!(grades.len(), 1, "one sign before the climb: {grades:?}");
    let labels = labels_of(&world);
    assert!(labels.iter().any(|(text, _)| text == "10 %"), "{labels:?}");
}

#[tokio::test]
async fn power_lines_run_on_poles_with_wires_from_pole_to_pole() {
    // 6 km of country road due north.
    let points: Vec<(f64, f64, f64)> = (0..=600)
        .map(|i| (0.0, f64::from(i) * 10.0, 500.0))
        .collect();
    let mut map = MapData::default();
    let route = route_through(&points, &mut map).await;
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;

    let poles = props_of(&world, "power_pole");
    assert!(
        poles.len() >= 10,
        "some stretches have a line: {}",
        poles.len()
    );
    for p in &poles {
        assert!((8.5..9.5).contains(&p[0].abs()), "beside the road: {p:?}");
    }
    // Poles of one line stand 50 m apart on one side.
    let mut east: Vec<f64> = poles.iter().filter(|p| p[0] > 0.0).map(|p| p[2]).collect();
    east.sort_by(f64::total_cmp);
    assert!(
        east.windows(2)
            .all(|w| (w[1] - w[0] - 50.0).abs() < 1.0 || w[1] - w[0] > 99.0),
        "every 50 m: {east:?}"
    );
    let wires: usize = world
        .chunks
        .iter()
        .map(|c| c.props.wires.indices.len())
        .sum();
    assert!(wires > 0, "wires between the poles");
    for chunk in &world.chunks {
        for vertex in &chunk.props.wires.vertices {
            let height = f64::from(vertex[1]) - 500.0;
            assert!(
                (6.5..8.5).contains(&height),
                "wires up on the poles, sagging a little: {height}"
            );
        }
    }
}
