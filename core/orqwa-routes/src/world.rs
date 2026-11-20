//! Routes of world files (R267, ADR 0042): the line an author drew through their world, ridden
//! as it is. Unlike a GPX track it is not matched to a map, its elevations are not corrected or
//! smoothed, and its distances are the world's own flat metres.

use orqwa_domain::units::Meters;

use crate::{
    ElevationSource, LocalProjection, RawPoint, Route, RouteError, RoutePoint, SPACING, Surface,
    climbs, key_of,
};

/// A point of a world's route, in the world's flat metres from its origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPoint {
    /// Metres east of the origin.
    pub east: f64,
    /// Metres north of the origin.
    pub north: f64,
    /// Metres above sea level.
    pub elevation: f64,
    /// What carries the road from this point to the next.
    pub surface: Surface,
}

impl Route {
    /// The route `points` draw through a world whose (0, 0) lies at `origin` (latitude,
    /// longitude). It is sampled every 10 m along the line, horizontally in the world's metres,
    /// with straight lines between the points; elevations are interpolated, never smoothed.
    /// Positions along it are the world's own metres ([`Route::origin`]).
    ///
    /// # Errors
    /// [`RouteError::TooShort`] without two points at least 10 cm apart.
    pub fn from_world(
        name: Option<String>,
        origin: (f64, f64),
        points: &[WorldPoint],
    ) -> Result<Self, RouteError> {
        let mut line: Vec<WorldPoint> = Vec::with_capacity(points.len());
        for &point in points {
            if line.last().is_none_or(|last| flat(last, &point) >= 0.1) {
                line.push(point);
            }
        }
        if line.len() < 2 {
            return Err(RouteError::TooShort);
        }
        let projection = LocalProjection::at(origin.0, origin.1);
        let mut samples = vec![(0.0, line[0])];
        let mut next = SPACING;
        let mut travelled = 0.0;
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = flat(&a, &b);
            while next < travelled + length {
                let t = (next - travelled) / length;
                let at = WorldPoint {
                    east: a.east + (b.east - a.east) * t,
                    north: a.north + (b.north - a.north) * t,
                    elevation: a.elevation + (b.elevation - a.elevation) * t,
                    surface: a.surface,
                };
                samples.push((next, at));
                next += SPACING;
            }
            travelled += length;
        }
        let end = line[line.len() - 1];
        if travelled - samples.last().map_or(0.0, |s| s.0) >= 0.1 {
            samples.push((travelled, end));
        }
        let points: Vec<RoutePoint> = samples
            .iter()
            .map(|&(distance, point)| {
                let (lat, lon) = projection.unproject(point.east, point.north);
                RoutePoint {
                    lat,
                    lon,
                    elevation: Meters(point.elevation),
                    distance: Meters(distance),
                    surface: point.surface,
                }
            })
            .collect();
        let raw: Vec<RawPoint> = points
            .iter()
            .map(|p| RawPoint {
                lat: p.lat,
                lon: p.lon,
                elevation: Some(p.elevation.0),
                time: None,
            })
            .collect();
        Ok(Self {
            key: key_of(&raw),
            name,
            climbs: climbs::detect(&points),
            points,
            elevation_source: ElevationSource::World,
            cuts: Vec::new(),
            origin: Some(origin),
        })
    }
}

fn flat(a: &WorldPoint, b: &WorldPoint) -> f64 {
    (b.east - a.east).hypot(b.north - a.north)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: (f64, f64) = (47.09, 7.16);

    fn line(steps: usize, step: f64, elevation: impl Fn(f64) -> f64) -> Vec<WorldPoint> {
        (0..=steps)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)] // test indices
                let along = i as f64 * step;
                WorldPoint {
                    east: 5000.0 + along * 0.6,
                    north: 8000.0 + along * 0.8,
                    elevation: elevation(along),
                    surface: Surface::Ground,
                }
            })
            .collect()
    }

    #[test]
    fn its_elevations_are_ridden_as_authored() {
        // A sharp crest: 10 % up, 10 % down. A GPX route would be smoothed; a world's is not.
        let route = Route::from_world(
            None,
            ORIGIN,
            &line(100, 5.0, |d| 400.0 + 0.1 * (250.0 - (d - 250.0).abs())),
        )
        .unwrap();
        assert!((route.length().0 - 500.0).abs() < 1e-6);
        assert!((route.grade_at(Meters(200.0)).0 - 10.0).abs() < 1e-6);
        assert!((route.grade_at(Meters(300.0)).0 + 10.0).abs() < 1e-6);
        let top = route.position(Meters(250.0)).elevation.0;
        assert!((top - 425.0).abs() < 1e-6, "{top}");
        assert!((route.max_grade().0 - 10.0).abs() < 1e-6);
        assert_eq!(route.elevation_source(), ElevationSource::World);
    }

    #[test]
    fn positions_along_it_are_the_worlds_own_metres() {
        // Far from the origin, where another projection would be metres off.
        let points = line(400, 10.0, |_| 500.0);
        let route = Route::from_world(None, ORIGIN, &points).unwrap();
        let projection = LocalProjection::for_route(&route);
        for (distance, point) in [
            (0.0, points[0]),
            (2500.0, points[250]),
            (4000.0, points[400]),
        ] {
            let at = route.position(Meters(distance));
            let (east, north) = projection.project(at.lat, at.lon);
            assert!((east - point.east).abs() < 1e-3, "{east} vs {}", point.east);
            assert!(
                (north - point.north).abs() < 1e-3,
                "{north} vs {}",
                point.north
            );
        }
        assert_eq!(route.origin(), ORIGIN);
    }

    #[test]
    fn bridges_and_tunnels_carry_through() {
        let mut points = line(40, 10.0, |_| 450.0);
        for point in &mut points[10..20] {
            point.surface = Surface::Bridge;
        }
        let route = Route::from_world(None, ORIGIN, &points).unwrap();
        let surface = |d: f64| {
            route
                .points()
                .iter()
                .find(|p| (p.distance.0 - d).abs() < 1e-6)
                .unwrap()
                .surface
        };
        assert_eq!(surface(50.0), Surface::Ground);
        assert_eq!(surface(150.0), Surface::Bridge);
        assert_eq!(surface(250.0), Surface::Ground);
    }

    #[test]
    fn records_follow_the_line_not_its_elevations() {
        let flat = Route::from_world(None, ORIGIN, &line(60, 10.0, |_| 400.0)).unwrap();
        let hilly = Route::from_world(None, ORIGIN, &line(60, 10.0, |d| 400.0 + d * 0.06)).unwrap();
        assert_eq!(flat.key(), hilly.key());
        let mut moved = line(60, 10.0, |_| 400.0);
        for point in &mut moved {
            point.east += 300.0;
        }
        let elsewhere = Route::from_world(None, ORIGIN, &moved).unwrap();
        assert_ne!(flat.key(), elsewhere.key());
        assert_eq!(hilly.climbs().len(), 1);
    }

    #[test]
    fn a_line_without_length_is_refused() {
        let point = line(0, 10.0, |_| 400.0)[0];
        assert!(matches!(
            Route::from_world(None, ORIGIN, &[point, point]),
            Err(RouteError::TooShort)
        ));
    }
}
