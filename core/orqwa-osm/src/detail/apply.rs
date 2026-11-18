//! Giving the tiles' roads what map detail knows of them. The tiles and the detail come from
//! the same OpenStreetMap ways, so a road and its way lie on each other; a way is that road's
//! where it runs along it, of the same kind, within a few metres.

use std::collections::HashMap;

use super::extract::{MappedAt, Roundabout, SurfaceWay};
use crate::{Building, LatLon, Road, RoadKind, RoadSurface};

/// Farthest a mapped way may lie from a road to be that road (metres): the tiles keep
/// positions to about half a metre and a parallel sidewalk or lane lies further off.
const NEAR: f64 = 4.0;
/// Least alignment (cosine of the angle) between a road and a mapped way along it.
const ALIGNED: f64 = 0.9;
/// Pieces of road shorter than this (metres) take what the road before them has, so a
/// junction leaves no sliver of another surface.
const SHORTEST: f64 = 5.0;
/// Cells of the index of mapped ways (metres).
const CELL: f64 = 25.0;

/// Cells of the index of buildings' kinds (degrees, about 50 m).
const KIND_CELL: f64 = 0.0005;

/// Gives each building the kind mapped at a point within its outline. The tiles and the
/// detail come from the same ways, so a building's point lies within the tile's outline of it.
pub(crate) fn buildings(buildings: &mut [Building], kinds: &[MappedAt]) {
    if kinds.is_empty() {
        return;
    }
    #[allow(clippy::cast_possible_truncation)] // degrees over a small cell size
    let cell = |(lat, lon): LatLon| {
        (
            (lat / KIND_CELL).floor() as i64,
            (lon / KIND_CELL).floor() as i64,
        )
    };
    let mut index: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, kind) in kinds.iter().enumerate() {
        let at = (f64::from(kind.at[0]) * 1e-7, f64::from(kind.at[1]) * 1e-7);
        index.entry(cell(at)).or_default().push(i);
    }
    for building in buildings {
        let Some(&first) = building.outline.first() else {
            continue;
        };
        let (low, high) =
            building
                .outline
                .iter()
                .fold((first, first), |(low, high), &(lat, lon)| {
                    (
                        (low.0.min(lat), low.1.min(lon)),
                        (high.0.max(lat), high.1.max(lon)),
                    )
                });
        let (from, to) = (cell(low), cell(high));
        'cells: for a in from.0..=to.0 {
            for b in from.1..=to.1 {
                for &i in index.get(&(a, b)).into_iter().flatten() {
                    let at = (
                        f64::from(kinds[i].at[0]) * 1e-7,
                        f64::from(kinds[i].at[1]) * 1e-7,
                    );
                    if inside(&building.outline, at) {
                        let mapped = &kinds[i];
                        building.kind = mapped.kind;
                        building.shape = mapped.shape;
                        if let Some(shape) = mapped.shape {
                            building.levels = building.levels.or(shape.levels.map(f64::from));
                            building.color = building
                                .color
                                .or(shape.colour.map(|c| c.map(|v| f32::from(v) / 255.0)));
                        }
                        break 'cells;
                    }
                }
            }
        }
    }
}

/// Whether `point` lies within the closed `ring`.
fn inside(ring: &[LatLon], (lat, lon): LatLon) -> bool {
    let mut within = false;
    for pair in ring.windows(2) {
        let ((a_lat, a_lon), (b_lat, b_lon)) = (pair[0], pair[1]);
        if (a_lat > lat) != (b_lat > lat)
            && lon < a_lon + (lat - a_lat) * (b_lon - a_lon) / (b_lat - a_lat)
        {
            within = !within;
        }
    }
    within
}

/// Gives each road the surface of the mapped ways along it, splitting it at its points where
/// that changes. Only a surface the tile's own tells no different of is taken: setts for a
/// paved road, dirt for an unpaved one, either for one the tiles tell nothing of.
pub(crate) fn surfaces(roads: &mut Vec<Road>, ways: &[SurfaceWay]) {
    let mapped: Vec<Mapped> = ways
        .iter()
        .map(|way| Mapped {
            kind: way.kind,
            line: &way.line,
        })
        .collect();
    apply(
        roads,
        &mapped,
        |road, way, _| {
            let surface = ways[way].surface;
            fits(road.details.surface, surface).then_some(surface)
        },
        |piece, surface| piece.details.surface = surface,
    );
}

