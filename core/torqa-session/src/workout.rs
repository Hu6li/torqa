//! Workouts (R56): the trainer holds a target power (ERG) that the workout sets instead of
//! following the road — a constant power, or one adjusted continuously to hold the rider's
//! heart rate.

use std::time::Duration;

use torqa_domain::profile::Profile;
use torqa_domain::units::{BeatsPerMinute, Watts};

/// The heart-rate hold adjusts the power this often; straps report about once a second.
const STEP: Duration = Duration::from_secs(1);
/// How fast the heart rate follows a change of power (the time constant of its lag, 30–60 s
/// for most riders). The hold's proportional part cancels this lag.
const HEART_LAG_S: f64 = 45.0;
/// How long the hold takes, once the lag is cancelled, to make about two thirds of a
/// correction: well above the lag, so a rider whose heart reacts twice as strongly or as
/// slowly as estimated still settles without swinging.
const SETTLE_S: f64 = 90.0;
/// The hold changes the power by at most this many watts per second (30 W a minute), so the
/// heart rate can keep up and the rider is never surprised by a jump.
const MAX_RAMP: f64 = 0.5;
/// From rest to threshold power (FTP) the heart rate rises by about this share of its maximum
/// (from about 35 % to about 90 %).
const REST_TO_THRESHOLD: f64 = 0.55;

/// What a workout asks of the rider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Workout {
    /// ERG at a fixed power, e.g. 200 W.
    ConstantPower(Watts),
    /// Power adjusted continuously so the heart rate settles at a target.
    HeartRate(HeartRateHold),
}

/// Holding a heart rate (R56). Heart rate lags power by 30–60 s, so the power ramps gently and
/// stays within the rider's limits; it starts at the lower limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeartRateHold {
    /// The heart rate to hold.
    pub target: BeatsPerMinute,
    /// Lowest power asked for, and where the hold starts.
    pub min_power: Watts,
    /// Highest power asked for.
    pub max_power: Watts,
    /// How many beats per minute the rider's heart rate rises per watt once settled: how
    /// much power a beat off target is worth.
    pub response: f64,
}

impl HeartRateHold {
    /// Holds the middle of the rider's heart-rate zone `zone` (1–5), e.g. 75 % of the maximum
    /// for zone 3.
    #[must_use]
    pub fn zone(profile: &Profile, zone: u8, min_power: Watts, max_power: Watts) -> Self {
        let (low, high) = profile.heart_rate_zone_range(zone);
        Self::bpm(
            profile,
            BeatsPerMinute(f64::midpoint(low.0, high.0)),
            min_power,
            max_power,
        )
    }

    /// Holds `target` beats per minute. The rider's FTP and maximum heart rate estimate how
    /// strongly their heart rate responds to power.
    #[must_use]
    pub fn bpm(
        profile: &Profile,
        target: BeatsPerMinute,
        min_power: Watts,
        max_power: Watts,
    ) -> Self {
        let response = if profile.ftp.0 > 0.0 {
            REST_TO_THRESHOLD * profile.max_heart_rate.0 / profile.ftp.0
        } else {
            0.5
        };
        Self {
            target,
            min_power,
            max_power,
            response,
        }
    }

    /// The limits in order, however they were given.
    fn limits(&self) -> (f64, f64) {
        let (a, b) = (self.min_power.0.max(0.0), self.max_power.0.max(0.0));
        (a.min(b), a.max(b))
    }
}

/// What a workout asks for right now, for display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkoutState {
    /// The power the trainer is asked to hold.
    pub target_power: Watts,
    /// The heart rate being held, in a heart-rate workout.
    pub target_heart_rate: Option<BeatsPerMinute>,
}

/// A workout under way: works out the power for the trainer as the ride goes on.
#[derive(Debug, Clone)]
pub(crate) struct WorkoutControl {
    workout: Workout,
    power: f64,
    /// The heart rate at the last adjustment, to see where it is heading.
    last_heart_rate: Option<f64>,
    /// Time since the last adjustment.
    pending: Duration,
}

impl WorkoutControl {
    pub(crate) fn new(workout: Workout) -> Self {
        let power = match workout {
            Workout::ConstantPower(power) => power.0.max(0.0),
            Workout::HeartRate(hold) => hold.limits().0,
        };
        Self {
            workout,
            power,
            last_heart_rate: None,
            pending: Duration::ZERO,
        }
    }

    /// Switches to another workout during the ride. A new heart-rate hold carries on from the
    /// current power rather than starting over.
    pub(crate) fn change(&mut self, workout: Workout) {
        let current = self.power;
        *self = Self::new(workout);
        if let Workout::HeartRate(hold) = workout {
            let (low, high) = hold.limits();
            self.power = current.clamp(low, high);
        }
    }

