//! Puts a recorded track onto the roads it rides (map matching) and smooths it. GPS positions
//! wander a few metres either side of the road and sparse files cut corners between their
//! points; the route drawn and ridden should follow the road, and above all look like one: no
//! hops onto side streets, sidewalks or parallel roads, no corners cut at junctions, no kinks.
//! Looking natural matters more than lying exactly on the map's lines.
//!
//! The road of every point is chosen for the whole track at once (the cheapest sequence, as in
//! a hidden Markov model): near roads are cheap, roads running across the track, footpaths and
//! lesser roads right beside bigger ones cost extra, and so does changing to another road unless
//! the two meet.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use orqwa_osm::{Road, RoadClass, Structure};

use crate::gpx::RawPoint;

/// Roads further than this from a point are not considered for it.
const MAX_SNAP_M: f64 = 20.0;
/// What leaving a point where it was recorded costs, in metres of distance to a road.
const OFF_ROAD_COST: f64 = 14.0;
/// Going on or off road.
const LEAVE_COST: f64 = 4.0;
/// Changing to a road that does not meet the current one: a rider cannot hop to a parallel
/// road or a sidewalk, so only overwhelming evidence makes the track do it.
const HOP_COST: f64 = 60.0;
/// Continuing onto a road that meets the current one (ways of one street, or a turn).
const TURN_COST: f64 = 1.0;
/// Extra cost of a road running across the track rather than along it, at right angles.
const CROSSING_COST: f64 = 14.0;
/// Extra cost of a lesser road (a service road, track or path) running alongside a bigger one
/// within `SIDE_REACH_M`: frontage roads, car park aisles and cycle paths are mapped right
/// beside the road, GPS wanders onto them, and every hop on and off draws an S-bend (#74). Only
/// a track clearly following the lesser road takes it.
const SIDE_ROAD_COST: f64 = 5.0;
const SIDE_REACH_M: f64 = 15.0;
/// Roads within this angle (cosine) of each other run alongside.
const ALONGSIDE: f64 = 0.9;
/// A road detour between two points much longer than the straight line is not what was ridden
/// (e.g. a loop of the road between them): then the points are joined straight.
const MAX_DETOUR: f64 = 2.5;
/// Points off every road between two points on roads the map connects are not what was ridden:
/// a receiver that loses its fix in a tunnel records them wandering until it finds the sky
/// again, and a planner draws a tunnel or a lakeside by its own old lines (#166). The road
/// between the two was ridden, and the line follows the map there, unless the points run more
/// than this many times the road's way: a real detour off the map, which stays as recorded.
const OFF_ROAD_DETOUR: f64 = 6.0;
/// Vertices of different roads this close to each other are one place: OpenStreetMap ways
/// share nodes at junctions, and pieces of a way from neighbouring tiles share their ends.
const JOIN_M: f64 = 1.0;
/// Where the matched line turns back on itself by more than this (degrees) within `FOLD_M`, it
/// has stepped back to a junction a few metres behind a matched point, or forward past the
/// next: a fold no rider rides, taken out (#172). Smoothing goes by points, so a fold's short
/// chord would carry a whole step's rise and read as a wall. Off the map the file's points
/// stay as recorded: a turn back on the spot there is the rider's (#101).
const FOLD_TURN: f64 = 150.0;
const FOLD_M: f64 = 5.0;
/// Roundabouts are rings of road this small and large (radius to the road's middle, metres),
/// every point within this share of the radius from the mean, as the world finds them for
/// their islands; made of at most this many ways (the map splits some, tiles cut others at
/// their borders). The map draws them the way traffic goes round, counter-clockwise where it
/// keeps right and clockwise where it keeps left, but rarely marks them one-way.
const ROUNDABOUT_RADIUS: (f64, f64) = (6.0, 45.0);
const ROUNDABOUT_ROUND: f64 = 0.12;
const ROUNDABOUT_PIECES: usize = 8;
/// A point matched at most this far back round a roundabout from the last is GPS wander, not
/// a lap of it.
const WANDER_M: f64 = 5.0;
/// A point of the line this close to a roundabout's road is on it (metres).
const ON_RING_M: f64 = 0.5;
/// A point matched this close to a roundabout's road, on a road leading off it, is at it
/// (metres).
const AT_RING_M: f64 = 3.0;

/// Points of the finished line are at most this far apart...
const STEP_M: f64 = 5.0;
/// ...and it is smoothed so it bends like a road, no point moving further than this from where
/// it was put: corners round off, the route stays on its road.
const MAX_SHIFT_M: f64 = 2.5;
/// Pairs of smoothing passes; each pair shrinks and re-inflates the line (Taubin), which takes
/// out kinks and jitter but keeps long bends as they are.
const SMOOTHING_PAIRS: usize = 8;
/// Short excursions to one side and back are ridden straight. Where a street splits around a
/// traffic island the map draws a one-way branch either side, and the track follows one of
/// them out and back: a kink no rider rides. An excursion is at most this long...
const EXCURSION_MAX_M: f64 = 80.0;
/// ...leaves the line by this much at most (wider ones are real roads)...
const EXCURSION_OFFSET_M: f64 = 8.0;
/// ...leaves and rejoins it in its direction, within this angle (radians)...
const EXCURSION_ALIGNED: f64 = 0.06;
/// ...and turns away by at least this much on the way (gentle bends never do).
const EXCURSION_TURN: f64 = 0.15;
/// Spatial index cell size.
const CELL_M: f64 = 50.0;

const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Footpaths and tracks are taken only where the track clearly follows them: sidewalks and
/// cycle lanes are often mapped beside streets.
fn class_cost(class: RoadClass) -> f64 {
    match class {
        RoadClass::Path => 5.0,
        RoadClass::Track => 2.0,
        RoadClass::Major | RoadClass::Street | RoadClass::Service => 0.0,
    }
}

/// A track put on the roads it rides.
#[derive(Debug, Default)]
pub(crate) struct Snapped {
    pub(crate) track: Vec<RawPoint>,
    /// The bridges and tunnels among the roads it was put on.
    pub(crate) structures: Vec<Structure>,
}

/// A flat projection around the track, accurate to a fraction of a metre over a route.
struct Flat {
    lat0: f64,
    lon0: f64,
    cos_lat0: f64,
}

impl Flat {
    fn new(lat0: f64, lon0: f64) -> Self {
        Self {
            lat0,
            lon0,
            cos_lat0: lat0.to_radians().cos(),
        }
    }

    fn to_xy(&self, lat: f64, lon: f64) -> (f64, f64) {
        (
            crate::east_of(self.lon0, lon).to_radians() * self.cos_lat0 * EARTH_RADIUS_M,
            (lat - self.lat0).to_radians() * EARTH_RADIUS_M,
        )
    }

    fn to_lat_lon(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.lat0 + (y / EARTH_RADIUS_M).to_degrees(),
            crate::wrapped(self.lon0 + (x / (EARTH_RADIUS_M * self.cos_lat0)).to_degrees()),
        )
    }
}

/// Where a point lies on a road: road, segment and position along that segment (0–1).
#[derive(Debug, Clone, Copy)]
struct Match {
    road: usize,
    segment: usize,
    along: f64,
    at: (f64, f64),
}

/// A point of the line being built, with where it was put (smoothing keeps it near there).
#[derive(Debug, Clone, Copy)]
struct Node {
    at: (f64, f64),
    anchor: (f64, f64),
    elevation: Option<f64>,
    time: Option<f64>,
    /// On a road, rather than where a point off road was recorded.
    on_road: bool,
}

/// The track on the roads it rides, smoothed, and the bridges and tunnels it uses.
pub(crate) fn to_roads(track: &[RawPoint], roads: &[Road]) -> Snapped {
    let Some(first) = track.first() else {
        return Snapped::default();
    };
    let flat = Flat::new(first.lat, first.lon);
    let positions: Vec<(f64, f64)> = track.iter().map(|p| flat.to_xy(p.lat, p.lon)).collect();
    let lines: Vec<Vec<(f64, f64)>> = roads
        .iter()
        .map(|r| {
            r.line
                .iter()
                .map(|&(lat, lon)| flat.to_xy(lat, lon))
                .collect()
        })
        .collect();
    let network = Network::new(lines, roads);
    let matches = network.choose(&positions);
    let kept = network.on_the_map(&matches, &positions);
    let kept = network.once_round(&kept, &matches);
    let mut used: Vec<usize> = matches.iter().flatten().map(|m| m.road).collect();

    let mut nodes: Vec<Node> = Vec::with_capacity(track.len() * 2);
    let mut previous: Option<usize> = None;
    for &i in &kept {
        let point = &track[i];
        let at = matches[i].map_or(positions[i], |m| m.at);
        if let Some(p) = previous
            && let (Some(from), Some(to)) = (matches[p], matches[i])
        {
            network.connect((&mut nodes, &mut used), from, to, (&track[p], point));
        }
        previous = Some(i);
        nodes.push(Node {
            at,
            anchor: at,
            elevation: point.elevation,
            time: point.time,
            on_road: matches[i].is_some(),
        });
    }
    remove_folds(&mut nodes);
    let mut nodes = densify(&nodes);
    straighten_excursions(&mut nodes);
    let nodes = smooth(&nodes);

    used.sort_unstable();
    used.dedup();
    Snapped {
        track: nodes
            .iter()
            .map(|n| {
                let (lat, lon) = flat.to_lat_lon(n.at);
                RawPoint {
                    lat,
                    lon,
                    elevation: n.elevation,
                    time: n.time,
                }
            })
            .collect(),
        structures: used
            .into_iter()
            .filter_map(|r| {
                roads[r].structure.map(|kind| Structure {
                    kind,
                    line: roads[r].line.clone(),
                })
            })
            .collect(),
    }
}

