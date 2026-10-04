//! The ride engine: moves the virtual rider along a route from measured power, tells the
//! trainer which gradient to simulate and records the ride.
//!
//! It is pure logic driven by [`Ride::tick`], independent of devices, threads and rendering, so
//! the same engine runs headless in the CLI, in tests and behind the 3D world.

pub mod analysis;
pub mod ghost;

use std::time::Duration;

use torqa_domain::recording::Sample;
use torqa_domain::telemetry::{Telemetry, TrainerControl};
use torqa_domain::units::{GradePercent, Meters, MetersPerSecond, Percent, Watts};
use torqa_physics::{DescentMode, Motion, RiderSetup, trainer_grade};
use torqa_routes::{Route, RoutePosition};

/// Trainers need time to change resistance; more frequent grade updates only add traffic.
const GRADE_UPDATE_INTERVAL: Duration = Duration::from_secs(1);
/// Grade changes smaller than this are not worth a trainer update.
const GRADE_UPDATE_THRESHOLD: f64 = 0.1;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Settings for one ride.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideConfig {
    /// Rider and bike.
    pub setup: RiderSetup,
    /// Trainer difficulty (R14).
    pub difficulty: Percent,
    /// Descent behaviour (R15).
    pub descent: DescentMode,
}

impl Default for RideConfig {
    /// 50 % trainer difficulty, as popular platforms default to, and coasting descents.
    fn default() -> Self {
        Self {
            setup: RiderSetup::default(),
            difficulty: Percent(50.0),
            descent: DescentMode::Coast,
        }
    }
}

/// A snapshot for display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideState {
    /// Time since the start.
    pub elapsed: Duration,
    /// Distance covered.
    pub distance: Meters,
    /// Distance left to the finish.
    pub remaining: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// Where the rider is.
    pub position: RoutePosition,
    /// Latest measurements.
    pub telemetry: Telemetry,
}

/// A ride along a route.
#[derive(Debug, Clone)]
pub struct Ride {
    route: Route,
    config: RideConfig,
    motion: Motion,
    distance: Meters,
    elapsed: Duration,
    telemetry: Telemetry,
    samples: Vec<Sample>,
    next_sample: Duration,
    last_grade: Option<(GradePercent, Duration)>,
}

impl Ride {
    /// Starts a ride at the beginning of `route`.
    #[must_use]
    pub fn new(route: Route, config: RideConfig) -> Self {
        Self {
            route,
            config,
            motion: Motion::default(),
            distance: Meters(0.0),
            elapsed: Duration::ZERO,
            telemetry: Telemetry::default(),
            samples: Vec::new(),
            next_sample: Duration::ZERO,
            last_grade: None,
        }
    }

    /// Feeds new measurements from a device.
    pub fn on_telemetry(&mut self, telemetry: &Telemetry) {
        self.telemetry.merge(telemetry);
    }

    /// Forgets the power source's last values, e.g. when the trainer disconnects, so the rider
    /// does not keep riding on stale power.
    pub fn on_power_source_lost(&mut self) {
        self.telemetry.power = None;
        self.telemetry.cadence = None;
        self.telemetry.speed = None;
    }

    /// Advances the ride by `dt`. Returns a control for the trainer when the simulated gradient
    /// should change.
    pub fn tick(&mut self, dt: Duration) -> Option<TrainerControl> {
        if self.is_finished() {
            return None;
        }
        // Samples sit on a whole-second grid, so FIT timestamps are exact.
        if self.elapsed >= self.next_sample {
            self.record();
            self.next_sample += SAMPLE_INTERVAL;
        }

        let grade = self.road_grade();
        let power = self.telemetry.power.unwrap_or(Watts(0.0));
        let covered = self
            .motion
            .step(&self.config.setup, power, grade, MetersPerSecond(0.0), dt);
        self.distance = Meters((self.distance.0 + covered.0).min(self.route.length().0));
        self.elapsed += dt;

        if self.is_finished() {
            self.record();
            return None;
        }
        self.grade_update()
    }

    /// Changes trainer difficulty and descent mode during the ride (R48); the trainer gets the
    /// new gradient on the next tick rather than at the next regular update.
    pub fn adjust(&mut self, difficulty: Percent, descent: DescentMode) {
        self.config.difficulty = difficulty;
        self.config.descent = descent;
        self.last_grade = None;
    }

