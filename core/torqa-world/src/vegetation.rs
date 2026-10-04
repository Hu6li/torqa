//! Trees in forests, and grass and flowers along the road (R45).

use torqa_osm::LandCover;

use crate::HeightGrid;
use crate::landcover::LandIndex;
use crate::road::RoadIndex;
use crate::streets::Clearance;

/// Tree spacing close to the road, where they are seen up close.
const NEAR_SPACING: f64 = 7.0;
/// Tree spacing further away, where the forest colour on the ground does most of the work.
const FAR_SPACING: f64 = 20.0;
/// Within this distance of the road trees use the near spacing.
const NEAR_DISTANCE: f64 = 300.0;
/// Trees keep this distance from the road centre.
const ROAD_CLEARANCE: f64 = 8.0;
/// Grass grows within this distance of the road centre, where riders see it up close.
const GRASS_DISTANCE: f64 = 30.0;
/// Grass keeps off the road: half its width and a little more.
const GRASS_CLEARANCE: f64 = 3.6;
/// Spacing of grass tufts, jittered.
const GRASS_SPACING: f64 = 1.1;

/// Trees of one chunk as Godot `MultiMesh` transform buffers (12 floats per tree), relative to
/// the chunk origin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trees {
    /// Conifers.
    pub conifers: Vec<f32>,
    /// Broadleaf trees.
    pub broadleaves: Vec<f32>,
    /// Grass tufts along the road.
    pub grass: Vec<f32>,
    /// Flower clumps in meadows along the road.
    pub flowers: Vec<f32>,
}

impl Trees {
    /// Number of trees (grass and flowers not counted).
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

/// What plants are placed on: the chunk's ground, its land cover, the road ridden and the
/// map's other streets.
#[derive(Clone, Copy)]
pub(crate) struct Ground<'a> {
    pub(crate) heights: &'a HeightGrid,
    pub(crate) land: &'a LandIndex,
    pub(crate) road: &'a RoadIndex,
    pub(crate) streets: &'a Clearance,
}

/// Places trees in forests within the square `[origin, origin + size]` (metres east/north).
pub(crate) fn place(
    origin: (f64, f64),
    size: f64,
    ground: &Ground,
    chunk_origin: [f64; 3],
) -> Trees {
    let Ground {
        heights,
        land,
        road,
        streets,
    } = *ground;
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
            if road_distance.is_some_and(|d| d < ROAD_CLEARANCE) || streets.blocked(e, n, 2.0) {
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

/// Places grass tufts and flower clumps along the road within the square `[origin, origin +
/// size]` into `plants`: on open ground (meadows, farmland verges, orchards, lawns), sparse on
/// the forest floor, never on the road or other streets, in water or on rock.
pub(crate) fn place_grass(
    plants: &mut Trees,
    origin: (f64, f64),
    size: f64,
    ground: &Ground,
    chunk_origin: [f64; 3],
) {
    let Ground {
        heights,
        land,
        road,
        streets,
    } = *ground;
    let mut north = (origin.1 / GRASS_SPACING).floor() * GRASS_SPACING;
    while north < origin.1 + size {
        let mut east = (origin.0 / GRASS_SPACING).floor() * GRASS_SPACING;
        while east < origin.0 + size {
            let seed = grass_seed(east, north);
            let (e, n) = (
                east + (crate::hash(seed) - 0.5) * GRASS_SPACING,
                north + (crate::hash(seed ^ 0x2c1b) - 0.5) * GRASS_SPACING,
            );
            east += GRASS_SPACING;
            let inside =
                e >= origin.0 && e < origin.0 + size && n >= origin.1 && n < origin.1 + size;
            let Some((distance, _, _)) = road.nearest(e, n, GRASS_DISTANCE) else {
                continue;
            };
            if !inside || distance < GRASS_CLEARANCE || streets.blocked(e, n, 0.2) {
                continue;
            }
            let cover = land.cover_at(e, n);
            let (density, flowery) = match cover {
                Some(LandCover::Water | LandCover::Rock) => (0.0, false),
                Some(LandCover::Forest) => (0.25, false),
                Some(LandCover::Meadow) | None => (1.0, true),
                Some(LandCover::Farmland | LandCover::Orchard | LandCover::Residential) => {
                    (0.8, false)
                }
            };
            // Thinner towards the edge of the strip, so it does not end in a hard line.
            let fade = 1.0 - ((distance - GRASS_DISTANCE * 0.6) / (GRASS_DISTANCE * 0.4)).max(0.0);
            if crate::hash(seed ^ 0x51ed) > density * fade {
                continue;
            }
            let height = heights.at(e, n);
            let scale = 0.7 + crate::hash(seed ^ 0x0dd5) * 0.7;
            let yaw = crate::hash(seed ^ 0x3a7c) * std::f64::consts::TAU;
            let target = if flowery && crate::hash(seed ^ 0x6b43) < FLOWER_SHARE {
                &mut plants.flowers
            } else {
                &mut plants.grass
            };
            push_transform(target, [e, height, n], scale, yaw, chunk_origin);
        }
        north += GRASS_SPACING;
    }
}

/// Share of meadow grass spots that are flower clumps instead.
const FLOWER_SHARE: f64 = 0.12;

/// A stable seed per grass cell.
fn grass_seed(east: f64, north: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)] // cell indices are small
    let (e, n) = (
        (east / GRASS_SPACING).round() as i64,
        (north / GRASS_SPACING).round() as i64,
    );
    e.wrapping_mul(83_492_791) ^ n.wrapping_mul(2_654_435_761)
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
