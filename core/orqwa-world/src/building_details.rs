//! Presentation-only façade additions reconstructed from the saved shell surface contract.

use std::collections::{BTreeMap, BTreeSet};

use crate::{MeshData, hash, palette, structures::quad_uv};

const CELL: f64 = 64.0;
const SPACING: f64 = 3.2;
const STOREY: f64 = 3.0;
const MAX_WINDOWS: usize = 256;

type Point = [f64; 3];
type Rect = [f64; 4];

/// Close façade frames, sills, glazing bars and interiors, grouped into small culling cells.
///
/// This reads the existing shell positions, normals, metric UVs and style colours. It neither
/// changes nor persists the input, so kept worlds and lower-quality uploads remain unchanged.
/// Non-rectangular panels and unsupported surface styles retain their ordinary appearance.
#[must_use]
pub fn building_details(shell: &MeshData) -> Vec<MeshData> {
    details(&faces(shell), &|_| true, None)
        .into_values()
        .filter(|mesh| !mesh.indices.is_empty())
        .collect()
}

/// A chunk's façades, read once from its saved shell, from which close details are derived cell
/// by cell as the camera moves: a refresh adds only the cells newly in reach.
#[derive(Debug, Clone, Default)]
pub struct Facades {
    faces: Vec<Face>,
}

/// A 64 m cell of close details, by shell-local `(x, z)` cell index.
pub type DetailCell = (i64, i64);

impl Facades {
    /// Reads the façade panels of a chunk's shell `shell`, which stays unchanged.
    #[must_use]
    pub fn new(shell: &MeshData) -> Self {
        Self {
            faces: faces(shell),
        }
    }

    /// The detail cells within `radius` metres of shell-local `(x, z)` `center` that are not
    /// `loaded` yet, each whole whatever the view, so cells loaded at different times join up.
    /// Cells with nothing to add are left out.
    #[must_use]
    pub fn nearby(
        &self,
        center: [f64; 2],
        radius: f64,
        loaded: &BTreeSet<DetailCell>,
    ) -> Vec<(DetailCell, MeshData)> {
        if !radius.is_finite() || radius <= 0.0 || center.iter().any(|n| !n.is_finite()) {
            return Vec::new();
        }
        let wanted = |cell: DetailCell| !loaded.contains(&cell) && in_reach(cell, center, radius);
        details(&self.faces, &wanted, Some((center, radius + CELL * 1.5)))
            .into_iter()
            .filter(|(_, mesh)| !mesh.indices.is_empty())
            .collect()
    }
}

/// The `loaded` cells to let go of: those a cell's width beyond `radius` metres of shell-local
/// `(x, z)` `center`, so a camera moving to and fro along the edge does not reload them.
#[must_use]
pub fn out_of_reach(
    loaded: &BTreeSet<DetailCell>,
    center: [f64; 2],
    radius: f64,
) -> Vec<DetailCell> {
    loaded
        .iter()
        .copied()
        .filter(|&cell| !in_reach(cell, center, radius + CELL))
        .collect()
}

/// Whether any of `cell` lies within `radius` metres of shell-local `(x, z)` `center`.
#[must_use]
pub fn in_reach((x, z): DetailCell, center: [f64; 2], radius: f64) -> bool {
    #[allow(clippy::cast_precision_loss)] // Cell indices of local world metres are small.
    let (left, top) = (x as f64 * CELL, z as f64 * CELL);
    let dx = (center[0] - center[0].clamp(left, left + CELL)).abs();
    let dz = (center[1] - center[1].clamp(top, top + CELL)).abs();
    dx.hypot(dz) <= radius
}