/// Where two roads meet, with the positions there on each.
type Meeting = ((f64, f64), Match, Match);
/// A way along roads as points, with the roads it passes onto, if any.
type Way = (Vec<(f64, f64)>, Vec<usize>);

/// A vertex of a road in the network's graph: the road and the vertex's index on it.
type Vertex = (usize, usize);

/// A way between two positions on roads (`Network::shortest`): the points between them, the
/// roads it passes onto, how long it is and how much of that runs the wrong way round
/// roundabouts.
struct Passage {
    through: Vec<(f64, f64)>,
    onto: Vec<usize>,
    length: f64,
    wrong: f64,
}

/// How the shortest-way search reached a vertex, or `to`: from the vertex before, if any.
#[derive(Clone, Copy)]
enum Step {
    Start,
    From(Vertex),
    /// Along `to`'s own road from `from`, on it too.
    Along,
}

/// A vertex reached by the shortest-way search, ordered for the heap by the way to it.
#[derive(PartialEq)]
struct Reached {
    cost: f64,
    vertex: Vertex,
}

impl Eq for Reached {}

impl Ord for Reached {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for Reached {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The roads, projected and indexed.
struct Network<'a> {
    lines: Vec<Vec<(f64, f64)>>,
    roads: &'a [Road],
    index: HashMap<(i64, i64), Vec<(usize, usize)>>,
    /// The vertices of other roads at each vertex (`shortest`): junctions, and the ends of a
    /// way's pieces; and a roundabout of one closed way's last vertex at its first.
    joins: HashMap<Vertex, Vec<Vertex>>,
    /// Whether each road goes round a roundabout, one way: as drawn.
    roundabout: Vec<bool>,
    /// The roundabouts, each as its roads.
    rings: Vec<Vec<usize>>,
    /// The roundabout each road goes round, if any (in `rings`).
    ring_of: Vec<Option<usize>>,
    /// The longest way once round a roundabout.
    longest_lap: f64,
}

impl<'a> Network<'a> {
    fn new(lines: Vec<Vec<(f64, f64)>>, roads: &'a [Road]) -> Self {
        let mut index: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        for (road, line) in lines.iter().enumerate() {
            for segment in 0..line.len().saturating_sub(1) {
                let (a, b) = (line[segment], line[segment + 1]);
                let (x0, x1) = (cell(a.0.min(b.0)), cell(a.0.max(b.0)));
                let (y0, y1) = (cell(a.1.min(b.1)), cell(a.1.max(b.1)));
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        index.entry((x, y)).or_default().push((road, segment));
                    }
                }
            }
        }
        #[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
        let place = |(x, y): (f64, f64)| ((x / JOIN_M).floor() as i64, (y / JOIN_M).floor() as i64);
        let mut by_place: HashMap<(i64, i64), Vec<Vertex>> = HashMap::new();
        for (road, line) in lines.iter().enumerate() {
            for (vertex, &point) in line.iter().enumerate() {
                by_place
                    .entry(place(point))
                    .or_default()
                    .push((road, vertex));
            }
        }
        let mut joins: HashMap<Vertex, Vec<Vertex>> = HashMap::new();
        for (road, line) in lines.iter().enumerate() {
            for (vertex, &point) in line.iter().enumerate() {
                let (cx, cy) = place(point);
                let near: Vec<Vertex> = (cx - 1..=cx + 1)
                    .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
                    .flat_map(|cell| by_place.get(&cell).into_iter().flatten().copied())
                    .filter(|&(other, v)| {
                        other != road && distance(lines[other][v], point) <= JOIN_M
                    })
                    .collect();
                if !near.is_empty() {
                    joins.insert((road, vertex), near);
                }
            }
        }
        // Rings the map detail tells, drawn the way traffic goes (ADR 0033); elsewhere those
        // their shape tells.
        let mut roundabout: Vec<bool> = roads.iter().map(|road| road.details.roundabout).collect();
        let mut rings = mapped_rings(&lines, &roundabout, &mut joins);
        for ring in roundabouts(&lines, roads, &joins) {
            // A ring the map detail tells is one already.
            if ring.iter().all(|&road| roundabout[road]) {
                continue;
            }
            if let &[road] = ring.as_slice() {
                let last = lines[road].len() - 1;
                joins.entry((road, last)).or_default().push((road, 0));
                joins.entry((road, 0)).or_default().push((road, last));
            }
            for &road in &ring {
                roundabout[road] = true;
            }
            rings.push(ring);
        }
        let mut ring_of = vec![None; lines.len()];
        for (ring, roads) in rings.iter().enumerate() {
            for &road in roads {
                ring_of[road] = Some(ring);
            }
        }
        let longest_lap = rings
            .iter()
            .map(|ring| ring.iter().map(|&road| path_length(&lines[road])).sum())
            .fold(0.0, f64::max);
        Self {
            lines,
            roads,
            index,
            joins,
            roundabout,
            rings,
            ring_of,
            longest_lap,
        }
    }

    /// The `kept` points, but for a run of them matched on one roundabout that would take the
    /// line round it more than once between the points before and after the run: such points
    /// lie behind where the line came onto the ring, as where a file runs beside it rather
    /// than round it. Without them the line leaves the ring the first time it passes its way
    /// out; a turn back at a roundabout, once round less a little, stays as matched.
    fn once_round(&self, kept: &[usize], matches: &[Option<Match>]) -> Vec<usize> {
        // On the ring, or at it on a road leading off it: a point put on the end of a side
        // street is as much at the roundabout as one on its ring.
        let ring_at = |i: usize| {
            let m = matches[i]?;
            self.ring_of[m.road]
                .or_else(|| (0..self.rings.len()).find(|&ring| self.at_ring(ring, m.at, AT_RING_M)))
        };
        let mut once = Vec::with_capacity(kept.len());
        let mut k = 0;
        while k < kept.len() {
            let Some(ring) = ring_at(kept[k]) else {
                once.push(kept[k]);
                k += 1;
                continue;
            };
            let start = k;
            while k < kept.len() && ring_at(kept[k]) == Some(ring) {
                k += 1;
            }
            let run = &kept[start..k];
            let lap: f64 = self.rings[ring]
                .iter()
                .map(|&road| path_length(&self.lines[road]))
                .sum();
            let round = match (start.checked_sub(1), kept.get(k)) {
                (Some(before), Some(&after)) => {
                    let through: Vec<usize> = std::iter::once(kept[before])
                        .chain(run.iter().copied())
                        .chain(std::iter::once(after))
                        .collect();
                    self.round(ring, &through, matches)
                }
                _ => 0.0,
            };
            if round <= lap + WANDER_M {
                once.extend_from_slice(run);
            }
        }
        once
    }

    /// Whether `p` lies within `reach` of `ring`'s roads.
    fn at_ring(&self, ring: usize, p: (f64, f64), reach: f64) -> bool {
        self.rings[ring].iter().any(|&road| {
            self.lines[road]
                .windows(2)
                .any(|s| distance(project(p, s[0], s[1]).0, p) <= reach)
        })
    }

    /// How far the line from point to point `through` goes round `ring`.
    fn round(&self, ring: usize, through: &[usize], matches: &[Option<Match>]) -> f64 {
        let on_ring = |p: (f64, f64)| self.at_ring(ring, p, ON_RING_M);
        let mut round = 0.0;
        for pair in through.windows(2) {
            let (Some(from), Some(to)) = (matches[pair[0]], matches[pair[1]]) else {
                continue;
            };
            let Some((path, _)) = self.way(from, to) else {
                continue;
            };
            round += path
                .windows(2)
                .filter(|s| on_ring(s[0]) && on_ring(s[1]))
                .map(|s| distance(s[0], s[1]))
                .sum::<f64>();
        }
        round
    }

    /// The closest point of every road within reach of `point`.
    fn candidates(&self, point: (f64, f64)) -> Vec<Match> {
        let (cx, cy) = (cell(point.0), cell(point.1));
        let mut best: HashMap<usize, (f64, Match)> = HashMap::new();
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &(road, segment) in self.index.get(&(x, y)).into_iter().flatten() {
                    let line = &self.lines[road];
                    let (at, along) = project(point, line[segment], line[segment + 1]);
                    let distance = (at.0 - point.0).hypot(at.1 - point.1);
                    if distance > MAX_SNAP_M {
                        continue;
                    }
                    let candidate = Match {
                        road,
                        segment,
                        along,
                        at,
                    };
                    best.entry(road)
                        .and_modify(|b| {
                            if distance < b.0 {
                                *b = (distance, candidate);
                            }
                        })
                        .or_insert((distance, candidate));
                }
            }
        }
        let mut found: Vec<Match> = best.into_values().map(|(_, m)| m).collect();
        // A deterministic order, whatever the hash map did.
        found.sort_by_key(|m| (m.road, m.segment));
        found
    }

    /// The road of every point (or none), the cheapest for the whole track.
    fn choose(&self, positions: &[(f64, f64)]) -> Vec<Option<Match>> {
        let states: Vec<Vec<(Option<Match>, f64)>> = positions
            .iter()
            .enumerate()
            .map(|(i, &point)| {
                let heading = heading(positions, i);
                let candidates: Vec<(Match, (f64, f64))> = self
                    .candidates(point)
                    .into_iter()
                    .map(|m| (m, self.direction(m)))
                    .collect();
                let mut options = vec![(None, OFF_ROAD_COST)];
                for &(m, direction) in &candidates {
                    let across = heading.map_or(0.0, |h| {
                        CROSSING_COST * (1.0 - (h.0 * direction.0 + h.1 * direction.1).abs())
                    });
                    let class = self.roads[m.road].class;
                    let beside_bigger = candidates.iter().any(|&(other, way)| {
                        self.roads[other.road].class < class
                            && distance(other.at, m.at) < SIDE_REACH_M
                            && (way.0 * direction.0 + way.1 * direction.1).abs() > ALONGSIDE
                    });
                    let side = if beside_bigger { SIDE_ROAD_COST } else { 0.0 };
                    let cost = distance(m.at, point) + class_cost(class) + across + side;
                    options.push((Some(m), cost));
                }
                options
            })
            .collect();

        let mut meet_cache: HashMap<(usize, usize), bool> = HashMap::new();
        let mut cost: Vec<f64> = states[0].iter().map(|s| s.1).collect();
        let mut back: Vec<Vec<usize>> = vec![Vec::new()];
        for i in 1..states.len() {
            let mut next_cost = Vec::with_capacity(states[i].len());
            let mut next_back = Vec::with_capacity(states[i].len());
            for (state, emission) in &states[i] {
                let mut best = (0, f64::INFINITY);
                for (p, (previous, _)) in states[i - 1].iter().enumerate() {
                    let total = cost[p] + self.transition(*previous, *state, &mut meet_cache);
                    if total < best.1 {
                        best = (p, total);
                    }
                }
                next_back.push(best.0);
                next_cost.push(best.1 + emission);
            }
            cost = next_cost;
            back.push(next_back);
        }
        let mut chosen = vec![None; states.len()];
        let mut state = cost
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(s, _)| s);
        for i in (0..states.len()).rev() {
            chosen[i] = states[i][state].0;
            if i > 0 {
                state = back[i][state];
            }
        }
        chosen
    }

    fn transition(
        &self,
        from: Option<Match>,
        to: Option<Match>,
        meet_cache: &mut HashMap<(usize, usize), bool>,
    ) -> f64 {
        match (from, to) {
            (Some(a), Some(b)) if a.road == b.road => 0.0,
            (Some(a), Some(b)) => {
                let key = (a.road.min(b.road), a.road.max(b.road));
                let meet = *meet_cache
                    .entry(key)
                    .or_insert_with(|| self.meet(a.road, b.road, None).is_some());
                if meet { TURN_COST } else { HOP_COST }
            }
            (None, None) => 0.0,
            _ => LEAVE_COST,
        }
    }

    /// Where roads `a` and `b` meet, closest to `near` if given: a vertex of one on the other
    /// (OpenStreetMap ways share nodes at junctions; pieces of a way from neighbouring tiles
    /// share theirs). With the positions there on `a` and on `b`.
    fn meet(&self, a: usize, b: usize, near: Option<(f64, f64)>) -> Option<Meeting> {
        let mut best: Option<(f64, Meeting)> = None;
        for (first, second) in [(a, b), (b, a)] {
            let line = &self.lines[first];
            for (vertex, &point) in line.iter().enumerate() {
                let Some(on_second) = self.closest_on(second, point) else {
                    continue;
                };
                if distance(on_second.at, point) > 1.0 {
                    continue;
                }
                let on_first = Match {
                    road: first,
                    segment: vertex.min(line.len().saturating_sub(2)),
                    along: if vertex + 1 == line.len() { 1.0 } else { 0.0 },
                    at: point,
                };
                let found = if first == a {
                    (point, on_first, on_second)
                } else {
                    (point, on_second, on_first)
                };
                let Some(near) = near else {
                    return Some(found);
                };
                let score = distance(point, near);
                if best.as_ref().is_none_or(|(s, _)| score < *s) {
                    best = Some((score, found));
                }
            }
        }
        best.map(|(_, found)| found)
    }

    /// The unit direction of the road's segment at `m`.
    fn direction(&self, m: Match) -> (f64, f64) {
        let line = &self.lines[m.road];
        let (a, b) = (line[m.segment], line[m.segment + 1]);
        let length = distance(a, b).max(1e-9);
        ((b.0 - a.0) / length, (b.1 - a.1) / length)
    }

    fn closest_on(&self, road: usize, point: (f64, f64)) -> Option<Match> {
        let line = &self.lines[road];
        (0..line.len().saturating_sub(1))
            .map(|segment| {
                let (at, along) = project(point, line[segment], line[segment + 1]);
                Match {
                    road,
                    segment,
                    along,
                    at,
                }
            })
            .min_by(|x, y| distance(x.at, point).total_cmp(&distance(y.at, point)))
    }

    /// The points of the track to keep: all but runs of points off every road between two
    /// points on roads the map connects without a long detour (`OFF_ROAD_DETOUR`). Those were
    /// not ridden: a receiver that lost its fix in a tunnel, a planner's own line through it
    /// or along a lake (#166). The road between the two is followed instead (`connect`).
    fn on_the_map(&self, matches: &[Option<Match>], positions: &[(f64, f64)]) -> Vec<usize> {
        let mut kept = Vec::with_capacity(matches.len());
        let mut i = 0;
        while i < matches.len() {
            if matches[i].is_some() {
                kept.push(i);
                i += 1;
                continue;
            }
            let start = i;
            while i < matches.len() && matches[i].is_none() {
                i += 1;
            }
            if !self.off_the_map(matches, positions, start, i) {
                kept.extend(start..i);
            }
        }
        kept
    }

    /// Whether the points `start..end` off every road lie between points on roads the map
    /// connects, and are no detour: see [`Network::on_the_map`].
    fn off_the_map(
        &self,
        matches: &[Option<Match>],
        positions: &[(f64, f64)],
        start: usize,
        end: usize,
    ) -> bool {
        if start == 0 || end >= matches.len() {
            return false;
        }
        let (Some(from), Some(to)) = (matches[start - 1], matches[end]) else {
            return false;
        };
        let Some((way, _)) = self.way(from, to) else {
            return false;
        };
        let recorded = path_length(&positions[start - 1..=end]);
        recorded <= path_length(&way) * OFF_ROAD_DETOUR
    }

    /// The way along roads from `from` to `to`: along their road's bends, or the shortest way
    /// through the network where the track changes road (`shortest`), round junctions and
    /// through tunnels whose points a file left out; straight where the roads do not connect.
    /// Round a roundabout the way traffic goes, even the long way round. With the roads the
    /// way passes onto. `None` where the way would be a detour (`MAX_DETOUR`): a loop of the
    /// road between the two is not what was ridden.
    fn way(&self, from: Match, to: Match) -> Option<Way> {
        let chord = distance(from.at, to.at).max(1.0);
        let limit = chord * MAX_DETOUR;
        let free = if from.road == to.road && !self.roundabout[from.road] {
            let through = self.bends(from, to);
            Some(Passage {
                length: length_via(from.at, &through, to.at),
                through,
                onto: Vec::new(),
                wrong: 0.0,
            })
        } else {
            self.shortest(from, to, limit, false)
        };
        let Some(free) = free else {
            // Straight across from a road the other does not connect to.
            return (from.road != to.road).then(|| (vec![from.at, to.at], Vec::new()));
        };
        // A detour or not as if traffic went either way round: going the long way round a
        // roundabout is none.
        if free.length > limit {
            return None;
        }
        let passage = if free.wrong > WANDER_M {
            self.shortest(from, to, free.length + self.longest_lap, true)
                .unwrap_or(free)
        } else {
            free
        };
        let mut path = Vec::with_capacity(passage.through.len() + 2);
        path.push(from.at);
        path.extend(passage.through);
        path.push(to.at);
        Some((path, passage.onto))
    }

    /// The shortest way through the network from `from` to `to`, no longer than `limit`: on
    /// their road where both lie on it, or on other roads. Only the way traffic goes round
    /// roundabouts if `keeping_to_them`.
    fn shortest(
        &self,
        from: Match,
        to: Match,
        limit: f64,
        keeping_to_them: bool,
    ) -> Option<Passage> {
        let line = |road: usize| &self.lines[road];
        let allowed = |wrong: f64| !keeping_to_them || wrong <= 0.0;
        let mut heap = BinaryHeap::new();
        let mut best: HashMap<Vertex, (f64, f64, Step)> = HashMap::new();
        let mut arrived: Option<(f64, f64, Step)> = None;
        if from.road == to.road {
            let length = length_via(from.at, &self.bends(from, to), to.at);
            let backwards = (to.segment, to.along) < (from.segment, from.along);
            let wrong = self.against(from.road, if backwards { length } else { 0.0 });
            if allowed(wrong) {
                arrived = Some((length, wrong, Step::Along));
            }
        }
        // From `from`, the ends of its segment are reached first; `to` is reached from the
        // ends of its own.
        for (vertex, back) in [(from.segment, 1.0), (from.segment + 1, 0.0)] {
            let cost = distance(from.at, line(from.road)[vertex]);
            let wrong = self.against(from.road, cost * back);
            if allowed(wrong) {
                best.insert((from.road, vertex), (cost, wrong, Step::Start));
                heap.push(Reached {
                    cost,
                    vertex: (from.road, vertex),
                });
            }
        }
        let goal = |(road, vertex): Vertex| {
            let last = distance(line(road)[vertex], to.at);
            if road != to.road {
                None
            } else if vertex == to.segment {
                Some((last, 0.0))
            } else if vertex == to.segment + 1 {
                Some((last, self.against(road, last)))
            } else {
                None
            }
        };
        while let Some(Reached { cost, vertex }) = heap.pop() {
            if cost > limit || arrived.is_some_and(|(total, ..)| cost >= total) {
                break;
            }
            let Some(&(known, wrong_so_far, _)) = best.get(&vertex) else {
                continue;
            };
            if cost > known {
                continue;
            }
            if let Some((last, wrong)) = goal(vertex)
                && allowed(wrong)
                && arrived.is_none_or(|(total, ..)| cost + last < total)
            {
                arrived = Some((cost + last, wrong_so_far + wrong, Step::From(vertex)));
            }
            for (other, step, wrong) in self.steps(vertex) {
                let total = cost + step;
                if allowed(wrong) && best.get(&other).is_none_or(|&(known, ..)| total < known) {
                    best.insert(other, (total, wrong_so_far + wrong, Step::From(vertex)));
                    heap.push(Reached {
                        cost: total,
                        vertex: other,
                    });
                }
            }
        }
        let (length, wrong, last) = arrived?;
        if length > limit {
            return None;
        }
        let (through, onto) = match last {
            Step::Along => (self.bends(from, to), Vec::new()),
            step => self.retrace(&best, step, (from.road, to.road)),
        };
        Some(Passage {
            through,
            onto,
            length,
            wrong,
        })
    }

    /// How much of going `back` metres along `road`, against the order of its vertices, is
    /// the wrong way round a roundabout.
    fn against(&self, road: usize, back: f64) -> f64 {
        if self.roundabout[road] { back } else { 0.0 }
    }

    /// The vertices one step on from `vertex`, along its road or onto another there, with how
    /// far each is and how much of that is the wrong way round a roundabout.
    fn steps(&self, (road, index): Vertex) -> Vec<(Vertex, f64, f64)> {
        let line = &self.lines[road];
        let mut next = Vec::new();
        if index > 0 {
            let step = distance(line[index], line[index - 1]);
            next.push(((road, index - 1), step, self.against(road, step)));
        }
        if index + 1 < line.len() {
            next.push((
                (road, index + 1),
                distance(line[index], line[index + 1]),
                0.0,
            ));
        }
        let joined = self.joins.get(&(road, index)).into_iter().flatten();
        next.extend(joined.map(|&other| (other, 0.0, 0.0)));
        next
    }

    /// The points and the roads (other than the two `ends`) of the way the shortest-way
    /// search found, from its `last` step back to the start, in order.
    fn retrace(
        &self,
        best: &HashMap<Vertex, (f64, f64, Step)>,
        last: Step,
        ends: (usize, usize),
    ) -> Way {
        let mut through: Vec<(f64, f64)> = Vec::new();
        let mut onto = Vec::new();
        let mut step = last;
        while let Step::From(vertex) = step {
            let (road, index) = vertex;
            let point = self.lines[road][index];
            if through
                .last()
                .is_none_or(|&last| distance(last, point) > 0.05)
            {
                through.push(point);
            }
            if road != ends.0 && road != ends.1 && !onto.contains(&road) {
                onto.push(road);
            }
            step = best[&vertex].2;
        }
        through.reverse();
        onto.reverse();
        (through, onto)
    }

    /// The way from `from` to `to` (the positions on roads of two consecutive points `ends`)
    /// as [`Network::way`] gives it, as points of the line; the roads passed onto join the
    /// roads `used`, so their bridges and tunnels come with them.
    fn connect(
        &self,
        (nodes, used): (&mut Vec<Node>, &mut Vec<usize>),
        from: Match,
        to: Match,
        ends: (&RawPoint, &RawPoint),
    ) {
        let Some((path, onto)) = self.way(from, to) else {
            return;
        };
        used.extend(onto);
        let lengths: Vec<f64> = path.windows(2).map(|w| distance(w[0], w[1])).collect();
        let total: f64 = lengths.iter().sum();
        let mut done = 0.0;
        for (point, length) in path[1..path.len() - 1].iter().zip(&lengths) {
            done += length;
            if nodes.last().is_some_and(|n| distance(n.at, *point) < 0.05) {
                continue;
            }
            let share = if total > 0.0 { done / total } else { 0.0 };
            nodes.push(Node {
                at: *point,
                anchor: *point,
                elevation: lerp_option(ends.0.elevation, ends.1.elevation, share),
                time: lerp_option(ends.0.time, ends.1.time, share),
                on_road: true,
            });
        }
    }

    /// A road's vertices strictly between two positions on it, in the direction of travel.
    fn bends(&self, from: Match, to: Match) -> Vec<(f64, f64)> {
        let line = &self.lines[from.road];
        if (to.segment, to.along) >= (from.segment, from.along) {
            (from.segment + 1..=to.segment).map(|v| line[v]).collect()
        } else {
            (to.segment + 1..=from.segment)
                .rev()
                .map(|v| line[v])
                .collect()
        }
    }
}

