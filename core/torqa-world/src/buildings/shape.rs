//! Plane geometry of footprints: orientation, area, triangulation, the rectangle a footprint
//! fills and outlines moved in or out.

use torqa_osm::Building;
use torqa_routes::LocalProjection;

/// A point in metres east/north.
pub(crate) type Point = (f64, f64);

/// Footprint in metres east/north, counter-clockwise, without the closing point.
pub(crate) fn footprint(building: &Building, projection: &LocalProjection) -> Vec<Point> {
    let mut points: Vec<Point> = building
        .outline
        .iter()
        .map(|&(lat, lon)| projection.project(lat, lon))
        .collect();
    points.pop(); // closing point
    points.dedup_by(|a, b| distance(*a, *b) < 0.05);
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    points
}

pub(crate) fn distance(a: Point, b: Point) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// Twice the signed area is avoided: this is the true area, positive for counter-clockwise.
pub(crate) fn signed_area(points: &[Point]) -> f64 {
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        sum += a.0 * b.1 - b.0 * a.1;
    }
    sum / 2.0
}

pub(crate) fn perimeter(points: &[Point]) -> f64 {
    (0..points.len())
        .map(|i| distance(points[i], points[(i + 1) % points.len()]))
        .sum()
}

/// The centre of mass of a polygon (its first corner if it has no area).
pub(crate) fn centroid(points: &[Point]) -> Point {
    let area = signed_area(points);
    let Some(&first) = points.first() else {
        return (0.0, 0.0);
    };
    if area.abs() < 1e-9 {
        return first;
    }
    let (mut e, mut n) = (0.0, 0.0);
    for i in 0..points.len() {
        // Relative to the first corner: world coordinates would cost precision.
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        let (a, b) = (
            (a.0 - first.0, a.1 - first.1),
            (b.0 - first.0, b.1 - first.1),
        );
        let cross = a.0 * b.1 - b.0 * a.1;
        e += (a.0 + b.0) * cross;
        n += (a.1 + b.1) * cross;
    }
    (first.0 + e / (6.0 * area), first.1 + n / (6.0 * area))
}

/// Whether a point lies inside a polygon (even-odd rule).
pub(crate) fn contains(ring: &[Point], (e, n): Point) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a.1 > n) != (b.1 > n) && e < (b.0 - a.0) * (n - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

/// Ear-clipping triangulation of a simple counter-clockwise polygon (vertex indices).
pub(crate) fn triangulate(points: &[Point]) -> Vec<[u32; 3]> {
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut triangles = Vec::new();
    let mut guard = 0;
    while remaining.len() > 3 && guard < points.len() * points.len() {
        guard += 1;
        let count = remaining.len();
        let ear = (0..count).find(|&i| {
            let (prev, cur, next) = (
                remaining[(i + count - 1) % count],
                remaining[i],
                remaining[(i + 1) % count],
            );
            let (a, b, c) = (points[prev], points[cur], points[next]);
            if cross(a, b, c) <= 0.0 {
                return false; // reflex or degenerate corner
            }
            remaining.iter().all(|&other| {
                other == prev
                    || other == cur
                    || other == next
                    || !(cross(a, b, points[other]) > 0.0
                        && cross(b, c, points[other]) > 0.0
                        && cross(c, a, points[other]) > 0.0)
            })
        });
        // Self-intersecting or degenerate outlines have no ear left; drop the worst corner.
        let i = ear.unwrap_or(0);
        let (prev, cur, next) = (
            remaining[(i + count - 1) % count],
            remaining[i],
            remaining[(i + 1) % count],
        );
        if ear.is_some() {
            triangles.push([prev, cur, next].map(|v| u32::try_from(v).expect("small polygon")));
        }
        remaining.remove(i);
    }
    if remaining.len() == 3 {
        triangles.push(
            [remaining[0], remaining[1], remaining[2]]
                .map(|v| u32::try_from(v).expect("small polygon")),
        );
    }
    triangles
}

/// Positive when `a`, `b`, `c` turn counter-clockwise.
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// An oriented rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) centre: Point,
    /// Unit direction of the longer sides.
    pub(crate) axis: Point,
    /// Half the length, along `axis`.
    pub(crate) half_length: f64,
    /// Half the width, across `axis`; never more than `half_length`.
    pub(crate) half_width: f64,
}

