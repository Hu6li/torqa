//! A region's map detail, made from its `.osm.pbf`: the roads whose surface the vector tiles
//! cannot tell, the roundabouts and what buildings are (ADR 0041), written as small JSON cells
//! (tiles at [`CELL_ZOOM`]) in a directory named for the region and the date of its data.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use prost::Message;
use serde::{Deserialize, Serialize};

use super::pbf::{self, NodeBlock, WayBlock};
use crate::mvt::{road_class, road_kind, road_surface};
use crate::{BuildingKind, OsmError, RoadKind, RoadSurface, tile_of};

/// The directory of extracts in the map cache. Raised when what an extract holds changes, so
/// they are made again from the regions' downloads.
pub const VERSION: &str = "detail-v1";
/// Extracts are cut into the tiles of this zoom, about 6 km wide in central Europe: a course
/// takes along the few it passes.
pub(crate) const CELL_ZOOM: u8 = 12;
/// Segments are followed in steps this long (degrees, about 1 km), so a long one is in every
/// cell it crosses, not only those of its ends.
const STEP: f64 = 0.01;
/// A node not (yet) found in the file.
const NOWHERE: i64 = i64::MIN;

/// What part of an extraction is under way, for its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading the file for its ways (bytes).
    Ways,
    /// Reading the file again for where their nodes are (bytes).
    Nodes,
    /// Writing the cells (cells).
    Cells,
}

/// A region's map detail, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extract {
    /// Its directory: the region and the date of its data, e.g. `switzerland-2026-10-24`.
    pub name: String,
    /// When the region's data was current (Unix seconds), as the download says.
    pub data_time: Option<i64>,
    /// Roads with a surface the tiles cannot tell.
    pub surfaces: usize,
    /// Roads going round a roundabout.
    pub roundabouts: usize,
    /// Buildings whose kind the map tells.
    pub buildings: usize,
    /// Cells written.
    pub cells: usize,
    /// Bytes written.
    pub bytes: u64,
}

/// What a cell holds: the detail of the ways in it, whole even where they run on beyond it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cell {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<SurfaceWay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roundabouts: Vec<Roundabout>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buildings: Vec<KindAt>,
}

/// What a building is, at a point within its outline: halfway between its first corner and
/// the one halfway round it, which for the outlines of buildings lies inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct KindAt {
    /// Latitude, longitude in 1e-7 degrees.
    pub at: [i32; 2],
    pub kind: BuildingKind,
}

/// A road with a surface the tiles cannot tell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SurfaceWay {
    /// The OpenStreetMap way, so ways in several cells or regions are taken once.
    pub id: i64,
    pub kind: RoadKind,
    pub surface: RoadSurface,
    /// Latitude, longitude in 1e-7 degrees, as OpenStreetMap keeps them.
    pub line: Vec<[i32; 2]>,
}

/// A road going round a roundabout, drawn the way traffic goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Roundabout {
    pub id: i64,
    pub kind: RoadKind,
    pub line: Vec<[i32; 2]>,
}

/// Where the cell (x, y) of extract `name` is under `dir`, the map cache's directory of
/// extracts.
pub(crate) fn cell_path(dir: &Path, name: &str, (x, y): (u32, u32)) -> PathBuf {
    dir.join(name)
        .join(CELL_ZOOM.to_string())
        .join(x.to_string())
        .join(format!("{y}.json"))
}

