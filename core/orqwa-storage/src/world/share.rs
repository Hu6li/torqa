//! Stores the images several of a world's glTF files embed once, under `textures/`, for those
//! files to name by `uri` (`docs/world-files.md`, "Shared textures"). A world repeats its
//! materials' images in every section: Twann – Chasseral carried 2.5 GB of images, 0.4 GB of
//! them distinct, and ORQWA read and decoded each copy again.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hasher};
use std::path::Path;

use serde_json::{Map, Value};

use super::{DESCRIPTION, Part, WorldError, description_of, invalid};

/// What [`share_textures`] did to a world's folder.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Shared {
    /// Images written under `textures/`.
    pub textures: usize,
    /// glTF files that name them now instead of holding them.
    pub files: usize,
    /// How much smaller those files are together.
    pub bytes_saved: u64,
}

/// An image by its content: a hash and its length. Images are compared byte for byte before
/// one is taken for another.
type Key = (u64, usize);

/// Moves every image that two or more of the world's glTF files in `folder` embed into a file
/// of its own under `textures/`, names it there by `uri` instead, and lists it in the folder's
/// `world.json`. An image only one file holds stays in it. Run on a world's folder before
/// packing it; run again, it finds nothing more to share, but lists again the textures files
/// already name, for a `world.json` written anew since.
///
/// # Errors
/// [`WorldError`] if `world.json` or a glTF file it names cannot be read or written, or breaks
/// the format.
pub fn share_textures(folder: &Path) -> Result<Shared, WorldError> {
    let description_path = folder.join(DESCRIPTION);
    let description = description_of(&std::fs::read(&description_path)?)?;
    let parts = description.parts()?;
    let glbs: Vec<&str> = parts
        .iter()
        .filter(|(_, part)| **part == Part::Glb)
        .map(|(name, _)| *name)
        .collect();
    let mut users: HashMap<Key, BTreeSet<&str>> = HashMap::new();
    for name in &glbs {
        let (gltf, bin) = read_glb(&folder.join(name))?;
        for image in embedded(&gltf, &bin) {
            users.entry(image.key).or_default().insert(name);
        }
    }
    let mut shared = Shared::default();
    let before: BTreeSet<String> = description.textures.iter().cloned().collect();
    let mut listed = before.clone();
    for name in &glbs {
        let path = folder.join(name);
        let (mut gltf, bin) = read_glb(&path)?;
        for uri in gltf["images"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["uri"].as_str())
        {
            if let Some(entry) = super::resolve(name, uri)
                && folder.join(&entry).is_file()
            {
                listed.insert(entry);
            }
        }
        let mut moved = BTreeMap::new();
        for image in embedded(&gltf, &bin) {
            if users.get(&image.key).map_or(0, BTreeSet::len) < 2 {
                continue;
            }
            let (hash, length) = image.key;
            let entry = format!("textures/{hash:016x}-{length}.{}", image.extension);
            let file = folder.join(&entry);
            if file.exists() {
                // Another image under the same hash and length stays where it is.
                if std::fs::read(&file)? != bin[image.range.clone()] {
                    continue;
                }
            } else {
                std::fs::create_dir_all(folder.join("textures"))?;
                std::fs::write(&file, &bin[image.range.clone()])?;
                shared.textures += 1;
            }
            listed.insert(entry.clone());
            moved.insert(image.index, entry);
        }
        if moved.is_empty() {
            continue;
        }
        let smaller = without_images(&mut gltf, &bin, name, &moved)?;
        shared.bytes_saved += bin.len().saturating_sub(smaller.len()) as u64;
        shared.files += 1;
        write_glb(&path, &gltf, &smaller)?;
    }
    if listed != before {
        let mut description: Value = serde_json::from_slice(&std::fs::read(&description_path)?)
            .map_err(|e| invalid(format!("{DESCRIPTION}: {e}")))?;
        description["textures"] = listed.into_iter().map(Value::String).collect();
        let mut text = serde_json::to_vec_pretty(&description)
            .map_err(|e| invalid(format!("{DESCRIPTION}: {e}")))?;
        text.push(b'\n');
        std::fs::write(&description_path, text)?;
    }
    Ok(shared)
}

