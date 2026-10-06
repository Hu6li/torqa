//! Workouts (R56, R21, R22): the trainer holds a target power (ERG) that the workout sets
//! instead of following the road — a constant power, one adjusted continuously to hold the
//! rider's heart rate, the steps of a structured workout, or one of the FTP tests.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use torqa_domain::profile::Profile;
use torqa_domain::recording::Sample;
use torqa_domain::telemetry::Telemetry;
use torqa_domain::units::{BeatsPerMinute, Rpm, Watts};
use torqa_domain::workout::{Cue, Intensity, Plan, Step, Target};

use crate::analysis::{effort_test_ftp, ramp_test_ftp};

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
/// A structured workout's text cue shows this long.
const CUE_SHOWS: Duration = Duration::from_secs(10);
/// In an FTP test, a cadence below this…
const FAILING_CADENCE: f64 = 50.0;
/// …for this long means the rider cannot hold the step any more: the test is over. In ERG the
/// trainer holds the power whatever the cadence, so the cadence is what gives way.
const FAILING_FOR: Duration = Duration::from_secs(10);
/// Easy pedalling between the parts of an FTP test, as a share of FTP (40–50 %).
const EASY: f64 = 0.45;
/// The cool-down after an FTP test: very easy.
const COOL_DOWN: f64 = 0.4;

/// What a workout asks of the rider.
#[derive(Debug, Clone, PartialEq)]
pub enum Workout {
    /// ERG at a fixed power, e.g. 200 W.
    ConstantPower(Watts),
    /// Power adjusted continuously so the heart rate settles at a target.
    HeartRate(HeartRateHold),
    /// A structured workout (R21): each step's power for a rider with `ftp`. Free steps leave
    /// the trainer simulating the road; without a route the ride ends after the last step.
    Structured {
        /// The steps.
        plan: Arc<Plan>,
        /// The rider's FTP, for steps given as a share of it.
        ftp: Watts,
    },
    /// An FTP test (R22): power rising step by step until the rider cannot hold it.
    RampTest(RampTest),
    /// An FTP test (R22, #125) of one or two all-out efforts: 20 minutes, or twice 8.
    EffortTest(EffortTest),
}

/// A ramp test (R22): after a warm-up the power rises every minute until the rider's cadence
/// gives way; 75 % of their best minute is then their FTP (see
/// [`crate::analysis::ramp_test_ftp`]). Short and maximal, it needs no pacing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampTest {
    /// How long the warm-up lasts.
    pub warm_up: Duration,
    /// Its power.
    pub warm_up_power: Watts,
    /// The power of the first step.
    pub start: Watts,
    /// How much each step adds.
    pub step: Watts,
    /// How long each step lasts.
    pub step_duration: Duration,
}

impl RampTest {
    /// A test for a rider whose FTP is about `ftp` (the profile's, or a guess): five minutes
    /// warming up at 40 %, then from half of it up by 6 % a minute — most riders give way
    /// after 15–25 minutes in all.
    #[must_use]
    pub fn for_ftp(ftp: Watts) -> Self {
        let round = |watts: f64, to: f64| (watts / to).round() * to;
        let ftp = ftp.0.max(100.0);
        Self {
            warm_up: Duration::from_mins(5),
            warm_up_power: Watts(round(ftp * 0.4, 5.0)),
            start: Watts(round(ftp * 0.5, 5.0)),
            step: Watts(round(ftp * 0.06, 5.0).max(5.0)),
            step_duration: Duration::from_mins(1),
        }
    }

    /// The power `elapsed` into the test.
    #[must_use]
    pub fn power_at(&self, elapsed: Duration) -> Watts {
        match elapsed.checked_sub(self.warm_up) {
            None => self.warm_up_power,
            Some(into) => {
                let steps = (into.as_secs_f64() / self.step_duration.as_secs_f64()).floor();
                Watts(self.start.0 + self.step.0 * steps)
            }
        }
    }
}

/// An FTP test of all-out efforts (R22, #125): after a warm-up, 20 minutes or twice 8 minutes
/// as hard as the rider can hold. The trainer holds the powers of the warm-up and the rests
/// (ERG) and leaves the efforts to the rider, who paces them with gears and cadence on a flat
/// road. A share of the efforts' average power is then their FTP (see
/// [`crate::analysis::effort_test_ftp`]).
#[derive(Debug, Clone, PartialEq)]
pub struct EffortTest {
    /// Warm-up, efforts, rests and cool-down; the efforts and other all-out parts are free
    /// steps.
    pub plan: Arc<Plan>,
    /// The FTP the other steps' powers are set for.
    pub ftp: Watts,
    /// Which of the plan's steps are the efforts the FTP is taken from.
    pub efforts: Vec<usize>,
    /// The share of the efforts' average power the rider can hold for an hour.
    pub share: f64,
}

