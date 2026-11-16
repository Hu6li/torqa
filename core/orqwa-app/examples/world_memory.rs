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
        if !app.build_world() {
            wait(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));
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
    let world = runtime.block_on(orqwa_world::generate_into(
        &imported.route,
        &mut terrain,
        &imported.map,
        // Chunks are built in batches, a few dozen per report.
        &mut |done, total| println!("World {done}/{total}: {}", memory()),
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
    ));
    println!(
        "generated in {:.0} s; {}",
        began.elapsed().as_secs_f64(),
        memory()
    );
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
        orqwa_storage::worlds::Chunks::built(store, centres)
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
