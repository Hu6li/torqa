//! Bridges and tunnels along the road. Short, low bridges are stone arch bridges (#75): arched
//! openings between piers, the walls over the arches and the vaults under them; longer and
//! higher ones are viaducts, their deck on piers as wide as itself. Tunnels are an arched tube.

use std::sync::LazyLock;

use torqa_routes::{ElevationModel, LocalProjection, Surface};

use crate::road::{CentrePoint, RoadIndex};
use crate::{MeshData, palette};

static CONCRETE: LazyLock<[f32; 4]> = LazyLock::new(|| palette::srgb("structure.concrete", 0.0));
static STONE: LazyLock<[f32; 4]> = LazyLock::new(|| palette::srgb("structure.stone", 0.0));
static TUNNEL_WALL: LazyLock<[f32; 4]> = LazyLock::new(|| palette::srgb("structure.tunnel", 0.0));

/// Half the deck width: the road plus a narrow kerb.
const DECK_HALF_WIDTH: f64 = 3.6;
const DECK_THICKNESS: f64 = 1.2;
const PARAPET_HEIGHT: f64 = 1.0;
const PARAPET_THICKNESS: f64 = 0.3;
const PILLAR_SPACING: f64 = 30.0;
/// Viaduct piers: this thick along the deck, and as wide as the deck less this on each side.
const PIER_HALF_LENGTH: f64 = 1.0;
const PIER_INSET: f64 = 0.4;
/// Pillars only where the ground is at least this far below the deck.
const MIN_PILLAR_HEIGHT: f64 = 2.0;
/// Bridges at most this long and high are stone arch bridges...
const ARCH_BRIDGE_LENGTH: f64 = 60.0;
const ARCH_BRIDGE_HEIGHT: f64 = 15.0;
/// ...with arches about this wide between piers this thick...
const ARCH_SPAN: f64 = 14.0;
const ARCH_PIER: f64 = 2.0;
/// ...their crowns this far under the deck, and drawn in this many pieces.
const ARCH_CROWN: f64 = 0.5;
const ARCH_PIECES: usize = 10;
/// The ground under a bridge is looked at this often.
const GROUND_STEP: f64 = 2.0;
/// Piers and walls reach this far below the natural ground: channels may cut it (`channels`).
const FOOTING: f64 = 2.5;
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
    let path = Path::new(run);
    // The ground under the bridge, every few metres.
    let mut ground = Vec::new();
    let mut along = 0.0;
    while along <= path.length {
        let point = path.at(along);
        let (lat, lon) = projection.unproject(point.position.0, point.position.1);
        ground.push(model.elevation(lat, lon).await.ok());
        along += GROUND_STEP;
    }
    let ground_at = |along: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short bridges
        let k = ((along / GROUND_STEP).round() as usize).min(ground.len().saturating_sub(1));
        ground.get(k).copied().flatten()
    };
    let height = (0..ground.len())
        .filter_map(|k| {
            #[allow(clippy::cast_precision_loss)] // a few hundred samples
            let along = k as f64 * GROUND_STEP;
            ground_at(along).map(|g| path.at(along).elevation - DECK_THICKNESS - g)
        })
        .fold(0.0, f64::max);
    let arched = path.length <= ARCH_BRIDGE_LENGTH
        && (MIN_PILLAR_HEIGHT..=ARCH_BRIDGE_HEIGHT).contains(&height);
    let color = if arched { *STONE } else { *CONCRETE };
    deck(mesh, run, color);
    if arched {
        arches(mesh, &path, &ground_at);
    } else {
        piers(mesh, &path, &ground_at);
    }
}

/// The deck: its sides from below the road up to the parapets, the parapets' inner faces and
/// tops, and its underside.
fn deck(mesh: &mut MeshData, run: &[CentrePoint], color: [f32; 4]) {
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
                color,
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
                color,
            );
            flat(mesh, a, b, inner, outer, PARAPET_HEIGHT, 1.0, color);
        }
        flat(
            mesh,
            a,
            b,
            -DECK_HALF_WIDTH,
            DECK_HALF_WIDTH,
            -DECK_THICKNESS,
            -1.0,
            color,
        );
    }
}