/// Marks each piece of road along a mapped roundabout as going round it, drawn the way traffic
/// goes: turned where the tiles draw it the other way, as when they join a ring to a road
/// leading off it. A road is split at its points where a ring begins or ends.
pub(crate) fn roundabouts(roads: &mut Vec<Road>, rings: &[Roundabout]) {
    let mapped: Vec<Mapped> = rings
        .iter()
        .map(|ring| Mapped {
            kind: ring.kind,
            line: &ring.line,
        })
        .collect();
    apply(
        roads,
        &mapped,
        |_, _, forward| Some(forward),
        |piece, forward| {
            piece.details.roundabout = true;
            if !forward {
                piece.line.reverse();
            }
        },
    );
}

/// A mapped way: its kind and line (1e-7 degrees).
struct Mapped<'a> {
    kind: RoadKind,
    line: &'a [[i32; 2]],
}

/// Gives each road what `value` makes of the mapped way along it (by its index, and whether it
/// runs the road's way), with `give`, splitting the road at its points where that changes.
fn apply<K: Copy + PartialEq>(
    roads: &mut Vec<Road>,
    ways: &[Mapped],
    value: impl Fn(&Road, usize, bool) -> Option<K>,
    give: impl Fn(&mut Road, K),
) {
    let Some(origin) = roads.iter().find_map(|road| road.line.first().copied()) else {
        return;
    };
    if ways.is_empty() {
        return;
    }
    let flat = Flat::at(origin);
    let index = Index::new(ways, &flat);
    let mut out = Vec::with_capacity(roads.len());
    for road in roads.drain(..) {
        let points: Vec<(f64, f64)> = road.line.iter().map(|&p| flat.xy(p)).collect();
        let mut runs: Vec<Run<K>> = Vec::new();
        for (segment, pair) in points.windows(2).enumerate() {
            let found = index.along(pair[0], pair[1], &road, &value);
            let length = distance(pair[0], pair[1]);
            match runs.last_mut() {
                Some(run) if run.value == found => {
                    run.end = segment + 1;
                    run.length += length;
                }
                _ => runs.push(Run {
                    value: found,
                    start: segment,
                    end: segment + 1,
                    length,
                }),
            }
        }
        let runs = without_slivers(runs);
        if let [run] = runs.as_slice() {
            let mut road = road;
            if let Some(found) = run.value {
                give(&mut road, found);
            }
            out.push(road);
            continue;
        }
        for run in runs {
            let mut piece = road.clone();
            piece.line = road.line[run.start..=run.end].to_vec();
            if let Some(found) = run.value {
                give(&mut piece, found);
            }
            out.push(piece);
        }
    }
    *roads = out;
}

/// Consecutive segments (points `start` to `end`) of one value; `None` where no mapped way
/// tells one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run<K> {
    value: Option<K>,
    start: usize,
    end: usize,
    length: f64,
}

/// The runs with each shorter than [`SHORTEST`] joined to the one before it (the first to the
/// one after), and runs of one value then joined.
fn without_slivers<K: Copy + PartialEq>(runs: Vec<Run<K>>) -> Vec<Run<K>> {
    let mut kept: Vec<Run<K>> = Vec::with_capacity(runs.len());
    for run in runs {
        match kept.last_mut() {
            Some(last) if run.length < SHORTEST || last.value == run.value => {
                last.end = run.end;
                last.length += run.length;
            }
            Some(last) if last.length < SHORTEST => {
                *last = Run {
                    start: last.start,
                    length: last.length + run.length,
                    ..run
                };
            }
            _ => kept.push(run),
        }
    }
    kept
}

/// The mapped ways' segments in cells, projected.
struct Index {
    kinds: Vec<RoadKind>,
    /// Each way's points, projected.
    lines: Vec<Vec<(f64, f64)>>,
    /// (way, segment) by cell.
    cells: HashMap<(i64, i64), Vec<(usize, usize)>>,
}

