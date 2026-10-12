//! What gets recorded during a ride.

use std::time::Duration;

use crate::units::{BeatsPerMinute, GradePercent, Joules, Meters, MetersPerSecond, Rpm, Watts};

/// The rider's state at one moment of a ride, recorded once per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Time since the start of the ride.
    pub elapsed: Duration,
    /// Where on the road the rider was; `None` on rides without a route, such as workouts
    /// (R56), and in activities recorded without positions.
    pub location: Option<Location>,
    /// Distance from the start.
    pub distance: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// Power, if a power source is connected.
    pub power: Option<Watts>,
    /// Cadence, if known.
    pub cadence: Option<Rpm>,
    /// Heart rate, if a sensor is connected.
    pub heart_rate: Option<BeatsPerMinute>,
}

/// Where on the road a sample was taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    /// Latitude in degrees (WGS84).
    pub lat: f64,
    /// Longitude in degrees (WGS84).
    pub lon: f64,
    /// Elevation.
    pub elevation: Meters,
    /// Road gradient.
    pub grade: GradePercent,
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

const fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// The durations a ride's power curve keeps (R62): spaced evenly on a logarithmic scale from a
/// second to eight hours, so short efforts are as finely resolved as long ones, and including
/// the durations riders compare ([`RECORD_DURATIONS`]).
pub const CURVE_DURATIONS: [Duration; 36] = [
    secs(1),
    secs(2),
    secs(3),
    secs(5),
    secs(8),
    secs(10),
    secs(15),
    secs(20),
    secs(30),
    secs(45),
    secs(60),
    secs(90),
    secs(120),
    secs(180),
    secs(240),
    secs(300),
    secs(360),
    secs(480),
    secs(600),
    secs(720),
    secs(900),
    secs(1200),
    secs(1500),
    secs(1800),
    secs(2400),
    secs(3000),
    secs(3600),
    secs(4500),
    secs(5400),
    secs(7200),
    secs(9000),
    secs(10_800),
    secs(14_400),
    secs(18_000),
    secs(21_600),
    secs(28_800),
];

/// The durations whose best power counts as a personal record (R62): sprint, anaerobic
/// capacity, maximal aerobic power, threshold and the hour.
pub const RECORD_DURATIONS: [Duration; 5] = [secs(5), secs(60), secs(300), secs(1200), secs(3600)];

/// The best average power held over each of the [`CURVE_DURATIONS`] (R62), shortest first, up
/// to the longest the rider rode without a pause.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PowerCurve(pub Vec<(Duration, Watts)>);

impl PowerCurve {
    /// The best average power over `duration`, if the curve reaches it.
    #[must_use]
    pub fn at(&self, duration: Duration) -> Option<Watts> {
        self.0.iter().find(|(d, _)| *d == duration).map(|(_, p)| *p)
    }
}

/// What the analysis across rides keeps of each ride (R62), derived from its samples.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RideAnalysis {
    /// The ride's best power over each duration.
    pub power_curve: PowerCurve,
}