/// Reads the region's download `pbf` and writes its map detail under `dir`, as
/// `<region>-<date of the data>`, replacing an extract of that name. It is written aside and
/// moved into place whole, so an extract is never seen half written. Once `stop` is set, it
/// stops.
///
/// # Errors
/// [`OsmError::Cache`] if the file cannot be read or the extract written,
/// [`OsmError::Invalid`] if it is no OpenStreetMap download this can read,
/// [`OsmError::Stopped`] once stopped.
pub fn extract(
    pbf: &Path,
    dir: &Path,
    region: &str,
    progress: &mut dyn FnMut(Stage, u64, u64),
    stop: &AtomicBool,
) -> Result<Extract, OsmError> {
    let (header, found) = pbf::fold(
        pbf,
        Ways::default,
        Ways::read,
        &mut |done, size| progress(Stage::Ways, done, size),
        stop,
    )?;
    let ways = Ways::joined(found);
    let mut needed = ways.refs.clone();
    needed.extend(ways.buildings.iter().flat_map(|b| b.corners()));
    needed.sort_unstable();
    needed.dedup();
    let positions: Vec<AtomicI64> = needed.iter().map(|_| AtomicI64::new(NOWHERE)).collect();
    pbf::fold(
        pbf,
        || (),
        |(), bytes| {
            let block = NodeBlock::decode(bytes).map_err(pbf::invalid)?;
            block.each(|id, (lat, lon)| {
                if let Ok(i) = needed.binary_search(&id) {
                    positions[i].store(pack(lat, lon), Ordering::Relaxed);
                }
            });
            Ok(())
        },
        &mut |done, size| progress(Stage::Nodes, done, size),
        stop,
    )?;
    let at = |node: &i64| {
        let i = needed.binary_search(node).ok()?;
        unpack(positions[i].load(Ordering::Relaxed))
    };

    let mut cells: HashMap<(u32, u32), Cell> = HashMap::new();
    let (surfaces, roundabouts) = place_ways(&ways, &at, &mut cells);
    let buildings = place_buildings(&ways.buildings, &at, &mut cells);

    let data_time = header.data_time;
    let name = format!(
        "{}-{}",
        file_name(region),
        date(data_time.or_else(|| modified(pbf)).unwrap_or(0))
    );
    let aside_name = format!("{name}.part");
    let aside = dir.join(&aside_name);
    if aside.exists() {
        fs::remove_dir_all(&aside)?;
    }
    let mut bytes = 0;
    let total = cells.len() as u64;
    for (done, (&cell, content)) in cells.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Err(OsmError::Stopped);
        }
        let path = cell_path(dir, &aside_name, cell);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec(content).map_err(|e| OsmError::Invalid(e.to_string()))?;
        fs::write(&path, &json)?;
        bytes += json.len() as u64;
        progress(Stage::Cells, done as u64 + 1, total);
    }
    fs::create_dir_all(&aside)?;
    let place = dir.join(&name);
    if place.exists() {
        fs::remove_dir_all(&place)?;
    }
    fs::rename(&aside, &place)?;
    Ok(Extract {
        name,
        data_time,
        surfaces,
        roundabouts,
        buildings,
        cells: cells.len(),
        bytes,
    })
}

/// Puts each road in every cell it passes, whole, given where its nodes are (`at`); how many
/// have a surface and how many go round a roundabout.
fn place_ways(
    ways: &Ways,
    at: &impl Fn(&i64) -> Option<[i32; 2]>,
    cells: &mut HashMap<(u32, u32), Cell>,
) -> (usize, usize) {
    let (mut surfaces, mut roundabouts) = (0, 0);
    for way in &ways.found {
        // Nodes the download lacks are left out; Geofabrik keeps the ways crossing its border
        // whole, so this is rare.
        let line: Vec<[i32; 2]> = ways.refs[way.refs.clone()].iter().filter_map(at).collect();
        if line.len() < 2 {
            continue;
        }
        for cell in cells_of(&line) {
            let cell = cells.entry(cell).or_default();
            if let Some(surface) = way.surface {
                cell.surfaces.push(SurfaceWay {
                    id: way.id,
                    kind: way.kind,
                    surface,
                    line: line.clone(),
                });
            }
            if way.roundabout {
                cell.roundabouts.push(Roundabout {
                    id: way.id,
                    kind: way.kind,
                    line: line.clone(),
                });
            }
        }
        surfaces += usize::from(way.surface.is_some());
        roundabouts += usize::from(way.roundabout);
    }
    (surfaces, roundabouts)
}

