//! Estimated time to the finish (R66): how long a rider holding a steady power takes over the
//! rest of a route, ridden with the same physics as the ride itself, so that climbs and
//! descents cost and give the time they will.

use std::time::Duration;

use torqa_domain::units::{Meters, MetersPerSecond, Watts};
use torqa_physics::{DescentMode, Motion, RiderSetup};
use torqa_routes::Route;

/// Share of FTP a ride is planned at: a steady endurance pace, the top of zone 2, that a rider
/// holds for hours rather than the hour FTP itself lasts.
pub const FTP_SHARE: f64 = 0.75;
/// Over a ride's first minutes the estimate moves from the planned power to the ride's own
/// average: before, a warm-up or the first climb would sway it too much.
const SETTLING: Duration = Duration::from_mins(10);
/// Simulation step.
const STEP: Duration = Duration::from_millis(500);
/// A rider still riding after this long is not going to finish (e.g. too little power for a
/// wall).
const MAX_TIME: Duration = Duration::from_hours(12);
/// How often a ride under way renews its estimate: simulating the rest of a long route takes a
/// few milliseconds, too long for every frame or even every second.
const RENEW_EVERY: Duration = Duration::from_secs(10);

/// The power a ride is planned at, before the rider has ridden any of it.
#[must_use]
pub fn planned_power(ftp: Watts) -> Watts {
    Watts(ftp.0 * FTP_SHARE)
}

/// The power the rider is expected to hold for the rest of a ride ridden for `elapsed` at
/// `average` power so far: the planned power at the start, moving over to the average during
/// the first ten minutes, so the estimate comes to follow the rider without jumping.
#[must_use]
pub fn expected_power(ftp: Watts, average: Option<Watts>, elapsed: Duration) -> Watts {
    let planned = planned_power(ftp).0;
    let Some(average) = average else {
        return Watts(planned);
    };
    let weight = (elapsed.as_secs_f64() / SETTLING.as_secs_f64()).min(1.0);
    Watts(planned + (average.0 - planned) * weight)
}

/// How long a rider with `setup` holding `power` takes from `from` along `route`, rolling at
/// `speed` there, to its finish; `None` if that power would not get them there (none on the
/// flat, or too little for a wall).
#[must_use]
pub fn time_to_finish(
    route: &Route,
    setup: &RiderSetup,
    descent: DescentMode,
    from: Meters,
    speed: MetersPerSecond,
    power: Watts,
) -> Option<Duration> {
    let finish = route.length().0;
    if from.0 >= finish {
        return Some(Duration::ZERO);
    }
    steady_ride(route, setup, descent, power, from, speed)
        .find(|&(distance, _)| distance.0 >= finish)
        .map(|(_, elapsed)| elapsed)
}

/// The time to the finish of a ride under way, renewed every ten seconds of riding and counting
/// down in between.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TimeToGo {
    /// When it was last estimated, `elapsed` into the ride, and what.
    estimated: Option<(Duration, Option<Duration>)>,
}

impl TimeToGo {
    /// Renews the estimate with `estimate` if it is due, `elapsed` into the ride.
    pub fn update(&mut self, elapsed: Duration, estimate: impl FnOnce() -> Option<Duration>) {
        let due = self
            .estimated
            .is_none_or(|(at, _)| elapsed.saturating_sub(at) >= RENEW_EVERY);
        if due {
            self.estimated = Some((elapsed, estimate()));
        }
    }

    /// The time to go, `elapsed` into the ride; `None` before the first estimate, or where
    /// the rider would not get to the finish.
    #[must_use]
    pub fn at(&self, elapsed: Duration) -> Option<Duration> {
        let (at, left) = self.estimated?;
        Some(left?.saturating_sub(elapsed.saturating_sub(at)))
    }
}