/// The details of the `wanted` cells; `near` skips faces too far from a centre to reach them.
#[allow(clippy::too_many_lines)] // One bounded pass over the saved façade styles and their trim.
fn details(
    faces: &[Face],
    wanted: &dyn Fn(DetailCell) -> bool,
    near: Option<([f64; 2], f64)>,
) -> BTreeMap<DetailCell, MeshData> {
    let shops: Vec<&Face> = faces.iter().filter(|face| face.style == 10).collect();
    let mut cells: BTreeMap<DetailCell, MeshData> = BTreeMap::new();
    for face in faces {
        if let Some((center, radius)) = near {
            let a = face.point(face.bounds[0], face.bounds[1], 0.0);
            let b = face.point(face.bounds[2], face.bounds[1], 0.0);
            let line = [b[0] - a[0], b[2] - a[2]];
            let t = (((center[0] - a[0]) * line[0] + (center[1] - a[2]) * line[1])
                / (line[0] * line[0] + line[1] * line[1]).max(1e-6))
            .clamp(0.0, 1.0);
            if (a[0] + t * line[0] - center[0]).hypot(a[2] + t * line[1] - center[1]) > radius {
                continue;
            }
        }
        let mut windows = Vec::new();
        match face.style {
            0 | 2 | 11 => {
                for row in 0..64 {
                    let y = f64::from(row) * STOREY;
                    if face.style == 11 && row == 0 {
                        continue;
                    }
                    for column in 0..64 {
                        let x = f64::from(column) * SPACING;
                        if x > face.bounds[2] {
                            break;
                        }
                        let pane = if face.style == 11 {
                            [x + 0.12, y + 0.95, x + SPACING - 0.12, y + 2.35]
                        } else {
                            [x + 0.96, y + 1.05, x + 2.24, y + 2.34]
                        };
                        if face.contains(expand(pane, 0.10))
                            && !shops.iter().any(|shop| {
                                shop.covers(
                                    face.point(
                                        f64::midpoint(pane[0], pane[2]),
                                        f64::midpoint(pane[1], pane[3]),
                                        0.0,
                                    ),
                                    face.normal,
                                )
                            })
                        {
                            windows.push(pane);
                        }
                        if windows.len() == MAX_WINDOWS {
                            break;
                        }
                    }
                    if windows.len() == MAX_WINDOWS || y > face.bounds[3] {
                        break;
                    }
                }
            }
            10 => {
                for column in 0..64 {
                    let x = f64::from(column) * SPACING;
                    let pane = [x + 0.18, 0.45, x + SPACING - 0.18, 2.55];
                    if face.contains(expand(pane, 0.08)) {
                        windows.push(pane);
                    }
                }
            }
            6 => {
                let pane = [0.12, 0.10, 0.88, 0.90];
                if face.contains(pane) && length(face.along) > 0.4 && length(face.up) > 0.6 {
                    windows.push(pane);
                }
            }
            _ => continue,
        }
        for pane in windows {
            let middle = face.point(
                f64::midpoint(pane[0], pane[2]),
                f64::midpoint(pane[1], pane[3]),
                0.0,
            );
            if !wanted(cell(middle)) {
                continue;
            }
            let mesh = cells.entry(cell(middle)).or_default();
            window(mesh, face, pane, face.style == 10);
        }
        if matches!(face.style, 0 | 2) && face.bounds[3] >= 3.0 {
            let [left, bottom, right, top] = face.bounds;
            let middle = face.point(f64::midpoint(left, right), f64::midpoint(bottom, top), 0.0);
            if !wanted(cell(middle)) {
                continue;
            }
            let mesh = cells.entry(cell(middle)).or_default();
            let paint = face.color;
            face.box_mesh(
                mesh,
                [left, top - 0.16, right, top - 0.04],
                [0.0, 0.14],
                paint,
            );
            let metal = color("buildings.metal", 2);
            let pipe = 0.07 / length(face.along);
            if (right - left) * length(face.along) > 2.0 {
                face.box_mesh(
                    mesh,
                    [
                        left + pipe,
                        bottom + if bottom < 0.0 { 0.10 } else { 0.0 },
                        left + pipe * 2.0,
                        top,
                    ],
                    [0.06, 0.13],
                    metal,
                );
            }
        }
    }
    cells
}

