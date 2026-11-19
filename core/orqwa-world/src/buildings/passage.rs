//! Buildings where the road ridden or a street runs. Roads stay clear (#100, #138), but the
//! buildings stay too: where the road is mapped wider than the gap between them, the road
//! narrows (`RoadIndex::fit_between`); what still stands on it is cut along it. The pieces
//! beside the road stand on their own; a piece across it, where the road or a street runs
//! through the building as through a gate or a passage, is raised for riders to pass beneath.

use std::collections::HashMap;

use super::shape::{contains, signed_area};
use super::{Point, drawn_outline, outline_distance};
use crate::road::{NARROWEST, RoadIndex};

/// A street's middle this far inside a building's outline runs through it; less is taken
/// for the map's inaccuracy, as in old towns' narrow lanes.
const THROUGH_DEPTH: f64 = 1.0;
/// Walls reaching less than this onto the road are taken as standing by it.
const TOLERANCE: f64 = 0.05;
/// A piece is cut again where the road bends away from the line of the first cut, along each
/// stretch of road it reaches onto, at most this often (a stretch is a few metres long)...
const ROUNDS: usize = 24;
/// ...after which a piece reaching no further than this onto the road, the lip of its edge,
/// stands as it is, and one reaching further is left out.
const LIP: f64 = 0.3;
/// Pieces smaller or thinner than this (m², m) are left out: scraps of a wall, no building.
const SCRAP: f64 = 4.0;
const SLIVER: f64 = 0.75;
/// Cells of the street index.
const CELL: f64 = 50.0;

/// What passes beneath a building without cutting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Beneath {
    /// Nothing: it stands on the ground.
    Nothing,
    /// Streets: it is a roof on posts.
    Streets,
    /// The road and streets: it is mapped raised off the ground as high as a passage (a
    /// bridge between two buildings, a building's upper floors over the road).
    Everything,
}

impl Beneath {
    /// What passes beneath `building`.
    pub(crate) fn of(building: &orqwa_osm::Building) -> Self {
        if building.min_height.is_some_and(|h| h >= OVERHEAD) {
            Self::Everything
        } else if building.kind == Some(orqwa_osm::BuildingKind::Carport) {
            Self::Streets
        } else {
            Self::Nothing
        }
    }
}

/// A building mapped this high off the ground spans the road: riders pass beneath it.
const OVERHEAD: f64 = 3.0;

/// How a building is cut by the road or a street running through it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum Cut {
    /// Not at all.
    #[default]
    Whole,
    /// A piece beside the road, of a building with this area.
    Beside(f64),
    /// The piece over the road, raised for riders to pass beneath, of a building with this
    /// area.
    Over(f64),
}

impl Cut {
    /// The area of the whole building a piece was cut from, which says what it is.
    pub(crate) fn whole_area(self) -> Option<f64> {
        match self {
            Self::Whole => None,
            Self::Beside(area) | Self::Over(area) => Some(area),
        }
    }
}

/// The streets buildings keep clear of: paved ones on the ground.
pub(crate) struct Streets {
    lines: Vec<Vec<Point>>,
    /// The segments (line, first point) in or by each cell.
    cells: HashMap<(i64, i64), Vec<(usize, usize)>>,
}

impl Streets {
    pub(crate) fn new(lines: Vec<Vec<Point>>) -> Self {
        let mut cells: HashMap<_, Vec<_>> = HashMap::new();
        for (l, line) in lines.iter().enumerate() {
            for (s, pair) in line.windows(2).enumerate() {
                let (low, high) = bounds(pair);
                for cell in cells_over(low, high) {
                    cells.entry(cell).or_default().push((l, s));
                }
            }
        }
        Self { lines, cells }
    }