/// Where a rider holding `power` is after each simulation step, starting at `from` with
/// `speed`: `(distance along the route, time since)`, until the finish or [`MAX_TIME`].
pub(crate) fn steady_ride<'a>(
    route: &'a Route,
    setup: &'a RiderSetup,
    descent: DescentMode,
    power: Watts,
    from: Meters,
    speed: MetersPerSecond,
) -> impl Iterator<Item = (Meters, Duration)> + 'a {
    let finish = route.length().0;
    let mut motion = Motion::moving_at(speed);
    let mut distance = from.0.min(finish);
    let mut elapsed = Duration::ZERO;
    std::iter::from_fn(move || {
        if distance >= finish || elapsed >= MAX_TIME {
            return None;
        }
        let grade = descent.apply(route.grade_at(Meters(distance)));
        let covered = motion.step(setup, power, grade, MetersPerSecond(0.0), STEP);
        distance = (distance + covered.0).min(finish);
        elapsed += STEP;
        Some((Meters(distance), elapsed))
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use torqa_domain::telemetry::Telemetry;

    use super::*;
    use crate::{Ride, RideConfig};

    /// `km` due north at a constant `grade` in percent.
    async fn route(km: u32, grade: f64) -> Route {
        let mut xml = String::from("<gpx><trk><trkseg>");
        for i in 0..=km * 10 {
            let lat = 46.0 + f64::from(i) * 100.0 / 111_195.0;
            let ele = 500.0 + f64::from(i) * grade;
            let _ = write!(
                xml,
                r#"<trkpt lat="{lat}" lon="7"><ele>{ele}</ele></trkpt>"#
            );
        }
        xml.push_str("</trkseg></trk></gpx>");
        Route::from_gpx(&xml, None).await.unwrap()
    }

    fn estimate(route: &Route, from: f64, speed: f64, power: f64) -> Option<Duration> {
        time_to_finish(
            route,
            &RiderSetup::default(),
            DescentMode::Coast,
            Meters(from),
            MetersPerSecond(speed),
            Watts(power),
        )
    }

    #[tokio::test]
    async fn a_rider_holding_the_power_takes_the_estimated_time() {
        let route = route(3, 4.0).await;
        let estimated = estimate(&route, 0.0, 0.0, 220.0).unwrap();

        let mut ride = Ride::new(route, RideConfig::default());
        ride.on_telemetry(&Telemetry {
            power: Some(Watts(220.0)),
            ..Telemetry::default()
        });
        while !ride.is_finished() {
            ride.tick(Duration::from_millis(100));
        }

        let ridden = ride.state().elapsed.as_secs_f64();
        assert!(
            (ridden - estimated.as_secs_f64()).abs() < 1.0,
            "ridden {ridden} s, estimated {estimated:?}"
        );
    }

    #[tokio::test]
    async fn climbs_and_less_power_take_longer() {
        let flat = route(2, 0.0).await;
        let climb = route(2, 6.0).await;

        let on_the_flat = estimate(&flat, 0.0, 0.0, 200.0).unwrap();
        let up_the_climb = estimate(&climb, 0.0, 0.0, 200.0).unwrap();
        let easier = estimate(&climb, 0.0, 0.0, 150.0).unwrap();

        assert!(up_the_climb > on_the_flat * 2, "{up_the_climb:?}");
        assert!(easier > up_the_climb);
    }

    #[tokio::test]
    async fn part_way_only_the_rest_of_the_route_counts() {
        let route = route(4, 0.0).await;
        let whole = estimate(&route, 0.0, 0.0, 250.0).unwrap();

        // Halfway, rolling at the speed 250 W holds on the flat (about 37 km/h).
        let rest = estimate(&route, 2_000.0, 10.3, 250.0).unwrap();

        let half = whole.as_secs_f64() / 2.0;
        assert!(
            (rest.as_secs_f64() - half).abs() < 5.0,
            "{rest:?} of {whole:?}"
        );
        assert_eq!(estimate(&route, 4_000.0, 10.3, 250.0), Some(Duration::ZERO));
    }

    #[tokio::test]
    async fn without_power_the_finish_is_out_of_reach() {
        let route = route(2, 0.0).await;

        assert_eq!(estimate(&route, 0.0, 0.0, 0.0), None);
    }

    #[test]
    fn a_ride_under_way_counts_down_between_estimates() {
        let mut time_to_go = TimeToGo::default();
        assert_eq!(time_to_go.at(Duration::ZERO), None);

        time_to_go.update(Duration::from_secs(60), || Some(Duration::from_mins(15)));
        time_to_go.update(Duration::from_secs(65), || unreachable!("not due yet"));
        assert_eq!(
            time_to_go.at(Duration::from_secs(66)),
            Some(Duration::from_secs(894))
        );
        // Ten seconds on the rider turns out slower than estimated.
        time_to_go.update(Duration::from_secs(70), || Some(Duration::from_secs(895)));
        assert_eq!(
            time_to_go.at(Duration::from_secs(70)),
            Some(Duration::from_secs(895))
        );
        // Out of reach, e.g. the rider stopped pedalling on a climb.
        time_to_go.update(Duration::from_secs(80), || None);
        assert_eq!(time_to_go.at(Duration::from_secs(81)), None);
    }

    #[test]
    fn the_expected_power_moves_from_the_plan_to_the_ride_in_its_first_minutes() {
        let ftp = Watts(240.0);
        let average = Some(Watts(200.0));

        assert_eq!(planned_power(ftp), Watts(180.0));
        assert_eq!(
            expected_power(ftp, None, Duration::from_mins(30)),
            Watts(180.0)
        );
        assert_eq!(expected_power(ftp, average, Duration::ZERO), Watts(180.0));
        assert_eq!(
            expected_power(ftp, average, Duration::from_mins(5)),
            Watts(190.0)
        );
        assert_eq!(
            expected_power(ftp, average, Duration::from_mins(10)),
            Watts(200.0)
        );
        assert_eq!(
            expected_power(ftp, average, Duration::from_hours(2)),
            Watts(200.0)
        );
    }
}