/// A pane's glass carries its seed in U and, for a shop window, V lifted by one: shops show a
/// lit sales floor rather than a home's curtains.
fn window(mesh: &mut MeshData, face: &Face, pane: Rect, shop: bool) {
    let horizontal = 0.07 / length(face.along);
    let vertical = 0.07 / length(face.up);
    let [left, bottom, right, top] = pane;
    let trim = color("buildings.frame", 0);
    for strip in [
        [left - horizontal, bottom - vertical, left, top + vertical],
        [right, bottom - vertical, right + horizontal, top + vertical],
        [left, bottom - vertical, right, bottom],
        [left, top, right, top + vertical],
    ] {
        face.box_mesh(mesh, strip, [0.012, 0.13], trim);
    }
    let centre = f64::midpoint(left, right);
    face.box_mesh(
        mesh,
        [
            centre - horizontal * 0.25,
            bottom,
            centre + horizontal * 0.25,
            top,
        ],
        [0.018, 0.045],
        trim,
    );
    let cross = bottom + (top - bottom) * 0.68;
    face.box_mesh(
        mesh,
        [
            left,
            cross - vertical * 0.25,
            right,
            cross + vertical * 0.25,
        ],
        [0.018, 0.045],
        trim,
    );
    face.box_mesh(
        mesh,
        [
            left - horizontal * 1.5,
            bottom - vertical * 2.0,
            right + horizontal * 1.5,
            bottom - vertical,
        ],
        [0.0, 0.22],
        color("buildings.stone", 0),
    );
    let position = face.point(centre, f64::midpoint(bottom, top), 0.0);
    #[allow(clippy::cast_possible_truncation)] // Local metric coordinates and a 64-value seed.
    let seed = (hash(
        (position[0] * 10.0) as i64
            ^ ((position[1] * 10.0) as i64).wrapping_mul(131)
            ^ ((position[2] * 10.0) as i64).wrapping_mul(31),
    ) * 64.0)
        .floor() as f32;
    let floor = if shop { 1.0 } else { 0.0 };
    quad_uv(
        mesh,
        [
            face.point(left, bottom, 0.014),
            face.point(right, bottom, 0.014),
            face.point(right, top, 0.014),
            face.point(left, top, 0.014),
        ],
        [
            [seed, floor],
            [seed + 0.999, floor],
            [seed + 0.999, floor + 0.999],
            [seed, floor + 0.999],
        ],
        face.normal,
        color("buildings.glass", 1),
    );
}

#[derive(Debug, Clone)]
struct Face {
    origin: Point,
    along: Point,
    up: Point,
    normal: Point,
    bounds: Rect,
    style: u8,
    color: [f32; 4],
}

impl Face {
    fn point(&self, u: f64, v: f64, out: f64) -> Point {
        [0, 1, 2].map(|k| {
            self.origin[k]
                + self.along[k] * (u - self.bounds[0])
                + self.up[k] * (v - self.bounds[1])
                + self.normal[k] * out
        })
    }

    fn contains(&self, [left, bottom, right, top]: Rect) -> bool {
        let [x0, y0, x1, y1] = self.bounds;
        left >= x0 - 1e-4 && bottom >= y0 - 1e-4 && right <= x1 + 1e-4 && top <= y1 + 1e-4
    }

    fn covers(&self, point: Point, normal: Point) -> bool {
        let delta = sub(point, self.origin);
        dot(normal, self.normal) > 0.99
            && dot(delta, self.normal).abs() < 0.15
            && self.contains_point(
                dot(delta, self.along) / dot(self.along, self.along) + self.bounds[0],
                dot(delta, self.up) / dot(self.up, self.up) + self.bounds[1],
            )
    }

    fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.bounds[0] && x <= self.bounds[2] && y >= self.bounds[1] && y <= self.bounds[3]
    }

    fn box_mesh(
        &self,
        mesh: &mut MeshData,
        [x0, y0, x1, y1]: Rect,
        [z0, z1]: [f64; 2],
        paint: [f32; 4],
    ) {
        let p = |x, y, z| self.point(x, y, z);
        let along = self.along.map(|n| n / length(self.along));
        let up = self.up.map(|n| n / length(self.up));
        for (points, normal) in [
            (
                [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
                self.normal,
            ),
            (
                [p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0)],
                self.normal.map(|n| -n),
            ),
            (
                [p(x0, y0, z0), p(x0, y1, z0), p(x0, y1, z1), p(x0, y0, z1)],
                along.map(|n| -n),
            ),
            (
                [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
                along,
            ),
            (
                [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
                up.map(|n| -n),
            ),
            (
                [p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1)],
                up,
            ),
        ] {
            quad_uv(mesh, points, [[0.0; 2]; 4], normal, paint);
        }
    }
}

