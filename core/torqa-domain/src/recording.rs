//! What gets recorded during a ride.

use std::time::Duration;

use crate::units::{BeatsPerMinute, GradePercent, Meters, MetersPerSecond, Rpm, Watts};

/// The rider's state at one moment of a ride, recorded once per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Time since the start of the ride.
    pub elapsed: Duration,
    /// Latitude in degrees (WGS84).
    pub lat: f64,
    /// Longitude in degrees (WGS84).
    pub lon: f64,
    /// Elevation.
    pub elevation: Meters,
    /// Distance from the start.
    pub distance: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// Road gradient.
    pub grade: GradePercent,
    /// Power, if a power source is connected.
    pub power: Option<Watts>,
    /// Cadence, if known.
    pub cadence: Option<Rpm>,
    /// Heart rate, if a sensor is connected.
    pub heart_rate: Option<BeatsPerMinute>,
}
