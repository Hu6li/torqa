//! The rules a world's glTF binaries follow (`docs/world-files.md`, "glTF content"): glTF 2.0,
//! static, with a short list of extensions, holding everything but the images it shares with
//! the world's other files. Checked from the JSON chunk and the BIN chunk's length, without
//! reading the binary data itself.

use std::collections::BTreeSet;
use std::io::Read;

use serde_json::{Map, Value};

use super::{WorldError, invalid};

/// Extensions ORQWA knows in a world's glTF. Others may be used, never required: this ORQWA
/// leaves them and their data aside, and draws the file as glTF without them.
pub const ALLOWED: [&str; 6] = [
    "KHR_texture_transform",
    "KHR_materials_emissive_strength",
    "KHR_materials_unlit",
    "KHR_mesh_quantization",
    "EXT_texture_webp",
    TERRAIN_LAYERS,
];
/// ORQWA's material extension for detailed ground; never required.
pub const TERRAIN_LAYERS: &str = "ORQWA_terrain_layers";
const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
const JSON: u32 = 0x4E4F_534A;
const BIN: u32 = 0x004E_4942;

/// A `.glb` as checked: how many of its bytes were read, and the images it takes from the
/// world's shared textures, by `uri` as written, relative to the file.
#[derive(Debug, Default)]
pub struct Checked {
    /// Bytes read: the header and the JSON chunk.
    pub read: u64,
    /// The `uri` of every image outside the file that a texture draws.
    pub shared: Vec<String>,
    /// The `uri` of images outside the file only extensions use, of any type: those this ORQWA
    /// does not know are never read.
    pub foreign: Vec<String>,
}

/// Checks a `.glb` of `length` bytes from `input`, reading its header and JSON. The BIN chunk's
/// data is left for the caller to read past; the images it names outside itself for the caller
/// to find among the world's textures.
///
/// # Errors
/// [`WorldError::Invalid`] naming the rule broken.
pub fn check_stream(input: &mut impl Read, length: u64) -> Result<Checked, WorldError> {
    let mut header = [0u8; 20];
    input
        .read_exact(&mut header)
        .map_err(|_| invalid("too short for glTF"))?;
    let word = |at: usize| le(&header, at);
    if &header[..4] != b"glTF" || word(4) != 2 || u64::from(word(8)) != length {
        return Err(invalid("not a glTF 2.0 binary of its stated length"));
    }
    let json_length = u64::from(word(12));
    if word(16) != JSON || json_length > MAX_JSON_BYTES || 20 + json_length > length {
        return Err(invalid("the first chunk is not JSON within the file"));
    }
    let mut json = Vec::new();
    input.take(json_length).read_to_end(&mut json)?;
    if json.len() as u64 != json_length {
        return Err(invalid("the JSON chunk is cut short"));
    }
    let mut read = 20 + json_length;
    let mut bin = None;
    if read < length {
        let mut chunk = [0u8; 8];
        input
            .read_exact(&mut chunk)
            .map_err(|_| invalid("cut short"))?;
        let chunk_length = u64::from(le(&chunk, 0));
        let kind = le(&chunk, 4);
        read += 8;
        if kind != BIN || read + chunk_length != length {
            return Err(invalid("the second chunk is not BIN ending the file"));
        }
        bin = Some(chunk_length);
    }
    let gltf: Value =
        serde_json::from_slice(&json).map_err(|e| invalid(format!("glTF JSON: {e}")))?;
    let (shared, foreign) = check(&gltf, bin)?;
    Ok(Checked {
        read,
        shared,
        foreign,
    })
}

fn le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn array<'a>(gltf: &'a Map<String, Value>, key: &str) -> Result<&'a [Value], WorldError> {
    match gltf.get(key) {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(invalid(format!("{key} is not a list"))),
    }
}

fn index(value: Option<&Value>, below: usize, what: &str) -> Result<Option<usize>, WorldError> {
    match value {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < below)
            .map(Some)
            .ok_or_else(|| invalid(format!("{what} refers to nothing"))),
    }
}

fn number(object: &Map<String, Value>, key: &str, default: u64) -> Result<u64, WorldError> {
    object.get(key).map_or(Ok(default), |value| {
        value
            .as_u64()
            .ok_or_else(|| invalid(format!("{key} is not a count")))
    })
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, WorldError> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{what} is not an object")))
}

fn names(gltf: &Map<String, Value>, key: &str) -> Result<BTreeSet<String>, WorldError> {
    array(gltf, key)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("{key} lists a non-name")))
        })
        .collect()
}

