//! Trees, bushes, ferns and rocks (Blender models, `art/vegetation`), and grass and flowers
//! along the road (R45, ADR 0011); in the subtropics palms, banana plants and tropical shrubs (#136).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use orqwa_osm::LandCover;
use serde::Deserialize;

use crate::HeightGrid;
use crate::buildings::Point;
use crate::climate::Climate;
use crate::landcover::LandIndex;
use crate::palette;
use crate::road::RoadIndex;
use crate::streets::Clearance;

/// The manifest the vegetation build writes next to the models.
const MANIFEST: &str = include_str!("../../../app/assets/models/vegetation/models.json");

/// Plant spacing close to the road, where they are seen up close.
const NEAR_SPACING: f64 = 7.0;
/// Forest crowns should overlap into woodland rather than look like solitary trees (R103).
const FOREST_SPACING: f64 = 4.0;
/// Distant woodland still needs enough crowns to read as forest against the horizon.
const FAR_SPACING: f64 = 10.0;
/// Within this distance of the road trees use the near spacing, and other plants grow at all.
pub(crate) const NEAR_DISTANCE: f64 = 300.0;
/// Bushes and ferns grow under the trees within this distance of the road.
const UNDERSTOREY_DISTANCE: f64 = 60.0;
/// Ferns share a patch decision across this much woodland floor, rather than isolated dots.
const FERN_PATCH_SIZE: f64 = 24.0;
/// A slope steeper than this (rise over run) shows rocks.
const ROCKY_SLOPE: f64 = 0.7;
/// Grass grows within this distance of the road centre, where riders see it up close.
const GRASS_DISTANCE: f64 = 30.0;
/// Grass keeps off the road: half its width and a little more.
const GRASS_CLEARANCE: f64 = 3.6;
/// Spacing of grass tufts, jittered.
const GRASS_SPACING: f64 = 1.1;
/// Floats per plant in a `MultiMesh` buffer: transform (12) and colour (4).
pub(crate) const PLANT_FLOATS: usize = 16;

/// What grows: each kind has its models, its palette colours and its distance from the road.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plant {
    Conifer,
    Broadleaf,
    Bush,
    Rock,
    /// Coconut and fan palms.
    Palm,
    Banana,
    /// Broadleaf shrubs of the tropics.
    TropicalBush,
    /// Low, arching fronds on the woodland floor.
    Fern,
}

impl Plant {
    fn kind(self) -> &'static str {
        match self {
            Self::Conifer => "conifer",
            Self::Broadleaf => "broadleaf",
            Self::Bush => "bush",
            Self::Rock => "rock",
            Self::Palm => "palm",
            Self::Banana => "banana",
            Self::TropicalBush => "tropical_bush",
            Self::Fern => "fern",
        }
    }

    fn colours(self) -> &'static str {
        match self {
            Self::Conifer => "plants.conifers",
            Self::Broadleaf => "plants.broadleaves",
            Self::Bush => "plants.bushes",
            Self::Rock => "plants.rocks",
            Self::Palm => "plants.palms",
            Self::Banana | Self::TropicalBush => "plants.tropical",
            Self::Fern => "plants.ferns",
        }
    }

    /// Distance kept from the road centre: the road's half width and the plant's reach.
    fn clearance(self) -> f64 {
        match self {
            Self::Conifer | Self::Broadleaf | Self::Palm => 8.0,
            Self::Bush | Self::Banana | Self::TropicalBush => 5.0,
            Self::Rock | Self::Fern => 4.5,
        }
    }

    /// Random size: a factor on the model.
    fn scale(self, dice: f64) -> f64 {
        match self {
            Self::Conifer | Self::Broadleaf => 0.75 + dice * 0.6,
            Self::Palm | Self::Banana => 0.8 + dice * 0.45,
            Self::Bush | Self::TropicalBush => 0.7 + dice * 0.7,
            Self::Rock => 0.6 + dice * 1.4,
            Self::Fern => 0.75 + dice * 0.5,
        }
    }
}

