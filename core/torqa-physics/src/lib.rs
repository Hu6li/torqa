//! Cycling physics for Torqa (R13–R15): how fast the virtual rider moves for a given power,
//! and which gradient the trainer should simulate.
//!
//! Forces on the rider: `P·η / v = m·g·sin θ + m·g·Crr·cos θ + ½·ρ·CdA·v_air² + m·a`.

use std::time::Duration;

use torqa_domain::telemetry::SimulationParameters;
use torqa_domain::units::{
    GradePercent, Kilograms, KilogramsPerCubicMeter, KilogramsPerMeter, Meters, MetersPerSecond,
    Percent, SquareMeters, Watts,
};

const GRAVITY: f64 = 9.81;
/// Longest integration step; larger time steps are split for stability.
const MAX_STEP: Duration = Duration::from_millis(50);
/// Below this speed the drive force is computed as if moving at it, so `P / v` stays finite
/// when starting from standstill.
const MIN_DRIVE_SPEED: f64 = 1.0;

/// Rider, bike and environment parameters (R13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiderSetup {
    /// Rider plus bike.
    pub mass: Kilograms,
    /// Drag area.
    pub cda: SquareMeters,
    /// Coefficient of rolling resistance.
    pub crr: f64,
    /// Share of pedal power reaching the wheel (0–1).
    pub drivetrain_efficiency: f64,
    /// Air density.
    pub air_density: KilogramsPerCubicMeter,
}

impl Default for RiderSetup {
    /// A 75 kg rider on an 8 kg road bike on the hoods, good tarmac, sea level at 15 °C.
    fn default() -> Self {
        Self {
            mass: Kilograms(83.0),
            cda: SquareMeters(0.32),
            crr: 0.004,
            drivetrain_efficiency: 0.975,
            air_density: KilogramsPerCubicMeter(1.225),
        }
    }
}

impl RiderSetup {
    /// Parameters for the trainer's slope simulation at a gradient.
    #[must_use]
    pub fn simulation_parameters(&self, grade: GradePercent) -> SimulationParameters {
        SimulationParameters {
            grade,
            wind_speed: MetersPerSecond(0.0),
            crr: self.crr,
            cw: KilogramsPerMeter(0.5 * self.air_density.0 * self.cda.0),
        }
    }
}

/// How descents are handled (R15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DescentMode {
    /// Gravity accelerates the rider and the trainer goes light.
    #[default]
    Coast,
    /// Descents are treated as flat: no free speed, the rider keeps pedalling.
    Flat,
}

impl DescentMode {
    /// The gradient to use for both physics and trainer under this mode.
    #[must_use]
    pub fn apply(self, grade: GradePercent) -> GradePercent {
        match self {
            Self::Coast => grade,
            Self::Flat => GradePercent(grade.0.max(0.0)),
        }
    }
}

/// The gradient the trainer should simulate: the road gradient scaled by the trainer
/// difficulty (R14). Virtual speed always uses the unscaled road gradient.
#[must_use]
pub fn trainer_grade(road: GradePercent, difficulty: Percent) -> GradePercent {
    GradePercent(road.0 * difficulty.0.clamp(0.0, 100.0) / 100.0)
}

/// The virtual rider's motion along the road.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    speed: MetersPerSecond,
}

impl Motion {
    /// Current speed.
    #[must_use]
    pub fn speed(&self) -> MetersPerSecond {
        self.speed
    }

