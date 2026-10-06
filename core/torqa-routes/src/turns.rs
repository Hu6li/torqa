//! Turns in place (#101): a planned track sometimes runs a few dozen metres into a side road and
//! straight back out, or zig-zags — back down a road it has just climbed and up it again. A
//! rider would not; riding in 3D, those stretches are taken out, so the rider stays on the road.
//! A real out and back, up a dead end to a summit and down again, stays.

use crate::{EARTH_RADIUS, RoutePoint};

/// A track coming back within this of its own way retraces it...
const RETRACE: f64 = 5.0;
/// ...after turning by more than this (radians, about 100°) over a few points.
const TURN: f64 = 1.75;
/// Turning back from at most this far is turning in place, and so is turning back towards a
/// turn just made, however far.
const SHORT: f64 = 100.0;
/// Turns at most this many points apart are the same.
const NEAR: usize = 2;
/// At most this many stretches are taken out of a route.
const MOST: usize = 100;

/// A stretch taken out of a route: where it was, on the route without it, and how long.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cut {
    pub(crate) at: f64,
    pub(crate) length: f64,
}

/// Takes the turns in place out of `points` (see the module), renumbering their distances; the
/// stretches taken out, in order.
pub(crate) fn straighten(points: &mut Vec<RoutePoint>) -> Vec<Cut> {
    let mut cuts = Vec::new();
    while cuts.len() < MOST {
        let local = metres(points);
        let turns = turns(&local);
        let found = turns.iter().find_map(|&turn| {
            // The turn's sharpest point may lie either side of the point found.
            let (back, apex) = (turn.saturating_sub(1)..=turn + 1)
                .map(|apex| (retrace(&local, apex), apex))
                .max()?;
            if back == 0 || back >= apex || apex + back + 1 >= local.len() {
                return None;
            }
            let (from, to) = (apex - back, apex + back);
            let short = points[apex].distance.0 - points[from].distance.0 <= SHORT;
            // Turning back at the other end too, rather than at a junction: a zig-zag.
            let zigzag = turns.iter().any(|&other| {
                other.abs_diff(turn) > NEAR
                    && (other.abs_diff(from) <= NEAR || other.abs_diff(to) <= NEAR)
                    && retrace(&local, other) > NEAR
            });
            // On from `from` where the track went on from `to`, at the same place...
            let mut next = to + 1;
            while next + 1 < local.len() && distance(local[from], local[next]) < RETRACE {
                next += 1;
            }
            // ...without turning back there: else the track came back another way (the other
            // carriageway of a divided road, round a roundabout), as a rider would.
            let (arriving, leaving) = (
                heading(&local, from - 1, from),
                heading(&local, next, next + 1),
            );
            let onwards = arriving.0 * leaving.0 + arriving.1 * leaving.1;
            ((short || zigzag) && onwards > -0.5).then_some((from, next))
        });
        let Some((from, next)) = found else {
            break;
        };
        let length =
            points[next].distance.0 - points[from].distance.0 - distance(local[from], local[next]);
        for point in &mut points[next..] {
            point.distance.0 -= length;
        }
        points.drain(from + 1..next);
        cuts.push(Cut {
            at: points[from].distance.0,
            length,
        });
    }
    cuts
}

/// The unit direction from point `a` to point `b`.
fn heading(local: &[(f64, f64)], a: usize, b: usize) -> (f64, f64) {
    let (from, to) = (local[a], local[b.min(local.len() - 1)]);
    let length = distance(from, to).max(1e-9);
    ((to.0 - from.0) / length, (to.1 - from.1) / length)
}

/// Where the track turns by more than `TURN`: at each turn the point where it turns most.
fn turns(local: &[(f64, f64)]) -> Vec<usize> {
    let turn = |k: usize| {
        let (a, b, c) = (
            local[k.saturating_sub(NEAR)],
            local[k],
            local[(k + NEAR).min(local.len() - 1)],
        );
        let (first, second) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
        let cross = first.0 * second.1 - first.1 * second.0;
        let dot = first.0 * second.0 + first.1 * second.1;
        cross.atan2(dot).abs()
    };
    let mut apexes: Vec<(usize, f64)> = Vec::new();
    for k in 1..local.len().saturating_sub(1) {
        let angle = turn(k);
        if angle <= TURN {
            continue;
        }
        match apexes.last_mut() {
            Some(last) if k - last.0 <= NEAR => {
                if angle > last.1 {
                    *last = (k, angle);
                }
            }
            _ => apexes.push((k, angle)),
        }
    }
    apexes.into_iter().map(|a| a.0).collect()
}

/// How many points either side of `apex` the track comes back over its own way.
fn retrace(local: &[(f64, f64)], apex: usize) -> usize {
    let mut back = 0;
    for x in 1..=apex {
        if apex + x + 1 >= local.len() {
            break;
        }
        let point = local[apex - x];
        let (low, high) = (
            (apex + x).saturating_sub(NEAR).max(apex),
            (apex + x + NEAR).min(local.len() - 1),
        );
        let on_its_way = (low..high).any(|j| to_segment(point, local[j], local[j + 1]) < RETRACE);
        if !on_its_way {
            break;
        }
        back = x;
    }
    back
}

/// The points in metres east and north of the first (flat, fine over a route's extent).
fn metres(points: &[RoutePoint]) -> Vec<(f64, f64)> {
    let Some(first) = points.first() else {
        return Vec::new();
    };
    let scale = EARTH_RADIUS.to_radians();
    points
        .iter()
        .map(|p| {
            (
                (p.lon - first.lon) * scale * first.lat.to_radians().cos(),
                (p.lat - first.lat) * scale,
            )
        })
        .collect()
}

/// Distance from `p` to the segment `a`–`b`.
fn to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let length_squared = de * de + dn * dn;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * de + (p.1 - a.1) * dn) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(p, (a.0 + de * t, a.1 + dn * t))
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}
