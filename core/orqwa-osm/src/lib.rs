//! OpenStreetMap data for ORQWA: buildings, land cover, water, roads, bridges and tunnels around
//! a route, and the countries it passes (#230), from [`OpenFreeMap`](https://openfreemap.org)
//! vector tiles (`OpenMapTiles` schema), cached on disk for offline rides (R3); and the
//! [`detail`] those tiles leave out, from regions a rider downloads.
//!
//! © `OpenFreeMap`, © `OpenMapTiles`, data © OpenStreetMap contributors (ODbL 1.0) — attribution
//! must be shown where the data is used.

mod countries;
pub mod detail;
mod iso;
mod mvt;
mod traffic_territories;

pub use countries::Country;
pub use traffic_territories::traffic_territory;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::{StreamExt, stream};
use orqwa_domain::files::UsedFiles;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// A position as (latitude, longitude) in degrees.
pub type LatLon = (f64, f64);

/// Vector tile zoom: the most detailed level OpenFreeMap serves (~1.7 km tiles at 46° N).
const ZOOM: u8 = 14;
/// Tiles downloaded at the same time; they come from a CDN.
const PARALLEL_DOWNLOADS: usize = 6;
/// Bumped when the data source or schema changes, so stale cached tiles are not reused.
const CACHE_VERSION: &str = "openfreemap-omt-v1";
/// Describes the current tile set, including the URL of the latest planet snapshot.
const TILEJSON: &str = "https://tiles.openfreemap.org/planet";

/// Errors while getting map data.
#[derive(Debug, thiserror::Error)]
pub enum OsmError {
    /// A tile is neither cached nor downloadable.
    #[error("map data unavailable for tile {0} (offline and not cached?)")]
    Unavailable(String),
    /// A tile could not be decoded.
    #[error("invalid map data: {0}")]
    Invalid(String),
    /// Reading or writing the cache failed.
    #[error("map cache: {0}")]
    Cache(#[from] std::io::Error),
    /// Stopped as asked, before it was done.
    #[error("stopped")]
    Stopped,
}

/// What covers an area of land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LandCover {
    /// Forest or woodland.
    Forest,
    /// Meadow, grassland, parks.
    Meadow,
    /// Fields.
    Farmland,
    /// Vineyards and orchards.
    Orchard,
    /// Built-up areas.
    Residential,
    /// Industrial land: halls and warehouses.
    Industrial,
    /// Commercial and retail land: offices and stores.
    Commercial,
    /// Grounds of schools, colleges, universities and hospitals.
    Public,
    /// Lakes, rivers, ponds.
    Water,
    /// Rock, scree, glaciers, sand.
    Rock,
}

/// An area with a land cover; rings are closed (first point repeated at the end).
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// What covers the area.
    pub cover: LandCover,
    /// Outer rings.
    pub outer: Vec<Vec<LatLon>>,
    /// Holes.
    pub inner: Vec<Vec<LatLon>>,
}

/// A building footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    /// Stable id (used for deterministic variation).
    pub id: i64,
    /// Closed outline.
    pub outline: Vec<LatLon>,
    /// Height in metres, if known.
    pub height: Option<f64>,
    /// Number of floors, if known.
    pub levels: Option<f64>,
    /// Façade colour as mapped (sRGB, 0–1), if known.
    pub color: Option<[f32; 3]>,
}

/// A river, stream or canal centre line.
#[derive(Debug, Clone, PartialEq)]
pub struct Waterway {
    /// Width in metres (typical for the kind).
    pub width: f64,
    /// Centre line.
    pub line: Vec<LatLon>,
}

/// A mapped road shared by world generation and the minimap.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    /// What kind of way it is.
    pub class: RoadClass,
    /// Appearance metadata retained from the saved transportation tiles.
    pub details: RoadDetails,
    /// Centre line.
    pub line: Vec<LatLon>,
    /// Carried over or under the ground here, if it is.
    pub structure: Option<StructureKind>,
}

