//! What a course's generated world holds in memory, by part and by distance from the road, and
//! the process's resident and peak memory along the way; a checksum of the whole world, to show
//! that a change meant to keep the world as it is does.
//!
//! `cargo run --release -p orqwa-app --example world_memory -- <course.oqc | route.gpx> [cache]`
//! in the dev container (memory is read from Linux's `/proc/self/status`). A course is read
//! offline from its own data; a GPX route downloads its map and terrain into `cache`.
//!
//! With `--stream` after them, a GPX route is built without the app, offline from `cache`, and
//! each chunk is hashed and dropped as soon as it is finished (`orqwa_world::generate_into`):
//! the peak is then the building's alone. Both ways print a `chunk checksum` from each chunk's
//! hash, which a streamed build and a whole one share.
#![allow(clippy::cast_precision_loss)] // megabytes for reading, not counting

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use orqwa_app::{App, AppEvent};
use orqwa_world::{MeshData, TerrainChunk, World};

fn main() {
    let course = std::env::args()
        .nth(1)
        .expect("usage: world_memory <course.oqc | route.gpx> [cache] [--stream]");
    let dir = std::env::temp_dir().join(format!("orqwa-world-memory-{}", std::process::id()));
    // A GPX route's map and terrain are downloaded once into this cache when it is given.
    let cache: std::path::PathBuf = std::env::args()
        .nth(2)
        .map_or_else(|| dir.join("cache"), Into::into);
    if std::env::args().nth(3).as_deref() == Some("--stream") {
        stream(course.as_ref(), &cache);
        return;
    }
    let mut app = App::new(dir.join("data"), cache).expect("app");
    let began = Instant::now();
    let gpx = std::path::Path::new(&course)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gpx"));
    if gpx {
        app.load_route(course.into(), false);
        wait(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));
    } else {
        app.open_course(course.into());
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
    let world = app.world().expect("world");
    println!("checksum {:016x}", checksum(world));
    let hashes: Vec<u64> = world.chunks.iter().map(chunk_hash).collect();
    println!("chunk checksum {:016x}", chunk_checksum(&hashes, world));
    let spare: usize = world.chunks.iter().flat_map(meshes).map(spare).sum();
    println!("unused mesh capacity {:.0} MB", mb(spare));
    by_part(world);
    by_distance(world);
    let _ = std::fs::remove_dir_all(dir);
}

/// Builds the world of the GPX route at `path` offline from `cache`, keeping only each chunk's
/// hash.
fn stream(path: &std::path::Path, cache: &std::path::Path) {
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
    let mut terrain =
        orqwa_terrain::Terrain::new(orqwa_terrain::TileSource::defaults(), cache.join("terrain"))
            .offline();
    let mut hashes = Vec::new();
    let world = runtime.block_on(orqwa_world::generate_into(
        &imported.route,
        &mut terrain,
        &imported.map,
        // Chunks are built in batches, a few dozen per report.
        &mut |done, total| println!("World {done}/{total}: {}", memory()),
        &mut |index, chunk| {
            if hashes.len() <= index {
                hashes.resize(index + 1, None);
            }
            hashes[index] = Some(chunk_hash(&chunk));
        },
    ));
    println!(
        "generated in {:.0} s; {}",
        began.elapsed().as_secs_f64(),
        memory()
    );
    let hashes: Vec<u64> = hashes
        .into_iter()
        .map(|hash| hash.expect("every chunk"))
        .collect();
    println!("chunk checksum {:016x}", chunk_checksum(&hashes, &world));
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

fn checksum(world: &World) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in &world.chunks {
        hash_chunk(&mut h, c);
    }
    for m in [&world.road, &world.structures, &world.railways] {
        mesh_hash(&mut h, m);
    }
    h.finish()
}

fn by_part(world: &World) {
    let mut parts: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |name: &str, bytes: usize| *parts.entry(name.to_owned()).or_default() += bytes;
    let floats = |v: &[f32]| size_of_val(v);
    for c in &world.chunks {
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
    }
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
    println!("{} chunks; world {:.0} MB", world.chunks.len(), mb(total));
    for (name, bytes) in sorted {
        println!("{:>9.1} MB  {name}", mb(bytes));
    }
}

/// Sizes by how far each chunk's middle lies from the ridden road, against the draw ranges in
/// `app/scenes/world.gd`.
fn by_distance(world: &World) {
    let road: Vec<(f32, f32)> = world
        .road
        .vertices
        .iter()
        .step_by(8)
        .map(|v| (v[0], v[2]))
        .collect();
    let mut bands: BTreeMap<u32, [usize; 5]> = BTreeMap::new();
    for c in &world.chunks {
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
    }
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