/// An image a glTF file holds in its BIN chunk.
struct Embedded {
    index: usize,
    key: Key,
    range: std::ops::Range<usize>,
    extension: &'static str,
}

fn embedded(gltf: &Value, bin: &[u8]) -> Vec<Embedded> {
    let views = gltf["bufferViews"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let images = gltf["images"].as_array().map_or(&[][..], Vec::as_slice);
    images
        .iter()
        .enumerate()
        .filter_map(|(index, image)| {
            let view = &views[usize::try_from(image["bufferView"].as_u64()?).ok()?];
            let start = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0)).ok()?;
            let length = usize::try_from(view["byteLength"].as_u64()?).ok()?;
            let bytes = bin.get(start..start.checked_add(length)?)?;
            let extension = match image["mimeType"].as_str()? {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => return None,
            };
            let mut hasher = DefaultHasher::new();
            hasher.write(bytes);
            Some(Embedded {
                index,
                key: (hasher.finish(), length),
                range: start..start + length,
                extension,
            })
        })
        .collect()
}

/// Names the images `moved` (index → texture entry) by `uri` relative to `glb`, the file's
/// entry, and returns its BIN chunk without them: the bytes the views left cover, in order,
/// each run of them as aligned as before, so views that overlap still do.
fn without_images(
    gltf: &mut Value,
    bin: &[u8],
    glb: &str,
    moved: &BTreeMap<usize, String>,
) -> Result<Vec<u8>, WorldError> {
    let up = "../".repeat(glb.matches('/').count());
    if let Some(images) = gltf["images"].as_array_mut() {
        for (&index, entry) in moved {
            let image = images[index]
                .as_object_mut()
                .ok_or_else(|| invalid(format!("{glb}: an image is not an object")))?;
            image.remove("bufferView");
            image.remove("mimeType");
            image.insert("uri".into(), Value::String(format!("{up}{entry}")));
        }
    }
    let mut used = BTreeSet::new();
    for_each_view(gltf, |view| {
        used.insert(*view);
    });
    let mut kept: Vec<(u64, Map<String, Value>, usize, usize)> = Vec::new();
    for (old, view) in gltf["bufferViews"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let old = old as u64;
        if !used.contains(&old) {
            continue;
        }
        let view = view
            .as_object()
            .ok_or_else(|| invalid(format!("{glb}: a buffer view is not an object")))?;
        let number = |key: &str| {
            usize::try_from(view.get(key).and_then(Value::as_u64).unwrap_or(0))
                .map_err(|_| invalid(format!("{glb}: a buffer view is too large")))
        };
        let start = number("byteOffset")?;
        let end = start
            .checked_add(number("byteLength")?)
            .filter(|end| *end <= bin.len())
            .ok_or_else(|| invalid(format!("{glb}: a buffer view lies beyond its buffer")))?;
        kept.push((old, view.clone(), start, end));
    }
    // The runs of bytes the views cover, overlapping or touching ones joined.
    let mut runs: Vec<(usize, usize)> = kept
        .iter()
        .map(|(_, _, start, end)| (*start, *end))
        .collect();
    runs.sort_unstable();
    let mut joined: Vec<(usize, usize)> = Vec::new();
    for (start, end) in runs {
        match joined.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => joined.push((start, end)),
        }
    }
    let mut packed = Vec::with_capacity(bin.len());
    let mut moved_to = Vec::with_capacity(joined.len());
    for &(start, end) in &joined {
        // The same remainder by four keeps every component as aligned as it was.
        while packed.len() % 4 != start % 4 {
            packed.push(0);
        }
        moved_to.push(packed.len());
        packed.extend_from_slice(&bin[start..end]);
    }
    let mut renumbered = BTreeMap::new();
    let mut views = Vec::with_capacity(kept.len());
    for (new, (old, mut view, start, _)) in kept.into_iter().enumerate() {
        let run = joined.partition_point(|run| run.0 <= start) - 1;
        let offset = moved_to[run] + (start - joined[run].0);
        view.insert("byteOffset".into(), Value::from(offset));
        renumbered.insert(old, new as u64);
        views.push(Value::Object(view));
    }
    gltf["bufferViews"] = Value::Array(views);
    for_each_view(gltf, |view| *view = renumbered[&*view]);
    if let Some(buffer) = gltf["buffers"].get_mut(0) {
        buffer["byteLength"] = Value::from(packed.len());
    }
    Ok(packed)
}