fn path_length(path: &[(f64, f64)]) -> f64 {
    path.windows(2).map(|w| distance(w[0], w[1])).sum()
}

/// The length of the way from `from` through the points `through` to `to`.
fn length_via(from: (f64, f64), through: &[(f64, f64)], to: (f64, f64)) -> f64 {
    match (through.first(), through.last()) {
        (Some(&first), Some(&last)) => {
            distance(from, first) + path_length(through) + distance(last, to)
        }
        _ => distance(from, to),
    }
}

/// The rings of the `mapped` roads (those the map detail tells go round a roundabout), each as
/// its pieces joined end to start; a ring of one closed way gets the join of its last vertex to
/// its first.
fn mapped_rings(
    lines: &[Vec<(f64, f64)>],
    mapped: &[bool],
    joins: &mut HashMap<Vertex, Vec<Vertex>>,
) -> Vec<Vec<usize>> {
    let ends = |road: usize| (0, lines[road].len() - 1);
    let mut rings = Vec::new();
    let mut seen = vec![false; lines.len()];
    for first in (0..lines.len()).filter(|&road| mapped[road] && lines[road].len() >= 2) {
        if seen[first] {
            continue;
        }
        let (start, end) = ends(first);
        if distance(lines[first][start], lines[first][end]) <= JOIN_M {
            joins.entry((first, end)).or_default().push((first, start));
            joins.entry((first, start)).or_default().push((first, end));
        }
        // The ring's pieces: those joined to it at their ends, as far as they go.
        let mut ring = vec![first];
        seen[first] = true;
        let mut at = 0;
        while at < ring.len() {
            let road = ring[at];
            let (start, end) = ends(road);
            for vertex in [start, end] {
                for &(other, other_vertex) in joins.get(&(road, vertex)).into_iter().flatten() {
                    let (other_start, other_end) = ends(other);
                    let at_end = other_vertex == other_start || other_vertex == other_end;
                    if mapped[other] && !seen[other] && at_end {
                        seen[other] = true;
                        ring.push(other);
                    }
                }
            }
            at += 1;
        }
        rings.push(ring);
    }
    rings
}