    /// Where a street runs through `outline`, its middle deeper inside than
    /// `THROUGH_DEPTH`, entering and leaving it: the first and last point of that run. A
    /// street ending inside, as a lane does at a barn's door, runs through nothing.
    fn through(&self, outline: &[Point]) -> Option<(Point, Point)> {
        let (low, high) = bounds(outline);
        let mut near: HashMap<usize, (usize, usize)> = HashMap::new();
        for cell in cells_over(low, high) {
            for &(l, s) in self.cells.get(&cell).into_iter().flatten() {
                let range = near.entry(l).or_insert((s, s));
                *range = (range.0.min(s), range.1.max(s));
            }
        }
        let mut near: Vec<_> = near.into_iter().collect();
        near.sort_unstable_by_key(|&(l, _)| l);
        near.into_iter().find_map(|(l, (first, last))| {
            let line = &self.lines[l];
            let mut points = crate::drape::densify(&line[first..=last + 1], 1.0);
            points.push(line[last + 1]);
            let deep: Vec<bool> = points
                .iter()
                .map(|&p| contains(outline, p) && outline_distance(outline, p) > THROUGH_DEPTH)
                .collect();
            let start = deep.iter().position(|&d| d)?;
            let end = start + deep[start..].iter().position(|&d| !d)? - 1;
            (start > 0).then_some((points[start], points[end]))
        })
    }
}

/// A line a building is cut along: the middle of the road or street, its unit direction,
/// the half width kept clear and whether the piece across it is raised over it (not under a
/// bridge's deck).
struct Line {
    origin: Point,
    along: Point,
    half: f64,
    raise: bool,
}

impl Line {
    /// Distance left of the line.
    fn offset(&self, (e, n): Point) -> f64 {
        self.along.0 * (n - self.origin.1) - self.along.1 * (e - self.origin.0)
    }
}

/// The pieces a building with `footprint` stands as: the footprint itself if neither the
/// road nor a street runs where it is drawn, or what does passes `beneath` it, else its drawn
/// outline cut along them.
pub(crate) fn pieces(
    footprint: Vec<Point>,
    beneath: Beneath,
    road: &RoadIndex,
    streets: &Streets,
) -> Vec<(Vec<Point>, Cut)> {
    if beneath == Beneath::Everything {
        return vec![(footprint, Cut::Whole)];
    }
    let none = Streets::new(Vec::new());
    let streets = if beneath == Beneath::Streets {
        &none
    } else {
        streets
    };
    let outline = drawn_outline(&footprint);
    if crossing(&outline, road, streets).is_none() {
        return vec![(footprint, Cut::Whole)];
    }
    let area = signed_area(&footprint);
    let mut kept = Vec::new();
    let mut queue = vec![(outline, 0)];
    while let Some((piece, round)) = queue.pop() {
        let Some((depth, line)) = crossing(&piece, road, streets) else {
            kept.push((piece, Cut::Beside(area)));
            continue;
        };
        if round == ROUNDS {
            if depth <= LIP {
                kept.push((piece, Cut::Beside(area)));
            }
            continue;
        }
        let (left, rest) = split(&piece, &line, line.half);
        let (over, right) = rest.iter().map(|p| split(p, &line, -line.half)).fold(
            (Vec::new(), Vec::new()),
            |mut all, (o, r)| {
                all.0.extend(o);
                all.1.extend(r);
                all
            },
        );
        queue.extend(
            left.into_iter()
                .chain(right)
                .filter(|p| substantial(p))
                .map(|p| (p, round + 1)),
        );
        kept.extend(
            over.into_iter()
                .filter(|p| line.raise && substantial(p) && straddles(p, &line))
                .map(|p| (p, Cut::Over(area))),
        );
    }
    kept
}

/// The line along which the road or a street runs where `outline` stands, and how far it
/// reaches across its edge: the stretch of road it reaches deepest onto, or else a street
/// running through it.
fn crossing(outline: &[Point], road: &RoadIndex, streets: &Streets) -> Option<(f64, Line)> {
    let mut deepest: Option<(f64, Line)> = None;
    let near = outline
        .first()
        .is_some_and(|&(e, n)| road.carriageway_near(e, n).next().is_some());
    if near {
        let mut ring = outline.to_vec();
        ring.push(outline[0]);
        for (e, n) in crate::drape::densify(&ring, 0.5) {
            for (a, b, half, bridge) in road.carriageway_near(e, n) {
                let depth = half - segment_distance((e, n), a, b);
                let length = (b.0 - a.0).hypot(b.1 - a.1);
                if depth > TOLERANCE
                    && length > 1e-6
                    && deepest.as_ref().is_none_or(|d| depth > d.0)
                {
                    let along = ((b.0 - a.0) / length, (b.1 - a.1) / length);
                    let line = Line {
                        origin: a,
                        along,
                        half,
                        raise: !bridge,
                    };
                    deepest = Some((depth, line));
                }
            }
        }
    }
    deepest.or_else(|| {
        let (entry, exit) = streets.through(outline)?;
        let length = (exit.0 - entry.0).hypot(exit.1 - entry.1);
        let line = Line {
            origin: entry,
            along: ((exit.0 - entry.0) / length, (exit.1 - entry.1) / length),
            half: NARROWEST / 2.0,
            raise: true,
        };
        (length > 1e-6).then_some((THROUGH_DEPTH + line.half, line))
    })
}

