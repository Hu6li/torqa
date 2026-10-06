//! Structured workouts (R21): steps of power targets over time, as read from workout files
//! (ZWO, ERG/MRC, FIT) through a [`WorkoutParser`].

use std::fmt;
use std::time::Duration;

use crate::units::{Rpm, Watts};

/// A structured workout: its steps one after the other, and text cues along the way.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Display name.
    pub name: String,
    /// What the workout is about, as its author put it; may be empty.
    pub description: String,
    /// The steps in order.
    pub steps: Vec<Step>,
    /// Messages shown during the workout, by time from its start.
    pub cues: Vec<Cue>,
}

/// One step of a [`Plan`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// How long it lasts.
    pub duration: Duration,
    /// What the trainer holds meanwhile.
    pub target: Target,
    /// The cadence the rider should keep, if the workout says.
    pub cadence: Option<Rpm>,
}

/// What a step asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// A power, changing evenly from `from` to `to` over the step (equal for a steady step).
    Power {
        /// At the start of the step.
        from: Intensity,
        /// At its end.
        to: Intensity,
    },
    /// No set power: the rider rides as they like (or all out), the trainer simulating the
    /// road.
    Free,
}

impl Target {
    /// A steady power.
    #[must_use]
    pub fn steady(intensity: Intensity) -> Self {
        Self::Power {
            from: intensity,
            to: intensity,
        }
    }
}

/// How hard: relative to the rider's FTP, as most workout files say, or in watts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intensity {
    /// A share of FTP, e.g. 0.75 for 75 %.
    Ftp(f64),
    /// A power.
    Watts(Watts),
}

impl Intensity {
    /// The power for a rider with `ftp`.
    #[must_use]
    pub fn watts(self, ftp: Watts) -> Watts {
        match self {
            Self::Ftp(share) => Watts(share * ftp.0),
            Self::Watts(watts) => watts,
        }
    }
}

/// A message shown during a workout.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// When, from the start of the workout.
    pub at: Duration,
    /// What it says.
    pub text: String,
}

/// Where a workout stands at one moment, see [`Plan::at`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Index of the step under way.
    pub step: usize,
    /// Time left in it.
    pub left: Duration,
    /// The power asked for now for a rider with the given FTP; `None` in a free step.
    pub power: Option<Watts>,
}

impl Plan {
    /// How long the whole workout lasts.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.steps.iter().map(|s| s.duration).sum()
    }

    /// Where the workout stands `elapsed` after its start, for a rider with `ftp`; `None` once
    /// it is over.
    #[must_use]
    pub fn at(&self, elapsed: Duration, ftp: Watts) -> Option<Position> {
        let mut start = Duration::ZERO;
        for (index, step) in self.steps.iter().enumerate() {
            let end = start + step.duration;
            if elapsed < end {
                let into =
                    elapsed.saturating_sub(start).as_secs_f64() / step.duration.as_secs_f64();
                let power = match step.target {
                    Target::Power { from, to } => {
                        let (a, b) = (from.watts(ftp).0, to.watts(ftp).0);
                        Some(Watts(a + (b - a) * into))
                    }
                    Target::Free => None,
                };
                return Some(Position {
                    step: index,
                    left: end.saturating_sub(elapsed),
                    power,
                });
            }
            start = end;
        }
        None
    }

    /// The cue to show at `elapsed`: the latest one that started at most `showing` before.
    #[must_use]
    pub fn cue_at(&self, elapsed: Duration, showing: Duration) -> Option<&Cue> {
        self.cues
            .iter()
            .filter(|cue| cue.at <= elapsed && elapsed < cue.at + showing)
            .max_by_key(|cue| cue.at)
    }
}

/// A workout file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkoutFileError(pub String);

impl fmt::Display for WorkoutFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WorkoutFileError {}

/// Reads one kind of workout file (R4, R21).
pub trait WorkoutParser {
    /// The file extensions it reads, lowercase without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Reads a workout; `name` is used when the file has none (e.g. its file name).
    ///
    /// # Errors
    /// [`WorkoutFileError`] if the file is not a workout this parser understands.
    fn parse(&self, bytes: &[u8], name: &str) -> Result<Plan, WorkoutFileError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            steps: vec![
                Step {
                    duration: Duration::from_secs(600),
                    target: Target::Power {
                        from: Intensity::Ftp(0.5),
                        to: Intensity::Ftp(0.7),
                    },
                    cadence: None,
                },
                Step {
                    duration: Duration::from_secs(60),
                    target: Target::steady(Intensity::Watts(Watts(300.0))),
                    cadence: Some(Rpm(100.0)),
                },
                Step {
                    duration: Duration::from_secs(120),
                    target: Target::Free,
                    cadence: None,
                },
            ],
            cues: vec![
                Cue {
                    at: Duration::from_secs(600),
                    text: "Go!".to_owned(),
                },
                Cue {
                    at: Duration::from_secs(640),
                    text: "Hold it".to_owned(),
                },
            ],
            ..Plan::default()
        }
    }

    #[test]
    fn ramps_rise_evenly_and_shares_of_ftp_follow_the_rider() {
        let plan = plan();
        let ftp = Watts(200.0);

        let start = plan.at(Duration::ZERO, ftp).unwrap();
        let middle = plan.at(Duration::from_secs(300), ftp).unwrap();

        assert_eq!(start.power, Some(Watts(100.0)));
        assert!((middle.power.unwrap().0 - 120.0).abs() < 1e-9);
        assert_eq!(middle.left, Duration::from_secs(300));
        // Watts stay watts, whatever the FTP.
        let hard = plan.at(Duration::from_secs(630), Watts(400.0)).unwrap();
        assert_eq!((hard.step, hard.power), (1, Some(Watts(300.0))));
    }

    #[test]
    fn free_steps_set_no_power_and_the_plan_ends_after_its_last_step() {
        let plan = plan();

        let free = plan.at(Duration::from_secs(700), Watts(200.0)).unwrap();

        assert_eq!((free.step, free.power), (2, None));
        assert_eq!(plan.duration(), Duration::from_mins(13));
        assert_eq!(plan.at(Duration::from_mins(13), Watts(200.0)), None);
    }

    #[test]
    fn a_cue_shows_for_a_while_until_the_next_one() {
        let plan = plan();
        let showing = Duration::from_secs(10);
        let text = |s| {
            plan.cue_at(Duration::from_secs(s), showing)
                .map(|c| c.text.as_str())
        };

        assert_eq!(text(599), None);
        assert_eq!(text(605), Some("Go!"));
        assert_eq!(text(615), None);
        assert_eq!(text(645), Some("Hold it"));
    }
}
