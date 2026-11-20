//! ORQWA World files (`.oqx`, ADR 0042): worlds an author built outside ORQWA, ridden as they
//! are (R266). The format is specified in `docs/world-files.md`.
//!
//! [`validate`] checks a whole file before it enters the library. [`WorldFile`] reads one that
//! is there, and checks each part again as it reads it: the file may have changed on disk since.
//! Nothing here writes to, repairs or deletes a world file.

mod archive;
pub mod glb;
pub mod instances;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub use instances::InstanceGroup;

/// File extension of world files.
pub const EXTENSION: &str = "oqx";
/// The world file format this ORQWA reads.
pub const FORMAT: u32 = 1;
/// The description inside every world file.
pub const DESCRIPTION: &str = "world.json";

/// Largest world file, packed and unpacked.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Most entries a world file may have.
pub const MAX_ENTRIES: u64 = 200_000;
const MAX_DESCRIPTION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ROUTE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PART_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SKY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

const MAX_SECTIONS: usize = 100_000;
const MAX_ASSETS: usize = 65_536;
const MAX_ROUTES: usize = 256;
const MAX_LODS: usize = 8;
const MAX_TEXT: usize = 4096;
const MAX_ATTRIBUTIONS: usize = 256;
/// The shortest route, horizontally.
const MIN_ROUTE_M: f64 = 100.0;

/// What can go wrong reading a world file.
#[derive(Debug, Error)]
pub enum WorldError {
    /// File system error.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// The file is not a readable zip archive.
    #[error("not a world file: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// A JSON part does not parse.
    #[error("{0}: {1}")]
    Json(String, serde_json::Error),
    /// The file was written for a newer ORQWA.
    #[error("world format {0} is newer than this ORQWA supports ({FORMAT}); please update")]
    NewerFormat(u32),
    /// The file breaks a rule of the format: which, and where.
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> WorldError {
    WorldError::Invalid(message.into())
}

/// A world's description, `world.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Description {
    /// Format version, [`FORMAT`].
    pub format: u32,
    /// Identifies the world across its versions: a UUID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The author's version of the world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Who made or sells the world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// The world's licence, as the author states it.
    pub licence: String,
    /// Credits its data and models require.
    #[serde(default)]
    pub attribution: Vec<String>,
    /// When the file was made, in seconds since the Unix epoch.
    pub created_unix_s: u64,
    /// Where the world's (0, 0) lies.
    pub origin: Origin,
    /// The side of the road riders keep to.
    pub drives_on: DrivesOn,
    /// The world's light.
    pub environment: Environment,
    /// The world's parts.
    pub sections: Vec<Section>,
    /// Models placed many times, by index from `instances` files.
    #[serde(default)]
    pub assets: Vec<Asset>,
    /// The far landscape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon: Option<String>,
    /// The library's picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// The routes through the world.
    pub routes: Vec<RouteEntry>,
}

/// Where a world lies on Earth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// Latitude of (0, 0), degrees.
    pub lat: f64,
    /// Longitude of (0, 0), degrees.
    pub lon: f64,
    /// Whether the world is a place on Earth, so rides keep their positions (R267).
    pub located: bool,
}

/// The side of the road riders keep to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrivesOn {
    /// On the right.
    Right,
    /// On the left.
    Left,
}

/// A world's light (R268).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// The sun.
    pub sun: Sun,
    /// Light from the sky.
    pub ambient: Ambient,
    /// The sky.
    pub sky: Sky,
    /// Fog, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
}

/// The sun of a world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sun {
    /// Degrees above the horizon.
    pub elevation_deg: f64,
    /// Degrees clockwise from north.
    pub azimuth_deg: f64,
    /// sRGB colour.
    pub colour: [f64; 3],
    /// Brightness.
    pub energy: f64,
}

/// A world's ambient light.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ambient {
    /// sRGB colour.
    pub colour: [f64; 3],
    /// Brightness.
    pub energy: f64,
}

/// A world's sky.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Sky {
    /// ORQWA's sky: the atmosphere lit by the world's sun, with clouds.
    Atmosphere {
        /// Cloud cover, 0 (none) to 1 (closed).
        clouds: f64,
        /// How grey the clouds and the sky are, 0 (fair) to 1 (overcast).
        #[serde(default)]
        overcast: f64,
    },
    /// An equirectangular picture of the sky.
    Panorama {
        /// The image in the file, under `sky/`.
        image: String,
        /// Brightness.
        energy: f64,
    },
}

