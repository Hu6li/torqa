use super::*;
use orqwa_osm::{Road, RoadDetails, RoadKind, RoadSurface};

struct Hillside(f64);

impl ElevationModel for Hillside {
    fn elevation(
        &mut self,
        _lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
        std::future::ready(Ok(500.0 + self.0 * east))
    }
}

/// Formed ways of each kind on both sides of the ride, within its formed reach, and a path.
fn roads() -> MapData {
    let specs = [
        (
            110.0,
            RoadClass::Major,
            RoadKind::Primary,
            RoadSurface::Paved,
        ),
        (
            55.0,
            RoadClass::Service,
            RoadKind::Service,
            RoadSurface::Unpaved,
        ),
        (
            -55.0,
            RoadClass::Track,
            RoadKind::Track,
            RoadSurface::Unpaved,
        ),
        (
            -110.0,
            RoadClass::Path,
            RoadKind::Cycleway,
            RoadSurface::Paved,
        ),
        (170.0, RoadClass::Path, RoadKind::Path, RoadSurface::Paved),
    ];
    MapData {
        roads: specs
            .into_iter()
            .map(|(east, class, kind, surface)| Road {
                class,
                details: RoadDetails {
                    kind,
                    surface,
                    ..RoadDetails::default()
                },
                line: vec![at(east, 100.0), at(east, 900.0)],
                structure: None,
            })
            .collect(),
        ..MapData::default()
    }
}

#[tokio::test]
async fn side_road_earthworks_cannot_close_either_ridden_tunnel_approach() {
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![
            road_north(),
            Road {
                class: RoadClass::Service,
                details: RoadDetails {
                    kind: RoadKind::Service,
                    ..RoadDetails::default()
                },
                line: vec![at(12.0, 200.0), at(12.0, 800.0)],
                structure: None,
            },
        ],
        ..MapData::default()
    };
    let world = generate(&route, &mut Level(500.0), &map, &mut |_, _| {}).await;
    let inside = palette::srgb("structure.tunnel", 0.0);
    let (entrance, exit) = world
        .structures
        .vertices
        .iter()
        .zip(&world.structures.colors)
        .filter(|(_, color)| **color == inside)
        .fold((f32::MAX, f32::MIN), |(low, high), (v, _)| {
            (low.min(-v[2]), high.max(-v[2]))
        });
    for (mouth, outward) in [(entrance, -1.0), (exit, 1.0)] {
        // Beyond the portal's short clipping volume, the approach itself must cut
        // through the hill. A clear tube alone does not ensure a usable entrance.
        for step in 8..=35 {
            let north = mouth + outward * step as f32;
            for east in [-2.9, 0.0, 2.9] {
                for height in [
                    ground_at(&world, east, -north),
                    street_at(&world, east, -north),
                ]
                .into_iter()
                .flatten()
                {
                    assert!(
                        height < 500.1,
                        "side-road earthworks block the ridden tunnel approach at {east}, {north}: {height}"
                    );
                }
            }
        }
        let north = mouth + outward * 12.0;
        for step in -24..32 {
            let east = step as f32 * 0.5;
            let a = ground_at(&world, east, -north).unwrap();
            let b = ground_at(&world, east + 0.5, -north).unwrap();
            assert!(
                (a - b).abs() < 0.8,
                "the approach cutting ends in an abrupt wall on flat terrain at {east}, {north}: {a}, {b}"
            );
        }
    }
}

#[tokio::test]
async fn a_close_parallel_service_road_keeps_its_bed_beside_the_ridden_cutting() {
    let map = MapData {
        roads: vec![
            road_north(),
            Road {
                class: RoadClass::Service,
                details: RoadDetails {
                    kind: RoadKind::Service,
                    ..RoadDetails::default()
                },
                line: vec![at(9.0, 100.0), at(9.0, 900.0)],
                structure: None,
            },
        ],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(0.6),
        &map,
        &mut |_, _| {},
    )
    .await;
    for north in [300.0, 479.9, 480.1, 700.0] {
        let left = street_at(&world, 7.4, -north).unwrap();
        let right = street_at(&world, 10.6, -north).unwrap();
        assert!(
            (left - right).abs() < 0.04,
            "folded service road at {north}: {left}, {right}"
        );
        assert!((left - 505.4).abs() < 0.15);
        for east in [-2.9, 0.0, 2.9] {
            assert!(
                ground_at(&world, east, -north).unwrap() < 500.01,
                "raised ground through the ridden pavement"
            );
        }
    }
}

