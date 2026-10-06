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
/// when starting from standstill. Kept low so any pedalling still climbs very steep ramps
/// (or terrain-model artefacts) slowly instead of stalling.
const MIN_DRIVE_SPEED: f64 = 0.1;

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

/// Number of virtual gears (R9, ADR 0003).
pub const GEARS: usize = 24;
/// The easiest and hardest virtual gear, as chainring over cog: a mountain-bike low to a road
/// sprint gear. The gears between step by the same factor (about 9 %).
const LOWEST_RATIO: f64 = 0.75;
const HIGHEST_RATIO: f64 = 5.5;
/// The largest rolling resistance and wind coefficient FTMS slope simulation can carry.
const MAX_CRR: f64 = 0.0255;
const MAX_CW: f64 = 2.55;

/// Virtual gears on a single cog (R9, ADR 0003). In slope simulation the trainer brakes its
/// flywheel, turning at the speed of the real gear, as the road would at that speed. In a
/// virtual gear `r` times as long, the same cadence means `r` times the speed: the road's force
/// at that speed, times `r` for the leverage. Scaling the grade and rolling resistance by `r`
/// and the wind coefficient by `r³` makes the trainer brake exactly so.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualGears {
    real_ratio: f64,
}

impl VirtualGears {
    /// Gears for a bike with a `chainring` on a trainer with a `cog`.
    #[must_use]
    pub fn new(chainring: u8, cog: u8) -> Self {
        Self {
            real_ratio: f64::from(chainring.max(1)) / f64::from(cog.max(1)),
        }
    }

    /// The gear (index from 0) closest to the real one: where a ride starts, as it feels on the
    /// bike.
    #[must_use]
    pub fn neutral(&self) -> usize {
        (0..GEARS)
            .min_by(|&a, &b| {
                let off = |gear: usize| (self.ratio(gear) / self.real_ratio).ln().abs();
                off(a).total_cmp(&off(b))
            })
            .unwrap_or(0)
    }

    /// The ratio of `gear` (index from 0, clamped), as chainring over cog.
    #[must_use]
    pub fn ratio(&self, gear: usize) -> f64 {
        let steps = u32::try_from(GEARS - 1).map_or(1.0, f64::from);
        let index = u32::try_from(gear.min(GEARS - 1)).map_or(0.0, f64::from);
        LOWEST_RATIO * (HIGHEST_RATIO / LOWEST_RATIO).powf(index / steps)
    }

    /// The road as the trainer must simulate it in `gear`, for the rider to feel that gear.
    #[must_use]
    pub fn in_gear(&self, road: SimulationParameters, gear: usize) -> SimulationParameters {
        let r = self.ratio(gear) / self.real_ratio;
        SimulationParameters {
            grade: GradePercent(road.grade.0 * r),
            wind_speed: road.wind_speed,
            crr: (road.crr * r).min(MAX_CRR),
            cw: KilogramsPerMeter((road.cw.0 * r.powi(3)).min(MAX_CW)),
        }
    }
}

/// Riders lean no further than this, however tight the bend (radians, 45°).
const MAX_LEAN: f64 = std::f64::consts::FRAC_PI_4;

/// How far a rider leans into a bend (R46): in a steady turn gravity and the centripetal force
/// balance when `tan φ = v²·κ / g`. `curvature` is 1 / radius, positive in bends to the right,
/// as is the result (radians, at most 45° either way).
#[must_use]
pub fn lean_angle(speed: MetersPerSecond, curvature: f64) -> f64 {
    (speed.0 * speed.0 * curvature / GRAVITY)
        .atan()
        .clamp(-MAX_LEAN, MAX_LEAN)
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

    #[test]
    fn virtual_gears_step_evenly_from_low_to_high() {
        let gears = VirtualGears::new(50, 14);

        assert!((gears.ratio(0) - 0.75).abs() < 1e-9);
        assert!((gears.ratio(GEARS - 1) - 5.5).abs() < 1e-9);
        let step = gears.ratio(1) / gears.ratio(0);
        for gear in 1..GEARS {
            assert!((gears.ratio(gear) / gears.ratio(gear - 1) - step).abs() < 1e-9);
        }
        // 50/14 = 3.57: the gear nearest to it.
        let neutral = gears.neutral();
        assert!((gears.ratio(neutral) / (50.0 / 14.0)).ln().abs() < step.ln() / 2.0);
    }

    #[test]
    fn a_virtual_gear_brakes_the_flywheel_as_the_road_would_in_that_gear() {
        let gears = VirtualGears::new(50, 14);
        let road = RiderSetup::default().simulation_parameters(GradePercent(4.0));
        // What a trainer brakes in slope simulation at flywheel speed `v` (small grades).
        let mass = 83.0;
        let force = |p: SimulationParameters, v: f64| {
            mass * GRAVITY * (p.grade.0 / 100.0 + p.crr) + p.cw.0 * v * v
        };
        let v = 8.0;
        for gear in [3, gears.neutral(), 16] {
            let r = gears.ratio(gear) / (50.0 / 14.0);

            let trainer = force(gears.in_gear(road, gear), v);

            // The road's force at the gear's speed, times its leverage.
            let expected = r * force(road, r * v);
            assert!(
                (trainer - expected).abs() < 1e-6,
                "gear {gear}: {trainer} vs {expected}"
            );
        }
    }

    #[test]
    fn the_hardest_gears_stay_within_what_ftms_can_send() {
        let gears = VirtualGears::new(34, 14);
        let road = RiderSetup::default().simulation_parameters(GradePercent(8.0));

        let hardest = gears.in_gear(road, GEARS - 1);

        assert!(
            hardest.crr <= MAX_CRR && hardest.cw.0 <= MAX_CW,
            "{hardest:?}"
        );
        assert!(
            hardest.grade.0 > 8.0,
            "the grade itself is the trainer's to limit"
        );
    }

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
    fn steep_ramps_are_slow_but_never_stall() {
        // 100 W up 40 %: a crawl, but still moving.
        let speed = settled_kmh(100.0, 40.0);
        assert!((0.5..3.0).contains(&speed), "{speed} km/h");
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

    #[test]
    fn riders_lean_into_bends_as_balance_demands() {
        // 36 km/h through a bend of 50 m radius: tan φ = 100 / (9.81 · 50), about 11.5°.
        let fast = MetersPerSecond::from_kilometers_per_hour(36.0);
        let lean = lean_angle(fast, 1.0 / 50.0).to_degrees();
        assert!((lean - 11.5).abs() < 0.1, "{lean}°");
        // To the left the other way; on the straight or standing still upright.
        assert!((lean_angle(fast, -1.0 / 50.0).to_degrees() + lean).abs() < 1e-9);
        assert!(lean_angle(fast, 0.0).abs() < 1e-12);
        assert!(lean_angle(MetersPerSecond(0.0), 1.0 / 10.0).abs() < 1e-12);
        // Twice as fast leans much further, but never past 45°.
        assert!(lean_angle(MetersPerSecond(20.0), 1.0 / 50.0) > 2.0 * lean.to_radians());
        let hairpin = lean_angle(MetersPerSecond(20.0), 1.0 / 5.0).to_degrees();
        assert!((hairpin - 45.0).abs() < 1e-9, "{hairpin}°");
    }
}
