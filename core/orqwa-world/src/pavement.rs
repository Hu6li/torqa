//! Barrier clearance against the pavement actually drawn, including junction fans and
//! overlapping curves. Metadata is used only while generating; kept-world layout is unchanged.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::{MeshData, World};

const CELL: f64 = 16.0;
/// Include the thickness of parapets, blades and posts around their placement line.
const MARGIN: f64 = 0.01;
const BARRIER_HEIGHT: f64 = 1.2;
/// How far beside its line a fence or barrier span must stay off pavement, unless it says.
pub(crate) const SPAN_HALF_WIDTH: f64 = 0.15;

#[derive(Default)]
pub(crate) struct Pavement {
    triangles: Vec<[[f64; 3]; 3]>,
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// The only cells kept, when the pavement is made for known questions (`Pavement::near`).
    only: Option<HashSet<(i64, i64)>>,
}

/// The cells barriers will be checked in. A pavement keeping only those answers every check as
/// the whole one would, without a copy of every street of the world: on a long course in a
/// densely mapped country that copy alone took more memory than a tablet has to spare.
#[derive(Default)]
pub(crate) struct Asked(HashSet<(i64, i64)>);

impl Asked {
    /// A span from `a` to `b` checked `half_width` either side (`Pavement::crosses_width`).
    pub(crate) fn span(&mut self, a: [f64; 3], b: [f64; 3], half_width: f64) {
        let margin = half_width + MARGIN;
        for x in cell(a[0].min(b[0]) - margin)..=cell(a[0].max(b[0]) + margin) {
            for z in cell(a[2].min(b[2]) - margin)..=cell(a[2].max(b[2]) + margin) {
                self.0.insert((x, z));
            }
        }
    }

    /// The checks `Pavement::clear` makes for `barriers` in `mesh`, centred at `origin`.
    pub(crate) fn barriers(&mut self, mesh: &MeshData, barriers: &[Barrier], origin: [f32; 3]) {
        for barrier in barriers {
            if barrier.slab {
                for &i in &mesh.indices[barrier.indices.clone()] {
                    let v = mesh.vertices[i as usize];
                    let x = f64::from(v[0]) + f64::from(origin[0]);
                    let z = f64::from(v[2]) + f64::from(origin[2]);
                    self.0.insert((cell(x), cell(z)));
                }
            } else {
                self.span(barrier.a, barrier.b, barrier.half_width);
            }
        }
    }
}

/// A generated barrier's triangles and its base line, in absolute Godot coordinates.
pub(crate) struct Barrier {
    pub(crate) indices: Range<usize>,
    pub(crate) a: [f64; 3],
    pub(crate) b: [f64; 3],
    pub(crate) half_width: f64,
    pub(crate) slab: bool,
}

impl Barrier {
    pub(crate) fn slab(indices: Range<usize>) -> Self {
        Self {
            indices,
            a: [0.0; 3],
            b: [0.0; 3],
            half_width: 0.0,
            slab: true,
        }
    }
}

impl Pavement {
    /// Actual carriageway footprint in east/north, without the conservative terrain envelope.
    pub(crate) fn contains(&self, east: f64, north: f64) -> bool {
        let p = [east, 0.0, -north];
        self.in_cell((cell(east), cell(-north))).iter().any(|&i| {
            let [a, b, c] = self.triangles[i];
            let area = cross(a, b, c);
            [cross(a, b, p), cross(b, c, p), cross(c, a, p)]
                .into_iter()
                .all(|side| side / area >= -1e-8)
        })
    }

    pub(crate) fn new(world: &World, track_lengths: impl IntoIterator<Item = usize>) -> Self {
        Self::default().with(world, track_lengths)
    }

    /// The pavement of `world` in the cells `asked` only: it answers those checks as
    /// [`Pavement::new`] would.
    pub(crate) fn near(
        world: &World,
        track_lengths: impl IntoIterator<Item = usize>,
        asked: Asked,
    ) -> Self {
        Self {
            only: Some(asked.0),
            ..Self::default()
        }
        .with(world, track_lengths)
    }