#[tokio::test]
async fn a_suppressed_ridden_copy_cannot_own_a_nearby_service_roads_terrain() {
    let mut ridden = road_north();
    ridden.class = RoadClass::Major;
    ridden.details.kind = RoadKind::Primary;
    ridden.line = vec![at(2.5, -20.0), at(2.5, 1020.0)];
    let service = Road {
        class: RoadClass::Service,
        details: RoadDetails {
            kind: RoadKind::Service,
            ..RoadDetails::default()
        },
        line: vec![at(7.0, 100.0), at(7.0, 900.0)],
        structure: None,
    };
    let map = MapData {
        roads: vec![ridden, service],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(0.6),
        &map,
        &mut |_, _| {},
    )
    .await;
    for north in [300.0, 479.9, 480.1, 700.0] {
        let left = street_at(&world, 5.4, -north).unwrap();
        let right = street_at(&world, 8.6, -north).unwrap();
        assert!(
            (left - right).abs() < 0.04,
            "side road folded over a suppressed copy at {north}: {left}, {right}"
        );
        assert!((left - 504.2).abs() < 0.15);
        for east in [-3.4, 0.0, 3.4] {
            assert!(
                ground_at(&world, east, -north).unwrap() < 500.01,
                "side road terrain covers the ridden carriageway"
            );
        }
    }
}

#[tokio::test]
async fn joining_a_sloping_road_does_not_drape_over_its_downhill_segment_ends() {
    struct Slope;
    impl ElevationModel for Slope {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46_f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(500.0 + 0.6 * east - 0.053 * north))
        }
    }
    let mut main = road_north();
    main.class = RoadClass::Major;
    main.details.kind = RoadKind::Secondary;
    let side = Road {
        class: RoadClass::Service,
        details: RoadDetails {
            kind: RoadKind::Service,
            ..RoadDetails::default()
        },
        line: vec![at(4.5, 100.0), at(4.5, 900.0)],
        structure: None,
    };
    let map = MapData {
        roads: vec![main, side],
        ..MapData::default()
    };
    let mut xml = String::from("<gpx><trk><trkseg>");
    for i in 0..=100 {
        let north = f64::from(i) * 10.0;
        let (lat, lon) = at(0.0, north);
        write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>{}</ele></trkpt>"#,
            500.0 - 0.053 * north
        )
        .unwrap();
    }
    xml.push_str("</trkseg></trk></gpx>");
    let route = Route::from_gpx_with::<Slope>(&xml, None, &map)
        .await
        .unwrap();
    let world = generate(&route, &mut Slope, &map, &mut |_, _| {}).await;
    let alone = MapData {
        roads: map.roads[..1].to_vec(),
        ..MapData::default()
    };
    let without = generate(&route, &mut Slope, &alone, &mut |_, _| {}).await;
    for north in [300.0, 479.9, 480.1, 700.0] {
        // Within the ridden verge the joining road follows the ridden road's own ground,
        // which lies a little lower towards the road along a slope, without a fold of its own.
        for east in [3.4, 4.5, 6.1] {
            let (street, ground) = (
                street_at(&world, east, -north).unwrap(),
                ground_at(&world, east, -north).unwrap(),
            );
            let lift = street - ground;
            assert!(
                (0.0..0.1).contains(&lift),
                "the joining road leaves its ground at {east}, {north}: {lift}"
            );
        }
        // The ridden road keeps its own ground: the joining road meets it. Finer terrain
        // pieces near the join only trace that ground more closely than coarse ones.
        for east in [-2.9, 0.0, 2.9] {
            let (with, alone) = (
                ground_at(&world, east, -north).unwrap(),
                ground_at(&without, east, -north).unwrap(),
            );
            assert!(
                (with - alone).abs() < 0.1,
                "a joining road moved the ridden road's ground at {east}, {north}: {with}, {alone}"
            );
        }
    }
}

