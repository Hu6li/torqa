//! Water: lakes and wide rivers mapped as areas, flat at their level; streams and rivers mapped
//! as lines, laid on the ground of each chunk.

use std::sync::LazyLock;

use torqa_osm::{Area, LandCover, Waterway};
use torqa_routes::{ElevationModel, LocalProjection};

use crate::buildings::{signed_area, triangulate};
use crate::minimap::simplify;
use crate::road::RoadIndex;
use crate::{CORRIDOR, HeightGrid, MeshData, drape, palette};

/// Lakes sit this far above the terrain sample, hiding the coarse terrain below them.
const SURFACE_OFFSET: f64 = 0.3;
/// Lake outlines are simplified to this tolerance in metres.
const SHORE_SIMPLIFY: f64 = 3.0;
/// Interior samples used to find a lake's surface level.
const LEVEL_SAMPLES: usize = 24;
/// Waterway lines are sampled this often.
const STEP: f64 = 10.0;
/// Colour of water; alpha 1 marks water for the shader.
static WATER: LazyLock<[f32; 4]> = LazyLock::new(|| palette::srgb("water.deep", 1.0));

/// Streams and rivers lie this far above the ground: below every street and the road ridden
/// (`streets::lift`, `ROAD_SINK`), so where they cross one they pass under it.
const STREAM_LIFT: f64 = 0.02;

/// A stream or river of the map near the route, in metres east/north, with its bounds for
/// quick chunk tests.
pub(crate) struct Stream {
    width: f64,
    points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
}

/// The map's streams and rivers, their stretches within the corridor around the road.
pub(crate) fn streams(
    waterways: &[Waterway],
    projection: &LocalProjection,
    road: &RoadIndex,
) -> Vec<Stream> {
    let mut streams = Vec::new();
    let mut add = |points: &mut Vec<(f64, f64)>, width: f64| {
        let taken = std::mem::take(points);
        let Some(&first) = taken.first() else {
            return;
        };
        if taken.len() < 2 {
            return;
        }
        let (mut min, mut max) = (first, first);
        for &(e, n) in &taken {
            min = (min.0.min(e), min.1.min(n));
            max = (max.0.max(e), max.1.max(n));
        }
        streams.push(Stream {
            width,
            points: taken,
            min,
            max,
        });
    };
    for waterway in waterways {
        let line: Vec<(f64, f64)> = waterway
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let mut run = Vec::new();
        for (east, north) in densify(&line) {
            if road.nearest(east, north, CORRIDOR).is_some() {
                run.push((east, north));
            } else {
                add(&mut run, waterway.width);
            }
        }
        add(&mut run, waterway.width);
    }
    streams
}

/// The streams within the chunk square `[origin, origin + size]`, relative to `chunk_origin`,
/// laid on the chunk's ground (see `drape`): water lies in the land, never floating above it
/// or sunk below it.
pub(crate) fn stream_mesh(
    streams: &[Stream],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    chunk_origin: [f64; 3],
) -> MeshData {
    let mut mesh = MeshData::default();
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
    for stream in streams {
        if stream.max.0 < low.0
            || stream.min.0 > high.0
            || stream.max.1 < low.1
            || stream.min.1 > high.1
        {
            continue;
        }
        for piece in drape::pieces(&stream.points, low, high) {
            drape::drape(
                &mut mesh,
                &piece,
                stream.width / 2.0,
                STREAM_LIFT,
                heights,
                chunk_origin,
            );
        }
    }
    mesh
}

/// Flat water surfaces for lakes, ponds and wide rivers mapped as areas near the road.
///
/// The terrain model measures the water surface itself, so the level is the median of samples
/// inside the area (shore samples would include banks).
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
pub(crate) async fn surfaces<M: ElevationModel>(
    areas: &[Area],
    projection: &LocalProjection,
    road: &RoadIndex,
    model: &mut M,
) -> MeshData {
    let mut mesh = MeshData::default();
    for area in areas.iter().filter(|a| a.cover == LandCover::Water) {
        for ring in &area.outer {
            let projected: Vec<(f64, f64)> = ring
                .iter()
                .map(|&(lat, lon)| projection.project(lat, lon))
                .collect();
            let near = projected
                .iter()
                .step_by((projected.len() / 32).max(1))
                .any(|&(e, n)| road.nearest(e, n, CORRIDOR).is_some());
            if !near {
                continue;
            }
            let mut outline = simplify(&projected, SHORE_SIMPLIFY);
            outline.pop(); // closing point
            if outline.len() < 3 {
                continue;
            }
            if signed_area(&outline) < 0.0 {
                outline.reverse();
            }
            let triangles = triangulate(&outline);
            let Some(level) = surface_level(&outline, &triangles, projection, model).await else {
                continue;
            };
            let base = u32::try_from(mesh.vertices.len()).expect("water mesh fits u32");
            for &(east, north) in &outline {
                mesh.vertices
                    .push([east as f32, (level + SURFACE_OFFSET) as f32, -north as f32]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.uvs.push([0.0, 0.0]);
                mesh.colors.push(*WATER);
            }
            for [a, b, c] in triangles {
                // Counter-clockwise outline seen from above; Godot's front faces are clockwise.
                mesh.indices.extend([base + c, base + b, base + a]);
            }
        }
    }
    mesh
}

/// Median terrain height at the centroids of the largest triangles.
async fn surface_level<M: ElevationModel>(
    outline: &[(f64, f64)],
    triangles: &[[u32; 3]],
    projection: &LocalProjection,
    model: &mut M,
) -> Option<f64> {
    let mut by_size: Vec<([f64; 2], f64)> = triangles
        .iter()
        .map(|t| {
            let corners = t.map(|i| outline[i as usize]);
            let centroid = [
                (corners[0].0 + corners[1].0 + corners[2].0) / 3.0,
                (corners[0].1 + corners[1].1 + corners[2].1) / 3.0,
            ];
            (centroid, signed_area(&corners).abs())
        })
        .collect();
    by_size.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut heights = Vec::new();
    for ([east, north], _) in by_size.into_iter().take(LEVEL_SAMPLES) {
        let (lat, lon) = projection.unproject(east, north);
        if let Ok(height) = model.elevation(lat, lon).await {
            heights.push(height);
        }
    }
    heights.sort_by(f64::total_cmp);
    heights.get(heights.len() / 2).copied()
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