    fn with(mut self, world: &World, track_lengths: impl IntoIterator<Item = usize>) -> Self {
        let pavement = &mut self;
        pavement.add(&world.road, [0.0; 3], true, world.road.indices.len());
        for (chunk, track_length) in world.chunks.iter().zip(track_lengths) {
            pavement.add(
                &chunk.streets,
                chunk.center,
                false,
                chunk.streets.indices.len(),
            );
            // Shore gravel shares this GPU material but is not a road.
            pavement.add(&chunk.tracks, chunk.center, false, track_length);
        }
        self
    }

    fn add(&mut self, mesh: &MeshData, origin: [f32; 3], ridden: bool, indices: usize) {
        for face in mesh.indices[..indices].as_chunks::<3>().0 {
            // Road skirts are verges, and mapped bridge details carry tints with alpha zero.
            if face.iter().any(|&i| {
                let i = i as usize;
                (ridden && mesh.normals[i][1] < 0.9999)
                    || (!mesh.colors.is_empty() && mesh.colors[i][3] <= 0.0)
            }) {
                continue;
            }
            let triangle = face.map(|i| {
                [0, 1, 2].map(|k| f64::from(mesh.vertices[i as usize][k]) + f64::from(origin[k]))
            });
            let us = face.map(|i| f64::from(mesh.uvs[i as usize][0]));
            let width = mesh
                .colors
                .first()
                .map_or(6.0, |_| f64::from(mesh.colors[face[0] as usize][1]) * 32.0);
            if !ridden && width > 0.0 && (us[0] - us[1]).abs().max((us[0] - us[2]).abs()) > 1e-9 {
                // The mapped deck includes its 16 cm edge kerb. Junction fans have
                // constant UVs and remain complete; they must not be mistaken for kerbs.
                let inset = 0.22 / width;
                let mut polygon: Vec<_> = triangle.into_iter().zip(us).collect();
                for (bound, sign) in [(inset, 1.0), (1.0 - inset, -1.0)] {
                    let mut clipped = Vec::new();
                    for i in 0..polygon.len() {
                        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                        let (fa, fb) = ((a.1 - bound) * sign, (b.1 - bound) * sign);
                        if fa >= 0.0 {
                            clipped.push(a);
                        }
                        if (fa >= 0.0) != (fb >= 0.0) {
                            let t = fa / (fa - fb);
                            clipped
                                .push(([0, 1, 2].map(|k| a.0[k] + (b.0[k] - a.0[k]) * t), bound));
                        }
                    }
                    polygon = clipped;
                }
                for i in 1..polygon.len().saturating_sub(1) {
                    self.insert([polygon[0].0, polygon[i].0, polygon[i + 1].0]);
                }
            } else {
                self.insert(triangle);
            }
        }
    }

    fn insert(&mut self, triangle: [[f64; 3]; 3]) {
        let [a, b, c] = triangle;
        if cross(a, b, c).abs() < 1e-9 {
            return;
        }
        let low = [a[0].min(b[0]).min(c[0]), a[2].min(b[2]).min(c[2])];
        let high = [a[0].max(b[0]).max(c[0]), a[2].max(b[2]).max(c[2])];
        let index = self.triangles.len();
        let mut kept = false;
        for x in cell(low[0])..=cell(high[0]) {
            for z in cell(low[1])..=cell(high[1]) {
                if self.only.as_ref().is_none_or(|only| only.contains(&(x, z))) {
                    self.cells.entry((x, z)).or_default().push(index);
                    kept = true;
                }
            }
        }
        if kept {
            self.triangles.push(triangle);
        }
    }

    /// The whole barrier span is checked, even when both ends lie outside a narrow road.
    /// Pavement at a different level does not remove overpass protection.
    #[cfg(test)]
    pub(crate) fn crosses(&self, a: [f64; 3], b: [f64; 3]) -> bool {
        self.crosses_width(a, b, SPAN_HALF_WIDTH)
    }

    pub(crate) fn crosses_width(&self, a: [f64; 3], b: [f64; 3], half_width: f64) -> bool {
        let margin = half_width + MARGIN;
        let low = [a[0].min(b[0]) - margin, a[2].min(b[2]) - margin];
        let high = [a[0].max(b[0]) + margin, a[2].max(b[2]) + margin];
        (cell(low[0])..=cell(high[0])).any(|x| {
            (cell(low[1])..=cell(high[1])).any(|z| {
                self.in_cell((x, z))
                    .iter()
                    .any(|&i| overlap(self.triangles[i], a, b, margin))
            })
        })
    }

