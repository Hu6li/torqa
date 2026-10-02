//! OpenStreetMap data for Torqa: buildings, land cover, waterways, bridges and tunnels around a
//! route, downloaded from the Overpass API in tiles and cached on disk for offline rides (R3).
//!
//! Map data © OpenStreetMap contributors, ODbL 1.0 — attribution must be shown where it is used.

mod parse;

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use tracing::{debug, info, warn};

/// A position as (latitude, longitude) in degrees.
pub type LatLon = (f64, f64);

/// Edge length of a download tile in degrees.
const TILE_DEGREES: f64 = 0.05;
/// Bumped when the query changes, so stale cached tiles are not reused.
const CACHE_VERSION: &str = "v2";

/// Public Overpass instances, tried in order.
const ENDPOINTS: [&str; 3] = [
    "https://overpass-api.de/api/interpreter",
    "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
];

/// Errors while getting map data.
#[derive(Debug, thiserror::Error)]
pub enum OsmError {
    /// A tile is neither cached nor downloadable.
    #[error("map data unavailable for tile {0} (offline and not cached?)")]
    Unavailable(String),
    /// The response could not be understood.
    #[error("invalid map data: {0}")]
    Invalid(String),
    /// Reading or writing the cache failed.
    #[error("map cache: {0}")]
    Cache(#[from] std::io::Error),
}

/// What covers an area of land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LandCover {
    /// Forest or woodland.
    Forest,
    /// Meadow, grassland, heath.
    Meadow,
    /// Fields.
    Farmland,
    /// Vineyards and orchards.
    Orchard,
    /// Built-up areas.
    Residential,
    /// Lakes, ponds, reservoirs.
    Water,
    /// Rock, scree, glaciers.
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
    /// OSM id, stable across downloads (used for deterministic variation).
    pub id: i64,
    /// Closed outline.
    pub outline: Vec<LatLon>,
    /// Height in metres, if tagged.
    pub height: Option<f64>,
    /// Number of floors, if tagged.
    pub levels: Option<f64>,
}

/// A river, stream or canal centre line.
#[derive(Debug, Clone, PartialEq)]
pub struct Waterway {
    /// Width in metres (tagged, or typical for the kind).
    pub width: f64,
    /// Centre line.
    pub line: Vec<LatLon>,
}

/// A road for the minimap.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    /// Through roads (primary, secondary, ...) rather than local streets and tracks.
    pub major: bool,
    /// Centre line.
    pub line: Vec<LatLon>,
}

/// Whether a road is carried over or under the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    /// Bridge, viaduct.
    Bridge,
    /// Tunnel, covered road.
    Tunnel,
}

/// A road bridge or tunnel.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// Bridge or tunnel.
    pub kind: StructureKind,
    /// Centre line of the road on the structure.
    pub line: Vec<LatLon>,
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
    /// Road bridges and tunnels.
    pub structures: Vec<Structure>,
    /// Roads.
    pub roads: Vec<Road>,
}

/// Downloads and caches OpenStreetMap data.
pub struct Osm {
    cache_dir: PathBuf,
    client: reqwest::Client,
    online: bool,
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
                "Torqa/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/bossm8/torqa)"
            ))
            // Big tiles download in ~15 s; a stuck server should not hold up the mirrors long.
            .timeout(Duration::from_secs(45))
            .build()
            .expect("HTTP client with TLS");
        Self {
            cache_dir,
            client,
            online: true,
        }
    }

    /// Uses only cached tiles and never downloads.
    #[must_use]
    pub fn offline(mut self) -> Self {
        self.online = false;
        self
    }

    /// Map features within `corridor` metres of the polyline `points`.
    ///
    /// Tiles that cannot be loaded are skipped with a warning, so partial data is still
    /// returned; the error is only reported if no tile could be loaded at all.
    ///
    /// # Errors
    /// [`OsmError`] if none of the needed tiles is available.
    pub async fn around(&self, points: &[LatLon], corridor: f64) -> Result<MapData, OsmError> {
        let tiles = tiles_near(points, corridor);
        info!(tiles = tiles.len(), "loading map data");
        let mut data = MapData::default();
        let mut seen = HashSet::new();
        let mut loaded = 0;
        let mut last_error = None;
        for tile in &tiles {
            match self.tile(*tile).await {
                Ok(json) => {
                    parse::merge(&json, &mut data, &mut seen)?;
                    loaded += 1;
                }
                Err(error) => {
                    warn!(tile = %tile_name(*tile), %error, "map tile unavailable");
                    last_error = Some(error);
                }
            }
        }
        match last_error {
            Some(error) if loaded == 0 => Err(error),
            _ => Ok(data),
        }
    }

    async fn tile(&self, tile: (i32, i32)) -> Result<String, OsmError> {
        let path = self
            .cache_dir
            .join(CACHE_VERSION)
            .join(format!("{}.json", tile_name(tile)));
        match tokio::fs::read_to_string(&path).await {
            Ok(json) => return Ok(json),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            Err(_) => {}
        }
        if !self.online {
            return Err(OsmError::Unavailable(tile_name(tile)));
        }
        let json = self.download(tile).await?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Write atomically, so an interrupted download never leaves a broken cache file.
        let partial = path.with_extension("part");
        tokio::fs::write(&partial, &json).await?;
        tokio::fs::rename(&partial, &path).await?;
        Ok(json)
    }

    async fn download(&self, tile: (i32, i32)) -> Result<String, OsmError> {
        let query = query(tile);
        for endpoint in ENDPOINTS {
            debug!(endpoint, tile = %tile_name(tile), "downloading map tile");
            let response = self
                .client
                .post(endpoint)
                .form(&[("data", query.as_str())])
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => match response.text().await {
                    Ok(text) => return Ok(text),
                    Err(error) => warn!(endpoint, %error, "map download failed"),
                },
                Ok(response) => warn!(endpoint, status = %response.status(), "map server refused"),
                Err(error) => warn!(endpoint, %error, "map download failed"),
            }
        }
        Err(OsmError::Unavailable(tile_name(tile)))
    }
}

