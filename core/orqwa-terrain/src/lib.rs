//! Elevation data for ORQWA (R12, later the 3D terrain in R16).
//!
//! Heights come from Terrarium-encoded raster tiles in Web Mercator. Tiles are downloaded from
//! the first [`TileSource`] that has them and cached on disk, so a route prepared online can be
//! ridden offline (R3).

mod source;
mod tile;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use orqwa_domain::files::UsedFiles;
use tracing::{debug, warn};

pub use source::{ImageFormat, TileSource};
use tile::{HeightTile, TileId};

/// Errors while looking up elevations.
#[derive(Debug, thiserror::Error)]
pub enum TerrainError {
    /// No source could provide the tile, neither from the cache nor online.
    #[error("no elevation data for tile {tile} (offline and not cached?)")]
    Unavailable {
        /// The missing tile, as `z/x/y`.
        tile: String,
    },
    /// A cached or downloaded tile could not be decoded.
    #[error("invalid elevation tile: {0}")]
    Decode(String),
    /// Reading or writing the tile cache failed.
    #[error("tile cache: {0}")]
    Cache(#[from] std::io::Error),
}

/// Bytes of decoded tiles kept in memory. A long route's corridor crosses hundreds of tiles,
/// but its world is built along the route, so only those around the build front are in use;
/// an evicted tile is read again from the disk cache, and decodes to the same heights.
const TILE_BUDGET: usize = 64 << 20;

/// Elevation lookup backed by downloaded, disk-cached tiles.
pub struct Terrain {
    sources: Vec<TileSource>,
    cache_dir: PathBuf,
    client: reqwest::Client,
    online: bool,
    /// Decoded tiles, each with when it was last used; the least recently used go first once
    /// they hold more than `budget` bytes.
    tiles: HashMap<(usize, TileId), (Arc<HeightTile>, u64)>,
    held: usize,
    budget: usize,
    clock: u64,
    /// Tiles that could not be loaded; not retried, so a missing tile costs one attempt.
    unavailable: HashSet<(usize, TileId)>,
    used: UsedFiles,
}

impl Terrain {
    /// Creates a lookup using `sources` in order of preference.
    ///
    /// # Panics
    /// If the HTTP client cannot be initialised (no TLS backend), which is a build error.
    #[must_use]
    pub fn new(sources: Vec<TileSource>, cache_dir: PathBuf) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "ORQWA/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/ORQWA/ORQWA)"
            ))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client with TLS");
        Self {
            sources,
            cache_dir,
            client,
            online: true,
            tiles: HashMap::new(),
            held: 0,
            budget: TILE_BUDGET,
            clock: 0,
            unavailable: HashSet::new(),
            used: UsedFiles::default(),
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

    /// Ground elevation in metres at a WGS84 position, bilinearly interpolated.
    ///
    /// # Errors
    /// [`TerrainError::Unavailable`] if no source has the tile (or we are offline and it is not
    /// cached), or a decode/cache error.
    pub async fn elevation(&mut self, lat: f64, lon: f64) -> Result<f64, TerrainError> {
        let mut last_error = None;
        for index in 0..self.sources.len() {
            match self.elevation_from(index, lat, lon).await {
                Ok(height) => return Ok(height),
                Err(error) => {
                    debug!(source = self.sources[index].name, %error, "source failed");
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(TerrainError::Unavailable {
            tile: "(no sources configured)".to_owned(),
        }))
    }

    async fn elevation_from(
        &mut self,
        source: usize,
        lat: f64,
        lon: f64,
    ) -> Result<f64, TerrainError> {
        let zoom = self.sources[source].zoom;
        let size = self.sources[source].tile_size;
        let (px, py) = tile::global_pixel(lat, lon, zoom, size);

        // Pixel centres sit at +0.5; interpolate between the four surrounding centres.
        let (u, v) = (px - 0.5, py - 0.5);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let mut corners = [0.0; 4];
        for (i, (dx, dy)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            corners[i] = self.height_at_pixel(source, x0 + dx, y0 + dy).await?;
        }
        let top = corners[0] * (1.0 - fx) + corners[1] * fx;
        let bottom = corners[2] * (1.0 - fx) + corners[3] * fx;
        Ok(top * (1.0 - fy) + bottom * fy)
    }

    async fn height_at_pixel(
        &mut self,
        source: usize,
        x: f64,
        y: f64,
    ) -> Result<f64, TerrainError> {
        let zoom = self.sources[source].zoom;
        let size = self.sources[source].tile_size;
        let (id, local_x, local_y) = tile::locate_pixel(x, y, zoom, size);
        let tile = self.tile(source, id).await?;
        Ok(tile.height(local_x, local_y))
    }

    async fn tile(&mut self, source: usize, id: TileId) -> Result<Arc<HeightTile>, TerrainError> {
        self.clock += 1;
        if let Some((tile, used)) = self.tiles.get_mut(&(source, id)) {
            *used = self.clock;
            return Ok(Arc::clone(tile));
        }
        if self.unavailable.contains(&(source, id)) {
            return Err(TerrainError::Unavailable {
                tile: id.to_string(),
            });
        }
        let result = self.load_tile(source, id).await;
        if result.is_err() {
            self.unavailable.insert((source, id));
        }
        result
    }

    async fn load_tile(
        &mut self,
        source: usize,
        id: TileId,
    ) -> Result<Arc<HeightTile>, TerrainError> {
        let src = &self.sources[source];
        let path = self.cache_dir.join(src.cache_path(id));

        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && self.online => {
                let bytes = self.download(source, id).await?;
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&path, &bytes).await?;
                bytes
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(TerrainError::Unavailable {
                    tile: id.to_string(),
                });
            }
            Err(error) => return Err(error.into()),
        };

        let tile = Arc::new(HeightTile::decode(&bytes, src.format).map_err(TerrainError::Decode)?);
        self.used.record(&path);
        self.keep((source, id), &tile);
        Ok(tile)
    }

    /// Keeps `tile`, first letting go of the least recently used ones it would not fit beside.
    fn keep(&mut self, key: (usize, TileId), tile: &Arc<HeightTile>) {
        while self.held + tile.bytes() > self.budget {
            let Some(oldest) = self
                .tiles
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(&key, _)| key)
            else {
                break;
            };
            if let Some((evicted, _)) = self.tiles.remove(&oldest) {
                self.held -= evicted.bytes();
            }
        }
        self.held += tile.bytes();
        self.tiles.insert(key, (Arc::clone(tile), self.clock));
    }

    async fn download(&self, source: usize, id: TileId) -> Result<Vec<u8>, TerrainError> {
        let src = &self.sources[source];
        let url = src.url(id);
        let unavailable = || TerrainError::Unavailable {
            tile: id.to_string(),
        };
        let response = self.client.get(&url).send().await.map_err(|error| {
            warn!(source = src.name, %url, %error, "tile download failed");
            unavailable()
        })?;
        if !response.status().is_success() {
            debug!(source = src.name, %url, status = %response.status(), "tile not available");
            return Err(unavailable());
        }
        let bytes = response.bytes().await.map_err(|error| {
            warn!(source = src.name, %url, %error, "tile download failed");
            unavailable()
        })?;
        Ok(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a PNG Terrarium tile with a constant height into the cache.
    fn cache_flat_png_tile(
        cache_dir: &std::path::Path,
        source: &TileSource,
        id: TileId,
        height: f64,
    ) {
        let terrarium = height + 32768.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (r, g, b) = (
            (terrarium / 256.0).floor() as u8,
            (terrarium % 256.0).floor() as u8,
            ((terrarium.fract()) * 256.0) as u8,
        );
        let size = source.tile_size;
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, size, size);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = std::iter::repeat_n([r, g, b], (size * size) as usize)
                .flatten()
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        let path = cache_dir.join(source.cache_path(id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, png_bytes).unwrap();
    }

    #[tokio::test]
    async fn offline_lookup_uses_cached_tiles() {
        let cache = std::env::temp_dir().join(format!("orqwa-terrain-test-{}", std::process::id()));
        let source = TileSource::aws_terrain_tiles();
        let (lat, lon) = (46.95, 7.45); // Bern
        let (px, py) = tile::global_pixel(lat, lon, source.zoom, source.tile_size);
        // Cache the tile and its neighbours so interpolation never needs the network.
        let (center, ..) = tile::locate_pixel(px, py, source.zoom, source.tile_size);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let id = TileId {
                    z: center.z,
                    x: center.x.wrapping_add_signed(dx),
                    y: center.y.wrapping_add_signed(dy),
                };
                cache_flat_png_tile(&cache, &source, id, 540.5);
            }
        }

        let mut terrain = Terrain::new(vec![source], cache.clone()).offline();
        let height = terrain.elevation(lat, lon).await.unwrap();

        assert!((height - 540.5).abs() < 0.01, "height {height}");
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// A long route crosses more tiles than fit in the budget; those let go of are read
    /// again and give the same heights.
    #[tokio::test]
    async fn tiles_beyond_the_budget_are_let_go_and_read_again() {
        let cache =
            std::env::temp_dir().join(format!("orqwa-terrain-budget-{}", std::process::id()));
        let source = TileSource::aws_terrain_tiles();
        // Two places far apart, so each lookup needs its own tiles.
        let places = [((46.95, 7.45), 540.5), ((46.0, 8.95), 271.25)];
        for ((lat, lon), height) in places {
            let (px, py) = tile::global_pixel(lat, lon, source.zoom, source.tile_size);
            let (center, ..) = tile::locate_pixel(px, py, source.zoom, source.tile_size);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let id = TileId {
                        z: center.z,
                        x: center.x.wrapping_add_signed(dx),
                        y: center.y.wrapping_add_signed(dy),
                    };
                    cache_flat_png_tile(&cache, &source, id, height);
                }
            }
        }
        let one_tile = (source.tile_size * source.tile_size) as usize * 4;
        let mut terrain = Terrain::new(vec![source], cache.clone()).offline();
        terrain.budget = 2 * one_tile;

        for _ in 0..2 {
            for ((lat, lon), height) in places {
                let found = terrain.elevation(lat, lon).await.unwrap();
                assert!(
                    (found - height).abs() < 0.01,
                    "height {found}, not {height}"
                );
                assert!(
                    terrain.held <= terrain.budget,
                    "{} bytes held",
                    terrain.held
                );
            }
        }
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[tokio::test]
    async fn offline_lookup_without_cache_is_unavailable() {
        let cache =
            std::env::temp_dir().join(format!("orqwa-terrain-empty-{}", std::process::id()));
        let mut terrain = Terrain::new(vec![TileSource::mapterhorn()], cache).offline();

        let result = terrain.elevation(46.95, 7.45).await;

        assert!(matches!(result, Err(TerrainError::Unavailable { .. })));
    }

    /// Hits the real tile servers; run with `cargo test -p orqwa-terrain -- --ignored`.
    #[tokio::test]
    #[ignore = "needs network"]
    async fn live_sources_agree_on_known_height() {
        // Zytglogge, Bern: street level about 542 m.
        let (lat, lon) = (46.947_96, 7.447_77);
        for source in TileSource::defaults() {
            let cache = std::env::temp_dir().join(format!("orqwa-live-{}", source.name));
            let mut terrain = Terrain::new(vec![source.clone()], cache);
            let height = terrain.elevation(lat, lon).await.unwrap();
            println!("{}: {height:.1} m", source.name);
            assert!(
                (530.0..555.0).contains(&height),
                "{}: {height}",
                source.name
            );
        }
    }
}
