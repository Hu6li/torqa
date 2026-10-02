//! Measurements reported by devices and the resistance controls sent to trainers.

use crate::units::{
    BeatsPerMinute, GradePercent, KilogramsPerMeter, MetersPerSecond, Percent, Rpm, Watts,
};

/// Measurements reported by a device. Fields the device did not report are `None`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Telemetry {
    /// Instantaneous power.
    pub power: Option<Watts>,
    /// Instantaneous cadence.
    pub cadence: Option<Rpm>,
    /// Speed as measured by the trainer.
    pub speed: Option<MetersPerSecond>,
    /// Heart rate.
    pub heart_rate: Option<BeatsPerMinute>,
}

impl Telemetry {
    /// Overlays the fields present in `newer`, keeping the current value for fields it lacks.
    ///
    /// Devices may split their data across packets (FTMS "More Data") and several devices
    /// contribute to one rider state, so a reading must not erase fields it does not carry.
    pub fn merge(&mut self, newer: &Telemetry) {
        self.power = newer.power.or(self.power);
        self.cadence = newer.cadence.or(self.cadence);
        self.speed = newer.speed.or(self.speed);
        self.heart_rate = newer.heart_rate.or(self.heart_rate);
    }
}

/// How a trainer should set its resistance (R8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrainerControl {
    /// Slope simulation: the trainer derives resistance from road conditions and wheel speed.
    Simulation(SimulationParameters),
    /// ERG: the trainer holds the target power regardless of cadence.
    TargetPower(Watts),
    /// A fixed share of the trainer's resistance range.
    Resistance(Percent),
}

/// Road conditions for slope simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationParameters {
    /// Gradient felt on the trainer.
    pub grade: GradePercent,
    /// Head wind (positive) or tail wind (negative).
    pub wind_speed: MetersPerSecond,
    /// Coefficient of rolling resistance (dimensionless).
    pub crr: f64,
    /// Wind resistance coefficient.
    pub cw: KilogramsPerMeter,
}

impl Default for SimulationParameters {
    /// Flat road, no wind, typical road-bike coefficients as used by common trainer apps.
    fn default() -> Self {
        Self {
            grade: GradePercent(0.0),
            wind_speed: MetersPerSecond(0.0),
            crr: 0.004,
            cw: KilogramsPerMeter(0.51),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_fields_missing_from_newer_reading() {
        let mut state = Telemetry {
            power: Some(Watts(200.0)),
            cadence: Some(Rpm(90.0)),
            speed: Some(MetersPerSecond(10.0)),
            heart_rate: None,
        };
        state.merge(&Telemetry {
            power: Some(Watts(210.0)),
            heart_rate: Some(BeatsPerMinute(140.0)),
            ..Telemetry::default()
        });

        assert_eq!(state.power, Some(Watts(210.0)));
        assert_eq!(state.cadence, Some(Rpm(90.0)));
        assert_eq!(state.speed, Some(MetersPerSecond(10.0)));
        assert_eq!(state.heart_rate, Some(BeatsPerMinute(140.0)));
    }
}
