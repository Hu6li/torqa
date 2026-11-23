//! Levels of detail for the copies an `instances` file places (ADR 0042). Each copy takes the
//! level its own distance from the camera asks for: a section is hundreds of metres across, and
//! one level for all its trees left the far side of a near section in near detail and the near
//! side of a far one in far detail.

/// Floats a copy's matrix takes; its translation is at 3 (x) and 11 (z).
const MATRIX: usize = 12;

/// The copies in `buffer`, `per_copy` floats each (the 3 × 4 matrix row by row, then any tint),
/// sorted into an asset's levels of detail: a copy goes to the first level whose `until`
/// (metres, ascending, the last may be infinite) its horizontal distance from `here` (x, z, in
/// the copies' own frame) is less than, and to none beyond the last. Copies keep their order.
///
/// # Panics
/// If `per_copy` is less than a matrix or does not divide `buffer`.
#[must_use]
pub fn by_distance(
    buffer: &[f32],
    per_copy: usize,
    here: [f32; 2],
    until: &[f32],
) -> Vec<Vec<f32>> {
    assert!(
        per_copy >= MATRIX && buffer.len().is_multiple_of(per_copy),
        "copies of {per_copy} floats"
    );
    let mut levels = vec![Vec::new(); until.len()];
    for copy in buffer.chunks_exact(per_copy) {
        let distance = (copy[3] - here[0]).hypot(copy[11] - here[1]);
        if let Some(level) = until.iter().position(|&limit| distance < limit) {
            levels[level].extend_from_slice(copy);
        }
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copies standing at `x` along the x axis, each with a tint whose red is its index.
    fn row(xs: &[f32]) -> Vec<f32> {
        xs.iter()
            .enumerate()
            .flat_map(|(index, &x)| {
                #[allow(clippy::cast_precision_loss)] // a handful of copies
                let red = index as f32;
                [
                    1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, red, 1.0, 1.0, 1.0,
                ]
            })
            .collect()
    }

    fn xs(level: &[f32]) -> Vec<f32> {
        level
            .as_chunks::<16>()
            .0
            .iter()
            .map(|copy| copy[3])
            .collect()
    }

    #[test]
    fn each_copy_takes_the_level_of_its_own_distance() {
        // A section 500 m across with the camera at one edge: its near trees in near detail,
        // its far ones in far detail.
        let copies = row(&[-240.0, -120.0, 40.0, 240.0]);
        let levels = by_distance(&copies, 16, [-250.0, 0.0], &[150.0, f32::INFINITY]);
        assert_eq!(xs(&levels[0]), [-240.0, -120.0]);
        assert_eq!(xs(&levels[1]), [40.0, 240.0]);
    }

    #[test]
    fn distance_is_horizontal() {
        let mut copies = row(&[0.0]);
        copies[7] = 500.0;
        copies[11] = 100.0;
        let levels = by_distance(&copies, 16, [0.0, 0.0], &[150.0, f32::INFINITY]);
        assert_eq!(
            levels[0].len(),
            16,
            "a copy high above the camera is still near"
        );
    }

    #[test]
    fn copies_beyond_the_last_level_are_left_out_and_tints_stay_with_their_copy() {
        let copies = row(&[10.0, 900.0, 20.0]);
        let levels = by_distance(&copies, 16, [0.0, 0.0], &[150.0, 600.0]);
        assert_eq!(xs(&levels[0]), [10.0, 20.0]);
        assert_eq!(levels[1].len(), 0, "no copy is drawn beyond the last level");
        let reds: Vec<f32> = levels[0]
            .as_chunks::<16>()
            .0
            .iter()
            .map(|copy| copy[12])
            .collect();
        assert_eq!(reds, [0.0, 2.0]);
    }

    #[test]
    fn untinted_copies_split_alike() {
        let copies: Vec<f32> = row(&[10.0, 300.0])
            .as_chunks::<16>()
            .0
            .iter()
            .flat_map(|copy| copy[..MATRIX].to_vec())
            .collect();
        let levels = by_distance(&copies, MATRIX, [0.0, 0.0], &[150.0, f32::INFINITY]);
        assert_eq!(levels[0], copies[..MATRIX]);
        assert_eq!(levels[1], copies[MATRIX..]);
    }
}