/// A viaduct's piers: slabs as wide as the deck, down to the ground wherever it lies low
/// enough.
fn piers(mesh: &mut MeshData, path: &Path, ground_at: &dyn Fn(f64) -> Option<f64>) {
    let mut along = PILLAR_SPACING / 2.0;
    while along < path.length {
        let point = path.at(along);
        let deck = point.elevation - DECK_THICKNESS;
        if let Some(ground) = ground_at(along)
            && deck - ground >= MIN_PILLAR_HEIGHT
        {
            pier(
                mesh,
                point,
                (PIER_HALF_LENGTH, DECK_HALF_WIDTH - PIER_INSET),
                (ground - FOOTING, deck),
                *CONCRETE,
            );
        }
        along += PILLAR_SPACING;
    }
}

/// A stone arch bridge's arches: piers between arched openings, the walls over the arches up
/// to the deck, and the vaults under them. Where the ground leaves too little room for an arch,
/// a solid wall stands instead.
fn arches(mesh: &mut MeshData, path: &Path, ground_at: &dyn Fn(f64) -> Option<f64>) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few spans
    let spans = (path.length / ARCH_SPAN).round().max(1.0) as usize;
    #[allow(clippy::cast_precision_loss)] // a few spans
    let span = path.length / spans as f64;
    let half_pier = ARCH_PIER / 2.0;
    let deck_at = |along: f64| path.at(along).elevation - DECK_THICKNESS;
    // The lowest ground along a stretch: piers and walls reach down to it.
    let lowest = |from: f64, to: f64| {
        let mut low = f64::MAX;
        let mut along = from;
        while along <= to + 1e-6 {
            if let Some(g) = ground_at(along) {
                low = low.min(g);
            }
            along += GROUND_STEP;
        }
        (low < f64::MAX).then_some(low - FOOTING)
    };
    // Piers at both ends (abutments, half as thick) and between the spans.
    for k in 0..=spans {
        #[allow(clippy::cast_precision_loss)] // a few spans
        let middle = k as f64 * span;
        let (from, to) = (
            (middle - half_pier).max(0.0),
            (middle + half_pier).min(path.length),
        );
        if let Some(bottom) = lowest(from, to) {
            sides(mesh, path, (from, to), &|_| bottom, &deck_at, *STONE);
        }
    }
    for k in 0..spans {
        #[allow(clippy::cast_precision_loss)] // a few spans
        let (from, to) = (
            k as f64 * span + half_pier,
            (k + 1) as f64 * span - half_pier,
        );
        let Some(floor) = lowest(from, to) else {
            continue;
        };
        opening(mesh, path, (from, to), floor, &deck_at);
    }
}

/// One arched opening of a stone bridge along `(from, to)` over ground at `floor`: the faces
/// of the piers either side up to the arch's springing, the walls over the arch up to the
/// deck, and the vault under it. A semicircle, or flatter where the ground lies higher; a solid
/// wall where there is no room for an arch at all.
fn opening(
    mesh: &mut MeshData,
    path: &Path,
    (from, to): (f64, f64),
    floor: f64,
    deck_at: &dyn Fn(f64) -> f64,
) {
    let middle = f64::midpoint(from, to);
    let radius = (to - from) / 2.0;
    let crown = deck_at(middle) - ARCH_CROWN;
    let rise = radius.min(crown - floor - 1.5);
    if rise < 1.0 {
        sides(mesh, path, (from, to), &|_| floor, deck_at, *STONE);
        return;
    }
    let spring = crown - rise;
    let arch = |t: f64| {
        let angle = std::f64::consts::PI * t;
        (middle - radius * angle.cos(), spring + rise * angle.sin())
    };
    for (at, facing) in [(from, 1.0), (to, -1.0)] {
        let point = path.at(at);
        quad(
            mesh,
            [
                offset(point, -DECK_HALF_WIDTH, floor - point.elevation),
                offset(point, DECK_HALF_WIDTH, floor - point.elevation),
                offset(point, DECK_HALF_WIDTH, spring - point.elevation),
                offset(point, -DECK_HALF_WIDTH, spring - point.elevation),
            ],
            forward(point).map(|v| v * facing),
            *STONE,
        );
    }
    for piece in 0..ARCH_PIECES {
        #[allow(clippy::cast_precision_loss)] // a few pieces
        let (t0, t1) = (
            piece as f64 / ARCH_PIECES as f64,
            (piece + 1) as f64 / ARCH_PIECES as f64,
        );
        let ((x0, y0), (x1, y1)) = (arch(t0), arch(t1));
        let (p0, p1) = (path.at(x0), path.at(x1));
        // The walls over the arch, up to the deck.
        for side in [-1.0, 1.0] {
            let across = DECK_HALF_WIDTH * side;
            quad(
                mesh,
                [
                    offset(p0, across, y0 - p0.elevation),
                    offset(p1, across, y1 - p1.elevation),
                    offset(p1, across, -DECK_THICKNESS),
                    offset(p0, across, -DECK_THICKNESS),
                ],
                right(p0).map(|v| v * side),
                *STONE,
            );
        }
        // The vault, facing down into the opening.
        let (xm, ym) = arch(f64::midpoint(t0, t1));
        let into = forward(path.at(xm)).map(|v| v * (middle - xm));
        quad(
            mesh,
            [
                offset(p0, -DECK_HALF_WIDTH, y0 - p0.elevation),
                offset(p0, DECK_HALF_WIDTH, y0 - p0.elevation),
                offset(p1, DECK_HALF_WIDTH, y1 - p1.elevation),
                offset(p1, -DECK_HALF_WIDTH, y1 - p1.elevation),
            ],
            [into[0], -(ym - spring).max(0.1), into[2]],
            *STONE,
        );
    }
}

