//! Channels (#93): water lies a little below the land beside it, in a channel with natural
//! banks, not on the ground. Along streams and rivers and round lakes the ground is carved: the
//! water's edge half a metre down, banks sloping out from it, the bed deepening towards the
//! middle. The water's surface lies at the uncarved ground less that half metre
//! (`HeightGrid::level`). Where the road ridden, a street or a railway crosses on the ground,
//! the channel stops short of it, as at a culvert; under bridges it runs on.

use std::collections::HashMap;

use torqa_routes::Surface;

use crate::VERGE;
use crate::railways::Railway;
use crate::road::RoadIndex;
use crate::streets::Street;
use crate::water::{Pool, Stream};

/// The water's surface lies this far below the land beside it...
pub(crate) const DROP: f64 = 0.5;
/// ...the ground at the foot of the banks this far, and the banks reach this far out...
const BANK: f64 = 0.9;
const BANK_WIDTH: f64 = 2.5;
/// ...from this far inside the water's edge, so the ground meets the water at its edge.
const INSET: f64 = BANK_WIDTH * (1.0 - DROP / BANK);
/// Towards the middle the bed deepens by this share of the distance, at most `DEEPEST` more.
const DEEPENING: f64 = 0.5;
const DEEPEST: f64 = 1.0;
/// Channels stop this far short of the road ridden's verge and of streets and railways
/// crossing on the ground, rising to the land over `BANK_WIDTH`.
const CROSSING_MARGIN: f64 = 0.5;
/// Index cell size; larger than the reach of any channel.
const CELL: f64 = 50.0;

/// Where the water is: its edges, with the half width of streams (0 for shores).
pub(crate) struct Channels {
    edges: Vec<Edge>,
    cells: HashMap<(i64, i64), Vec<usize>>,
    crossings: Vec<((f64, f64), f64)>,
    crossing_cells: HashMap<(i64, i64), Vec<usize>>,
}

/// A piece of a stream's centre line (`half` its half width) or of a shore (`half` 0, `pool`
/// the lake or river it bounds).
struct Edge {
    a: (f64, f64),
    b: (f64, f64),
    half: f64,
    pool: Option<usize>,
}

impl Channels {
    pub(crate) fn new(
        streams: &[Stream],
        pools: &[Pool],
        streets: &[Street],
        railways: &[Railway],
    ) -> Self {
        let mut edges = Vec::new();
        for stream in streams {
            for pair in stream.points().windows(2) {
                edges.push(Edge {
                    a: pair[0],
                    b: pair[1],
                    half: stream.width() / 2.0,
                    pool: None,
                });
            }
        }
        for (index, pool) in pools.iter().enumerate() {
            let outline = pool.outline();
            for k in 0..outline.len() {
                edges.push(Edge {
                    a: outline[k],
                    b: outline[(k + 1) % outline.len()],
                    half: 0.0,
                    pool: Some(index),
                });
            }
        }
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let reach = BANK_WIDTH + 1.0;
        for (index, edge) in edges.iter().enumerate() {
            let low = (
                edge.a.0.min(edge.b.0) - edge.half - reach,
                edge.a.1.min(edge.b.1) - edge.half - reach,
            );
            let high = (
                edge.a.0.max(edge.b.0) + edge.half + reach,
                edge.a.1.max(edge.b.1) + edge.half + reach,
            );
            for ce in cell(low.0)..=cell(high.0) {
                for cn in cell(low.1)..=cell(high.1) {
                    cells.entry((ce, cn)).or_default().push(index);
                }
            }
        }
        // Streets and railways on the ground, by their points and half widths.
        let mut crossings = Vec::new();
        for street in streets.iter().filter(|s| !s.on_bridge()) {
            crossings.extend(street.points().iter().map(|&p| (p, street.half_width())));
        }
        for railway in railways.iter().filter(|r| !r.on_bridge()) {
            crossings.extend(
                railway
                    .points
                    .iter()
                    .map(|&p| (p, crate::railways::BED_M / 2.0)),
            );
        }
        let mut crossing_cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (index, &((e, n), _)) in crossings.iter().enumerate() {
            crossing_cells
                .entry((cell(e), cell(n)))
                .or_default()
                .push(index);
        }
        Self {
            edges,
            cells,
            crossings,
            crossing_cells,
        }
    }

    /// Whether a channel may change the ground within `reach` of (`east`, `north`).
    pub(crate) fn near(&self, east: f64, north: f64, reach: f64) -> bool {
        self.signed_distance(east, north, None)
            .is_some_and(|d| d < BANK_WIDTH - INSET + reach)
    }

    /// How far the ground at (`east`, `north`) lies below the land because of channels; 0 away
    /// from water. `inside` says whether the point lies in a lake or river mapped as an area.
    pub(crate) fn depth(&self, east: f64, north: f64, inside: bool, road: &RoadIndex) -> f64 {
        let distance = match self.signed_distance(east, north, Some(inside)) {
            Some(distance) => distance,
            // Deep inside a lake, far from its shores.
            None if inside => -1e9,
            None => return 0.0,
        };
        let bank = BANK * ((BANK_WIDTH - (distance + INSET)) / BANK_WIDTH).clamp(0.0, 1.0);
        let bed = (DEEPENING * -(distance + INSET)).clamp(0.0, DEEPEST);
        let carve = bank + bed;
        if carve <= 0.0 {
            return 0.0;
        }
        carve * self.open(east, north, road)
    }

    /// The distance from (`east`, `north`) to the water's edge, negative in the water, if the
    /// water lies within reach. `inside`: whether the point lies in a lake or river (when
    /// known; else only the distance to the edge counts).
    fn signed_distance(&self, east: f64, north: f64, inside: Option<bool>) -> Option<f64> {
        let indices = self.cells.get(&(cell(east), cell(north)))?;
        let mut best: Option<f64> = None;
        for &index in indices {
            let edge = &self.edges[index];
            let off = segment_distance((east, north), edge.a, edge.b) - edge.half;
            let signed = match (edge.pool, inside) {
                (Some(_), Some(true)) => -off,
                _ => off,
            };
            best = Some(best.map_or(signed, |b: f64| b.min(signed)));
        }
        best
    }

    /// 1 where a channel may cut the ground, 0 at the road ridden and at streets and railways
    /// crossing on the ground, in between over `BANK_WIDTH`.
    fn open(&self, east: f64, north: f64, road: &RoadIndex) -> f64 {
        let reach = VERGE + CROSSING_MARGIN + BANK_WIDTH;
        let road_gap = road
            .near(east, north, reach)
            .into_iter()
            .filter(|r| r.2 == Surface::Ground)
            .map(|r| r.0 - VERGE - CROSSING_MARGIN)
            .fold(f64::INFINITY, f64::min);
        let mut gap = road_gap;
        let (ce, cn) = (cell(east), cell(north));
        for x in ce - 1..=ce + 1 {
            for y in cn - 1..=cn + 1 {
                for &index in self.crossing_cells.get(&(x, y)).into_iter().flatten() {
                    let ((e, n), half) = self.crossings[index];
                    // Points lie a few metres apart: allow half a step along.
                    gap = gap.min((e - east).hypot(n - north) - half - CROSSING_MARGIN - 1.5);
                }
            }
        }
        (gap / BANK_WIDTH).clamp(0.0, 1.0)
    }
}

/// Distance from `p` to the segment `a`–`b`.
fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let length_squared = de * de + dn * dn;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * de + (p.1 - a.1) * dn) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - (a.0 + de * t)).hypot(p.1 - (a.1 + dn * t))
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell(metres: f64) -> i64 {
    (metres / CELL).floor() as i64
}