impl Rect {
    /// The smallest rectangle around `points`; one of its sides lies along an edge of their
    /// convex hull, so trying each hull edge finds it.
    pub(crate) fn around(points: &[Point]) -> Option<Self> {
        let hull = convex_hull(points);
        let mut best: Option<(f64, Self)> = None;
        for i in 0..hull.len() {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            let length = distance(a, b);
            if length < 1e-6 {
                continue;
            }
            let u = ((b.0 - a.0) / length, (b.1 - a.1) / length);
            let v = (-u.1, u.0);
            let (mut low, mut high) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
            for p in &hull {
                // Relative to `a`: world coordinates would cost precision.
                let (de, dn) = (p.0 - a.0, p.1 - a.1);
                let (along, across) = (de * u.0 + dn * u.1, de * v.0 + dn * v.1);
                low = (low.0.min(along), low.1.min(across));
                high = (high.0.max(along), high.1.max(across));
            }
            let (length, width) = (high.0 - low.0, high.1 - low.1);
            if best.is_some_and(|(area, _)| area <= length * width) {
                continue;
            }
            let (along, across) = (f64::midpoint(low.0, high.0), f64::midpoint(low.1, high.1));
            let centre = (
                a.0 + u.0 * along + v.0 * across,
                a.1 + u.1 * along + v.1 * across,
            );
            let rect = if length >= width {
                Self {
                    centre,
                    axis: u,
                    half_length: length / 2.0,
                    half_width: width / 2.0,
                }
            } else {
                Self {
                    centre,
                    axis: v,
                    half_length: width / 2.0,
                    half_width: length / 2.0,
                }
            };
            best = Some((length * width, rect));
        }
        best.map(|(_, rect)| rect)
    }

    /// A square of half side `half` around `centre`, its sides along `axis`.
    pub(crate) fn square(centre: Point, axis: Point, half: f64) -> Self {
        Self {
            centre,
            axis,
            half_length: half,
            half_width: half,
        }
    }

    /// The same rectangle turned a quarter: its axis along the former width.
    pub(crate) fn turned(self) -> Self {
        Self {
            axis: self.across(),
            half_length: self.half_width,
            half_width: self.half_length,
            ..self
        }
    }

    pub(crate) fn area(&self) -> f64 {
        4.0 * self.half_length * self.half_width
    }

    /// Unit direction across the rectangle, to the left of `axis`.
    pub(crate) fn across(&self) -> Point {
        (-self.axis.1, self.axis.0)
    }

    /// The point `along` the axis and `across` it from the centre.
    pub(crate) fn point(&self, along: f64, across: f64) -> Point {
        let side = self.across();
        (
            self.centre.0 + self.axis.0 * along + side.0 * across,
            self.centre.1 + self.axis.1 * along + side.1 * across,
        )
    }

