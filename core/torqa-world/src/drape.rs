//! Laying strips on the ground: streets and streams are cut along the ground's own triangles,
//! so each piece lies in the plane of the triangle under it and the ground can never show
//! through, however it folds.

use crate::{HeightGrid, MeshData};

/// Draped, a straight strip needs no points between its bends but these, at most this far
/// apart; the ground's triangles add the rest...
const PIECE_M: f64 = 30.0;
/// ...and bends this sharp (radians) keep their points.
const TURN: f64 = 0.03;
/// Bridge decks are this thick at their edges.
const DECK_DEPTH_M: f64 = 0.7;
/// Vertices per point of a deck: left and right edge, and each side's top and bottom.
const DECK_POINTS: usize = 6;
/// Piers stand under decks this far apart, at the same places along the line in every chunk,
/// where the ground lies at least `DECK_PIER_MIN` below the deck.
const DECK_PIER_SPACING: f64 = 20.0;
const DECK_PIER_MIN: f64 = 2.0;
/// A deck's piers are this long, and as wide as this share of the deck.
const DECK_PIER_HALF_LENGTH: f64 = 0.6;
const DECK_PIER_WIDTH: f64 = 0.7;

/// Points along `line` at most `step` metres apart.
pub(crate) fn densify(line: &[(f64, f64)], step: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for pair in line.windows(2) {
        let ((e0, n0), (e1, n1)) = (pair[0], pair[1]);
        let length = (e1 - e0).hypot(n1 - n0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short segments
        let steps = (length / step).ceil().max(1.0) as usize;
        for k in 0..steps {
            #[allow(clippy::cast_precision_loss)]
            let t = k as f64 / steps as f64;
            points.push((e0 + (e1 - e0) * t, n0 + (n1 - n0) * t));
        }
    }
    points.extend(line.last());
    points
}

/// A line cut exactly at the edges of the square `low`–`high` (a chunk), so the neighbour's
/// piece starts where this one ends: the pieces inside, as points with their distance along
/// the whole line.
pub(crate) fn pieces(
    points: &[(f64, f64)],
    low: (f64, f64),
    high: (f64, f64),
) -> Vec<Vec<((f64, f64), f64)>> {
    let mut pieces: Vec<Vec<((f64, f64), f64)>> = Vec::new();
    let mut piece: Vec<((f64, f64), f64)> = Vec::new();
    let mut travelled = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if let Some((t0, t1)) = clip(a, b, low, high) {
            let start = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
            let end = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
            if piece
                .last()
                .is_none_or(|&(p, _)| (p.0 - start.0).hypot(p.1 - start.1) > 1e-6)
            {
                pieces.push(std::mem::take(&mut piece));
                piece.push((start, travelled + length * t0));
            }
            piece.push((end, travelled + length * t1));
        } else {
            pieces.push(std::mem::take(&mut piece));
        }
        travelled += length;
    }
    pieces.push(piece);
    pieces.retain(|p| p.len() >= 2);
    pieces
}

/// A strip of `half` width along `run`, `lift` above the ground, cut along the ground's
/// triangles: each piece lies in the plane of the triangle under it, so the ground can never
/// rise through the strip, however it folds.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
pub(crate) fn drape(
    mesh: &mut MeshData,
    run: &[((f64, f64), f64)],
    half: f64,
    lift: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    let run = straightened(run);
    if run.len() < 2 {
        return;
    }
    // The street's edges at each point, square to it there.
    let edges: Vec<((f64, f64), (f64, f64))> = (0..run.len())
        .map(|i| {
            let (before, after) = (
                run[i.saturating_sub(1)].0,
                run[(i + 1).min(run.len() - 1)].0,
            );
            let (de, dn) = (after.0 - before.0, after.1 - before.1);
            let length = de.hypot(dn).max(1e-6);
            // Right of travel is the direction turned clockwise by 90°.
            let (re, rn) = (dn / length, -de / length);
            let (east, north) = run[i].0;
            (
                (east - re * half, north - rn * half),
                (east + re * half, north + rn * half),
            )
        })
        .collect();
    for i in 0..run.len() - 1 {
        let ((left_0, right_0), (left_1, right_1)) = (edges[i], edges[i + 1]);
        // Clockwise seen from above, as the ground's triangles.
        let quad = [left_0, left_1, right_1, right_0];
        let ((start, distance_a), (end, distance_b)) = (run[i], run[i + 1]);
        let length = (end.0 - start.0).hypot(end.1 - start.1).max(1e-6);
        let forward = ((end.0 - start.0) / length, (end.1 - start.1) / length);
        let uv = |p: (f64, f64)| {
            let (e, n) = (p.0 - start.0, p.1 - start.1);
            let along = (e * forward.0 + n * forward.1).clamp(0.0, length);
            let across = e * forward.1 - n * forward.0;
            [
                (0.5 + across / (2.0 * half)) as f32,
                (distance_a + (distance_b - distance_a) * along / length) as f32,
            ]
        };
        drape_polygon(mesh, &quad, lift, heights, origin, &uv);
    }
}