/// The checks on a parsed glTF with a BIN chunk of `bin` bytes, if any.
fn check(gltf: &Value, bin: Option<u64>) -> Result<(Vec<String>, Vec<String>), WorldError> {
    let gltf = object(gltf, "glTF")?;
    let used = extensions(gltf)?;
    let views = buffer_views(gltf, bin)?;
    let accessors = array(gltf, "accessors")?;
    for accessor in accessors {
        accessor_fits(object(accessor, "accessor")?, &views)?;
    }
    let (textures, shared, foreign) =
        textures(gltf, views.len(), used.contains("EXT_texture_webp"))?;
    let materials = array(gltf, "materials")?;
    for material in materials {
        texture_references(material, textures)?;
        if let Some(layers) = material
            .get("extensions")
            .and_then(|e| e.get(TERRAIN_LAYERS))
        {
            terrain_layers(layers, textures)?;
        }
    }
    let meshes = meshes(gltf, accessors.len(), materials.len())?;
    let nodes = nodes(gltf, meshes)?;
    let scenes = array(gltf, "scenes")?;
    for scene in scenes {
        for node in array(object(scene, "scene")?, "nodes")? {
            index(Some(node), nodes, "a scene")?;
        }
    }
    index(gltf.get("scene"), scenes.len(), "the scene")?;
    Ok((shared, foreign))
}

/// glTF 2.0 and static; extensions it requires are ones ORQWA knows. Returns those used.
fn extensions(gltf: &Map<String, Value>) -> Result<BTreeSet<String>, WorldError> {
    let asset = object(gltf.get("asset").unwrap_or(&Value::Null), "asset")?;
    if asset.get("version").and_then(Value::as_str) != Some("2.0")
        || asset
            .get("minVersion")
            .is_some_and(|v| v.as_str() != Some("2.0"))
    {
        return Err(invalid("not glTF 2.0"));
    }
    let used = names(gltf, "extensionsUsed")?;
    let required = names(gltf, "extensionsRequired")?;
    if let Some(other) = used.iter().find(|e| !extension_name(e)) {
        return Err(invalid(format!("{other:?} is not an extension name")));
    }
    if required.contains(TERRAIN_LAYERS) || !required.is_subset(&used) {
        return Err(invalid(format!(
            "required extensions must be used, and {TERRAIN_LAYERS} never required"
        )));
    }
    // A newer extension the file cannot be drawn without: the rider needs a newer ORQWA.
    if let Some(unknown) = required.iter().find(|e| !ALLOWED.contains(&e.as_str())) {
        return Err(WorldError::NeedsExtension(unknown.clone()));
    }
    for key in ["animations", "skins", "cameras"] {
        if !array(gltf, key)?.is_empty() {
            return Err(invalid(format!("{key} are not allowed: worlds are static")));
        }
    }
    Ok(used)
}

/// The buffer is the BIN chunk and every view lies within it; returns each view's length and
/// stride, if it sets one.
fn buffer_views(
    gltf: &Map<String, Value>,
    bin: Option<u64>,
) -> Result<Vec<(u64, Option<u64>)>, WorldError> {
    let buffers = array(gltf, "buffers")?;
    let buffer_length = match buffers {
        [] => 0,
        [buffer] => {
            let buffer = object(buffer, "buffer")?;
            let length = number(buffer, "byteLength", u64::MAX)?;
            if buffer.contains_key("uri") || bin.is_none_or(|bin| length > bin || bin - length > 3)
            {
                return Err(invalid("the buffer is not the BIN chunk"));
            }
            length
        }
        _ => return Err(invalid("more than one buffer")),
    };
    array(gltf, "bufferViews")?
        .iter()
        .map(|view| {
            let view = object(view, "buffer view")?;
            index(view.get("buffer"), buffers.len(), "a buffer view")?;
            let offset = number(view, "byteOffset", 0)?;
            let length = number(view, "byteLength", u64::MAX)?;
            let stride = number(view, "byteStride", 4)?;
            if offset
                .checked_add(length)
                .is_none_or(|end| end > buffer_length)
                || !(4..=252).contains(&stride)
                || !stride.is_multiple_of(4)
            {
                return Err(invalid("a buffer view lies outside the buffer"));
            }
            Ok((length, view.get("byteStride").map(|_| stride)))
        })
        .collect()
}