    /// Corners, counter-clockwise.
    pub(crate) fn corners(&self) -> Vec<Point> {
        let (l, w) = (self.half_length, self.half_width);
        vec![
            self.point(-l, -w),
            self.point(l, -w),
            self.point(l, w),
            self.point(-l, w),
        ]
    }
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let half = |points: &mut dyn Iterator<Item = Point>| {
        let mut chain: Vec<Point> = Vec::new();
        for p in points {
            while chain.len() >= 2
                && cross(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0
            {
                chain.pop();
            }
            chain.push(p);
        }
        chain.pop(); // the next chain starts with it
        chain
    };
    let mut hull = half(&mut sorted.iter().copied());
    hull.extend(half(&mut sorted.iter().rev().copied()));
    hull
}

/// The outline with every edge moved `distance` inwards (outwards when negative), corners
/// mitred. `None` where that would fold it: edges turning round, or an inward outline
/// leaving the original.
pub(crate) fn offset(ring: &[Point], distance: f64) -> Option<Vec<Point>> {
    let count = ring.len();
    let inward = |a: Point, b: Point| {
        let length = self::distance(a, b);
        (length > 1e-6).then(|| ((a.1 - b.1) / length, (b.0 - a.0) / length))
    };
    let mut moved = Vec::with_capacity(count);
    for i in 0..count {
        let (prev, cur, next) = (
            ring[(i + count - 1) % count],
            ring[i],
            ring[(i + 1) % count],
        );
        let (n1, n2) = (inward(prev, cur)?, inward(cur, next)?);
        // Sharp corners would send the mitre far away; capping it rounds them a little.
        let scale = distance / (1.0 + n1.0 * n2.0 + n1.1 * n2.1).max(0.25);
        moved.push((cur.0 + (n1.0 + n2.0) * scale, cur.1 + (n1.1 + n2.1) * scale));
    }
    for i in 0..count {
        let (a, b) = (ring[i], ring[(i + 1) % count]);
        let (c, d) = (moved[i], moved[(i + 1) % count]);
        if (b.0 - a.0) * (d.0 - c.0) + (b.1 - a.1) * (d.1 - c.1) <= 0.0 {
            return None;
        }
    }
    let inside = distance <= 0.0 || moved.iter().all(|&p| contains(ring, p));
    (inside && signed_area(&moved) > 0.0).then_some(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const L_SHAPE: [Point; 6] = [
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (10.0, 10.0),
        (10.0, 20.0),
        (0.0, 20.0),
    ];

    #[test]
    fn triangulates_concave_outlines() {
        // An L-shaped house: 6 corners, 4 triangles, covering 300 m².
        let triangles = triangulate(&L_SHAPE);

        assert_eq!(triangles.len(), 4);
        let area: f64 = triangles
            .iter()
            .map(|t| signed_area(&t.map(|i| L_SHAPE[i as usize])))
            .sum();
        assert!((area - 300.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn finds_the_rectangle_a_turned_house_fills() {
        // A 16 × 9 m house turned 30°, mapped with an extra corner on one side.
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let turn = |(x, y): Point| (100.0 + x * cos - y * sin, 200.0 + x * sin + y * cos);
        let outline: Vec<Point> = [(0.0, 0.0), (8.0, 0.0), (16.0, 0.0), (16.0, 9.0), (0.0, 9.0)]
            .into_iter()
            .map(turn)
            .collect();

        let rect = Rect::around(&outline).unwrap();

        assert!((rect.half_length - 8.0).abs() < 1e-9);
        assert!((rect.half_width - 4.5).abs() < 1e-9);
        // The axis runs along the long side, either way round.
        assert!((rect.axis.0 * cos + rect.axis.1 * sin).abs() > 1.0 - 1e-9);
        let centre = turn((8.0, 4.5));
        assert!(distance(rect.centre, centre) < 1e-9);
        assert!((signed_area(&rect.corners()) - 144.0).abs() < 1e-6);
    }

    #[test]
    fn an_l_shaped_house_fills_little_of_its_rectangle() {
        let rect = Rect::around(&L_SHAPE).unwrap();

        assert!((rect.area() - 400.0).abs() < 1e-9);
        assert!(signed_area(&L_SHAPE) / rect.area() < 0.8);
    }

    #[test]
    fn outlines_move_in_and_out_evenly() {
        let inner = offset(&L_SHAPE, 2.0).unwrap();
        let outer = offset(&L_SHAPE, -1.0).unwrap();

        // Inwards: the L with 6 m arms, 2 m from every wall.
        assert!(inner.iter().all(|&p| contains(&L_SHAPE, p)));
        assert!((signed_area(&inner) - (2.0 * 16.0 * 6.0 - 36.0)).abs() < 1e-6);
        // Outwards: 1 m beyond every wall.
        assert!(outer.iter().all(|&p| !contains(&L_SHAPE, p)));
        assert!((signed_area(&outer) - (2.0 * 22.0 * 12.0 - 144.0)).abs() < 1e-6);
        // Too far in, the arms would turn inside out.
        assert_eq!(offset(&L_SHAPE, 6.0), None);
    }

    #[test]
    fn centroids_lie_at_the_centre_of_mass() {
        let (e, n) = centroid(&L_SHAPE);

        // Two 10 × 10 squares at (5, 5) and (15, 5) plus one at (5, 15).
        assert!((e - 25.0 / 3.0).abs() < 1e-9 && (n - 25.0 / 3.0).abs() < 1e-9);
    }
}
