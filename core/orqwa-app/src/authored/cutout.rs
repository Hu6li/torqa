//! Mipmaps for a world file's cut-out textures (ADR 0042): leaves, needles, vine rows, drawn with
//! alpha and cut where it falls below their material's cutoff (glTF `alphaMode: MASK`).
//! Averaged smaller levels thin out alpha: a crown's cards empty as they shrink on screen, until
//! only the trunk is left. Each level here keeps as much of its area at or above the cutoff as
//! the full image has, so a tree holds its leaves at every distance.

/// Highest factor a level's alpha is raised by.
const MOST: f32 = 64.0;

/// The mipmap chain of an RGBA8 image, `width` × `height` with rows from the top: the image,
/// then each level halved (rounding down, at least 1) down to 1 × 1, laid one after the other as
/// Godot reads mipmaps. Each level averages 2 × 2 texels of the one before; its alpha is then
/// raised as little as it takes for the share of texels at or above `cutoff` (0–1) to reach
/// that of the full image: never less cover, for a crown thinned out is what is seen wrong.
///
/// # Panics
/// If `rgba` is not `width × height × 4` bytes long.
#[must_use]
pub fn mipmaps(rgba: &[u8], width: usize, height: usize, cutoff: f32) -> Vec<u8> {
    assert_eq!(
        rgba.len(),
        width * height * 4,
        "an RGBA8 image of {width} × {height}"
    );
    let threshold = cutoff.clamp(0.0, 1.0) * 255.0;
    let mut out = rgba.to_vec();
    let wanted = coverage(&alphas(rgba), threshold, 1.0);
    // Levels are averaged from the unscaled level before, so scaling never compounds.
    let mut level: Vec<f32> = rgba.iter().map(|&v| f32::from(v)).collect();
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        let (next_w, next_h) = ((w / 2).max(1), (h / 2).max(1));
        level = halved(&level, w, h, next_w, next_h);
        (w, h) = (next_w, next_h);
        let alpha: Vec<f32> = level
            .as_chunks::<4>()
            .0
            .iter()
            .map(|texel| texel[3])
            .collect();
        let scale = scale_for(&alpha, threshold, wanted);
        out.extend(
            level
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|texel| [texel[0], texel[1], texel[2], texel[3] * scale].map(byte)),
        );
    }
    out
}