impl Index {
    fn new(ways: &[Mapped], flat: &Flat) -> Self {
        let lines: Vec<Vec<(f64, f64)>> = ways
            .iter()
            .map(|way| {
                way.line
                    .iter()
                    .map(|&[lat, lon]| flat.xy((f64::from(lat) * 1e-7, f64::from(lon) * 1e-7)))
                    .collect()
            })
            .collect();
        let mut cells: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        for (way, line) in lines.iter().enumerate() {
            for (segment, pair) in line.windows(2).enumerate() {
                let (x0, x1) = (
                    cell(pair[0].0.min(pair[1].0) - NEAR),
                    cell(pair[0].0.max(pair[1].0) + NEAR),
                );
                let (y0, y1) = (
                    cell(pair[0].1.min(pair[1].1) - NEAR),
                    cell(pair[0].1.max(pair[1].1) + NEAR),
                );
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        cells.entry((x, y)).or_default().push((way, segment));
                    }
                }
            }
        }
        Self {
            kinds: ways.iter().map(|way| way.kind).collect(),
            lines,
            cells,
        }
    }

    /// What `value` makes of the mapped way along the road's segment from `a` to `b`: of that
    /// along most of it, taken at a quarter, the half and three quarters of its length.
    fn along<K: Copy + PartialEq>(
        &self,
        a: (f64, f64),
        b: (f64, f64),
        road: &Road,
        value: &impl Fn(&Road, usize, bool) -> Option<K>,
    ) -> Option<K> {
        let direction = unit(a, b)?;
        let found: Vec<K> = [0.25, 0.5, 0.75]
            .iter()
            .filter_map(|&t| {
                let at = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                self.nearest(at, direction, road, value)
            })
            .collect();
        found
            .iter()
            .copied()
            .find(|&one| found.iter().filter(|&&other| other == one).count() >= 2)
    }

    /// What `value` makes of the nearest mapped way of the road's kind along `direction` at
    /// `at`, of those it makes something of.
    fn nearest<K>(
        &self,
        at: (f64, f64),
        direction: (f64, f64),
        road: &Road,
        value: &impl Fn(&Road, usize, bool) -> Option<K>,
    ) -> Option<K> {
        let candidates = self.cells.get(&(cell(at.0), cell(at.1)))?;
        candidates
            .iter()
            .filter(|&&(way, _)| self.kinds[way] == road.details.kind)
            .filter_map(|&(way, segment)| {
                let (p, q) = (self.lines[way][segment], self.lines[way][segment + 1]);
                let along = unit(p, q)?;
                let cosine = along.0 * direction.0 + along.1 * direction.1;
                let off = distance_to_segment(at, p, q);
                if cosine.abs() < ALIGNED || off > NEAR {
                    return None;
                }
                Some((off, value(road, way, cosine > 0.0)?))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, found)| found)
    }
}

/// Whether a mapped surface may be that of a road the tiles say `tile` of: they tell paved
/// and unpaved from the same tag.
fn fits(tile: RoadSurface, mapped: RoadSurface) -> bool {
    let paved = matches!(
        mapped,
        RoadSurface::Concrete | RoadSurface::Paving | RoadSurface::Cobbles
    );
    match tile {
        RoadSurface::Unknown => true,
        RoadSurface::Paved => paved,
        RoadSurface::Unpaved => !paved,
        _ => false,
    }
}

/// Positions in metres east and north of an origin; enough for the few kilometres around a
/// road.
struct Flat {
    origin: LatLon,
    east: f64,
}

impl Flat {
    const NORTH: f64 = 111_195.0;

    fn at(origin: LatLon) -> Self {
        Self {
            origin,
            east: Self::NORTH * origin.0.to_radians().cos(),
        }
    }

