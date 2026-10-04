//! Puts a recorded track onto the roads it rides (map matching). GPS positions wander a few
//! metres either side of the road and sparse files cut corners between their points; the route
//! drawn and ridden should follow the road itself.

use std::collections::HashMap;

use torqa_osm::Road;

use crate::gpx::RawPoint;

/// Points further than this from any road are off road and keep their position.
const MAX_SNAP_M: f64 = 20.0;
/// Staying on the road matched last is preferred, so the track does not hop to a parallel
/// road or into side streets at junctions: its distance counts this much less.
const SAME_ROAD_BIAS: f64 = 0.6;
/// Spatial index cell size.
const CELL_M: f64 = 50.0;
/// A road detour between two points much longer than the straight line is not what was ridden
/// (e.g. a loop of the road between them): then the points are joined straight.
const MAX_DETOUR: f64 = 2.5;

const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// A flat projection around the track, accurate to a fraction of a metre over a route.
struct Flat {
    lat0: f64,
    lon0: f64,
    cos_lat0: f64,
}

impl Flat {
    fn new(lat0: f64, lon0: f64) -> Self {
        Self {
            lat0,
            lon0,
            cos_lat0: lat0.to_radians().cos(),
        }
    }

    fn to_xy(&self, lat: f64, lon: f64) -> (f64, f64) {
        (
            (lon - self.lon0).to_radians() * self.cos_lat0 * EARTH_RADIUS_M,
            (lat - self.lat0).to_radians() * EARTH_RADIUS_M,
        )
    }

    fn to_lat_lon(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.lat0 + (y / EARTH_RADIUS_M).to_degrees(),
            self.lon0 + (x / (EARTH_RADIUS_M * self.cos_lat0)).to_degrees(),
        )
    }
}

/// Where a point lies on a road: road, segment and position along that segment (0–1).
#[derive(Debug, Clone, Copy)]
struct Match {
    road: usize,
    segment: usize,
    along: f64,
    at: (f64, f64),
}

/// The track with its points on the roads they ride, and the roads' bends between them.
pub(crate) fn to_roads(track: &[RawPoint], roads: &[Road]) -> Vec<RawPoint> {
    let Some(first) = track.first() else {
        return Vec::new();
    };
    if roads.is_empty() {
        return smooth_off_road(track, &vec![None; track.len()]);
    }
    let flat = Flat::new(first.lat, first.lon);
    let lines: Vec<Vec<(f64, f64)>> = roads
        .iter()
        .map(|r| {
            r.line
                .iter()
                .map(|&(lat, lon)| flat.to_xy(lat, lon))
                .collect()
        })
        .collect();
    let index = Index::new(&lines);

    let mut matches: Vec<Option<Match>> = Vec::with_capacity(track.len());
    let mut last_road = None;
    for point in track {
        let found = index.nearest(&lines, flat.to_xy(point.lat, point.lon), last_road);
        last_road = found.map(|m| m.road).or(last_road);
        matches.push(found);
    }

    let mut out: Vec<RawPoint> = Vec::with_capacity(track.len() * 2);
    let smoothed = smooth_off_road(track, &matches);
    for i in 0..track.len() {
        if i > 0
            && let (Some(from), Some(to)) = (matches[i - 1], matches[i])
        {
            add_bends(&mut out, &flat, &lines, &track[i - 1], &track[i], from, to);
        }
        let mut point = smoothed[i];
        if let Some(m) = matches[i] {
            (point.lat, point.lon) = flat.to_lat_lon(m.at);
        }
        out.push(point);
    }
    out
}

