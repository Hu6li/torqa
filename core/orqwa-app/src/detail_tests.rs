//! Courses prepared with map detail (ADR 0033): the surfaces of a region the rider downloaded,
//! carried in the course file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use orqwa_osm::{LatLon, Osm, Road, RoadKind, RoadSurface};

use crate::tests::{run_until, temp_dir};
use crate::{App, AppEvent, WorldPlan, course};

/// The tile of the test data: Wabern, near the Gurten.
const TILE: (u32, u32) = (8531, 5767);
const EXTRACT: &str = "bern-2026-10-24";

/// A cache with the test tile, and the longest local street in it.
fn cache_with_tile(cache: &Path) -> Road {
    let osm = cache.join("osm");
    let tile = orqwa_osm::tile_path(&osm, 14, TILE);
    std::fs::create_dir_all(tile.parent().unwrap()).unwrap();
    std::fs::write(
        &tile,
        include_bytes!("../../orqwa-osm/tests/data/14_8531_5767.pbf"),
    )
    .unwrap();
    let map = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let osm = Osm::new(osm).offline();
        osm.around(&[(46.9296, 7.4512)], 50.0, &mut |_, _| {})
            .await
            .unwrap()
    });
    let length = |road: &Road| {
        road.line
            .windows(2)
            .map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1))
            .sum::<f64>()
    };
    map.roads
        .into_iter()
        .filter(|r| r.details.kind == RoadKind::Local && r.details.surface != RoadSurface::Unpaved)
        .max_by(|a, b| length(a).total_cmp(&length(b)))
        .unwrap()
}