/// The roundabouts among the roads (see `ROUNDABOUT_RADIUS`), each as its roads in the order
/// traffic takes them: rings of one closed way or of ways each starting where the last ends.
fn roundabouts(
    lines: &[Vec<(f64, f64)>],
    roads: &[Road],
    joins: &HashMap<Vertex, Vec<Vertex>>,
) -> Vec<Vec<usize>> {
    let (smallest, largest) = ROUNDABOUT_RADIUS;
    let longest_lap = std::f64::consts::TAU * largest * (1.0 + ROUNDABOUT_ROUND);
    let widest = 2.0 * largest * (1.0 + ROUNDABOUT_ROUND);
    // Roundabouts are roads; a path round a pond is none.
    let road_like = |road: usize| {
        lines[road].len() >= 2 && !matches!(roads[road].class, RoadClass::Path | RoadClass::Track)
    };
    let mut rings: Vec<Vec<usize>> = Vec::new();
    let mut taken = vec![false; lines.len()];
    for first in 0..lines.len() {
        if taken[first] || !road_like(first) {
            continue;
        }
        let start = lines[first][0];
        // Ways on from the end of the last, depth first; each ring is found from its road
        // listed first.
        let mut chains = vec![(vec![first], path_length(&lines[first]))];
        while let Some((chain, length)) = chains.pop() {
            let last = chain[chain.len() - 1];
            let end = lines[last][lines[last].len() - 1];
            if distance(end, start) <= JOIN_M {
                if round(&chain, lines, (smallest, largest)) && arms(&chain, lines, joins) >= 2 {
                    for &road in &chain {
                        taken[road] = true;
                    }
                    rings.push(chain);
                    break;
                }
                continue;
            }
            if chain.len() == ROUNDABOUT_PIECES {
                continue;
            }
            for &(next, vertex) in joins
                .get(&(last, lines[last].len() - 1))
                .into_iter()
                .flatten()
            {
                let length = length + path_length(&lines[next]);
                if vertex == 0
                    && next > first
                    && road_like(next)
                    && !chain.contains(&next)
                    && length <= longest_lap
                    && lines[next].iter().all(|&p| distance(p, start) <= widest)
                {
                    chains.push(([chain.as_slice(), &[next]].concat(), length));
                }
            }
        }
    }
    rings
}

