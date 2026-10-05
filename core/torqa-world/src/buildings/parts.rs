//! Surfaces of buildings: walls, roofs of several shapes, boxes and windows.

use crate::MeshData;
use crate::structures::{quad_uv, triangle_uv};

use super::shape::{Point, Rect, distance, offset, perimeter, signed_area, triangulate};

/// Roofs are this thick at their edges.
const ROOF_THICKNESS: f64 = 0.22;
/// Windows stand this far out of their wall, so they never flicker into it.
const WINDOW_RELIEF: f64 = 0.04;
/// Windows repeat this often along plastered and timber walls, and church windows this often
/// (`app/shaders/building.gdshader`: `window_spacing`, and the church's 4.5 m).
pub(crate) const WINDOW_SPACING: f64 = 3.2;
const CHURCH_WINDOW_SPACING: f64 = 4.5;

/// How the building shader draws a surface. Stored in the vertex colour's alpha as
/// code / `Style::LAST`; `app/shaders/building.gdshader` uses the same codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    /// Plastered wall with a grid of windows.
    Plaster = 0,
    /// Plastered wall without windows.
    Blank = 1,
    /// Wooden boards with windows.
    Timber = 2,
    /// Wooden boards without windows.
    Boards = 3,
    /// Church wall with tall arched windows.
    Church = 4,
    /// Metal cladding with a band of windows.
    Cladding = 5,
    /// A single window: frame and glass over the whole surface; the colour is the frame's.
    Window = 6,
    /// Tiled roof.
    Tiles = 7,
    /// Sheet-metal roof.
    Sheet = 8,
    /// Flat roof: gravel or membrane.
    Flat = 9,
}

impl Style {
    const LAST: f32 = 9.0;

    /// How often windows repeat along a wall of this style, if they do.
    fn window_spacing(self) -> Option<f64> {
        match self {
            Self::Plaster | Self::Timber => Some(WINDOW_SPACING),
            Self::Church => Some(CHURCH_WINDOW_SPACING),
            _ => None,
        }
    }

    fn alpha(self) -> f32 {
        f32::from(self as u8) / Self::LAST
    }
}

/// A surface's colour (sRGB) and style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Paint {
    pub(crate) rgb: [f32; 3],
    pub(crate) style: Style,
}

impl Paint {
    pub(crate) fn new(rgb: [f32; 3], style: Style) -> Self {
        Self { rgb, style }
    }

    pub(crate) fn with(self, style: Style) -> Self {
        Self { style, ..self }
    }

    fn rgba(self) -> [f32; 4] {
        [self.rgb[0], self.rgb[1], self.rgb[2], self.style.alpha()]
    }
}

/// A pitched roof's shape.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pitch {
    /// Height of the eaves, where the walls end.
    pub(crate) eaves: f64,
    /// Slope in radians.
    pub(crate) angle: f64,
    /// How far the roof reaches beyond the walls below its eaves.
    pub(crate) overhang: f64,
    /// How far it reaches beyond the gable walls.
    pub(crate) verge: f64,
}

/// Paints of a pitched roof.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoofPaint {
    pub(crate) top: Paint,
    /// Underside and edges.
    pub(crate) under: Paint,
    /// Gable walls.
    pub(crate) gables: Paint,
}

/// Adds the surfaces of one building to a chunk mesh. Heights are absolute.
pub(crate) struct Builder<'a> {
    pub(crate) mesh: &'a mut MeshData,
    /// Chunk centre the vertices are relative to.
    pub(crate) origin: [f64; 3],
    /// Ground floor level: walls' texture coordinates count from it.
    pub(crate) ground: f64,
    /// Where walls end below the ground.
    pub(crate) footing: f64,
}

