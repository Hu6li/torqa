//! What gets recorded during a ride.

use std::time::Duration;

use crate::units::{BeatsPerMinute, GradePercent, Joules, Meters, MetersPerSecond, Rpm, Watts};

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

/// Key figures of a recorded ride, for the history and its analysis (R31).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RideSummary {
    /// Duration from start to the last sample.
    pub elapsed: Duration,
    /// Distance ridden.
    pub distance: Meters,
    /// Total climbing.
    pub elevation_gain: Meters,
    /// Distance over elapsed time.
    pub avg_speed: MetersPerSecond,
    /// Highest speed.
    pub max_speed: MetersPerSecond,
    /// Average over samples with power.
    pub avg_power: Option<Watts>,
    /// Highest power.
    pub max_power: Option<Watts>,
    /// Normalized power: what the ride cost physiologically, weighting hard efforts more than
    /// an average does. Needs at least 30 s of power.
    pub normalized_power: Option<Watts>,
    /// Normalized power relative to [`RideSummary::ftp`].
    pub intensity_factor: Option<f64>,
    /// Training stress score: one hour at FTP scores 100.
    pub training_stress: Option<f64>,
    /// Mechanical work.
    pub work: Option<Joules>,
    /// Average cadence while pedalling.
    pub avg_cadence: Option<Rpm>,
    /// Average heart rate.
    pub avg_heart_rate: Option<BeatsPerMinute>,
    /// Highest heart rate.
    pub max_heart_rate: Option<BeatsPerMinute>,
    /// The rider's FTP when the ride was analysed; base of intensity and stress.
    pub ftp: Watts,
}