    /// Moves the rider to `distance` along the route, keeping their speed — for simulated
    /// rides (#53). The trainer gets the gradient there on the next tick.
    pub fn jump_to(&mut self, distance: Meters) {
        self.distance = Meters(distance.0.clamp(0.0, self.route.length().0));
        self.last_grade = None;
    }

    /// Whether the rider has reached the end of the route.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.distance.0 >= self.route.length().0
    }

    /// Current state for display.
    #[must_use]
    pub fn state(&self) -> RideState {
        RideState {
            elapsed: self.elapsed,
            distance: self.distance,
            remaining: Meters(self.route.length().0 - self.distance.0),
            speed: self.motion.speed(),
            position: self.route.position(self.distance),
            telemetry: self.telemetry,
        }
    }

    /// The route being ridden.
    #[must_use]
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// The recorded samples, one per second.
    #[must_use]
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The gradient used for physics: the road, adjusted for the descent mode.
    fn road_grade(&self) -> GradePercent {
        self.config
            .descent
            .apply(self.route.position(self.distance).grade)
    }

    fn grade_update(&mut self) -> Option<TrainerControl> {
        let target = trainer_grade(self.road_grade(), self.config.difficulty);
        let due = match self.last_grade {
            None => true,
            Some((sent, at)) => {
                self.elapsed.saturating_sub(at) >= GRADE_UPDATE_INTERVAL
                    && (target.0 - sent.0).abs() >= GRADE_UPDATE_THRESHOLD
            }
        };
        if !due {
            return None;
        }
        self.last_grade = Some((target, self.elapsed));
        Some(TrainerControl::Simulation(
            self.config.setup.simulation_parameters(target),
        ))
    }

    fn record(&mut self) {
        let position = self.route.position(self.distance);
        self.samples.push(Sample {
            elapsed: self.elapsed,
            lat: position.lat,
            lon: position.lon,
            elevation: position.elevation,
            distance: self.distance,
            speed: self.motion.speed(),
            grade: position.grade,
            power: self.telemetry.power,
            cadence: self.telemetry.cadence,
            heart_rate: self.telemetry.heart_rate,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use torqa_domain::units::Rpm;

    use super::*;

    /// A straight route due north: one segment per grade, each `length` metres.
    async fn route(grades: &[f64], length: f64) -> Route {
        let degrees_per_meter = 180.0 / (std::f64::consts::PI * 6_371_000.0);
        let mut xml = String::from("<gpx><trk><trkseg>");
        let mut elevation = 500.0;
        let mut add = |i: usize, elevation: f64| {
            #[allow(clippy::cast_precision_loss)]
            let lat = 46.0 + i as f64 * 10.0 * degrees_per_meter;
            let _ = write!(
                xml,
                r#"<trkpt lat="{lat}" lon="7"><ele>{elevation}</ele></trkpt>"#
            );
        };
        let mut i = 0;
        add(i, elevation);
        for grade in grades {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            for _ in 0..(length / 10.0) as usize {
                i += 1;
                elevation += grade / 100.0 * 10.0;
                add(i, elevation);
            }
        }
        xml.push_str("</trkseg></trk></gpx>");
        Route::from_gpx(&xml, None).await.unwrap()
    }

    fn pedal(ride: &mut Ride, power: f64, seconds: u32) -> Vec<TrainerControl> {
        ride.on_telemetry(&Telemetry {
            power: Some(Watts(power)),
            cadence: Some(Rpm(90.0)),
            ..Telemetry::default()
        });
        (0..seconds * 4)
            .filter_map(|_| ride.tick(Duration::from_millis(250)))
            .collect()
    }

    fn grade_of(control: &TrainerControl) -> f64 {
        match control {
            TrainerControl::Simulation(p) => p.grade.0,
            other => panic!("unexpected control {other:?}"),
        }
    }

    #[tokio::test]
    async fn rides_along_the_route_at_physics_speed() {
        let mut ride = Ride::new(route(&[0.0], 5000.0).await, RideConfig::default());

        pedal(&mut ride, 250.0, 120);

        // Accelerating from standstill to ~37 km/h, about 1.2 km in two minutes.
        let distance = ride.state().distance.0;
        assert!((1100.0..1300.0).contains(&distance), "{distance} m");
    }

    #[tokio::test]
    async fn trainer_feels_the_scaled_gradient() {
        let mut ride = Ride::new(route(&[6.0], 2000.0).await, RideConfig::default());

        let controls = pedal(&mut ride, 250.0, 5);

        // 6 % at the default 50 % difficulty.
        assert!((grade_of(&controls[0]) - 3.0).abs() < 0.1, "{controls:?}");
    }

    #[tokio::test]
    async fn grade_updates_are_throttled_and_skip_unchanged_grades() {
        let mut ride = Ride::new(route(&[4.0], 3000.0).await, RideConfig::default());

        let controls = pedal(&mut ride, 250.0, 30);

        assert_eq!(
            controls.len(),
            1,
            "steady climb needs one update: {controls:?}"
        );
    }

    #[tokio::test]
    async fn grade_changes_reach_the_trainer() {
        let config = RideConfig {
            difficulty: Percent(100.0),
            ..RideConfig::default()
        };
        let mut ride = Ride::new(route(&[0.0, 8.0], 300.0).await, config);

        let controls = pedal(&mut ride, 300.0, 120);

        let last = controls.last().map(grade_of).unwrap();
        assert!((last - 8.0).abs() < 0.5, "{controls:?}");
    }

    #[tokio::test]
    async fn a_jump_moves_the_rider_and_the_trainer_feels_the_road_there() {
        let config = RideConfig {
            difficulty: Percent(100.0),
            ..RideConfig::default()
        };
        let mut ride = Ride::new(route(&[0.0, 8.0], 1000.0).await, config);
        pedal(&mut ride, 250.0, 10);
        let speed = ride.state().speed;

        ride.jump_to(Meters(1500.0));
        let controls = pedal(&mut ride, 250.0, 1);

        assert!((ride.state().distance.0 - 1500.0).abs() < 15.0);
        // Not braked by the jump, and the trainer gets the 8 % climb at once.
        assert!(ride.state().speed.0 > speed.0 * 0.5);
        assert!((grade_of(&controls[0]) - 8.0).abs() < 0.5, "{controls:?}");
        // Beyond the end means the end.
        ride.jump_to(Meters(5000.0));
        assert!(ride.is_finished());
    }

    #[tokio::test]
    async fn difficulty_changes_reach_the_trainer_at_once() {
        let config = RideConfig {
            difficulty: Percent(100.0),
            ..RideConfig::default()
        };
        let mut ride = Ride::new(route(&[8.0], 5000.0).await, config);
        pedal(&mut ride, 300.0, 30);

        ride.adjust(Percent(50.0), DescentMode::Coast);
        let controls = pedal(&mut ride, 300.0, 1);

        let first = controls.first().map(grade_of).unwrap();
        assert!((first - 4.0).abs() < 0.3, "{controls:?}");
    }

    #[tokio::test]
    async fn records_one_sample_per_second() {
        let mut ride = Ride::new(route(&[0.0], 5000.0).await, RideConfig::default());

        pedal(&mut ride, 200.0, 10);

        let samples = ride.samples();
        assert_eq!(samples.len(), 10);
        assert_eq!(samples[3].elapsed, Duration::from_secs(3));
        assert_eq!(samples[3].power, Some(Watts(200.0)));
    }

    #[tokio::test]
    async fn stops_at_the_finish() {
        let mut ride = Ride::new(route(&[0.0], 200.0).await, RideConfig::default());

        pedal(&mut ride, 300.0, 120);

        assert!(ride.is_finished());
        assert!((ride.state().distance.0 - ride.route().length().0).abs() < 1e-9);
        let recorded = ride.samples().len();
        pedal(&mut ride, 300.0, 5);
        assert_eq!(
            ride.samples().len(),
            recorded,
            "no recording after the finish"
        );
    }

    #[tokio::test]
    async fn lost_power_source_stops_propulsion() {
        let mut ride = Ride::new(route(&[0.0], 5000.0).await, RideConfig::default());
        pedal(&mut ride, 250.0, 60);

        ride.on_power_source_lost();
        for _ in 0..1200 {
            ride.tick(Duration::from_millis(250));
        }

        assert!(ride.state().speed.0 < 1.0, "{:?}", ride.state().speed);
    }

    #[tokio::test]
    async fn flat_descent_mode_gives_no_free_speed() {
        let descent = route(&[-6.0], 3000.0).await;
        let mut coasting = Ride::new(descent.clone(), RideConfig::default());
        let flat_config = RideConfig {
            descent: DescentMode::Flat,
            ..RideConfig::default()
        };
        let mut flat = Ride::new(descent, flat_config);

        let flat_controls = pedal(&mut flat, 100.0, 60);
        pedal(&mut coasting, 100.0, 60);

        assert!(flat.state().distance.0 < coasting.state().distance.0 * 0.8);
        assert!(flat_controls.iter().all(|c| grade_of(c) >= 0.0));
    }
}