/// Road appearance tags, separate from the broad classes used for route matching.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoadDetails {
    /// Original road hierarchy, when supplied by the tile.
    pub kind: RoadKind,
    /// Mapped material or paved/unpaved category; unknown is not evidence of paving.
    pub surface: RoadSurface,
    /// One carriageway of a divided road, or another one-way street.
    pub oneway: bool,
    /// A connecting ramp rather than the through carriageway.
    pub ramp: bool,
    /// Goes round a roundabout, drawn the way traffic goes; known only from map detail.
    pub roundabout: bool,
}

/// Road hierarchy retained for widths and markings, without changing routing priorities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoadKind {
    /// No detailed classification; use the broad road class.
    #[default]
    Unknown,
    /// Motorway carriageway.
    Motorway,
    /// Trunk road.
    Trunk,
    /// Primary through road.
    Primary,
    /// Secondary through road.
    Secondary,
    /// Tertiary through road.
    Tertiary,
    /// Residential or unclassified local road.
    Local,
    /// Driveway or other service road.
    Service,
    /// Farm or forest track.
    Track,
    /// Dedicated cycleway.
    Cycleway,
    /// Shared path or trail.
    Path,
}

/// Surface information, as detailed as the source permits. OpenMapTiles normally supplies
/// only paved/unpaved; specific materials are accepted when present, never inferred as facts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoadSurface {
    /// Untagged or unsupported material.
    #[default]
    Unknown,
    /// Paved, without a specific material.
    Paved,
    /// Unpaved, without a specific material.
    Unpaved,
    /// Asphalt.
    Asphalt,
    /// Concrete, including concrete slabs.
    Concrete,
    /// Pavers or brick paving.
    Paving,
    /// Setts or cobblestones.
    Cobbles,
    /// Gravel or compacted aggregate.
    Gravel,
    /// Exposed soil.
    Dirt,
    /// Grass.
    Grass,
}

impl Road {
    /// Through roads (primary, secondary, ...) rather than local streets, tracks and paths.
    #[must_use]
    pub fn major(&self) -> bool {
        self.class == RoadClass::Major
    }
}

/// Broad routing classes, ordered from through roads to paths. Detailed appearance is
/// retained separately in [`RoadDetails`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoadClass {
    /// Motorways, trunk, primary and secondary roads.
    Major,
    /// Tertiary roads and local streets.
    Street,
    /// Service roads: driveways, car parks, farm access.
    Service,
    /// Farm and forest tracks.
    Track,
    /// Footpaths and cycleways.
    Path,
}

/// Whether a road is carried over or under the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    /// Bridge, viaduct.
    Bridge,
    /// Tunnel, covered road.
    Tunnel,
}

/// A road bridge or tunnel: a road the route rides, where it is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// Bridge or tunnel.
    pub kind: StructureKind,
    /// Centre line of the road on the structure.
    pub line: Vec<LatLon>,
}

/// A railway line: main lines, narrow gauge, funiculars and light rail (not trams, which run
/// in the streets, nor subways, which run underground).
#[derive(Debug, Clone, PartialEq)]
pub struct Railway {
    /// Centre line.
    pub line: Vec<LatLon>,
    /// Carried over or under the ground here, if it is.
    pub structure: Option<StructureKind>,
    /// A funicular, which may climb far more steeply than other railways.
    pub funicular: bool,
}

/// How big a settlement is, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceKind {
    /// A city.
    City,
    /// A town.
    Town,
    /// A village.
    Village,
}

/// A named settlement, for the names on a course's map (#203).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The name as on the ground, in the local language.
    pub name: String,
    /// The name in Latin letters: the same where the local one is written so, else a
    /// transliteration; empty if unknown.
    pub latin: String,
    /// City, town or village.
    pub kind: PlaceKind,
    /// Importance among places of its kind as `OpenMapTiles` ranks them: lower is more
    /// important.
    pub rank: u32,
    /// Where its name belongs.
    pub position: LatLon,
}

