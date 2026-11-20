//! Worlds authored outside ORQWA in the app (R266–R269): imported whole, each route a ride,
//! ridden as authored without building or fetching anything, never shared or renamed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use orqwa_devices::fake::FakeRider;
use orqwa_domain::units::{Meters, Percent, Rpm, Watts};
use orqwa_physics::DescentMode;
use serde_json::json;

use crate::tests::{run_until, temp_dir};
use crate::{App, AppEvent, GhostChoice, TrainerChoice, WHOLE_WORLD, authored};

/// A glTF binary of one triangle: the least a section can be.
fn glb() -> Vec<u8> {
    let bin: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut json = serde_json::to_vec(&json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [{"buffer": 0, "byteLength": bin.len()}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}]
    }))
    .unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let length = 28 + json.len() + bin.len();
    let mut out = b"glTF".to_vec();
    for word in [2, length, json.len()] {
        out.extend_from_slice(&u32::try_from(word).unwrap().to_le_bytes());
    }
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&u32::try_from(bin.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// A world of two routes north from 2 km east of its origin: a flat one and one climbing 4 %.
fn world(dir: &Path, name: &str, version: &str, located: bool) -> PathBuf {
    let folder = dir.join(format!("{name}-{version}"));
    let write = |file: &str, bytes: Vec<u8>| {
        let path = folder.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    let route = |grade: f64| {
        serde_json::to_vec(&json!({"points": (0..=40).map(|i| json!({
            "east": 2000.0, "north": f64::from(i) * 10.0,
            "elevation": 600.0 + f64::from(i) * 10.0 * grade,
            "surface": "ground", "width": 4.0
        })).collect::<Vec<_>>()}))
        .unwrap()
    };
    write("routes/flat.json", route(0.0));
    write("routes/climb.json", route(0.04));
    write("sections/0/lod0.glb", glb());
    write(
        "world.json",
        serde_json::to_vec(&json!({
            "format": 1,
            "id": "0b6c38f4-3f0e-4c47-9a2c-2d36c6a0e2f1",
            "name": name,
            "version": version,
            "licence": "All rights reserved",
            "attribution": ["Geodata © swisstopo (modified)"],
            "created_unix_s": 1_795_000_000,
            "origin": {"lat": 47.09, "lon": 7.16, "located": located},
            "drives_on": "right",
            "environment": {
                "sun": {"elevation_deg": 30, "azimuth_deg": 220, "colour": [1, 0.95, 0.9], "energy": 1.5},
                "ambient": {"colour": [0.6, 0.7, 0.8], "energy": 0.8},
                "sky": {"kind": "atmosphere", "clouds": 0.3}
            },
            "sections": [{"center": [2000.0, 200.0, 600.0], "radius": 300.0,
                          "lods": [{"file": "sections/0/lod0.glb"}]}],
            "routes": [
                {"id": "flat", "name": "Along the lake", "file": "routes/flat.json"},
                {"id": "climb", "name": "Up the hill", "file": "routes/climb.json"}
            ]
        }))
        .unwrap(),
    );
    let file = dir.join(format!("{name}-{version}.oqx"));
    orqwa_storage::world::pack(&folder, &file).unwrap();
    file
}

fn added(app: &mut App) -> Result<PathBuf, String> {
    let events = run_until(app, |e| {
        matches!(e, AppEvent::CourseAdded(_) | AppEvent::Error(_))
    });
    match events.last() {
        Some(AppEvent::CourseAdded(path)) => Ok(path.clone()),
        other => Err(format!("{other:?}")),
    }
}

#[test]
fn a_world_file_is_imported_whole_and_each_route_is_a_ride() {
    let dir = temp_dir("world-import");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    app.import_course(world(&dir, "Lake", "1.0", true));
    let first = added(&mut app).unwrap();
    assert!(authored::WorldRoute::parse(&first).is_some());

    let courses = app.courses();
    assert_eq!(courses.len(), 2);
    let climb = courses
        .iter()
        .find(|c| c.manifest.name == "Lake – Up the hill")
        .unwrap();
    assert!((climb.manifest.length_m - 400.0).abs() < 1e-6);
    assert!((climb.manifest.max_grade_percent - 4.0).abs() < 1e-6);
    let info = climb.world.as_ref().unwrap();
    assert_eq!(info.licence, "All rights reserved");
    assert_eq!(info.routes, ["Along the lake", "Up the hill"]);
    assert_eq!(
        climb.manifest.countries,
        Some(Vec::new()),
        "nothing to look up"
    );

    // A new version of the same world replaces it; a broken file is refused whole.
    app.import_course(world(&dir, "Lake", "1.1", true));
    added(&mut app).unwrap();
    let courses = app.courses();
    assert_eq!(courses.len(), 2);
    assert_eq!(
        courses[0].world.as_ref().unwrap().version.as_deref(),
        Some("1.1")
    );
    let broken = dir.join("broken.oqx");
    std::fs::write(&broken, b"PK not a world").unwrap();
    app.import_course(broken);
    assert!(added(&mut app).is_err());
    assert_eq!(app.courses().len(), 2);
    app.shutdown();
    std::fs::remove_dir_all(dir).unwrap();
}

/// Opens a world's route and rides it with the fake trainer; returns the saved activity.
fn ride(app: &mut App, path: PathBuf) -> PathBuf {
    app.open_course(path);
    let events = run_until(app, |e| matches!(e, AppEvent::RouteLoaded(_)));
    let Some(AppEvent::RouteLoaded(summary)) = events.last() else {
        unreachable!()
    };
    assert_eq!(
        summary.elevation_source,
        orqwa_routes::ElevationSource::World
    );
    assert!(!app.build_world(false, WHOLE_WORLD));
    let events = run_until(app, |e| matches!(e, AppEvent::WorldReady { .. }));
    assert!(matches!(
        events.last(),
        Some(AppEvent::WorldReady { chunks: 0, .. })
    ));
    app.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(250.0),
        cadence: Rpm(90.0),
        heart: None,
    }))
    .unwrap();
    app.start_ride(Percent(100.0), DescentMode::Coast, &GhostChoice::None)
        .unwrap();
    run_until(app, |e| matches!(e, AppEvent::Connected(_)));
    for _ in 0..120 {
        std::thread::sleep(Duration::from_millis(16));
        app.update(Duration::from_millis(16));
    }
    let state = app.ride_state().unwrap();
    assert!(state.distance.0 > 1.0, "{state:?}");
    let events = app.finish_ride();
    let Some(AppEvent::RideSaved(fit)) = events.first() else {
        panic!("not saved: {events:?}");
    };
    fit.clone()
}

