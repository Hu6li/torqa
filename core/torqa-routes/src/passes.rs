//! Where a route rides the same road again — back the way it came, round a loop a second time
//! or across its own track — that road is one, at one height (#101). Elevations are sampled and
//! smoothed along the route, so its passes would differ there: a file's heights may lie metres
//! apart, and smoothing lifts the bottom of a turn and lowers its top. The road would be drawn
//! twice, one pass above the other. Later passes take the heights of the first instead, easing
//! into and out of them.

use std::collections::HashMap;

use crate::gpx::RawPoint;
use crate::{EARTH_RADIUS, Surface};

/// A point this close to an earlier pass rides the same road...
const SAME_ROAD: f64 = 3.0;
/// ...if it comes back the other way (as after turning round), or this far further on along the
/// route (closer, in the same direction, it is the same pass going round a bend).
const APART: f64 = 40.0;
/// Heights ease into and out of a pass over at least this distance, at most this much steeper.
const EASING: f64 = 40.0;
const EASING_GRADE: f64 = 0.03;
/// Index cell size; larger than `SAME_ROAD` and the points' spacing.
const CELL: f64 = 20.0;

/// Gives the route's later passes over a road the `heights` of its first. Passes on different
/// structures (a bridge over the road ridden earlier) keep their own.
pub(crate) fn join(points: &[RawPoint], surfaces: &[Surface], heights: &mut [f64]) {
    let Some(first) = points.first() else {
        return;
    };
    let metres = EARTH_RADIUS.to_radians();
    let local: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            (
                (p.lon - first.lon) * metres * first.lat.to_radians().cos(),
                (p.lat - first.lat) * metres,
            )
        })
        .collect();
    let mut along = vec![0.0];
    for pair in local.windows(2) {
        along.push(along[along.len() - 1] + distance(pair[0], pair[1]));
    }
    let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for k in 0..local.len().saturating_sub(1) {
        for point in [local[k], local[k + 1]] {
            let list = cells.entry(cell(point)).or_default();
            if list.last() != Some(&k) {
                list.push(k);
            }
        }
    }
    // Twice: the second time, passes meet the first as eased in by the first time.
    for _ in 0..2 {
        let mut offsets = vec![None; points.len()];
        for i in 0..local.len() {
            let Some((k, t)) = earlier_pass(i, &local, &along, surfaces, &cells) else {
                continue;
            };
            let target = heights[k] + (heights[k + 1] - heights[k]) * t;
            offsets[i] = Some(target - heights[i]);
            heights[i] = target;
        }
        ease(heights, &along, &offsets);
    }
}

/// The segment (and how far along it) of the first earlier pass over the road at point `i`.
fn earlier_pass(
    i: usize,
    local: &[(f64, f64)],
    along: &[f64],
    surfaces: &[Surface],
    cells: &HashMap<(i64, i64), Vec<usize>>,
) -> Option<(usize, f64)> {
    let point = local[i];
    let heading = direction(local, i);
    let (ce, cn) = cell(point);
    let mut best: Option<(usize, f64)> = None;
    for x in ce - 1..=ce + 1 {
        for y in cn - 1..=cn + 1 {
            for &k in cells.get(&(x, y)).into_iter().flatten() {
                if k + 1 >= i || best.is_some_and(|b| b.0 <= k) {
                    continue;
                }
                if surfaces[k] != surfaces[i] || surfaces[k + 1] != surfaces[i] {
                    continue;
                }
                let (a, b) = (local[k], local[k + 1]);
                let (gap, t) = to_segment(point, a, b);
                let length = distance(a, b).max(1e-9);
                let way = ((b.0 - a.0) / length, (b.1 - a.1) / length);
                let back = way.0 * heading.0 + way.1 * heading.1 < -0.7;
                if gap < SAME_ROAD && (back || along[i] - along[k + 1] >= APART) {
                    best = Some((k, t));
                }
            }
        }
    }
    best
}

/// Eases the heights round the points moved onto an earlier pass (`offsets`, how far each
/// moved) into those either side: between two such points closer than their easing, straight
/// from one move to the other; else fading out from each.
fn ease(heights: &mut [f64], along: &[f64], offsets: &[Option<f64>]) {
    let reach = |offset: f64| EASING.max(offset.abs() / EASING_GRADE);
    let mut previous = vec![None; offsets.len()];
    let mut last = None;
    for k in 0..offsets.len() {
        previous[k] = last;
        if offsets[k].is_some() {
            last = Some(k);
        }
    }
    let mut next = None;
    for k in (0..offsets.len()).rev() {
        if offsets[k].is_some() {
            next = Some(k);
            continue;
        }
        let side = |j: Option<usize>| j.and_then(|j| offsets[j].map(|o| (j, o)));
        heights[k] += match (side(previous[k]), side(next)) {
            (Some((p, before)), Some((q, after)))
                if along[q] - along[p] <= reach(before) + reach(after) =>
            {
                before + (after - before) * (along[k] - along[p]) / (along[q] - along[p])
            }
            (before, after) => {
                let fade = |(j, offset): (usize, f64)| {
                    offset * (1.0 - (along[k] - along[j]).abs() / reach(offset)).max(0.0)
                };
                before.map_or(0.0, fade) + after.map_or(0.0, fade)
            }
        };
    }
}

/// Unit direction of travel at point `i`.
fn direction(local: &[(f64, f64)], i: usize) -> (f64, f64) {
    let (a, b) = if i + 1 < local.len() {
        (local[i], local[i + 1])
    } else {
        (local[i.saturating_sub(1)], local[i])
    };
    let length = distance(a, b).max(1e-9);
    ((b.0 - a.0) / length, (b.1 - a.1) / length)
}

/// Distance from `p` to the segment `a`–`b`, and how far along it the nearest point lies (0–1).
fn to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let length_squared = de * de + dn * dn;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * de + (p.1 - a.1) * dn) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (distance(p, (a.0 + de * t, a.1 + dn * t)), t)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell((east, north): (f64, f64)) -> (i64, i64) {
    ((east / CELL).floor() as i64, (north / CELL).floor() as i64)
}