impl EffortTest {
    /// The 20-minute test for a rider whose FTP is about `ftp`: 15 minutes of warm-up, a
    /// minute each at 90, 100, 110 and 120 % and all out with a minute's rest between, 5 easy
    /// minutes, then 20 minutes all out and 10 minutes of cool-down: an hour in all. FTP is
    /// 95 % of the 20 minutes' average.
    #[must_use]
    pub fn twenty_minutes(ftp: Watts) -> Self {
        let mut test = Self::warmed_up(ftp, 0.95);
        // i18n-begin
        test.cue("Activation: a minute each at 90, 100, 110 and 120 %, then one all out");
        for share in [0.9, 1.0, 1.1, 1.2] {
            test.steady(1, share);
            test.steady(1, EASY);
        }
        test.cue("One minute all out!");
        test.all_out(1, false);
        test.cue("Easy for 5 minutes before the test");
        test.steady(5, EASY);
        test.cue("20 minutes all out: start controlled, give everything in the second half");
        test.all_out(20, true);
        test.cue("Done! Cool down easy");
        // i18n-end
        test.steady(10, COOL_DOWN);
        test
    }

    /// The 2 × 8-minute test for a rider whose FTP is about `ftp`: 15 minutes of warm-up,
    /// 8 minutes all out, 10 easy minutes, 8 minutes all out again and 10 minutes of
    /// cool-down: 51 minutes in all. FTP is 90 % of the two efforts' average.
    #[must_use]
    pub fn two_by_eight(ftp: Watts) -> Self {
        let mut test = Self::warmed_up(ftp, 0.9);
        // i18n-begin
        test.cue("8 minutes all out: as hard as you can hold to the end");
        test.all_out(8, true);
        test.cue("Easy for 10 minutes");
        test.steady(10, EASY);
        test.cue("The second 8 minutes all out");
        test.all_out(8, true);
        test.cue("Done! Cool down easy");
        // i18n-end
        test.steady(10, COOL_DOWN);
        test
    }

    /// When the efforts are, from the start of the test.
    #[must_use]
    pub fn effort_times(&self) -> Vec<Range<Duration>> {
        let mut start = Duration::ZERO;
        let mut times = Vec::new();
        for (index, step) in self.plan.steps.iter().enumerate() {
            let end = start + step.duration;
            if self.efforts.contains(&index) {
                times.push(start..end);
            }
            start = end;
        }
        times
    }

    /// The start of every test: 10 minutes rising from 50 to 65 % of FTP, then 5 minutes at
    /// 72 % (the "medium" 70–75 %).
    fn warmed_up(ftp: Watts, share: f64) -> Self {
        let mut test = Self {
            plan: Arc::new(Plan {
                name: "FTP test".to_owned(),
                ..Plan::default()
            }),
            ftp,
            efforts: Vec::new(),
            share,
        };
        // i18n-begin
        test.cue("Warm-up: easy at first, then a little harder");
        test.step(
            10,
            Target::Power {
                from: Intensity::Ftp(0.5),
                to: Intensity::Ftp(0.65),
            },
        );
        test.cue("Find the cadence you will hold in the test");
        // i18n-end
        test.steady(5, 0.72);
        test
    }

    fn step(&mut self, minutes: u64, target: Target) {
        Arc::make_mut(&mut self.plan).steps.push(Step {
            duration: Duration::from_mins(minutes),
            target,
            cadence: None,
        });
    }

    fn steady(&mut self, minutes: u64, share: f64) {
        self.step(minutes, Target::steady(Intensity::Ftp(share)));
    }

    /// A free step ridden all out; `counts` if the FTP is taken from it.
    fn all_out(&mut self, minutes: u64, counts: bool) {
        if counts {
            self.efforts.push(self.plan.steps.len());
        }
        self.step(minutes, Target::Free);
    }