    /// Advances by `dt` with constant `power`, `grade` and head `wind`; returns the distance
    /// covered.
    pub fn step(
        &mut self,
        setup: &RiderSetup,
        power: Watts,
        grade: GradePercent,
        wind: MetersPerSecond,
        dt: Duration,
    ) -> Meters {
        let theta = (grade.0 / 100.0).atan();
        let mass = setup.mass.0;
        let gravity = mass * GRAVITY * theta.sin();
        let rolling = mass * GRAVITY * setup.crr * theta.cos();
        let drag_factor = 0.5 * setup.air_density.0 * setup.cda.0;
        let wheel_power = power.0.max(0.0) * setup.drivetrain_efficiency;

        let mut remaining = dt;
        let mut distance = 0.0;
        let mut v = self.speed.0;
        while !remaining.is_zero() {
            let step = remaining.min(MAX_STEP);
            remaining -= step;
            let h = step.as_secs_f64();

            let drive = wheel_power / v.max(MIN_DRIVE_SPEED);
            let air_speed = v + wind.0;
            let drag = drag_factor * air_speed * air_speed.abs();
            // Rolling resistance only opposes motion; it cannot push a stopped rider backwards.
            let rolling = if v > 0.0 || drive > rolling + gravity {
                rolling
            } else {
                0.0
            };
            let acceleration = (drive - gravity - rolling - drag) / mass;

            let next = (v + acceleration * h).max(0.0);
            distance += f64::midpoint(v, next) * h;
            v = next;
        }
        self.speed = MetersPerSecond(v);
        Meters(distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rides for two minutes at constant conditions and returns the settled speed in km/h.
    fn settled_kmh(power: f64, grade: f64) -> f64 {
        let mut motion = Motion::default();
        for _ in 0..1200 {
            motion.step(
                &RiderSetup::default(),
                Watts(power),
                GradePercent(grade),
                MetersPerSecond(0.0),
                Duration::from_millis(100),
            );
        }
        motion.speed().as_kilometers_per_hour()
    }

    #[test]
    fn flat_road_speed_matches_power_calculators() {
        // 250 W, 83 kg, CdA 0.32: about 37 km/h.
        let speed = settled_kmh(250.0, 0.0);
        assert!((36.5..37.5).contains(&speed), "{speed} km/h");
    }

    #[test]
    fn climbing_is_slow() {
        // 250 W up 8 %: about 12.6 km/h.
        let speed = settled_kmh(250.0, 8.0);
        assert!((12.0..13.2).contains(&speed), "{speed} km/h");
    }

    #[test]
    fn coasting_downhill_builds_speed() {
        // No pedalling at −6 %: terminal speed about 55 km/h.
        let speed = settled_kmh(0.0, -6.0);
        assert!((53.0..57.0).contains(&speed), "{speed} km/h");
    }

    #[test]
    fn stopped_rider_without_power_stays_put() {
        let mut motion = Motion::default();
        let distance = motion.step(
            &RiderSetup::default(),
            Watts(0.0),
            GradePercent(0.0),
            MetersPerSecond(0.0),
            Duration::from_secs(5),
        );

        assert_eq!(distance, Meters(0.0));
        assert_eq!(motion.speed(), MetersPerSecond(0.0));
    }

    #[test]
    fn head_wind_slows_the_rider() {
        let still = settled_kmh(250.0, 0.0);
        let mut motion = Motion::default();
        for _ in 0..1200 {
            motion.step(
                &RiderSetup::default(),
                Watts(250.0),
                GradePercent(0.0),
                MetersPerSecond(5.0),
                Duration::from_millis(100),
            );
        }
        assert!(motion.speed().as_kilometers_per_hour() < still - 5.0);
    }

    #[test]
    fn flat_descent_mode_removes_negative_grades() {
        assert_eq!(
            DescentMode::Flat.apply(GradePercent(-7.0)),
            GradePercent(0.0)
        );
        assert_eq!(
            DescentMode::Flat.apply(GradePercent(4.0)),
            GradePercent(4.0)
        );
        assert_eq!(
            DescentMode::Coast.apply(GradePercent(-7.0)),
            GradePercent(-7.0)
        );
    }

    #[test]
    fn difficulty_scales_trainer_grade() {
        assert_eq!(
            trainer_grade(GradePercent(10.0), Percent(50.0)),
            GradePercent(5.0)
        );
        assert_eq!(
            trainer_grade(GradePercent(-4.0), Percent(100.0)),
            GradePercent(-4.0)
        );
        assert_eq!(
            trainer_grade(GradePercent(10.0), Percent(150.0)),
            GradePercent(10.0)
        );
    }

    #[test]
    fn simulation_parameters_use_the_rider_drag() {
        let parameters = RiderSetup::default().simulation_parameters(GradePercent(3.0));

        assert!((parameters.cw.0 - 0.196).abs() < 1e-9);
        assert_eq!(parameters.grade, GradePercent(3.0));
    }
}
