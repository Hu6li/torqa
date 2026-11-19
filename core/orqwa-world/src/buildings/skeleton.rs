//! The straight skeleton of a building's outline: where a roof rising at the same pitch from
//! every wall has its hips, valleys and ridges (ADR 0041, step 4). Each wall's edge of the
//! outline moves inwards at the same speed; corners follow the bisectors between their edges,
//! an edge whose corners meet is gone, and a reflex corner reaching another edge splits the
//! outline in two. The region each edge sweeps is its face of the roof, and how far an edge
//! has moved is the roof's height there over the pitch.
//!
//! Every step recomputes the next event from all corners, which is plenty for the tens of
//! corners of a building. The faces are checked against the outline before they are used:
//! they must cover it exactly, every point at its distance from its edge. Where they do not
//! (outlines too irregular for the floating-point steps), there is no skeleton and the roof
//! falls back to another shape.

use super::shape::{Point, signed_area};

/// Outlines with more corners than this have no skeleton: their roofs fall back.
const MAX_CORNERS: usize = 256;
/// Tolerance of positions and heights, relative to the outline's size.
const EPSILON: f64 = 1e-8;
/// How far corners are moved apart, relative to the outline's size.
const JITTER: f64 = 1e-4;

/// The roof faces over an outline's edges.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Skeleton {
    /// The face over edge `i` (from corner `i` to the next), counter-clockwise: the edge's two
    /// ends, then the skeleton's points over it back to the first, each with how far it lies
    /// from the edge.
    pub(crate) faces: Vec<Vec<(Point, f64)>>,
    /// The greatest such distance: the roof's height over its pitch, at the ridge.
    pub(crate) height: f64,
}

#[derive(Debug, Clone)]
struct Corner {
    /// Where the corner started, and when (how far its edges had moved).
    at: Point,
    time: f64,
    /// The edges before and after it.
    left: usize,
    right: usize,
    prev: usize,
    next: usize,
    alive: bool,
    /// How it moves per unit of time, if its edges are not parallel and opposite.
    velocity: Option<Point>,
    /// The way it moves: along its bisector, or along its edges when they are opposite.
    direction: Point,
    reflex: bool,
}

struct Edge {
    from: Point,
    along: Point,
    inward: Point,
}

impl Edge {
    fn distance(&self, p: Point) -> f64 {
        dot(sub(p, self.from), self.inward)
    }
}

enum Event {
    /// Corners `a` and `a`'s next meet at a point: the edge between them is gone.
    Edge { a: usize, at: Point },
    /// Reflex corner `corner` reaches the edge after `left`, which it splits.
    Split {
        corner: usize,
        left: usize,
        at: Point,
    },
}

/// The straight skeleton of a counter-clockwise `outline`, if it can be found and checked.
///
/// Building outlines are full of corners whose events coincide (a T's two inner corners meet
/// its ridges at one point at one time). The sweep runs on the outline moved by a hair, a
/// ten-thousandth of its size, different for each corner, which puts such events one after
/// another, where the outline as it is fails; a few such moves are tried. The faces then end
/// at the outline's own corners.
pub(crate) fn skeleton(outline: &[Point]) -> Option<Skeleton> {
    let count = outline.len();
    if !(3..=MAX_CORNERS).contains(&count) || signed_area(outline) <= 0.0 {
        return None;
    }
    // Around the first corner: world coordinates run to tens of kilometres.
    let origin = outline[0];
    let local: Vec<Point> = outline.iter().map(|&p| sub(p, origin)).collect();
    let size = local
        .iter()
        .flat_map(|&(e, n)| [e.abs(), n.abs()])
        .fold(1.0, f64::max);
    for attempt in 0..4_u32 {
        let moved: Vec<Point> = local
            .iter()
            .enumerate()
            .map(|(i, &(e, n))| {
                #[allow(clippy::cast_precision_loss)] // a few dozen corners
                let k = (i as f64 + 1.0) * (f64::from(attempt) + 0.7);
                // The outline as it is first, then moved.
                let hair = if attempt == 0 { 0.0 } else { JITTER * size };
                (
                    e + hair * (k * 12.9898).sin(),
                    n + hair * (k * 78.233).cos(),
                )
            })
            .collect();
        if let Some(mut found) = sweep(&moved, size) {
            for (i, face) in found.faces.iter_mut().enumerate() {
                face[0].0 = local[i];
                face[1].0 = local[(i + 1) % count];
                for (p, _) in face.iter_mut() {
                    *p = (p.0 + origin.0, p.1 + origin.1);
                }
            }
            return Some(found);
        }
    }
    None
}

