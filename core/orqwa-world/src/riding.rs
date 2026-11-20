//! Rider clearance from the road that is actually drawn, including kept worlds.

use std::collections::HashMap;

use orqwa_domain::units::Meters;

use crate::MeshData;
use crate::road::RING;

#[derive(Debug)]
struct Span {
    a: [f64; 3],
    b: [f64; 3],
    widths: [f64; 2],
}

/// A carriageway's width at a point and its middle there (east, north).
type Found = (f64, (f64, f64));

/// Widths of the drawn road, indexed for rider placement. Reading the mesh rather than
/// estimating again from the map also follows worlds kept with an older generator.
#[derive(Debug, Default)]
pub struct RoadWidths {
    spans: Vec<Span>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl RoadWidths {
    /// Indexes the carriageway, excluding its bevels, skirts and gaps between separate runs.
    ///
    /// # Panics
    /// If the mesh contains invalid vertex indices rather than a generated road.
    #[must_use]
    pub fn new(road: &MeshData) -> Self {
        let mut result = Self::default();
        #[allow(clippy::cast_possible_truncation)] // six vertices per cross-section
        let ring = RING as u32;
        let section = |left: u32| {
            let a = road.vertices[left as usize].map(f64::from);
            let b = road.vertices[left as usize + 1].map(f64::from);
            (
                std::array::from_fn(|i| f64::midpoint(a[i], b[i])),
                (b[0] - a[0]).hypot(b[2] - a[2]),
            )
        };
        for triangle in road.indices.as_chunks::<3>().0 {
            // Only the first triangle across the flat carriageway joins two cross-sections.
            if triangle[0] % ring != 2 || triangle[1] % ring != 2 || triangle[2] != triangle[1] + 1
            {
                continue;
            }
            let (a, width_a) = section(triangle[0]);
            let (b, width_b) = section(triangle[1]);
            result.add(Span {
                a,
                b,
                widths: [width_a, width_b],
            });
        }
        result
    }

    /// A road known by the line down its middle and its width along it, such as a world
    /// file's route (ADR 0042): points in the scene's axes (east, elevation, −north), each
    /// with the road's width there.
    #[must_use]
    pub fn along(points: &[([f64; 3], f64)]) -> Self {
        let mut result = Self::default();
        for pair in points.windows(2) {
            let ((a, width_a), (b, width_b)) = (pair[0], pair[1]);
            result.add(Span {
                a,
                b,
                widths: [width_a, width_b],
            });
        }
        result
    }

    fn add(&mut self, span: Span) {
        let index = self.spans.len();
        let (from, to) = (cell(span.a[0], -span.a[2]), cell(span.b[0], -span.b[2]));
        self.spans.push(span);
        for x in from.0.min(to.0)..=from.0.max(to.0) {
            for y in from.1.min(to.1)..=from.1.max(to.1) {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }

    /// Carriageway width at metres east/north and the route's elevation. Height distinguishes
    /// stacked roads; spatial lookup also follows a road ridden again in the other direction.
    /// Without a drawn road, uses the generator's six-metre fallback.
    #[must_use]
    pub fn at(&self, east: f64, north: f64, elevation: Meters) -> Meters {
        Meters(
            self.nearest(east, north, elevation)
                .map_or(6.0, |(width, _)| width),
        )
    }

    /// The middle of the carriageway nearest the route at metres east/north and the route's
    /// elevation, in metres east/north: the road is laid off the map's line where the
    /// buildings beside it show the street lies (`RoadIndex::fit_between`). The route's own
    /// point without a drawn road.
    #[must_use]
    pub fn middle(&self, east: f64, north: f64, elevation: Meters) -> (f64, f64) {
        // Further off, the nearest carriageway is another stretch of the road (a hairpin's).
        self.nearest(east, north, elevation)
            .map(|(_, middle)| middle)
            .filter(|m| (m.0 - east).hypot(m.1 - north) <= crate::road::MAX_SHIFT + 0.5)
            .unwrap_or((east, north))
    }

    /// The width of the nearest stretch of carriageway and its middle there (east, north).
    fn nearest(&self, east: f64, north: f64, elevation: Meters) -> Option<Found> {
        let (cx, cy) = cell(east, north);
        let mut best: Option<(f64, Found)> = None;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &i in self.cells.get(&(x, y)).into_iter().flatten() {
                    let span = &self.spans[i];
                    let dx = span.b[0] - span.a[0];
                    let dz = span.b[2] - span.a[2];
                    let length_sq = dx * dx + dz * dz;
                    let t = if length_sq > 1e-9 {
                        (((east - span.a[0]) * dx + (-north - span.a[2]) * dz) / length_sq)
                            .clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let point: [f64; 3] = std::array::from_fn(|axis| {
                        span.a[axis] + (span.b[axis] - span.a[axis]) * t
                    });
                    let distance = (east - point[0])
                        .hypot(north + point[2])
                        .hypot(elevation.0 - point[1]);
                    if best.is_none_or(|(near, _)| distance < near) {
                        best = Some((
                            distance,
                            (
                                span.widths[0] + (span.widths[1] - span.widths[0]) * t,
                                (point[0], -point[2]),
                            ),
                        ));
                    }
                }
            }
        }
        best.map(|(_, found)| found)
    }
}

#[allow(clippy::cast_possible_truncation)] // bounded local map coordinates
fn cell(east: f64, north: f64) -> (i64, i64) {
    (
        (east / 100.0).floor() as i64,
        (north / 100.0).floor() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::road::{Centre, RoadIndex};
    use orqwa_routes::Surface;

    fn road(width: f64, height: f64) -> MeshData {
        RoadIndex::from_lines(&[vec![
            Centre {
                position: (0.0, 0.0),
                elevation: height,
                distance: 0.0,
                surface: Surface::Ground,
                mapped: true,
            },
            Centre {
                position: (0.0, 100.0),
                elevation: height,
                distance: 100.0,
                surface: Surface::Ground,
                mapped: true,
            },
        ]])
        .mesh(width / 2.0, &[])
    }

    #[test]
    fn rider_clearance_follows_narrow_and_wide_drawn_roads() {
        for width in [1.6, 2.5, 3.5, 6.0, 10.5] {
            let world = crate::World {
                road: road(width, 400.0),
                ..crate::World::default()
            };
            let mut stored = Vec::new();
            crate::stored::write_whole(&world, crate::GENERATOR, &mut stored).unwrap();
            let (kept, _) = crate::stored::read_whole(&mut stored.as_slice()).unwrap();
            let profile = RoadWidths::new(&kept.road);
            assert!((profile.at(0.0, 50.0, Meters(400.0)).0 - width).abs() < 1e-5);
        }
    }

    #[test]
    fn rider_clearance_interpolates_a_narrowing_carriageway() {
        let mut mesh = road(6.0, 400.0);
        for vertex in mesh.vertices.iter_mut().skip(RING) {
            vertex[0] *= 0.5;
        }
        let profile = RoadWidths::new(&mesh);
        for (north, width) in [(0.0, 6.0), (50.0, 4.5), (100.0, 3.0)] {
            assert!((profile.at(0.0, north, Meters(400.0)).0 - width).abs() < 1e-5);
        }
    }

    #[test]
    fn bridges_follow_their_own_width_above_another_road() {
        let mut mesh = road(2.5, 400.0);
        mesh.append(road(7.0, 410.0));
        let profile = RoadWidths::new(&mesh);
        assert!((profile.at(0.0, 50.0, Meters(400.0)).0 - 2.5).abs() < 1e-5);
        assert!((profile.at(0.0, 50.0, Meters(410.0)).0 - 7.0).abs() < 1e-5);
    }
}
