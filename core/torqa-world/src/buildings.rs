//! Buildings extruded from OpenStreetMap footprints.

use torqa_osm::Building;
use torqa_routes::LocalProjection;

use crate::{HeightGrid, MeshData, hash};

/// Height of a storey in metres.
const STOREY: f64 = 3.0;
/// Walls reach this far below the lowest ground point, so slopes never show a gap.
const FOUNDATION: f64 = 1.0;

const WALL_COLORS: [[f32; 4]; 4] = [
    [0.86, 0.83, 0.76, 0.0],
    [0.80, 0.78, 0.74, 0.0],
    [0.90, 0.88, 0.84, 0.0],
    [0.74, 0.68, 0.60, 0.0],
];
const ROOF_COLORS: [[f32; 4]; 3] = [
    [0.45, 0.22, 0.18, 0.0],
    [0.32, 0.32, 0.34, 0.0],
    [0.52, 0.30, 0.22, 0.0],
];

/// Footprint in metres east/north, counter-clockwise, without the closing point.
pub(crate) fn footprint(building: &Building, projection: &LocalProjection) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = building
        .outline
        .iter()
        .map(|&(lat, lon)| projection.project(lat, lon))
        .collect();
    points.pop(); // closing point
    points.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 0.05);
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    points
}

/// Adds the walls and flat roof of a building standing on `heights`.
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
pub(crate) fn add(
    mesh: &mut MeshData,
    building: &Building,
    footprint: &[(f64, f64)],
    heights: &HeightGrid,
    origin: [f64; 3],
) {
    if footprint.len() < 3 {
        return;
    }
    let ground = footprint
        .iter()
        .map(|&(e, n)| heights.at(e, n))
        .fold(f64::INFINITY, f64::min);
    let area = signed_area(footprint);
    let variation = hash(building.id);
    let height = building
        .height
        .or_else(|| building.levels.map(|levels| levels * STOREY + 1.0))
        // Untagged: houses are low, large footprints are halls or blocks.
        .unwrap_or((if area > 600.0 { 10.0 } else { 6.0 }) + variation * 4.0);
    let (bottom, top) = (ground - FOUNDATION, ground + height);
    // `hash` lies in [0, 1), so the products are small non-negative indices.
    #[allow(clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let (wall, roof) = (
        (hash(building.id ^ 0x77) * WALL_COLORS.len() as f64) as usize,
        (hash(building.id ^ 0x99) * ROOF_COLORS.len() as f64) as usize,
    );
    let (wall_color, roof_color) = (WALL_COLORS[wall], ROOF_COLORS[roof]);
    let local = |e: f64, y: f64, n: f64| {
        [
            (e - origin[0]) as f32,
            (y - origin[1]) as f32,
            (-n - origin[2]) as f32,
        ]
    };

    for i in 0..footprint.len() {
        let (a, b) = (footprint[i], footprint[(i + 1) % footprint.len()]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length < 0.01 {
            continue;
        }
        // Counter-clockwise outline: the outside is to the right of travel.
        let normal = [
            ((b.1 - a.1) / length) as f32,
            0.0,
            ((b.0 - a.0) / length) as f32,
        ];
        let base = index(mesh);
        for (e, n, y) in [
            (a.0, a.1, bottom),
            (a.0, a.1, top),
            (b.0, b.1, top),
            (b.0, b.1, bottom),
        ] {
            mesh.vertices.push(local(e, y, n));
            mesh.normals.push(normal);
            mesh.uvs.push([0.0, 0.0]);
            mesh.colors.push(wall_color);
        }
        // Clockwise seen from outside, Godot's front-face order.
        mesh.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let base = index(mesh);
    for &(e, n) in footprint {
        mesh.vertices.push(local(e, top, n));
        mesh.normals.push([0.0, 1.0, 0.0]);
        mesh.uvs.push([0.0, 0.0]);
        mesh.colors.push(roof_color);
    }
    for [a, b, c] in triangulate(footprint) {
        // Counter-clockwise seen from above; Godot's front faces are clockwise.
        mesh.indices.extend([base + c, base + b, base + a]);
    }
}

fn index(mesh: &MeshData) -> u32 {
    u32::try_from(mesh.vertices.len()).expect("chunk mesh fits u32")
}

/// Twice the signed area is avoided: this is the true area, positive for counter-clockwise.
pub(crate) fn signed_area(points: &[(f64, f64)]) -> f64 {
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        sum += a.0 * b.1 - b.0 * a.1;
    }
    sum / 2.0
}

/// Ear-clipping triangulation of a simple counter-clockwise polygon (vertex indices).
pub(crate) fn triangulate(points: &[(f64, f64)]) -> Vec<[u32; 3]> {
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut triangles = Vec::new();
    let cross = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let mut guard = 0;
    while remaining.len() > 3 && guard < points.len() * points.len() {
        guard += 1;
        let count = remaining.len();
        let ear = (0..count).find(|&i| {
            let (prev, cur, next) = (
                remaining[(i + count - 1) % count],
                remaining[i],
                remaining[(i + 1) % count],
            );
            let (a, b, c) = (points[prev], points[cur], points[next]);
            if cross(a, b, c) <= 0.0 {
                return false; // reflex or degenerate corner
            }
            remaining.iter().all(|&other| {
                other == prev
                    || other == cur
                    || other == next
                    || !(cross(a, b, points[other]) > 0.0
                        && cross(b, c, points[other]) > 0.0
                        && cross(c, a, points[other]) > 0.0)
            })
        });
        // Self-intersecting or degenerate outlines have no ear left; drop the worst corner.
        let i = ear.unwrap_or(0);
        let (prev, cur, next) = (
            remaining[(i + count - 1) % count],
            remaining[i],
            remaining[(i + 1) % count],
        );
        if ear.is_some() {
            triangles.push([prev, cur, next].map(|v| u32::try_from(v).expect("small polygon")));
        }
        remaining.remove(i);
    }
    if remaining.len() == 3 {
        triangles.push(
            [remaining[0], remaining[1], remaining[2]]
                .map(|v| u32::try_from(v).expect("small polygon")),
        );
    }
    triangles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_concave_outlines() {
        // An L-shaped house: 6 corners, 4 triangles, covering 300 m².
        let l_shape = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, 20.0),
            (0.0, 20.0),
        ];

        let triangles = triangulate(&l_shape);

        assert_eq!(triangles.len(), 4);
        let area: f64 = triangles
            .iter()
            .map(|t| signed_area(&t.map(|i| l_shape[i as usize])))
            .sum();
        assert!((area - 300.0).abs() < 1e-9, "{area}");
    }
}
