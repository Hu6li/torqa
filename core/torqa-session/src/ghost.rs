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
/// Seconds of an activity over which its positions along the route are averaged (see
/// [`smoothed`]).
const SMOOTHING_S: f64 = 5.0;

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
            let reach = along[from].2 + SEARCH_AHEAD_M;
            // The nearest place on the route's line, not its nearest point: those lie metres
            // apart, and an activity snapped from one to the next moves in jerks.
            let Some((offset, gap, distance)) = along[from..]
                .windows(2)
                .take_while(|pair| pair[0].2 <= reach)
                .enumerate()
                .map(|(i, pair)| {
                    let (gap, distance) = onto_segment(pair[0], pair[1], (x, y));
                    (i, gap, distance)
                })
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
        Self::from_trace(
            name,
            smoothed(&matched).into_iter().map(|(d, t)| (d, t - start)),
        )
    }

    /// From `(distance m, time s)` points; standing still and going back are skipped. `None`
    /// with fewer than two usable points.
    pub fn from_trace(name: &str, points: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
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

/// How far `point` lies from the route between `a` and `b` (`(x, y, distance along the route)`),
/// and the route distance of the place there nearest to it.
fn onto_segment(a: (f64, f64, f64), b: (f64, f64, f64), (x, y): (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let along = if length_squared > 0.0 {
        (((x - a.0) * dx + (y - a.1) * dy) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (
        (a.0 + dx * along - x).hypot(a.1 + dy * along - y),
        a.2 + (b.2 - a.2) * along,
    )
}

/// `(distance, time)` points of an activity with each distance averaged over the points within
/// [`SMOOTHING_S`] around it. GPS positions wander metres along the road from second to second;
/// followed as they are, a ghost — or a video timed by them (R17) — speeds up and slows down
/// where the activity went steadily. As many points are taken before as after, so the ends and
/// the edges of detours left out stay where they are.
fn smoothed(matched: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let half = SMOOTHING_S / 2.0;
    (0..matched.len())
        .map(|i| {
            let time = matched[i].1;
            let before = matched[..i]
                .iter()
                .rev()
                .take_while(|p| time - p.1 <= half)
                .count();
            let after = matched[i + 1..]
                .iter()
                .take_while(|p| p.1 - time <= half)
                .count();
            let n = before.min(after);
            let window = &matched[i - n..=i + n];
            #[allow(clippy::cast_precision_loss)] // a few dozen points
            let mean = window.iter().map(|p| p.0).sum::<f64>() / window.len() as f64;
            (mean, time)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use torqa_domain::units::Kilograms;

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
                location: None,
                distance: Meters(speed * f64::from(s)),
                speed: MetersPerSecond(speed),
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

    /// A recorded position `metres` north of the flat route's start and `east` of it.
    fn point(metres: f64, east: f64, time: f64) -> TimedPoint {
        TimedPoint {
            lat: 46.0 + metres / 111_195.0,
            lon: 7.0 + east / (111_195.0 * 46f64.to_radians().cos()),
            time,
        }
    }

    #[tokio::test]
    async fn activities_are_matched_onto_the_route_from_its_start() {
        let route = flat_route().await;
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

    #[tokio::test]
    async fn an_activity_ridden_steadily_stays_steady_on_the_route() {
        // A camera's or bike computer's GPS once a second, at a pace out of step with the
        // route's points, wandering metres along and across the road.
        let route = flat_route().await;
        let speed = 7.3;
        let mut state = 0x2545_f491_u32;
        let mut wander = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            f64::from(state % 601) / 100.0 - 3.0
        };
        let points: Vec<TimedPoint> = (0..=270)
            .map(|s| {
                let time = f64::from(s);
                point(speed * time + wander(), wander(), time)
            })
            .collect();

        let ghost = Ghost::from_activity("Steady", &route, &points).unwrap();

        // Each 20 m took it as long as the next: a video timed by it plays evenly (R17).
        let expected = 20.0 / speed;
        for start in (40..1_900).step_by(20) {
            let at = |m: u32| ghost.time_at(Meters(f64::from(m))).unwrap().as_secs_f64();
            let took = at(start + 20) - at(start);
            assert!(
                (took - expected).abs() < 0.15 * expected,
                "{took:.2} s instead of {expected:.2} s from {start} m"
            );
        }
    }
}