/// The skeleton of `outline`, local and moved off coinciding events, `size` its extent.
#[allow(clippy::too_many_lines)] // one sweep: corners, events and the faces they leave
fn sweep(outline: &[Point], size: f64) -> Option<Skeleton> {
    let count = outline.len();
    let eps = EPSILON * size;
    let edges: Vec<Edge> = (0..count)
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % count]);
            let along = normalize(sub(b, a))?;
            Some(Edge {
                from: a,
                along,
                inward: (-along.1, along.0),
            })
        })
        .collect::<Option<_>>()?;
    let mut corners: Vec<Corner> = (0..count)
        .map(|i| {
            let left = (i + count - 1) % count;
            corner(&edges, outline[i], 0.0, left, i, left, (i + 1) % count)
        })
        .collect();
    for (i, c) in corners.iter_mut().enumerate() {
        c.prev = (i + count - 1) % count;
        c.next = (i + 1) % count;
    }
    let mut points: Vec<Vec<(Point, f64)>> = vec![Vec::new(); count];
    let mut now = 0.0_f64;
    let mut steps = 0;
    while corners.iter().any(|c| c.alive) {
        steps += 1;
        if steps > 4 * count * count {
            return None;
        }
        // A loop of two corners is a ridge between two edges: it ends where they are now.
        if let Some(i) = (0..corners.len()).find(|&i| {
            let c = &corners[i];
            c.alive && c.next == c.prev
        }) {
            let j = corners[i].next;
            for k in [i, j] {
                let c = &corners[k];
                let at = position(c, now);
                points[c.left].push((at, now));
                points[c.right].push((at, now));
            }
            corners[i].alive = false;
            corners[j].alive = false;
            continue;
        }
        let (time, event) = next_event(&corners, &edges, now, eps)?;
        now = time.max(now);
        match event {
            Event::Edge { a, at } => {
                let b = corners[a].next;
                let (left, middle, right) = (corners[a].left, corners[a].right, corners[b].right);
                for edge in [left, middle, right] {
                    points[edge].push((at, now));
                }
                let (prev, next) = (corners[a].prev, corners[b].next);
                corners[a].alive = false;
                corners[b].alive = false;
                if prev == b {
                    // The last two corners of a loop met.
                    continue;
                }
                if prev == next {
                    // A triangle closing: its third corner reaches the same point.
                    let third = &corners[prev];
                    for edge in [third.left, third.right] {
                        points[edge].push((at, now));
                    }
                    corners[prev].alive = false;
                    continue;
                }
                let new = corner(&edges, at, now, left, right, prev, next);
                let index = corners.len();
                corners.push(new);
                corners[prev].next = index;
                corners[next].prev = index;
            }
            Event::Split {
                corner: v,
                left,
                at,
            } => {
                let edge = corners[left].right;
                let right = corners[left].next;
                let (v_left, v_right) = (corners[v].left, corners[v].right);
                for e in [v_left, v_right, edge] {
                    points[e].push((at, now));
                }
                let (v_prev, v_next) = (corners[v].prev, corners[v].next);
                corners[v].alive = false;
                let first = corners.len();
                corners.push(corner(&edges, at, now, v_left, edge, v_prev, right));
                corners.push(corner(&edges, at, now, edge, v_right, left, v_next));
                corners[v_prev].next = first;
                corners[right].prev = first;
                corners[left].next = first + 1;
                corners[v_next].prev = first + 1;
            }
        }
    }
    let mut faces = Vec::with_capacity(count);
    for (i, edge) in edges.iter().enumerate() {
        let end = outline[(i + 1) % count];
        let length = dot(sub(end, edge.from), edge.along);
        let mut upper: Vec<(Point, f64)> = Vec::new();
        for &(p, h) in &points[i] {
            if h <= eps {
                continue;
            }
            if upper
                .iter()
                .any(|&(q, _)| (q.0 - p.0).abs() < eps * 10.0 && (q.1 - p.1).abs() < eps * 10.0)
            {
                continue;
            }
            upper.push((p, h));
        }
        upper.sort_by(|a, b| {
            let (pa, pb) = (
                dot(sub(a.0, edge.from), edge.along),
                dot(sub(b.0, edge.from), edge.along),
            );
            pb.total_cmp(&pa)
        });
        let mut face = vec![(edge.from, 0.0), (end, 0.0)];
        face.extend(upper);
        if length <= 0.0 {
            return None;
        }
        faces.push(face);
    }
    let height = faces.iter().flatten().map(|&(_, h)| h).fold(0.0, f64::max);
    let skeleton = Skeleton { faces, height };
    checked(&skeleton, &edges, outline, eps).then_some(skeleton)
}