#[tokio::test]
async fn all_formed_ways_have_level_beds_across_slopes_and_chunk_edges() {
    for slope in [0.25, 1.1] {
        let map = roads();
        let world = generate(
            &route_north(&[]).await,
            &mut Hillside(slope),
            &map,
            &mut |_, _| {},
        )
        .await;
        for (east, half, paved) in [
            (110.0, 3.5, true),
            (55.0, 1.75, false),
            (-55.0, 1.4, false),
            (-110.0, 1.25, true),
        ] {
            for north in [300.0, 478.0, 479.9, 480.1, 482.0, 700.0] {
                let height = |x| {
                    if paved {
                        street_at(&world, x, -north)
                    } else {
                        height_on(&world, |c| &c.tracks, x, -north)
                    }
                    .expect("road surface")
                };
                let (left, right) = (height(east - half + 0.15), height(east + half - 0.15));
                assert!(
                    (left - right).abs() < 0.025,
                    "tilted road at {east}, {north}: {left}, {right}"
                );
                assert!((f64::from(left) - (500.0 + slope * f64::from(east))).abs() < 0.15);
                assert!(height(east) > ground_at(&world, east, -north).unwrap());
            }
        }
        let path = |x| street_at(&world, x, -500.0).expect("small paved path");
        assert!(
            path(170.65) - path(169.35) > slope as f32,
            "small paths retain the hillside"
        );
        let walls: Vec<_> = world
            .chunks
            .iter()
            .flat_map(|c| c.streets.colors.iter().zip(&c.streets.normals))
            .filter(|(c, _)| c[3] == 0.0)
            .collect();
        if slope < 0.5 {
            assert!(
                walls.is_empty(),
                "modest slopes use earthworks without concrete"
            );
        } else {
            assert!(walls.len() > 100, "steep banks need retaining faces");
            assert!(walls.iter().all(|(_, n)| n[1] < 0.53));
        }
        for chunk in &world.chunks {
            assert_valid(&chunk.streets);
            assert_valid(&chunk.tracks);
            assert_faces_follow_normals(&chunk.streets);
        }
    }
}

#[tokio::test]
async fn a_service_road_beside_a_railway_keeps_its_own_bed() {
    let service = Road {
        class: RoadClass::Service,
        details: RoadDetails {
            kind: RoadKind::Service,
            ..RoadDetails::default()
        },
        line: vec![at(205.4, 100.0), at(205.4, 900.0)],
        structure: None,
    };
    let map = MapData {
        roads: vec![road_north(), service],
        railways: vec![railway(&[(200.0, 100.0), (200.0, 900.0)], None)],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(0.6),
        &map,
        &mut |_, _| {},
    )
    .await;
    for north in [300.0, 479.9, 480.1, 700.0] {
        let left = street_at(&world, 203.8, -north).unwrap();
        let right = street_at(&world, 207.0, -north).unwrap();
        assert!(
            (left - right).abs() < 0.04,
            "folded road beside railway at {north}: {left}, {right}"
        );
        assert!((left - 623.24).abs() < 0.15);
    }
}

#[tokio::test]
async fn ground_profiles_are_shared_at_tag_changes_and_branch_endpoints() {
    struct Bump;
    impl ElevationModel for Bump {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(500.0 + 8.0 * (-(north - 480.0).powi(2) / 150.0).exp()))
        }
    }
    let mut map = roads();
    map.roads.truncate(1);
    map.roads[0].line = vec![at(80.0, 100.0), at(80.0, 480.0)];
    let mut continuation = map.roads[0].clone();
    continuation.details.kind = RoadKind::Secondary;
    continuation.line = vec![at(80.0, 480.0), at(80.0, 900.0)];
    let mut branch = continuation.clone();
    branch.line = vec![at(80.0, 480.0), at(180.0, 480.0)];
    map.roads.extend([continuation, branch]);
    let route = route_north(&[]).await;
    let projection = LocalProjection::for_route(&route);
    let streets: streets::Shared = streets::lines(&map, &projection, &mut Bump).await.into();
    let road = RoadIndex::new(&route, &projection);
    let levels = streets::Levels::new(
        &streets,
        &appearance::Index::new(&map, &projection),
        &road,
        &RoadIndex::from_lines(&[]),
    );
    for (east, north) in [
        (80.0, 480.0),
        (80.0, 479.99),
        (80.0, 480.01),
        (80.01, 480.0),
    ] {
        let level = levels.nearest(east, north, 0.0).unwrap().height.unwrap();
        assert!((level - 508.0).abs() < 0.02, "junction height {level}");
    }
}

