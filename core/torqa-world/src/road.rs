//! The road: a spatial index of its centre line and its mesh.

use std::collections::HashMap;

use torqa_routes::{LocalProjection, Route, Surface, catmull_rom};

use crate::MeshData;

/// Size of the index cells.
const CELL: f64 = 100.0;
/// The road is drawn as a smooth curve through the route's points, sampled this often.
const DRAW_STEP: f64 = 2.0;
/// The road's edges bevel this far out and down, a little below the level ground beside it
/// (`ROAD_SINK`): a low, soft edge rather than a kerb...
const BEVEL_REACH: f64 = 0.4;
const BEVEL_DROP: f64 = 0.25;
/// ...and skirts hang on from there this far down and out, so wherever the ground falls away the
/// road shows an edge rather than a gap below it.
const SKIRT_DEPTH: f64 = 1.2;
const SKIRT_REACH: f64 = 0.8;

/// Vertices across the road: skirt, bevel and edge on either side.
const RING: usize = 6;

/// Where another street meets the road: distance along the road, side (1 right of travel, −1
/// left) and the street's half width.
pub(crate) type Mouth = (f64, f64, f64);

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

/// A point of the route's centre line.
#[derive(Debug, Clone, Copy)]
struct Centre {
    position: (f64, f64),
    elevation: f64,
    distance: f64,
    surface: Surface,
}