/// The pieces of a simple polygon left of `line` moved `at` to its left, and right of it,
/// each kept in the polygon's winding. Where the line runs through the polygon more than
/// once, the stretches of it inside pair up in their order along it.
fn split(polygon: &[Point], line: &Line, at: f64) -> (Vec<Vec<Point>>, Vec<Vec<Point>>) {
    // A corner on the line counts as left of it, so the line never runs through one.
    let sides: Vec<bool> = polygon
        .iter()
        .map(|&p| line.offset(p) - at >= 0.0)
        .collect();
    if sides.iter().all(|&s| s) {
        return (vec![polygon.to_vec()], Vec::new());
    }
    if sides.iter().all(|&s| !s) {
        return (Vec::new(), vec![polygon.to_vec()]);
    }
    // The ring with the crossings put in: a corner's side, or a crossing's place along the line.
    let mut ring: Vec<(Point, Result<bool, f64>)> = Vec::new();
    let n = polygon.len();
    for i in 0..n {
        let (p, q) = (polygon[i], polygon[(i + 1) % n]);
        ring.push((p, Ok(sides[i])));
        if sides[i] != sides[(i + 1) % n] {
            let (sp, sq) = (line.offset(p) - at, line.offset(q) - at);
            let t = sp / (sp - sq);
            let cross = (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
            ring.push((cross, Err(cross.0 * line.along.0 + cross.1 * line.along.1)));
        }
    }
    let mut crossings: Vec<(usize, f64)> = ring
        .iter()
        .enumerate()
        .filter_map(|(i, (_, kind))| kind.err().map(|place| (i, place)))
        .collect();
    crossings.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut partner = vec![usize::MAX; ring.len()];
    for pair in crossings.as_chunks::<2>().0 {
        partner[pair[0].0] = pair[1].0;
        partner[pair[1].0] = pair[0].0;
    }
    let mut visited = vec![false; ring.len()];
    let mut pieces = (Vec::new(), Vec::new());
    for start in 0..ring.len() {
        let Ok(left) = ring[start].1 else {
            continue;
        };
        if visited[start] {
            continue;
        }
        let mut piece = Vec::new();
        let mut at = start;
        // Walk the ring, along the line from each crossing to its partner, back to the start.
        for _ in 0..=2 * ring.len() {
            piece.push(ring[at].0);
            visited[at] = true;
            let next = (at + 1) % ring.len();
            at = if ring[next].1.is_err() {
                piece.push(ring[next].0);
                let across = partner[next];
                if across == usize::MAX {
                    // Not a simple polygon: leave it whole.
                    return (vec![polygon.to_vec()], Vec::new());
                }
                piece.push(ring[across].0);
                (across + 1) % ring.len()
            } else {
                next
            };
            if at == start {
                break;
            }
        }
        let piece = tidy(piece);
        if piece.len() >= 3 {
            if left {
                pieces.0.push(piece);
            } else {
                pieces.1.push(piece);
            }
        }
    }
    pieces
}

/// A ring without repeated corners.
fn tidy(mut ring: Vec<Point>) -> Vec<Point> {
    ring.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 1e-6);
    while ring.len() > 1 && {
        let (a, b) = (ring[0], ring[ring.len() - 1]);
        (a.0 - b.0).hypot(a.1 - b.1) < 1e-6
    } {
        ring.pop();
    }
    ring
}