/// Map features around a route.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapData {
    /// Building footprints.
    pub buildings: Vec<Building>,
    /// Land cover areas.
    pub areas: Vec<Area>,
    /// Rivers and streams.
    pub waterways: Vec<Waterway>,
    /// Roads.
    pub roads: Vec<Road>,
    /// Railways.
    pub railways: Vec<Railway>,
    /// Churches and chapels, as points on or near their building.
    pub churches: Vec<LatLon>,
    /// Shops, cafés, restaurants and the like, as points in or by their building.
    pub shops: Vec<LatLon>,
    /// Hotels, guest houses and hostels, as points in or by their building.
    pub hotels: Vec<LatLon>,
    /// Offices, as points in or by their building.
    pub offices: Vec<LatLon>,
    /// Schools, hospitals, town halls, libraries, post offices, police and fire stations, as
    /// points in or by their building.
    pub public: Vec<LatLon>,
    /// Castles (not ruins), as points on or near their keep.
    pub castles: Vec<LatLon>,
    /// Lighthouses, as points on their tower or where it stands when it has no outline.
    pub lighthouses: Vec<LatLon>,
    /// Cities, towns and villages.
    pub places: Vec<Place>,
    /// What the tiles leave out, where a region's map detail was asked for and is at hand.
    pub detail: detail::Detail,
}

impl MapData {
    /// Marks the roads going round the roundabouts their [`detail`] knows (ADR 0033), drawn
    /// the way traffic goes, for matching a route to them; a road is split where a ring begins
    /// or ends.
    pub fn apply_roundabouts(&mut self) {
        detail::apply::roundabouts(&mut self.roads, self.detail.roundabouts());
    }

    /// Gives the roads the surfaces their [`detail`] knows (ADR 0033), splitting a road where
    /// its surface changes. Matching a route reads the roads as the tiles have them, so this
    /// comes after it.
    pub fn apply_surfaces(&mut self) {
        detail::apply::surfaces(&mut self.roads, self.detail.surfaces());
    }
}

/// Downloads and caches map tiles.
pub struct Osm {
    cache_dir: PathBuf,
    client: reqwest::Client,
    online: bool,
    used: UsedFiles,
    /// The map detail extracts read along with the tiles around a route.
    detail: Vec<String>,
}

