//! Bounded organizer progress for explicit entry into an already running app ride.

use std::time::Duration;

use orqwa_domain::units::Meters;

use super::LobbyError;

/// Current organizer position and workout time, without personal telemetry or FTP targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RideProgress {
    /// Absolute position on the shared course, or flat-road position without a course.
    pub distance: Meters,
    /// Time into the selected structured workout, including any skipped steps.
    pub workout: Option<Duration>,
}
impl RideProgress {
    pub(crate) fn validate(self) -> Result<(), LobbyError> {
        if !self.distance.0.is_finite()
            || !(0.0..=10_000_000.0).contains(&self.distance.0)
            || self
                .workout
                .is_some_and(|time| time > Duration::from_hours(48))
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
}

/// Validates the signed age of a shared start, including a countdown still in progress.
pub(crate) fn clock(age: i64) -> Result<(), LobbyError> {
    if !(-60_000..=172_800_000).contains(&age) {
        return Err(LobbyError::Protocol);
    }
    Ok(())
}