/// Whether a piece is big and thick enough to stand as a building.
fn substantial(piece: &[Point]) -> bool {
    let area = signed_area(piece).abs();
    let perimeter: f64 = piece
        .iter()
        .zip(piece.iter().cycle().skip(1))
        .map(|(a, b)| (b.0 - a.0).hypot(b.1 - a.1))
        .sum();
    area >= SCRAP && 2.0 * area / perimeter >= SLIVER
}

/// Whether a piece reaches across the middle of the road or street.
fn straddles(piece: &[Point], line: &Line) -> bool {
    let offsets = piece.iter().map(|&p| line.offset(p));
    offsets.clone().any(|o| o > TOLERANCE) && offsets.into_iter().any(|o| o < -TOLERANCE)
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (de, dn) = (b.0 - a.0, b.1 - a.1);
    let length_squared = de * de + dn * dn;
    let t = if length_squared > 0.0 {
        (((p.0 - a.0) * de + (p.1 - a.1) * dn) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - a.0 - de * t).hypot(p.1 - a.1 - dn * t)
}

fn bounds(points: &[Point]) -> (Point, Point) {
    points.iter().fold(
        ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)),
        |(low, high), p| {
            (
                (low.0.min(p.0), low.1.min(p.1)),
                (high.0.max(p.0), high.1.max(p.1)),
            )
        },
    )
}

fn cells_over(low: Point, high: Point) -> impl Iterator<Item = (i64, i64)> {
    #[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
    let cell = |v: f64| (v / CELL).floor() as i64;
    let (low, high) = ((cell(low.0), cell(low.1)), (cell(high.0), cell(high.1)));
    (low.0..=high.0).flat_map(move |x| (low.1..=high.1).map(move |y| (x, y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_north(half: f64) -> Line {
        Line {
            origin: (0.0, 0.0),
            along: (0.0, 1.0),
            half,
            raise: true,
        }
    }

    fn area(pieces: &[Vec<Point>]) -> f64 {
        pieces.iter().map(|p| signed_area(p)).sum()
    }

    #[test]
    fn a_cut_divides_a_building_into_pieces_keeping_its_area_and_winding() {
        // A U open to the north, its arms either side of a line due north: one piece left
        // of it, the arm and the base right of it as one piece.
        let u = vec![
            (-6.0, 0.0),
            (6.0, 0.0),
            (6.0, 10.0),
            (2.0, 10.0),
            (2.0, 4.0),
            (-2.0, 4.0),
            (-2.0, 10.0),
            (-6.0, 10.0),
        ];
        // East of the line is its right.
        let (left, right) = split(&u, &line_north(0.0), 1.0);
        assert_eq!((left.len(), right.len()), (1, 1));
        assert!((area(&left) + area(&right) - signed_area(&u)).abs() < 1e-9);
        assert!(left.iter().chain(&right).all(|p| signed_area(p) > 0.0));
        // A line through both arms above the base: one piece of the base and two arms.
        let across = Line {
            origin: (0.0, 6.0),
            along: (1.0, 0.0),
            half: 0.0,
            raise: true,
        };
        let (north, south) = split(&u, &across, 0.0);
        assert_eq!((north.len(), south.len()), (2, 1));
        assert!((area(&north) + area(&south) - signed_area(&u)).abs() < 1e-9);
    }

    #[test]
    fn a_line_missing_a_building_leaves_it_whole() {
        let square = vec![(2.0, 0.0), (6.0, 0.0), (6.0, 4.0), (2.0, 4.0)];
        assert_eq!(
            split(&square, &line_north(0.0), 0.0),
            (Vec::new(), vec![square])
        );
    }

    #[test]
    fn only_pieces_across_the_middle_are_over_it_and_scraps_are_no_buildings() {
        let line = line_north(2.0);
        let across = vec![(-2.0, 0.0), (2.0, 0.0), (2.0, 8.0), (-2.0, 8.0)];
        let edge = vec![(1.0, 0.0), (2.0, 0.0), (2.0, 8.0), (1.0, 8.0)];
        assert!(straddles(&across, &line));
        assert!(!straddles(&edge, &line));
        assert!(substantial(&across));
        assert!(!substantial(&[
            (0.0, 0.0),
            (0.5, 0.0),
            (0.5, 20.0),
            (0.0, 20.0)
        ]));
    }
}