/// The bridge's sides along `(from, to)`, from `bottom(along)` up to `top(along)`, and their
/// ends facing along the bridge: a pier, an abutment or a solid wall.
fn sides(
    mesh: &mut MeshData,
    path: &Path,
    (from, to): (f64, f64),
    bottom: &dyn Fn(f64) -> f64,
    top: &dyn Fn(f64) -> f64,
    color: [f32; 4],
) {
    let pieces = ((to - from) / GROUND_STEP).ceil().max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short stretches
    for piece in 0..pieces as usize {
        #[allow(clippy::cast_precision_loss)] // short stretches
        let (x0, x1) = (
            from + (to - from) * piece as f64 / pieces,
            from + (to - from) * (piece + 1) as f64 / pieces,
        );
        let (p0, p1) = (path.at(x0), path.at(x1));
        for side in [-1.0, 1.0] {
            let across = DECK_HALF_WIDTH * side;
            quad(
                mesh,
                [
                    offset(p0, across, bottom(x0) - p0.elevation),
                    offset(p1, across, bottom(x1) - p1.elevation),
                    offset(p1, across, top(x1) - p1.elevation),
                    offset(p0, across, top(x0) - p0.elevation),
                ],
                right(p0).map(|v| v * side),
                color,
            );
        }
    }
}

/// The bridge's centre line by distance along it.
struct Path<'a> {
    run: &'a [CentrePoint],
    /// Distance along at each point.
    at: Vec<f64>,
    length: f64,
}

impl<'a> Path<'a> {
    fn new(run: &'a [CentrePoint]) -> Self {
        let mut at = vec![0.0];
        for pair in run.windows(2) {
            let step = (pair[1].position.0 - pair[0].position.0)
                .hypot(pair[1].position.1 - pair[0].position.1);
            at.push(at[at.len() - 1] + step);
        }
        let length = at.last().copied().unwrap_or(0.0);
        Self { run, at, length }
    }

