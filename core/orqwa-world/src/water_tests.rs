//! River surfaces follow their course, independently of steep valley sides and road shaping.

use super::*;

struct Valley;

impl ElevationModel for Valley {
    fn elevation(
        &mut self,
        lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
        let north = (lat - 46.0) * METERS_PER_DEGREE;
        std::future::ready(Ok(500.0 + east * 0.02 + (north - 500.0).abs() * 1.5))
    }
}

#[tokio::test]
async fn river_is_level_across_a_steep_valley_and_its_bed_stays_submerged() {
    let route = route_north(&[]).await;
    // An asymmetric outline reproduces terrain samples on the steep side of a valley.
    // The centre line supplies the water datum for both the area and its symbolic strip.
    let map = MapData {
        waterways: vec![Waterway {
            width: 12.0,
            line: vec![at(80.0, 500.0), at(1100.0, 500.0)],
        }],
        areas: vec![Area {
            cover: LandCover::Water,
            outer: vec![rectangle(590.0, 505.0, 510.0, 25.0)],
            inner: vec![],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Valley, &map, &mut |_, _| {}).await;
    for x in [160.0, 400.0, 479.5, 480.5, 800.0, 959.5, 960.5, 1000.0] {
        let expected = 500.0 + x * 0.02 - 0.48;
        for north in [485.0, 495.0, 500.0, 510.0, 525.0] {
            let water = height_on(&world, |c| &c.water, x, -north).expect("river surface");
            let bed = ground_at(&world, x, -north).expect("riverbed");
            assert!(
                (water - expected).abs() < 0.05,
                "water climbs the valley at {x}, {north}: {water} vs {expected}"
            );
            assert!(
                bed < water - 0.05,
                "bed emerges at {x}, {north}: {bed} vs {water}"
            );
        }
    }
    for chunk in &world.chunks {
        assert_valid(&chunk.water);
        assert_faces_follow_normals(&chunk.water);
    }
    let mut normals: HashMap<(i32, i32), [f32; 3]> = HashMap::new();
    let mut shared = 0;
    for chunk in &world.chunks {
        for (vertex, normal) in chunk.mesh.vertices.iter().zip(&chunk.mesh.normals) {
            let east = vertex[0] + chunk.center[0];
            let north = -(vertex[2] + chunk.center[2]);
            if (east - 480.0).abs() < 0.001 && (470.0..540.0).contains(&north) {
                let key = (
                    (east * 100.0).round() as i32,
                    (north * 100.0).round() as i32,
                );
                if let Some(previous) = normals.insert(key, *normal) {
                    assert!(
                        (0..3).all(|axis| (previous[axis] - normal[axis]).abs() < 0.01),
                        "bank lighting seam at {east}, {north}: {previous:?} vs {normal:?}"
                    );
                    shared += 1;
                }
            }
        }
    }
    assert!(shared > 10, "expected shared riverbank border vertices");
}

#[tokio::test]
async fn stream_does_not_tilt_across_its_width_on_a_hillside() {
    let route = route_north(&[]).await;
    let map = MapData {
        waterways: vec![Waterway {
            width: 20.0,
            line: vec![at(80.0, 500.0), at(1100.0, 500.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Valley, &map, &mut |_, _| {}).await;
    for x in [200.0, 400.0, 480.0, 900.0, 960.0] {
        let center = height_on(&world, |c| &c.water, x, -500.0).expect("stream");
        for north in [492.0, 496.0, 504.0, 508.0] {
            let edge = height_on(&world, |c| &c.water, x, -north).expect("stream edge");
            assert!(
                (edge - center).abs() < 0.05,
                "tilted stream at {x}, {north}: {edge} vs {center}"
            );
            assert!(
                ground_at(&world, x, -north).unwrap() < edge,
                "exposed stream bed"
            );
        }
    }
}

#[tokio::test]
async fn river_area_without_a_center_line_stays_level_across_its_width() {
    let route = route_north(&[]).await;
    let map = MapData {
        areas: vec![Area {
            cover: LandCover::Water,
            outer: vec![rectangle(590.0, 500.0, 510.0, 20.0)],
            inner: vec![],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Valley, &map, &mut |_, _| {}).await;
    for x in [200.0, 400.0, 480.0, 900.0, 960.0] {
        let center = height_on(&world, |c| &c.water, x, -500.0).expect("river");
        for north in [485.0, 495.0, 505.0, 515.0] {
            let edge = height_on(&world, |c| &c.water, x, -north).expect("river edge");
            assert!(
                (edge - center).abs() < 0.05,
                "tilted river at {x}, {north}: {edge} vs {center}"
            );
        }
    }
}

#[tokio::test]
async fn tall_banks_blend_out_instead_of_becoming_a_vertical_cut() {
    struct RaisedBanks;
    impl ElevationModel for RaisedBanks {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * METERS_PER_DEGREE;
            std::future::ready(Ok(if (north - 500.0).abs() < 0.1 {
                500.0
            } else {
                520.0
            }))
        }
    }
    let route = route_north(&[]).await;
    let map = MapData {
        waterways: vec![Waterway {
            width: 40.0,
            line: vec![at(80.0, 500.0), at(1100.0, 500.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut RaisedBanks, &map, &mut |_, _| {}).await;
    let mut previous = ground_at(&world, 200.0, -480.0).unwrap();
    for north in (440..480).rev() {
        let ground = ground_at(&world, 200.0, -(north as f32)).unwrap();
        assert!(
            (ground - previous).abs() < 4.0,
            "abrupt bank at {north}: {ground} after {previous}"
        );
        previous = ground;
    }
    assert!(
        world.chunks.iter().any(
            |chunk| chunk
                .mesh
                .vertices
                .iter()
                .zip(&chunk.mesh.normals)
                .any(|(vertex, normal)| {
                    let east = vertex[0] + chunk.center[0];
                    let north = -(vertex[2] + chunk.center[2]);
                    (east - 200.0).abs() < 2.1 && (north - 478.0).abs() < 1.1 && normal[1] < 0.85
                })
        ),
        "carved bank normals still describe level terrain"
    );
}

#[tokio::test]
async fn river_does_not_climb_back_over_a_terrain_bump() {
    struct Bump;
    impl ElevationModel for Bump {
        fn elevation(
            &mut self,
            _lat: f64,
            lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let east = (lon - 7.0) * METERS_PER_DEGREE * 46f64.to_radians().cos();
            let bump = (1.0 - ((east - 400.0) / 50.0).abs()).max(0.0) * 20.0;
            std::future::ready(Ok(500.0 + east * 0.02 + bump))
        }
    }
    let route = route_north(&[]).await;
    let map = MapData {
        waterways: vec![Waterway {
            width: 12.0,
            // Reversed map order must give the same downhill direction.
            line: vec![at(1100.0, 500.0), at(80.0, 500.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Bump, &map, &mut |_, _| {}).await;
    let mut previous = f32::NEG_INFINITY;
    for x in 100..=1050 {
        let height = height_on(&world, |c| &c.water, x as f32, -500.0).expect("river");
        assert!(
            height >= previous - 0.001,
            "river climbs upstream over a DEM bump at {x}: {height} after {previous}"
        );
        previous = height;
    }
}

#[tokio::test]
async fn lake_remains_level_when_terrain_rises_across_it() {
    let route = route_north(&[]).await;
    let map = MapData {
        areas: vec![Area {
            cover: LandCover::Water,
            outer: vec![square(400.0, 500.0, 200.0)],
            inner: vec![],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut Valley, &map, &mut |_, _| {}).await;
    let reference = height_on(&world, |c| &c.water, 400.0, -500.0).unwrap();
    for x in [210.0, 300.0, 512.0, 590.0] {
        for north in [310.0, 400.0, 600.0, 690.0] {
            let water = height_on(&world, |c| &c.water, x, -north).expect("lake");
            assert!(
                (water - reference).abs() < 0.01,
                "lake tilts at {x}, {north}"
            );
            assert!(
                ground_at(&world, x, -north).unwrap() < water,
                "lake bed exposed"
            );
        }
    }
}

#[tokio::test]
async fn lake_bed_stays_submerged_around_streams_far_from_the_shore() {
    let route = route_north(&[]).await;
    let map = MapData {
        areas: vec![Area {
            cover: LandCover::Water,
            outer: vec![rectangle(600.0, 500.0, 400.0, 400.0)],
            inner: vec![],
        }],
        waterways: vec![Waterway {
            width: 12.0,
            line: vec![at(250.0, 250.0), at(950.0, 750.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut FlatLake, &map, &mut |_, _| {}).await;
    // Follow both sides of the diagonal stream, including terrain chunk and index-cell joins.
    for x in [400.0, 479.5, 480.5, 600.0, 800.0, 959.5, 960.5] {
        let center = 250.0 + (x - 250.0) * 500.0 / 700.0;
        for offset in [-65.0, -45.0, -25.0, -10.0, 0.0, 10.0, 25.0, 45.0, 65.0] {
            let north = center + offset;
            let water = height_on(&world, |c| &c.water, x, -north).expect("lake water");
            let bed = ground_at(&world, x, -north).expect("lake bed");
            assert!(
                bed < water - 0.05,
                "exposed lakebed beside the stream at {x}, {north}: bed {bed}, water {water}"
            );
        }
    }
}

struct FlatLake;

impl ElevationModel for FlatLake {
    fn elevation(
        &mut self,
        _lat: f64,
        _lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
        std::future::ready(Ok(500.0))
    }
}

#[tokio::test]
async fn water_passes_below_a_ground_crossing_cut_into_the_dem() {
    struct HighDem;
    impl ElevationModel for HighDem {
        fn elevation(
            &mut self,
            _lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            std::future::ready(Ok(520.0))
        }
    }
    let route = route_north(&[]).await;
    let map = MapData {
        waterways: vec![Waterway {
            width: 12.0,
            line: vec![at(-400.0, 500.0), at(400.0, 500.0)],
        }],
        ..MapData::default()
    };
    let world = generate(&route, &mut HighDem, &map, &mut |_, _| {}).await;
    for x in [-3.0, 0.0, 3.0] {
        let water = height_on(&world, |c| &c.water, x, -500.0).expect("culvert water");
        assert!(water < 500.0 - 0.05, "water floods the road cut: {water}");
    }
    let open = height_on(&world, |c| &c.water, 150.0, -500.0).expect("open river");
    assert!(
        (open - 519.52).abs() < 0.02,
        "road shaping changes the open river"
    );
}
