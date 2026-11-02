//! Conservative clearance for a disposable snow blanket, derived from retained surfaces.

use crate::{MeshData, World};

/// Ground on which snow may gain depth, with roads and raised paving kept clear.
pub struct SnowGroundMask {
    /// North-west corner in Godot x/z metres.
    pub origin: [f32; 2],
    /// Cell edge in metres.
    pub cell: f32,
    /// Columns west to east.
    pub columns: usize,
    /// Rows north to south.
    pub rows: usize,
    /// R8 pixels: zero near ways, 255 on open ground.
    pub pixels: Vec<u8>,
}

/// A bounded, conservative mask of the actual retained road, streets, tracks and paving.
/// One-cell padding keeps interpolated snow depth from burying narrow ways or chunk joins.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // bounded 2048-cell grid
pub fn snow_ground_mask(world: &World) -> SnowGroundMask {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for point in world
        .road
        .vertices
        .iter()
        .copied()
        .chain(world.chunks.iter().flat_map(|chunk| {
            chunk
                .mesh
                .vertices
                .iter()
                .map(move |v| [v[0] + chunk.center[0], v[1], v[2] + chunk.center[2]])
        }))
    {
        for (axis, component) in [0, 2].into_iter().enumerate() {
            low[axis] = low[axis].min(point[component]);
            high[axis] = high[axis].max(point[component]);
        }
    }
    if !low[0].is_finite() {
        return SnowGroundMask {
            origin: [0.0; 2],
            cell: 16.0,
            columns: 0,
            rows: 0,
            pixels: Vec::new(),
        };
    }
    let cell = 16.0_f32.max((high[0] - low[0]).max(high[1] - low[1]) / 2044.0);
    let origin = low.map(|value| value - cell * 2.0);
    let columns = ((high[0] - origin[0]) / cell).ceil() as usize + 1;
    let rows = ((high[1] - origin[1]) / cell).ceil() as usize + 1;
    let mut mask = SnowGroundMask {
        origin,
        cell,
        columns,
        rows,
        pixels: vec![255; columns * rows],
    };
    mask.exclude(&world.road, [0.0; 3]);
    mask.exclude(&world.railways, [0.0; 3]);
    for chunk in &world.chunks {
        for mesh in [&chunk.streets, &chunk.tracks, &chunk.urban] {
            mask.exclude(mesh, chunk.center);
        }
    }
    mask
}

impl SnowGroundMask {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )] // padded bounds clamped to the image
    fn exclude(&mut self, mesh: &MeshData, origin: [f32; 3]) {
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let mut low = [f32::INFINITY; 2];
            let mut high = [f32::NEG_INFINITY; 2];
            for &index in triangle {
                let point = mesh.vertices[index as usize];
                for (axis, component) in [0, 2].into_iter().enumerate() {
                    let value =
                        (point[component] + origin[component] - self.origin[axis]) / self.cell;
                    low[axis] = low[axis].min(value.floor() - 1.0);
                    high[axis] = high[axis].max(value.ceil() + 1.0);
                }
            }
            let from_x = low[0].max(0.0).min(self.columns as f32) as usize;
            let to_x = high[0].max(0.0).min(self.columns as f32) as usize;
            let from_z = low[1].max(0.0).min(self.rows as f32) as usize;
            let to_z = high[1].max(0.0).min(self.rows as f32) as usize;
            for row in from_z..to_z {
                self.pixels[row * self.columns + from_x..row * self.columns + to_x].fill(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive points inside the test grid
    fn snow_depth_keeps_retained_roads_and_neighbouring_chunk_paving_exposed() {
        let mut world = World::default();
        world.road.vertices = vec![
            [495.0, 700.0, 0.0],
            [505.0, 700.0, 0.0],
            [500.0, 700.0, 1000.0],
        ];
        world.road.indices = vec![0, 1, 2];
        let chunk = crate::TerrainChunk {
            center: [100.0, 0.0, 100.0],
            mesh: MeshData {
                vertices: vec![[-100.0, 700.0, -100.0], [900.0, 700.0, 900.0]],
                ..MeshData::default()
            },
            streets: MeshData {
                vertices: vec![
                    [-100.0, 700.0, 395.0],
                    [900.0, 700.0, 400.0],
                    [-100.0, 700.0, 405.0],
                ],
                indices: vec![0, 1, 2],
                ..MeshData::default()
            },
            buildings: MeshData::default(),
            modelled: Vec::new(),
            tracks: MeshData::default(),
            urban: MeshData::default(),
            water: MeshData::default(),
            trees: crate::Trees::default(),
            props: crate::Props::default(),
        };
        world.chunks.push(chunk);
        let retained = world.road.clone();
        let mask = snow_ground_mask(&world);
        let at = |x: f32, z: f32| {
            let column = ((x - mask.origin[0]) / mask.cell) as usize;
            let row = ((z - mask.origin[1]) / mask.cell) as usize;
            mask.pixels[row * mask.columns + column]
        };
        assert_eq!(at(500.0, 200.0), 0);
        assert_eq!(at(200.0, 500.0), 0);
        assert_eq!(at(200.0, 200.0), 255);
        assert_eq!(world.road, retained);
        assert!(mask.columns <= 2048 && mask.rows <= 2048);
        assert_eq!(snow_ground_mask(&World::default()).pixels, Vec::<u8>::new());
    }
}