fn alphas(rgba: &[u8]) -> Vec<f32> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|texel| f32::from(texel[3]))
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to a byte first
fn byte(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// The share of `alpha` at or above `threshold` once multiplied by `scale`, as stored (bytes).
#[allow(clippy::cast_precision_loss)] // texel counts far below 2^24
fn coverage(alpha: &[f32], threshold: f32, scale: f32) -> f32 {
    if alpha.is_empty() {
        return 0.0;
    }
    let above = alpha
        .iter()
        .filter(|&&a| f32::from(byte(a * scale)) >= threshold)
        .count();
    above as f32 / alpha.len() as f32
}

/// The least factor that brings a level's coverage to `wanted` (more alpha never covers less).
fn scale_for(alpha: &[f32], threshold: f32, wanted: f32) -> f32 {
    let (mut low, mut high) = (0.0_f32, MOST);
    if coverage(alpha, threshold, high) <= wanted {
        return high;
    }
    for _ in 0..24 {
        let middle = f32::midpoint(low, high);
        if coverage(alpha, threshold, middle) < wanted {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

/// A level of `next_w` × `next_h` RGBA texels, each the average of the 2 × 2 (fewer at an odd
/// edge) of `level` it covers.
fn halved(level: &[f32], w: usize, h: usize, next_w: usize, next_h: usize) -> Vec<f32> {
    let mut out = vec![0.0; next_w * next_h * 4];
    for y in 0..next_h {
        for x in 0..next_w {
            let mut sum = [0.0_f32; 4];
            let mut count = 0.0_f32;
            for sy in (y * 2)..((y * 2 + 2).min(h)) {
                for sx in (x * 2)..((x * 2 + 2).min(w)) {
                    let at = (sy * w + sx) * 4;
                    for (total, value) in sum.iter_mut().zip(&level[at..at + 4]) {
                        *total += value;
                    }
                    count += 1.0;
                }
            }
            let at = (y * next_w + x) * 4;
            for (target, total) in out[at..at + 4].iter_mut().zip(sum) {
                *target = total / count;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaves as cut-out textures draw them: opaque texels scattered on a clear card, about
    /// one in `every`.
    fn spots(size: usize, every: u32) -> Vec<u8> {
        let mut seed = 0x9e37_79b9_u32;
        let mut rgba = vec![0u8; size * size * 4];
        for texel in rgba.as_chunks_mut::<4>().0 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *texel = [
                60,
                100,
                40,
                if seed.is_multiple_of(every) { 255 } else { 0 },
            ];
        }
        rgba
    }

    /// Each level's slice of a chain, from the image down to 1 × 1.
    fn levels(chain: &[u8], width: usize, height: usize) -> Vec<(usize, usize, &[u8])> {
        let (mut w, mut h, mut at) = (width, height, 0);
        let mut out = Vec::new();
        loop {
            let size = w * h * 4;
            out.push((w, h, &chain[at..at + size]));
            at += size;
            if w == 1 && h == 1 {
                break;
            }
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
        }
        assert_eq!(at, chain.len(), "the chain is exactly its levels");
        out
    }

    #[test]
    fn leaves_keep_their_cover_at_every_level() {
        let image = spots(64, 3);
        let wanted = coverage(&alphas(&image), 127.5, 1.0);
        let chain = mipmaps(&image, 64, 64, 0.5);
        for (w, h, level) in levels(&chain, 64, 64) {
            // Smaller levels hold too few texels to match a share closely.
            if w * h < 256 {
                continue;
            }
            let found = coverage(&alphas(level), 127.5, 1.0);
            assert!(
                found >= wanted && found - wanted < 0.15,
                "{w} × {h} covers {found:.2}, the image {wanted:.2}"
            );
        }
    }

    #[test]
    fn plain_averaging_would_lose_them() {
        // The fault this answers: averaged over a few levels, scattered leaves all fall below a
        // cutoff of a half, while the chain here keeps them.
        let image = spots(64, 3);
        let mut level: Vec<f32> = image.iter().map(|&v| f32::from(v)).collect();
        let mut size = 64;
        for _ in 0..3 {
            level = halved(&level, size, size, size / 2, size / 2);
            size /= 2;
        }
        let alpha: Vec<f32> = level
            .as_chunks::<4>()
            .0
            .iter()
            .map(|texel| texel[3])
            .collect();
        assert!(coverage(&alpha, 127.5, 1.0) < 0.05);
        let chain = mipmaps(&image, 64, 64, 0.5);
        let (_, _, kept) = levels(&chain, 64, 64)[3];
        assert!(coverage(&alphas(kept), 127.5, 1.0) > 0.25);
    }

    #[test]
    fn the_image_itself_is_unchanged() {
        let image = spots(16, 3);
        assert_eq!(&mipmaps(&image, 16, 16, 0.5)[..image.len()], &image[..]);
    }

    #[test]
    fn colours_are_averaged_and_odd_sizes_end_at_one_texel() {
        let image = spots(6, 3).into_iter().take(6 * 3 * 4).collect::<Vec<u8>>();
        let chain = mipmaps(&image, 6, 3, 0.5);
        let found = levels(&chain, 6, 3);
        assert_eq!(
            found.iter().map(|(w, h, _)| (*w, *h)).collect::<Vec<_>>(),
            [(6, 3), (3, 1), (1, 1)]
        );
        assert_eq!(&found[2].2[..3], &[60, 100, 40]);
    }

    #[test]
    fn solid_and_clear_images_stay_so() {
        let solid = vec![255u8; 8 * 8 * 4];
        assert!(mipmaps(&solid, 8, 8, 0.5).iter().all(|&v| v == 255));
        let clear: Vec<u8> = (0..8 * 8).flat_map(|_| [10, 20, 30, 0]).collect();
        let chain = mipmaps(&clear, 8, 8, 0.5);
        assert!(chain.as_chunks::<4>().0.iter().all(|texel| texel[3] == 0));
    }
}
