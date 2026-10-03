//! Bridges (deck, parapets, pillars) and tunnels (an arched tube) along the road.

use torqa_routes::{ElevationModel, LocalProjection, Surface};

use crate::MeshData;
use crate::road::{CentrePoint, RoadIndex};

const CONCRETE: [f32; 4] = [0.62, 0.61, 0.58, 0.0];
const TUNNEL_WALL: [f32; 4] = [0.38, 0.37, 0.35, 0.0];

/// Half the deck width: the road plus a narrow kerb.
const DECK_HALF_WIDTH: f64 = 3.6;
const DECK_THICKNESS: f64 = 1.2;
const PARAPET_HEIGHT: f64 = 1.0;
const PARAPET_THICKNESS: f64 = 0.3;
const PILLAR_SPACING: f64 = 30.0;
const PILLAR_HALF_SIZE: f64 = 0.8;
/// Pillars only where the ground is at least this far below the deck.
const MIN_PILLAR_HEIGHT: f64 = 2.0;
const TUNNEL_RADIUS: f64 = 5.0;
const ARCH_SEGMENTS: usize = 12;

/// Geometry of all bridges and tunnels, in route coordinates.
pub(crate) async fn build<M: ElevationModel>(
    road: &RoadIndex,
    projection: &LocalProjection,
    model: &mut M,
) -> MeshData {
    let mut mesh = MeshData::default();
    for (surface, run) in road.structure_runs() {
        match surface {
            Surface::Bridge => bridge(&mut mesh, &run, projection, model).await,
            Surface::Tunnel => tunnel(&mut mesh, &run),
            Surface::Ground => {}
        }
    }
    mesh
}

async fn bridge<M: ElevationModel>(
    mesh: &mut MeshData,
    run: &[CentrePoint],
    projection: &LocalProjection,
    model: &mut M,
) {
    for pair in run.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        for side in [-1.0, 1.0] {
            let outer = DECK_HALF_WIDTH * side;
            let inner = (DECK_HALF_WIDTH - PARAPET_THICKNESS) * side;
            let outward = right(a).map(|v| v * side);
            // Deck side, from the road surface down.
            wall(
                mesh,
                a,
                b,
                outer,
                -DECK_THICKNESS,
                PARAPET_HEIGHT,
                outward,
                CONCRETE,
            );
            // Parapet: inner face and top.
            wall(
                mesh,
                a,
                b,
                inner,
                0.0,
                PARAPET_HEIGHT,
                outward.map(|v| -v),
                CONCRETE,
            );
            flat(mesh, a, b, inner, outer, PARAPET_HEIGHT, 1.0, CONCRETE);
        }
        flat(
            mesh,
            a,
            b,
            -DECK_HALF_WIDTH,
            DECK_HALF_WIDTH,
            -DECK_THICKNESS,
            -1.0,
            CONCRETE,
        );
    }

    let mut travelled = PILLAR_SPACING / 2.0;
    let mut next = PILLAR_SPACING;
    for pair in run.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.position.0 - a.position.0).hypot(b.position.1 - a.position.1);
        while next <= travelled + length {
            let t = (next - travelled) / length;
            let east = a.position.0 + (b.position.0 - a.position.0) * t;
            let north = a.position.1 + (b.position.1 - a.position.1) * t;
            let deck = a.elevation + (b.elevation - a.elevation) * t - DECK_THICKNESS;
            let (lat, lon) = projection.unproject(east, north);
            if let Ok(ground) = model.elevation(lat, lon).await
                && deck - ground >= MIN_PILLAR_HEIGHT
            {
                pillar(mesh, (east, north), ground - 1.0, deck, a.direction);
            }
            next += PILLAR_SPACING;
        }
        travelled += length;
    }
}

fn tunnel(mesh: &mut MeshData, run: &[CentrePoint]) {
    for pair in run.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        for k in 0..ARCH_SEGMENTS {
            #[allow(clippy::cast_precision_loss)] // small segment counts
            let angle = |k: usize| std::f64::consts::PI * k as f64 / ARCH_SEGMENTS as f64;
            let (start, end) = (angle(k), angle(k + 1));
            let ring = |p: CentrePoint, angle: f64| {
                let (sin, cos) = angle.sin_cos();
                offset(p, TUNNEL_RADIUS * cos, TUNNEL_RADIUS * sin)
            };
            let corners = [ring(a, start), ring(b, start), ring(b, end), ring(a, end)];
            let middle = f64::midpoint(start, end);
            let (sin, cos) = middle.sin_cos();
            let right = right(a);
            // Facing the axis, seen from inside the tunnel...
            let inward = [-right[0] * cos, -sin, -right[2] * cos];
            quad(mesh, corners, inward, TUNNEL_WALL);
            // ...and the outside, visible at the portals.
            quad(mesh, corners, inward.map(|v| -v), CONCRETE);
        }
    }
}