    fn xy(&self, (lat, lon): LatLon) -> (f64, f64) {
        (
            (lon - self.origin.1) * self.east,
            (lat - self.origin.0) * Self::NORTH,
        )
    }
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell(v: f64) -> i64 {
    (v / CELL).floor() as i64
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn unit(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let length = distance(a, b);
    (length > 1e-6).then(|| ((b.0 - a.0) / length, (b.1 - a.1) / length))
}

fn distance_to_segment(at: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(at, (a.0 + dx * t, a.1 + dy * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildingKind;
    use crate::{RoadClass, RoadDetails};

    /// A point `east` and `north` metres from Bern's Zytglogge, as (lat, lon).
    fn at(east: f64, north: f64) -> LatLon {
        let flat = Flat::at((46.948, 7.447));
        (46.948 + north / Flat::NORTH, 7.447 + east / flat.east)
    }

    fn e7((lat, lon): LatLon) -> [i32; 2] {
        #[allow(clippy::cast_possible_truncation)] // positions on the map
        [(lat * 1e7).round() as i32, (lon * 1e7).round() as i32]
    }

    /// A paved local street through these points (metres east, north).
    fn street(points: &[(f64, f64)]) -> Road {
        Road {
            class: RoadClass::Street,
            details: RoadDetails {
                kind: RoadKind::Local,
                surface: RoadSurface::Paved,
                ..RoadDetails::default()
            },
            line: points.iter().map(|&(e, n)| at(e, n)).collect(),
            structure: None,
        }
    }

    fn way(id: i64, kind: RoadKind, surface: RoadSurface, points: &[(f64, f64)]) -> SurfaceWay {
        SurfaceWay {
            id,
            kind,
            surface,
            line: points.iter().map(|&(e, n)| e7(at(e, n))).collect(),
        }
    }

    fn surfaced(roads: Vec<Road>, ways: &[SurfaceWay]) -> Vec<(RoadSurface, usize)> {
        let mut roads = roads;
        surfaces(&mut roads, ways);
        roads
            .iter()
            .map(|road| (road.details.surface, road.line.len()))
            .collect()
    }

    #[test]
    fn a_cobbled_street_is_cobbled_along_its_length() {
        let road = street(&[(0.0, 0.0), (60.0, 0.0), (120.0, 0.0)]);
        // The way as mapped, its points elsewhere along the line and a metre off it.
        let sett = way(
            1,
            RoadKind::Local,
            RoadSurface::Cobbles,
            &[(-5.0, 1.0), (130.0, 1.0)],
        );

        assert_eq!(surfaced(vec![road], &[sett]), [(RoadSurface::Cobbles, 3)]);
    }

    #[test]
    fn a_street_cobbled_in_part_is_split_where_its_surface_changes() {
        let road = street(&[(0.0, 0.0), (40.0, 0.0), (80.0, 0.0), (120.0, 0.0)]);
        let sett = way(
            1,
            RoadKind::Local,
            RoadSurface::Cobbles,
            &[(0.0, 0.0), (80.0, 0.0)],
        );

        assert_eq!(
            surfaced(vec![road], &[sett]),
            [(RoadSurface::Cobbles, 3), (RoadSurface::Paved, 2)]
        );
    }

    #[test]
    fn a_paved_sidewalk_beside_an_asphalt_street_leaves_the_street_as_it_is() {
        let road = street(&[(0.0, 0.0), (100.0, 0.0)]);
        let sidewalk = way(
            1,
            RoadKind::Path,
            RoadSurface::Paving,
            &[(0.0, 3.0), (100.0, 3.0)],
        );
        // A street of the same kind beside it is another street.
        let lane = way(
            2,
            RoadKind::Local,
            RoadSurface::Paving,
            &[(0.0, 6.0), (100.0, 6.0)],
        );

        assert_eq!(
            surfaced(vec![road], &[sidewalk, lane]),
            [(RoadSurface::Paved, 2)]
        );
    }

    #[test]
    fn a_crossing_way_or_a_surface_the_tiles_deny_changes_nothing() {
        let road = street(&[(0.0, 0.0), (100.0, 0.0)]);
        let crossing = way(
            1,
            RoadKind::Local,
            RoadSurface::Cobbles,
            &[(50.0, -50.0), (50.0, 50.0)],
        );
        // The tiles say paved: a mapped dirt surface is some other way's.
        let dirt = way(
            2,
            RoadKind::Local,
            RoadSurface::Dirt,
            &[(0.0, 0.0), (100.0, 0.0)],
        );

        assert_eq!(
            surfaced(vec![road], &[crossing, dirt]),
            [(RoadSurface::Paved, 2)]
        );
    }

    #[test]
    fn a_few_metres_of_another_surface_leave_no_sliver() {
        let run = |value, start, length| Run {
            value,
            start,
            end: start + 1,
            length,
        };
        let sett = Some(RoadSurface::Cobbles);
        let dirt = Some(RoadSurface::Dirt);
        let joined = |runs: Vec<Run<RoadSurface>>| -> Vec<_> {
            without_slivers(runs)
                .iter()
                .map(|r| (r.value, r.start, r.end))
                .collect()
        };
        // Within a run, at its start and at its end.
        assert_eq!(
            joined(vec![
                run(sett, 0, 50.0),
                run(None, 1, 3.0),
                run(sett, 2, 50.0)
            ]),
            [(sett, 0, 3)]
        );
        assert_eq!(
            joined(vec![run(None, 0, 3.0), run(sett, 1, 50.0)]),
            [(sett, 0, 2)]
        );
        assert_eq!(
            joined(vec![run(sett, 0, 50.0), run(None, 1, 3.0)]),
            [(sett, 0, 2)]
        );
        // Long enough, each keeps its own.
        assert_eq!(
            joined(vec![
                run(sett, 0, 50.0),
                run(None, 1, 6.0),
                run(dirt, 2, 50.0)
            ]),
            [(sett, 0, 1), (None, 1, 2), (dirt, 2, 3)]
        );
    }

    /// The points of a ring of 15 m round (0, 0), every 45° from the south, counter-clockwise
    /// (as traffic goes where it keeps right) or not.
    fn ring(counter_clockwise: bool) -> Vec<(f64, f64)> {
        (0..=8)
            .map(|step| {
                let turn = f64::from(step) * 45.0 * if counter_clockwise { 1.0 } else { -1.0 };
                let angle = (turn - 90.0).to_radians();
                (15.0 * angle.cos(), 15.0 * angle.sin())
            })
            .collect()
    }

    fn roundabout(points: &[(f64, f64)]) -> Roundabout {
        Roundabout {
            id: 7,
            kind: RoadKind::Local,
            line: points.iter().map(|&(e, n)| e7(at(e, n))).collect(),
        }
    }

    fn rounded(roads: Vec<Road>, rings: &[Roundabout]) -> Vec<Road> {
        let mut roads = roads;
        roundabouts(&mut roads, rings);
        roads
    }

    #[test]
    fn a_ring_the_tiles_draw_against_the_traffic_is_turned_and_marked() {
        let tiles = street(&ring(false));

        let roads = rounded(vec![tiles], &[roundabout(&ring(true))]);

        assert_eq!(roads.len(), 1);
        assert!(roads[0].details.roundabout);
        let expected = street(&ring(true)).line;
        for (got, want) in roads[0].line.iter().zip(&expected) {
            assert!((got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9);
        }
    }

    #[test]
    fn a_ring_the_tiles_join_to_a_road_leading_off_it_is_split_off_and_turned() {
        // The road from the south, then round the ring the wrong way, as one line.
        let mut points = vec![(0.0, -60.0)];
        points.extend(ring(false));
        let tiles = street(&points);

        let roads = rounded(vec![tiles], &[roundabout(&ring(true))]);

        let shape: Vec<(bool, usize)> = roads
            .iter()
            .map(|r| (r.details.roundabout, r.line.len()))
            .collect();
        assert_eq!(shape, [(false, 2), (true, 9)]);
        let first = roads[1].line[1];
        let want = at(
            15.0 * (-45.0_f64).to_radians().cos(),
            15.0 * (-45.0_f64).to_radians().sin(),
        );
        assert!(
            (first.0 - want.0).abs() < 1e-9 && (first.1 - want.1).abs() < 1e-9,
            "on round counter-clockwise: {first:?}"
        );
    }

    #[test]
    fn a_street_passing_a_roundabout_is_none() {
        let passing = street(&[(-100.0, 25.0), (100.0, 25.0)]);
        let roads = rounded(vec![passing.clone()], &[roundabout(&ring(true))]);
        assert_eq!(roads, [passing]);
    }

    #[test]
    fn without_detail_the_roads_stay_as_they_are() {
        let roads = vec![street(&[(0.0, 0.0), (100.0, 0.0)])];
        let mut same = roads.clone();
        surfaces(&mut same, &[]);
        assert_eq!(same, roads);
    }

    #[test]
    fn buildings_take_the_kind_mapped_within_them() {
        let square = |lat: f64, lon: f64| {
            let d = 0.0001;
            vec![
                (lat, lon),
                (lat, lon + d),
                (lat + d, lon + d),
                (lat + d, lon),
                (lat, lon),
            ]
        };
        let building = |id, outline| Building {
            id,
            outline,
            height: None,
            levels: None,
            color: None,
            min_height: None,
            in_parts: false,
            kind: None,
            shape: None,
        };
        let mut buildings = vec![
            building(1, square(46.948, 7.447)),
            building(2, square(46.9483, 7.447)),
        ];
        let kinds = [MappedAt {
            at: [469_480_500, 74_470_500],
            kind: Some(BuildingKind::Barn),
            shape: None,
        }];
        super::buildings(&mut buildings, &kinds);
        assert_eq!(buildings[0].kind, Some(BuildingKind::Barn));
        assert_eq!(
            buildings[1].kind, None,
            "its neighbour's point is not within it"
        );
    }
}