#[test]
fn a_worlds_route_is_ridden_as_authored_without_building_or_fetching() {
    let dir = temp_dir("world-ride");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    app.set_keeps_worlds(true).unwrap();
    app.import_course(world(&dir, "Hill", "1.0", true));
    added(&mut app).unwrap();
    let climb = app
        .courses()
        .into_iter()
        .find(|c| c.manifest.name.ends_with("Up the hill"))
        .unwrap();
    let fit = ride(&mut app, climb.path.clone());

    // The authored grade, not a smoothed or corrected one, at the world's own place.
    let route = app.route().unwrap();
    assert!((route.grade_at(Meters(200.0)).0 - 4.0).abs() < 1e-6);
    let start = route.position(Meters(0.0));
    let (east, north) =
        orqwa_routes::LocalProjection::for_route(route).project(start.lat, start.lon);
    assert!((east - 2000.0).abs() < 1e-3 && north.abs() < 1e-3);
    // Kept to the right of the 4 m road's middle.
    let rider = app.rider_position(Meters(100.0), false).unwrap();
    let middle = route.position(Meters(100.0));
    let (mx, _) = orqwa_routes::LocalProjection::for_route(route).project(middle.lat, middle.lon);
    assert!((rider.0 - mx - 1.0).abs() < 0.05, "{rider:?} vs {mx}");

    // Nothing was built, kept or downloaded for it.
    let cache = dir.join("cache");
    for nothing in ["worlds", "terrain", "osm"] {
        assert!(!cache.join(nothing).exists(), "{nothing} in the cache");
    }
    // Records belong to its line.
    let history = app.history();
    assert_eq!(history[0].fit, fit);
    assert_eq!(history[0].record.route_key, climb.manifest.route_key);
    let (_, samples) = orqwa_storage::decode_fit(&std::fs::read(&fit).unwrap()).unwrap();
    assert!(samples.iter().any(|s| s.location.is_some()));
    app.shutdown();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rides_on_a_world_not_on_earth_keep_no_positions() {
    let dir = temp_dir("world-unplaced");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    app.import_course(world(&dir, "Nowhere", "1.0", false));
    let path = added(&mut app).unwrap();
    let fit = ride(&mut app, path);
    let (_, samples) = orqwa_storage::decode_fit(&std::fs::read(&fit).unwrap()).unwrap();
    assert_ne!(samples.len(), 0);
    assert!(samples.iter().all(|s| s.location.is_none()));
    app.shutdown();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_worlds_routes_keep_their_names_and_go_with_it() {
    let dir = temp_dir("world-delete");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    app.import_course(world(&dir, "Gone", "1.0", true));
    let path = added(&mut app).unwrap();
    assert!(app.rename_course(&path, "Mine now").is_err());
    let which = authored::WorldRoute::parse(&path).unwrap();
    assert!(which.file.starts_with(app.worlds_dir()));
    app.delete_course(&path).unwrap();
    assert!(!which.file.exists());
    assert_eq!(app.courses().len(), 0);
    app.shutdown();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_world_route_path_names_its_file_and_route() {
    let which = authored::WorldRoute {
        file: PathBuf::from("/data/worlds/lake-biel.oqx"),
        route: "twann-chasseral".into(),
    };
    assert_eq!(authored::WorldRoute::parse(&which.path()), Some(which));
    for other in [
        "/data/courses/a#b.oqc",
        "/data/worlds/lake.oqx",
        "/data/worlds/lake.oqx#",
    ] {
        assert_eq!(
            authored::WorldRoute::parse(Path::new(other)),
            None,
            "{other}"
        );
    }
}