#[derive(Deserialize)]
struct Entry {
    kind: String,
}

#[derive(Deserialize)]
struct Manifest {
    models: BTreeMap<String, Entry>,
}

/// Model names by kind, in name order.
static MODELS: LazyLock<BTreeMap<String, Vec<String>>> = LazyLock::new(|| {
    let manifest: Manifest =
        serde_json::from_str(MANIFEST).expect("the committed vegetation models.json is valid");
    let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, entry) in manifest.models {
        by_kind.entry(entry.kind).or_default().push(name);
    }
    by_kind
});

/// The kind (`conifer`, `broadleaf`, `bush`, `rock`, `palm`, …) of a vegetation model.
#[cfg(test)]
pub(crate) fn kind_of(model: &str) -> Option<&'static str> {
    MODELS
        .iter()
        .find(|(_, names)| names.iter().any(|name| name == model))
        .map(|(kind, _)| kind.as_str())
}

/// Plants of one chunk, relative to the chunk origin: trees, bushes, ferns and rocks as Godot
/// `MultiMesh` buffers per model (transform and colour, `PLANT_FLOATS` per plant), grass and
/// flowers as transform buffers (12 floats each).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trees {
    /// Trees, bushes, ferns and rocks by vegetation model name.
    pub models: BTreeMap<String, Vec<f32>>,
    /// Grass tufts along the road.
    pub grass: Vec<f32>,
    /// Flower clumps in meadows along the road.
    pub flowers: Vec<f32>,
}

impl Trees {
    /// Small plants for a seasonal presentation, preserving every planted position. A stable
    /// subset of flower clumps stays in bloom; the others become full grass tufts. The kept
    /// world remains complete so a later season can restore all its flowers.
    #[must_use]
    pub fn small_plants(&self, flower_share: f64) -> (Cow<'_, [f32]>, Cow<'_, [f32]>) {
        if flower_share >= 1.0 || self.flowers.is_empty() {
            return (Cow::Borrowed(&self.grass), Cow::Borrowed(&self.flowers));
        }
        let mut grass = self.grass.clone();
        let mut flowers = Vec::new();
        for plant in self.flowers.as_chunks::<12>().0 {
            let seed = i64::from(plant[3].to_bits()) | (i64::from(plant[11].to_bits()) << 32);
            if crate::hash(seed) < flower_share {
                flowers.extend_from_slice(plant);
            } else {
                grass.extend_from_slice(plant);
            }
        }
        (Cow::Owned(grass), Cow::Owned(flowers))
    }

    /// Model buffers for drawing at a tree density in `[0, 1]` (R108). Smaller plants keep
    /// their population. Position-based sampling keeps the same trees as quality rises,
    /// including in worlds kept by older generators, without changing the stored world.
    pub fn models_for_density(
        &self,
        tree_density: f64,
    ) -> impl Iterator<Item = (&str, Cow<'_, [f32]>)> {
        self.models.iter().map(move |(name, buffer)| {
            let tree = matches!(
                name.split('_').next(),
                Some("conifer" | "broadleaf" | "palm" | "banana")
            );
            let buffer = if tree && tree_density < 1.0 {
                Cow::Owned(
                    buffer
                        .as_chunks::<PLANT_FLOATS>()
                        .0
                        .iter()
                        .filter(|plant| {
                            let seed = i64::from(plant[3].to_bits())
                                | (i64::from(plant[11].to_bits()) << 32);
                            crate::hash(seed) < tree_density
                        })
                        .flatten()
                        .copied()
                        .collect(),
                )
            } else {
                Cow::Borrowed(buffer.as_slice())
            };
            (name.as_str(), buffer)
        })
    }

