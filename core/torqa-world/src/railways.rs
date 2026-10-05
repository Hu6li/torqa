//! The railways of the map around the route (#75): a bed of ballast with sleepers and two rails
//! (drawn by the app's rail shader), laid on the ground of each chunk like the streets
//! (`drape`), or on bridges a straight deck between the ground at their ends. Tunnels are left
//! out (`torqa_osm`); plants keep off the tracks (`streets::Clearance`).

use torqa_osm::MapData;
use torqa_routes::{ElevationModel, LocalProjection};

use crate::{CORRIDOR, HeightGrid, MeshData, drape, road::RoadIndex};

/// Width of a track's bed of ballast.
pub(crate) const BED_M: f64 = 3.2;
/// The bed lies this far above the ground: below every street, which crosses it at level
/// crossings (`streets::lift`), above streams.
const LIFT: f64 = 0.03;
/// Points of a line at most this far apart: close enough to keep plants off it.
const STEP_M: f64 = 3.0;

/// A railway of the map near the route, in metres east/north, with its bounds for quick chunk
/// tests.
pub(crate) struct Railway {
    pub(crate) points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
    /// For bridges: the deck's height at both ends (the ground's there).
    deck: Option<(f64, f64)>,
}

/// The map's railways within the corridor around the road, densified, with bridge decks' end
/// heights from `model`.
pub(crate) async fn lines<M: ElevationModel>(
    map: &MapData,
    projection: &LocalProjection,
    road: &RoadIndex,
    model: &mut M,
) -> Vec<Railway> {
    let mut railways = Vec::new();
    for railway in &map.railways {
        let line: Vec<(f64, f64)> = railway
            .line
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect();
        let points = drape::densify(&line, STEP_M);
        if !points
            .iter()
            .any(|&(e, n)| road.nearest(e, n, CORRIDOR).is_some())
        {
            continue;
        }
        let Some(&first) = points.first() else {
            continue;
        };
        let deck = if railway.bridge {
            let mut end = async |(lat, lon): (f64, f64)| model.elevation(lat, lon).await.ok();
            let (start, finish) = (railway.line[0], railway.line[railway.line.len() - 1]);
            match (end(start).await, end(finish).await) {
                (Some(a), Some(b)) => Some((a, b)),
                // Without the ground's heights a deck cannot be placed.
                _ => continue,
            }
        } else {
            None
        };
        let (mut min, mut max) = (first, first);
        for &(e, n) in &points {
            min = (min.0.min(e), min.1.min(n));
            max = (max.0.max(e), max.1.max(n));
        }
        railways.push(Railway {
            points,
            min,
            max,
            deck,
        });
    }
    railways
}

/// The railways within the chunk square `[origin, origin + size]`, relative to `chunk_origin`.
/// Texture coordinates: `u` 0–1 across the bed, `v` metres along the line.
pub(crate) fn mesh(
    railways: &[Railway],
    origin: (f64, f64),
    size: f64,
    heights: &HeightGrid,
    chunk_origin: [f64; 3],
) -> MeshData {
    let mut mesh = MeshData::default();
    let (low, high) = (origin, (origin.0 + size, origin.1 + size));
    for railway in railways {
        if railway.max.0 < low.0
            || railway.min.0 > high.0
            || railway.max.1 < low.1
            || railway.min.1 > high.1
        {
            continue;
        }
        let total: f64 = railway
            .points
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum();
        for piece in drape::pieces(&railway.points, low, high) {
            if let Some(ends) = railway.deck {
                drape::deck(
                    &mut mesh,
                    &piece,
                    BED_M / 2.0,
                    ends,
                    LIFT,
                    total,
                    chunk_origin,
                );
            } else {
                drape::drape(&mut mesh, &piece, BED_M / 2.0, LIFT, heights, chunk_origin);
            }
        }
    }
    mesh
}
