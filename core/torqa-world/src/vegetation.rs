//! Trees in forests.

use torqa_osm::LandCover;

use crate::HeightGrid;
use crate::landcover::LandIndex;
use crate::road::RoadIndex;

/// Tree spacing close to the road, where they are seen up close.
const NEAR_SPACING: f64 = 7.0;
/// Tree spacing further away, where the forest colour on the ground does most of the work.
const FAR_SPACING: f64 = 20.0;
/// Within this distance of the road trees use the near spacing.
const NEAR_DISTANCE: f64 = 300.0;
/// Trees keep this distance from the road centre.
const ROAD_CLEARANCE: f64 = 8.0;

/// Trees of one chunk as Godot `MultiMesh` transform buffers (12 floats per tree), relative to
/// the chunk origin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trees {
    /// Conifers.
    pub conifers: Vec<f32>,
    /// Broadleaf trees.
    pub broadleaves: Vec<f32>,
}

impl Trees {
    /// Number of trees.
    #[must_use]
    pub fn len(&self) -> usize {
        (self.conifers.len() + self.broadleaves.len()) / 12
    }

    /// Whether there are no trees.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Places trees in forests within the square `[origin, origin + size]` (metres east/north).
pub(crate) fn place(
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    land: &LandIndex,
    road: &RoadIndex,
    chunk_origin: [f64; 3],
) -> Trees {
    let mut trees = Trees::default();
    let mut north = origin.1;
    // Rows use the near spacing and thin out far from the road, keeping placement
    // deterministic for a position regardless of chunk boundaries.
    while north < origin.1 + size {
        let mut east = origin.0;
        while east < origin.0 + size {
            let seed = cell_seed(east, north);
            let (e, n) = (
                east + (crate::hash(seed) - 0.5) * NEAR_SPACING,
                north + (crate::hash(seed ^ 0x5bd1) - 0.5) * NEAR_SPACING,
            );
            east += NEAR_SPACING;
            if land.cover_at(e, n) != Some(LandCover::Forest) {
                continue;
            }
            let road_distance = road.nearest(e, n, NEAR_DISTANCE).map(|(d, _, _)| d);
            if road_distance.is_some_and(|d| d < ROAD_CLEARANCE) {
                continue;
            }
            let keep = (NEAR_SPACING / FAR_SPACING).powi(2);
            if road_distance.is_none() && crate::hash(seed ^ 0x9e37) > keep {
                continue;
            }
            let height = heights.at(e, n);
            let scale = 0.75 + crate::hash(seed ^ 0x1234) * 0.6;
            let yaw = crate::hash(seed ^ 0x4321) * std::f64::consts::TAU;
            // Conifers dominate higher up.
            let conifer_share = ((height - 600.0) / 800.0).clamp(0.3, 0.9);
            let target = if crate::hash(seed ^ 0x7777) < conifer_share {
                &mut trees.conifers
            } else {
                &mut trees.broadleaves
            };
            push_transform(target, [e, height, n], scale, yaw, chunk_origin);
        }
        north += NEAR_SPACING;
    }
    trees
}

/// A stable seed per grid cell.
fn cell_seed(east: f64, north: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)] // cell indices are small
    let (e, n) = (
        (east / NEAR_SPACING).round() as i64,
        (north / NEAR_SPACING).round() as i64,
    );
    e.wrapping_mul(73_856_093) ^ n.wrapping_mul(19_349_663)
}

/// Appends a Godot `Transform3D` in `MultiMesh` buffer layout (row-major 3×4).
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
fn push_transform(
    buffer: &mut Vec<f32>,
    [east, height, north]: [f64; 3],
    scale: f64,
    yaw: f64,
    origin: [f64; 3],
) {
    let (sin, cos) = yaw.sin_cos();
    let (x, y, z) = (east - origin[0], height - origin[1], -north - origin[2]);
    buffer.extend(
        [
            cos * scale,
            0.0,
            sin * scale,
            x,
            0.0,
            scale,
            0.0,
            y,
            -sin * scale,
            0.0,
            cos * scale,
            z,
        ]
        .map(|v| v as f32),
    );
}