/// Images inside the file in allowed formats, textures pointing at them; returns the number
/// of textures.
/// Textures whose images are PNG, JPEG or WebP, inside the file or shared by `uri`. Images no
/// texture draws belong to extensions and may be of any type. Returns how many textures there
/// are, the shared images' URIs and the URIs of images only extensions use.
fn textures(
    gltf: &Map<String, Value>,
    views: usize,
    webp: bool,
) -> Result<(usize, Vec<String>, Vec<String>), WorldError> {
    let images = array(gltf, "images")?;
    let samplers = array(gltf, "samplers")?;
    let textures = array(gltf, "textures")?;
    let mut drawn = BTreeSet::new();
    for texture in textures {
        let texture = object(texture, "texture")?;
        drawn.extend(index(texture.get("source"), images.len(), "a texture")?);
        index(texture.get("sampler"), samplers.len(), "a texture")?;
        if webp
            && let Some(Value::Object(extensions)) = texture.get("extensions")
            && let Some(Value::Object(webp)) = extensions.get("EXT_texture_webp")
        {
            drawn.extend(index(webp.get("source"), images.len(), "a WebP texture")?);
        }
    }
    let format = |mime: Option<&str>| {
        matches!(mime, Some("image/png" | "image/jpeg")) || (webp && mime == Some("image/webp"))
    };
    let mut shared = Vec::new();
    let mut foreign = Vec::new();
    for (number, image) in images.iter().enumerate() {
        let image = object(image, "image")?;
        let mime = image.get("mimeType").and_then(Value::as_str);
        let known = drawn.contains(&number);
        match (image.get("uri"), image.get("bufferView")) {
            (None, Some(view)) => {
                index(Some(view), views, "an image")?;
                if known && !format(mime) {
                    return Err(invalid("images are PNG, JPEG or WebP"));
                }
            }
            (Some(Value::String(uri)), None) => {
                let by_name = mime_of(uri);
                let typed = format(by_name) && mime.is_none_or(|m| Some(m) == by_name);
                if !relative(uri) || (known && !typed) {
                    return Err(invalid(format!(
                        "image uri {uri:?} is not a relative path to a PNG, JPEG or WebP"
                    )));
                }
                if known {
                    shared.push(uri.clone());
                } else {
                    foreign.push(uri.clone());
                }
            }
            _ => return Err(invalid("an image is in the file or named by one uri")),
        }
    }
    Ok((textures.len(), shared, foreign))
}