    /// Number of trees, bushes, ferns and rocks (grass and flowers not counted).
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.values().map(Vec::len).sum::<usize>() / PLANT_FLOATS
    }

    /// Whether there are no trees, bushes, ferns or rocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Adds a plant of `kind` standing at `position`: one of its models and palette colours,
    /// size and heading chosen by `seed`.
    fn add(&mut self, kind: Plant, position: [f64; 3], seed: i64, chunk_origin: [f64; 3]) {
        let Some(models) = MODELS.get(kind.kind()).filter(|m| !m.is_empty()) else {
            return;
        };
        let pick = |salt: i64, count: usize| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )] // a small count; the product lies in [0, count)
            let index = (crate::hash(seed ^ salt) * count as f64) as usize;
            index.min(count - 1)
        };
        let model = &models[pick(0x2b7e, models.len())];
        let colour = palette::pick(kind.colours(), pick(0x6c8f, 64), 1.0);
        let scale = kind.scale(crate::hash(seed ^ 0x1234));
        let yaw = crate::hash(seed ^ 0x4321) * std::f64::consts::TAU;
        let buffer = self.models.entry(model.clone()).or_default();
        push_transform(buffer, position, scale, yaw, chunk_origin);
        buffer.extend(colour);
    }
}

/// A building's footprint as a circle around it, which plants keep out of, and its outline,
/// for what may stand close to its walls.
#[derive(Debug, Clone)]
pub(crate) struct Footprint {
    east: f64,
    north: f64,
    radius: f64,
    outline: Vec<Point>,
}

/// Whether something of `reach` metres standing at a point would touch one of `buildings`.
pub(crate) fn by_building(buildings: &[Footprint], east: f64, north: f64, reach: f64) -> bool {
    buildings
        .iter()
        .any(|b| (b.east - east).hypot(b.north - north) < b.radius + reach)
}

impl Footprint {
    /// How far a point is from the building's edge (roughly: from its centre, less its radius).
    pub(crate) fn distance(&self, east: f64, north: f64) -> f64 {
        ((self.east - east).hypot(self.north - north) - self.radius).max(0.0)
    }

    /// How far a point is from the building's walls; 0 inside.
    pub(crate) fn wall_distance(&self, east: f64, north: f64) -> f64 {
        if self.distance(east, north) > 0.0 {
            // Outside the circle round it: no nearer than that.
            return self.distance(east, north);
        }
        let mut inside = false;
        let mut nearest = f64::INFINITY;
        let count = self.outline.len();
        for i in 0..count {
            let (a, b) = (self.outline[i], self.outline[(i + 1) % count]);
            if (a.1 > north) != (b.1 > north)
                && east < a.0 + (north - a.1) * (b.0 - a.0) / (b.1 - a.1)
            {
                inside = !inside;
            }
            let (de, dn) = (b.0 - a.0, b.1 - a.1);
            let length_squared = (de * de + dn * dn).max(1e-12);
            let t = (((east - a.0) * de + (north - a.1) * dn) / length_squared).clamp(0.0, 1.0);
            nearest = nearest.min((east - a.0 - de * t).hypot(north - a.1 - dn * t));
        }
        if inside { 0.0 } else { nearest }
    }

    pub(crate) fn around(outline: &[Point]) -> Self {
        #[allow(clippy::cast_precision_loss)] // few corners
        let count = outline.len().max(1) as f64;
        let east = outline.iter().map(|p| p.0).sum::<f64>() / count;
        let north = outline.iter().map(|p| p.1).sum::<f64>() / count;
        let radius = outline
            .iter()
            .map(|p| (p.0 - east).hypot(p.1 - north))
            .fold(0.0, f64::max);
        Self {
            east,
            north,
            radius,
            outline: outline.to_vec(),
        }
    }
}

/// What plants are placed on: the chunk's ground, its land cover, the road ridden, the map's
/// other streets and the buildings around, in the region's climate.
#[derive(Clone, Copy)]
pub(crate) struct Ground<'a> {
    pub(crate) heights: &'a HeightGrid,
    pub(crate) land: &'a LandIndex,
    pub(crate) road: &'a RoadIndex,
    /// Formed ways' beds: the banks they cut are earth, not scree.
    pub(crate) beds: &'a crate::streets::Levels,
    pub(crate) streets: &'a Clearance,
    pub(crate) buildings: &'a [Footprint],
    pub(crate) climate: Climate,
    pub(crate) urban: &'a crate::urban::Urban,
}

