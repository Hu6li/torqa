//! Spatial upload order for a complete generated or kept world (R224), from its chunks' centres
//! alone: the chunks themselves stay on disk until uploaded, and the scene lets go of those the
//! rider has left far behind (ADR 0040).

use orqwa_domain::units::Meters;

use crate::CHUNK_SIZE;

/// How far the rider (or the free camera) moves before the plan is made again. Chunks are let go
/// only beyond this and one chunk more than the upload radius, so a chunk at the edge isn't
/// dropped and uploaded again with every new plan, and none a plan needs is ever dropped before
/// the next.
pub const REPLAN_DISTANCE: Meters = Meters(240.0);

/// Chunks nearest a rider first, with every chunk touching the requested area in the prefix.
#[derive(Debug, PartialEq, Eq)]
pub struct ChunkPlan {
    /// Indices into [`World::chunks`](crate::World::chunks), sorted by horizontal distance and then original index.
    pub indices: Vec<usize>,
    /// Number of leading indices needed to cover the requested area.
    pub nearby: usize,
    /// Number of leading indices the scene keeps: the rest is dropped from it until the rider
    /// comes back within reach.
    pub kept: usize,
}

/// Prioritizes uploads around a rider in metres east/north of the route start.
/// The half diagonal includes chunks whose centres are outside the area but whose ground
/// can still be visible. Height is ignored so valleys and hills cannot leave coverage gaps.
#[must_use]
pub fn chunk_plan(centres: &[[f32; 3]], east: Meters, north: Meters, radius: Meters) -> ChunkPlan {
    let mut distances: Vec<_> = centres
        .iter()
        .enumerate()
        .map(|(index, center)| {
            let dx = f64::from(center[0]) - east.0;
            let dn = -f64::from(center[2]) - north.0;
            (index, dx.hypot(dn))
        })
        .collect();
    distances.sort_unstable_by(|(a, da), (b, db)| da.total_cmp(db).then_with(|| a.cmp(b)));
    let reach = radius.0.max(0.0) + CHUNK_SIZE * std::f64::consts::FRAC_1_SQRT_2;
    let nearby = distances.partition_point(|(_, distance)| *distance <= reach);
    let keep = radius.0.max(0.0) + REPLAN_DISTANCE.0 + CHUNK_SIZE;
    let kept = distances.partition_point(|(_, distance)| *distance <= keep);
    ChunkPlan {
        indices: distances.into_iter().map(|(index, _)| index).collect(),
        nearby,
        kept,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_surroundings_precede_far_chunks_on_every_side() {
        let centres = [
            [-9000.0, 0.0, 0.0],
            [-240.0, 0.0, 240.0],
            [240.0, 0.0, -240.0],
            [8000.0, 0.0, 0.0],
            [0.0, 0.0, -1000.0],
        ];
        let plan = chunk_plan(&centres, Meters(0.0), Meters(0.0), Meters(1000.0));
        assert_eq!(plan.indices, [1, 2, 4, 3, 0]);
        assert_eq!(plan.nearby, 3);
    }

    #[test]
    fn a_visible_chunk_edge_counts_even_when_its_centre_is_outside() {
        let centres = [[1300.0, 0.0, 0.0], [1400.0, 0.0, 0.0]];
        let plan = chunk_plan(&centres, Meters(0.0), Meters(0.0), Meters(1000.0));
        assert_eq!(plan.nearby, 1);
    }

    #[test]
    fn moving_the_rider_changes_priority_without_waiting_for_distant_ground() {
        let centres = [[0.0, 0.0, 0.0], [0.0, 0.0, -7000.0], [0.0, 0.0, 8000.0]];
        let plan = chunk_plan(&centres, Meters(0.0), Meters(7200.0), Meters(1000.0));
        assert_eq!(plan.indices, [1, 0, 2]);
        assert_eq!(plan.nearby, 1);
    }

    #[test]
    fn a_short_course_and_an_empty_world_need_no_missing_chunks() {
        let centres = [[0.0, 0.0, 0.0], [480.0, 0.0, 0.0]];
        let plan = chunk_plan(&centres, Meters(0.0), Meters(0.0), Meters(5000.0));
        assert_eq!(plan.nearby, centres.len());
        let empty = chunk_plan(&[], Meters(0.0), Meters(0.0), Meters(5000.0));
        assert_eq!(empty.indices, Vec::<usize>::new());
        assert_eq!(empty.nearby, 0);
        assert_eq!(empty.kept, 0);
    }

    #[test]
    fn chunks_left_far_behind_are_let_go_but_not_those_a_replan_may_need() {
        // Upload radius 1000 m: kept up to 1000 + 240 + 480 = 1720 m from the plan's focus.
        let centres = [
            [0.0, 0.0, 0.0],
            [1570.0, 0.0, 0.0],
            [1750.0, 0.0, 0.0],
            [0.0, 0.0, 9000.0],
        ];
        let plan = chunk_plan(&centres, Meters(0.0), Meters(0.0), Meters(1000.0));
        assert_eq!(plan.indices[..plan.kept], [0, 1]);
        // Just short of the next plan, towards the edge, the rider needs the chunk at 1570 m,
        // which was kept.
        let moved = chunk_plan(&centres, Meters(239.0), Meters(0.0), Meters(1000.0));
        assert_eq!(moved.indices[..moved.nearby], [0, 1]);
    }
}