fn faces(mesh: &MeshData) -> Vec<Face> {
    let count = mesh.vertices.len();
    if mesh.normals.len() != count || mesh.uvs.len() != count || mesh.colors.len() != count {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut offset = 0;
    while offset + 5 < mesh.indices.len() {
        let indices = &mesh.indices[offset..offset + 6];
        if indices[0] == indices[3] && indices[2] == indices[4] {
            let ids = [indices[0], indices[1], indices[2], indices[5]].map(|i| i as usize);
            if let Some(face) = face(mesh, ids) {
                result.push(face);
            }
            offset += 6;
        } else {
            offset += 3;
        }
    }
    result
}

fn face(mesh: &MeshData, ids: [usize; 4]) -> Option<Face> {
    if ids.iter().any(|&i| i >= mesh.vertices.len()) {
        return None;
    }
    let normal = mesh.normals[ids[0]].map(f64::from);
    let paint = mesh.colors[ids[0]];
    if normal[1].abs() > 0.001
        || (length(normal) - 1.0).abs() > 0.01
        || normal.iter().any(|n| !n.is_finite())
        || paint.iter().any(|n| !n.is_finite())
        || !(0.0..=1.0).contains(&paint[3])
    {
        return None;
    }
    let uvs = ids.map(|i| mesh.uvs[i].map(f64::from));
    if uvs.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let x0 = uvs.iter().map(|uv| uv[0]).fold(f64::INFINITY, f64::min);
    let y0 = uvs.iter().map(|uv| uv[1]).fold(f64::INFINITY, f64::min);
    let x1 = uvs.iter().map(|uv| uv[0]).fold(f64::NEG_INFINITY, f64::max);
    let y1 = uvs.iter().map(|uv| uv[1]).fold(f64::NEG_INFINITY, f64::max);
    if x1 - x0 < 0.01 || y1 - y0 < 0.01 || x1 - x0 > 2048.0 || y1 - y0 > 600.0 {
        return None;
    }
    let at = |x: f64, y: f64| {
        uvs.iter()
            .position(|uv| (uv[0] - x).abs() < 1e-4 && (uv[1] - y).abs() < 1e-4)
            .map(|i| mesh.vertices[ids[i]].map(f64::from))
    };
    let origin = at(x0, y0)?;
    let along = sub(at(x1, y0)?, origin).map(|n| n / (x1 - x0));
    let up = sub(at(x0, y1)?, origin).map(|n| n / (y1 - y0));
    at(x1, y1)?;
    if origin
        .iter()
        .chain(along.iter())
        .chain(up.iter())
        .any(|v| !v.is_finite())
        || length(along) < 0.01
        || length(up) < 0.01
    {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // Checked finite alpha in 0..=1.
    let style = (paint[3] * 11.0).round() as u8;
    Some(Face {
        origin,
        along,
        up,
        normal,
        bounds: [x0, y0, x1, y1],
        style,
        color: [paint[0], paint[1], paint[2], 0.0],
    })
}

fn color(name: &str, style: u8) -> [f32; 4] {
    palette::srgb(name, f32::from(style) / 2.0)
}

fn expand([x0, y0, x1, y1]: Rect, margin: f64) -> Rect {
    [x0 - margin, y0 - margin, x1 + margin, y1 + margin]
}

fn sub(a: Point, b: Point) -> Point {
    [0, 1, 2].map(|k| a[k] - b[k])
}
fn dot(a: Point, b: Point) -> f64 {
    (0..3).map(|k| a[k] * b[k]).sum()
}
fn length(a: Point) -> f64 {
    dot(a, a).sqrt()
}

#[allow(clippy::cast_possible_truncation)] // Local world metres are far below i64 bounds.
fn cell([x, _, z]: Point) -> DetailCell {
    ((x / CELL).floor() as i64, (z / CELL).floor() as i64)
}

#[cfg(test)]
mod tests;