// Geometry is stored as f32 for the GPU.
#[allow(clippy::cast_possible_truncation)]
impl Builder<'_> {
    fn at(&self, (e, n): Point, y: f64) -> [f64; 3] {
        [e - self.origin[0], y - self.origin[1], -n - self.origin[2]]
    }

    /// Texture coordinates on walls: metres along and above the ground, for windows.
    fn wall_uv(&self, along: f64, y: f64) -> [f32; 2] {
        [along as f32, (y - self.ground) as f32]
    }

    /// Walls along a counter-clockwise outline from `bottom` to `top`, facing out.
    pub(crate) fn walls(&mut self, outline: &[Point], bottom: f64, top: f64, paint: Paint) {
        self.walls_facing(outline, bottom, top, paint, 1.0);
    }

    /// Walls facing out (`side` 1) or in (−1). Each wall holds whole windows only: its own grid
    /// from corner to corner, stretched a little to a whole number of them, and none on walls
    /// too short for one.
    fn walls_facing(&mut self, outline: &[Point], bottom: f64, top: f64, paint: Paint, side: f64) {
        for i in 0..outline.len() {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            let length = distance(a, b);
            if length < 0.01 {
                continue;
            }
            let span = match paint.style.window_spacing() {
                Some(spacing) => (length / spacing).round() * spacing,
                None => length,
            };
            // Counter-clockwise outline: the outside is to the right of travel.
            let out = ((b.1 - a.1) / length * side, (a.0 - b.0) / length * side);
            let uvs = [
                self.wall_uv(0.0, bottom),
                self.wall_uv(span, bottom),
                self.wall_uv(span, top),
                self.wall_uv(0.0, top),
            ];
            self.upright(
                &[(a, bottom), (b, bottom), (b, top), (a, top)],
                &uvs,
                out,
                paint,
            );
        }
    }

    /// A vertical quad or triangle facing the horizontal direction `out` (east, north).
    fn upright(&mut self, corners: &[(Point, f64)], uvs: &[[f32; 2]], out: Point, paint: Paint) {
        let normal = [out.0, 0.0, -out.1];
        self.surface(corners, uvs, normal, paint);
    }

    /// A sloped quad or triangle showing its upper side, or its lower one.
    fn slope(&mut self, corners: &[(Point, f64)], uvs: &[[f32; 2]], paint: Paint, upper: bool) {
        let points: Vec<[f64; 3]> = corners.iter().map(|&(p, y)| self.at(p, y)).collect();
        let normal = upward_normal(&points).map(|v| if upper { v } else { -v });
        self.surface(corners, uvs, normal, paint);
    }

    fn surface(
        &mut self,
        corners: &[(Point, f64)],
        uvs: &[[f32; 2]],
        normal: [f64; 3],
        paint: Paint,
    ) {
        let points: Vec<[f64; 3]> = corners.iter().map(|&(p, y)| self.at(p, y)).collect();
        let rgba = paint.rgba();
        match (points.as_slice(), uvs) {
            (&[a, b, c], &[ua, ub, uc]) => {
                triangle_uv(self.mesh, [a, b, c], [ua, ub, uc], normal, rgba);
            }
            (&[a, b, c, d], &[ua, ub, uc, ud]) => {
                quad_uv(self.mesh, [a, b, c, d], [ua, ub, uc, ud], normal, rgba);
            }
            _ => unreachable!("surfaces are triangles or quads"),
        }
    }

    /// A horizontal polygon (counter-clockwise outline) showing its upper side, or its lower.
    fn flat(&mut self, outline: &[Point], height: f64, paint: Paint, upper: bool) {
        let Some(&first) = outline.first() else {
            return;
        };
        let points: Vec<[f32; 3]> = outline
            .iter()
            .map(|&p| self.at(p, height).map(|v| v as f32))
            .collect();
        let mesh = &mut *self.mesh;
        let base = u32::try_from(mesh.vertices.len()).expect("chunk mesh fits u32");
        for (&point, vertex) in outline.iter().zip(points) {
            mesh.vertices.push(vertex);
            mesh.normals
                .push([0.0, if upper { 1.0 } else { -1.0 }, 0.0]);
            mesh.uvs
                .push([(point.0 - first.0) as f32, (point.1 - first.1) as f32]);
            mesh.colors.push(paint.rgba());
        }
        for [a, b, c] in triangulate(outline) {
            // Counter-clockwise seen from above; Godot's front faces are clockwise.
            let triangle = if upper { [c, b, a] } else { [a, b, c] };
            mesh.indices.extend(triangle.map(|k| base + k));
        }
    }

    /// A flat roof at `height` behind a parapet `parapet` high, whose outside the walls
    /// provide by reaching its top.
    pub(crate) fn flat_roof(
        &mut self,
        outline: &[Point],
        height: f64,
        parapet: f64,
        roof: Paint,
        inside: Paint,
    ) {
        self.flat(outline, height, roof, true);
        if parapet > 0.0 {
            self.walls_facing(outline, height, height + parapet, inside, -1.0);
        }
    }

    /// A gable roof over `rect` with its ridge along the axis and triangular gable walls at
    /// the ends. Returns the height of the ridge's top.
    pub(crate) fn gable_roof(&mut self, rect: &Rect, pitch: Pitch, paint: RoofPaint) -> f64 {
        let (length, width) = (rect.half_length, rect.half_width);
        let slope = pitch.angle.tan();
        let reach = length + pitch.verge;
        let ridge = pitch.eaves + width * slope;
        let edge = pitch.eaves - pitch.overhang * slope;
        let rafter = ((width + pitch.overhang) / pitch.angle.cos()) as f32;
        let along = reach as f32;
        for side in [-1.0, 1.0] {
            let across = side * (width + pitch.overhang);
            for (lift, upper, paint) in
                [(ROOF_THICKNESS, true, paint.top), (0.0, false, paint.under)]
            {
                self.slope(
                    &[
                        (rect.point(-reach, across), edge + lift),
                        (rect.point(reach, across), edge + lift),
                        (rect.point(reach, 0.0), ridge + lift),
                        (rect.point(-reach, 0.0), ridge + lift),
                    ],
                    &[
                        [-along, 0.0],
                        [along, 0.0],
                        [along, rafter],
                        [-along, rafter],
                    ],
                    paint,
                    upper,
                );
            }
            let out = rect.across();
            self.upright(
                &[
                    (rect.point(-reach, across), edge),
                    (rect.point(reach, across), edge),
                    (rect.point(reach, across), edge + ROOF_THICKNESS),
                    (rect.point(-reach, across), edge + ROOF_THICKNESS),
                ],
                &[[0.0; 2]; 4],
                (out.0 * side, out.1 * side),
                paint.under,
            );
            for end in [-1.0, 1.0] {
                let verge = end * reach;
                self.upright(
                    &[
                        (rect.point(verge, across), edge),
                        (rect.point(verge, 0.0), ridge),
                        (rect.point(verge, 0.0), ridge + ROOF_THICKNESS),
                        (rect.point(verge, across), edge + ROOF_THICKNESS),
                    ],
                    &[[0.0; 2]; 4],
                    (rect.axis.0 * end, rect.axis.1 * end),
                    paint.under,
                );
            }
        }
        for end in [-1.0, 1.0] {
            let gable = end * length;
            let uvs = [
                self.wall_uv(0.0, pitch.eaves),
                self.wall_uv(2.0 * width, pitch.eaves),
                self.wall_uv(width, ridge),
            ];
            self.upright(
                &[
                    (rect.point(gable, -width), pitch.eaves),
                    (rect.point(gable, width), pitch.eaves),
                    (rect.point(gable, 0.0), ridge),
                ],
                &uvs,
                (rect.axis.0 * end, rect.axis.1 * end),
                paint.gables,
            );
        }
        ridge + ROOF_THICKNESS
    }

    /// A hipped roof over `rect`: slopes on all four sides up to a ridge along the axis, or
    /// to a point over squares. Returns the height of the ridge's top.
    pub(crate) fn hipped_roof(&mut self, rect: &Rect, pitch: Pitch, paint: RoofPaint) -> f64 {
        let slope = pitch.angle.tan();
        let reach_length = rect.half_length + pitch.overhang;
        let reach_width = rect.half_width + pitch.overhang;
        let ridge = pitch.eaves + rect.half_width * slope;
        let edge = pitch.eaves - pitch.overhang * slope;
        // The hips meet the ridge where the end slopes reach its height.
        let ridge_end = rect.half_length - rect.half_width;
        let rafter = (reach_width / pitch.angle.cos()) as f32;
        let (uv_length, uv_width, uv_ridge) =
            (reach_length as f32, reach_width as f32, ridge_end as f32);
        for (lift, upper, paint) in [(ROOF_THICKNESS, true, paint.top), (0.0, false, paint.under)] {
            for side in [-1.0, 1.0] {
                let across = side * reach_width;
                let eaves = [
                    (rect.point(-reach_length, across), edge + lift),
                    (rect.point(reach_length, across), edge + lift),
                ];
                if ridge_end > 0.01 {
                    self.slope(
                        &[
                            eaves[0],
                            eaves[1],
                            (rect.point(ridge_end, 0.0), ridge + lift),
                            (rect.point(-ridge_end, 0.0), ridge + lift),
                        ],
                        &[
                            [-uv_length, 0.0],
                            [uv_length, 0.0],
                            [uv_ridge, rafter],
                            [-uv_ridge, rafter],
                        ],
                        paint,
                        upper,
                    );
                } else {
                    // Over a square the ridge is a point.
                    self.slope(
                        &[eaves[0], eaves[1], (rect.centre, ridge + lift)],
                        &[[-uv_length, 0.0], [uv_length, 0.0], [0.0, rafter]],
                        paint,
                        upper,
                    );
                }
                let along = side * reach_length;
                self.slope(
                    &[
                        (rect.point(along, -reach_width), edge + lift),
                        (rect.point(along, reach_width), edge + lift),
                        (rect.point(side * ridge_end, 0.0), ridge + lift),
                    ],
                    &[[-uv_width, 0.0], [uv_width, 0.0], [0.0, rafter]],
                    paint,
                    upper,
                );
            }
        }
        let eaves = Rect {
            half_length: reach_length,
            half_width: reach_width,
            ..*rect
        };
        self.walls(&eaves.corners(), edge, edge + ROOF_THICKNESS, paint.under);
        ridge + ROOF_THICKNESS
    }

    /// A roof for outlines no rectangle fits: a band sloping up from the eaves all round to
    /// a flat top, which reads as a hipped roof from the ground. Returns the height of its
    /// top, or `None` where the outline is too irregular for the band.
    pub(crate) fn hip_band_roof(
        &mut self,
        outline: &[Point],
        pitch: Pitch,
        paint: RoofPaint,
    ) -> Option<f64> {
        // A fraction of the outline's typical width, so narrow wings keep a flat top.
        let band = (0.7 * signed_area(outline) / perimeter(outline)).min(3.0);
        let inner = offset(outline, band)?;
        let outer = offset(outline, -pitch.overhang)?;
        let slope = pitch.angle.tan();
        let t = ROOF_THICKNESS;
        let edge = pitch.eaves - pitch.overhang * slope;
        let top = pitch.eaves + band * slope;
        let rafter = ((band + pitch.overhang) / pitch.angle.cos()) as f32;
        let mut along = 0.0;
        for i in 0..outline.len() {
            let j = (i + 1) % outline.len();
            let length = distance(outer[i], outer[j]);
            let (from, to) = (along as f32, (along + length) as f32);
            self.slope(
                &[
                    (outer[i], edge + t),
                    (outer[j], edge + t),
                    (inner[j], top + t),
                    (inner[i], top + t),
                ],
                &[[from, 0.0], [to, 0.0], [to, rafter], [from, rafter]],
                paint.top,
                true,
            );
            self.slope(
                &[
                    (outer[i], edge),
                    (outer[j], edge),
                    (outline[j], pitch.eaves),
                    (outline[i], pitch.eaves),
                ],
                &[[0.0; 2]; 4],
                paint.under,
                false,
            );
            along += length;
        }
        self.walls(&outer, edge, edge + t, paint.under);
        self.flat(&inner, top + t, paint.top.with(Style::Flat), true);
        Some(top + t)
    }

    /// A band standing `out` metres out of the walls of a counter-clockwise `outline`, from
    /// `bottom` to `top`: a cornice or string course. Nothing where the outline cannot be
    /// widened (sharp corners).
    pub(crate) fn band(
        &mut self,
        outline: &[Point],
        (bottom, top): (f64, f64),
        out: f64,
        paint: Paint,
    ) {
        let Some(outer) = offset(outline, -out) else {
            return;
        };
        self.walls(&outer, bottom, top, paint);
        self.ring(outline, &outer, top, paint, true);
        self.ring(outline, &outer, bottom, paint, false);
    }

    /// The level ring between `inner` and `outer` (corresponding points) at `height`, facing up
    /// or down.
    fn ring(&mut self, inner: &[Point], outer: &[Point], height: f64, paint: Paint, upper: bool) {
        let normal = [0.0, if upper { 1.0 } else { -1.0 }, 0.0];
        for i in 0..inner.len() {
            let j = (i + 1) % inner.len();
            let corners = [inner[i], inner[j], outer[j], outer[i]].map(|p| self.at(p, height));
            quad_uv(self.mesh, corners, [[0.0; 2]; 4], normal, paint.rgba());
        }
    }

    /// A box on `rect` from `bottom` to `top`: chimneys, towers, balconies.
    pub(crate) fn cuboid(
        &mut self,
        rect: &Rect,
        (bottom, top): (f64, f64),
        sides: Paint,
        cap: Paint,
        underside: bool,
    ) {
        let corners = rect.corners();
        self.walls(&corners, bottom, top, sides);
        self.flat(&corners, top, cap, true);
        if underside {
            self.flat(&corners, bottom, cap, false);
        }
    }

    /// A window centred on the wall point `centre`, the wall facing `out`; `frame` colours
    /// its frame.
    pub(crate) fn window(
        &mut self,
        centre: Point,
        out: Point,
        half_width: f64,
        (bottom, top): (f64, f64),
        frame: [f32; 3],
    ) {
        let side = (-out.1, out.0);
        let middle = (
            centre.0 + out.0 * WINDOW_RELIEF,
            centre.1 + out.1 * WINDOW_RELIEF,
        );
        let left = (
            middle.0 - side.0 * half_width,
            middle.1 - side.1 * half_width,
        );
        let right = (
            middle.0 + side.0 * half_width,
            middle.1 + side.1 * half_width,
        );
        self.upright(
            &[(left, bottom), (right, bottom), (right, top), (left, top)],
            &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            out,
            Paint::new(frame, Style::Window),
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
