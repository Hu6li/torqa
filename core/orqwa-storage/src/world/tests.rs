//! What world files must be (R266, docs/world-files.md): whole files are checked before they
//! enter the library, and reading one never changes it.

use std::io::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;

/// A glTF binary of one triangle with a texture, from `gltf` patched by `patch`.
pub(super) fn glb(patch: impl FnOnce(&mut Value)) -> Vec<u8> {
    let png = b"\x89PNG\r\n\x1a\n0000";
    let mut bin: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    bin.extend_from_slice(png);
    let mut gltf = json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": png.len()}
        ],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "images": [{"bufferView": 1, "mimeType": "image/png"}],
        "textures": [{"source": 0}],
        "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}],
        "scene": 0
    });
    patch(&mut gltf);
    let mut json = serde_json::to_vec(&gltf).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
    out.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&u32::try_from(bin.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

fn instances(groups: &[(u32, u32, u32, f32)]) -> Vec<u8> {
    let mut out = b"OQXI".to_vec();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(groups.len()).unwrap().to_le_bytes());
    for &(asset, count, flags, value) in groups {
        for word in [asset, count, flags] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        let floats = count as usize * if flags == 1 { 16 } else { 12 };
        for _ in 0..floats {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

fn route() -> Value {
    json!({"points": (0..30).map(|i| json!({
        "east": f64::from(i) * 5.0, "north": 0.0, "elevation": 430.0 + f64::from(i) * 0.3,
        "surface": if i == 10 { "bridge" } else { "ground" }, "width": 5.5
    })).collect::<Vec<_>>()})
}

pub(super) fn description() -> Value {
    json!({
        "format": 1,
        "id": "6f1c1d84-6a1e-4d55-9a53-0d9e2f1f7a10",
        "name": "Test world",
        "licence": "CC0-1.0",
        "attribution": ["Made for tests"],
        "created_unix_s": 0,
        "origin": {"lat": 47.09, "lon": 7.16, "located": true},
        "drives_on": "right",
        "environment": {
            "sun": {"elevation_deg": 30, "azimuth_deg": 220, "colour": [1, 0.95, 0.9], "energy": 1.5},
            "ambient": {"colour": [0.6, 0.7, 0.8], "energy": 0.8},
            "sky": {"kind": "gradient", "zenith": [0.2, 0.4, 0.7], "horizon": [0.7, 0.8, 0.9], "clouds": 0.1}
        },
        "sections": [{
            "center": [70.0, 0.0, 430.0],
            "radius": 120.0,
            "lods": [{"file": "sections/0/lod0.glb", "until_m": 500}, {"file": "sections/0/lod1.glb"}],
            "instances": "sections/0/instances.bin"
        }],
        "assets": [{"name": "tree", "lods": [{"file": "assets/tree/lod0.glb"}]}],
        "preview": "preview.png",
        "routes": [{"id": "loop", "name": "The loop", "file": "routes/loop.json"}]
    })
}

/// The entries of a valid world.
pub(super) fn entries() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            DESCRIPTION.into(),
            serde_json::to_vec(&description()).unwrap(),
        ),
        ("sections/0/lod0.glb".into(), glb(|_| {})),
        ("sections/0/lod1.glb".into(), glb(|_| {})),
        (
            "sections/0/instances.bin".into(),
            instances(&[(0, 3, 1, 0.5)]),
        ),
        ("assets/tree/lod0.glb".into(), glb(|_| {})),
        (
            "routes/loop.json".into(),
            serde_json::to_vec(&route()).unwrap(),
        ),
        ("preview.png".into(), b"\x89PNG\r\n\x1a\nrest".to_vec()),
    ]
}

