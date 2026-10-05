//! What covers the ground at a point, from OpenStreetMap areas.

use std::collections::HashMap;
use std::sync::LazyLock;

use torqa_osm::{Area, LandCover};
use torqa_routes::LocalProjection;

use crate::palette;

/// Size of the index cells.
const CELL: f64 = 250.0;

/// Ring in metres east/north.
type Ring = Vec<(f64, f64)>;

struct Polygon {
    cover: LandCover,
    /// Outer rings and holes together: the even-odd rule handles both.
    rings: Vec<Ring>,
}

/// Land cover areas in local coordinates, indexed for point queries.
pub(crate) struct LandIndex {
    polygons: Vec<Polygon>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl LandIndex {
    pub(crate) fn new(areas: &[Area], projection: &LocalProjection) -> Self {
        let mut polygons = Vec::new();
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for area in areas {
            let rings: Vec<Ring> = area
                .outer
                .iter()
                .chain(&area.inner)
                .map(|ring| {
                    ring.iter()
                        .map(|&(lat, lon)| projection.project(lat, lon))
                        .collect()
                })
                .collect();
            let (mut low, mut high) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
            for &(e, n) in rings.iter().flatten() {
                low = (low.0.min(e), low.1.min(n));
                high = (high.0.max(e), high.1.max(n));
            }
            let index = polygons.len();
            let (low_cell, high_cell) = (cell_of(low.0, low.1), cell_of(high.0, high.1));
            for ce in low_cell.0..=high_cell.0 {
                for cn in low_cell.1..=high_cell.1 {
                    cells.entry((ce, cn)).or_default().push(index);
                }
            }
            polygons.push(Polygon {
                cover: area.cover,
                rings,
            });
        }
        Self { polygons, cells }
    }

    /// Whether an area of `cover` contains the point, whatever else covers it too.
    pub(crate) fn has(&self, east: f64, north: f64, cover: LandCover) -> bool {
        self.cells
            .get(&cell_of(east, north))
            .is_some_and(|indices| {
                indices
                    .iter()
                    .map(|&i| &self.polygons[i])
                    .any(|polygon| polygon.cover == cover && contains(&polygon.rings, east, north))
            })
    }

    /// The land cover at a point; where areas overlap, the most specific one wins.
    pub(crate) fn cover_at(&self, east: f64, north: f64) -> Option<LandCover> {
        self.cells
            .get(&cell_of(east, north))?
            .iter()
            .map(|&i| &self.polygons[i])
            .filter(|polygon| contains(&polygon.rings, east, north))
            .map(|polygon| polygon.cover)
            .max_by_key(|&cover| priority(cover))
    }
}

/// Water and rock are mapped precisely and win; forests inside towns (parks) beat the town.
fn priority(cover: LandCover) -> u8 {
    match cover {
        LandCover::Water => 6,
        LandCover::Rock => 5,
        LandCover::Forest => 4,
        LandCover::Orchard => 3,
        LandCover::Farmland => 2,
        LandCover::Residential | LandCover::Industrial => 1,
        LandCover::Meadow => 0,
    }
}

/// Even-odd point-in-polygon test over all rings.
fn contains(rings: &[Ring], east: f64, north: f64) -> bool {
    let mut inside = false;
    for ring in rings {
        for pair in ring.windows(2) {
            let ((e1, n1), (e2, n2)) = (pair[0], pair[1]);
            if (n1 > north) != (n2 > north) && east < (e2 - e1) * (north - n1) / (n2 - n1) + e1 {
                inside = !inside;
            }
        }
    }
    inside
}

fn cell_of(east: f64, north: f64) -> (i64, i64) {
    #[allow(clippy::cast_possible_truncation)] // local coordinates stay far below 2^63 cells
    ((east / CELL).floor() as i64, (north / CELL).floor() as i64)
}

/// Ground colours of the land covers, read from the palette once (it is asked per vertex).
struct Ground {
    meadow: [f32; 4],
    forest: [f32; 4],
    farmland: [f32; 4],
    orchard: [f32; 4],
    town: [f32; 4],
    industrial: [f32; 4],
    rock: [f32; 4],
    bed: [f32; 4],
}

static GROUND: LazyLock<Ground> = LazyLock::new(|| Ground {
    meadow: palette::srgb("ground.meadow", 0.0),
    forest: palette::srgb("ground.forest", 0.0),
    farmland: palette::srgb("ground.farmland", 0.0),
    orchard: palette::srgb("ground.orchard", 0.0),
    town: palette::srgb("ground.town", 0.0),
    industrial: palette::srgb("ground.industrial", 0.0),
    rock: palette::srgb("ground.rock", 0.0),
    bed: palette::srgb("water.bed", 1.0),
});

/// Ground colour (sRGB) for a land cover; alpha 1 marks the bed under water for the shader.
pub(crate) fn color(cover: Option<LandCover>) -> [f32; 4] {
    let ground = &*GROUND;
    match cover {
        None | Some(LandCover::Meadow) => ground.meadow,
        Some(LandCover::Forest) => ground.forest,
        Some(LandCover::Farmland) => ground.farmland,
        Some(LandCover::Orchard) => ground.orchard,
        Some(LandCover::Residential) => ground.town,
        Some(LandCover::Industrial) => ground.industrial,
        Some(LandCover::Rock) => ground.rock,
        Some(LandCover::Water) => ground.bed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COVERS: [Option<LandCover>; 9] = [
        None,
        Some(LandCover::Forest),
        Some(LandCover::Meadow),
        Some(LandCover::Farmland),
        Some(LandCover::Orchard),
        Some(LandCover::Residential),
        Some(LandCover::Industrial),
        Some(LandCover::Water),
        Some(LandCover::Rock),
    ];

    #[test]
    fn every_cover_has_a_palette_colour_and_only_water_is_marked() {
        for cover in COVERS {
            let colour = color(cover);
            assert_eq!(
                colour[3] > 0.5,
                cover == Some(LandCover::Water),
                "{cover:?}"
            );
        }
    }

    #[test]
    fn forests_read_darker_than_open_land() {
        let luminance = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let forest = luminance(color(Some(LandCover::Forest)));
        for open in [None, Some(LandCover::Meadow), Some(LandCover::Farmland)] {
            assert!(forest < luminance(color(open)) - 0.1, "{open:?}");
        }
    }
}