/// A convex `polygon` (corners in metres east/north, clockwise seen from above) laid on the
/// ground `lift` above it: cut along the ground's triangles, each piece in the plane of the
/// triangle under it. `uv` gives each corner's texture coordinates.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
pub(crate) fn drape_polygon(
    mesh: &mut MeshData,
    polygon: &[(f64, f64)],
    lift: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
    uv: &dyn Fn((f64, f64)) -> [f32; 2],
) {
    let low = polygon
        .iter()
        .fold((f64::MAX, f64::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
    let high = polygon
        .iter()
        .fold((f64::MIN, f64::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
    for triangle in heights.triangles(low, high) {
        let piece = inside_triangle(polygon, &triangle);
        if piece.len() < 3 {
            continue;
        }
        let normal = plane_normal(&triangle, heights);
        let base = u32::try_from(mesh.vertices.len()).expect("draped meshes fit u32");
        for &(east, north) in &piece {
            mesh.vertices.push([
                (east - origin[0]) as f32,
                (heights.at(east, north) + lift - origin[1]) as f32,
                (-north - origin[2]) as f32,
            ]);
            mesh.normals.push(normal);
            mesh.uvs.push(uv((east, north)));
        }
        for k in 1..piece.len() - 1 {
            let (first, second, third) = (piece[0], piece[k], piece[k + 1]);
            let area = (second.0 - first.0) * (third.1 - first.1)
                - (second.1 - first.1) * (third.0 - first.0);
            // Slivers along an edge add nothing.
            if area.abs() > 1e-6 {
                let corner = base + u32::try_from(k).expect("small");
                mesh.indices.extend([base, corner, corner + 1]);
            }
        }
    }
}

/// `run` without the points where the strip goes straight on (see `PIECE_M`).
fn straightened(run: &[((f64, f64), f64)]) -> Vec<((f64, f64), f64)> {
    let Some((&first, rest)) = run.split_first() else {
        return Vec::new();
    };
    let mut kept = vec![first];
    for (i, &point) in rest.iter().enumerate() {
        let Some(&(next, _)) = rest.get(i + 1) else {
            kept.push(point);
            break;
        };
        let (from, here) = (kept[kept.len() - 1].0, point.0);
        let turn = ((here.0 - from.0).atan2(here.1 - from.1)
            - (next.0 - here.0).atan2(next.1 - here.1)
            + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if turn.abs() > TURN || (next.0 - from.0).hypot(next.1 - from.1) > PIECE_M {
            kept.push(point);
        }
    }
    kept
}

/// The part of polygon `subject` inside `triangle`, both clockwise seen from above
/// (Sutherland–Hodgman).
fn inside_triangle(subject: &[(f64, f64)], triangle: &[(f64, f64); 3]) -> Vec<(f64, f64)> {
    let mut polygon = subject.to_vec();
    for k in 0..3 {
        let (a, b) = (triangle[k], triangle[(k + 1) % 3]);
        // Clockwise: the inside lies to the right of each edge.
        let side = |p: (f64, f64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        let input = std::mem::take(&mut polygon);
        for (i, &current) in input.iter().enumerate() {
            let previous = input[(i + input.len() - 1) % input.len()];
            let (now, before) = (side(current), side(previous));
            let crossing = |from: (f64, f64), to: (f64, f64)| {
                let t = before / (before - now);
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            };
            if now <= 0.0 {
                if before > 0.0 {
                    polygon.push(crossing(previous, current));
                }
                polygon.push(current);
            } else if before <= 0.0 {
                polygon.push(crossing(previous, current));
            }
        }
        if polygon.is_empty() {
            break;
        }
    }
    polygon
}

/// The upward normal of the ground's triangle, as the mesh would light it.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn plane_normal(triangle: &[(f64, f64); 3], heights: &HeightGrid) -> [f32; 3] {
    // Just inside the corners, so each height comes from this very triangle.
    let middle = (
        (triangle[0].0 + triangle[1].0 + triangle[2].0) / 3.0,
        (triangle[0].1 + triangle[1].1 + triangle[2].1) / 3.0,
    );
    let point = |p: (f64, f64)| {
        let q = (p.0 + (middle.0 - p.0) * 0.01, p.1 + (middle.1 - p.1) * 0.01);
        [q.0, heights.at(q.0, q.1), -q.1]
    };
    let corners = triangle.map(point);
    let edge = |k: usize| {
        [
            corners[k][0] - corners[0][0],
            corners[k][1] - corners[0][1],
            corners[k][2] - corners[0][2],
        ]
    };
    let (first, second) = (edge(1), edge(2));
    let normal = [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ];
    let n = if normal[1] < 0.0 {
        normal.map(|x| -x)
    } else {
        normal
    };
    normalize(n).map(|x| x as f32)
}

/// The part of segment `a`–`b` inside the rectangle `low`–`high`, as positions along it
/// (0–1), if any (Liang–Barsky).
fn clip(a: (f64, f64), b: (f64, f64), low: (f64, f64), high: (f64, f64)) -> Option<(f64, f64)> {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-de, a.0 - low.0),
        (de, high.0 - a.0),
        (-dn, a.1 - low.1),
        (dn, high.1 - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
}

fn normalize([x, y, z]: [f64; 3]) -> [f64; 3] {
    let length = (x * x + y * y + z * z).sqrt().max(1e-9);
    [x / length, y / length, z / length]
}

/// A bridge deck of `half` width along `run` (points with their distance along a line `total`
/// long): straight between the heights of its ends, `lift` above them, with its sides.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
#[allow(clippy::too_many_arguments)]
pub(crate) fn deck(
    mesh: &mut MeshData,
    run: &[((f64, f64), f64)],
    half: f64,
    (start, end): (f64, f64),
    lift: f64,
    total: f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    if run.len() < 2 {
        return;
    }
    let top_at = |distance: f64| start + (end - start) * (distance / total.max(1e-6)) + lift;
    deck_piers(mesh, run, half, &top_at, heights, origin);
    let base = u32::try_from(mesh.vertices.len()).expect("streets fit u32");
    for (i, &((east, north), distance)) in run.iter().enumerate() {
        let (before, after) = (
            run[i.saturating_sub(1)].0,
            run[(i + 1).min(run.len() - 1)].0,
        );
        let (de, dn) = (after.0 - before.0, after.1 - before.1);
        let length = de.hypot(dn).max(1e-6);
        // Right of travel is the direction turned clockwise by 90°.
        let (re, rn) = (dn / length, -de / length);
        let top = top_at(distance);
        let mut push = |side: f64, height: f64, normal: [f64; 3], u: f32| {
            let (e, n) = (east + re * half * side, north + rn * half * side);
            mesh.vertices.push([
                (e - origin[0]) as f32,
                (height - origin[1]) as f32,
                (-n - origin[2]) as f32,
            ]);
            mesh.normals.push(normal.map(|c| c as f32));
            mesh.uvs.push([u, distance as f32]);
        };
        // The deck's edges, then each side's top and bottom with their own normals.
        push(-1.0, top, [0.0, 1.0, 0.0], 0.0);
        push(1.0, top, [0.0, 1.0, 0.0], 1.0);
        for (side, u) in [(-1.0, 0.0), (1.0, 1.0)] {
            let out = [re * side, 0.0, -rn * side];
            push(side, top, out, u);
            push(side, top - DECK_DEPTH_M, out, u);
        }
        if i > 0 {
            let step = u32::try_from(DECK_POINTS).expect("small");
            let at = base + u32::try_from(i * DECK_POINTS).expect("streets fit u32");
            let previous = at - step;
            let (left_0, right_0, left_1, right_1) = (previous, previous + 1, at, at + 1);
            // The sides facing out.
            let (side_left_0, bottom_left_0) = (previous + 2, previous + 3);
            let (side_right_0, bottom_right_0) = (previous + 4, previous + 5);
            let (side_left_1, bottom_left_1) = (at + 2, at + 3);
            let (side_right_1, bottom_right_1) = (at + 4, at + 5);
            mesh.indices.extend([
                left_0,
                left_1,
                right_1,
                left_0,
                right_1,
                right_0,
                side_left_0,
                bottom_left_0,
                bottom_left_1,
                side_left_0,
                bottom_left_1,
                side_left_1,
                side_right_0,
                side_right_1,
                bottom_right_1,
                side_right_0,
                bottom_right_1,
                bottom_right_0,
            ]);
        }
    }
}

/// Piers under a deck along `run`, down to the ground wherever it lies low enough. Their
/// texture coordinates are those of the deck's edge (`u` 0), so they take its plain colour.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn deck_piers(
    mesh: &mut MeshData,
    run: &[((f64, f64), f64)],
    half: f64,
    top_at: &dyn Fn(f64) -> f64,
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    let (first, last) = (run[0].1, run[run.len() - 1].1);
    let mut along = ((first / DECK_PIER_SPACING).floor() + 0.5) * DECK_PIER_SPACING;
    while along <= last {
        let segment = run
            .windows(2)
            .position(|w| w[0].1 <= along && along <= w[1].1);
        if let Some(index) = segment.filter(|_| along >= first) {
            let ((from, from_d), (to, to_d)) = (run[index], run[index + 1]);
            let share = ((along - from_d) / (to_d - from_d).max(1e-9)).clamp(0.0, 1.0);
            let (east, north) = (
                from.0 + (to.0 - from.0) * share,
                from.1 + (to.1 - from.1) * share,
            );
            let length = (to.0 - from.0).hypot(to.1 - from.1).max(1e-9);
            let ahead = ((to.0 - from.0) / length, (to.1 - from.1) / length);
            let top = top_at(along) - DECK_DEPTH_M;
            let ground = heights.at(east, north);
            if top - ground >= DECK_PIER_MIN {
                let right = (ahead.1, -ahead.0);
                let (long, wide) = (DECK_PIER_HALF_LENGTH, half * DECK_PIER_WIDTH);
                let corner = |forth: f64, aside: f64, height: f64| {
                    [
                        east + ahead.0 * forth + right.0 * aside - origin[0],
                        height - origin[1],
                        -(north + ahead.1 * forth + right.1 * aside) - origin[2],
                    ]
                };
                for ([(x0, y0), (x1, y1)], (ne, nn)) in [
                    ([(long, -wide), (long, wide)], ahead),
                    ([(-long, wide), (-long, -wide)], (-ahead.0, -ahead.1)),
                    ([(-long, wide), (long, wide)], right),
                    ([(long, -wide), (-long, -wide)], (-right.0, -right.1)),
                ] {
                    let corners = [
                        corner(x0, y0, ground - 0.5),
                        corner(x1, y1, ground - 0.5),
                        corner(x1, y1, top),
                        corner(x0, y0, top),
                    ];
                    upright(mesh, corners, [ne, 0.0, -nn], along as f32);
                }
            }
        }
        along += DECK_PIER_SPACING;
    }
}

/// An upright quad (corners around its edge) facing `normal`, wound clockwise seen from that
/// side as Godot's front faces are, with texture coordinates `u` 0 and `v` as given.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
fn upright(mesh: &mut MeshData, corners: [[f64; 3]; 4], normal: [f64; 3], v: f32) {
    let edge = |k: usize| [0, 1, 2].map(|c| corners[k][c] - corners[0][c]);
    let (first, second) = (edge(1), edge(2));
    let cross = [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ];
    // Clockwise seen from the front: the right-hand normal points away from the viewer.
    let order = if cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0 {
        [0, 3, 2, 1]
    } else {
        [0, 1, 2, 3]
    };
    let base = u32::try_from(mesh.vertices.len()).expect("streets fit u32");
    let length = normal[0].hypot(normal[2]).max(1e-9);
    for k in order {
        mesh.vertices.push(corners[k].map(|c| c as f32));
        mesh.normals.push(normal.map(|c| (c / length) as f32));
        mesh.uvs.push([0.0, v]);
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}