impl Osm {
    /// Creates a downloader caching under `cache_dir`.
    ///
    /// # Panics
    /// If the HTTP client cannot be initialised (no TLS backend), which is a build error.
    #[must_use]
    pub fn new(cache_dir: PathBuf) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "ORQWA/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/ORQWA/ORQWA)"
            ))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client with TLS");
        Self {
            cache_dir,
            client,
            online: true,
            used: UsedFiles::default(),
            detail: Vec::new(),
        }
    }

    /// Uses only cached tiles and never downloads.
    #[must_use]
    pub fn offline(mut self) -> Self {
        self.online = false;
        self
    }

    /// Records every cached tile file read or written in `used`.
    #[must_use]
    pub fn recording(mut self, used: UsedFiles) -> Self {
        self.used = used;
        self
    }

    /// Also reads the map detail of these `extracts` (see [`detail`]) around a route, from the
    /// cache only and recorded like the tiles: a region's detail is downloaded on its own,
    /// never with the tiles.
    #[must_use]
    pub fn with_detail(mut self, extracts: Vec<String>) -> Self {
        self.detail = extracts;
        self
    }

    /// Map features within `corridor` metres of the polyline `points`. `progress` is called
    /// with (tiles done, tiles total) as tiles arrive.
    ///
    /// Tiles that cannot be loaded are skipped with a warning, so partial data is still
    /// returned; the error is only reported if no tile could be loaded at all.
    ///
    /// # Errors
    /// [`OsmError`] if none of the needed tiles is available.
    pub async fn around(
        &self,
        points: &[LatLon],
        corridor: f64,
        progress: &mut (dyn FnMut(usize, usize) + Send),
    ) -> Result<MapData, OsmError> {
        let tiles = tiles_near(points, corridor);
        let mut data = self.load(ZOOM, tiles.clone(), progress).await?;
        if !self.detail.is_empty() {
            data.detail = detail::load(&self.cache_dir, &self.detail, &tiles, &self.used).await;
        }
        Ok(data)
    }

    /// Map features between the corners `south_west` and `north_east`, from tiles at `zoom`
    /// (at most 14): lower zooms cover more land with less detail, as for a map of a whole
    /// course (#203). Progress and errors as for [`Osm::around`].
    ///
    /// # Errors
    /// [`OsmError`] if none of the needed tiles is available.
    pub async fn area(
        &self,
        (south_west, north_east): (LatLon, LatLon),
        zoom: u8,
        progress: &mut (dyn FnMut(usize, usize) + Send),
    ) -> Result<MapData, OsmError> {
        let zoom = zoom.min(ZOOM);
        let (west, north) = tile_of(zoom, north_east.0, south_west.1);
        let (east, south) = tile_of(zoom, south_west.0, north_east.1);
        let tiles = (west..=east)
            .flat_map(|x| (north..=south).map(move |y| (x, y)))
            .collect();
        self.load(zoom, tiles, progress).await
    }

    async fn load(
        &self,
        zoom: u8,
        tiles: BTreeSet<(u32, u32)>,
        progress: &mut (dyn FnMut(usize, usize) + Send),
    ) -> Result<MapData, OsmError> {
        let total = tiles.len();
        info!(zoom, tiles = total, "loading map data");
        progress(0, total);
        let template = self.template().await;
        let template = template.as_deref();
        let mut downloads = stream::iter(tiles)
            .map(|tile| async move { (tile, self.tile(zoom, tile, template).await) })
            .buffer_unordered(PARALLEL_DOWNLOADS);
        // Download in parallel, but merge in tile order so the data does not depend on
        // download timing.
        let mut results = Vec::with_capacity(total);
        while let Some(result) = downloads.next().await {
            results.push(result);
            progress(results.len(), total);
        }
        results.sort_by_key(|(tile, _)| *tile);
        let mut data = MapData::default();
        let mut loaded = 0;
        let mut last_error = None;
        for (tile, result) in results {
            match result.and_then(|bytes| mvt::merge(zoom, tile, bytes, &mut data)) {
                Ok(()) => loaded += 1,
                Err(error) => {
                    warn!(tile = %tile_name(zoom, tile), %error, "map tile unavailable");
                    last_error = Some(error);
                }
            }
        }
        match last_error {
            Some(error) if loaded == 0 => Err(error),
            _ => Ok(data),
        }
    }

    /// The tile URL template of the latest snapshot, if online.
    async fn template(&self) -> Option<String> {
        if !self.online {
            return None;
        }
        let response = self.client.get(TILEJSON).send().await.ok()?;
        let tilejson: serde_json::Value = response.json().await.ok()?;
        let template = tilejson["tiles"][0].as_str().map(ToOwned::to_owned);
        if template.is_none() {
            warn!("map tile index without tile URL");
        }
        template
    }

    async fn tile(
        &self,
        zoom: u8,
        tile: (u32, u32),
        template: Option<&str>,
    ) -> Result<Vec<u8>, OsmError> {
        let path = tile_path(&self.cache_dir, zoom, tile);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                self.used.record(&path);
                return Ok(bytes);
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            Err(_) => {}
        }
        let Some(template) = template else {
            return Err(OsmError::Unavailable(tile_name(zoom, tile)));
        };
        let url = template
            .replace("{z}", &zoom.to_string())
            .replace("{x}", &tile.0.to_string())
            .replace("{y}", &tile.1.to_string());
        debug!(%url, "downloading map tile");
        let bytes = self
            .download(&url)
            .await
            .ok_or_else(|| OsmError::Unavailable(tile_name(zoom, tile)))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Write atomically, so an interrupted download never leaves a broken cache file.
        let partial = path.with_extension("part");
        tokio::fs::write(&partial, &bytes).await?;
        tokio::fs::rename(&partial, &path).await?;
        self.used.record(&path);
        Ok(bytes)
    }

    /// Downloads with one retry, as CDNs occasionally drop a request.
    async fn download(&self, url: &str) -> Option<Vec<u8>> {
        for attempt in 1..=2 {
            match self.client.get(url).send().await {
                Ok(response) if response.status().is_success() => match response.bytes().await {
                    Ok(bytes) => return Some(bytes.to_vec()),
                    Err(error) => warn!(%url, %error, attempt, "map download failed"),
                },
                Ok(response) => {
                    warn!(%url, status = %response.status(), attempt, "map server refused");
                }
                Err(error) => warn!(%url, %error, attempt, "map download failed"),
            }
        }
        None
    }
}