/// Puts each building's kind in the cell of its point, given where its corners are (`at`);
/// how many found their place.
fn place_buildings(
    buildings: &[FoundBuilding],
    at: &impl Fn(&i64) -> Option<[i32; 2]>,
    cells: &mut HashMap<(u32, u32), Cell>,
) -> usize {
    let mut placed = 0;
    for building in buildings {
        let [Some(a), Some(b)] = building.corners().map(|node| at(&node)) else {
            continue;
        };
        let middle = [0, 1].map(|k| i32::midpoint(a[k], b[k]));
        let degrees = (f64::from(middle[0]) * 1e-7, f64::from(middle[1]) * 1e-7);
        cells
            .entry(tile_of(CELL_ZOOM, degrees.0, degrees.1))
            .or_default()
            .buildings
            .push(KindAt {
                at: middle,
                kind: building.kind(),
            });
        placed += 1;
    }
    placed
}

/// The ways found in the blocks one thread read; their nodes one list.
#[derive(Default)]
struct Ways {
    found: Vec<Found>,
    refs: Vec<i64>,
    buildings: Vec<FoundBuilding>,
}

/// A building whose kind the map tells, and two of its corners: its first and the one halfway
/// round. Tens of millions of them are held at once for a large region, so its kind is kept
/// in the top byte of the first corner's node id (OpenStreetMap's ids are far below 2^56).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FoundBuilding([i64; 2]);

impl FoundBuilding {
    const SHIFT: u32 = 56;

    fn new(kind: BuildingKind, corners: [i64; 2]) -> Option<Self> {
        let index = BuildingKind::ALL.iter().position(|&k| k == kind)?;
        let index = i64::try_from(index).ok()?;
        (corners[0] >> Self::SHIFT == 0)
            .then_some(Self([corners[0] | (index << Self::SHIFT), corners[1]]))
    }

    fn kind(self) -> BuildingKind {
        let index = usize::try_from(self.0[0] >> Self::SHIFT).unwrap_or(0);
        BuildingKind::ALL[index.min(BuildingKind::ALL.len() - 1)]
    }

    fn corners(self) -> [i64; 2] {
        [self.0[0] & ((1 << Self::SHIFT) - 1), self.0[1]]
    }
}

struct Found {
    id: i64,
    kind: RoadKind,
    surface: Option<RoadSurface>,
    roundabout: bool,
    /// Its nodes in [`Ways::refs`].
    refs: Range<usize>,
}

impl Ways {
    fn read(&mut self, bytes: &[u8]) -> Result<(), OsmError> {
        let block = WayBlock::decode(bytes).map_err(pbf::invalid)?;
        for way in block.primitivegroup.iter().flat_map(|group| &group.ways) {
            let mut tags = Tags::default();
            for (&key, &value) in way.keys.iter().zip(&way.vals) {
                let value = std::str::from_utf8(block.text(value)).unwrap_or_default();
                match block.text(key) {
                    b"highway" => tags.highway = value,
                    b"surface" => tags.surface = value,
                    b"junction" => tags.junction = value,
                    b"bicycle" => tags.bicycle = value,
                    b"building" => tags.building = value,
                    _ => {}
                }
            }
            if let Some(kind) = BuildingKind::from_tag(tags.building) {
                let mut node = 0;
                let nodes: Vec<i64> = way
                    .refs
                    .iter()
                    .map(|delta| {
                        node += delta;
                        node
                    })
                    .collect();
                if nodes.len() >= 4 {
                    self.buildings
                        .extend(FoundBuilding::new(kind, [nodes[0], nodes[nodes.len() / 2]]));
                }
                continue;
            }
            let Some((kind, surface, roundabout)) = tags.detail() else {
                continue;
            };
            let start = self.refs.len();
            let mut node = 0;
            self.refs.extend(way.refs.iter().map(|delta| {
                node += delta;
                node
            }));
            self.found.push(Found {
                id: way.id,
                kind,
                surface,
                roundabout,
                refs: start..self.refs.len(),
            });
        }
        Ok(())
    }

    /// The ways each thread found, as one, in the order of their ids: the same download makes
    /// the same cells.
    fn joined(parts: Vec<Ways>) -> Ways {
        let mut all = Ways::default();
        for part in parts {
            let offset = all.refs.len();
            all.refs.extend(part.refs);
            all.found.extend(part.found.into_iter().map(|way| Found {
                refs: way.refs.start + offset..way.refs.end + offset,
                ..way
            }));
            all.buildings.extend(part.buildings);
        }
        all.found.sort_unstable_by_key(|way| way.id);
        // By their corners: the same download makes the same cells.
        all.buildings.sort_unstable();
        all
    }
}