/// The route's centre line in local coordinates, indexed for nearest-point queries. It is a
/// smooth curve through the route's points, so bends look like a road's, not a polygon's.
pub(crate) struct RoadIndex {
    segments: Vec<Segment>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl RoadIndex {
    pub(crate) fn new(route: &Route, projection: &LocalProjection) -> Self {
        let points: Vec<Centre> = route
            .points()
            .iter()
            .map(|p| Centre {
                position: projection.project(p.lat, p.lon),
                elevation: p.elevation.0,
                distance: p.distance.0,
                surface: p.surface,
            })
            .collect();
        let local = smooth_curve(&points);
        let segments: Vec<Segment> = local
            .windows(2)
            .map(|w| Segment {
                a: w[0].position,
                b: w[1].position,
                elevation_a: w[0].elevation,
                elevation_b: w[1].elevation,
                distance_a: w[0].distance,
                distance_b: w[1].distance,
                // A segment touching a bridge or tunnel belongs to it.
                surface: if w[0].surface == Surface::Ground {
                    w[1].surface
                } else {
                    w[0].surface
                },
            })
            .collect();
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (index, segment) in segments.iter().enumerate() {
            // Segments are a few metres long, far shorter than a cell: both end cells cover
            // them.
            for point in [segment.a, segment.b] {
                let list = cells.entry(cell_of(point.0, point.1)).or_default();
                if list.last() != Some(&index) {
                    list.push(index);
                }
            }
        }
        Self { segments, cells }
    }

    /// Whether a way through (`east`, `north`) heading in `direction` (unit) runs along the road
    /// within `reach` there, rather than joining or crossing it.
    pub(crate) fn runs_along(
        &self,
        east: f64,
        north: f64,
        reach: f64,
        direction: (f64, f64),
    ) -> bool {
        let (low_e, low_n) = cell_of(east - reach, north - reach);
        let (high_e, high_n) = cell_of(east + reach, north + reach);
        (low_e..=high_e).any(|ce| {
            (low_n..=high_n).any(|cn| {
                self.cells
                    .get(&(ce, cn))
                    .into_iter()
                    .flatten()
                    .any(|&index| {
                        let segment = &self.segments[index];
                        let (d, _, _) = closest_on_segment(segment, east, north);
                        let (de, dn) = self::direction(segment);
                        d <= reach && (de * direction.0 + dn * direction.1).abs() > 0.8
                    })
            })
        })
    }

    /// Every piece of road within `reach`: its distance, the road's elevation there and what
    /// carries it. Where the road passes a place more than once (hairpins), each pass is there.
    pub(crate) fn near(&self, east: f64, north: f64, reach: f64) -> Vec<(f64, f64, Surface)> {
        let (low_e, low_n) = cell_of(east - reach, north - reach);
        let (high_e, high_n) = cell_of(east + reach, north + reach);
        let mut found = Vec::new();
        for ce in low_e..=high_e {
            for cn in low_n..=high_n {
                for &index in self.cells.get(&(ce, cn)).into_iter().flatten() {
                    let segment = &self.segments[index];
                    // Listed in both its ends' cells: look at it from the first one in range.
                    let first = cell_of(segment.a.0, segment.a.1);
                    let in_range = |(e, n): (i64, i64)| {
                        (low_e..=high_e).contains(&e) && (low_n..=high_n).contains(&n)
                    };
                    if first != (ce, cn) && in_range(first) {
                        continue;
                    }
                    let candidate = closest_on_segment(segment, east, north);
                    if candidate.0 <= reach {
                        found.push(candidate);
                    }
                }
            }
        }
        found
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

    /// The nearest point of the road within `reach` of (`east`, `north`): the distance to it,
    /// how far along the road it is and on which side the point lies (1 right of travel, −1
    /// left).
    pub(crate) fn locate(&self, east: f64, north: f64, reach: f64) -> Option<(f64, f64, f64)> {
        let (low_e, low_n) = cell_of(east - reach, north - reach);
        let (high_e, high_n) = cell_of(east + reach, north + reach);
        let mut best: Option<(f64, f64, f64)> = None;
        for ce in low_e..=high_e {
            for cn in low_n..=high_n {
                for &index in self.cells.get(&(ce, cn)).into_iter().flatten() {
                    let segment = &self.segments[index];
                    let (de, dn) = (segment.b.0 - segment.a.0, segment.b.1 - segment.a.1);
                    let length_squared = (de * de + dn * dn).max(1e-12);
                    let t = (((east - segment.a.0) * de + (north - segment.a.1) * dn)
                        / length_squared)
                        .clamp(0.0, 1.0);
                    let (pe, pn) = (segment.a.0 + de * t, segment.a.1 + dn * t);
                    let distance = (east - pe).hypot(north - pn);
                    if distance <= reach && best.is_none_or(|b| distance < b.0) {
                        // Right of travel is where the direction turns clockwise.
                        let cross = de * (north - segment.a.1) - dn * (east - segment.a.0);
                        let along =
                            segment.distance_a + (segment.distance_b - segment.distance_a) * t;
                        best = Some((distance, along, if cross > 0.0 { -1.0 } else { 1.0 }));
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

    /// A ribbon `2 × half_width` wide along the centre line, its edges bevelled down to the
    /// ground and skirts hanging on below. Texture coordinates: `u` 0–1 across the road (below
    /// 0 and above 1 on bevels and skirts, which are shoulders — except where another street
    /// meets the road, `mouths`, where the bevel is road too), `v` the distance in metres.
    #[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
    pub(crate) fn mesh(&self, half_width: f64, mouths: &[Mouth]) -> MeshData {
        let mut mesh = MeshData::default();
        let Some(last) = self.segments.last() else {
            return mesh;
        };
        let centres: Vec<((f64, f64), f64, f64)> = self
            .segments
            .iter()
            .map(|s| (s.a, s.elevation_a, s.distance_a))
            .chain([(last.b, last.elevation_b, last.distance_b)])
            .collect();
        let joined = |distance: f64, side: f64| {
            mouths
                .iter()
                .any(|&(along, at, half)| at * side > 0.0 && (along - distance).abs() <= half + 1.0)
        };
        for (index, &((east, north), elevation, distance)) in centres.iter().enumerate() {
            // Across the road square to the curve: halfway between the pieces either side.
            let before = self.segments[index.saturating_sub(1)];
            let after = self.segments[index.min(self.segments.len() - 1)];
            let (d1, d2) = (direction(&before), direction(&after));
            let (de, dn) = (d1.0 + d2.0, d1.1 + d2.1);
            let length = de.hypot(dn).max(f64::EPSILON);
            // Right of travel is the direction turned clockwise by 90°.
            let (re, rn) = (dn / length, -de / length);
            let at = |across: f64, drop: f64| {
                [
                    (east + re * across) as f32,
                    (elevation - drop) as f32,
                    (-(north + rn * across)) as f32,
                ]
            };
            let (bevel, skirt) = (half_width + BEVEL_REACH, half_width + SKIRT_REACH);
            let left = if joined(distance, -1.0) { 0.0 } else { -0.08 };
            let right = if joined(distance, 1.0) { 1.0 } else { 1.08 };
            let tilt = BEVEL_DROP / BEVEL_REACH;
            // Left skirt, left bevel, left edge, right edge, right bevel, right skirt.
            let ring = [
                (
                    at(-skirt, SKIRT_DEPTH),
                    [-re as f32, 0.5, rn as f32],
                    -0.2_f32,
                ),
                (
                    at(-bevel, BEVEL_DROP),
                    [(-re * tilt) as f32, 1.0, (rn * tilt) as f32],
                    left,
                ),
                (at(-half_width, 0.0), [0.0, 1.0, 0.0], 0.0),
                (at(half_width, 0.0), [0.0, 1.0, 0.0], 1.0),
                (
                    at(bevel, BEVEL_DROP),
                    [(re * tilt) as f32, 1.0, (-rn * tilt) as f32],
                    right,
                ),
                (at(skirt, SKIRT_DEPTH), [re as f32, 0.5, -rn as f32], 1.2),
            ];
            for (vertex, normal, u) in ring {
                mesh.vertices.push(vertex);
                let length =
                    (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
                mesh.normals.push(normal.map(|v| v / length));
                mesh.uvs.push([u, distance as f32]);
            }
            if index > 0 {
                let base = u32::try_from(index * RING).expect("road fits u32");
                let previous = base - u32::try_from(RING).expect("small");
                for k in 0..u32::try_from(RING - 1).expect("small") {
                    let (a0, b0, a1, b1) = (previous + k, previous + k + 1, base + k, base + k + 1);
                    mesh.indices.extend([a0, a1, b1, a0, b1, b0]);
                }
            }
        }
        mesh
    }
}

/// A smooth curve through the route's points (centripetal Catmull-Rom), every `DRAW_STEP`
/// metres or so. Elevation and distance change linearly between the points, so the road's
/// profile stays as smoothed on import.
fn smooth_curve(points: &[Centre]) -> Vec<Centre> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 5);
    for i in 0..points.len() - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        let p0 = points[i.saturating_sub(1)];
        let p3 = points[(i + 2).min(points.len() - 1)];
        let length = distance(p1.position, p2.position);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short segments
        let pieces = ((length / DRAW_STEP).ceil() as usize).max(1);
        for k in 0..pieces {
            #[allow(clippy::cast_precision_loss)] // few pieces
            let u = k as f64 / pieces as f64;
            out.push(Centre {
                position: catmull_rom([p0.position, p1.position, p2.position, p3.position], u),
                elevation: p1.elevation + (p2.elevation - p1.elevation) * u,
                distance: p1.distance + (p2.distance - p1.distance) * u,
                // Between a structure's point and the ground, the piece belongs to the
                // structure, as the route's segments do.
                surface: if u == 0.0 || p1.surface != Surface::Ground {
                    p1.surface
                } else {
                    p2.surface
                },
            });
        }
    }
    out.extend(points.last());
    out
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
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