    /// Remove complete offending barrier pieces, keeping slab sides and undersides closed.
    pub(crate) fn clear(&self, mesh: &mut MeshData, barriers: Vec<Barrier>, origin: [f32; 3]) {
        // A deck's exposed edge must also stay below a same-level junction's pavement.
        // Recess its top instead of deleting the slab wall and reopening its underside.
        for edge in barriers.iter().filter(|b| b.slab) {
            for &i in &mesh.indices[edge.indices.clone()] {
                let vertex = &mut mesh.vertices[i as usize];
                let point = [0, 1, 2].map(|k| f64::from(vertex[k]) + f64::from(origin[k]));
                if let Some(height) = self.recess(point) {
                    #[allow(clippy::cast_possible_truncation)] // f32 GPU vertices
                    {
                        vertex[1] = (height - f64::from(origin[1])) as f32;
                    }
                }
            }
        }
        let blocked: Vec<_> = barriers
            .into_iter()
            .filter(|b| !b.slab && self.crosses_width(b.a, b.b, b.half_width))
            .map(|b| b.indices)
            .collect();
        if blocked.is_empty() {
            return;
        }
        let mut blocked = blocked.iter().peekable();
        let mut output = MeshData::default();
        let mut remap = vec![u32::MAX; mesh.vertices.len()];
        for (i, face) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            while blocked.peek().is_some_and(|range| range.end <= i * 3) {
                blocked.next();
            }
            if blocked.peek().is_some_and(|range| range.contains(&(i * 3))) {
                continue;
            }
            for &old in face {
                let old = old as usize;
                if remap[old] == u32::MAX {
                    remap[old] = u32::try_from(output.vertices.len()).expect("mesh fits u32");
                    output.vertices.push(mesh.vertices[old]);
                    output.normals.push(mesh.normals[old]);
                    output.uvs.push(mesh.uvs[old]);
                    if !mesh.colors.is_empty() {
                        output.colors.push(mesh.colors[old]);
                    }
                }
                output.indices.push(remap[old]);
            }
        }
        output.shrink_to_fit();
        *mesh = output;
    }

    /// The triangles touching cell `key`. A pavement gathered for known checks
    /// ([`Pavement::near`]) holds nothing outside the cells asked for, so a check reaching
    /// beyond them would quietly find no pavement there: debug builds, and so the tests, stop.
    fn in_cell(&self, key: (i64, i64)) -> &[usize] {
        debug_assert!(
            self.only.as_ref().is_none_or(|only| only.contains(&key)),
            "pavement checked in cell {key:?}, which no barrier asked for"
        );
        self.cells.get(&key).map_or(&[], Vec::as_slice)
    }

    fn recess(&self, point: [f64; 3]) -> Option<f64> {
        self.in_cell((cell(point[0]), cell(point[2])))
            .iter()
            .filter_map(|&i| {
                let triangle = self.triangles[i];
                let area = cross(triangle[0], triangle[1], triangle[2]);
                let weights = [
                    cross(triangle[1], triangle[2], point) / area,
                    cross(triangle[2], triangle[0], point) / area,
                    cross(triangle[0], triangle[1], point) / area,
                ];
                if weights.into_iter().any(|w| w < -1e-8) {
                    return None;
                }
                let height = (0..3).map(|k| weights[k] * triangle[k][1]).sum::<f64>();
                (height < point[1] + 0.02 && point[1] - height <= 0.5).then_some(height - 0.02)
            })
            .reduce(f64::min)
    }
}