/// Fog in a world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fog {
    /// sRGB colour.
    pub colour: [f64; 3],
    /// Density per metre.
    pub density: f64,
}

/// A part of the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// East, north and elevation of its centre, metres.
    pub center: [f64; 3],
    /// How far it reaches from its centre horizontally, metres.
    pub radius: f64,
    /// Its levels of detail, nearest first.
    pub lods: Vec<Lod>,
    /// The assets placed in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<String>,
}

/// A level of detail of a section or an asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lod {
    /// Its `.glb` in the file.
    pub file: String,
    /// Drawn up to this distance from the camera; the last level may reach as far as the view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_m: Option<f64>,
}

/// A model placed many times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Its name, unique in the world.
    pub name: String,
    /// Its levels of detail, nearest first.
    pub lods: Vec<Lod>,
}

/// A route through a world, as `world.json` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEntry {
    /// Identifies the route in its world.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Its points in the file, under `routes/`.
    pub file: String,
}

/// A route's points (R267).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePoints {
    /// The line riders ride, in order.
    pub points: Vec<RoutePoint>,
}

/// A point of a route.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePoint {
    /// Metres east of the origin.
    pub east: f64,
    /// Metres north of the origin.
    pub north: f64,
    /// Metres above sea level.
    pub elevation: f64,
    /// What the road lies on.
    pub surface: Surface,
    /// Width of the road, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// What a route's road lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// The ground.
    Ground,
    /// A bridge.
    Bridge,
    /// A tunnel.
    Tunnel,
}

/// What an entry of a world file is, by what `world.json` names it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Glb,
    Instances,
    Route,
    Sky,
    Preview,
}

impl Part {
    fn limit(self) -> u64 {
        match self {
            Self::Glb | Self::Instances => MAX_PART_BYTES,
            Self::Route => MAX_ROUTE_BYTES,
            Self::Sky => MAX_SKY_BYTES,
            Self::Preview => MAX_PREVIEW_BYTES,
        }
    }
}

impl Description {
    /// Checks everything the description says by itself, and lists the entries it names.
    fn parts(&self) -> Result<BTreeMap<&str, Part>, WorldError> {
        if self.format != FORMAT {
            return Err(invalid(format!("unknown world format {}", self.format)));
        }
        if !uuid(&self.id) {
            return Err(invalid("id is not a UUID"));
        }
        text("name", &self.name, 1024)?;
        text("licence", &self.licence, MAX_TEXT)?;
        for (field, value) in [("version", &self.version), ("publisher", &self.publisher)] {
            if let Some(value) = value {
                text(field, value, 1024)?;
            }
        }
        if self.attribution.len() > MAX_ATTRIBUTIONS {
            return Err(invalid("too many attributions"));
        }
        for credit in &self.attribution {
            text("attribution", credit, MAX_TEXT)?;
        }
        let Origin { lat, lon, .. } = self.origin;
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err(invalid("origin is not on Earth"));
        }
        let mut parts = BTreeMap::new();
        self.environment.check(&mut parts)?;
        if self.sections.is_empty() || self.sections.len() > MAX_SECTIONS {
            return Err(invalid("a world has 1 to 100,000 sections"));
        }
        for (index, section) in self.sections.iter().enumerate() {
            let at = || format!("section {index}");
            let placed = section.center.iter().all(|v| v.abs() <= 1e7)
                && section.radius > 0.0
                && section.radius <= 1e6;
            if !placed {
                return Err(invalid(format!("{}: centre or radius out of range", at())));
            }
            lods(&section.lods, "sections/", &mut parts).map_err(|e| prefixed(&at(), e))?;
            if let Some(file) = &section.instances {
                name(file, "sections/", ".bin", Part::Instances, &mut parts)
                    .map_err(|e| prefixed(&at(), e))?;
            }
        }
        if self.assets.len() > MAX_ASSETS {
            return Err(invalid("too many assets"));
        }
        let mut names = BTreeSet::new();
        for asset in &self.assets {
            if !identifier(&asset.name, true) || !names.insert(asset.name.as_str()) {
                return Err(invalid(format!(
                    "asset name {:?} is not allowed",
                    asset.name
                )));
            }
            lods(&asset.lods, "assets/", &mut parts)
                .map_err(|e| prefixed(&format!("asset {}", asset.name), e))?;
        }
        if let Some(horizon) = &self.horizon {
            if horizon != "horizon.glb" {
                return Err(invalid("the horizon is horizon.glb"));
            }
            parts.insert(horizon.as_str(), Part::Glb);
        }
        if let Some(preview) = &self.preview {
            if preview != "preview.png" {
                return Err(invalid("the preview is preview.png"));
            }
            parts.insert(preview.as_str(), Part::Preview);
        }
        if self.routes.is_empty() || self.routes.len() > MAX_ROUTES {
            return Err(invalid("a world has 1 to 256 routes"));
        }
        let mut ids = BTreeSet::new();
        for route in &self.routes {
            if !identifier(&route.id, false) || !ids.insert(route.id.as_str()) {
                return Err(invalid(format!("route id {:?} is not allowed", route.id)));
            }
            text("route name", &route.name, 1024)?;
            name(&route.file, "routes/", ".json", Part::Route, &mut parts)?;
        }
        Ok(parts)
    }

    /// The route `id` lists, if the world has it.
    #[must_use]
    pub fn route(&self, id: &str) -> Option<&RouteEntry> {
        self.routes.iter().find(|route| route.id == id)
    }
}