    /// A message at the start of the next step.
    fn cue(&mut self, text: &str) {
        let at = self.plan.duration();
        Arc::make_mut(&mut self.plan).cues.push(Cue {
            at,
            text: text.to_owned(),
        });
    }
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
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutState {
    /// The power the trainer is asked to hold; `None` in a free step or after the last one.
    pub target_power: Option<Watts>,
    /// The heart rate being held, in a heart-rate workout.
    pub target_heart_rate: Option<BeatsPerMinute>,
    /// Where a structured workout stands.
    pub progress: Option<Progress>,
}

/// Where a structured workout stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// Index of the step under way, and how many there are.
    pub step: usize,
    /// Number of steps.
    pub steps: usize,
    /// Time left in this step.
    pub step_left: Duration,
    /// Time left in the whole workout.
    pub left: Duration,
    /// The cadence this step asks for.
    pub cadence: Option<Rpm>,
    /// The step after this one.
    pub next: Option<NextStep>,
    /// A message of the workout to show now.
    pub cue: Option<String>,
    /// This step is ridden all out: a free step of an FTP test (#125).
    pub all_out: bool,
}

/// The step coming up in a structured workout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextStep {
    /// The power it starts at; `None` for a free step.
    pub power: Option<Watts>,
    /// How long it lasts.
    pub duration: Duration,
    /// It is ridden all out, as in [`Progress::all_out`].
    pub all_out: bool,
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
    /// Time since the workout started.
    elapsed: Duration,
    /// How long the cadence has been too low in an FTP test.
    failing_for: Duration,
    /// The FTP test has ended: the rider gave way.
    gave_way: bool,
}

impl WorkoutControl {
    pub(crate) fn new(workout: Workout) -> Self {
        let power = match &workout {
            Workout::ConstantPower(power) => power.0.max(0.0),
            Workout::HeartRate(hold) => hold.limits().0,
            Workout::Structured { .. } | Workout::EffortTest(_) => 0.0,
            Workout::RampTest(test) => test.warm_up_power.0,
        };
        Self {
            workout,
            power,
            last_heart_rate: None,
            pending: Duration::ZERO,
            elapsed: Duration::ZERO,
            failing_for: Duration::ZERO,
            gave_way: false,
        }
    }

    /// Switches to another workout during the ride. A new heart-rate hold carries on from the
    /// current power rather than starting over.
    pub(crate) fn change(&mut self, workout: Workout) {
        let current = self.power;
        let carry_on = match &workout {
            Workout::HeartRate(hold) => {
                let (low, high) = hold.limits();
                Some(current.clamp(low, high))
            }
            _ => None,
        };
        *self = Self::new(workout);
        if let Some(power) = carry_on {
            self.power = power;
        }
    }

    /// The power for the trainer now; `None` lets it simulate the road.
    pub(crate) fn power(&self) -> Option<Watts> {
        match &self.workout {
            Workout::Structured { plan, ftp } => plan.at(self.elapsed, *ftp)?.power,
            Workout::EffortTest(test) => test.plan.at(self.elapsed, test.ftp)?.power,
            Workout::RampTest(test) => Some(test.power_at(self.elapsed)),
            _ => Some(Watts(self.power)),
        }
    }

    /// Whether a structured workout or an FTP test of efforts has run its last step, or the
    /// rider gave way in a ramp test; the others go on until stopped.
    pub(crate) fn is_finished(&self) -> bool {
        match &self.workout {
            Workout::Structured { plan, .. } => self.elapsed >= plan.duration(),
            Workout::EffortTest(test) => self.elapsed >= test.plan.duration(),
            Workout::RampTest(_) => self.gave_way,
            _ => false,
        }
    }

    /// The FTP an FTP test (R22) shows from the ride's `samples`; `None` for other workouts
    /// and for a test left too early.
    pub(crate) fn ftp_estimate(&self, samples: &[Sample]) -> Option<Watts> {
        match &self.workout {
            Workout::RampTest(_) => ramp_test_ftp(samples),
            Workout::EffortTest(test) => effort_test_ftp(samples, &test.effort_times(), test.share),
            _ => None,
        }
    }

    pub(crate) fn state(&self) -> WorkoutState {
        WorkoutState {
            target_power: self.power(),
            target_heart_rate: match &self.workout {
                Workout::HeartRate(hold) => Some(hold.target),
                _ => None,
            },
            progress: match &self.workout {
                Workout::Structured { plan, ftp } => self.progress(plan, *ftp, false),
                Workout::EffortTest(test) => self.progress(&test.plan, test.ftp, true),
                Workout::RampTest(test) => Some(self.ramp_progress(test)),
                _ => None,
            },
        }
    }