#[allow(clippy::many_single_char_names)] // vertices and interpolation in polygon clipping
fn overlap(triangle: [[f64; 3]; 3], a: [f64; 3], b: [f64; 3], thickness: f64) -> bool {
    if [0, 2].into_iter().any(|k| {
        triangle.iter().all(|p| p[k] < a[k].min(b[k]) - thickness)
            || triangle.iter().all(|p| p[k] > a[k].max(b[k]) + thickness)
    }) {
        return false;
    }
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let length = dx.hypot(dz);
    let along = if length > 1e-9 {
        [dx / length, dz / length]
    } else {
        [1.0, 0.0]
    };
    let side = [-along[1], along[0]];
    let rise = if length > 1e-9 {
        (b[1] - a[1]) / length
    } else {
        0.0
    };
    let mut polygon: Vec<_> = [
        (a, -1.0, -1.0),
        (b, 1.0, -1.0),
        (b, 1.0, 1.0),
        (a, -1.0, 1.0),
    ]
    .into_iter()
    .map(|(p, end, edge)| {
        [
            p[0] + (along[0] * end + side[0] * edge) * thickness,
            p[1] + rise * end * thickness,
            p[2] + (along[1] * end + side[1] * edge) * thickness,
        ]
    })
    .collect();
    let area = cross(triangle[0], triangle[1], triangle[2]);
    let sign = area.signum();
    for i in 0..3 {
        let (p, q) = (triangle[i], triangle[(i + 1) % 3]);
        let mut clipped = Vec::new();
        for j in 0..polygon.len() {
            let (a, b) = (polygon[j], polygon[(j + 1) % polygon.len()]);
            let (fa, fb) = (cross(p, q, a) * sign, cross(p, q, b) * sign);
            if fa >= 0.0 {
                clipped.push(a);
            }
            if (fa >= 0.0) != (fb >= 0.0) {
                let t = fa / (fa - fb);
                clipped.push([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t));
            }
        }
        if clipped.is_empty() {
            return false;
        }
        polygon = clipped;
    }
    let differences = polygon.iter().map(|&point| {
        let wa = cross(triangle[1], triangle[2], point) / area;
        let wb = cross(triangle[2], triangle[0], point) / area;
        point[1] - (wa * triangle[0][1] + wb * triangle[1][1] + (1.0 - wa - wb) * triangle[2][1])
    });
    let (low, high) = differences.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), d| {
        (low.min(d), high.max(d))
    });
    high >= -BARRIER_HEIGHT && low <= 0.5
}

fn cross(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (p[2] - a[2]) - (b[2] - a[2]) * (p[0] - a[0])
}

