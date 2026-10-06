//! Roundabouts (#74): the map draws them as a ring of road, the ground inside left as it was.
//! Rings (small, round, closed loops of road) get a raised island of grass inside, its kerb
//! steep enough for the terrain shader to draw it in stone, so the ring reads as one.

use torqa_osm::{LandCover, MapData, RoadClass};
use torqa_routes::LocalProjection;

use crate::{HeightGrid, MeshData, On, drape, landcover, streets};

/// Rings this small and large (radius to the road's middle, metres) are roundabouts.
const RADIUS: (f64, f64) = (6.0, 45.0);
/// Every point of a ring lies within this share of its radius from the mean: the map draws
/// roundabouts as near-circles, while a road round a square block strays 17 %.
const ROUND: f64 = 0.12;
/// The island stands this high over the ground and keeps this far off the road's edge.
const ISLAND_HEIGHT: f64 = 0.2;
const ISLAND_MARGIN: f64 = 0.4;
/// Islands smaller than this (radius, metres) are left out.
const SMALLEST_ISLAND: f64 = 2.0;

/// A roundabout's island: its outline, counter-clockwise, in metres east/north.
pub(crate) struct Island {
    outline: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
}

/// The islands of the map's roundabouts.
pub(crate) fn islands(map: &MapData, projection: &LocalProjection) -> Vec<Island> {
    let mut islands = Vec::new();
    // Roundabouts are roads; a path round a pond is none.
    let roads = map
        .roads
        .iter()
        .filter(|r| !matches!(r.class, RoadClass::Path | RoadClass::Track));
    for road in roads {
        let mut ring: Vec<(f64, f64)> = road
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let (Some(&first), Some(&last)) = (ring.first(), ring.last()) else {
            continue;
        };
        if ring.len() < 6 || (first.0 - last.0).hypot(first.1 - last.1) > 1.0 {
            continue;
        }
        ring.pop();
        #[allow(clippy::cast_precision_loss)] // a few dozen points
        let count = ring.len() as f64;
        let centre = (
            ring.iter().map(|p| p.0).sum::<f64>() / count,
            ring.iter().map(|p| p.1).sum::<f64>() / count,
        );
        let distances: Vec<f64> = ring
            .iter()
            .map(|p| (p.0 - centre.0).hypot(p.1 - centre.1))
            .collect();
        let radius = distances.iter().sum::<f64>() / count;
        let round = distances
            .iter()
            .all(|d| (d - radius).abs() <= ROUND * radius);
        if !round || !(RADIUS.0..=RADIUS.1).contains(&radius) {
            continue;
        }
        let inset = streets::width(road.class) / 2.0 + ISLAND_MARGIN;
        if radius - inset < SMALLEST_ISLAND {
            continue;
        }
        let mut outline: Vec<(f64, f64)> = ring
            .iter()
            .zip(&distances)
            .map(|(p, d)| {
                let scale = (d - inset).max(0.0) / d.max(1e-9);
                (
                    centre.0 + (p.0 - centre.0) * scale,
                    centre.1 + (p.1 - centre.1) * scale,
                )
            })
            .collect();
        let signed: f64 = outline
            .iter()
            .zip(outline.iter().cycle().skip(1))
            .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
            .sum();
        if signed < 0.0 {
            outline.reverse();
        }
        let (mut min, mut max) = (outline[0], outline[0]);
        for &(e, n) in &outline {
            min = (min.0.min(e), min.1.min(n));
            max = (max.0.max(e), max.1.max(n));
        }
        islands.push(Island { outline, min, max });
    }
    islands
}

/// The islands within the chunk square `[origin, origin + size]`, relative to `chunk_origin`,
/// for the chunk's ground mesh: their tops laid on the ground `ISLAND_HEIGHT` up, coloured as
/// meadow, and their kerbs down to below the ground.
#[allow(clippy::cast_possible_truncation)] // f32 GPU data
pub(crate) fn mesh(
    islands: &[Island],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    chunk_origin: [f64; 3],
) -> MeshData {
    let mut mesh = MeshData::default();
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
    let inside = |p: (f64, f64)| p.0 >= low.0 && p.0 < high.0 && p.1 >= low.1 && p.1 < high.1;
    for island in islands {
        // Each island belongs to the chunk its middle lies in.
        let middle = (
            f64::midpoint(island.min.0, island.max.0),
            f64::midpoint(island.min.1, island.max.1),
        );
        if !inside(middle) {
            continue;
        }
        let count = island.outline.len();
        // The top: a fan of triangles from the middle, clockwise seen from above.
        for k in 0..count {
            let (a, b) = (island.outline[k], island.outline[(k + 1) % count]);
            drape::drape_polygon(
                &mut mesh,
                &[middle, b, a],
                (On::Ground, ISLAND_HEIGHT),
                heights,
                chunk_origin,
                &|p| [(p.0 / 16.0) as f32, (p.1 / 16.0) as f32],
            );
        }
        // The kerb: upright all round, facing out.
        for k in 0..count {
            let (a, b) = (island.outline[k], island.outline[(k + 1) % count]);
            let at = |p: (f64, f64), up: f64| {
                [
                    (p.0 - chunk_origin[0]) as f32,
                    (heights.at(p.0, p.1) + up - chunk_origin[1]) as f32,
                    (-p.1 - chunk_origin[2]) as f32,
                ]
            };
            let length = (b.0 - a.0).hypot(b.1 - a.1).max(1e-9);
            // Counter-clockwise outline: outward is to the right of the way round.
            let out = [
                ((b.1 - a.1) / length) as f32,
                0.0,
                (-(a.0 - b.0) / length) as f32,
            ];
            let base = u32::try_from(mesh.vertices.len()).expect("chunk fits u32");
            for vertex in [
                at(a, -0.1),
                at(a, ISLAND_HEIGHT),
                at(b, ISLAND_HEIGHT),
                at(b, -0.1),
            ] {
                mesh.vertices.push(vertex);
                mesh.normals.push(out);
                mesh.uvs.push([0.0, 0.0]);
            }
            mesh.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let meadow = landcover::color(Some(LandCover::Meadow));
    mesh.colors = vec![meadow; mesh.vertices.len()];
    mesh
}