    /// An FTP test's progress: open-ended, so `steps` is 0 and nothing is left in all; the
    /// warm-up is step 0.
    fn ramp_progress(&self, test: &RampTest) -> Progress {
        let (step, step_left) = match self.elapsed.checked_sub(test.warm_up) {
            None => (0, test.warm_up.saturating_sub(self.elapsed)),
            Some(into) => {
                let length = test.step_duration.as_secs_f64();
                let done = (into.as_secs_f64() / length).floor();
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // ≥ 0
                let index = done as usize + 1;
                let left = Duration::from_secs_f64(length * (done + 1.0)).saturating_sub(into);
                (index, left)
            }
        };
        Progress {
            step,
            steps: 0,
            step_left,
            left: Duration::ZERO,
            cadence: None,
            next: Some(NextStep {
                power: Some(test.power_at(self.elapsed + step_left)),
                duration: test.step_duration,
                all_out: false,
            }),
            cue: None,
            all_out: false,
        }
    }

    /// Where a structured workout stands; in an FTP test (`test`) its free steps are ridden
    /// all out.
    fn progress(&self, plan: &Plan, ftp: Watts, test: bool) -> Option<Progress> {
        let at = plan.at(self.elapsed, ftp)?;
        let next = plan.steps.get(at.step + 1).map(|step| NextStep {
            power: match step.target {
                Target::Power { from, .. } => Some(from.watts(ftp)),
                Target::Free => None,
            },
            duration: step.duration,
            all_out: test && step.target == Target::Free,
        });
        Some(Progress {
            step: at.step,
            steps: plan.steps.len(),
            step_left: at.left,
            left: plan.duration().saturating_sub(self.elapsed),
            cadence: plan.steps[at.step].cadence,
            next,
            cue: plan
                .cue_at(self.elapsed, CUE_SHOWS)
                .map(|cue| cue.text.clone()),
            all_out: test && plan.steps[at.step].target == Target::Free,
        })
    }

    /// Advances by `dt` with the latest measurements.
    pub(crate) fn update(&mut self, telemetry: &Telemetry, dt: Duration) {
        self.elapsed += dt;
        if let Workout::RampTest(test) = &self.workout {
            // Only the steps count: a slow start in the warm-up is no failure.
            let pedalling = telemetry.cadence.is_some_and(|c| c.0 >= FAILING_CADENCE);
            self.failing_for = if self.elapsed > test.warm_up && !pedalling {
                self.failing_for + dt
            } else {
                Duration::ZERO
            };
            self.gave_way = self.gave_way || self.failing_for >= FAILING_FOR;
            return;
        }
        let Workout::HeartRate(hold) = &self.workout else {
            return;
        };
        let hold = *hold;
        let heart_rate = telemetry.heart_rate;
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

    fn beating(bpm: Option<f64>) -> Telemetry {
        Telemetry {
            heart_rate: bpm.map(BeatsPerMinute),
            ..Telemetry::default()
        }
    }

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

        control.update(&beating(Some(180.0)), Duration::from_secs(60));

        assert_eq!(control.power(), Some(Watts(200.0)));
        assert_eq!(control.state().target_heart_rate, None);
    }

    #[test]
    fn heart_rate_holds_start_at_the_lower_limit_given_in_any_order() {
        let mut hold = HeartRateHold::zone(&rider(), 2, Watts(220.0), Watts(120.0));
        assert_eq!(
            WorkoutControl::new(Workout::HeartRate(hold)).power(),
            Some(Watts(120.0))
        );

        hold.min_power = Watts(-50.0);
        assert_eq!(
            WorkoutControl::new(Workout::HeartRate(hold)).power(),
            Some(Watts(0.0))
        );
    }

    #[test]
    fn without_a_heart_rate_the_power_is_kept() {
        let hold = HeartRateHold::zone(&rider(), 3, Watts(100.0), Watts(250.0));
        let mut control = WorkoutControl::new(Workout::HeartRate(hold));
        control.update(&beating(Some(100.0)), Duration::from_secs(30));
        let before = control.power();

        control.update(&beating(None), Duration::from_secs(120));

        assert_eq!(control.power(), before);
    }

    #[test]
    fn a_new_hold_carries_on_from_the_current_power() {
        let hold = HeartRateHold::zone(&rider(), 3, Watts(100.0), Watts(250.0));
        let mut control = WorkoutControl::new(Workout::ConstantPower(Watts(180.0)));

        control.change(Workout::HeartRate(hold));
        assert_eq!(control.power(), Some(Watts(180.0)));

        control.change(Workout::HeartRate(HeartRateHold {
            max_power: Watts(150.0),
            ..hold
        }));
        assert_eq!(control.power(), Some(Watts(150.0)), "within the new limits");
    }
}