#[tokio::test]
#[ignore = "writes offline earthworks comparison meshes"]
async fn earthworks_render_fixture() {
    let mut scenes = Vec::new();
    for slope in [0.25, 1.1] {
        let world = generate(
            &route_north(&[]).await,
            &mut Hillside(slope),
            &roads(),
            &mut |_, _| {},
        )
        .await;
        let mut meshes = Vec::new();
        for chunk in &world.chunks {
            for (mesh, terrain) in [
                (&chunk.mesh, true),
                (&chunk.streets, false),
                (&chunk.tracks, false),
            ] {
                let indices: Vec<_> = mesh
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter(|face| {
                        face.iter().all(|&i| {
                            let v = mesh.vertices[i as usize];
                            let (x, north) = (v[0] + chunk.center[0], -v[2] - chunk.center[2]);
                            (-140.0..=200.0).contains(&x) && (420.0..=560.0).contains(&north)
                        })
                    })
                    .flatten()
                    .copied()
                    .collect();
                if indices.is_empty() {
                    continue;
                }
                meshes.push(serde_json::json!({
                    "vertices": mesh.vertices.iter().map(|v| [v[0] + chunk.center[0], v[1] - 500.0, v[2] + chunk.center[2]]).collect::<Vec<_>>(),
                    "normals": mesh.normals, "uvs": mesh.uvs, "colors": mesh.colors,
                    "indices": indices, "terrain": terrain,
                }));
            }
        }
        scenes.push(serde_json::json!({"slope": slope, "meshes": meshes}));
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../screenshots/earthworks.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&scenes).unwrap()).unwrap();
}

/// A way of `class`/`kind` along `line` (metres east/north of the route start).
fn way(class: RoadClass, kind: RoadKind, surface: RoadSurface, line: &[(f64, f64)]) -> Road {
    Road {
        class,
        details: RoadDetails {
            kind,
            surface,
            ..RoadDetails::default()
        },
        line: line.iter().map(|&(east, north)| at(east, north)).collect(),
        structure: None,
    }
}

/// Terrain heights east of the route across `north`, every 0.1 m from `from` to `to`.
fn profile(world: &World, north: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    let steps = ((to - from) / 0.1).round() as i32;
    (0..=steps)
        .filter_map(|i| {
            let east = from + i as f32 * 0.1;
            ground_at(world, east, -north).map(|height| (east, height))
        })
        .collect()
}

#[tokio::test]
async fn surrounding_ways_leave_the_ridden_roadside_as_it_is() {
    // R141: the ridden road has priority. A track beside it forms its own bed, but the
    // ridden carriageway and the level verge the terrain's pieces keep beside it look as
    // without the track.
    for (slope, offset) in [(0.3, -12.0), (0.9, -18.0), (0.9, 18.0)] {
        let track = way(
            RoadClass::Track,
            RoadKind::Track,
            RoadSurface::Unpaved,
            &[(offset, 100.0), (offset, 900.0)],
        );
        let with = MapData {
            roads: vec![road_north(), track],
            ..MapData::default()
        };
        let without = MapData {
            roads: vec![road_north()],
            ..MapData::default()
        };
        let route = route_north(&[]).await;
        let world = generate(&route, &mut Hillside(slope), &with, &mut |_, _| {}).await;
        let alone = generate(&route, &mut Hillside(slope), &without, &mut |_, _| {}).await;
        let side = offset.signum() as f32;
        for north in [300.0, 479.9, 700.0] {
            for distance in [0.0_f32, 2.0, 3.0, 4.0, 5.0] {
                let (a, b) = (
                    ground_at(&world, side * distance, -north).unwrap(),
                    ground_at(&alone, side * distance, -north).unwrap(),
                );
                assert!(
                    (a - b).abs() < 0.02,
                    "a track {offset} m away moved the ridden roadside at {distance} m: {a}, {b}"
                );
            }
        }
    }
}