impl Environment {
    fn check<'a>(&'a self, parts: &mut BTreeMap<&'a str, Part>) -> Result<(), WorldError> {
        let Sun {
            elevation_deg,
            azimuth_deg,
            colour,
            energy,
        } = self.sun;
        if !(-10.0..=90.0).contains(&elevation_deg)
            || !(0.0..360.0).contains(&azimuth_deg)
            || !colour_ok(colour)
            || !(0.0..=16.0).contains(&energy)
        {
            return Err(invalid("sun out of range"));
        }
        if !colour_ok(self.ambient.colour) || !(0.0..=16.0).contains(&self.ambient.energy) {
            return Err(invalid("ambient light out of range"));
        }
        match &self.sky {
            Sky::Atmosphere { clouds, overcast } => {
                if !(0.0..=1.0).contains(clouds) || !(0.0..=1.0).contains(overcast) {
                    return Err(invalid("sky out of range"));
                }
            }
            Sky::Panorama { image, energy } => {
                if !(0.0..=16.0).contains(energy) {
                    return Err(invalid("sky out of range"));
                }
                let extension = [".png", ".jpg", ".jpeg", ".webp"]
                    .into_iter()
                    .find(|extension| image.ends_with(extension))
                    .ok_or_else(|| invalid("a sky panorama is PNG, JPEG or WebP"))?;
                name(image, "sky/", extension, Part::Sky, parts)?;
            }
        }
        if let Some(fog) = self.fog
            && (!colour_ok(fog.colour) || !(0.0..=0.01).contains(&fog.density))
        {
            return Err(invalid("fog out of range"));
        }
        Ok(())
    }
}

fn prefixed(at: &str, error: WorldError) -> WorldError {
    match error {
        WorldError::Invalid(message) => invalid(format!("{at}: {message}")),
        other => other,
    }
}

fn colour_ok(colour: [f64; 3]) -> bool {
    colour.iter().all(|c| (0.0..=1.0).contains(c))
}

fn text(field: &str, value: &str, max: usize) -> Result<(), WorldError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} is empty, too long or has control characters"
        )));
    }
    Ok(())
}

fn uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn identifier(id: &str, underscore: bool) -> bool {
    (1..=64).contains(&id.len())
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || (underscore && b == b'_')
        })
}

fn name<'a>(
    file: &'a str,
    folder: &str,
    extension: &str,
    part: Part,
    parts: &mut BTreeMap<&'a str, Part>,
) -> Result<(), WorldError> {
    let inside = file
        .strip_prefix(folder)
        .and_then(|rest| rest.strip_suffix(extension));
    if inside.is_none_or(str::is_empty) || !crate::course::shared::safe(file, false) {
        return Err(invalid(format!(
            "{file:?} is not a {folder}…{extension} name"
        )));
    }
    if parts
        .insert(file, part)
        .is_some_and(|before| before != part)
    {
        return Err(invalid(format!("{file} is named for two purposes")));
    }
    Ok(())
}

