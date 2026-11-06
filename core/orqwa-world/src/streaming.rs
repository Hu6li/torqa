//! Spatial upload order for a complete generated or kept world (R224).

use orqwa_domain::units::Meters;

use crate::{CHUNK_SIZE, World};

/// Chunks nearest a rider first, with every chunk touching the requested area in the prefix.
#[derive(Debug, PartialEq, Eq)]
pub struct ChunkPlan {
    /// Indices into [`World::chunks`], sorted by horizontal distance and then original index.
    pub indices: Vec<usize>,
    /// Number of leading indices needed to cover the requested area.
    pub nearby: usize,
}

/// Prioritizes uploads around a rider in metres east/north of the route start.
/// The half diagonal includes chunks whose centres are outside the area but whose ground
/// can still be visible. Height is ignored so valleys and hills cannot leave coverage gaps.
#[must_use]
pub fn chunk_plan(world: &World, east: Meters, north: Meters, radius: Meters) -> ChunkPlan {
    let mut distances: Vec<_> = world
        .chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let dx = f64::from(chunk.center[0]) - east.0;
            let dn = -f64::from(chunk.center[2]) - north.0;
            (index, dx.hypot(dn))
        })
        .collect();
    distances.sort_unstable_by(|(a, da), (b, db)| da.total_cmp(db).then_with(|| a.cmp(b)));
    let reach = radius.0.max(0.0) + CHUNK_SIZE * std::f64::consts::FRAC_1_SQRT_2;
    let nearby = distances.partition_point(|(_, distance)| *distance <= reach);
    ChunkPlan {
        indices: distances.into_iter().map(|(index, _)| index).collect(),
        nearby,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeshData, Props, TerrainChunk, Trees};

    fn world(centres: &[[f32; 3]]) -> World {
        World {
            chunks: centres
                .iter()
                .map(|&center| TerrainChunk {
                    center,
                    mesh: MeshData::default(),
                    buildings: MeshData::default(),
                    modelled: Vec::new(),
                    streets: MeshData::default(),
                    tracks: MeshData::default(),
                    urban: MeshData::default(),
                    water: MeshData::default(),
                    trees: Trees::default(),
                    props: Props::default(),
                })
                .collect(),
            ..World::default()
        }
    }

    #[test]
    fn start_surroundings_precede_far_chunks_on_every_side() {
        let world = world(&[
            [-9000.0, 0.0, 0.0],
            [-240.0, 0.0, 240.0],
            [240.0, 0.0, -240.0],
            [8000.0, 0.0, 0.0],
            [0.0, 0.0, -1000.0],
        ]);
        let unchanged = world.clone();
        let plan = chunk_plan(&world, Meters(0.0), Meters(0.0), Meters(1000.0));
        assert_eq!(plan.indices, [1, 2, 4, 3, 0]);
        assert_eq!(plan.nearby, 3);
        assert_eq!(world, unchanged);
    }

    #[test]
    fn a_visible_chunk_edge_counts_even_when_its_centre_is_outside() {
        let world = world(&[[1300.0, 0.0, 0.0], [1400.0, 0.0, 0.0]]);
        let plan = chunk_plan(&world, Meters(0.0), Meters(0.0), Meters(1000.0));
        assert_eq!(plan.nearby, 1);
    }

    #[test]
    fn moving_the_rider_changes_priority_without_waiting_for_distant_ground() {
        let world = world(&[[0.0, 0.0, 0.0], [0.0, 0.0, -7000.0], [0.0, 0.0, 8000.0]]);
        let plan = chunk_plan(&world, Meters(0.0), Meters(7200.0), Meters(1000.0));
        assert_eq!(plan.indices, [1, 0, 2]);
        assert_eq!(plan.nearby, 1);
    }

    #[test]
    fn a_short_course_and_an_empty_world_need_no_missing_chunks() {
        let world = world(&[[0.0, 0.0, 0.0], [480.0, 0.0, 0.0]]);
        let plan = chunk_plan(&world, Meters(0.0), Meters(0.0), Meters(5000.0));
        assert_eq!(plan.nearby, world.chunks.len());
        let empty = chunk_plan(&World::default(), Meters(0.0), Meters(0.0), Meters(5000.0));
        assert_eq!(empty.indices, Vec::<usize>::new());
        assert_eq!(empty.nearby, 0);
    }
}