#[tokio::test]
async fn ground_beside_a_lower_side_road_has_no_steps_or_berms() {
    // On a steep hillside the ridden road's embankment ends at a lower road instead of
    // burying it or rising again beyond it, and no edge of either earthwork leaves a step.
    let service = way(
        RoadClass::Service,
        RoadKind::Service,
        RoadSurface::Paved,
        &[(-18.0, 100.0), (-18.0, 900.0)],
    );
    let map = MapData {
        roads: vec![road_north(), service],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(0.9),
        &map,
        &mut |_, _| {},
    )
    .await;
    for north in [300.0, 700.0] {
        let heights = profile(&world, north, -60.0, 20.0);
        let steepest = heights
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).abs() / (w[1].0 - w[0].0))
            .fold(0.0_f32, f32::max);
        assert!(
            steepest < 4.0,
            "a step in the ground at {north}: {steepest}"
        );
        // Beyond the side road's own banks the hillside is natural again.
        for &(east, height) in heights.iter().filter(|(east, _)| *east < -33.0) {
            let natural = 500.0 + 0.9 * east;
            assert!(
                height < natural + 0.3,
                "the ridden embankment rises again beyond the side road at {east}: {height}"
            );
        }
        // The side road keeps its bed: level across at its own natural datum.
        let (left, right) = (
            street_at(&world, -19.4, -north).unwrap(),
            street_at(&world, -16.6, -north).unwrap(),
        );
        assert!(
            (left - right).abs() < 0.03,
            "tilted side road: {left}, {right}"
        );
        assert!((f64::from(left) - (500.0 - 0.9 * 18.0)).abs() < 0.2);
    }
}

#[tokio::test]
async fn a_portal_keeps_its_hillside_when_a_track_climbs_beside_it() {
    // Alpenbrevet 40.07 km: a track leaves the ridden road just before a portal and climbs
    // beside the tunnel. Its bed is cut naturally beside the portal; the hill the portal
    // stands in, the approach and the tube stay as without the track.
    struct Ridge;
    impl ElevationModel for Ridge {
        fn elevation(
            &mut self,
            lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            let ridge = 40.0 * (1.0 - (north - 500.0).abs() / 250.0).clamp(0.0, 1.0);
            std::future::ready(Ok(500.0 + ridge + 0.1 * east))
        }
    }
    let track = way(
        RoadClass::Track,
        RoadKind::Track,
        RoadSurface::Unpaved,
        &[
            (0.0, 287.0),
            (10.0, 289.0),
            (15.0, 293.0),
            (18.0, 300.0),
            (18.0, 330.0),
            (24.0, 380.0),
        ],
    );
    let route = route_north(&[Structure {
        kind: StructureKind::Tunnel,
        line: vec![at(0.0, 300.0), at(0.0, 700.0)],
    }])
    .await;
    let with = MapData {
        roads: vec![road_north(), track],
        ..MapData::default()
    };
    let without = MapData {
        roads: vec![road_north()],
        ..MapData::default()
    };
    let world = generate(&route, &mut Ridge, &with, &mut |_, _| {}).await;
    let alone = generate(&route, &mut Ridge, &without, &mut |_, _| {}).await;
    // Where the track joins, only the ridden carriageway must stay as it was...
    for north in (280..=286).step_by(2) {
        for east in [-2.9_f32, 0.0, 2.9] {
            let (a, b) = (
                ground_at(&world, east, -(north as f32)).unwrap(),
                ground_at(&alone, east, -(north as f32)).unwrap(),
            );
            assert!(
                (a - b).abs() < 0.1,
                "the join moved the approach at {east}, {north}"
            );
        }
    }
    // ...the ground the portal stands in, before, around and over its opening, entirely.
    for north in (292..=330).step_by(4) {
        for east in [-8.0_f32, -6.0, -3.0, 0.0, 3.0, 6.0, 8.0] {
            let (a, b) = (
                ground_at(&world, east, -(north as f32)),
                ground_at(&alone, east, -(north as f32)),
            );
            assert!(
                match (a, b) {
                    (Some(a), Some(b)) => (a - b).abs() < 0.1,
                    (a, b) => a.is_none() && b.is_none(),
                },
                "the track moved the portal's ground at {east}, {north}: {a:?}, {b:?}"
            );
        }
    }
    // The track still climbs on its own level bed beside the tunnel.
    let (left, right) = (
        height_on(&world, |c| &c.tracks, 16.9, -315.0).unwrap(),
        height_on(&world, |c| &c.tracks, 19.1, -315.0).unwrap(),
    );
    assert!((left - right).abs() < 0.03, "tilted track: {left}, {right}");
}