fn lods<'a>(
    lods: &'a [Lod],
    folder: &str,
    parts: &mut BTreeMap<&'a str, Part>,
) -> Result<(), WorldError> {
    if lods.is_empty() || lods.len() > MAX_LODS {
        return Err(invalid("1 to 8 levels of detail"));
    }
    let mut nearer = 0.0;
    for (index, lod) in lods.iter().enumerate() {
        match lod.until_m {
            Some(until) if until.is_finite() && until > nearer && until <= 1e6 => nearer = until,
            None if index + 1 == lods.len() => {}
            _ => {
                return Err(invalid(
                    "levels of detail must reach further each, nearest first",
                ));
            }
        }
        name(&lod.file, folder, ".glb", Part::Glb, parts)?;
    }
    Ok(())
}

impl RoutePoints {
    /// Checks a route against the format's rules.
    ///
    /// # Errors
    /// [`WorldError::Invalid`] naming the rule broken.
    pub fn check(&self) -> Result<(), WorldError> {
        if self.points.len() < 2 {
            return Err(invalid("a route has at least two points"));
        }
        let mut length = 0.0;
        for (index, point) in self.points.iter().enumerate() {
            if !(point.east.is_finite() && point.north.is_finite())
                || point.east.abs() > 1e7
                || point.north.abs() > 1e7
                || !(-500.0..=9000.0).contains(&point.elevation)
                || point.width.is_some_and(|w| !(w > 0.0 && w <= 100.0))
            {
                return Err(invalid(format!("route point {index} out of range")));
            }
            if index > 0 {
                let before = self.points[index - 1];
                let step = (point.east - before.east).hypot(point.north - before.north);
                if step <= 0.0 {
                    return Err(invalid(format!(
                        "route point {index} repeats the one before"
                    )));
                }
                length += step;
            }
        }
        if length < MIN_ROUTE_M {
            return Err(invalid("a route is at least 100 m long"));
        }
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(name: &str, bytes: &[u8]) -> Result<T, WorldError> {
    serde_json::from_slice(bytes).map_err(|e| WorldError::Json(name.to_owned(), e))
}

/// Parses a description, refusing a newer format before reading anything else of it.
fn description_of(bytes: &[u8]) -> Result<Description, WorldError> {
    #[derive(Deserialize)]
    struct Format {
        format: u32,
    }
    let Format { format } = parse(DESCRIPTION, bytes)?;
    if format > FORMAT {
        return Err(WorldError::NewerFormat(format));
    }
    let description: Description = parse(DESCRIPTION, bytes)?;
    description.parts()?;
    Ok(description)
}

fn read_capped(entry: impl Read, declared: u64, limit: u64) -> Result<Vec<u8>, WorldError> {
    if declared > limit {
        return Err(invalid("an entry exceeds its size limit"));
    }
    let mut bytes =
        Vec::with_capacity(usize::try_from(declared).map_err(|_| invalid("too large"))?);
    entry.take(declared + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != declared {
        return Err(invalid("an entry is not as long as its archive says"));
    }
    Ok(bytes)
}

fn open_zip(path: &Path) -> Result<ZipArchive<BufReader<File>>, WorldError> {
    let mut file = File::open(path)?;
    let count = archive::entries(&mut file)?;
    let zip = ZipArchive::new(BufReader::new(file))?;
    if zip.len() as u64 != count {
        return Err(invalid(
            "the archive's directory does not match its trailer",
        ));
    }
    Ok(zip)
}

/// Reads a world's description only, e.g. for listing the library.
///
/// # Errors
/// [`WorldError`] if the file cannot be read, is of a newer format or its description breaks
/// the format's rules.
pub fn read_description(path: &Path) -> Result<Description, WorldError> {
    let mut zip = open_zip(path)?;
    let entry = zip.by_name(DESCRIPTION)?;
    let size = entry.size();
    description_of(&read_capped(entry, size, MAX_DESCRIPTION_BYTES)?)
}

/// Checks a whole world file against the format (R266): its archive, its description and every
/// part it names, each read to its last byte. Run on a worker: it reads gigabytes.
///
/// # Errors
/// [`WorldError`] naming the first rule the file breaks.
pub fn validate(path: &Path) -> Result<Description, WorldError> {
    let mut zip = open_zip(path)?;
    let description = {
        let entry = zip.by_name(DESCRIPTION)?;
        let size = entry.size();
        description_of(&read_capped(entry, size, MAX_DESCRIPTION_BYTES)?)?
    };
    let parts = description.parts()?;
    let mut seen = BTreeSet::new();
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name()?.into_owned();
        let mode = entry.unix_mode().unwrap_or(0) & 0o170_000;
        if entry.is_dir()
            || !crate::course::shared::safe(&name, false)
            || (mode != 0 && mode != 0o100_000)
            || entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || !seen.insert(name.clone())
        {
            return Err(invalid(format!("{name}: not a plain, safely named file")));
        }
        let declared = entry.size();
        expanded = expanded.saturating_add(declared);
        if expanded > MAX_BYTES {
            return Err(invalid("the world is larger than 64 GiB unpacked"));
        }
        let at = |e| prefixed(&name, e);
        if name == DESCRIPTION {
            io::copy(&mut entry, &mut io::sink())?;
            continue;
        }
        let part = *parts
            .get(name.as_str())
            .ok_or_else(|| invalid(format!("{name}: not named by world.json")))?;
        if declared > part.limit() {
            return Err(invalid(format!("{name}: larger than its limit")));
        }
        let mut entry = (&mut entry).take(declared + 1);
        let read = match part {
            Part::Glb => glb::check_stream(&mut entry, declared).map_err(at)?,
            Part::Instances => {
                instances::read(&mut entry, description.assets.len(), false)
                    .map_err(at)?
                    .1
            }
            Part::Route => {
                let bytes = read_capped(&mut entry, declared, part.limit())?;
                let route: RoutePoints = parse(&name, &bytes)?;
                route.check().map_err(at)?;
                declared
            }
            Part::Sky | Part::Preview => image(&mut entry, part == Part::Preview).map_err(at)?,
        };
        let rest = io::copy(&mut entry, &mut io::sink())?;
        if part == Part::Instances && rest != 0 {
            return Err(invalid(format!("{name}: bytes after its last group")));
        }
        if read + rest != declared {
            return Err(invalid(format!("{name}: not as long as its archive says")));
        }
    }
    if let Some(missing) = parts.keys().find(|name| !seen.contains(**name)) {
        return Err(invalid(format!(
            "{missing}: named by world.json but missing"
        )));
    }
    Ok(description)
}

/// Checks an image's signature, reading its first bytes; returns how many it read.
fn image(input: &mut impl Read, png_only: bool) -> Result<u64, WorldError> {
    let mut head = [0u8; 12];
    let mut read = 0;
    while read < head.len() {
        match input.read(&mut head[read..])? {
            0 => break,
            n => read += n,
        }
    }
    let head = &head[..read];
    let png = head.starts_with(b"\x89PNG\r\n\x1a\n");
    let other = head.starts_with(b"\xff\xd8\xff")
        || (head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP"));
    if png || (!png_only && other) {
        Ok(read as u64)
    } else {
        Err(invalid(if png_only {
            "not a PNG image"
        } else {
            "not a PNG, JPEG or WebP image"
        }))
    }
}

/// A world file in the library, open for riding. It reads only what its description names,
/// checks each part again as it reads it, and never writes to or deletes the file.
pub struct WorldFile {
    path: PathBuf,
    description: Description,
    parts: BTreeMap<String, u64>,
    zip: Mutex<ZipArchive<BufReader<File>>>,
}

impl std::fmt::Debug for WorldFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldFile")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl WorldFile {
    /// Opens the world file at `path`, reading its description.
    ///
    /// # Errors
    /// As [`read_description`].
    pub fn open(path: &Path) -> Result<Self, WorldError> {
        let mut zip = open_zip(path)?;
        let description = {
            let entry = zip.by_name(DESCRIPTION)?;
            let size = entry.size();
            description_of(&read_capped(entry, size, MAX_DESCRIPTION_BYTES)?)?
        };
        let parts = description
            .parts()?
            .into_iter()
            .map(|(name, part)| (name.to_owned(), part.limit()))
            .collect();
        Ok(Self {
            path: path.to_owned(),
            description,
            parts,
            zip: Mutex::new(zip),
        })
    }

    /// Where the file is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its description.
    #[must_use]
    pub fn description(&self) -> &Description {
        &self.description
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, WorldError> {
        let limit = *self
            .parts
            .get(name)
            .ok_or_else(|| invalid(format!("{name}: not named by world.json")))?;
        let mut zip = self
            .zip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = zip.by_name(name)?;
        let size = entry.size();
        read_capped(entry, size, limit).map_err(|e| prefixed(name, e))
    }

    /// The bytes of a `.glb` the description names, checked against the format.
    ///
    /// # Errors
    /// [`WorldError`] if it cannot be read or breaks the format.
    pub fn glb(&self, name: &str) -> Result<Vec<u8>, WorldError> {
        let bytes = self.read(name)?;
        glb::check_stream(&mut bytes.as_slice(), bytes.len() as u64)
            .map_err(|e| prefixed(name, e))?;
        Ok(bytes)
    }

    /// The copies of assets an `instances` file places.
    ///
    /// # Errors
    /// [`WorldError`] if it cannot be read or breaks the format.
    pub fn instances(&self, name: &str) -> Result<Vec<InstanceGroup>, WorldError> {
        let bytes = self.read(name)?;
        let (groups, read) =
            instances::read(&mut bytes.as_slice(), self.description.assets.len(), true)
                .map_err(|e| prefixed(name, e))?;
        if read != bytes.len() as u64 {
            return Err(invalid(format!("{name}: bytes after its last group")));
        }
        Ok(groups)
    }

    /// The points of the route `id`.
    ///
    /// # Errors
    /// [`WorldError`] if the world has no such route, or it cannot be read or breaks the format.
    pub fn route(&self, id: &str) -> Result<RoutePoints, WorldError> {
        let entry = self
            .description
            .route(id)
            .ok_or_else(|| invalid(format!("no route {id}")))?;
        let bytes = self.read(&entry.file)?;
        let route: RoutePoints = parse(&entry.file, &bytes)?;
        route.check().map_err(|e| prefixed(&entry.file, e))?;
        Ok(route)
    }

    /// The bytes of an image the description names: the preview or a sky panorama.
    ///
    /// # Errors
    /// [`WorldError`] if it cannot be read or is not an image of its kind.
    pub fn image(&self, name: &str) -> Result<Vec<u8>, WorldError> {
        let bytes = self.read(name)?;
        image(
            &mut bytes.as_slice(),
            Some(name) == self.description.preview.as_deref(),
        )
        .map_err(|e| prefixed(name, e))?;
        Ok(bytes)
    }
}

/// Packs the folder `from` into the world file `to`: `world.json` and every part it names,
/// under their names relative to the folder, then checks the result as importing would.
/// Written beside `to` first and moved there once checked, so a failed pack leaves nothing.
/// For tools that write worlds as folders.
///
/// # Errors
/// [`WorldError`] if the folder's description or a part is missing or breaks the format.
pub fn pack(from: &Path, to: &Path) -> Result<Description, WorldError> {
    let description = description_of(&std::fs::read(from.join(DESCRIPTION))?)?;
    let parts = description.parts()?;
    let partial = to.with_extension("oqx.part");
    let written = (|| {
        let mut zip = ZipWriter::new(io::BufWriter::new(File::create(&partial)?));
        // Parts already compressed (textures in glTF, images) gain nothing from deflating, and
        // stored ones read faster.
        let stored = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(true);
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(true);
        let names =
            std::iter::once((DESCRIPTION, Part::Route)).chain(parts.iter().map(|(n, p)| (*n, *p)));
        for (name, part) in names {
            let options = match part {
                Part::Glb | Part::Sky | Part::Preview => stored,
                Part::Instances | Part::Route => deflated,
            };
            zip.start_file(name, options)?;
            io::copy(&mut File::open(from.join(name))?, &mut zip)?;
        }
        zip.finish()?
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        validate(&partial)
    })();
    match written {
        Ok(description) => {
            std::fs::rename(&partial, to)?;
            Ok(description)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            Err(error)
        }
    }
}
