//! What covers the ground at a point, from OpenStreetMap areas.

use std::collections::HashMap;

use torqa_osm::{Area, LandCover};
use torqa_routes::LocalProjection;

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
        LandCover::Residential => 1,
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

/// Terrain tint for a land cover; alpha 1 marks water for the shader.
pub(crate) fn color(cover: Option<LandCover>) -> [f32; 4] {
    match cover {
        None => [0.30, 0.46, 0.20, 0.0],
        Some(LandCover::Meadow) => [0.36, 0.52, 0.22, 0.0],
        Some(LandCover::Forest) => [0.13, 0.24, 0.10, 0.0],
        Some(LandCover::Farmland) => [0.55, 0.53, 0.30, 0.0],
        Some(LandCover::Orchard) => [0.38, 0.48, 0.22, 0.0],
        Some(LandCover::Residential) => [0.42, 0.45, 0.38, 0.0],
        Some(LandCover::Rock) => [0.45, 0.43, 0.40, 0.0],
        Some(LandCover::Water) => [0.10, 0.22, 0.30, 1.0],
    }
}