/// A corner at `at`, `time`, between edges `left` and `right`.
fn corner(
    edges: &[Edge],
    at: Point,
    time: f64,
    left: usize,
    right: usize,
    prev: usize,
    next: usize,
) -> Corner {
    let (l, r) = (&edges[left], &edges[right]);
    let denominator = 1.0 + dot(l.inward, r.inward);
    let velocity = (denominator > 1e-9).then(|| {
        let sum = (l.inward.0 + r.inward.0, l.inward.1 + r.inward.1);
        (sum.0 / denominator, sum.1 / denominator)
    });
    let direction = velocity.and_then(normalize).unwrap_or(r.along);
    Corner {
        at,
        time,
        left,
        right,
        prev,
        next,
        alive: true,
        velocity,
        direction,
        reflex: cross(l.along, r.along) < -1e-9,
    }
}

/// Where a corner is at `time`.
fn position(corner: &Corner, time: f64) -> Point {
    match corner.velocity {
        Some(v) => {
            let dt = time - corner.time;
            (corner.at.0 + v.0 * dt, corner.at.1 + v.1 * dt)
        }
        None => corner.at,
    }
}

/// The earliest event from `now` on, and when.
fn next_event(corners: &[Corner], edges: &[Edge], now: f64, eps: f64) -> Option<(f64, Event)> {
    let mut best: Option<(f64, Event)> = None;
    let offer = |time: f64, event: Event, best: &mut Option<(f64, Event)>| {
        if time >= now - eps && best.as_ref().is_none_or(|(t, _)| time < *t - eps * 0.01) {
            *best = Some((time, event));
        }
    };
    for (i, c) in corners.iter().enumerate() {
        if !c.alive {
            continue;
        }
        let n = &corners[c.next];
        if let Some(at) = rays(c.at, c.direction, n.at, n.direction, eps) {
            let time = edges[c.right].distance(at);
            offer(time, Event::Edge { a: i, at }, &mut best);
        }
        let Some(v) = c.velocity.filter(|_| c.reflex) else {
            continue;
        };
        let mut l = c.next;
        while l != c.prev {
            let left = &corners[l];
            let current = l;
            l = left.next;
            let edge_index = left.right;
            if edge_index == c.left || edge_index == c.right {
                continue;
            }
            let edge = &edges[edge_index];
            let closing = 1.0 - dot(v, edge.inward);
            if closing <= 1e-9 {
                continue;
            }
            let time = (edge.distance(c.at) - dot(v, edge.inward) * c.time) / closing;
            if time < c.time - eps {
                continue;
            }
            let at = position(c, time);
            let right = &corners[left.next];
            let (from, to) = (position(left, time), position(right, time));
            if dot(sub(at, from), edge.along) < -eps || dot(sub(at, to), edge.along) > eps {
                continue;
            }
            offer(
                time,
                Event::Split {
                    corner: i,
                    left: current,
                    at,
                },
                &mut best,
            );
        }
    }
    best
}

/// Where rays from `a` along `da` and from `b` along `db` meet, both forward.
fn rays(a: Point, da: Point, b: Point, db: Point, eps: f64) -> Option<Point> {
    let denominator = cross(da, db);
    let gap = sub(b, a);
    if denominator.abs() < 1e-12 {
        // Along one line towards each other, or the same point.
        let apart = (gap.0 * gap.0 + gap.1 * gap.1).sqrt();
        if apart < eps {
            return Some(a);
        }
        return (cross(gap, da).abs() < eps && dot(gap, da) > 0.0 && dot(gap, db) < 0.0)
            .then_some((f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1)));
    }
    let s = cross(gap, db) / denominator;
    let u = cross(gap, da) / denominator;
    (s >= -eps && u >= -eps).then_some((a.0 + s * da.0, a.1 + s * da.1))
}

/// Whether the faces cover the outline once, every point at its distance from its edge.
fn checked(skeleton: &Skeleton, edges: &[Edge], outline: &[Point], eps: f64) -> bool {
    let area = signed_area(outline);
    let mut covered = 0.0;
    for (face, edge) in skeleton.faces.iter().zip(edges) {
        let ring: Vec<Point> = face.iter().map(|&(p, _)| p).collect();
        let face_area = signed_area(&ring);
        if face_area < -eps {
            return false;
        }
        covered += face_area;
        if face
            .iter()
            .any(|&(p, h)| (edge.distance(p) - h).abs() > eps * 1e4)
        {
            return false;
        }
    }
    (covered - area).abs() <= area * 1e-6 + eps
}

