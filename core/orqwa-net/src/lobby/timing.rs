//! Organizer-owned workout time, independent of physical riding and personal recording.

use std::time::{Duration, Instant};

use super::LobbyError;

/// An ordered shared workout clock anchored to the original group start.
#[derive(Clone, Copy, Debug)]
pub struct WorkoutClock {
    /// Frozen activity revision.
    pub revision: u64,
    /// Strictly increasing organizer update, including pause/resume and skips.
    pub sequence: u64,
    /// Workout position at `at`, including skipped steps.
    pub elapsed: Duration,
    /// Local monotonic instant corresponding to the organizer's sample.
    pub at: Instant,
    /// Whether the workout clock is stopped; physical riding can continue.
    pub paused: bool,
}
impl WorkoutClock {
    /// Current workout time. A local safety pause does not stop the shared clock.
    #[must_use]
    pub fn position(self, now: Instant) -> Duration {
        let added = if self.paused {
            Duration::ZERO
        } else {
            now.saturating_duration_since(self.at)
        };
        self.elapsed
            .saturating_add(added)
            .min(crate::workout::MAX_DURATION)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkoutSample {
    pub(crate) revision: u64,
    pub(crate) sequence: u64,
    pub(crate) group: Duration,
    pub(crate) elapsed: Duration,
    pub(crate) paused: bool,
}
impl WorkoutSample {
    pub(crate) fn validate(self) -> Result<(), LobbyError> {
        if self.revision == 0
            || self.sequence == 0
            || self.group > crate::workout::MAX_DURATION
            || self.elapsed > crate::workout::MAX_DURATION
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
    pub(crate) fn clock(self, start: Instant) -> Result<WorkoutClock, LobbyError> {
        self.validate()?;
        let at = start.checked_add(self.group).ok_or(LobbyError::Protocol)?;
        if at > Instant::now() + Duration::from_secs(2) {
            return Err(LobbyError::Protocol);
        }
        Ok(WorkoutClock {
            revision: self.revision,
            sequence: self.sequence,
            elapsed: self.elapsed,
            at,
            paused: self.paused,
        })
    }
}

pub(super) struct Timeline(WorkoutClock);
impl Timeline {
    pub(super) const fn from_clock(clock: WorkoutClock) -> Self {
        Self(clock)
    }
    pub(super) fn new(revision: u64, start: Instant) -> Self {
        Self(WorkoutClock {
            revision,
            sequence: 0,
            elapsed: Duration::ZERO,
            at: start,
            paused: false,
        })
    }
    pub(super) fn pause(&mut self, paused: bool) {
        let now = Instant::now();
        self.0.elapsed = self.0.position(now);
        self.0.at = now;
        self.0.paused = paused;
    }
    pub(super) fn skip(&mut self, next: Duration) -> Result<(), LobbyError> {
        let now = Instant::now();
        if next > crate::workout::MAX_DURATION || next < self.0.position(now) {
            return Err(LobbyError::Protocol);
        }
        self.0.elapsed = next;
        self.0.at = now;
        Ok(())
    }
    pub(super) fn position(&self) -> Duration {
        self.0.position(Instant::now())
    }
    pub(super) fn sample(&mut self, start: Instant) -> Result<WorkoutSample, LobbyError> {
        self.0.sequence = self.0.sequence.checked_add(1).ok_or(LobbyError::Protocol)?;
        let now = Instant::now();
        let sample = WorkoutSample {
            revision: self.0.revision,
            sequence: self.0.sequence,
            group: now.saturating_duration_since(start),
            elapsed: self.0.position(now),
            paused: self.0.paused,
        };
        sample.validate()?;
        Ok(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_pause_and_delivery_delay_do_not_change_the_shared_workout_clock() {
        let start = Instant::now().checked_sub(Duration::from_mins(1)).unwrap();
        let sample = WorkoutSample {
            revision: 1,
            sequence: 4,
            group: Duration::from_secs(40),
            elapsed: Duration::from_secs(700),
            paused: false,
        };
        let clock = sample.clock(start).unwrap();
        assert_eq!(
            clock.position(start + Duration::from_secs(60)),
            Duration::from_mins(12)
        );
        let stopped = WorkoutClock {
            paused: true,
            ..clock
        };
        assert_eq!(
            stopped.position(start + Duration::from_secs(600)),
            Duration::from_secs(700)
        );
        let mut timeline = Timeline::new(1, start);
        timeline.pause(true);
        let held = timeline.position();
        timeline.skip(Duration::from_mins(15)).unwrap();
        assert_eq!(timeline.position(), Duration::from_mins(15));
        assert!(timeline.skip(held).is_err());
        let first = timeline.sample(start).unwrap();
        timeline.pause(false);
        let second = timeline.sample(start).unwrap();
        assert!(second.sequence > first.sequence);
        assert!(first.paused && !second.paused);
    }
}