/// Where tile (x, y) at `zoom` is cached under `cache_dir`, the directory an [`Osm`] is
/// created with.
#[must_use]
pub fn tile_path(cache_dir: &Path, zoom: u8, (x, y): (u32, u32)) -> PathBuf {
    cache_dir
        .join(CACHE_VERSION)
        .join(zoom.to_string())
        .join(x.to_string())
        .join(format!("{y}.pbf"))
}

/// Tiles (x, y at [`ZOOM`]) within `corridor` metres of the polyline.
fn tiles_near(points: &[LatLon], corridor: f64) -> BTreeSet<(u32, u32)> {
    const METERS_PER_DEGREE: f64 = 111_195.0;
    // Samples no further apart than the corridor, so its boxes overlap along every segment:
    // a long straight between two points crossed tiles that no vertex touched (#593).
    let spacing = corridor.max(10.0);
    let mut tiles = BTreeSet::new();
    let mut cover = |lat: f64, lon: f64| {
        let d_lat = corridor / METERS_PER_DEGREE;
        let d_lon = corridor / (METERS_PER_DEGREE * lat.to_radians().cos().max(0.01));
        let (west, north) = tile_of(ZOOM, lat + d_lat, lon - d_lon);
        let (east, south) = tile_of(ZOOM, lat - d_lat, lon + d_lon);
        for x in west..=east {
            for y in north..=south {
                tiles.insert((x, y));
            }
        }
    };
    if let Some(&(lat, lon)) = points.first() {
        cover(lat, lon);
    }
    for pair in points.windows(2) {
        let ((lat_a, lon_a), (lat_b, lon_b)) = (pair[0], pair[1]);
        let north = (lat_b - lat_a) * METERS_PER_DEGREE;
        let east = (lon_b - lon_a) * METERS_PER_DEGREE * lat_a.to_radians().cos();
        let steps = (north.hypot(east) / spacing).ceil().clamp(1.0, 1e6);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped above
        let count = steps as u32;
        for step in 1..=count {
            let t = f64::from(step) / steps;
            cover(lat_a + (lat_b - lat_a) * t, lon_a + (lon_b - lon_a) * t);
        }
    }
    tiles
}

/// The Web Mercator tile at `zoom` containing a position.
fn tile_of(zoom: u8, lat: f64, lon: f64) -> (u32, u32) {
    let n = f64::from(1u32 << zoom);
    let lat = lat.clamp(-85.05, 85.05).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to the map
    let clamp = |v: f64| v.floor().clamp(0.0, n - 1.0) as u32;
    (clamp(x), clamp(y))
}