/// Whether the ring of roads `chain` is a circle with a radius within `radii`, as a
/// roundabout is (`ROUNDABOUT_ROUND`).
fn round(chain: &[usize], lines: &[Vec<(f64, f64)>], (smallest, largest): (f64, f64)) -> bool {
    let points: Vec<(f64, f64)> = chain
        .iter()
        .flat_map(|&r| lines[r].iter().copied())
        .collect();
    // The middle by length, not by vertices: the map draws some pieces with more.
    let (mut weight, mut middle) = (0.0, (0.0, 0.0));
    for pair in points.windows(2) {
        let length = distance(pair[0], pair[1]);
        let centre = lerp(pair[0], pair[1], 0.5);
        weight += length;
        middle = (middle.0 + centre.0 * length, middle.1 + centre.1 * length);
    }
    if weight <= 0.0 {
        return false;
    }
    let middle = (middle.0 / weight, middle.1 / weight);
    #[allow(clippy::cast_precision_loss)] // a few dozen points
    let radius = points.iter().map(|&p| distance(p, middle)).sum::<f64>() / points.len() as f64;
    (smallest..=largest).contains(&radius)
        && points
            .iter()
            .all(|&p| (distance(p, middle) - radius).abs() <= ROUNDABOUT_ROUND * radius)
}

/// How many other roads join the ring of roads `chain`. Only one leads to a turning loop,
/// which traffic goes round either way.
fn arms(chain: &[usize], lines: &[Vec<(f64, f64)>], joins: &HashMap<Vertex, Vec<Vertex>>) -> usize {
    let mut others: Vec<usize> = chain
        .iter()
        .flat_map(|&road| (0..lines[road].len()).map(move |vertex| (road, vertex)))
        .flat_map(|vertex| joins.get(&vertex).into_iter().flatten())
        .map(|&(road, _)| road)
        .filter(|road| !chain.contains(road))
        .collect();
    others.sort_unstable();
    others.dedup();
    others.len()
}

/// Direction of travel at point `i` (unit), from its neighbours; `None` where they coincide.
fn heading(positions: &[(f64, f64)], i: usize) -> Option<(f64, f64)> {
    let before = positions[i.saturating_sub(1)];
    let after = positions[(i + 1).min(positions.len() - 1)];
    let (dx, dy) = (after.0 - before.0, after.1 - before.1);
    let length = dx.hypot(dy);
    (length > 0.5).then(|| (dx / length, dy / length))
}

/// Takes out the matched points where the line turns back on itself within a few metres (see
/// `FOLD_TURN`), as often as it takes.
fn remove_folds(nodes: &mut Vec<Node>) {
    let folds = |nodes: &[Node], i: usize| {
        if !nodes[i].on_road {
            return false;
        }
        let (a, b, c) = (nodes[i - 1].at, nodes[i].at, nodes[i + 1].at);
        let (ab, bc) = (distance(a, b), distance(b, c));
        if ab.min(bc) >= FOLD_M || ab < 1e-9 || bc < 1e-9 {
            return false;
        }
        let dot = (b.0 - a.0) * (c.0 - b.0) + (b.1 - a.1) * (c.1 - b.1);
        (dot / (ab * bc)).clamp(-1.0, 1.0).acos().to_degrees() > FOLD_TURN
    };
    let mut i = 1;
    while i + 1 < nodes.len() {
        if folds(nodes, i) {
            nodes.remove(i);
            i = i.saturating_sub(1).max(1);
        } else {
            i += 1;
        }
    }
}

/// The line with points inserted so that, along roads, none are more than `STEP_M` apart.
/// Between points off road the line is not the road ridden (sparse files cut corners), so
/// nothing is added there: elevations along it are interpolated rather than taken from the
/// terrain.
fn densify(nodes: &[Node]) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len() * 2);
    for pair in nodes.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        out.push(from);
        if !(from.on_road && to.on_road) {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // short segments
        let pieces = (distance(from.at, to.at) / STEP_M).ceil() as usize;
        for piece in 1..pieces {
            #[allow(clippy::cast_precision_loss)] // few pieces
            let share = piece as f64 / pieces as f64;
            out.push(Node {
                at: lerp(from.at, to.at, share),
                anchor: lerp(from.anchor, to.anchor, share),
                elevation: lerp_option(from.elevation, to.elevation, share),
                time: from
                    .time
                    .zip(to.time)
                    .map(|(start, end)| start + (end - start) * share),
                on_road: true,
            });
        }
    }
    out.extend(nodes.last());
    out
}

/// Puts short excursions to one side and back (see `EXCURSION_MAX_M`) onto the line they leave
/// and rejoin.
fn straighten_excursions(nodes: &mut [Node]) {
    // Points this far before and after an excursion give the line's direction there.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small constants
    let (lead, reach) = (
        (15.0 / STEP_M).ceil() as usize,
        (EXCURSION_MAX_M / STEP_M).ceil() as usize,
    );
    let mut start = lead;
    while start + lead < nodes.len() {
        let last = (start + reach).min(nodes.len() - 1 - lead);
        // The longest excursion from here, so it is taken out as a whole.
        let end = (start + 2..=last)
            .rev()
            .find(|&end| is_excursion(nodes, start, end, lead));
        let Some(end) = end else {
            start += 1;
            continue;
        };
        let (a, b) = (nodes[start].at, nodes[end].at);
        for node in &mut nodes[start + 1..end] {
            let (on_line, _) = project(node.at, a, b);
            node.at = on_line;
            node.anchor = on_line;
        }
        start = end;
    }
}

/// Whether the line leaves the chord from point `start` to point `end` to one side and comes
/// back to it, as an excursion does.
fn is_excursion(nodes: &[Node], start: usize, end: usize, lead: usize) -> bool {
    let direction = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).atan2(b.1 - a.1);
    let apart = |x: f64, y: f64| {
        (x - y + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    };
    let (a, b) = (nodes[start].at, nodes[end].at);
    let length = distance(a, b);
    if !(15.0..=EXCURSION_MAX_M).contains(&length) {
        return false;
    }
    let chord = direction(a, b);
    let before = direction(nodes[start - lead].at, a);
    let after = direction(b, nodes[end + lead].at);
    if apart(before, chord).abs() > EXCURSION_ALIGNED
        || apart(after, chord).abs() > EXCURSION_ALIGNED
    {
        return false;
    }
    let inside = &nodes[start + 1..end];
    // Off-road stretches keep the course they were recorded on.
    if inside.iter().any(|n| !n.on_road) {
        return false;
    }
    let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
    let offsets: Vec<f64> = inside
        .iter()
        .map(|n| (n.at.0 - a.0) * uy - (n.at.1 - a.1) * ux)
        .collect();
    let widest = offsets.iter().fold(0.0_f64, |m, o| m.max(o.abs()));
    let one_side = offsets.iter().all(|&o| o >= -0.2) || offsets.iter().all(|&o| o <= 0.2);
    let turned = nodes[start..end]
        .windows(2)
        .map(|w| apart(direction(w[0].at, w[1].at), chord).abs())
        .fold(0.0, f64::max);
    one_side && (1.0..=EXCURSION_OFFSET_M).contains(&widest) && turned >= EXCURSION_TURN
}

/// Rounds corners and takes out jitter, keeping every point within `MAX_SHIFT_M` of where it
/// was put; the ends stay.
fn smooth(nodes: &[Node]) -> Vec<Node> {
    let mut out = nodes.to_vec();
    for pass in 0..2 * SMOOTHING_PAIRS {
        // Taubin's factors: shrink, then inflate a little more.
        let factor = if pass % 2 == 0 { 0.5 } else { -0.53 };
        let previous: Vec<(f64, f64)> = out.iter().map(|n| n.at).collect();
        for i in 1..out.len().saturating_sub(1) {
            let middle = lerp(previous[i - 1], previous[i + 1], 0.5);
            let moved = lerp(previous[i], middle, factor);
            let anchor = out[i].anchor;
            let (dx, dy) = (moved.0 - anchor.0, moved.1 - anchor.1);
            let shift = dx.hypot(dy);
            out[i].at = if shift > MAX_SHIFT_M {
                let scale = MAX_SHIFT_M / shift;
                (anchor.0 + dx * scale, anchor.1 + dy * scale)
            } else {
                moved
            };
        }
    }
    out
}

fn lerp(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn lerp_option(a: Option<f64>, b: Option<f64>, share: f64) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + (b - a) * share),
        (a, b) => a.or(b),
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

#[allow(clippy::cast_possible_truncation)] // local metres over a route stay far below 2^63 cells
fn cell(metres: f64) -> i64 {
    (metres / CELL_M).floor() as i64
}

/// The point on segment `a`–`b` closest to `p`, and how far along the segment it is.
fn project(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> ((f64, f64), f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.0 + dx * t, a.1 + dy * t), t)
}

#[cfg(test)]
mod tests {
    use orqwa_osm::StructureKind;

    use super::*;