fn sub(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}

fn dot(a: Point, b: Point) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: Point, b: Point) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn normalize(a: Point) -> Option<Point> {
    let length = a.0.hypot(a.1);
    (length > 1e-12).then(|| (a.0 / length, a.1 / length))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights(skeleton: &Skeleton) -> Vec<f64> {
        let mut all: Vec<f64> = skeleton.faces.iter().flatten().map(|&(_, h)| h).collect();
        all.sort_by(f64::total_cmp);
        all.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        all
    }

    #[test]
    fn a_rectangle_has_two_hips_at_each_end_and_a_ridge() {
        let rect = [(0.0, 0.0), (12.0, 0.0), (12.0, 8.0), (0.0, 8.0)];
        let s = skeleton(&rect).unwrap();
        assert!((s.height - 4.0).abs() < 1e-6);
        // The long sides' faces are trapezoids reaching the ridge from 4 m to 8 m east.
        assert_eq!(s.faces[0].len(), 4);
        assert_eq!(s.faces[1].len(), 3);
        let ridge: Vec<Point> = s.faces[0][2..].iter().map(|&(p, _)| p).collect();
        assert!(
            ridge
                .iter()
                .any(|p| (p.0 - 8.0).abs() < 1e-6 && (p.1 - 4.0).abs() < 1e-6)
        );
        assert!(
            ridge
                .iter()
                .any(|p| (p.0 - 4.0).abs() < 1e-6 && (p.1 - 4.0).abs() < 1e-6)
        );
    }

    #[test]
    fn a_square_has_a_pyramid() {
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let s = skeleton(&square).unwrap();
        assert!(s.faces.iter().all(|f| f.len() == 3));
        assert_eq!(heights(&s), [0.0, 5.0]);
    }

    #[test]
    fn an_l_shape_has_a_valley_where_its_wings_meet() {
        // A 20 × 8 m main body with an 8 m wide wing to 16 m north at its west end.
        let l = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 8.0),
            (8.0, 8.0),
            (8.0, 16.0),
            (0.0, 16.0),
        ];
        let s = skeleton(&l).unwrap();
        assert!((s.height - 4.0).abs() < 1e-6);
        // The reflex corner's valley runs up to the ridges at (4, 4)–(8, 4)... at 4 m.
        let valley = s.faces[2].iter().chain(&s.faces[3]).any(|&(p, h)| {
            (p.0 - 4.0).abs() < 1e-6 && (p.1 - 4.0).abs() < 1e-6 && (h - 4.0).abs() < 1e-6
        });
        assert!(valley, "{:?}", s.faces);
    }

    #[test]
    fn t_and_u_shapes_and_bays_are_covered_exactly() {
        let shapes: [&[Point]; 4] = [
            // T
            &[
                (0.0, 0.0),
                (24.0, 0.0),
                (24.0, 8.0),
                (16.0, 8.0),
                (16.0, 18.0),
                (8.0, 18.0),
                (8.0, 8.0),
                (0.0, 8.0),
            ],
            // U
            &[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 16.0),
                (14.0, 16.0),
                (14.0, 6.0),
                (6.0, 6.0),
                (6.0, 16.0),
                (0.0, 16.0),
            ],
            // A house with a bay on its front
            &[
                (0.0, 0.0),
                (4.0, 0.0),
                (4.0, -1.5),
                (8.0, -1.5),
                (8.0, 0.0),
                (12.0, 0.0),
                (12.0, 9.0),
                (0.0, 9.0),
            ],
            // A skewed, irregular outline
            &[
                (0.0, 0.0),
                (15.0, 1.0),
                (17.0, 9.0),
                (9.0, 8.0),
                (7.0, 13.0),
                (-1.0, 11.0),
            ],
        ];
        for shape in shapes {
            let s = skeleton(shape).unwrap_or_else(|| panic!("no skeleton for {shape:?}"));
            assert_eq!(s.faces.len(), shape.len());
            assert!(s.height > 0.0);
        }
    }

    #[test]
    fn clockwise_or_degenerate_outlines_have_none() {
        assert!(skeleton(&[(0.0, 0.0), (0.0, 5.0), (5.0, 5.0), (5.0, 0.0)]).is_none());
        assert!(skeleton(&[(0.0, 0.0), (5.0, 0.0)]).is_none());
    }
}