/// An extension's name as glTF gives them: a prefix, an underscore, a name.
fn extension_name(name: &str) -> bool {
    (3..=64).contains(&name.len())
        && name.contains('_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// An image's type by its name's extension.
fn mime_of(uri: &str) -> Option<&'static str> {
    match uri.rsplit_once('.')?.1 {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// A plain relative path: letters, digits, `.`, `_` and `-` in segments between slashes, so it
/// needs no escaping, names no scheme and reads the same on every system. Where it leads is
/// the world's to check ([`super::resolve`]).
fn relative(uri: &str) -> bool {
    !uri.is_empty()
        && uri.len() <= 1024
        && uri.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// Meshes whose primitives point at accessors and materials; returns the number of meshes.
fn meshes(
    gltf: &Map<String, Value>,
    accessors: usize,
    materials: usize,
) -> Result<usize, WorldError> {
    let meshes = array(gltf, "meshes")?;
    for mesh in meshes {
        let primitives = array(object(mesh, "mesh")?, "primitives")?;
        if primitives.is_empty() {
            return Err(invalid("a mesh without primitives"));
        }
        for primitive in primitives {
            let primitive = object(primitive, "primitive")?;
            let attributes = object(
                primitive.get("attributes").unwrap_or(&Value::Null),
                "attributes",
            )?;
            for attribute in attributes.values() {
                index(Some(attribute), accessors, "an attribute")?;
            }
            index(primitive.get("indices"), accessors, "indices")?;
            index(primitive.get("material"), materials, "a primitive")?;
            for target in array(primitive, "targets")? {
                for attribute in object(target, "morph target")?.values() {
                    index(Some(attribute), accessors, "a morph target")?;
                }
            }
        }
    }
    Ok(meshes.len())
}

/// Nodes without cameras or skins, forming trees; returns the number of nodes.
fn nodes(gltf: &Map<String, Value>, meshes: usize) -> Result<usize, WorldError> {
    let nodes = array(gltf, "nodes")?;
    let not_a_tree = || invalid("nodes do not form a tree");
    let mut parent = vec![None; nodes.len()];
    for (at, node) in nodes.iter().enumerate() {
        let node = object(node, "node")?;
        if node.contains_key("camera") || node.contains_key("skin") {
            return Err(invalid("nodes with cameras or skins are not allowed"));
        }
        index(node.get("mesh"), meshes, "a node")?;
        for child in array(node, "children")? {
            let child =
                index(Some(child), nodes.len(), "a node's child")?.ok_or_else(not_a_tree)?;
            if child == at || parent[child].replace(at).is_some() {
                return Err(not_a_tree());
            }
        }
    }
    // With one parent each, a cycle is the only way not to reach a root: walk up from every
    // node once, remembering which nodes are known to reach one.
    let mut rooted = vec![false; nodes.len()];
    let mut path = Vec::new();
    for start in 0..nodes.len() {
        let mut at = start;
        path.clear();
        while !rooted[at] {
            if path.len() > nodes.len() {
                return Err(not_a_tree());
            }
            path.push(at);
            match parent[at] {
                Some(up) => at = up,
                None => break,
            }
        }
        for &node in &path {
            rooted[node] = true;
        }
    }
    Ok(nodes.len())
}

fn accessor_fits(
    accessor: &Map<String, Value>,
    views: &[(u64, Option<u64>)],
) -> Result<(), WorldError> {
    if accessor.contains_key("sparse") {
        return Err(invalid("sparse accessors are not allowed"));
    }
    let component = match number(accessor, "componentType", 0)? {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        _ => return Err(invalid("an accessor of an unknown component type")),
    };
    let components = match accessor.get("type").and_then(Value::as_str) {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4" | "MAT2") => 4,
        Some("MAT3") => 9,
        Some("MAT4") => 16,
        _ => return Err(invalid("an accessor of an unknown type")),
    };
    let count = number(accessor, "count", 0)?;
    if count == 0 {
        return Err(invalid("an empty accessor"));
    }
    let Some(view) = index(accessor.get("bufferView"), views.len(), "an accessor")? else {
        return Ok(());
    };
    let (length, stride) = views[view];
    let element = component * components;
    let stride = stride.unwrap_or(element);
    let end = (count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(element))
        .and_then(|span| span.checked_add(number(accessor, "byteOffset", 0).ok()?));
    if end.is_none_or(|end| end > length) || stride < element {
        return Err(invalid("an accessor reaches beyond its buffer view"));
    }
    Ok(())
}

/// Every `{"index": n}` texture reference in a material points at a texture.
fn texture_references(value: &Value, textures: usize) -> Result<(), WorldError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                // An extension this ORQWA does not know is its own to define.
                if key == "extensions"
                    && let Value::Object(extensions) = child
                {
                    for (name, data) in extensions {
                        if ALLOWED.contains(&name.as_str()) {
                            texture_references(data, textures)?;
                        }
                    }
                    continue;
                }
                if key.ends_with("Texture")
                    || key == "albedo"
                    || key == "normal"
                    || key == "weights"
                {
                    for reference in std::slice::from_ref(child)
                        .iter()
                        .chain(child.as_array().into_iter().flatten())
                        .filter(|r| r.is_object())
                    {
                        index(reference.get("index"), textures, "a material's texture")?;
                    }
                }
                texture_references(child, textures)?;
            }
            Ok(())
        }
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| texture_references(item, textures)),
        _ => Ok(()),
    }
}

fn terrain_layers(value: &Value, textures: usize) -> Result<(), WorldError> {
    let refused = || invalid(format!("{TERRAIN_LAYERS} breaks its rules"));
    let layers = object(value, TERRAIN_LAYERS)?;
    let weights = array(layers, "weights")?;
    let list = array(layers, "layers")?;
    if !(1..=2).contains(&weights.len())
        || list.is_empty()
        || list.len() > 8
        || list.len() > weights.len() * 4
    {
        return Err(refused());
    }
    for weight in weights {
        index(weight.get("index"), textures, "a terrain weight map")?.ok_or_else(refused)?;
    }
    let in_unit = |v: Option<&Value>, default: f64| {
        v.map_or(Some(default), Value::as_f64)
            .is_some_and(|v| (0.0..=1.0).contains(&v))
    };
    for layer in list {
        let layer = object(layer, "terrain layer")?;
        index(
            layer.get("albedo").and_then(|a| a.get("index")),
            textures,
            "a layer",
        )?
        .ok_or_else(refused)?;
        if let Some(normal) = layer.get("normal") {
            index(normal.get("index"), textures, "a layer's normal")?.ok_or_else(refused)?;
        }
        let size = layer.get("size_m").and_then(Value::as_f64);
        if !in_unit(layer.get("roughness"), 1.0)
            || !size.is_some_and(|s| s.is_finite() && s > 0.0 && s <= 1000.0)
        {
            return Err(refused());
        }
    }
    let fade = array(layers, "fade_m")?;
    let near = fade.first().and_then(Value::as_f64);
    let far = fade.get(1).and_then(Value::as_f64);
    if fade.len() != 2
        || !matches!((near, far), (Some(n), Some(f)) if n >= 0.0 && f > n && f <= 10_000.0)
        || !in_unit(layers.get("strength"), 1.0)
    {
        return Err(refused());
    }
    Ok(())
}