pub(super) fn archive(entries: &[(String, Vec<u8>)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "orqwa-world-{}-{}.oqx",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(
            name.as_str(),
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn with(name: &str, bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    let mut entries = entries();
    match entries.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = bytes,
        None => entries.push((name.into(), bytes)),
    }
    entries
}

fn with_description(patch: impl FnOnce(&mut Value)) -> Vec<(String, Vec<u8>)> {
    let mut description = description();
    patch(&mut description);
    with(DESCRIPTION, serde_json::to_vec(&description).unwrap())
}

#[track_caller]
fn refused(entries: &[(String, Vec<u8>)], because: &str) {
    let path = archive(entries);
    let error = validate(&path).expect_err("refused").to_string();
    std::fs::remove_file(path).unwrap();
    assert!(error.contains(because), "{error:?} should say {because:?}");
}

#[test]
fn a_whole_world_is_accepted_with_its_description() {
    let path = archive(&entries());
    let description = validate(&path).unwrap();
    assert_eq!(description.name, "Test world");
    assert_eq!(description.drives_on, DrivesOn::Right);
    assert_eq!(read_description(&path).unwrap(), description);
    std::fs::remove_file(path).unwrap();
}

// scripts/fuzz.sh runs this before fuzzing the validator from a small accepted world.
#[test]
#[ignore = "writes the fuzz corpus named by ORQWA_FUZZ_CORPUS"]
fn write_fuzz_seed() {
    let directory =
        PathBuf::from(std::env::var_os("ORQWA_FUZZ_CORPUS").expect("corpus")).join("world");
    std::fs::create_dir_all(&directory).unwrap();
    let path = archive(&entries());
    validate(&path).unwrap();
    std::fs::copy(&path, directory.join("seed.oqx")).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_newer_format_asks_for_a_newer_orqwa() {
    let entries = with_description(|d| {
        d["format"] = json!(2);
        d["something_new"] = json!(true);
    });
    let path = archive(&entries);
    assert!(matches!(validate(&path), Err(WorldError::NewerFormat(2))));
    assert!(matches!(
        read_description(&path),
        Err(WorldError::NewerFormat(2))
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_file_holds_exactly_what_its_description_names() {
    refused(
        &with("notes.txt", b"hi".to_vec()),
        "not named by world.json",
    );
    let mut entries = entries();
    entries.retain(|(name, _)| name != "assets/tree/lod0.glb");
    refused(&entries, "missing");
    refused(
        &with_description(|d| d["sections"][0]["lods"][0]["file"] = json!("../lod0.glb")),
        "is not a sections/",
    );
    refused(
        &with_description(|d| d["sections"][0]["lods"][0]["file"] = json!("assets/tree/lod0.glb")),
        "is not a sections/",
    );
}

#[test]
fn the_description_is_checked() {
    refused(&with_description(|d| d["id"] = json!("not-a-uuid")), "UUID");
    refused(&with_description(|d| d["typo"] = json!(1)), "unknown field");
    refused(
        &with_description(|d| d["origin"]["lat"] = json!(95.0)),
        "on Earth",
    );
    refused(
        &with_description(|d| d["routes"] = json!([])),
        "1 to 256 routes",
    );
    refused(
        &with_description(|d| d["environment"]["sun"]["energy"] = json!(99)),
        "sun out of range",
    );
    refused(
        &with_description(|d| d["sections"][0]["lods"][0]["until_m"] = Value::Null),
        "nearest first",
    );
    refused(
        &with_description(|d| d["assets"][0]["name"] = json!("Big Tree")),
        "asset name",
    );
}

#[test]
fn gltf_is_self_contained_static_and_within_its_extensions() {
    let lod = "sections/0/lod0.glb";
    refused(
        &with(
            lod,
            glb(|g| g["images"][0]["uri"] = json!("https://example.com/a.png")),
        ),
        "inside the file",
    );
    refused(
        &with(
            lod,
            glb(|g| g["buffers"][0]["uri"] = json!("data:application/octet-stream;base64,AA==")),
        ),
        "BIN chunk",
    );
    refused(
        &with(
            lod,
            glb(|g| {
                g["extensionsUsed"] = json!(["KHR_draco_mesh_compression"]);
            }),
        ),
        "not allowed",
    );
    refused(
        &with(
            lod,
            glb(|g| {
                g["extensionsUsed"] = json!([glb::TERRAIN_LAYERS]);
                g["extensionsRequired"] = json!([glb::TERRAIN_LAYERS]);
            }),
        ),
        "never required",
    );
    refused(&with(lod, glb(|g| g["animations"] = json!([{}]))), "static");
    refused(
        &with(lod, glb(|g| g["accessors"][0]["count"] = json!(4))),
        "beyond its buffer view",
    );
    refused(
        &with(
            lod,
            glb(|g| g["nodes"] = json!([{"mesh": 0, "children": [0]}])),
        ),
        "tree",
    );
    let mut cut = glb(|_| {});
    cut.truncate(cut.len() - 4);
    refused(&with(lod, cut), "stated length");
}

#[test]
fn terrain_layers_are_optional_detail() {
    let lod = "sections/0/lod0.glb";
    let layers = |fade: Value| {
        glb(move |g| {
            g["extensionsUsed"] = json!([glb::TERRAIN_LAYERS]);
            g["materials"][0]["extensions"] = json!({glb::TERRAIN_LAYERS: {
                "weights": [{"index": 0}],
                "layers": [{"albedo": {"index": 0}, "roughness": 0.9, "size_m": 2.0}],
                "fade_m": fade
            }});
        })
    };
    let path = archive(&with(lod, layers(json!([40, 160]))));
    validate(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    refused(&with(lod, layers(json!([160, 40]))), glb::TERRAIN_LAYERS);
}

#[test]
fn instances_place_known_assets_with_finite_matrices() {
    let at = "sections/0/instances.bin";
    refused(&with(at, instances(&[(1, 1, 0, 0.0)])), "unknown asset");
    refused(&with(at, instances(&[(0, 1, 0, f32::NAN)])), "not a number");
    let mut extra = instances(&[(0, 1, 0, 0.0)]);
    extra.push(0);
    refused(&with(at, extra), "after its last group");
    let mut short = instances(&[(0, 2, 0, 0.0)]);
    short.truncate(short.len() - 1);
    refused(&with(at, short), "cut short");
}

#[test]
fn routes_are_long_enough_and_without_repeated_points() {
    let at = "routes/loop.json";
    let mut repeated = route();
    repeated["points"][3] = repeated["points"][2].clone();
    refused(&with(at, serde_json::to_vec(&repeated).unwrap()), "repeats");
    let mut short = route();
    short["points"] = json!([short["points"][0], short["points"][1]]);
    refused(&with(at, serde_json::to_vec(&short).unwrap()), "100 m");
    let mut odd = route();
    odd["points"][0]["surface"] = json!("water");
    refused(
        &with(at, serde_json::to_vec(&odd).unwrap()),
        "routes/loop.json",
    );
}

#[test]
fn a_world_file_is_read_as_it_is_and_never_changed() {
    let path = archive(&entries());
    let before = std::fs::read(&path).unwrap();
    let world = WorldFile::open(&path).unwrap();
    let route = world.route("loop").unwrap();
    assert_eq!(route.points.len(), 30);
    assert_eq!(route.points[10].surface, Surface::Bridge);
    let groups = world.instances("sections/0/instances.bin").unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].tinted);
    assert_eq!(groups[0].count(), 3);
    assert!(
        world
            .glb("sections/0/lod1.glb")
            .unwrap()
            .starts_with(b"glTF")
    );
    assert!(world.image("preview.png").is_ok());
    assert!(world.glb("routes/loop.json").is_err());
    assert!(world.glb("elsewhere.glb").is_err());
    assert!(world.route("missing").is_err());
    drop(world);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_damaged_part_is_refused_but_the_file_stays() {
    let mut damaged = glb(|_| {});
    damaged[0] = b'X';
    let path = archive(&with("sections/0/lod1.glb", damaged));
    let before = std::fs::read(&path).unwrap();
    let world = WorldFile::open(&path).unwrap();
    assert!(world.glb("sections/0/lod1.glb").is_err());
    assert!(world.glb("sections/0/lod0.glb").is_ok());
    drop(world);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn archives_beyond_the_format_are_refused_before_reading_entries() {
    let path = std::env::temp_dir().join(format!("orqwa-world-junk-{}.oqx", std::process::id()));
    std::fs::write(&path, b"PK\x05\x06 not a trailer").unwrap();
    assert!(validate(&path).is_err());
    std::fs::write(&path, b"").unwrap();
    assert!(validate(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_folder_packs_into_a_world_that_passes_its_checks() {
    let folder = std::env::temp_dir().join(format!("orqwa-world-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    for (name, bytes) in entries() {
        let path = folder.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let out = folder.with_extension("oqx");
    assert_eq!(pack(&folder, &out).unwrap().name, "Test world");
    assert_eq!(validate(&out).unwrap().routes[0].id, "loop");
    // A part missing from the folder packs nothing.
    std::fs::remove_file(folder.join("routes/loop.json")).unwrap();
    std::fs::remove_file(&out).unwrap();
    assert!(pack(&folder, &out).is_err());
    assert!(!out.exists() && !out.with_extension("oqx.part").exists());
    std::fs::remove_dir_all(folder).unwrap();
}