impl Ground<'_> {
    /// Whether something of `reach` metres standing at a point would touch a building.
    fn by_building(&self, east: f64, north: f64, reach: f64) -> bool {
        by_building(self.buildings, east, north, reach)
    }

    /// The ground's steepest rise over run at a point.
    fn slope(&self, east: f64, north: f64) -> f64 {
        steepest(|e, n| self.heights.at(e, n), east, north)
    }

    /// The natural hillside's steepest rise over run at a point, before earthworks.
    fn natural_slope(&self, east: f64, north: f64) -> f64 {
        steepest(|e, n| self.heights.natural_at(e, n), east, north)
    }
}

/// The steepest rise over run of `height` at a point.
fn steepest(height: impl Fn(f64, f64) -> f64, east: f64, north: f64) -> f64 {
    let step = 2.0;
    let east_slope = (height(east + step, north) - height(east - step, north)) / (2.0 * step);
    let north_slope = (height(east, north + step) - height(east, north - step)) / (2.0 * step);
    east_slope.hypot(north_slope)
}

/// Places trees, bushes, ferns and rocks within the square `[origin, origin + size]` (metres
/// east/north): forests of conifers and broadleaf trees (conifers higher up) with bushes under
/// them and patches of ferns near the road; near the road also a few solitary trees and bushes
/// in meadows and gardens, and rocks on rocky ground and steep slopes. In subtropical lowlands palms
/// take the conifers' place, and banana plants and tropical shrubs the bushes'. Nothing on the
/// road or streets, in water or in buildings.
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
        ..
    } = *ground;
    let mut trees = Trees::default();
    // Give mapped woods their own denser grid without filling meadows and gardens too.
    for (spacing, forest) in [(FOREST_SPACING, true), (NEAR_SPACING, false)] {
        // One grid for the whole world: restarted at each chunk, a 7 m grid in 480 m chunks
        // gave the cells either side of some borders one seed, so twin plants 4 m apart (#599).
        let cells = |from: f64| cell_index(from, spacing)..cell_index(from + size, spacing);
        for row in cells(origin.1) {
            #[allow(clippy::cast_precision_loss)] // cell indices are small
            let north = row as f64 * spacing;
            for column in cells(origin.0) {
                #[allow(clippy::cast_precision_loss)] // cell indices are small
                let east = column as f64 * spacing;
                let seed = cell_seed(east, north, spacing);
                let (e, n) = (
                    east + (crate::hash(seed) - 0.5) * spacing,
                    north + (crate::hash(seed ^ 0x5bd1) - 0.5) * spacing,
                );
                let cover = land.cover_at(e, n);
                if ground.urban.paved_at((e, n), land) {
                    continue;
                }
                if (cover == Some(LandCover::Forest)) != forest {
                    continue;
                }
                let profile = road.nearest_profile(e, n, NEAR_DISTANCE);
                let road_distance = profile.map(|p| p.0);
                let Some(kind) = choose(cover, road_distance, heights.at(e, n), seed, ground, e, n)
                else {
                    continue;
                };
                let too_close =
                    profile.is_some_and(|(d, look)| d - look.width / 2.0 < kind.clearance() - 3.0);
                if too_close || streets.blocked(e, n, 2.0) || ground.by_building(e, n, 2.0) {
                    continue;
                }
                trees.add(kind, [e, heights.at(e, n), n], seed, chunk_origin);
            }
        }
    }
    trees
}