/// Calls `visit` with every buffer view index the file's accessors and images point at.
fn for_each_view(gltf: &mut Value, mut visit: impl FnMut(&mut u64)) {
    let mut at = |value: Option<&mut Value>| {
        if let Some(value) = value
            && let Some(mut index) = value.as_u64()
        {
            visit(&mut index);
            *value = Value::from(index);
        }
    };
    for accessor in gltf["accessors"].as_array_mut().into_iter().flatten() {
        at(accessor.get_mut("bufferView"));
        if let Some(sparse) = accessor.get_mut("sparse") {
            at(sparse
                .get_mut("indices")
                .and_then(|i| i.get_mut("bufferView")));
            at(sparse
                .get_mut("values")
                .and_then(|v| v.get_mut("bufferView")));
        }
    }
    for image in gltf["images"].as_array_mut().into_iter().flatten() {
        at(image.get_mut("bufferView"));
    }
}

const JSON: u32 = 0x4E4F_534A;
const BIN: u32 = 0x004E_4942;

fn read_glb(path: &Path) -> Result<(Value, Vec<u8>), WorldError> {
    let bytes = std::fs::read(path)?;
    let at = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    let broken = || invalid(format!("{}: not a glTF binary", path.display()));
    if bytes.get(..4) != Some(b"glTF") || at(16) != Some(JSON) {
        return Err(broken());
    }
    let json_end = 20 + at(12).ok_or_else(broken)? as usize;
    let gltf: Value = serde_json::from_slice(bytes.get(20..json_end).ok_or_else(broken)?)
        .map_err(|e| invalid(format!("{}: glTF JSON: {e}", path.display())))?;
    let bin = match at(json_end + 4) {
        Some(BIN) => {
            let end = json_end + 8 + at(json_end).ok_or_else(broken)? as usize;
            bytes.get(json_end + 8..end).ok_or_else(broken)?.to_vec()
        }
        _ => Vec::new(),
    };
    Ok((gltf, bin))
}

fn write_glb(path: &Path, gltf: &Value, bin: &[u8]) -> Result<(), WorldError> {
    let mut json = serde_json::to_vec(gltf).map_err(|e| invalid(e.to_string()))?;
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bin = bin.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);
    let has_buffer = gltf["buffers"].as_array().is_some_and(|b| !b.is_empty());
    let length = 20 + json.len() + if has_buffer { 8 + bin.len() } else { 0 };
    let word = |value: usize| -> Result<[u8; 4], WorldError> {
        Ok(u32::try_from(value)
            .map_err(|_| invalid("a glTF binary is at most 4 GiB"))?
            .to_le_bytes())
    };
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&word(length)?);
    out.extend_from_slice(&word(json.len())?);
    out.extend_from_slice(&JSON.to_le_bytes());
    out.extend_from_slice(&json);
    if has_buffer {
        out.extend_from_slice(&word(bin.len())?);
        out.extend_from_slice(&BIN.to_le_bytes());
        out.extend_from_slice(&bin);
    }
    std::fs::write(path, out)?;
    Ok(())
}
