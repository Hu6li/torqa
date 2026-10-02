//! Where elevation tiles come from.

use std::path::PathBuf;

use crate::tile::TileId;

/// Image encoding of a tile source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Lossless WebP.
    WebP,
    /// PNG.
    Png,
}

/// A provider of Terrarium-encoded elevation tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSource {
    /// Short name, also used as cache directory.
    pub name: &'static str,
    /// URL with `{z}`, `{x}` and `{y}` placeholders.
    pub url_template: &'static str,
    /// Tile edge length in pixels.
    pub tile_size: u32,
    /// Zoom level used for lookups.
    pub zoom: u8,
    /// Image encoding.
    pub format: ImageFormat,
}

impl TileSource {
    /// [Mapterhorn](https://mapterhorn.com): Copernicus GLO-30 worldwide plus high-resolution
    /// national LIDAR data across Europe. Data CC BY 4.0, see <https://mapterhorn.com/attribution>.
    ///
    /// Zoom 12 is the deepest level with worldwide coverage (~19 m per pixel at the equator).
    #[must_use]
    pub fn mapterhorn() -> Self {
        Self {
            name: "mapterhorn",
            url_template: "https://tiles.mapterhorn.com/{z}/{x}/{y}.webp",
            tile_size: 512,
            zoom: 12,
            format: ImageFormat::WebP,
        }
    }

    /// AWS Open Data Terrain Tiles (SRTM, GMTED and national datasets), used as a fallback.
    ///
    /// Zoom 13 with 256-pixel tiles matches Mapterhorn's ground resolution.
    #[must_use]
    pub fn aws_terrain_tiles() -> Self {
        Self {
            name: "aws-terrain-tiles",
            url_template: "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png",
            tile_size: 256,
            zoom: 13,
            format: ImageFormat::Png,
        }
    }

    /// The default sources in order of preference: Mapterhorn, then AWS.
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        vec![Self::mapterhorn(), Self::aws_terrain_tiles()]
    }

    pub(crate) fn url(&self, id: TileId) -> String {
        self.url_template
            .replace("{z}", &id.z.to_string())
            .replace("{x}", &id.x.to_string())
            .replace("{y}", &id.y.to_string())
    }

    pub(crate) fn cache_path(&self, id: TileId) -> PathBuf {
        let extension = match self.format {
            ImageFormat::WebP => "webp",
            ImageFormat::Png => "png",
        };
        PathBuf::from(self.name)
            .join(id.z.to_string())
            .join(id.x.to_string())
            .join(format!("{}.{extension}", id.y))
    }
}