    pub(crate) fn power(&self) -> Watts {
        Watts(self.power)
    }

    pub(crate) fn state(&self) -> WorkoutState {
        WorkoutState {
            target_power: self.power(),
            target_heart_rate: match self.workout {
                Workout::ConstantPower(_) => None,
                Workout::HeartRate(hold) => Some(hold.target),
            },
        }
    }

    /// Advances by `dt` with the latest heart rate.
    pub(crate) fn update(&mut self, heart_rate: Option<BeatsPerMinute>, dt: Duration) {
        let Workout::HeartRate(hold) = self.workout else {
            return;
        };
        self.pending += dt;
        while self.pending >= STEP {
            self.pending -= STEP;
            self.adjust(&hold, heart_rate);
        }
    }

    /// One step of a PI controller in velocity form: the integral part pushes the power while
    /// the heart rate is off target, the proportional part pulls back as the heart rate moves
    /// towards it. With their ratio at the heart's lag, the power stops rising before the
    /// heart rate overshoots. Working on changes of power, it cannot wind up at the limits.
    fn adjust(&mut self, hold: &HeartRateHold, heart_rate: Option<BeatsPerMinute>) {
        let Some(heart_rate) = heart_rate.map(|h| h.0) else {
            // Without a heart rate, keep the power until it is back.
            self.last_heart_rate = None;
            return;
        };
        let response = hold.response.max(0.05);
        let integral = 1.0 / (response * SETTLE_S);
        let proportional = integral * HEART_LAG_S;
        let step = STEP.as_secs_f64();
        let rise = self.last_heart_rate.map_or(0.0, |last| heart_rate - last);
        let change = integral * (hold.target.0 - heart_rate) * step - proportional * rise;
        let (low, high) = hold.limits();
        self.power =
            (self.power + change.clamp(-MAX_RAMP * step, MAX_RAMP * step)).clamp(low, high);
        self.last_heart_rate = Some(heart_rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rider() -> Profile {
        Profile {
            ftp: Watts(200.0),
            max_heart_rate: BeatsPerMinute(185.0),
            ..Profile::default()
        }
    }

    #[test]
    fn zone_holds_aim_at_the_middle_of_the_zone() {
        let hold = HeartRateHold::zone(&rider(), 3, Watts(100.0), Watts(250.0));

        // Zone 3 of 185 bpm: 70–80 %.
        assert!((hold.target.0 - 138.75).abs() < 1e-9, "{:?}", hold.target);
        // From rest to FTP the heart rate rises by about 100 bpm over 200 W.
        assert!((hold.response - 0.51).abs() < 0.01, "{}", hold.response);
    }

    #[test]
    fn constant_power_asks_for_its_power_whatever_the_heart_rate() {
        let mut control = WorkoutControl::new(Workout::ConstantPower(Watts(200.0)));

        control.update(Some(BeatsPerMinute(180.0)), Duration::from_secs(60));

        assert_eq!(control.power(), Watts(200.0));
        assert_eq!(control.state().target_heart_rate, None);
    }

    #[test]
    fn heart_rate_holds_start_at_the_lower_limit_given_in_any_order() {
        let mut hold = HeartRateHold::zone(&rider(), 2, Watts(220.0), Watts(120.0));
        assert_eq!(
            WorkoutControl::new(Workout::HeartRate(hold)).power(),
            Watts(120.0)
        );

        hold.min_power = Watts(-50.0);
        assert_eq!(
            WorkoutControl::new(Workout::HeartRate(hold)).power(),
            Watts(0.0)
        );
    }

    #[test]
    fn without_a_heart_rate_the_power_is_kept() {
        let hold = HeartRateHold::zone(&rider(), 3, Watts(100.0), Watts(250.0));
        let mut control = WorkoutControl::new(Workout::HeartRate(hold));
        control.update(Some(BeatsPerMinute(100.0)), Duration::from_secs(30));
        let before = control.power();

        control.update(None, Duration::from_secs(120));

        assert_eq!(control.power(), before);
    }

    #[test]
    fn a_new_hold_carries_on_from_the_current_power() {
        let hold = HeartRateHold::zone(&rider(), 3, Watts(100.0), Watts(250.0));
        let mut control = WorkoutControl::new(Workout::ConstantPower(Watts(180.0)));

        control.change(Workout::HeartRate(hold));
        assert_eq!(control.power(), Watts(180.0));

        control.change(Workout::HeartRate(HeartRateHold {
            max_power: Watts(150.0),
            ..hold
        }));
        assert_eq!(control.power(), Watts(150.0), "within the new limits");
    }
}