/// The tags of a way that map detail reads.
#[derive(Default)]
struct Tags<'a> {
    highway: &'a str,
    surface: &'a str,
    junction: &'a str,
    bicycle: &'a str,
    building: &'a str,
}

impl Tags<'_> {
    /// What the way is to map detail: a road the tiles keep (as `road_class` decides for
    /// them), with a surface they cannot tell or going round a roundabout.
    fn detail(&self) -> Option<(RoadKind, Option<RoadSurface>, bool)> {
        let class = tiles_class(self.highway)?;
        road_class(class, self.highway, self.bicycle)?;
        let surface = Some(road_surface(self.surface)).filter(|surface| {
            matches!(
                surface,
                RoadSurface::Concrete
                    | RoadSurface::Paving
                    | RoadSurface::Cobbles
                    | RoadSurface::Dirt
                    | RoadSurface::Grass
            )
        });
        // Both go round one way only, as drawn (OpenStreetMap's `junction` tag).
        let roundabout = matches!(self.junction, "roundabout" | "circular");
        (surface.is_some() || roundabout).then_some((
            road_kind(class, self.highway),
            surface,
            roundabout,
        ))
    }
}

/// The `class` the tiles (`OpenMapTiles`) give a way of this `highway`; its `subclass` there is
/// the `highway` itself.
fn tiles_class(highway: &str) -> Option<&'static str> {
    Some(match highway {
        "motorway" | "motorway_link" => "motorway",
        "trunk" | "trunk_link" => "trunk",
        "primary" | "primary_link" => "primary",
        "secondary" | "secondary_link" => "secondary",
        "tertiary" | "tertiary_link" => "tertiary",
        "unclassified" | "residential" | "living_street" | "road" => "minor",
        "service" => "service",
        "track" => "track",
        "path" | "footway" | "cycleway" | "bridleway" | "steps" | "pedestrian" | "corridor"
        | "platform" => "path",
        _ => return None,
    })
}

/// The cells a line passes.
fn cells_of(line: &[[i32; 2]]) -> BTreeSet<(u32, u32)> {
    let degrees = |[lat, lon]: [i32; 2]| (f64::from(lat) * 1e-7, f64::from(lon) * 1e-7);
    let mut cells = BTreeSet::new();
    for pair in line.windows(2) {
        let (a, b) = (degrees(pair[0]), degrees(pair[1]));
        let span = (b.0 - a.0).abs().max((b.1 - a.1).abs());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few steps at most
        let steps = (span / STEP).ceil().max(1.0) as u32;
        for step in 0..=steps {
            let t = f64::from(step) / f64::from(steps);
            cells.insert(tile_of(
                CELL_ZOOM,
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
            ));
        }
    }
    cells
}

/// A position as one number, so threads can each fill in their nodes' without a lock.
fn pack(lat: i32, lon: i32) -> i64 {
    (i64::from(lat) << 32) | i64::from(lon.cast_unsigned())
}

#[allow(clippy::cast_possible_truncation)] // the halves packed above
fn unpack(packed: i64) -> Option<[i32; 2]> {
    (packed != NOWHERE).then_some([(packed >> 32) as i32, packed as i32])
}

/// A region's name as part of a directory's: letters, digits, `-` and `_`.
pub(crate) fn file_name(region: &str) -> String {
    region
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// When the file was last written (Unix seconds), for downloads that do not say how old their
/// data is.
fn modified(path: &Path) -> Option<i64> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    let seconds = time.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok()
}