#[allow(clippy::cast_possible_truncation)] // bounded local world coordinates
fn cell(coordinate: f64) -> i64 {
    (coordinate / CELL).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road() -> Pavement {
        let mut road = Pavement::default();
        road.insert([[-1.0, 0.0, -2.0], [-1.0, 0.0, 2.0], [1.0, 0.0, 0.0]]);
        road
    }

    #[test]
    fn a_span_cannot_cut_a_road_between_clear_endpoints() {
        assert!(road().crosses([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0]));
        assert!(!road().crosses([-3.0, 0.0, 4.0], [3.0, 0.0, 4.0]));
    }

    #[test]
    fn overpass_protection_and_lower_fences_remain() {
        assert!(!road().crosses([-3.0, 6.0, 0.0], [3.0, 6.0, 0.0]));
        assert!(!road().crosses([-3.0, -6.0, 0.0], [3.0, -6.0, 0.0]));
        assert!(road().crosses([-3.0, -0.5, 0.0], [3.0, -0.5, 0.0]));
        assert!(road().crosses([-3.0, 0.4, 0.0], [3.0, 0.4, 0.0]));
    }

    #[test]
    fn sloping_pavement_uses_its_interpolated_height() {
        let mut road = Pavement::default();
        road.insert([[-1.0, 3.0, -2.0], [-1.0, 3.0, 2.0], [1.0, 5.0, 0.0]]);
        assert!(road.crosses([-3.0, 1.0, 0.0], [3.0, 7.0, 0.0]));
        assert!(!road.crosses([-3.0, 9.0, 0.0], [3.0, 15.0, 0.0]));
    }

    #[test]
    fn a_thin_pavement_fragment_does_not_swallow_a_fence_beside_it() {
        let mut road = Pavement::default();
        road.insert([[0.0, 0.0, 0.0], [100.0, 0.0, 0.001], [100.0, 0.0, 0.0]]);
        assert!(!road.crosses([40.0, 0.0, -0.6], [44.0, 0.0, -0.6]));
        assert!(road.crosses([40.0, 0.0, -0.6], [44.0, 0.0, 0.6]));
    }

    /// Pieces of road 2 km apart, each as in [`road`].
    fn spread(pavement: &mut Pavement) {
        for x in [0.0, 2_000.0] {
            pavement.insert([
                [x - 1.0, 0.0, -2.0],
                [x - 1.0, 0.0, 2.0],
                [x + 1.0, 0.0, 0.0],
            ]);
        }
    }

    fn slab(x: f32) -> MeshData {
        MeshData {
            vertices: vec![
                [x - 0.25, -0.7, -0.2],
                [x - 0.25, 0.1, -0.2],
                [x + 0.25, 0.1, 0.2],
                [x + 0.25, -0.7, 0.2],
            ],
            normals: vec![[1.0, 0.0, 0.0]; 4],
            uvs: vec![[0.0; 2]; 4],
            colors: vec![],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    #[test]
    fn pavement_gathered_for_known_checks_answers_them_as_the_whole_and_keeps_only_their_cells() {
        let mut whole = Pavement::default();
        spread(&mut whole);
        let fence = ([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0]);
        let clear_fence = ([-3.0, 0.0, 4.0], [3.0, 0.0, 4.0]);
        let flared = ([-3.0, 0.0, 2.3], [3.0, 0.0, 2.3]);
        let mut asked = Asked::default();
        asked.span(fence.0, fence.1, SPAN_HALF_WIDTH);
        asked.span(clear_fence.0, clear_fence.1, SPAN_HALF_WIDTH);
        asked.span(flared.0, flared.1, 0.5);
        let barrier = Barrier::slab(0..6);
        asked.barriers(&slab(0.0), std::slice::from_ref(&barrier), [0.0; 3]);
        let mut near = Pavement {
            only: Some(asked.0),
            ..Pavement::default()
        };
        spread(&mut near);

        assert_eq!(near.triangles.len(), 1, "the far piece is not kept");
        for (a, b, width) in [
            (fence.0, fence.1, SPAN_HALF_WIDTH),
            (clear_fence.0, clear_fence.1, SPAN_HALF_WIDTH),
            (flared.0, flared.1, 0.5),
        ] {
            assert_eq!(
                near.crosses_width(a, b, width),
                whole.crosses_width(a, b, width)
            );
        }
        assert!(near.crosses(fence.0, fence.1));
        assert!(near.crosses_width(flared.0, flared.1, 0.5));
        let (mut by_whole, mut by_near) = (slab(0.0), slab(0.0));
        whole.clear(&mut by_whole, vec![Barrier::slab(0..6)], [0.0; 3]);
        near.clear(&mut by_near, vec![barrier], [0.0; 3]);
        assert_eq!(
            by_near, by_whole,
            "the slab edge recedes as against the whole pavement"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "no barrier asked for")]
    fn a_check_beyond_the_cells_asked_for_stops_debug_builds() {
        let mut asked = Asked::default();
        asked.span([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], SPAN_HALF_WIDTH);
        let mut near = Pavement {
            only: Some(asked.0),
            ..Pavement::default()
        };
        spread(&mut near);
        near.contains(2_000.0, 0.0);
    }

    #[test]
    fn a_slab_edge_recedes_below_joining_pavement_without_opening_the_side() {
        let mut mesh = MeshData {
            vertices: vec![
                [-0.25, -0.7, -0.2],
                [-0.25, 0.1, -0.2],
                [0.25, 0.1, 0.2],
                [0.25, -0.7, 0.2],
            ],
            normals: vec![[1.0, 0.0, 0.0]; 4],
            uvs: vec![[0.0; 2]; 4],
            colors: vec![],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        road().clear(&mut mesh, vec![Barrier::slab(0..6)], [0.0; 3]);
        assert_eq!(
            mesh.indices.len(),
            6,
            "the slab remains closed below the road"
        );
        assert!(
            mesh.vertices.iter().all(|v| v[1] < 0.0),
            "no concrete edge protrudes above the carriageway"
        );
        assert!(
            mesh.vertices.iter().any(|v| v[1] < -0.69),
            "the bottom remains in place"
        );
    }
}
