//! Rivers and streams.

use torqa_osm::Waterway;
use torqa_routes::{ElevationModel, LocalProjection};

use crate::road::RoadIndex;
use crate::{CORRIDOR, MeshData};

/// Water sits this far above the terrain sample, hiding the coarse terrain below it.
const SURFACE_OFFSET: f64 = 0.3;
/// Waterway lines are sampled this often.
const STEP: f64 = 10.0;
/// Colour of water; alpha 1 marks water for the shader.
const WATER: [f32; 4] = [0.10, 0.22, 0.30, 1.0];

/// Ribbons for the waterways' parts within the corridor around the road.
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
pub(crate) async fn ribbons<M: ElevationModel>(
    waterways: &[Waterway],
    projection: &LocalProjection,
    road: &RoadIndex,
    model: &mut M,
) -> MeshData {
    let mut mesh = MeshData::default();
    for waterway in waterways {
        let line: Vec<(f64, f64)> = waterway
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let mut run: Vec<(f64, f64, f64)> = Vec::new();
        for (east, north) in densify(&line) {
            if road.nearest(east, north, CORRIDOR).is_none() {
                add_ribbon(&mut mesh, &run, waterway.width);
                run.clear();
                continue;
            }
            let (lat, lon) = projection.unproject(east, north);
            let Ok(height) = model.elevation(lat, lon).await else {
                add_ribbon(&mut mesh, &run, waterway.width);
                run.clear();
                continue;
            };
            run.push((east, north, height + SURFACE_OFFSET));
        }
        add_ribbon(&mut mesh, &run, waterway.width);
    }
    mesh
}

/// Points along the line at most [`STEP`] apart.
fn densify(line: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = (length / STEP).ceil().max(1.0) as usize;
        for k in 0..steps {
            #[allow(clippy::cast_precision_loss)]
            let t = k as f64 / steps as f64;
            points.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    if let Some(&last) = line.last() {
        points.push(last);
    }
    points
}

#[allow(clippy::cast_possible_truncation)]
fn add_ribbon(mesh: &mut MeshData, run: &[(f64, f64, f64)], width: f64) {
    if run.len() < 2 {
        return;
    }
    let half = width / 2.0;
    for (i, &(east, north, height)) in run.iter().enumerate() {
        let (from, to) = (run[i.saturating_sub(1)], run[(i + 1).min(run.len() - 1)]);
        let (de, dn) = (to.0 - from.0, to.1 - from.1);
        let length = de.hypot(dn).max(f64::EPSILON);
        // Right of the flow direction.
        let (re, rn) = (dn / length, -de / length);
        for side in [-1.0, 1.0] {
            let (e, n) = (east + re * half * side, north + rn * half * side);
            mesh.vertices.push([e as f32, height as f32, -n as f32]);
            mesh.normals.push([0.0, 1.0, 0.0]);
            mesh.uvs.push([0.0, 0.0]);
            mesh.colors.push(WATER);
        }
        if i > 0 {
            let base = u32::try_from(mesh.vertices.len() - 4).expect("water mesh fits u32");
            let (left_0, right_0, left_1, right_1) = (base, base + 1, base + 2, base + 3);
            mesh.indices
                .extend([left_0, left_1, right_1, left_0, right_1, right_0]);
        }
    }
}