/// The Overpass query for one tile: only features Torqa draws, to keep downloads small.
fn query((lat, lon): (i32, i32)) -> String {
    let south = f64::from(lat) * TILE_DEGREES;
    let west = f64::from(lon) * TILE_DEGREES;
    let bbox = format!(
        "{south:.4},{west:.4},{:.4},{:.4}",
        south + TILE_DEGREES,
        west + TILE_DEGREES
    );
    let landuse = "^(forest|meadow|grass|farmland|farmyard|vineyard|orchard|residential|\
                   commercial|industrial|retail|allotments|village_green|reservoir)$";
    let natural = "^(wood|scrub|grassland|heath|water|bare_rock|scree|glacier|wetland)$";
    let roads = "^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|\
                 living_street|track)$";
    format!(
        "[out:json][timeout:90];(\
         way[\"building\"]({bbox});\
         way[\"landuse\"~\"{landuse}\"]({bbox});\
         relation[\"landuse\"~\"{landuse}\"][\"type\"=\"multipolygon\"]({bbox});\
         way[\"natural\"~\"{natural}\"]({bbox});\
         relation[\"natural\"~\"{natural}\"][\"type\"=\"multipolygon\"]({bbox});\
         way[\"waterway\"~\"^(river|stream|canal)$\"]({bbox});\
         way[\"highway\"~\"{roads}\"]({bbox});\
         way[\"highway\"][\"bridge\"][\"bridge\"!=\"no\"]({bbox});\
         way[\"highway\"][\"tunnel\"][\"tunnel\"!=\"no\"]({bbox});\
         );out tags geom;"
    )
}

/// Tiles within `corridor` metres of the polyline.
fn tiles_near(points: &[LatLon], corridor: f64) -> BTreeSet<(i32, i32)> {
    const METERS_PER_DEGREE: f64 = 111_195.0;
    let mut tiles = BTreeSet::new();
    for &(lat, lon) in points {
        let d_lat = corridor / METERS_PER_DEGREE;
        let d_lon = corridor / (METERS_PER_DEGREE * lat.to_radians().cos().max(0.01));
        let index = |degrees: f64| {
            #[allow(clippy::cast_possible_truncation)] // tile indices are small
            let index = (degrees / TILE_DEGREES).floor() as i32;
            index
        };
        for tile_lat in index(lat - d_lat)..=index(lat + d_lat) {
            for tile_lon in index(lon - d_lon)..=index(lon + d_lon) {
                tiles.insert((tile_lat, tile_lon));
            }
        }
    }
    tiles
}

fn tile_name((lat, lon): (i32, i32)) -> String {
    format!("{lat}_{lon}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_the_corridor_with_tiles() {
        // A 1 km route in Bern well inside one tile still needs its neighbours if the
        // corridor crosses a tile border.
        let inside = tiles_near(&[(46.925, 7.425), (46.934, 7.425)], 100.0);
        assert_eq!(inside, BTreeSet::from([(938, 148)]));

        let near_border = tiles_near(&[(46.9499, 7.425)], 1500.0);
        assert!(near_border.contains(&(938, 148)) && near_border.contains(&(939, 148)));
    }

    #[test]
    fn query_asks_only_for_drawn_features_in_the_tile() {
        let query = query((938, 148));

        assert!(query.contains("(46.9000,7.4000,46.9500,7.4500)"));
        assert!(query.contains("way[\"building\"]"));
        assert!(query.contains("[\"bridge\"!=\"no\"]"));
    }

    #[tokio::test]
    async fn offline_without_cache_is_unavailable() {
        let cache = std::env::temp_dir().join(format!("torqa-osm-empty-{}", std::process::id()));
        let osm = Osm::new(cache).offline();

        let result = osm.around(&[(46.93, 7.44)], 100.0).await;

        assert!(matches!(result, Err(OsmError::Unavailable(_))));
    }

    #[tokio::test]
    async fn offline_reads_cached_tiles() {
        let cache = std::env::temp_dir().join(format!("torqa-osm-cache-{}", std::process::id()));
        std::fs::create_dir_all(cache.join(CACHE_VERSION)).unwrap();
        std::fs::write(
            cache.join(CACHE_VERSION).join("938_148.json"),
            parse::tests::SAMPLE,
        )
        .unwrap();
        let osm = Osm::new(cache.clone()).offline();

        let data = osm.around(&[(46.93, 7.44)], 100.0).await.unwrap();

        assert_eq!(data.buildings.len(), 1);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// Downloads real data; run with `cargo test -p torqa-osm -- --ignored`.
    #[tokio::test]
    #[ignore = "needs network"]
    async fn live_download_around_the_gurten() {
        let cache = std::env::temp_dir().join("torqa-osm-live");
        let osm = Osm::new(cache);

        let started = std::time::Instant::now();
        let data = osm
            .around(&[(46.925, 7.445), (46.918, 7.44)], 1500.0)
            .await
            .unwrap();

        println!(
            "{:?}: {} buildings, {} areas, {} waterways, {} structures",
            started.elapsed(),
            data.buildings.len(),
            data.areas.len(),
            data.waterways.len(),
            data.structures.len()
        );
        assert!(!data.buildings.is_empty() && !data.areas.is_empty());
    }
}
