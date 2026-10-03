//! Ghosts and pacers (R20): a second rider on the route to race against, given as the time at
//! which it passed each distance — from an earlier ride, a constant-power simulation or a
//! recorded activity matched onto the route.

use std::time::Duration;

use torqa_domain::recording::Sample;
use torqa_domain::units::{Meters, MetersPerSecond, Watts};
use torqa_physics::{DescentMode, Motion, RiderSetup};
use torqa_routes::{LocalProjection, Route, TimedPoint};

/// Simulation step for pacers.
const STEP: Duration = Duration::from_millis(500);
/// A pacer that has not finished after this long is stopped (e.g. too little power for a wall).
const MAX_PACER_TIME: Duration = Duration::from_hours(12);
/// Activity points further than this from the route are not on it (detours, GPS jumps).
const MATCH_TOLERANCE_M: f64 = 40.0;
/// How far ahead along the route the next activity point is looked for.
const SEARCH_AHEAD_M: f64 = 3_000.0;
/// An activity must pass this close to the route start to be raced from the start.
const START_TOLERANCE_M: f64 = 100.0;

/// A ghost rider: when it passed each distance along the route.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghost {
    /// What it is, for display: e.g. "Your best" or "Pacer 250 W".
    pub name: String,
    /// `(distance m, time s)`, both increasing.
    trace: Vec<(f64, f64)>,
}

impl Ghost {
    /// Replays a recorded ride on the same route.
    #[must_use]
    pub fn from_samples(name: &str, samples: &[Sample]) -> Option<Self> {
        Self::from_trace(
            name,
            samples
                .iter()
                .map(|s| (s.distance.0, s.elapsed.as_secs_f64())),
        )
    }

    /// A pacer riding `route` at constant `power` with the rider's `setup` and descent mode, so
    /// it climbs and descends like the rider would at that power.
    #[must_use]
    pub fn pacer(
        name: &str,
        route: &Route,
        setup: &RiderSetup,
        descent: DescentMode,
        power: Watts,
    ) -> Self {
        let mut motion = Motion::default();
        let mut distance = 0.0;
        let mut elapsed = Duration::ZERO;
        let mut trace = vec![(0.0, 0.0)];
        while distance < route.length().0 && elapsed < MAX_PACER_TIME {
            let grade = descent.apply(route.position(Meters(distance)).grade);
            let covered = motion.step(setup, power, grade, MetersPerSecond(0.0), STEP);
            distance = (distance + covered.0).min(route.length().0);
            elapsed += STEP;
            if covered.0 > 0.0 {
                trace.push((distance, elapsed.as_secs_f64()));
            }
        }
        Self {
            name: name.to_owned(),
            trace,
        }
    }

    /// Follows a recorded activity (GPX or FIT) where it rode along `route`. The race starts
    /// where the activity passed the route start; detours and other roads are left out.
    #[must_use]
    pub fn from_activity(name: &str, route: &Route, points: &[TimedPoint]) -> Option<Self> {
        let projection = LocalProjection::for_route(route);
        let along: Vec<(f64, f64, f64)> = route
            .points()
            .iter()
            .map(|p| {
                let (x, y) = projection.project(p.lat, p.lon);
                (x, y, p.distance.0)
            })
            .collect();
        let mut matched: Vec<(f64, f64)> = Vec::new();
        let mut from = 0;
        for point in points {
            let (x, y) = projection.project(point.lat, point.lon);
            let window = along[from..]
                .iter()
                .enumerate()
                .take_while(|(_, (_, _, d))| *d <= along[from].2 + SEARCH_AHEAD_M);
            let Some((offset, gap, distance)) = window
                .map(|(i, (px, py, d))| (i, (px - x).hypot(py - y), *d))
                .min_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            if gap > MATCH_TOLERANCE_M || (matched.is_empty() && distance > START_TOLERANCE_M) {
                continue;
            }
            from += offset;
            matched.push((distance, point.time));
        }
        let start = matched.first()?.1;
        Self::from_trace(name, matched.into_iter().map(|(d, t)| (d, t - start)))
    }

    fn from_trace(name: &str, points: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
        let mut trace: Vec<(f64, f64)> = Vec::new();
        for (distance, time) in points {
            match trace.last() {
                // Standing still or going back (GPS noise) adds nothing to race against.
                Some(&(d, t)) if distance <= d || time <= t => {}
                _ => trace.push((distance, time)),
            }
        }
        (trace.len() >= 2).then(|| Self {
            name: name.to_owned(),
            trace,
        })
    }

    /// Where the ghost is after `elapsed`; at its last point once it has finished.
    #[must_use]
    pub fn distance_at(&self, elapsed: Duration) -> Meters {
        let t = elapsed.as_secs_f64();
        let after = self.trace.partition_point(|&(_, time)| time <= t);
        if after == 0 {
            return Meters(self.trace[0].0);
        }
        if after == self.trace.len() {
            return Meters(self.trace[after - 1].0);
        }
        let ((d0, t0), (d1, t1)) = (self.trace[after - 1], self.trace[after]);
        Meters(d0 + (d1 - d0) * (t - t0) / (t1 - t0))
    }

    /// When the ghost passed `distance`; `None` beyond its last point.
    #[must_use]
    pub fn time_at(&self, distance: Meters) -> Option<Duration> {
        let d = distance.0;
        let after = self.trace.partition_point(|&(dist, _)| dist < d);
        if after == self.trace.len() {
            return None;
        }
        if after == 0 {
            return Some(Duration::from_secs_f64(self.trace[0].1));
        }
        let ((d0, t0), (d1, t1)) = (self.trace[after - 1], self.trace[after]);
        Some(Duration::from_secs_f64(
            t0 + (t1 - t0) * (d - d0) / (d1 - d0),
        ))
    }