/// The road's vertices between two matched points on the same road, with elevation and time
/// interpolated between theirs.
fn add_bends(
    out: &mut Vec<RawPoint>,
    flat: &Flat,
    lines: &[Vec<(f64, f64)>],
    a: &RawPoint,
    b: &RawPoint,
    from: Match,
    to: Match,
) {
    if from.road != to.road {
        return;
    }
    let line = &lines[from.road];
    // Vertices strictly between the two positions, in the direction of travel.
    let between: Vec<(f64, f64)> = if (to.segment, to.along) >= (from.segment, from.along) {
        (from.segment + 1..=to.segment).map(|v| line[v]).collect()
    } else {
        (to.segment + 1..=from.segment)
            .rev()
            .map(|v| line[v])
            .collect()
    };
    if between.is_empty() {
        return;
    }
    let mut path = vec![from.at];
    path.extend(&between);
    path.push(to.at);
    let lengths: Vec<f64> = path
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .collect();
    let total: f64 = lengths.iter().sum();
    let straight = (to.at.0 - from.at.0).hypot(to.at.1 - from.at.1);
    if total > straight.max(1.0) * MAX_DETOUR {
        return;
    }
    let mut done = 0.0;
    for (vertex, length) in between.iter().zip(&lengths) {
        done += length;
        let share = if total > 0.0 { done / total } else { 0.0 };
        let (lat, lon) = flat.to_lat_lon(*vertex);
        out.push(RawPoint {
            lat,
            lon,
            elevation: lerp_option(a.elevation, b.elevation, share),
            time: lerp_option(a.time, b.time, share),
        });
    }
}

fn lerp_option(a: Option<f64>, b: Option<f64>, share: f64) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + (b - a) * share),
        (a, b) => a.or(b),
    }
}

/// Off-road points (not matched) averaged with their neighbours, to take out GPS jitter; matched
/// points and the ends are left alone.
fn smooth_off_road(track: &[RawPoint], matches: &[Option<Match>]) -> Vec<RawPoint> {
    let mut out = track.to_vec();
    for i in 1..track.len().saturating_sub(1) {
        if matches[i].is_none() {
            out[i].lat = (track[i - 1].lat + 2.0 * track[i].lat + track[i + 1].lat) / 4.0;
            out[i].lon = (track[i - 1].lon + 2.0 * track[i].lon + track[i + 1].lon) / 4.0;
        }
    }
    out
}

/// Road segments by grid cell.
struct Index {
    cells: HashMap<(i64, i64), Vec<(usize, usize)>>,
}

impl Index {
    fn new(lines: &[Vec<(f64, f64)>]) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        for (road, line) in lines.iter().enumerate() {
            for segment in 0..line.len().saturating_sub(1) {
                let (a, b) = (line[segment], line[segment + 1]);
                let (x0, x1) = (cell(a.0.min(b.0)), cell(a.0.max(b.0)));
                let (y0, y1) = (cell(a.1.min(b.1)), cell(a.1.max(b.1)));
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        cells.entry((x, y)).or_default().push((road, segment));
                    }
                }
            }
        }
        Self { cells }
    }

    /// The closest point on a road within reach, preferring `last_road`.
    fn nearest(
        &self,
        lines: &[Vec<(f64, f64)>],
        point: (f64, f64),
        last_road: Option<usize>,
    ) -> Option<Match> {
        let (cx, cy) = (cell(point.0), cell(point.1));
        let mut best: Option<(f64, Match)> = None;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &(road, segment) in self.cells.get(&(x, y)).into_iter().flatten() {
                    let line = &lines[road];
                    let (at, along) = project(point, line[segment], line[segment + 1]);
                    let distance = (at.0 - point.0).hypot(at.1 - point.1);
                    if distance > MAX_SNAP_M {
                        continue;
                    }
                    let score = if Some(road) == last_road {
                        distance * SAME_ROAD_BIAS
                    } else {
                        distance
                    };
                    if best.as_ref().is_none_or(|(s, _)| score < *s) {
                        best = Some((
                            score,
                            Match {
                                road,
                                segment,
                                along,
                                at,
                            },
                        ));
                    }
                }
            }
        }
        best.map(|(_, m)| m)
    }
}