    /// The centre line `along` metres from the start, clamped to the bridge.
    fn at(&self, along: f64) -> CentrePoint {
        let along = along.clamp(0.0, self.length);
        let index = self
            .at
            .partition_point(|&d| d <= along)
            .clamp(1, self.run.len().max(2) - 1)
            - 1;
        let (a, b) = (
            self.run[index],
            self.run[(index + 1).min(self.run.len() - 1)],
        );
        let span = (self.at.get(index + 1).copied().unwrap_or(along) - self.at[index]).max(1e-9);
        let t = ((along - self.at[index]) / span).clamp(0.0, 1.0);
        CentrePoint {
            position: (
                a.position.0 + (b.position.0 - a.position.0) * t,
                a.position.1 + (b.position.1 - a.position.1) * t,
            ),
            elevation: a.elevation + (b.elevation - a.elevation) * t,
            direction: a.direction,
        }
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
            quad(mesh, corners, inward, *TUNNEL_WALL);
            // ...and the outside, visible at the portals.
            quad(mesh, corners, inward.map(|v| -v), *CONCRETE);
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

/// A box under the deck at `point`, `half` long and wide (along, across), from `bottom` to `top`.
fn pier(
    mesh: &mut MeshData,
    point: CentrePoint,
    (half_along, half_across): (f64, f64),
    (bottom, top): (f64, f64),
    color: [f32; 4],
) {
    let corner = |along: f64, across: f64, height: f64| {
        let shifted = CentrePoint {
            position: (
                point.position.0 + point.direction.0 * along,
                point.position.1 + point.direction.1 * along,
            ),
            ..point
        };
        offset(shifted, across, height - point.elevation)
    };
    let (l, w) = (half_along, half_across);
    let ahead = forward(point);
    let side = right(point);
    for (corners, normal) in [
        ([(l, -w), (l, w)], ahead),
        ([(-l, w), (-l, -w)], ahead.map(|v| -v)),
        ([(-l, w), (l, w)], side),
        ([(l, -w), (-l, -w)], side.map(|v| -v)),
    ] {
        let [(a0, c0), (a1, c1)] = corners;
        quad(
            mesh,
            [
                corner(a0, c0, bottom),
                corner(a1, c1, bottom),
                corner(a1, c1, top),
                corner(a0, c0, top),
            ],
            normal,
            color,
        );
    }
}

/// Unit vector along the direction of travel, in Godot coordinates.
fn forward(p: CentrePoint) -> [f64; 3] {
    let (de, dn) = p.direction;
    [de, 0.0, -dn]
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
pub(crate) fn quad(mesh: &mut MeshData, corners: [[f64; 3]; 4], normal: [f64; 3], color: [f32; 4]) {
    quad_uv(mesh, corners, [[0.0; 2]; 4], normal, color);
}

/// Like [`quad`], with a texture coordinate per corner.
pub(crate) fn quad_uv(
    mesh: &mut MeshData,
    corners: [[f64; 3]; 4],
    uvs: [[f32; 2]; 4],
    normal: [f64; 3],
    color: [f32; 4],
) {
    let base = push_facing(mesh, &corners, &uvs, normal, color);
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Adds a triangle wound to face `normal`.
pub(crate) fn triangle_uv(
    mesh: &mut MeshData,
    corners: [[f64; 3]; 3],
    uvs: [[f32; 2]; 3],
    normal: [f64; 3],
    color: [f32; 4],
) {
    let base = push_facing(mesh, &corners, &uvs, normal, color);
    mesh.indices.extend([base, base + 1, base + 2]);
}

/// Pushes a polygon's vertices, reversed if needed so they run clockwise seen from `normal`'s
/// side; returns the index of the first vertex.
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
fn push_facing(
    mesh: &mut MeshData,
    corners: &[[f64; 3]],
    uvs: &[[f32; 2]],
    normal: [f64; 3],
    color: [f32; 4],
) -> u32 {
    let (first, second, third) = (corners[0], corners[1], corners[2]);
    let edge_1 = [0, 1, 2].map(|k| second[k] - first[k]);
    let edge_2 = [0, 1, 2].map(|k| third[k] - first[k]);
    let cross = [
        edge_1[1] * edge_2[2] - edge_1[2] * edge_2[1],
        edge_1[2] * edge_2[0] - edge_1[0] * edge_2[2],
        edge_1[0] * edge_2[1] - edge_1[1] * edge_2[0],
    ];
    // Clockwise seen from the front means the right-hand normal points away from the viewer.
    let facing_viewer = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0;
    let mut order: Vec<usize> = (0..corners.len()).collect();
    if facing_viewer {
        order[1..].reverse();
    }
    let base = u32::try_from(mesh.vertices.len()).expect("mesh fits u32");
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    let unit = normal.map(|n| (n / length) as f32);
    for i in order {
        mesh.vertices.push(corners[i].map(|v| v as f32));
        mesh.normals.push(unit);
        mesh.uvs.push(uvs[i]);
        mesh.colors.push(color);
    }
    base
}