    /// Seconds the rider is behind the ghost (negative: ahead), comparing when each passed the
    /// rider's position; `None` where the ghost has no time (beyond its last point).
    #[must_use]
    pub fn gap(&self, rider_distance: Meters, rider_elapsed: Duration) -> Option<f64> {
        self.time_at(rider_distance)
            .map(|ghost| rider_elapsed.as_secs_f64() - ghost.as_secs_f64())
    }

    /// The ghost's time over its whole trace.
    #[must_use]
    pub fn total_time(&self) -> Duration {
        Duration::from_secs_f64(self.trace[self.trace.len() - 1].1)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use torqa_domain::units::{GradePercent, Kilograms};

    use super::*;

    /// 2 km due north, flat.
    async fn flat_route() -> Route {
        let mut xml = String::from("<gpx><trk><trkseg>");
        for i in 0..=20 {
            let lat = 46.0 + f64::from(i) * 100.0 / 111_195.0;
            let _ = write!(xml, r#"<trkpt lat="{lat}" lon="7"><ele>500</ele></trkpt>"#);
        }
        xml.push_str("</trkseg></trk></gpx>");
        Route::from_gpx(&xml, None).await.unwrap()
    }

    fn samples(speed: f64, seconds: u32) -> Vec<Sample> {
        (0..=seconds)
            .map(|s| Sample {
                elapsed: Duration::from_secs(u64::from(s)),
                lat: 46.0,
                lon: 7.0,
                elevation: Meters(500.0),
                distance: Meters(speed * f64::from(s)),
                speed: MetersPerSecond(speed),
                grade: GradePercent(0.0),
                power: None,
                cadence: None,
                heart_rate: None,
            })
            .collect()
    }

    #[test]
    fn a_replayed_ride_is_where_it_was_at_each_moment() {
        let ghost = Ghost::from_samples("Your best", &samples(10.0, 100)).unwrap();

        assert_eq!(
            ghost.distance_at(Duration::from_millis(12_500)),
            Meters(125.0)
        );
        assert_eq!(ghost.time_at(Meters(250.0)), Some(Duration::from_secs(25)));
        // At 300 m after 35 s, the rider is 5 s behind; after 25 s, 5 s ahead.
        assert!((ghost.gap(Meters(300.0), Duration::from_secs(35)).unwrap() - 5.0).abs() < 1e-9);
        assert!((ghost.gap(Meters(300.0), Duration::from_secs(25)).unwrap() + 5.0).abs() < 1e-9);
        // Finished: it waits at the end, and has no time beyond it.
        assert_eq!(ghost.distance_at(Duration::from_secs(500)), Meters(1000.0));
        assert_eq!(ghost.time_at(Meters(1500.0)), None);
    }

    #[test]
    fn stops_and_gps_noise_do_not_break_the_trace() {
        let mut ride = samples(10.0, 10);
        ride[5].distance = Meters(30.0); // jumps back
        ride[6].distance = Meters(40.0); // stands still at the earlier distance

        let ghost = Ghost::from_samples("x", &ride).unwrap();

        assert_eq!(ghost.time_at(Meters(70.0)), Some(Duration::from_secs(7)));
        assert!(Ghost::from_samples("x", &samples(0.0, 10)).is_none());
    }

    #[tokio::test]
    async fn pacers_ride_at_the_speed_their_power_gives() {
        let route = flat_route().await;
        let setup = RiderSetup {
            mass: Kilograms(83.0),
            ..RiderSetup::default()
        };

        let strong = Ghost::pacer("Pacer", &route, &setup, DescentMode::Coast, Watts(300.0));
        let easy = Ghost::pacer("Pacer", &route, &setup, DescentMode::Coast, Watts(150.0));

        // Flat 2 km: roughly 38–42 km/h at 300 W, 30–33 km/h at 150 W.
        let kmh = |g: &Ghost| route.length().0 / g.total_time().as_secs_f64() * 3.6;
        assert!((36.0..44.0).contains(&kmh(&strong)), "{}", kmh(&strong));
        assert!((28.0..35.0).contains(&kmh(&easy)), "{}", kmh(&easy));
        assert_eq!(strong.distance_at(Duration::from_hours(1)), route.length());
    }

    #[tokio::test]
    async fn activities_are_matched_onto_the_route_from_its_start() {
        let route = flat_route().await;
        let point = |metres: f64, east: f64, time: f64| TimedPoint {
            lat: 46.0 + metres / 111_195.0,
            lon: 7.0 + east / (111_195.0 * 46f64.to_radians().cos()),
            time,
        };
        let points = vec![
            point(-500.0, 0.0, 1000.0), // warming up before the start
            point(0.0, 5.0, 1100.0),    // race starts here
            point(500.0, 3.0, 1150.0),
            point(1000.0, 400.0, 1200.0), // a detour
            point(1500.0, 0.0, 1250.0),
            point(2000.0, 0.0, 1300.0),
        ];

        let ghost = Ghost::from_activity("Club ride", &route, &points).unwrap();

        assert_eq!(ghost.total_time(), Duration::from_secs(200));
        let at_1500 = ghost.time_at(Meters(1500.0)).unwrap().as_secs_f64();
        assert!((at_1500 - 150.0).abs() < 2.0, "{at_1500}");
        // Elsewhere entirely: nothing to race.
        let far: Vec<TimedPoint> = points.iter().map(|p| point(0.0, 5_000.0, p.time)).collect();
        assert!(Ghost::from_activity("x", &route, &far).is_none());
    }
}