    const ORIGIN: (f64, f64) = (46.0, 7.0);

    fn flat() -> Flat {
        Flat::new(ORIGIN.0, ORIGIN.1)
    }

    fn point(x: f64, y: f64) -> RawPoint {
        let (lat, lon) = flat().to_lat_lon((x, y));
        RawPoint {
            lat,
            lon,
            elevation: Some(500.0),
            time: None,
        }
    }

    fn point2((x, y): (f64, f64)) -> RawPoint {
        point(x, y)
    }

    fn road(points: &[(f64, f64)]) -> Road {
        road_of(RoadClass::Street, points)
    }

    fn tunnel(points: &[(f64, f64)]) -> Road {
        Road {
            structure: Some(StructureKind::Tunnel),
            ..road(points)
        }
    }

    /// A road bending through a hill, a quarter circle of 300 m radius, in a tunnel from 10°
    /// to 80° in two pieces (the map splits the way at the tunnel's mouths and at a tile
    /// border), and the point of the arc at an angle.
    fn tunnel_bend() -> ([Road; 4], impl Fn(f64) -> (f64, f64)) {
        let arc = |degrees: f64| {
            let r = degrees.to_radians();
            (300.0 * r.cos(), 300.0 * r.sin())
        };
        let every_5 = |from: i32, to: i32| -> Vec<(f64, f64)> {
            (from..=to).map(|i| arc(f64::from(i) * 5.0)).collect()
        };
        (
            [
                road(&every_5(0, 2)),
                tunnel(&every_5(2, 9)),
                tunnel(&every_5(9, 16)),
                road(&every_5(16, 18)),
            ],
            arc,
        )
    }

    fn road_of(class: RoadClass, points: &[(f64, f64)]) -> Road {
        Road {
            class,
            details: orqwa_osm::RoadDetails::default(),
            line: points.iter().map(|&p| flat().to_lat_lon(p)).collect(),
            structure: None,
        }
    }

    fn xy(track: &[RawPoint]) -> Vec<(f64, f64)> {
        track.iter().map(|p| flat().to_xy(p.lat, p.lon)).collect()
    }

    fn length(points: &[(f64, f64)]) -> f64 {
        points.windows(2).map(|w| distance(w[0], w[1])).sum()
    }

    /// The largest change of direction between consecutive pieces of the line, in degrees.
    fn sharpest_turn(points: &[(f64, f64)]) -> f64 {
        points
            .windows(3)
            .map(|w| {
                let a = (w[1].0 - w[0].0).atan2(w[1].1 - w[0].1);
                let b = (w[2].0 - w[1].0).atan2(w[2].1 - w[1].1);
                let turn = (b - a).to_degrees().abs();
                if turn > 180.0 { 360.0 - turn } else { turn }
            })
            .fold(0.0, f64::max)
    }

