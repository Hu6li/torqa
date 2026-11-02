//! Disposable woodland coverage for autumn road litter, from the retained course maps.

use orqwa_routes::{RoutePoint, Surface};

use crate::{FlatMap, MapFeature, MeshData};

/// A bounded R8 woodland image in Godot x/z coordinates.
#[derive(Default)]
pub struct ForestMask {
    /// North-west image corner, in metres.
    pub origin: [f32; 2],
    /// Metres per pixel.
    pub cell: f32,
    /// Columns west to east.
    pub columns: usize,
    /// Rows north to south.
    pub rows: usize,
    /// Woodland coverage, zero outside forest and in clearings/water.
    pub pixels: Vec<u8>,
}

/// Rasterizes the broad map followed by detailed corridor areas. Road ribbons do not erase
/// the woods underneath; open-land holes and more specific natural covers do.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // bounded 2048-pixel dimensions
pub fn forest_mask(maps: &[&FlatMap]) -> ForestMask {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for point in maps.iter().flat_map(|map| &map.vertices) {
        for (axis, value) in [point[0], -point[1]].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    if !low[0].is_finite() {
        return ForestMask::default();
    }
    let cell = 4.0_f32.max((high[0] - low[0]).max(high[1] - low[1]) / 2044.0);
    let origin = low.map(|value| value - cell);
    let columns = ((high[0] - origin[0]) / cell).ceil() as usize + 1;
    let rows = ((high[1] - origin[1]) / cell).ceil() as usize + 1;
    let mut mask = ForestMask {
        origin,
        cell,
        columns,
        rows,
        pixels: vec![0; columns * rows],
    };
    for map in maps {
        for (triangle, features) in map
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(map.features.as_chunks::<3>().0)
        {
            if matches!(features[0], MapFeature::MinorRoad | MapFeature::MajorRoad) {
                continue;
            }
            let triangle = triangle.map(|[x, north]| [x, -north]);
            mask.paint(
                triangle,
                if features[0] == MapFeature::Forest {
                    255
                } else {
                    0
                },
            );
        }
    }
    mask
}

impl ForestMask {
    /// Removes tunnel footprints so leaves never appear inside the ridden tunnel tube.
    pub fn clear_tunnels(&mut self, road: &MeshData, points: &[RoutePoint]) {
        if self.pixels.is_empty() {
            return;
        }
        for triangle in road.indices.as_chunks::<3>().0 {
            let distance = triangle
                .iter()
                .map(|&i| f64::from(road.uvs[i as usize][1]))
                .sum::<f64>()
                / 3.0;
            let index = points
                .partition_point(|point| point.distance.0 <= distance)
                .saturating_sub(1);
            if points
                .get(index)
                .is_none_or(|point| point.surface != Surface::Tunnel)
            {
                continue;
            }
            let triangle = triangle.map(|i| {
                let point = road.vertices[i as usize];
                [point[0], point[2]]
            });
            let (from, to) = self.bounds(triangle, 8.0 + self.cell);
            for row in from[1]..to[1] {
                self.pixels[row * self.columns + from[0]..row * self.columns + to[0]].fill(0);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )] // clamped to image dimensions
    fn bounds(&self, triangle: [[f32; 2]; 3], margin: f32) -> ([usize; 2], [usize; 2]) {
        let limits = [self.columns, self.rows];
        let from = std::array::from_fn(|axis| {
            let low = triangle
                .iter()
                .map(|point| point[axis])
                .fold(f32::INFINITY, f32::min);
            (((low - margin - self.origin[axis]) / self.cell)
                .floor()
                .max(0.0) as usize)
                .min(limits[axis])
        });
        let to = std::array::from_fn(|axis| {
            let high = triangle
                .iter()
                .map(|point| point[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            (((high + margin - self.origin[axis]) / self.cell)
                .ceil()
                .max(0.0) as usize)
                .min(limits[axis])
        });
        (from, to)
    }

    #[allow(clippy::cast_precision_loss)] // at most 2048 pixels per axis
    fn paint(&mut self, triangle: [[f32; 2]; 3], value: u8) {
        let [a, b, c] = triangle;
        let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        };
        let area = cross(a, b, c);
        if area.abs() < 1e-6 {
            return;
        }
        let (from, to) = self.bounds(triangle, 0.0);
        for row in from[1]..to[1] {
            for column in from[0]..to[0] {
                let point = [
                    self.origin[0] + (column as f32 + 0.5) * self.cell,
                    self.origin[1] + (row as f32 + 0.5) * self.cell,
                ];
                if [cross(point, b, c), cross(point, c, a), cross(point, a, b)]
                    .iter()
                    .all(|side| side / area >= -1e-5)
                {
                    self.pixels[row * self.columns + column] = value;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // points within the test map
    fn road_litter_follows_woods_preserves_clearings_and_ignores_road_ink() {
        let mut map = FlatMap::default();
        for (triangle, feature) in [
            ([[0.0, 0.0], [100.0, 0.0], [0.0, 100.0]], MapFeature::Land),
            (
                [[20.0, 20.0], [80.0, 20.0], [20.0, 80.0]],
                MapFeature::Forest,
            ),
            ([[40.0, 40.0], [55.0, 40.0], [40.0, 55.0]], MapFeature::Land),
            (
                [[25.0, 20.0], [35.0, 20.0], [30.0, 40.0]],
                MapFeature::MinorRoad,
            ),
        ] {
            map.vertices.extend(triangle);
            map.features.extend([feature; 3]);
        }
        let mut mask = forest_mask(&[&map]);
        let at = |x: f32, north: f32| {
            let col = ((x - mask.origin[0]) / mask.cell) as usize;
            let row = ((-north - mask.origin[1]) / mask.cell) as usize;
            mask.pixels[row * mask.columns + col]
        };
        assert_eq!(at(30.0, 30.0), 255);
        assert_eq!(at(44.0, 44.0), 0);
        assert_eq!(at(90.0, 90.0), 0);
        assert!(mask.columns <= 2048 && mask.rows <= 2048);
        let column = ((30.0 - mask.origin[0]) / mask.cell) as usize;
        let row = ((-30.0 - mask.origin[1]) / mask.cell) as usize;
        let pixel = row * mask.columns + column;
        let road = MeshData {
            vertices: vec![
                [28.0, 700.0, -30.0],
                [32.0, 700.0, -30.0],
                [30.0, 700.0, -35.0],
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 10.0]],
            indices: vec![0, 1, 2],
            ..MeshData::default()
        };
        let mut point = RoutePoint {
            lat: 47.0,
            lon: 7.0,
            elevation: orqwa_domain::units::Meters(700.0),
            distance: orqwa_domain::units::Meters(0.0),
            surface: Surface::Ground,
        };
        mask.clear_tunnels(&road, &[point]);
        assert_eq!(mask.pixels[pixel], 255);
        point.surface = Surface::Tunnel;
        mask.clear_tunnels(&road, &[point]);
        assert_eq!(mask.pixels[pixel], 0);
        assert!(mask.pixels.contains(&255));
        assert_eq!(forest_mask(&[]).pixels, Vec::<u8>::new());
    }
}
