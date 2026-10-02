//! Web Mercator tile maths and Terrarium height decoding.

use std::f64::consts::PI;
use std::fmt;
use std::io::Cursor;

use crate::source::ImageFormat;

/// A tile in the Web Mercator (XYZ) scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TileId {
    pub(crate) z: u8,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

impl fmt::Display for TileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

/// Position in pixels across the whole world map at `zoom`.
pub(crate) fn global_pixel(lat: f64, lon: f64, zoom: u8, tile_size: u32) -> (f64, f64) {
    // Web Mercator is undefined at the poles; clamp to its usual latitude limit.
    let lat = lat.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    let world = f64::from(tile_size) * f64::from(1u32 << zoom);
    let x = (lon + 180.0) / 360.0 * world;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * world;
    (x, y)
}

/// The tile containing a global pixel, and the pixel's position within it.
///
/// X wraps around the antimeridian; y is clamped to the map.
// Coordinates are floored and range-limited before casting.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub(crate) fn locate_pixel(x: f64, y: f64, zoom: u8, tile_size: u32) -> (TileId, u32, u32) {
    let world = u64::from(tile_size) << zoom;
    let gx = (x.floor() as i64).rem_euclid(world as i64) as u64;
    let gy = (y.floor().max(0.0) as u64).min(world - 1);
    let size = u64::from(tile_size);
    let id = TileId {
        z: zoom,
        x: (gx / size) as u32,
        y: (gy / size) as u32,
    };
    (id, (gx % size) as u32, (gy % size) as u32)
}

/// Decoded heights of one tile in metres, row-major.
pub(crate) struct HeightTile {
    size: u32,
    heights: Vec<f32>,
}

impl HeightTile {
    pub(crate) fn decode(bytes: &[u8], format: ImageFormat) -> Result<Self, String> {
        let (width, height, channels, pixels) = match format {
            ImageFormat::WebP => decode_webp(bytes)?,
            ImageFormat::Png => decode_png(bytes)?,
        };
        if width != height || channels < 3 {
            return Err(format!(
                "unexpected tile shape {width}x{height} with {channels} channels"
            ));
        }
        let heights = pixels
            .chunks_exact(channels)
            .map(|px| terrarium_height(px[0], px[1], px[2]))
            .collect();
        Ok(Self {
            size: width,
            heights,
        })
    }

    pub(crate) fn height(&self, x: u32, y: u32) -> f64 {
        f64::from(self.heights[(y * self.size + x) as usize])
    }
}

/// Terrarium encoding: `height = R·256 + G + B/256 − 32768` metres.
// Precision of f32 (~1 mm at 8 km) is ample for terrain heights and halves tile memory.
#[allow(clippy::cast_possible_truncation)]
fn terrarium_height(r: u8, g: u8, b: u8) -> f32 {
    (f64::from(r) * 256.0 + f64::from(g) + f64::from(b) / 256.0 - 32768.0) as f32
}

fn decode_webp(bytes: &[u8]) -> Result<(u32, u32, usize, Vec<u8>), String> {
    let mut decoder =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    let size = decoder.output_buffer_size().ok_or("WebP image too large")?;
    let mut pixels = vec![0; size];
    decoder.read_image(&mut pixels).map_err(|e| e.to_string())?;
    Ok((width, height, channels, pixels))
}

fn decode_png(bytes: &[u8]) -> Result<(u32, u32, usize, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("PNG image too large")?;
    let mut pixels = vec![0; size];
    let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    let channels = info.line_size / info.width as usize;
    pixels.truncate(info.line_size * info.height as usize);
    Ok((info.width, info.height, channels, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_terrarium_heights() {
        assert!((terrarium_height(128, 0, 0) - 0.0).abs() < f32::EPSILON);
        assert!((terrarium_height(130, 28, 128) - 540.5).abs() < f32::EPSILON);
        assert!((terrarium_height(127, 255, 0) + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn locates_known_tile() {
        // Bern at zoom 12 lies in tile 12/2132/1441.
        let (x, y) = global_pixel(46.948, 7.447, 12, 256);
        let (id, ..) = locate_pixel(x, y, 12, 256);

        assert_eq!(
            id,
            TileId {
                z: 12,
                x: 2132,
                y: 1441
            }
        );
    }

    #[test]
    fn wraps_across_the_antimeridian() {
        let (x, y) = global_pixel(0.0, 180.0, 1, 256);
        let (id, local_x, _) = locate_pixel(x, y, 1, 256);

        assert_eq!((id.x, local_x), (0, 0));
    }
}