    /// Distance from `p` to the polyline `line`.
    fn off(line: &[(f64, f64)], p: (f64, f64)) -> f64 {
        line.windows(2)
            .map(|w| distance(project(p, w[0], w[1]).0, p))
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn gps_wandering_beside_the_road_is_put_on_it() {
        let straight = road(&[(0.0, -10.0), (0.0, 510.0)]);
        // ±6 m either side, every 20 m.
        let track: Vec<RawPoint> = (0..=25)
            .map(|i| point(if i % 2 == 0 { 6.0 } else { -6.0 }, f64::from(i) * 20.0))
            .collect();

        let snapped = xy(&to_roads(&track, &[straight]).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
        assert!((length(&snapped) - 500.0).abs() < 0.1);
    }

    #[test]
    fn sparse_points_follow_the_roads_bends_between_them() {
        // A half circle of 100 m radius drawn every 5°, ridden with a point every 60°.
        let arc = |degrees: f64| {
            let r = degrees.to_radians();
            (100.0 * r.cos(), 100.0 * r.sin())
        };
        let bend = road(
            &(0..=36)
                .map(|i| arc(f64::from(i) * 5.0))
                .collect::<Vec<_>>(),
        );
        let track: Vec<RawPoint> = (0..=3)
            .map(|i| {
                let (x, y) = arc(f64::from(i) * 60.0);
                point(x, y)
            })
            .collect();

        let snapped = xy(&to_roads(&track, &[bend]).track);

        // Along the curve (π × 100 m), not its chords (300 m), every point on it.
        assert!((length(&snapped) - std::f64::consts::PI * 100.0).abs() < 1.5);
        for (x, y) in &snapped {
            assert!((x.hypot(*y) - 100.0).abs() < 0.5, "off the road: {x}, {y}");
        }
    }

    #[test]
    fn a_fix_lost_in_a_tunnel_follows_the_road_through_it() {
        // #166: a road bending through a hill, a quarter circle of 300 m radius, in a tunnel
        // from 10° to 80° in two pieces (the map splits the way at the tunnel's mouths and at
        // a tile border); the receiver loses its fix inside and records three points
        // wandering 150 m off until it is out again.
        let (roads, arc) = tunnel_bend();
        let mut track: Vec<RawPoint> = (0..=2).map(|i| point2(arc(f64::from(i) * 5.0))).collect();
        for degrees in [25.0, 45.0, 65.0] {
            let (x, y) = arc(degrees);
            let r = degrees.to_radians();
            track.push(point(x + 150.0 * r.cos(), y + 150.0 * r.sin()));
        }
        track.extend((16..=18).map(|i| point2(arc(f64::from(i) * 5.0))));

        let snapped = to_roads(&track, &roads);
        let line = xy(&snapped.track);

        // The wandering is gone, the line follows the road's bend through the hill...
        let furthest = line.iter().map(|(x, y)| x.hypot(*y)).fold(0.0, f64::max);
        assert!(furthest < 310.0, "the line wanders {furthest} m out");
        for (x, y) in &line {
            assert!((x.hypot(*y) - 300.0).abs() < 2.5, "off the road: {x}, {y}");
        }
        // ...as long as the road, not the chords between the wandering points...
        assert!((length(&line) - std::f64::consts::FRAC_PI_2 * 300.0).abs() < 5.0);
        // ...and the tunnel ridden comes with it, both pieces.
        assert_eq!(snapped.structures.len(), 2, "{:?}", snapped.structures);
    }

    #[test]
    fn a_file_without_points_in_a_tunnel_follows_the_road_through_it() {
        // #166: a planned route whose file leaves out the points inside the tunnel, so one
        // chord runs from mouth to mouth: the line follows the road through the tunnel.
        let (roads, arc) = tunnel_bend();
        let track: Vec<RawPoint> = [0, 1, 2, 16, 17, 18]
            .into_iter()
            .map(|i| point2(arc(f64::from(i) * 5.0)))
            .collect();

        let snapped = to_roads(&track, &roads);
        let line = xy(&snapped.track);

        for (x, y) in &line {
            assert!((x.hypot(*y) - 300.0).abs() < 2.5, "off the road: {x}, {y}");
        }
        assert!((length(&line) - std::f64::consts::FRAC_PI_2 * 300.0).abs() < 5.0);
        assert_eq!(snapped.structures.len(), 2, "{:?}", snapped.structures);
    }

    #[test]
    fn a_planners_line_off_the_road_follows_the_road() {
        // #166: a road round a lake, a half circle of 200 m radius; the planner's file draws
        // the stretch by its own old line, a chord through the lake with points every 20 m,
        // which lies off every road. The line follows the road round.
        let arc = |degrees: f64| {
            let r = degrees.to_radians();
            (200.0 * r.cos(), 200.0 * r.sin())
        };
        let bend = road(
            &(0..=36)
                .map(|i| arc(f64::from(i) * 5.0))
                .collect::<Vec<_>>(),
        );
        let mut track: Vec<RawPoint> = (0..=2).map(|i| point2(arc(f64::from(i) * 5.0))).collect();
        for i in 1..19 {
            track.push(point(200.0 - f64::from(i) * 20.0, 40.0));
        }
        track.extend((34..=36).map(|i| point2(arc(f64::from(i) * 5.0))));

        let snapped = xy(&to_roads(&track, &[bend]).track);

        for (x, y) in &snapped {
            assert!((x.hypot(*y) - 200.0).abs() < 2.5, "off the road: {x}, {y}");
        }
        assert!((length(&snapped) - std::f64::consts::PI * 200.0).abs() < 5.0);
    }

    #[test]
    fn a_junction_just_behind_the_match_is_no_fold() {
        // #172: the last point before a tunnel lies 3 m past the junction into it; the way
        // goes on into the tunnel from there, not back to the junction and on.
        let roads = [
            road(&[(0.0, -100.0), (0.0, 200.0), (0.0, 500.0)]),
            tunnel(&[(0.0, 200.0), (300.0, 200.0)]),
            road(&[(300.0, 200.0), (600.0, 200.0)]),
        ];
        let track = [
            point(0.0, 0.0),
            point(0.0, 100.0),
            point(0.0, 203.0),
            point(350.0, 200.0),
            point(400.0, 200.0),
        ];

        let line = xy(&to_roads(&track, &roads).track);

        assert!(
            sharpest_turn(&line) < 100.0,
            "a fold of {}°",
            sharpest_turn(&line)
        );
    }

    #[test]
    fn a_real_detour_off_the_map_is_kept() {
        // Leaving the road for a loop off the map eight times as long as the road between
        // the points: ridden, not the planner's line.
        let straight = road(&[(0.0, -100.0), (0.0, 500.0)]);
        let mut track: Vec<RawPoint> = vec![point(0.0, -50.0), point(0.0, 0.0)];
        for i in 1..=16 {
            let r = f64::from(i) * std::f64::consts::PI / 17.0;
            track.push(point(500.0 * r.sin(), 100.0 - 500.0 * r.cos()));
        }
        track.extend([point(0.0, 200.0), point(0.0, 250.0)]);

        let snapped = xy(&to_roads(&track, &[straight]).track);

        assert!(
            snapped.iter().any(|(x, _)| *x > 400.0),
            "the detour was cut: {snapped:?}"
        );
    }

    #[test]
    fn off_road_stretches_keep_their_course() {
        let far_road = road(&[(200.0, 0.0), (200.0, 500.0)]);
        let track: Vec<RawPoint> = (0..=10).map(|i| point(0.0, f64::from(i) * 50.0)).collect();

        let snapped = xy(&to_roads(&track, &[far_road]).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01));
        assert!((length(&snapped) - 500.0).abs() < 0.1);
    }

    #[test]
    fn drifting_towards_a_parallel_road_does_not_hop_onto_it() {
        let ridden = road(&[(0.0, -10.0), (0.0, 310.0)]);
        let parallel = road(&[(10.0, -10.0), (10.0, 310.0)]);
        // Closer to the ridden road at first, then a little closer to the other.
        let track: Vec<RawPoint> = (0..=15)
            .map(|i| point(if i < 5 { 3.0 } else { 5.8 }, f64::from(i) * 20.0))
            .collect();

        let snapped = xy(&to_roads(&track, &[ridden, parallel]).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
    }

    #[test]
    fn a_sidewalk_beside_the_street_does_not_pull_the_route_over() {
        let street = road(&[(0.0, -10.0), (0.0, 410.0)]);
        let sidewalk = road_of(RoadClass::Path, &[(5.0, -10.0), (5.0, 410.0)]);
        // GPS mostly between them, sometimes right on the sidewalk.
        let track: Vec<RawPoint> = (0..=20)
            .map(|i| point(if i % 3 == 0 { 5.0 } else { 2.8 }, f64::from(i) * 20.0))
            .collect();

        let snapped = xy(&to_roads(&track, &[street, sidewalk]).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
    }

    /// A main road north, and a service road branching off it at 100 m that runs 7.5 m beside
    /// it and joins it again at 320 m, as frontage roads and car park aisles are mapped.
    fn main_road_and_frontage() -> [Road; 2] {
        [
            road_of(RoadClass::Major, &[(0.0, -10.0), (0.0, 510.0)]),
            road_of(
                RoadClass::Service,
                &[(0.0, 100.0), (7.5, 120.0), (7.5, 300.0), (0.0, 320.0)],
            ),
        ]
    }

    #[test]
    fn a_service_road_alongside_does_not_pull_the_route_over() {
        // GPS between them along the service road, a little nearer to it (#74).
        let track: Vec<RawPoint> = (0..=50)
            .map(|i| {
                let y = f64::from(i) * 10.0;
                point(
                    if (130.0..=290.0).contains(&y) {
                        4.0
                    } else {
                        1.0
                    },
                    y,
                )
            })
            .collect();

        let snapped = xy(&to_roads(&track, &main_road_and_frontage()).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
    }

    #[test]
    fn a_service_road_clearly_ridden_is_kept() {
        // GPS right on the service road along it.
        let track: Vec<RawPoint> = (0..=50)
            .map(|i| {
                let y = f64::from(i) * 10.0;
                point(
                    if (130.0..=290.0).contains(&y) {
                        7.2
                    } else {
                        0.5
                    },
                    y,
                )
            })
            .collect();

        let snapped = xy(&to_roads(&track, &main_road_and_frontage()).track);

        for &(x, y) in snapped.iter().filter(|(_, y)| (150.0..=270.0).contains(y)) {
            assert!((x - 7.5).abs() < 0.5, "{x} m east at {y} m");
        }
    }

    #[test]
    fn side_streets_passed_at_junctions_are_not_taken() {
        // A street north with side streets branching east and west at 200 m.
        let main = road(&[(0.0, -10.0), (0.0, 200.0), (0.0, 410.0)]);
        let east = road(&[(0.0, 200.0), (300.0, 200.0)]);
        let west = road(&[(0.0, 200.0), (-300.0, 200.0)]);
        // The track passes the junction drifting east, one point right on the side street.
        let track: Vec<RawPoint> = [
            (1.0, 0.0),
            (2.0, 100.0),
            (4.0, 190.0),
            (7.0, 200.0),
            (4.0, 210.0),
            (1.0, 300.0),
            (0.0, 400.0),
        ]
        .iter()
        .map(|&(x, y)| point(x, y))
        .collect();

        let snapped = xy(&to_roads(&track, &[main, east, west]).track);

        assert!(snapped.iter().all(|(x, _)| x.abs() < 0.01), "{snapped:?}");
    }

    #[test]
    fn turns_at_junctions_go_round_the_corner_not_across_it() {
        // North, then east at a junction 200 m along; one point each side of the corner.
        let north = road(&[(0.0, -10.0), (0.0, 200.0)]);
        let east = road(&[(0.0, 200.0), (300.0, 200.0)]);
        let track: Vec<RawPoint> = [(0.0, 0.0), (0.0, 120.0), (90.0, 200.0), (250.0, 200.0)]
            .iter()
            .map(|&(x, y)| point(x, y))
            .collect();
        let streets = [(0.0, -10.0), (0.0, 200.0), (300.0, 200.0)];

        let snapped = xy(&to_roads(&track, &[north, east]).track);

        // Round the corner, cutting it a little but never across the block, and turning over
        // several points rather than at one.
        let corner = snapped
            .iter()
            .map(|&p| distance(p, (0.0, 200.0)))
            .fold(f64::INFINITY, f64::min);
        assert!((0.3..4.0).contains(&corner), "{corner}");
        for &p in &snapped {
            assert!(off(&streets, p) <= MAX_SHIFT_M + 1e-6, "{p:?}");
        }
        assert!(
            sharpest_turn(&snapped) < 65.0,
            "{}",
            sharpest_turn(&snapped)
        );
    }

    #[test]
    fn excursions_round_traffic_islands_are_ridden_straight() {
        // A street north splitting round a traffic island 60 m long, its one-way branches 5 m
        // either side (as the map draws them), ridden straight through.
        let roads = [
            road(&[(0.0, 0.0), (0.0, 200.0)]),
            road(&[(0.0, 200.0), (5.0, 215.0), (5.0, 245.0), (0.0, 260.0)]),
            road(&[(0.0, 200.0), (-5.0, 215.0), (-5.0, 245.0), (0.0, 260.0)]),
            road(&[(0.0, 260.0), (0.0, 500.0)]),
        ];
        let track: Vec<RawPoint> = (0..=50).map(|i| point(0.5, f64::from(i) * 10.0)).collect();

        let snapped = xy(&to_roads(&track, &roads).track);

        for &(x, y) in &snapped {
            assert!(x.abs() < 0.5, "{x} m aside at {y} m");
        }
        assert!(sharpest_turn(&snapped) < 3.0, "{}", sharpest_turn(&snapped));
    }

    #[test]
    fn a_road_stepping_aside_keeps_its_course() {
        // A real dog-leg: the road moves 20 m east and goes on there; not an excursion.
        let dogleg = [(0.0, 0.0), (0.0, 200.0), (20.0, 240.0), (20.0, 500.0)];
        let track: Vec<RawPoint> = (0..=50)
            .map(|i| {
                let y = f64::from(i) * 10.0;
                point(
                    if y < 200.0 {
                        0.5
                    } else if y < 240.0 {
                        (y - 200.0) / 2.0
                    } else {
                        20.5
                    },
                    y,
                )
            })
            .collect();

        let snapped = xy(&to_roads(&track, &[road(&dogleg)]).track);

        for &p in &snapped {
            assert!(off(&dogleg, p) <= MAX_SHIFT_M + 1e-6, "{p:?}");
        }
    }

    #[test]
    fn the_route_bends_smoothly_without_kinks() {
        // A road of straight pieces with sharp corners, ridden densely beside it.
        let zigzag = [
            (0.0, 0.0),
            (0.0, 100.0),
            (40.0, 140.0),
            (40.0, 260.0),
            (0.0, 300.0),
        ];
        let track: Vec<RawPoint> = (0..=60)
            .map(|i| {
                let t = f64::from(i) / 60.0 * 300.0;
                let x = if t < 100.0 {
                    0.0
                } else if t < 140.0 {
                    t - 100.0
                } else if t < 260.0 {
                    40.0
                } else {
                    300.0 - t
                };
                point(x + 1.5, t)
            })
            .collect();

        let snapped = xy(&to_roads(&track, &[road(&zigzag)]).track);

        assert!(
            sharpest_turn(&snapped) < 30.0,
            "{}",
            sharpest_turn(&snapped)
        );
        assert!(
            snapped
                .windows(2)
                .all(|w| distance(w[0], w[1]) <= STEP_M * 1.5)
        );
        for &p in &snapped {
            assert!(off(&zigzag, p) <= MAX_SHIFT_M + 1e-6, "{p:?}");
        }
    }

    /// The point at `degrees` on a circle of 15 m radius round the origin.
    fn on_ring(degrees: f64) -> (f64, f64) {
        let r = degrees.to_radians();
        (15.0 * r.cos(), 15.0 * r.sin())
    }

    /// A roundabout of 15 m radius round the origin, drawn from the east counter-clockwise
    /// (where traffic keeps right) or clockwise in `pieces` ways, and streets 150 m out from
    /// it at the compass points `arms` (degrees counter-clockwise from east).
    fn roundabout(clockwise: bool, pieces: usize, arms: &[f64]) -> Vec<Road> {
        let turn = if clockwise { -10.0 } else { 10.0 };
        let ring: Vec<(f64, f64)> = (0..=36).map(|i| on_ring(f64::from(i) * turn)).collect();
        let each = 36 / pieces;
        let mut roads: Vec<Road> = (0..pieces)
            .map(|p| road(&ring[p * each..=(p + 1) * each]))
            .collect();
        for &degrees in arms {
            let (x, y) = on_ring(degrees);
            let r = degrees.to_radians();
            roads.push(road(&[(x, y), (x + 150.0 * r.cos(), y + 150.0 * r.sin())]));
        }
        roads
    }

    const COMPASS: [f64; 4] = [0.0, 90.0, 180.0, 270.0];

    /// A planner's file turning left at the roundabout: two points on the street from the
    /// south, two on the street to the west, none round the ring.
    fn left_at_the_roundabout() -> Vec<RawPoint> {
        [(0.0, -140.0), (0.0, -60.0), (-60.0, 0.0), (-140.0, 0.0)]
            .into_iter()
            .map(point2)
            .collect()
    }

    /// How far the line goes round the origin within 20 m of it, in degrees, counter-clockwise
    /// positive.
    fn round_the_middle(line: &[(f64, f64)]) -> f64 {
        round_within(line, 20.0)
    }

    /// How far the line goes round the origin within `within` metres of it, in degrees,
    /// counter-clockwise positive.
    fn round_within(line: &[(f64, f64)], within: f64) -> f64 {
        line.windows(2)
            .filter(|w| w.iter().all(|p| p.0.hypot(p.1) < within))
            .map(|w| {
                let turn = w[1].1.atan2(w[1].0) - w[0].1.atan2(w[0].0);
                (turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI
            })
            .sum::<f64>()
            .to_degrees()
    }

    #[test]
    fn roundabouts_are_ridden_the_way_traffic_goes_round() {
        // R126: where traffic keeps right, turning left is three quarters of the way round,
        // counter-clockwise, never the quarter clockwise, nor across the island.
        let line = xy(&to_roads(&left_at_the_roundabout(), &roundabout(false, 1, &COMPASS)).track);

        let round = round_the_middle(&line);
        assert!((250.0..290.0).contains(&round), "{round}° round");
        for &(x, y) in &line {
            assert!(
                x.hypot(y) > 15.0 - MAX_SHIFT_M,
                "across the island: {x}, {y}"
            );
        }
    }

    /// A ring too long to be round, 60 m by 24 m as squashed and turbo roundabouts are, drawn
    /// counter-clockwise from the east as one closed way, with streets to the south and the
    /// west; marked as a roundabout if `mapped` by the map detail.
    fn oval(mapped: bool) -> Vec<Road> {
        let ring: Vec<(f64, f64)> = (0..=36)
            .map(|i| {
                let angle = (f64::from(i) * 10.0).to_radians();
                (30.0 * angle.cos(), 12.0 * angle.sin())
            })
            .collect();
        let mut ring = road(&ring);
        ring.details.roundabout = mapped;
        vec![
            ring,
            road(&[(0.0, -12.0), (0.0, -150.0)]),
            road(&[(-30.0, 0.0), (-150.0, 0.0)]),
        ]
    }

    #[test]
    fn a_ring_the_map_detail_marks_is_ridden_the_way_traffic_goes_whatever_its_shape() {
        // R143: its shape tells nothing of a ring this long; the map detail does. Turning left
        // is three quarters of the way round, counter-clockwise.
        let marked = xy(&to_roads(&left_at_the_roundabout(), &oval(true)).track);
        let round = round_within(&marked, 32.0);
        assert!((250.0..290.0).contains(&round), "{round}° round");
        // Without the detail, as before: the quarter the file suggests.
        let unmarked = xy(&to_roads(&left_at_the_roundabout(), &oval(false)).track);
        let round = round_within(&unmarked, 32.0);
        assert!((-110.0..-70.0).contains(&round), "{round}° round");
    }

    #[test]
    fn a_point_beside_the_ring_past_the_way_out_is_no_reason_to_go_round_again() {
        // R145: from the south out to the east is a quarter of the way round. A point of the
        // file 3 m beside the ring beyond the east street, as where a file runs along a path
        // beside it or rests there, would take the line on round and once more to get out.
        let track: Vec<RawPoint> = [
            (0.0, -140.0),
            (0.0, -60.0),
            (12.7, 12.7),
            (60.0, 0.0),
            (140.0, 0.0),
        ]
        .into_iter()
        .map(point2)
        .collect();
        let line = xy(&to_roads(&track, &roundabout(false, 1, &COMPASS)).track);

        let round = round_the_middle(&line);
        assert!((70.0..110.0).contains(&round), "{round}° round");
    }

    #[test]
    fn a_turn_back_at_a_roundabout_goes_round_it_once() {
        // R145: once round and back the way it came, as a planner draws turning back there.
        let track: Vec<RawPoint> = [
            (0.0, -140.0),
            (0.0, -60.0),
            (15.0, 0.0),
            (0.0, 15.0),
            (-15.0, 0.0),
            (0.0, -60.0),
            (0.0, -140.0),
        ]
        .into_iter()
        .map(point2)
        .collect();
        let line = xy(&to_roads(&track, &roundabout(false, 1, &COMPASS)).track);

        let round = round_the_middle(&line);
        assert!((330.0..390.0).contains(&round), "{round}° round");
    }

    #[test]
    fn where_traffic_keeps_left_roundabouts_are_ridden_clockwise() {
        // R126: the map draws them clockwise there, and left is the first turn.
        let line = xy(&to_roads(&left_at_the_roundabout(), &roundabout(true, 1, &COMPASS)).track);

        let round = round_the_middle(&line);
        assert!((-110.0..-70.0).contains(&round), "{round}° round");
    }

    #[test]
    fn roundabouts_of_several_ways_are_ridden_the_way_traffic_goes_round() {
        // R126: the map splits some rings, tiles cut others at their borders.
        for pieces in [2, 3, 6] {
            let roads = roundabout(false, pieces, &COMPASS);

            let line = xy(&to_roads(&left_at_the_roundabout(), &roads).track);

            let round = round_the_middle(&line);
            assert!(
                (250.0..290.0).contains(&round),
                "{pieces} ways: {round}° round"
            );
        }
    }

    #[test]
    fn wandering_back_a_little_round_a_roundabout_is_no_lap() {
        // A recorded track round the ring from the south to the west, one point 3 m behind
        // the one before it.
        let mut track: Vec<RawPoint> = (2..=10)
            .rev()
            .map(|i| point(0.0, -10.0 * f64::from(i)))
            .collect();
        for degrees in [285.0, 300.0, 315.0, 303.0, 330.0, 345.0] {
            track.push(point2(on_ring(degrees)));
        }
        track.extend((0..=11).map(|i| point2(on_ring(f64::from(i) * 15.0))));
        track.extend((2..=10).map(|i| point(-10.0 * f64::from(i), 0.0)));

        let line = xy(&to_roads(&track, &roundabout(false, 1, &COMPASS)).track);

        // 85 m in and out each, three quarters of the way round, a little less round the
        // corners: no extra lap of 94 m.
        let expected = 170.0 + 0.75 * std::f64::consts::TAU * 15.0;
        assert!(
            (length(&line) - expected).abs() < 10.0,
            "{} m instead of {expected} m",
            length(&line)
        );
    }

    #[test]
    fn turning_loops_are_ridden_either_way() {
        // A loop of road at the end of a street from the south, ridden round clockwise and
        // back: only one road leads to it, so traffic goes either way round.
        let roads = roundabout(false, 1, &[270.0]);
        let mut track = vec![point(0.0, -140.0), point(0.0, -60.0)];
        track.extend([240.0, 180.0, 120.0, 60.0, 0.0, -60.0].map(|d| point2(on_ring(d))));
        track.extend([point(0.0, -60.0), point(0.0, -140.0)]);

        let line = xy(&to_roads(&track, &roads).track);

        let round = round_the_middle(&line);
        assert!(round < -270.0, "{round}° round");
    }

    #[test]
    fn only_the_roads_ridden_bring_their_bridges_and_tunnels() {
        let ridden = Road {
            structure: Some(StructureKind::Bridge),
            ..road(&[(0.0, -10.0), (0.0, 310.0)])
        };
        // A tunnel of another road running alongside, 15 m away.
        let other = Road {
            structure: Some(StructureKind::Tunnel),
            ..road(&[(15.0, -10.0), (15.0, 310.0)])
        };
        let track: Vec<RawPoint> = (0..=15).map(|i| point(1.0, f64::from(i) * 20.0)).collect();

        let snapped = to_roads(&track, &[ridden, other]);

        assert_eq!(snapped.structures.len(), 1);
        assert_eq!(snapped.structures[0].kind, StructureKind::Bridge);
    }
}