#[tokio::test]
async fn the_ridden_roads_own_mapped_pieces_form_no_bed_where_tags_disagree() {
    // Alpenbrevet 36.40 km: map and route change to a bridge a few metres apart. The mapped
    // ground piece left over the route's bridge is the ridden road, not a second road with a
    // bed and walls on the gorge's rock.
    let route = route_north(&[Structure {
        kind: StructureKind::Bridge,
        line: vec![at(0.0, 400.0), at(0.0, 450.0)],
    }])
    .await;
    let map = MapData {
        roads: vec![road_north()],
        ..MapData::default()
    };
    let projection = LocalProjection::for_route(&route);
    let streets: streets::Shared = streets::lines(&map, &projection, &mut Hillside(1.1))
        .await
        .into();
    let mut road = RoadIndex::new(&route, &projection);
    road.set_appearances(&appearance::Index::new(&map, &projection));
    let levels = streets::Levels::new(
        &streets,
        &appearance::Index::new(&map, &projection),
        &road,
        &RoadIndex::from_lines(&[]),
    );
    for north in (380..=470).step_by(2) {
        assert!(
            levels.nearest(0.0, f64::from(north), 0.0).is_none(),
            "the ridden road's mapped piece formed a bed at {north}"
        );
    }
}

#[tokio::test]
async fn only_paved_roads_dress_their_steep_banks_with_walls() {
    let map = roads();
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(1.1),
        &map,
        &mut |_, _| {},
    )
    .await;
    let walls: Vec<f32> = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.streets
                .vertices
                .iter()
                .zip(&c.streets.colors)
                .filter(|(_, color)| color[3] == 0.0)
                .map(move |(v, _)| v[0] + c.center[0])
        })
        .collect();
    assert!(
        !walls.is_empty(),
        "the primary road's steep banks need walls"
    );
    assert!(
        walls.iter().all(|east| (east - 110.0).abs() < 16.0),
        "tracks, unpaved service roads and cycleways keep earth banks"
    );
}

#[tokio::test]
async fn terrain_is_refined_only_for_close_beds_at_another_height() {
    let map = MapData {
        roads: vec![
            road_north(),
            way(
                RoadClass::Track,
                RoadKind::Track,
                RoadSurface::Unpaved,
                &[(30.0, 100.0), (30.0, 900.0)],
            ),
            way(
                RoadClass::Service,
                RoadKind::Service,
                RoadSurface::Unpaved,
                &[(-8.0, 100.0), (-8.0, 900.0)],
            ),
        ],
        ..MapData::default()
    };
    let route = route_north(&[]).await;
    let projection = LocalProjection::for_route(&route);
    let streets: streets::Shared = streets::lines(&map, &projection, &mut Hillside(0.6))
        .await
        .into();
    let mut road = RoadIndex::new(&route, &projection);
    road.set_appearances(&appearance::Index::new(&map, &projection));
    let levels = streets::Levels::new(
        &streets,
        &appearance::Index::new(&map, &projection),
        &road,
        &RoadIndex::from_lines(&[]),
    );
    // A bed 4.8 m below the ridden road and 3 m from it needs a finer bank; one far off, or
    // at the ridden road's own level where it joins, keeps the usual pieces.
    assert!(levels.detail_near(-8.0, 500.0, 0.0, true, crate::SUB) > crate::SUB);
    assert_eq!(
        levels.detail_near(30.0, 500.0, 0.0, true, crate::SUB),
        crate::SUB
    );
    let flat: streets::Shared = streets::lines(&map, &projection, &mut Hillside(0.0))
        .await
        .into();
    let flat = streets::Levels::new(
        &flat,
        &appearance::Index::new(&map, &projection),
        &road,
        &RoadIndex::from_lines(&[]),
    );
    assert_eq!(
        flat.detail_near(-8.0, 500.0, 0.0, true, crate::SUB),
        crate::SUB
    );
}

