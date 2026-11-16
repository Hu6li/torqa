//! What a course's generated world holds in memory, by part and by distance from the road, and
//! the process's resident and peak memory along the way; a checksum of the whole world, to show
//! that a change meant to keep the world as it is does.
//!
//! `cargo run --release -p orqwa-app --example world_memory -- <course.oqc | route.gpx> [cache]`
//! in the dev container (memory is read from Linux's `/proc/self/status`). A course is read
//! offline from its own data; a GPX route downloads its map and terrain into `cache`.
//!
//! With `--stream`, a GPX route is built without the app, offline from `cache`, and
//! each chunk is hashed and dropped as soon as it is finished (`orqwa_world::generate_into`):
//! the peak is then the building's alone. Both ways print a `chunk checksum` from each chunk's
//! hash, which a streamed build and a whole one share. Streamed, the world's first two reports
//! (0 chunks) come before and after the whole-route preparation.
//!
//! With `--stream --store`, each chunk is also written to a world store as the app writes one
//! it keeps (ADR 0040), in a temporary directory, which is then packed into a kept world: the
//! store's size and how long packing takes are printed.
//!
//! The app holds a world's chunks on disk (ADR 0040): the memory printed once the world is
//! ready is the core's while riding (R240), and the checksums and sizes are made from the
//! chunks read back one at a time. With `--keep`, a course (`.oqc`) is put in the library and
//! its world kept in `cache`: the first run builds and keeps it, the next rides it from there.
//!
//! With `--ahead <metres>`, a course's world can be ridden once the chunks within that radius
//! of its start are finished (ADR 0040, 2d): when that was, and when the rest was built, are
//! printed.
//!
//! With `--stream --ride <metres a minute> --ahead <metres>`, a rider sets off from the start
//! at that speed once the chunks within that radius are finished, and the build gives way to
//! the ride as it does in the app (`orqwa_world::Pace`, without lowering its threads' priority
//! as nothing is drawn): how fast chunks were built while the build kept half the cores and
//! while it hurried on all, and how far ahead of the rider it stayed, are printed.
//!
//! With `--stream --ahead <metres> --turn <metres along the route>`, the build is turned there
//! as a late joiner's is (`orqwa_world::Pace::turn_to`), to see what the chunks left waiting
//! at the start's frontier hold meanwhile.
#![allow(clippy::cast_precision_loss)] // megabytes for reading, not counting

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use orqwa_app::{App, AppEvent, WorldChunks};
use orqwa_world::{MeshData, TerrainChunk, World};

