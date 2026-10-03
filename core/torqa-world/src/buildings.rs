//! Buildings extruded from OpenStreetMap footprints.

use torqa_osm::Building;
use torqa_routes::LocalProjection;

use crate::structures::{quad_uv, triangle_uv};
use crate::{HeightGrid, MeshData, hash};

/// Height of a storey in metres.
const STOREY: f64 = 3.0;
/// Walls reach this far below the lowest ground point, so slopes never show a gap.
const FOUNDATION: f64 = 1.0;

/// Roof pitch of gabled houses.
const ROOF_PITCH_DEGREES: f64 = 35.0;
/// Houses up to this footprint and height get a gable roof; larger buildings stay flat.
const GABLE_MAX_AREA: f64 = 450.0;
const GABLE_MAX_HEIGHT: f64 = 14.0;

/// Façades: cream, beige, sand, white, light grey, ochre (sRGB; alpha 0 marks walls).
const WALL_COLORS: [[f32; 4]; 6] = [
    [0.93, 0.89, 0.80, 0.0],
    [0.88, 0.83, 0.74, 0.0],
    [0.84, 0.76, 0.63, 0.0],
    [0.94, 0.93, 0.90, 0.0],
    [0.78, 0.78, 0.76, 0.0],
    [0.86, 0.72, 0.55, 0.0],
];
/// Roofs: terracotta, brown, anthracite, slate (alpha 1 marks roofs for the shader).
const ROOF_COLORS: [[f32; 4]; 4] = [
    [0.60, 0.29, 0.20, 1.0],
    [0.44, 0.27, 0.20, 1.0],
    [0.24, 0.25, 0.27, 1.0],
    [0.36, 0.37, 0.40, 1.0],
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
    let at = |(e, n): (f64, f64), y: f64| [e - origin[0], y - origin[1], -n - origin[2]];
    // Wall texture coordinates: metres along the outline and above the ground, for windows.
    let uv = |along: f64, y: f64| [along as f32, (y - ground) as f32];

    let mut along = 0.0;
    for i in 0..footprint.len() {
        let (a, b) = (footprint[i], footprint[(i + 1) % footprint.len()]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length < 0.01 {
            continue;
        }
        // Counter-clockwise outline: the outside is to the right of travel.
        let normal = [(b.1 - a.1) / length, 0.0, (b.0 - a.0) / length];
        quad_uv(
            mesh,
            [at(a, bottom), at(b, bottom), at(b, top), at(a, top)],
            [
                uv(along, bottom),
                uv(along + length, bottom),
                uv(along + length, top),
                uv(along, top),
            ],
            normal,
            wall_color,
        );
        along += length;
    }

    if footprint.len() == 4 && area < GABLE_MAX_AREA && height <= GABLE_MAX_HEIGHT {
        gable_roof(mesh, footprint, top, ground, wall_color, roof_color, &at);
    } else {
        flat_roof(mesh, footprint, top, roof_color, &at);
    }
}

#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
fn flat_roof(
    mesh: &mut MeshData,
    footprint: &[(f64, f64)],
    top: f64,
    color: [f32; 4],
    at: &impl Fn((f64, f64), f64) -> [f64; 3],
) {
    let base = index(mesh);
    for &point in footprint {
        mesh.vertices.push(at(point, top).map(|v| v as f32));
        mesh.normals.push([0.0, 1.0, 0.0]);
        mesh.uvs.push([0.0, 0.0]);
        mesh.colors.push(color);
    }
    for [a, b, c] in triangulate(footprint) {
        // Counter-clockwise seen from above; Godot's front faces are clockwise.
        mesh.indices.extend([base + c, base + b, base + a]);
    }
}

/// A pitched roof over a four-cornered house, its ridge along the longer side, with
/// triangular gable walls at the short ends.
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn gable_roof(
    mesh: &mut MeshData,
    footprint: &[(f64, f64)],
    eaves: f64,
    ground: f64,
    wall_color: [f32; 4],
    roof_color: [f32; 4],
    at: &impl Fn((f64, f64), f64) -> [f64; 3],
) {
    let length = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    // Rotate so the edges p0–p1 and p2–p3 are the short (gable) ends.
    let start = usize::from(
        length(footprint[0], footprint[1]) + length(footprint[2], footprint[3])
            > length(footprint[1], footprint[2]) + length(footprint[3], footprint[0]),
    );
    let p = [0, 1, 2, 3].map(|k| footprint[(k + start) % 4]);
    let middle = |a: (f64, f64), b: (f64, f64)| (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
    let (ridge_a, ridge_b) = (middle(p[0], p[1]), middle(p[2], p[3]));
    let half_width = (length(p[0], p[1]) + length(p[2], p[3])) / 4.0;
    let ridge = eaves + half_width * ROOF_PITCH_DEGREES.to_radians().tan();

    // Slopes over the long sides p1–p2 and p3–p0, facing out and up.
    for (from, to, near, far) in [
        (p[1], p[2], ridge_a, ridge_b),
        (p[3], p[0], ridge_b, ridge_a),
    ] {
        let corners = [
            at(from, eaves),
            at(to, eaves),
            at(far, ridge),
            at(near, ridge),
        ];
        let normal = upward_normal(&corners);
        quad_uv(mesh, corners, [[0.0; 2]; 4], normal, roof_color);
    }
    // Gable walls at the short ends, with window coordinates like the walls below.
    for (from, to, peak) in [(p[0], p[1], ridge_a), (p[2], p[3], ridge_b)] {
        let edge = length(from, to);
        let normal = [(to.1 - from.1) / edge, 0.0, (to.0 - from.0) / edge];
        let uv = |along: f64, y: f64| [along as f32, (y - ground) as f32];
        triangle_uv(
            mesh,
            [at(from, eaves), at(to, eaves), at(peak, ridge)],
            [uv(0.0, eaves), uv(edge, eaves), uv(edge / 2.0, ridge)],
            normal,
            wall_color,
        );
    }
}

/// The normal of a planar polygon, oriented upwards.
fn upward_normal(corners: &[[f64; 3]]) -> [f64; 3] {
    let (first, second, third) = (corners[0], corners[1], corners[2]);
    let edge_1 = [0, 1, 2].map(|k| second[k] - first[k]);
    let edge_2 = [0, 1, 2].map(|k| third[k] - first[k]);
    let normal = [
        edge_1[1] * edge_2[2] - edge_1[2] * edge_2[1],
        edge_1[2] * edge_2[0] - edge_1[0] * edge_2[2],
        edge_1[0] * edge_2[1] - edge_1[1] * edge_2[0],
    ];
    if normal[1] < 0.0 {
        normal.map(|x| -x)
    } else {
        normal
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