/// The region Bern made ready in `cache`, its detail cobbling `street`, as a download and its
/// extraction leave them.
fn install_bern(cache: &Path, street: &Road) {
    let osm = cache.join("osm");
    let line: Vec<String> = street
        .line
        .iter()
        .map(|&(lat, lon)| format!("[{},{}]", (lat * 1e7).round(), (lon * 1e7).round()))
        .collect();
    let cell = osm
        .join(orqwa_osm::detail::VERSION)
        .join(EXTRACT)
        .join("12")
        .join((TILE.0 >> 2).to_string())
        .join(format!("{}.json", TILE.1 >> 2));
    std::fs::create_dir_all(cell.parent().unwrap()).unwrap();
    let json = format!(
        r#"{{"surfaces":[{{"id":1,"kind":"local","surface":"cobbles","line":[{}]}}]}}"#,
        line.join(",")
    );
    std::fs::write(cell, json).unwrap();
    let record = osm.join(orqwa_osm::detail::DOWNLOADS).join("bern.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    let json = format!(r#"{{"id":"bern","name":"Bern","extract":"{EXTRACT}"}}"#);
    std::fs::write(record, json).unwrap();
}

/// A GPX file along the street.
fn gpx_along(dir: &Path, street: &Road) -> PathBuf {
    let mut xml = String::from("<gpx><trk><name>Cobbles</name><trkseg>");
    for &(lat, lon) in &street.line {
        let _ = write!(
            xml,
            r#"<trkpt lat="{lat}" lon="{lon}"><ele>550</ele></trkpt>"#
        );
    }
    xml.push_str("</trkseg></trk></gpx>");
    let path = dir.join("cobbles.gpx");
    std::fs::write(&path, xml).unwrap();
    path
}

fn prepare(app: &mut App, gpx: &Path) -> PathBuf {
    app.load_route(gpx.to_owned(), true);
    let events = run_until(app, |e| {
        matches!(e, AppEvent::CourseAdded(_) | AppEvent::Error(_))
    });
    match events.last() {
        Some(AppEvent::CourseAdded(course)) => course.clone(),
        other => panic!("not prepared: {other:?}"),
    }
}

fn route_points(app: &App) -> Vec<LatLon> {
    let route = app.route.as_ref().unwrap();
    route.points().iter().map(|p| (p.lat, p.lon)).collect()
}

/// The surfaces of the course's roads along `street` once it is opened, on another computer
/// with an empty cache.
fn surfaces_on_opening(dir: &Path, course: &Path, street: &Road) -> Vec<RoadSurface> {
    let other = dir.join(format!("other-{}", course.file_stem().unwrap().display()));
    let mut app = App::new(other.join("data"), other.join("cache")).unwrap();
    app.open_course(course.to_owned());
    run_until(&mut app, |e| {
        matches!(e, AppEvent::RouteLoaded(_) | AppEvent::Error(_))
    });
    let WorldPlan::OnRequest(Some(map)) = &app.world_plan else {
        panic!("no map data for the world");
    };
    let surfaces = map
        .roads
        .iter()
        .filter(|r| r.line.iter().all(|p| street.line.contains(p)))
        .map(|r| r.details.surface)
        .collect();
    app.shutdown();
    surfaces
}

#[test]
fn a_course_prepared_with_a_regions_detail_carries_its_surfaces() {
    let dir = temp_dir("detail-course");
    let cache = dir.join("cache");
    let street = cache_with_tile(&cache);
    let gpx = gpx_along(&dir, &street);
    let mut app = App::new(dir.join("data"), cache.clone()).unwrap();
    let plain = prepare(&mut app, &gpx);
    let plain_route = route_points(&app);
    let plain_course = dir.join("plain.oqc");
    std::fs::rename(&plain, &plain_course).unwrap();

    install_bern(&cache, &street);
    let cobbled = prepare(&mut app, &gpx);
    app.shutdown();

    assert_eq!(
        route_points(&app),
        plain_route,
        "the route matched as before"
    );
    let unpacked_to = dir.join("unpacked");
    let manifest = course::unpack(&cobbled, &unpacked_to).unwrap().manifest;
    assert_eq!(manifest.map_detail, [EXTRACT]);
    let cell = format!(
        "osm/detail-v1/{EXTRACT}/12/{}/{}.json",
        TILE.0 >> 2,
        TILE.1 >> 2
    );
    assert!(
        unpacked_to.join(cell).is_file(),
        "the cell it used goes along"
    );
    let surfaces = surfaces_on_opening(&dir, &cobbled, &street);
    assert!(!surfaces.is_empty() && surfaces.iter().all(|&s| s == RoadSurface::Cobbles));

    // Prepared without the region and opened where it is not, a course stays as it was.
    let manifest = course::unpack(&plain_course, &dir.join("plain"))
        .unwrap()
        .manifest;
    assert_eq!(manifest.map_detail, Vec::<String>::new());
    let surfaces = surfaces_on_opening(&dir, &plain_course, &street);
    assert!(!surfaces.is_empty() && !surfaces.contains(&RoadSurface::Cobbles));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_course_in_the_library_takes_the_detail_of_a_region_downloaded_since_once_opened() {
    let dir = temp_dir("detail-opened");
    let cache = dir.join("cache");
    let street = cache_with_tile(&cache);
    let gpx = gpx_along(&dir, &street);
    let mut app = App::new(dir.join("data"), cache.clone()).unwrap();
    let course = prepare(&mut app, &gpx);
    let before = course::fingerprint(&course).unwrap();
    install_bern(&cache, &street);

    app.open_course(course.clone());
    let events = run_until(&mut app, |e| {
        matches!(e, AppEvent::RouteLoaded(_) | AppEvent::Error(_))
    });

    assert!(
        events.iter().any(|e| matches!(e, AppEvent::CoursesChanged)),
        "the library shows it anew: {events:?}"
    );
    let WorldPlan::OnRequest(Some(map)) = &app.world_plan else {
        panic!("no map data for the world");
    };
    let cobbled = map
        .roads
        .iter()
        .filter(|r| r.line.iter().all(|p| street.line.contains(p)))
        .all(|r| r.details.surface == RoadSurface::Cobbles);
    assert!(cobbled, "its street is cobbled now");
    app.shutdown();
    let manifest = course::read_manifest(&course).unwrap();
    assert_eq!(manifest.map_detail, [EXTRACT], "its file names the detail");
    assert_ne!(
        course::fingerprint(&course).unwrap(),
        before,
        "so its kept world is built again"
    );
    // And it carries the detail to a computer without the region.
    let surfaces = surfaces_on_opening(&dir, &course, &street);
    assert!(!surfaces.is_empty() && surfaces.iter().all(|&s| s == RoadSurface::Cobbles));
    std::fs::remove_dir_all(dir).unwrap();
}

/// How far the route runs against the traffic round each mapped roundabout it passes (R126,
/// ADR 0033): metres of it along a road marked as going round one, against its drawing.
fn wrong_way_round(route: &[LatLon], roads: &[Road]) -> Vec<(LatLon, f64)> {
    let metres = |a: LatLon, b: LatLon| {
        let east = (b.1 - a.1) * 111_195.0 * a.0.to_radians().cos();
        (b.0 - a.0).mul_add(111_195.0, 0.0).hypot(east)
    };
    // Where along `line` (metres from its start) a point lies, if within 3 m of it.
    let along = |line: &[LatLon], at: LatLon| -> Option<f64> {
        let mut best: Option<(f64, f64)> = None;
        let mut start = 0.0;
        for pair in line.windows(2) {
            let length = metres(pair[0], pair[1]);
            for step in 0..=20 {
                let t = f64::from(step) / 20.0;
                let p = (
                    pair[0].0 + (pair[1].0 - pair[0].0) * t,
                    pair[0].1 + (pair[1].1 - pair[0].1) * t,
                );
                let off = metres(p, at);
                if off <= 3.0 && best.is_none_or(|(o, _)| off < o) {
                    best = Some((off, start + length * t));
                }
            }
            start += length;
        }
        best.map(|(_, s)| s)
    };
    let mut found = Vec::new();
    for road in roads.iter().filter(|r| r.details.roundabout) {
        let length: f64 = road.line.windows(2).map(|p| metres(p[0], p[1])).sum();
        let mut wrong = 0.0;
        for pair in route.windows(2) {
            // Along the ring, not across it: a step cutting across a small ring has its middle
            // near the ring's, far off its road.
            let middle = (
                f64::midpoint(pair[0].0, pair[1].0),
                f64::midpoint(pair[0].1, pair[1].1),
            );
            if along(&road.line, middle).is_none() {
                continue;
            }
            if let (Some(a), Some(b)) = (along(&road.line, pair[0]), along(&road.line, pair[1])) {
                let back = a - b;
                // Round past the start of a ring drawn as one closed way is on round.
                if back > 0.5 && back < length / 2.0 {
                    wrong += back;
                }
            }
        }
        if wrong > 0.0 {
            found.push((road.line[0], wrong));
        }
    }
    found
}

/// How far round each mapped roundabout the route turns on each pass (degrees,
/// counter-clockwise positive): its rings are the roads marked as going round one, joined end
/// to start; a pass, the route's points within the ring.
fn turns_round(route: &[LatLon], roads: &[Road]) -> Vec<(LatLon, f64)> {
    let flat = |origin: LatLon, p: LatLon| {
        let east = (p.1 - origin.1) * 111_195.0 * origin.0.to_radians().cos();
        (east, (p.0 - origin.0) * 111_195.0)
    };
    let marked: Vec<&Road> = roads.iter().filter(|r| r.details.roundabout).collect();
    // Rings: pieces whose ends meet, gathered.
    let mut ring_of: Vec<usize> = (0..marked.len()).collect();
    let find = |ring_of: &mut Vec<usize>, mut i: usize| {
        while ring_of[i] != i {
            i = ring_of[i];
        }
        i
    };
    for a in 0..marked.len() {
        for b in a + 1..marked.len() {
            let ends = |r: &Road| [r.line[0], r.line[r.line.len() - 1]];
            let meet = ends(marked[a]).iter().any(|&p| {
                ends(marked[b]).iter().any(|&q| {
                    let (x, y) = flat(p, q);
                    x.hypot(y) < 1.5
                })
            });
            if meet {
                let (ra, rb) = (find(&mut ring_of, a), find(&mut ring_of, b));
                ring_of[ra] = rb;
            }
        }
    }
    let mut passes = Vec::new();
    let roots: std::collections::BTreeSet<usize> =
        (0..marked.len()).map(|i| find(&mut ring_of, i)).collect();
    for root in roots {
        let points: Vec<LatLon> = (0..marked.len())
            .filter(|&i| find(&mut ring_of, i) == root)
            .flat_map(|i| marked[i].line.iter().copied())
            .collect();
        #[allow(clippy::cast_precision_loss)] // a few dozen points
        let n = points.len() as f64;
        let middle = (
            points.iter().map(|p| p.0).sum::<f64>() / n,
            points.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let radius = points
            .iter()
            .map(|&p| {
                let (x, y) = flat(middle, p);
                x.hypot(y)
            })
            .fold(0.0, f64::max);
        let mut turned: Option<f64> = None;
        let mut last: Option<f64> = None;
        for &point in route {
            let (x, y) = flat(middle, point);
            if x.hypot(y) > radius + 2.0 {
                if let Some(turn) = turned.take() {
                    passes.push((middle, turn));
                }
                last = None;
                continue;
            }
            let angle = y.atan2(x);
            if let Some(before) = last {
                let step = (angle - before + std::f64::consts::PI)
                    .rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                *turned.get_or_insert(0.0) += step.to_degrees();
            }
            last = Some(angle);
        }
        if let Some(turn) = turned {
            passes.push((middle, turn));
        }
    }
    passes
}

/// Each mapped roundabout the course at `ORQWA_COURSE` goes round is ridden the way traffic
/// goes (R126, ADR 0033), with the regions downloaded into this computer's cache:
/// `ORQWA_COURSE=… cargo test -p orqwa-app -- --ignored --nocapture roundabouts_of_a_course`.
#[test]
#[ignore = "needs a course and a region's download in the cache"]
fn roundabouts_of_a_course_are_ridden_the_way_traffic_goes() {
    let Some(course) = std::env::var_os("ORQWA_COURSE") else {
        return;
    };
    let cache = crate::paths::cache_dir();
    let osm = cache.join("osm");
    if orqwa_osm::detail::installed(&osm).is_empty() {
        let never = std::sync::atomic::AtomicBool::new(false);
        for kept in orqwa_osm::detail::kept(&osm)
            .into_iter()
            .filter(|k| k.downloaded)
        {
            let id = kept.id.clone();
            orqwa_osm::detail::install(&osm, (&id, &id, None), &mut |_, _, _| {}, &never).unwrap();
        }
    }
    let extracts: Vec<String> = orqwa_osm::detail::installed(&osm)
        .into_iter()
        .map(|region| region.extract)
        .collect();
    assert!(
        !extracts.is_empty(),
        "no region's detail in {}",
        osm.display()
    );
    let unpacked = course::unpack(Path::new(&course), &cache).unwrap();
    let used = orqwa_domain::files::UsedFiles::default();
    let mut imported = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::import_gpx(
            unpacked.gpx,
            "course",
            (&cache, true),
            (false, &extracts),
            &used,
            &mut |_, _, _| {},
        ))
        .unwrap();
    imported.map.apply_roundabouts();
    let route: Vec<LatLon> = imported
        .route
        .points()
        .iter()
        .map(|p| (p.lat, p.lon))
        .collect();
    let rings = imported
        .map
        .roads
        .iter()
        .filter(|r| r.details.roundabout)
        .count();
    let wrong = wrong_way_round(&route, &imported.map.roads);
    println!("{rings} pieces of roundabouts mapped; the wrong way round: {wrong:?}");
    let passes = turns_round(&route, &imported.map.roads);
    for (at, turn) in &passes {
        println!("round {:.5},{:.5}: {turn:.0}°", at.0, at.1);
    }
    let laps: Vec<_> = passes
        .iter()
        .filter(|(_, turn)| turn.abs() > 400.0)
        .collect();
    assert_eq!(wrong, Vec::new());
    assert_eq!(
        laps,
        Vec::<&(LatLon, f64)>::new(),
        "once round at most per pass"
    );
}