/// A vertical strip along the road at `across` metres right of the centre line, from `low` to
/// `high` relative to the road surface.
#[allow(clippy::too_many_arguments)]
fn wall(
    mesh: &mut MeshData,
    a: CentrePoint,
    b: CentrePoint,
    across: f64,
    low: f64,
    high: f64,
    normal: [f64; 3],
    color: [f32; 4],
) {
    let corners = [
        offset(a, across, low),
        offset(b, across, low),
        offset(b, across, high),
        offset(a, across, high),
    ];
    quad(mesh, corners, normal, color);
}

/// A horizontal strip between `left` and `right` metres across, at `height` above the road,
/// facing up (`facing` 1) or down (−1).
#[allow(clippy::too_many_arguments)]
fn flat(
    mesh: &mut MeshData,
    a: CentrePoint,
    b: CentrePoint,
    left: f64,
    right: f64,
    height: f64,
    facing: f64,
    color: [f32; 4],
) {
    let corners = [
        offset(a, left, height),
        offset(b, left, height),
        offset(b, right, height),
        offset(a, right, height),
    ];
    quad(mesh, corners, [0.0, facing, 0.0], color);
}

fn pillar(
    mesh: &mut MeshData,
    (east, north): (f64, f64),
    bottom: f64,
    top: f64,
    direction: (f64, f64),
) {
    let (de, dn) = direction;
    let (re, rn) = (dn, -de);
    let s = PILLAR_HALF_SIZE;
    let corner = |along: f64, across: f64, height: f64| {
        [
            east + de * along + re * across,
            height,
            -(north + dn * along + rn * across),
        ]
    };
    for (along, across, normal) in [
        (s, 0.0, [de, 0.0, -dn]),
        (-s, 0.0, [-de, 0.0, dn]),
        (0.0, s, [re, 0.0, -rn]),
        (0.0, -s, [-re, 0.0, rn]),
    ] {
        // The face's two corners span perpendicular to its normal.
        let (u, v) = if along == 0.0 { (s, 0.0) } else { (0.0, s) };
        let corners = [
            corner(along - u, across - v, bottom),
            corner(along + u, across + v, bottom),
            corner(along + u, across + v, top),
            corner(along - u, across - v, top),
        ];
        quad(mesh, corners, normal, CONCRETE);
    }
}

/// Unit vector to the right of travel, in Godot coordinates.
fn right(p: CentrePoint) -> [f64; 3] {
    let (de, dn) = p.direction;
    [dn, 0.0, de]
}

/// A point `across` metres right of the centre line and `up` metres above the road surface,
/// in Godot coordinates.
fn offset(p: CentrePoint, across: f64, up: f64) -> [f64; 3] {
    let (de, dn) = p.direction;
    let (re, rn) = (dn, -de);
    [
        p.position.0 + re * across,
        p.elevation + up,
        -(p.position.1 + rn * across),
    ]
}

/// Adds a quad with corners in order around its edge, wound to face `normal` in Godot's
/// clockwise front-face order.
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
pub(crate) fn quad(mesh: &mut MeshData, corners: [[f64; 3]; 4], normal: [f64; 3], color: [f32; 4]) {
    let [first, second, third, _] = corners;
    let edge_1 = [0, 1, 2].map(|k| second[k] - first[k]);
    let edge_2 = [0, 1, 2].map(|k| third[k] - first[k]);
    let cross = [
        edge_1[1] * edge_2[2] - edge_1[2] * edge_2[1],
        edge_1[2] * edge_2[0] - edge_1[0] * edge_2[2],
        edge_1[0] * edge_2[1] - edge_1[1] * edge_2[0],
    ];
    // Clockwise seen from the front means the right-hand normal points away from the viewer.
    let facing_viewer = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0;
    let order = if facing_viewer {
        [0, 3, 2, 1]
    } else {
        [0, 1, 2, 3]
    };
    let base = u32::try_from(mesh.vertices.len()).expect("structure mesh fits u32");
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    let unit = normal.map(|n| (n / length) as f32);
    for i in order {
        mesh.vertices.push(corners[i].map(|v| v as f32));
        mesh.normals.push(unit);
        mesh.uvs.push([0.0, 0.0]);
        mesh.colors.push(color);
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}