fn tile_name(zoom: u8, (x, y): (u32, u32)) -> String {
    format!("{zoom}/{x}/{y}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test tile's coordinates (see `tests/data`).
    const TEST_TILE: (u32, u32) = (8531, 5767);

    fn cache_with_test_tile(name: &str) -> PathBuf {
        let cache = std::env::temp_dir().join(format!("orqwa-osm-{name}-{}", std::process::id()));
        let dir = cache.join(CACHE_VERSION).join("14").join("8531");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("5767.pbf"),
            include_bytes!("../tests/data/14_8531_5767.pbf"),
        )
        .unwrap();
        cache
    }

    #[test]
    fn covers_the_corridor_with_tiles() {
        // Wabern, near the Gurten: inside one tile with a small corridor.
        assert_eq!(
            tiles_near(&[(46.9296, 7.4512)], 50.0),
            BTreeSet::from([TEST_TILE])
        );
        // A 1.5 km corridor reaches into the neighbouring tiles.
        let wide = tiles_near(&[(46.9296, 7.4512)], 1500.0);
        assert!(wide.contains(&TEST_TILE) && wide.len() >= 4);
    }

    #[test]
    fn covers_every_tile_a_long_straight_segment_crosses() {
        // About 20 km due east, two points only: every tile the line passes holds part of it.
        let (lat, west, east) = (46.9296, 7.30, 7.56);
        let tiles = tiles_near(&[(lat, west), (lat, east)], 50.0);
        let (first_x, y) = tile_of(ZOOM, lat, west);
        let (last_x, _) = tile_of(ZOOM, lat, east);
        assert!(last_x - first_x > 4, "the line spans several tiles");
        for x in first_x..=last_x {
            assert!(tiles.contains(&(x, y)), "tile {x} missing: {tiles:?}");
        }
    }

    #[tokio::test]
    async fn offline_without_cache_is_unavailable() {
        let cache = std::env::temp_dir().join(format!("orqwa-osm-empty-{}", std::process::id()));
        let osm = Osm::new(cache).offline();

        let result = osm.around(&[(46.9296, 7.4512)], 50.0, &mut |_, _| {}).await;

        assert!(matches!(result, Err(OsmError::Unavailable(_))));
    }

    #[tokio::test]
    async fn reads_cached_tiles_offline_with_progress() {
        let cache = cache_with_test_tile("offline");
        let osm = Osm::new(cache.clone()).offline();
        let mut reports = Vec::new();

        let data = osm
            .around(&[(46.9296, 7.4512)], 50.0, &mut |done, total| {
                reports.push((done, total));
            })
            .await
            .unwrap();

        assert!(
            data.buildings.len() > 50,
            "{} buildings",
            data.buildings.len()
        );
        assert!(data.roads.len() > 10);
        assert!(data.areas.iter().any(|a| a.cover == LandCover::Forest));
        assert_eq!(reports, [(0, 1), (1, 1)]);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[tokio::test]
    async fn an_area_comes_from_tiles_of_its_zoom() {
        let cache = std::env::temp_dir().join(format!("orqwa-osm-area-{}", std::process::id()));
        let dir = cache.join(CACHE_VERSION).join("12").join("2141");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("1445.pbf"),
            include_bytes!("../tests/data/12_2141_1445.pbf"),
        )
        .unwrap();
        let osm = Osm::new(cache.clone()).offline();
        let mut reports = Vec::new();

        // Around Meiringen, within that one tile.
        let data = osm
            .area(((46.70, 8.18), (46.73, 8.25)), 12, &mut |done, total| {
                reports.push((done, total));
            })
            .await
            .unwrap();

        assert!(data.places.iter().any(|p| p.name == "Meiringen"));
        assert!(data.areas.iter().any(|a| a.cover == LandCover::Water));
        assert_eq!(reports, [(0, 1), (1, 1)]);
        // The neighbouring tiles a larger area needs are not cached.
        let wider = osm
            .area(((46.6, 8.0), (46.8, 8.4)), 12, &mut |_, _| {})
            .await
            .unwrap();
        assert_eq!(wider.places, data.places);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// Downloads real data; run with `cargo test -p orqwa-osm -- --ignored`.
    #[tokio::test]
    #[ignore = "needs network"]
    async fn live_download_around_the_gurten() {
        let cache = std::env::temp_dir().join(format!("orqwa-osm-live-{}", std::process::id()));
        let osm = Osm::new(cache);

        let started = std::time::Instant::now();
        let data = osm
            .around(&[(46.925, 7.445), (46.918, 7.44)], 1500.0, &mut |_, _| {})
            .await
            .unwrap();

        let structures = data.roads.iter().filter(|r| r.structure.is_some()).count();
        println!(
            "{:?}: {} buildings, {} areas, {} waterways, {} roads ({} bridges or tunnels)",
            started.elapsed(),
            data.buildings.len(),
            data.areas.len(),
            data.waterways.len(),
            data.roads.len(),
            structures
        );
        assert!(!data.buildings.is_empty() && structures > 0);
    }
}