/// What grows at a spot, if anything: by its land cover, its distance from the road (`None`
/// beyond `NEAR_DISTANCE`), its height and slope, and dice from `seed`.
fn choose(
    cover: Option<LandCover>,
    road_distance: Option<f64>,
    height: f64,
    seed: i64,
    ground: &Ground,
    east: f64,
    north: f64,
) -> Option<Plant> {
    let dice = |salt: i64| crate::hash(seed ^ salt);
    if cover == Some(LandCover::Water) {
        return None;
    }
    let tropical = ground.climate.tropical_at(height);
    let near = road_distance.is_some();
    // A mapped wooded hillside stays wooded; slope alone cannot turn it into scree. Beside a
    // surrounding way the hillside's own slope counts, not its banks' (R141).
    let steep = || {
        let slope = if ground.beds.nearest(east, north, 0.0).is_some() {
            ground.natural_slope(east, north)
        } else {
            ground.slope(east, north)
        };
        slope > ROCKY_SLOPE
    };
    if near && (cover == Some(LandCover::Rock) || (cover != Some(LandCover::Forest) && steep())) {
        let share = if cover == Some(LandCover::Rock) {
            0.2
        } else {
            0.1
        };
        return (dice(0x0dc5) < share).then_some(Plant::Rock);
    }
    match cover {
        Some(LandCover::Forest) => {
            if road_distance.is_some_and(|d| d < UNDERSTOREY_DISTANCE) && dice(0x3b9a) < 0.12 {
                // Shared patch seeds gather ferns into stretches of woodland floor (R104).
                let patch = cell_seed(east, north, FERN_PATCH_SIZE);
                if crate::hash(patch ^ 0x6fe2) < 0.65 {
                    return Some(Plant::Fern);
                }
                return Some(if tropical {
                    tropical_shrub(dice(0x2d4f), 0.3)
                } else {
                    Plant::Bush
                });
            }
            let keep = (FOREST_SPACING / FAR_SPACING).powi(2);
            if !near && dice(0x9e37) > keep {
                return None;
            }
            if tropical {
                // Evergreen broadleaf forest, palms among it.
                return Some(if dice(0x7777) < TROPICAL_FOREST_PALMS {
                    Plant::Palm
                } else {
                    Plant::Broadleaf
                });
            }
            // Conifers dominate higher up.
            let conifer_share = ((height - 600.0) / 800.0).clamp(0.3, 0.9);
            Some(if dice(0x7777) < conifer_share {
                Plant::Conifer
            } else {
                Plant::Broadleaf
            })
        }
        // Open land near the road: now and then a tree on its own, or a bush.
        Some(LandCover::Meadow | LandCover::Public | LandCover::Residential) | None if near => {
            let (tree, shrub) = if cover == Some(LandCover::Residential) {
                (0.03, 0.04)
            } else {
                (0.006, 0.015)
            };
            let roll = dice(0x51a3);
            if roll < tree {
                // In the subtropics most trees standing alone are palms.
                Some(if tropical && dice(0x6a09) < TROPICAL_OPEN_PALMS {
                    Plant::Palm
                } else {
                    Plant::Broadleaf
                })
            } else if roll < tree + shrub {
                Some(if tropical {
                    // Banana plants grow in gardens more than in the open.
                    let bananas = if cover == Some(LandCover::Residential) {
                        0.4
                    } else {
                        0.15
                    };
                    tropical_shrub(dice(0x2d4f), bananas)
                } else {
                    Plant::Bush
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Share of palms among the trees of tropical forests, and among trees standing alone there.
const TROPICAL_FOREST_PALMS: f64 = 0.25;
const TROPICAL_OPEN_PALMS: f64 = 0.7;

/// A shrub of the tropics: a banana plant for a `roll` below `bananas`, else a broadleaf shrub.
fn tropical_shrub(roll: f64, bananas: f64) -> Plant {
    if roll < bananas {
        Plant::Banana
    } else {
        Plant::TropicalBush
    }
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
        ..
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
            let Some((distance, look)) = road.nearest_profile(e, n, GRASS_DISTANCE) else {
                continue;
            };
            if !inside
                || distance - look.width / 2.0 < GRASS_CLEARANCE - 3.0
                || streets.blocked(e, n, 0.2)
            {
                continue;
            }
            let cover = land.cover_at(e, n);
            if ground.urban.paved_at((e, n), land) {
                continue;
            }
            let (density, flowery) = match cover {
                Some(LandCover::Water | LandCover::Rock) => (0.0, false),
                // Yards and car parks: a little grass at the edges.
                Some(LandCover::Industrial | LandCover::Commercial) => (0.2, false),
                Some(LandCover::Forest) => (0.25, false),
                Some(LandCover::Meadow | LandCover::Public) | None => (1.0, true),
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

/// The first cell of a world-wide grid of `spacing` at or after `from`: each cell belongs
/// to exactly one chunk.
fn cell_index(from: f64, spacing: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)] // cell indices are small
    {
        (from / spacing).ceil() as i64
    }
}

/// A stable seed per grid cell.
fn cell_seed(east: f64, north: f64, spacing: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)] // cell indices are small
    let (e, n) = (
        (east / spacing).round() as i64,
        (north / spacing).round() as i64,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn plant_cells_either_side_of_a_chunk_border_have_their_own_seeds() {
        let size = crate::CHUNK_SIZE;
        for spacing in [FOREST_SPACING, NEAR_SPACING] {
            let cells = |from: f64| cell_index(from, spacing)..cell_index(from + size, spacing);
            // Neighbouring chunks share the grid: no cell twice, none left out.
            for origin in [-size, 0.0, size, 2.0 * size, 7.0 * size] {
                assert_eq!(cells(origin).end, cells(origin + size).start, "{spacing} m");
            }
            #[allow(clippy::cast_precision_loss)] // cell indices are small
            let seed = |cell: i64| cell_seed(cell as f64 * spacing, 0.0, spacing);
            let west = cells(size).last().unwrap();
            let east = cells(2.0 * size).next().unwrap();
            assert_ne!(seed(west), seed(east), "{spacing} m");
        }
    }

    #[test]
    fn seasonal_flowers_become_grass_without_losing_or_moving_plants() {
        let mut trees = Trees::default();
        push_transform(&mut trees.grass, [-5.0, 700.0, 8.0], 1.0, 0.0, [0.0; 3]);
        for index in 0..1000 {
            push_transform(
                &mut trees.flowers,
                [f64::from(index), 700.0, 5.0],
                1.0,
                0.0,
                [0.0; 3],
            );
        }
        let original = trees.clone();
        let (grass, flowers) = trees.small_plants(0.1);
        assert!(
            (60..=140).contains(&(flowers.len() / 12)),
            "sparse summer flowers"
        );
        assert_eq!(
            grass.len() + flowers.len(),
            trees.grass.len() + trees.flowers.len()
        );
        for plant in trees.flowers.as_chunks::<12>().0 {
            let count = grass
                .as_chunks::<12>()
                .0
                .iter()
                .chain(flowers.as_chunks::<12>().0)
                .filter(|p| p.iter().zip(plant).all(|(a, b)| a.to_bits() == b.to_bits()))
                .count();
            assert_eq!(count, 1, "each transform is kept exactly once");
        }
        assert_eq!(
            trees.small_plants(0.1),
            (grass, flowers),
            "selection is stable"
        );
        let (grass, flowers) = trees.small_plants(0.0);
        assert!(flowers.is_empty(), "autumn has no flowers");
        assert_eq!(grass.len(), trees.grass.len() + trees.flowers.len());
        let (grass, flowers) = trees.small_plants(1.0);
        assert_eq!(grass.as_ref(), trees.grass);
        assert_eq!(flowers.as_ref(), trees.flowers);
        assert_eq!(trees, original, "seasons do not alter the kept world");
    }

    #[test]
    fn quality_thins_trees_in_stable_nested_subsets_without_changing_the_world() {
        let mut trees = Trees::default();
        for (index, kind) in [Plant::Conifer, Plant::Broadleaf, Plant::Palm, Plant::Banana]
            .into_iter()
            .enumerate()
        {
            for seed in 0..2000 {
                #[allow(clippy::cast_precision_loss)] // four fixture species
                let north = f64::from(seed / 100) * 7.0 + index as f64 * 200.0;
                trees.add(
                    kind,
                    [f64::from(seed % 100) * 7.0, 0.0, north],
                    seed.into(),
                    [0.0; 3],
                );
            }
        }
        let original = trees.clone();
        let mut previous: BTreeMap<String, Vec<f32>> = BTreeMap::new();
        for density in [0.0, 0.125, 0.25, 0.5, 1.0] {
            let current: BTreeMap<String, Vec<f32>> = trees
                .models_for_density(density)
                .map(|(name, buffer)| (name.to_owned(), buffer.into_owned()))
                .collect();
            let count = current.values().map(Vec::len).sum::<usize>() / PLANT_FLOATS;
            #[allow(clippy::cast_precision_loss)] // small fixture counts
            let expected = trees.len() as f64 * density;
            #[allow(clippy::cast_precision_loss)] // small fixture counts
            let actual = count as f64;
            assert!(
                (actual - expected).abs() <= expected * 0.12,
                "density {density}: {actual} trees, expected about {expected}"
            );
            for (name, before) in &previous {
                let after = current[name].as_chunks::<PLANT_FLOATS>().0;
                for plant in before.as_chunks::<PLANT_FLOATS>().0 {
                    assert!(after.contains(plant), "higher quality keeps existing trees");
                }
            }
            let repeated: BTreeMap<String, Vec<f32>> = trees
                .models_for_density(density)
                .map(|(name, buffer)| (name.to_owned(), buffer.into_owned()))
                .collect();
            assert_eq!(current, repeated);
            previous = current;
        }
        assert_eq!(previous, original.models, "Ultra keeps every tree");
        assert_eq!(trees, original, "quality never changes the kept world");
    }

    #[test]
    fn tree_quality_keeps_undergrowth_and_rocks_and_samples_by_position() {
        let mut trees = Trees::default();
        for kind in [Plant::Bush, Plant::Rock, Plant::Fern, Plant::TropicalBush] {
            trees.add(kind, [12.0, 0.0, 20.0], 0, [0.0; 3]);
        }
        for (name, buffer) in trees.models_for_density(0.0) {
            assert!(matches!(buffer, Cow::Borrowed(_)));
            assert_eq!(buffer.as_ref(), trees.models[name]);
        }
        for seed in 0..500 {
            trees.add(
                Plant::Conifer,
                [f64::from(seed), 0.0, 10.0],
                seed.into(),
                [0.0; 3],
            );
        }
        let selected: BTreeMap<String, Vec<f32>> = trees
            .models_for_density(0.25)
            .map(|(name, buffer)| (name.to_owned(), buffer.into_owned()))
            .collect();
        for buffer in trees.models.values_mut() {
            let mut plants = buffer.as_chunks::<PLANT_FLOATS>().0.to_vec();
            plants.reverse();
            *buffer = plants.into_iter().flatten().collect();
        }
        for (name, buffer) in trees.models_for_density(0.25) {
            let mut before = selected[name].as_chunks::<PLANT_FLOATS>().0.to_vec();
            before.reverse();
            assert_eq!(buffer.as_ref(), before.concat());
        }
    }

    #[test]
    fn every_plant_has_models_with_files_and_palette_colours() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/assets/models/vegetation");
        for plant in [
            Plant::Conifer,
            Plant::Broadleaf,
            Plant::Bush,
            Plant::Rock,
            Plant::Palm,
            Plant::Banana,
            Plant::TropicalBush,
            Plant::Fern,
        ] {
            let models = MODELS.get(plant.kind()).map_or(&[][..], Vec::as_slice);
            assert!(!models.is_empty(), "no {} models", plant.kind());
            for name in models {
                assert!(
                    directory.join(format!("{name}.glb")).is_file(),
                    "{name} is missing"
                );
            }
            // Panics if the palette lacks the plant's colours.
            palette::pick(plant.colours(), 0, 1.0);
        }
    }
}