fn main() {
    let course = std::env::args()
        .nth(1)
        .expect("usage: world_memory <course.oqc | route.gpx> [cache] [--stream [--store]]");
    let dir = std::env::temp_dir().join(format!("orqwa-world-memory-{}", std::process::id()));
    // A GPX route's map and terrain are downloaded once into this cache when it is given.
    let cache: std::path::PathBuf = std::env::args()
        .nth(2)
        .map_or_else(|| dir.join("cache"), Into::into);
    if std::env::args().any(|arg| arg == "--stream") {
        let gpx = std::path::Path::new(&course)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gpx"));
        assert!(
            gpx,
            "--stream builds a GPX route: give the course's route.gpx"
        );
        let store = std::env::args()
            .any(|arg| arg == "--store")
            .then(|| orqwa_storage::worlds::KeptWorlds::new(&dir));
        stream(course.as_ref(), &cache, store.as_ref());
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    let gpx = std::path::Path::new(&course)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gpx"));
    let keep = std::env::args().any(|arg| arg == "--keep");
    let mut course = std::path::PathBuf::from(course);
    if keep && !gpx {
        // Only the library's courses' worlds stay kept: it is there before the app tidies.
        let library = dir.join("data/courses");
        std::fs::create_dir_all(&library).expect("library");
        let copy = library.join(course.file_name().expect("a course file"));
        std::fs::copy(&course, &copy).expect("course in the library");
        course = copy;
    }
    let mut app = App::new(dir.join("data"), cache).expect("app");
    let began = Instant::now();
    if gpx {
        app.load_route(course, false);
        wait(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));
    } else {
        if keep {
            app.set_keeps_worlds(true).expect("keeping worlds");
        }
        app.open_course(course);
        wait(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        let radius = std::env::args()
            .skip_while(|arg| arg != "--ahead")
            .nth(1)
            .map_or(orqwa_app::WHOLE_WORLD, |radius| {
                orqwa_domain::units::Meters(radius.parse().expect("--ahead <metres>"))
            });
        if !app.build_world(false, radius) {
            wait(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));
            println!(
                "ready to ride in {:.1} s; {}",
                began.elapsed().as_secs_f64(),
                memory()
            );
            if !app.world_chunks().is_some_and(WorldChunks::complete) {
                wait(&mut app, |e| {
                    matches!(e, AppEvent::WorldGrew { complete: true, .. })
                });
            }
        }
    }
    println!(
        "generated in {:.0} s; {}",
        began.elapsed().as_secs_f64(),
        memory()
    );
    if keep {
        // Kept in the background, as it is ridden.
        while app.kept_worlds().is_empty() {
            app.update(Duration::from_millis(20));
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("kept: {}", memory());
    }
    let world = app.world().expect("world");
    let chunks = app.world_chunks().expect("chunks");
    let reading = Instant::now();
    let (mut whole, mut hashes, mut spare) = (hasher(), Vec::new(), 0);
    each_chunk(chunks, |chunk| {
        hash_chunk(&mut whole, chunk);
        hashes.push(chunk_hash(chunk));
        spare += meshes(chunk).map(self::spare).sum::<usize>();
    });
    println!(
        "read back one by one in {:.1} s; {}",
        reading.elapsed().as_secs_f64(),
        memory()
    );
    println!("checksum {:016x}", checksum(whole, world));
    println!("chunk checksum {:016x}", chunk_checksum(&hashes, world));
    println!("unused mesh capacity {:.0} MB", mb(spare));
    by_part(world, chunks);
    by_distance(world, chunks);
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

/// Gives each chunk of `chunks` to `each` in index order, read one at a time.
fn each_chunk(chunks: &WorldChunks, mut each: impl FnMut(&TerrainChunk)) {
    for index in 0..chunks.len() {
        each(&chunks.get(index).expect("every chunk readable"));
    }
}

fn hasher() -> std::collections::hash_map::DefaultHasher {
    std::collections::hash_map::DefaultHasher::new()
}

/// Builds the world of the GPX route at `path` offline from `cache`, keeping only each chunk's
/// hash, and writing it into a store of `kept`'s if given.
#[allow(clippy::too_many_lines)] // one measured build, from import to checksum
fn stream(
    path: &std::path::Path,
    cache: &std::path::Path,
    kept: Option<&orqwa_storage::worlds::KeptWorlds>,
) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let began = Instant::now();
    let used = orqwa_domain::files::UsedFiles::default();
    let imported = runtime
        .block_on(orqwa_app::import_route(
            path,
            cache,
            true,
            &used,
            &mut |_, _, _| {},
        ))
        .expect("route");
    // The map and route alone; the world's first report follows, the second comes once its
    // whole-route preparation is done.
    println!("imported: {}", memory());
    let mut terrain =
        orqwa_terrain::Terrain::new(orqwa_terrain::TileSource::defaults(), cache.join("terrain"))
            .offline();
    let store = kept.map(|kept| kept.building("course").expect("store"));
    let hashes = std::sync::Mutex::new(Vec::new());
    let centres = std::sync::Mutex::new(Vec::new());
    let ride = Ride::from_args();
    let pace = orqwa_world::Pace::new(pace_radius(ride), None);
    let turn = turn_to(&imported.route, pace_radius(ride));
    if let Some(entry) = turn {
        pace.turn_to(entry);
    }
    let set_off = std::sync::OnceLock::new();
    let mut parts = None;
    let start = imported.route.points()[0];
    let at =
        orqwa_routes::LocalProjection::for_route(&imported.route).project(start.lat, start.lon);
    let whole = orqwa_world::Ahead {
        from: orqwa_domain::units::Meters(0.0),
        at: (
            orqwa_domain::units::Meters(at.0),
            orqwa_domain::units::Meters(at.1),
        ),
        radius: pace_radius(ride),
        ready: &mut |world, _| {
            parts = Some(world);
            let _ = set_off.set(Instant::now());
            println!(
                "ready to ride in {:.1} s; {}",
                began.elapsed().as_secs_f64(),
                memory()
            );
        },
        structures: &mut |_| {},
        pace: (ride.is_some() || turn.is_some()).then_some(&pace),
        written: &[],
    };
    let mut log = PaceLog::default();
    let riding = std::sync::atomic::AtomicBool::new(true);
    let rest = std::thread::scope(|scope| {
        if let Some(ride) = ride {
            let (riding, pace, set_off) = (&riding, &pace, &set_off);
            scope.spawn(move || {
                while riding.load(std::sync::atomic::Ordering::Relaxed) {
                    let at = set_off.get().map(|set_off| {
                        orqwa_domain::units::Meters(
                            ride.speed * set_off.elapsed().as_secs_f64() / 60.0,
                        )
                    });
                    pace.ride_at(at);
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
        }
        let rest = runtime.block_on(orqwa_world::generate_into(
            &imported.route,
            &mut terrain,
            &imported.map,
            // Chunks are built in batches, a few dozen per report.
            &mut |done, total| {
                let rider = set_off
                    .get()
                    .zip(ride)
                    .map(|(set_off, ride)| ride.speed * set_off.elapsed().as_secs_f64() / 60.0);
                log.report(done, rider.is_some(), pace.hurrying());
                match rider {
                    Some(rider) => println!(
                        "World {done}/{total}: rider at {:.1} km{}; {}",
                        rider / 1e3,
                        if pace.hurrying() { ", hurrying" } else { "" },
                        memory()
                    ),
                    None => println!("World {done}/{total}: {}", memory()),
                }
            },
            &|index, chunk| {
                if let Some(store) = &store {
                    store.write(index, &chunk).expect("chunk stored");
                }
                let mut hashes = hashes.lock().expect("hashes");
                if hashes.len() <= index {
                    hashes.resize(index + 1, None);
                }
                hashes[index] = Some(chunk_hash(&chunk));
                let mut centres = centres.lock().expect("centres");
                if centres.len() <= index {
                    centres.resize(index + 1, [0.0; 3]);
                }
                centres[index] = chunk.center;
            },
            whole,
        ));
        riding.store(false, std::sync::atomic::Ordering::Relaxed);
        rest
    });
    let world = World {
        structures: rest.structures,
        fallback_samples: rest.fallback_samples,
        ..parts.expect("handed over")
    };
    println!(
        "generated in {:.0} s; {}",
        began.elapsed().as_secs_f64(),
        memory()
    );
    if ride.is_some() {
        log.print();
    }
    let hashes: Vec<u64> = hashes
        .into_inner()
        .expect("hashes")
        .into_iter()
        .map(|hash| hash.expect("every chunk"))
        .collect();
    println!("chunk checksum {:016x}", chunk_checksum(&hashes, &world));
    if let (Some(kept), Some(store)) = (kept, store) {
        let packing = Instant::now();
        let centres = centres.into_inner().expect("centres");
        orqwa_storage::worlds::Chunks::built(std::sync::Arc::new(store), centres)
            .keep(kept, "course", &world, orqwa_world::GENERATOR)
            .expect("kept");
        println!(
            "kept {:.0} MB ({:.1} MB per km) in {:.1} s",
            kept.size() as f64 / 1e6,
            kept.size() as f64 / 1e3 / imported.route.length().0,
            packing.elapsed().as_secs_f64()
        );
    }
}

/// Where `--turn` turns the build to along `route`, the first route point at or past it.
fn turn_to(
    route: &orqwa_routes::Route,
    radius: orqwa_domain::units::Meters,
) -> Option<orqwa_world::Entry> {
    let distance: f64 = std::env::args()
        .skip_while(|arg| arg != "--turn")
        .nth(1)?
        .parse()
        .expect("--turn <metres>");
    let point = route
        .points()
        .iter()
        .find(|point| point.distance.0 >= distance)
        .expect("--turn within the route");
    let at = orqwa_routes::LocalProjection::for_route(route).project(point.lat, point.lon);
    Some(orqwa_world::Entry {
        from: point.distance,
        at: (
            orqwa_domain::units::Meters(at.0),
            orqwa_domain::units::Meters(at.1),
        ),
        radius,
    })
}

/// A rider setting off once the world around the start is built, at a steady speed.
#[derive(Clone, Copy)]
struct Ride {
    /// Metres a minute.
    speed: f64,
    /// How far around the rider the scene uploads chunks.
    radius: orqwa_domain::units::Meters,
}

impl Ride {
    fn from_args() -> Option<Self> {
        let after = |flag: &str| {
            std::env::args()
                .skip_while(|arg| arg != flag)
                .nth(1)
                .map(|value| value.parse::<f64>().expect("a number"))
        };
        Some(Self {
            speed: after("--ride")?,
            radius: orqwa_domain::units::Meters(after("--ahead").expect("--ride needs --ahead")),
        })
    }
}

/// The radius built before a ride: `ride`'s, `--ahead`'s, or the whole world.
fn pace_radius(ride: Option<Ride>) -> orqwa_domain::units::Meters {
    ride.map(|ride| ride.radius)
        .or_else(|| {
            std::env::args()
                .skip_while(|arg| arg != "--ahead")
                .nth(1)
                .map(|radius| {
                    orqwa_domain::units::Meters(radius.parse().expect("--ahead <metres>"))
                })
        })
        .unwrap_or(orqwa_app::WHOLE_WORLD)
}

/// How fast chunks were built while the build gave way to a ride and while it hurried.
#[derive(Default)]
struct PaceLog {
    /// When the build last reported, and how many chunks it had built then.
    last: Option<(Instant, usize)>,
    /// Chunks built and seconds taken while giving way, and while hurrying.
    giving_way: (usize, f64),
    hurrying: (usize, f64),
}

impl PaceLog {
    /// The build reported `done` chunks, the batch since built `hurrying` or not.
    fn report(&mut self, done: usize, riding: bool, hurrying: bool) {
        let now = Instant::now();
        if riding && let Some((then, before)) = self.last {
            let span = if hurrying {
                &mut self.hurrying
            } else {
                &mut self.giving_way
            };
            span.0 += done.saturating_sub(before);
            span.1 += now.duration_since(then).as_secs_f64();
        }
        self.last = Some((now, done));
    }

    fn print(&self) {
        for (name, (chunks, seconds)) in [
            ("giving way (half the cores)", self.giving_way),
            ("hurrying (all cores)", self.hurrying),
        ] {
            println!(
                "{name}: {chunks} chunks in {seconds:.0} s, {:.2} a second",
                chunks as f64 / seconds.max(f64::EPSILON)
            );
        }
    }
}

/// Runs the app until `done`, printing memory at every tenth of each loading step.
fn wait(app: &mut App, done: impl Fn(&AppEvent) -> bool) {
    loop {
        for event in app.update(Duration::from_millis(20)) {
            if let AppEvent::Error(failure) = &event {
                panic!("{failure:?}");
            }
            if let AppEvent::LoadProgress {
                stage,
                done: units,
                total,
            } = &event
            {
                let tenth = |n: usize| n * 10 / (*total).max(1);
                if *units == 0 || *units == *total || tenth(*units) != tenth(units - 1) {
                    println!("{stage:?} {units}/{total}: {}", memory());
                }
            }
            if done(&event) {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn memory() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("VmRSS") || l.starts_with("VmHWM"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1e6
}

fn meshes(c: &TerrainChunk) -> impl Iterator<Item = &MeshData> {
    [
        &c.mesh,
        &c.buildings,
        &c.streets,
        &c.tracks,
        &c.urban,
        &c.water,
        &c.props.wires,
    ]
    .into_iter()
    .chain(c.modelled.iter().map(|cell| &cell.shells))
}

fn bytes(m: &MeshData) -> usize {
    size_of_val(m.vertices.as_slice())
        + size_of_val(m.normals.as_slice())
        + size_of_val(m.uvs.as_slice())
        + size_of_val(m.colors.as_slice())
        + size_of_val(m.indices.as_slice())
}

fn spare(m: &MeshData) -> usize {
    (m.vertices.capacity() - m.vertices.len()) * size_of::<[f32; 3]>()
        + (m.normals.capacity() - m.normals.len()) * size_of::<[f32; 3]>()
        + (m.uvs.capacity() - m.uvs.len()) * size_of::<[f32; 2]>()
        + (m.colors.capacity() - m.colors.len()) * size_of::<[f32; 4]>()
        + (m.indices.capacity() - m.indices.len()) * size_of::<u32>()
}

fn mesh_hash(h: &mut impl Hasher, m: &MeshData) {
    for v in m.vertices.iter().chain(&m.normals) {
        v.map(f32::to_bits).hash(h);
    }
    for v in &m.uvs {
        v.map(f32::to_bits).hash(h);
    }
    for v in &m.colors {
        v.map(f32::to_bits).hash(h);
    }
    m.indices.hash(h);
}

fn chunk_hash(c: &TerrainChunk) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_chunk(&mut h, c);
    h.finish()
}

/// A checksum of the world from its chunks' hashes, in index order, and its other meshes.
fn chunk_checksum(chunks: &[u64], world: &World) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    chunks.hash(&mut h);
    for m in [&world.road, &world.structures, &world.railways] {
        mesh_hash(&mut h, m);
    }
    h.finish()
}

fn hash_chunk(h: &mut impl Hasher, c: &TerrainChunk) {
    c.center.map(f32::to_bits).hash(h);
    for m in meshes(c) {
        mesh_hash(h, m);
    }
    let buffers = c
        .trees
        .models
        .iter()
        .chain(&c.props.models)
        .chain(c.modelled.iter().flat_map(|cell| &cell.models));
    for (name, buffer) in buffers {
        name.hash(h);
        buffer.iter().map(|f| f.to_bits()).for_each(|b| b.hash(h));
    }
    for buffer in [&c.trees.grass, &c.trees.flowers] {
        buffer.iter().map(|f| f.to_bits()).for_each(|b| b.hash(h));
    }
    for label in &c.props.labels {
        label.text.hash(h);
        label.transform.map(f32::to_bits).hash(h);
    }
}

/// A checksum of the world from `h`, which hashed its chunks in index order, and its other
/// meshes.
fn checksum(mut h: impl Hasher, world: &World) -> u64 {
    for m in [&world.road, &world.structures, &world.railways] {
        mesh_hash(&mut h, m);
    }
    h.finish()
}

fn by_part(world: &World, chunks: &WorldChunks) {
    let mut parts: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |name: &str, bytes: usize| *parts.entry(name.to_owned()).or_default() += bytes;
    let floats = |v: &[f32]| size_of_val(v);
    each_chunk(chunks, |c| {
        add("ground", bytes(&c.mesh));
        add("streets", bytes(&c.streets));
        add("tracks", bytes(&c.tracks));
        add("urban", bytes(&c.urban));
        add("water", bytes(&c.water));
        add("buildings (shells)", bytes(&c.buildings));
        for cell in &c.modelled {
            add(
                "buildings (models)",
                cell.models.values().map(|v| floats(v)).sum(),
            );
            add("buildings (cell shells)", bytes(&cell.shells));
        }
        add("plants", c.trees.models.values().map(|v| floats(v)).sum());
        add(
            "grass and flowers",
            floats(&c.trees.grass) + floats(&c.trees.flowers),
        );
        add("props", c.props.models.values().map(|v| floats(v)).sum());
        add("wires", bytes(&c.props.wires));
    });
    add("road", bytes(&world.road));
    add("structures", bytes(&world.structures));
    add("railways", bytes(&world.railways));
    add(
        "horizon",
        bytes(&world.horizon.ground) + bytes(&world.horizon.water),
    );
    let total: usize = parts.values().sum();
    let mut sorted: Vec<_> = parts.into_iter().collect();
    sorted.sort_by_key(|part| std::cmp::Reverse(part.1));
    println!("{} chunks; world {:.0} MB", chunks.len(), mb(total));
    for (name, bytes) in sorted {
        println!("{:>9.1} MB  {name}", mb(bytes));
    }
}

/// Sizes by how far each chunk's middle lies from the ridden road, against the draw ranges in
/// `app/scenes/world.gd`.
fn by_distance(world: &World, chunks: &WorldChunks) {
    let road: Vec<(f32, f32)> = world
        .road
        .vertices
        .iter()
        .step_by(8)
        .map(|v| (v[0], v[2]))
        .collect();
    let mut bands: BTreeMap<u32, [usize; 5]> = BTreeMap::new();
    each_chunk(chunks, |c| {
        let distance = road
            .iter()
            .map(|&(x, z)| (x - c.center[0]).hypot(z - c.center[2]))
            .fold(f32::INFINITY, f32::min);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // metres, ≥ 0
        let band = bands.entry((distance / 250.0) as u32 * 250).or_default();
        band[0] += 1;
        band[1] += bytes(&c.streets);
        band[2] += bytes(&c.mesh);
        band[3] += bytes(&c.urban);
        band[4] += bytes(&c.tracks);
    });
    println!("from road  chunks  streets   ground    urban   tracks (MB)");
    for (from, [n, streets, ground, urban, tracks]) in bands {
        println!(
            "{from:>6} m {n:>7} {:>8.0} {:>8.0} {:>8.0} {:>8.0}",
            mb(streets),
            mb(ground),
            mb(urban),
            mb(tracks)
        );
    }
}