/// The UTC date of a time (Unix seconds) as `yyyy-mm-dd`.
fn date(time: i64) -> String {
    // Howard Hinnant's days-to-civil algorithm.
    let days = time.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * of_year + 2) / 153;
    let day = of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::pbf::{
        DenseNodes, HeaderBlock, NodeBlock, NodeGroup, StringTable, Way, WayBlock, WayGroup, write,
    };
    use super::*;

    /// A way of the test file: its tags and its nodes (see `NODES`).
    pub(crate) struct TestWay {
        pub id: i64,
        pub tags: &'static [(&'static str, &'static str)],
        pub nodes: &'static [i64],
    }

    /// Nodes 1–6 along Bern's Kramgasse, eastwards; 7 and 8 far to the east and the west of
    /// them, in other cells; 9 is in no block of the file.
    const NODES: [(i64, (i32, i32)); 8] = [
        (1, (469_480_000, 74_470_000)),
        (2, (469_480_500, 74_480_000)),
        (3, (469_481_000, 74_490_000)),
        (4, (469_481_500, 74_500_000)),
        (5, (469_482_000, 74_510_000)),
        (6, (469_482_500, 74_520_000)),
        (7, (469_480_000, 75_100_000)),
        (8, (469_480_000, 73_800_000)),
    ];

    /// A download of `NODES` and `ways`, with data current at `data_time` (Unix seconds).
    pub(crate) fn test_file(ways: &[TestWay], data_time: i64) -> Vec<u8> {
        let mut previous = (0, 0, 0);
        let mut dense = DenseNodes {
            id: Vec::new(),
            lat: Vec::new(),
            lon: Vec::new(),
        };
        for &(id, (lat, lon)) in &NODES {
            let (lat, lon) = (i64::from(lat), i64::from(lon));
            dense.id.push(id - previous.0);
            dense.lat.push(lat - previous.1);
            dense.lon.push(lon - previous.2);
            previous = (id, lat, lon);
        }
        let nodes = NodeBlock {
            primitivegroup: vec![NodeGroup {
                nodes: Vec::new(),
                dense: Some(dense),
            }],
            granularity: None,
            lat_offset: None,
            lon_offset: None,
        };
        let mut strings: Vec<Vec<u8>> = vec![Vec::new()];
        let mut index = |text: &str| {
            let at = strings.iter().position(|s| s == text.as_bytes());
            u32::try_from(at.unwrap_or_else(|| {
                strings.push(text.as_bytes().to_vec());
                strings.len() - 1
            }))
            .unwrap()
        };
        let ways: Vec<Way> = ways
            .iter()
            .map(|way| {
                let mut last = 0;
                Way {
                    id: way.id,
                    keys: way.tags.iter().map(|(k, _)| index(k)).collect(),
                    vals: way.tags.iter().map(|(_, v)| index(v)).collect(),
                    refs: way
                        .nodes
                        .iter()
                        .map(|&n| {
                            let d = n - last;
                            last = n;
                            d
                        })
                        .collect(),
                }
            })
            .collect();
        let ways = WayBlock {
            stringtable: StringTable { s: strings },
            primitivegroup: vec![WayGroup { ways }],
        };
        let header = HeaderBlock {
            required_features: vec!["OsmSchema-V0.6".to_owned(), "DenseNodes".to_owned()],
            osmosis_replication_timestamp: Some(data_time),
        };
        write::file(&header, &[nodes.encode_to_vec(), ways.encode_to_vec()])
    }

    fn extract_of(name: &str, ways: &[TestWay]) -> (PathBuf, Extract) {
        let dir = std::env::temp_dir().join(format!("orqwa-detail-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pbf = dir.join("bern-latest.osm.pbf");
        // 2026-10-24 21:00 UTC, a Geofabrik update.
        fs::write(&pbf, test_file(ways, 1_792_875_600)).unwrap();
        let extract = extract(
            &pbf,
            &dir.join(VERSION),
            "bern",
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        (dir, extract)
    }

    fn cells(dir: &Path, extract: &Extract) -> Vec<((u32, u32), Cell)> {
        let root = dir.join(VERSION).join(&extract.name).join("12");
        let mut cells = Vec::new();
        for x in fs::read_dir(&root).unwrap() {
            let x = x.unwrap();
            for y in fs::read_dir(x.path()).unwrap() {
                let y = y.unwrap().path();
                let cell = (
                    x.file_name().to_str().unwrap().parse().unwrap(),
                    y.file_stem().unwrap().to_str().unwrap().parse().unwrap(),
                );
                cells.push((
                    cell,
                    serde_json::from_slice(&fs::read(&y).unwrap()).unwrap(),
                ));
            }
        }
        cells.sort_by_key(|(cell, _)| *cell);
        cells
    }

    fn kramgasse() -> (u32, u32) {
        tile_of(CELL_ZOOM, 46.948, 7.447)
    }

    #[test]
    fn keeps_the_surfaces_the_tiles_cannot_tell_of_roads_they_keep() {
        let (dir, extract) = extract_of(
            "surfaces",
            &[
                TestWay {
                    id: 10,
                    tags: &[("highway", "residential"), ("surface", "sett")],
                    nodes: &[1, 2, 3],
                },
                // Asphalt and gravel the tiles tell already, as paved and unpaved.
                TestWay {
                    id: 11,
                    tags: &[("highway", "residential"), ("surface", "asphalt")],
                    nodes: &[3, 4],
                },
                TestWay {
                    id: 12,
                    tags: &[("highway", "track"), ("surface", "gravel")],
                    nodes: &[4, 5],
                },
                // A sidewalk for pedestrians only is no road the tiles keep.
                TestWay {
                    id: 13,
                    tags: &[("highway", "footway"), ("surface", "paving_stones")],
                    nodes: &[1, 2],
                },
                TestWay {
                    id: 14,
                    tags: &[("highway", "cycleway"), ("surface", "paving_stones")],
                    nodes: &[5, 6],
                },
                TestWay {
                    id: 15,
                    tags: &[("highway", "proposed"), ("surface", "sett")],
                    nodes: &[5, 6],
                },
            ],
        );
        assert_eq!(extract.name, "bern-2026-10-24");
        assert_eq!(extract.data_time, Some(1_792_875_600));
        assert_eq!((extract.surfaces, extract.roundabouts), (2, 0));
        let cells = cells(&dir, &extract);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, kramgasse());
        assert_eq!(
            cells[0].1.surfaces,
            vec![
                SurfaceWay {
                    id: 10,
                    kind: RoadKind::Local,
                    surface: RoadSurface::Cobbles,
                    line: vec![
                        [469_480_000, 74_470_000],
                        [469_480_500, 74_480_000],
                        [469_481_000, 74_490_000],
                    ],
                },
                SurfaceWay {
                    id: 14,
                    kind: RoadKind::Cycleway,
                    surface: RoadSurface::Paving,
                    line: vec![[469_482_000, 74_510_000], [469_482_500, 74_520_000]],
                },
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_what_buildings_are_at_a_point_within_them() {
        let (dir, extract) = extract_of(
            "buildings",
            &[
                // A garage on nodes 1, 2, 3 and back: its point halfway between 1 and 3.
                TestWay {
                    id: 30,
                    tags: &[("building", "garage")],
                    nodes: &[1, 2, 3, 1],
                },
                // Saying only that it is a building, it says no more than the tiles.
                TestWay {
                    id: 31,
                    tags: &[("building", "yes")],
                    nodes: &[4, 5, 6, 4],
                },
                TestWay {
                    id: 32,
                    tags: &[("building", "roof"), ("highway", "residential")],
                    nodes: &[4, 5, 6, 4],
                },
            ],
        );
        assert_eq!(extract.buildings, 2);
        assert_eq!(extract.surfaces, 0, "a building is no road");
        let cells = cells(&dir, &extract);
        let buildings: Vec<&KindAt> = cells.iter().flat_map(|(_, c)| &c.buildings).collect();
        assert_eq!(
            buildings,
            [
                &KindAt {
                    at: [469_480_500, 74_480_000],
                    kind: BuildingKind::Garage
                },
                &KindAt {
                    at: [469_482_000, 74_510_000],
                    kind: BuildingKind::Carport
                },
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_roundabouts_the_way_traffic_goes_round() {
        let (dir, extract) = extract_of(
            "roundabouts",
            &[
                TestWay {
                    id: 20,
                    tags: &[("highway", "tertiary"), ("junction", "roundabout")],
                    nodes: &[4, 3, 2, 4],
                },
                // A cobbled roundabout is both.
                TestWay {
                    id: 21,
                    tags: &[
                        ("highway", "residential"),
                        ("junction", "circular"),
                        ("surface", "cobblestone"),
                    ],
                    nodes: &[6, 5],
                },
                TestWay {
                    id: 22,
                    tags: &[("highway", "residential"), ("junction", "spui")],
                    nodes: &[1, 2],
                },
            ],
        );
        assert_eq!((extract.surfaces, extract.roundabouts), (1, 2));
        let cells = cells(&dir, &extract);
        let roundabouts = &cells[0].1.roundabouts;
        assert_eq!(roundabouts.len(), 2);
        // As drawn: the order of its nodes is the way round.
        assert_eq!(
            roundabouts[0],
            Roundabout {
                id: 20,
                kind: RoadKind::Tertiary,
                line: vec![
                    [469_481_500, 74_500_000],
                    [469_481_000, 74_490_000],
                    [469_480_500, 74_480_000],
                    [469_481_500, 74_500_000],
                ],
            }
        );
        assert_eq!(cells[0].1.surfaces[0].id, 21);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_way_is_in_every_cell_it_passes_whole() {
        let (dir, extract) = extract_of(
            "cells",
            &[TestWay {
                id: 30,
                tags: &[("highway", "track"), ("surface", "dirt")],
                // Two nodes some 100 km apart, and one the file lacks.
                nodes: &[8, 9, 7],
            }],
        );
        let cells = cells(&dir, &extract);
        let (west, east) = (
            tile_of(CELL_ZOOM, 46.948, 7.38),
            tile_of(CELL_ZOOM, 46.948, 7.51),
        );
        assert_eq!(u32::try_from(cells.len()).unwrap(), east.0 - west.0 + 1);
        assert!(
            cells
                .iter()
                .all(|(_, cell)| cell.surfaces[0].line.len() == 2)
        );
        assert_eq!(extract.cells, cells.len());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extracting_again_replaces_the_extract_whole() {
        let sett = [TestWay {
            id: 10,
            tags: &[("highway", "residential"), ("surface", "sett")],
            nodes: &[1, 2],
        }];
        let (dir, first) = extract_of("again", &sett);
        let pbf = dir.join("bern-latest.osm.pbf");
        fs::write(
            &pbf,
            test_file(
                &[TestWay {
                    id: 40,
                    tags: &[("highway", "residential"), ("surface", "grass")],
                    nodes: &[5, 6],
                }],
                1_792_875_600,
            ),
        )
        .unwrap();
        let again = extract(
            &pbf,
            &dir.join(VERSION),
            "bern",
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(again.name, first.name);
        let cells = cells(&dir, &again);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].1.surfaces[0].id, 40);
        let left: Vec<_> = fs::read_dir(dir.join(VERSION)).unwrap().collect();
        assert_eq!(left.len(), 1, "nothing written aside is left");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The download of `tests/data/bern.osm.pbf`, for tests elsewhere: Kramgasse's setts.
    fn bern_download() -> Vec<u8> {
        test_file(
            &[TestWay {
                id: 10,
                tags: &[("highway", "residential"), ("surface", "sett")],
                nodes: &[1, 2, 3],
            }],
            1_792_875_600,
        )
    }

    #[test]
    fn the_bern_download_for_tests_elsewhere_is_current() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/bern.osm.pbf");
        if std::env::var_os("ORQWA_WRITE_DETAIL_FIXTURE").is_some() {
            fs::write(&path, bern_download()).unwrap();
        }
        assert_eq!(
            fs::read(&path).unwrap(),
            bern_download(),
            "write it again with ORQWA_WRITE_DETAIL_FIXTURE=1"
        );
    }

    #[test]
    fn region_names_and_dates_make_directory_names() {
        assert_eq!(file_name("us/california"), "us-california");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_792_875_600), "2026-10-24");
    }

    #[test]
    fn positions_pack_into_one_number_and_back() {
        for (lat, lon) in [
            (469_480_000, 74_470_000),
            (-337_000_000, -1_799_999_999),
            (0, 0),
        ] {
            assert_eq!(unpack(pack(lat, lon)), Some([lat, lon]));
        }
        assert_eq!(unpack(NOWHERE), None);
    }
}