#[tokio::test]
async fn a_track_across_a_steep_meadow_cuts_only_a_low_grassed_bank() {
    // Alpenbrevet 40.06 km: a level shelf as wide as the coarse terrain needs turned tracks on
    // steep meadows into wide bare scars strewn with boulders.
    let track = way(
        RoadClass::Track,
        RoadKind::Track,
        RoadSurface::Unpaved,
        &[(60.0, 100.0), (60.0, 900.0)],
    );
    let map = MapData {
        roads: vec![road_north(), track],
        ..MapData::default()
    };
    let world = generate(
        &route_north(&[]).await,
        &mut Hillside(0.6),
        &map,
        &mut |_, _| {},
    )
    .await;
    for north in [300.0, 479.9, 700.0] {
        let (left, right) = (
            height_on(&world, |c| &c.tracks, 59.0, -north).unwrap(),
            height_on(&world, |c| &c.tracks, 61.0, -north).unwrap(),
        );
        assert!((left - right).abs() < 0.03, "tilted track: {left}, {right}");
        for (east, height) in profile(&world, north, 50.0, 70.0) {
            let natural = 500.0 + 0.6 * east;
            assert!(
                (height - natural).abs() < 2.5,
                "a tall bank beside the track at {east}: {height}, natural {natural}"
            );
        }
    }
    // Earth banks stay below the slope the terrain shows as bare rock (about 53°)...
    for chunk in &world.chunks {
        for (v, n) in chunk.mesh.vertices.iter().zip(&chunk.mesh.normals) {
            let east = v[0] + chunk.center[0];
            if (52.0..68.0).contains(&east) {
                assert!(n[1] > 0.6, "a bare-rock bank beside the track at {east}");
            }
        }
    }
    // ...and carry no boulders: this meadow is too gentle for rocks of its own.
    let rocks: Vec<f32> = world
        .chunks
        .iter()
        .flat_map(|c| {
            c.trees
                .models_for_density(1.0)
                .filter(|(name, _)| name.starts_with("rock"))
                .flat_map(|(_, buffer)| {
                    buffer
                        .as_chunks::<{ crate::vegetation::PLANT_FLOATS }>()
                        .0
                        .iter()
                        .map(|t| t[3] + c.center[0])
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .filter(|east| (52.0..68.0).contains(east))
        .collect();
    assert!(rocks.is_empty(), "boulders on the track's bank: {rocks:?}");
}

#[tokio::test]
async fn far_tracks_follow_the_land_and_paved_roads_keep_plain_shelves() {
    // Riders see formed beds near the ride. Far off, tracks keep following the land, so the
    // coarser distant ground cannot swallow them, and paved roads build no walls.
    let map = MapData {
        roads: vec![
            road_north(),
            way(
                RoadClass::Track,
                RoadKind::Track,
                RoadSurface::Unpaved,
                &[(60.0, 100.0), (60.0, 900.0)],
            ),
            way(
                RoadClass::Track,
                RoadKind::Track,
                RoadSurface::Unpaved,
                &[(400.0, 100.0), (400.0, 900.0)],
            ),
            way(
                RoadClass::Street,
                RoadKind::Local,
                RoadSurface::Paved,
                &[(-400.0, 100.0), (-400.0, 900.0)],
            ),
        ],
        ..MapData::default()
    };
    let route = route_north(&[]).await;
    let projection = LocalProjection::for_route(&route);
    let streets: streets::Shared = streets::lines(&map, &projection, &mut Hillside(1.1))
        .await
        .into();
    let mut road = RoadIndex::new(&route, &projection);
    road.set_appearances(&appearance::Index::new(&map, &projection));
    let levels = streets::Levels::new(
        &streets,
        &appearance::Index::new(&map, &projection),
        &road,
        &RoadIndex::from_lines(&[]),
    );
    assert!(
        levels.nearest(60.0, 500.0, 0.0).is_some(),
        "a near track is formed"
    );
    assert!(
        levels.nearest(400.0, 500.0, 0.0).is_none(),
        "a far track follows the land"
    );
    let far = levels
        .nearest(-400.0, 500.0, 0.0)
        .expect("a far paved road keeps a shelf");
    assert!(!far.facing, "no walls far from the ride");
    assert_eq!(
        levels.detail_near(-400.0, 500.0, 0.0, false, crate::SUB),
        crate::SUB
    );
    assert_eq!(
        levels.detail_near(-400.0, 500.0, 0.0, false, 1),
        crate::FAR_SUB,
        "its shelf takes coarser pieces"
    );
}