#[allow(clippy::cast_possible_truncation)] // local metres over a route stay far below 2^63 cells
fn cell(metres: f64) -> i64 {
    (metres / CELL_M).floor() as i64
}

/// The point on segment `a`–`b` closest to `p`, and how far along the segment it is.
fn project(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> ((f64, f64), f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.0 + dx * t, a.1 + dy * t), t)
}

#[cfg(test)]
mod tests {
    use torqa_osm::RoadClass;

    use super::*;

    const ORIGIN: (f64, f64) = (46.0, 7.0);

    fn flat() -> Flat {
        Flat::new(ORIGIN.0, ORIGIN.1)
    }

    fn point(x: f64, y: f64) -> RawPoint {
        let (lat, lon) = flat().to_lat_lon((x, y));
        RawPoint {
            lat,
            lon,
            elevation: Some(500.0),
            time: None,
        }
    }

    fn road(points: &[(f64, f64)]) -> Road {
        Road {
            class: RoadClass::Street,
            line: points.iter().map(|&p| flat().to_lat_lon(p)).collect(),
            structure: None,
        }
    }

    fn xy(track: &[RawPoint]) -> Vec<(f64, f64)> {
        track.iter().map(|p| flat().to_xy(p.lat, p.lon)).collect()
    }

    fn length(points: &[(f64, f64)]) -> f64 {
        points
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum()
    }

    #[test]
    fn gps_wandering_beside_the_road_is_put_on_it() {
        let straight = road(&[(0.0, -10.0), (0.0, 510.0)]);
        // ±6 m either side, every 20 m.
        let track: Vec<RawPoint> = (0..=25)
            .map(|i| point(if i % 2 == 0 { 6.0 } else { -6.0 }, f64::from(i) * 20.0))
            .collect();

        let snapped = xy(&to_roads(&track, &[straight]));

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
        assert!((length(&snapped) - 500.0).abs() < 0.1);
    }

    #[test]
    fn sparse_points_follow_the_roads_bends_between_them() {
        // A half circle of 100 m radius drawn every 5°, ridden with a point every 60°.
        let arc = |degrees: f64| {
            let r = degrees.to_radians();
            (100.0 * r.cos(), 100.0 * r.sin())
        };
        let bend = road(
            &(0..=36)
                .map(|i| arc(f64::from(i) * 5.0))
                .collect::<Vec<_>>(),
        );
        let track: Vec<RawPoint> = (0..=3)
            .map(|i| {
                let (x, y) = arc(f64::from(i) * 60.0);
                point(x, y)
            })
            .collect();

        let snapped = xy(&to_roads(&track, &[bend]));

        // Along the curve (π × 100 m), not its chords (300 m), every point on it.
        assert!((length(&snapped) - std::f64::consts::PI * 100.0).abs() < 1.0);
        for (x, y) in &snapped {
            assert!((x.hypot(*y) - 100.0).abs() < 0.5, "off the road: {x}, {y}");
        }
    }

    #[test]
    fn off_road_stretches_keep_their_course() {
        let far_road = road(&[(200.0, 0.0), (200.0, 500.0)]);
        let track: Vec<RawPoint> = (0..=10).map(|i| point(0.0, f64::from(i) * 50.0)).collect();

        let snapped = xy(&to_roads(&track, &[far_road]));

        assert_eq!(snapped.len(), track.len());
        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01));
    }

    #[test]
    fn drifting_towards_a_parallel_road_does_not_hop_onto_it() {
        let ridden = road(&[(0.0, -10.0), (0.0, 310.0)]);
        let parallel = road(&[(10.0, -10.0), (10.0, 310.0)]);
        // Closer to the ridden road at first, then a little closer to the other.
        let track: Vec<RawPoint> = (0..=15)
            .map(|i| point(if i < 5 { 3.0 } else { 5.8 }, f64::from(i) * 20.0))
            .collect();

        let snapped = xy(&to_roads(&track, &[ridden, parallel]));

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
    }
}
