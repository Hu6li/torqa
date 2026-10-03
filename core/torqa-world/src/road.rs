//! The road: a spatial index of its centre line and its mesh.

use std::collections::HashMap;

use torqa_routes::{LocalProjection, Route, Surface};

use crate::MeshData;

/// Size of the index cells.
const CELL: f64 = 100.0;

#[derive(Debug, Clone, Copy)]
struct Segment {
    /// Start and end in metres east/north.
    a: (f64, f64),
    b: (f64, f64),
    /// Road elevation at start and end.
    elevation_a: f64,
    elevation_b: f64,
    /// Distance along the route at the start.
    distance_a: f64,
    distance_b: f64,
    /// What carries the road on this segment.
    surface: Surface,
}

/// The route's centre line in local coordinates, indexed for nearest-point queries.
pub(crate) struct RoadIndex {
    segments: Vec<Segment>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl RoadIndex {
    pub(crate) fn new(route: &Route, projection: &LocalProjection) -> Self {
        let local: Vec<_> = route
            .points()
            .iter()
            .map(|p| {
                (
                    projection.project(p.lat, p.lon),
                    p.elevation.0,
                    p.distance.0,
                    p.surface,
                )
            })
            .collect();
        let segments: Vec<Segment> = local
            .windows(2)
            .map(|w| Segment {
                a: w[0].0,
                b: w[1].0,
                elevation_a: w[0].1,
                elevation_b: w[1].1,
                distance_a: w[0].2,
                distance_b: w[1].2,
                // A segment touching a bridge or tunnel belongs to it.
                surface: if w[0].3 == Surface::Ground {
                    w[1].3
                } else {
                    w[0].3
                },
            })
            .collect();
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (index, segment) in segments.iter().enumerate() {
            // Segments are 10 m long, far shorter than a cell: both end cells cover them.
            for point in [segment.a, segment.b] {
                let list = cells.entry(cell_of(point.0, point.1)).or_default();
                if list.last() != Some(&index) {
                    list.push(index);
                }
            }
        }
        Self { segments, cells }
    }

    /// Distance to the closest point of the road within `max_distance`, the road's elevation
    /// there and what carries the road.
    pub(crate) fn nearest(
        &self,
        east: f64,
        north: f64,
        max_distance: f64,
    ) -> Option<(f64, f64, Surface)> {
        let reach = if max_distance.is_finite() {
            max_distance
        } else {
            // Unbounded: the whole road.
            return self
                .segments
                .iter()
                .map(|s| closest_on_segment(s, east, north))
                .min_by(|a, b| a.0.total_cmp(&b.0));
        };
        let (low_e, low_n) = cell_of(east - reach, north - reach);
        let (high_e, high_n) = cell_of(east + reach, north + reach);
        let mut best: Option<(f64, f64, Surface)> = None;
        for ce in low_e..=high_e {
            for cn in low_n..=high_n {
                let Some(indices) = self.cells.get(&(ce, cn)) else {
                    continue;
                };
                for &index in indices {
                    let candidate = closest_on_segment(&self.segments[index], east, north);
                    if candidate.0 <= reach && best.is_none_or(|b| candidate.0 < b.0) {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    /// Points along the road roughly every `spacing` metres, in metres east/north.
    pub(crate) fn samples(&self, spacing: f64) -> Vec<(f64, f64)> {
        let mut samples = Vec::new();
        let mut next = 0.0;
        for segment in &self.segments {
            if segment.distance_a >= next {
                samples.push(segment.a);
                next = segment.distance_a + spacing;
            }
        }
        if let Some(last) = self.segments.last() {
            samples.push(last.b);
        }
        samples
    }

    /// Consecutive centre-line points on bridges and in tunnels, one list per structure.
    pub(crate) fn structure_runs(&self) -> Vec<(Surface, Vec<CentrePoint>)> {
        let mut runs: Vec<(Surface, Vec<CentrePoint>)> = Vec::new();
        let mut previous = Surface::Ground;
        for segment in &self.segments {
            let point = |position: (f64, f64), elevation: f64| CentrePoint {
                position,
                elevation,
                direction: direction(segment),
            };
            if segment.surface != Surface::Ground {
                if segment.surface != previous {
                    runs.push((segment.surface, vec![point(segment.a, segment.elevation_a)]));
                }
                if let Some((_, points)) = runs.last_mut() {
                    points.push(point(segment.b, segment.elevation_b));
                }
            }
            previous = segment.surface;
        }
        runs
    }

    /// A ribbon `2 × half_width` wide along the centre line.
    #[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
    pub(crate) fn mesh(&self, half_width: f64) -> MeshData {
        let mut mesh = MeshData::default();
        let Some(last) = self.segments.last() else {
            return mesh;
        };
        // Centre points with their direction; the last point reuses the last segment's.
        let centres = self
            .segments
            .iter()
            .map(|s| (s.a, s.elevation_a, s.distance_a, direction(s)))
            .chain([(last.b, last.elevation_b, last.distance_b, direction(last))]);
        for (index, ((east, north), elevation, distance, (de, dn))) in centres.enumerate() {
            // Right of travel is the direction turned clockwise by 90°.
            let (re, rn) = (dn, -de);
            for (side, u) in [(-1.0, 0.0), (1.0, 1.0)] {
                let (e, n) = (
                    east + re * half_width * side,
                    north + rn * half_width * side,
                );
                mesh.vertices.push([e as f32, elevation as f32, -n as f32]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.uvs.push([u, distance as f32]);
            }
            if index > 0 {
                let base = u32::try_from(index * 2).expect("road fits u32");
                let (left_0, right_0, left_1, right_1) = (base - 2, base - 1, base, base + 1);
                mesh.indices
                    .extend([left_0, left_1, right_1, left_0, right_1, right_0]);
            }
        }
        mesh
    }
}

/// A point on the road's centre line with its direction of travel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CentrePoint {
    /// Metres east/north.
    pub(crate) position: (f64, f64),
    /// Road surface elevation.
    pub(crate) elevation: f64,
    /// Unit direction of travel (east, north).
    pub(crate) direction: (f64, f64),
}

fn cell_of(east: f64, north: f64) -> (i64, i64) {
    #[allow(clippy::cast_possible_truncation)] // local coordinates stay far below 2^63 cells
    ((east / CELL).floor() as i64, (north / CELL).floor() as i64)
}

/// Unit direction of travel of a segment (east, north).
fn direction(segment: &Segment) -> (f64, f64) {
    let (de, dn) = (segment.b.0 - segment.a.0, segment.b.1 - segment.a.1);
    let length = de.hypot(dn).max(f64::EPSILON);
    (de / length, dn / length)
}

/// Distance from a point to a segment, the road elevation at the closest point and the
/// segment's surface.
fn closest_on_segment(segment: &Segment, east: f64, north: f64) -> (f64, f64, Surface) {
    let (de, dn) = (segment.b.0 - segment.a.0, segment.b.1 - segment.a.1);
    let length_squared = de * de + dn * dn;
    let t = if length_squared > 0.0 {
        (((east - segment.a.0) * de + (north - segment.a.1) * dn) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (pe, pn) = (segment.a.0 + de * t, segment.a.1 + dn * t);
    let elevation = segment.elevation_a + (segment.elevation_b - segment.elevation_a) * t;
    ((east - pe).hypot(north - pn), elevation, segment.surface)
}
