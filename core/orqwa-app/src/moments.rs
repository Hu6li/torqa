//! The moments of a ride worth telling the rider of (R124): what the ride screen shows on a
//! card while it happens and the summary lists as the ride's highlights.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orqwa_domain::profile::UnitSystem;
use orqwa_domain::recording::{RECORD_DURATIONS, Sample};
use orqwa_domain::units::{GradePercent, Meters, Watts};
use orqwa_session::fitness;
use orqwa_session::workout::WorkoutState;
use orqwa_storage::rides::RideRecord;

/// Distance between the "so far" milestones: 25 of the rider's kilometres or miles.
const MILESTONE_STEPS: f64 = 25.0;
const METERS_PER_MILE: f64 = 1_609.344;
/// Halfway and "1 km to go" need a route longer than this, or they crowd its start.
const MIN_MILESTONE_ROUTE: Meters = Meters(2_000.0);
/// A "so far" milestone this close to halfway or the last kilometre would only repeat it.
const MILESTONE_GAP: Meters = Meters(1_000.0);

/// A workout step counts as held from this long…
const STEP_MIN: Duration = Duration::from_mins(1);
/// …at this share of FTP or above: recoveries and warm-ups need no praise…
const STEP_MIN_SHARE: f64 = 0.75;
/// …with the average power this close to the target's.
const STEP_TOLERANCE: f64 = 0.05;
/// A step left with more than this to go was skipped, not held.
const STEP_SKIPPED: Duration = Duration::from_secs(2);

/// Something that happened on the ride.
#[derive(Debug, Clone, PartialEq)]
pub enum Moment {
    /// The rider topped a climb, not faster than before.
    ClimbTopped {
        /// Index in [`Route::climbs`](orqwa_routes::Route::climbs).
        index: usize,
        /// Time from foot to top.
        elapsed: Duration,
        /// The climb's average gradient.
        grade: GradePercent,
    },
    /// The rider topped a climb faster than ever before (R27).
    ClimbRecord {
        /// Index in [`Route::climbs`](orqwa_routes::Route::climbs).
        index: usize,
        /// Time from foot to top.
        elapsed: Duration,
        /// How much faster than the best time before.
        gain: Duration,
    },
    /// Half the route lies behind the rider.
    Halfway {
        /// The distance ridden.
        distance: Meters,
    },
    /// The finish is a kilometre away, or a mile for a rider in miles.
    OneToGo,
    /// Another 25 km ridden, or 25 miles for a rider in miles: the distance so far.
    Distance(Meters),
    /// The rider's best average power ever over one of the
    /// [`RECORD_DURATIONS`](orqwa_domain::recording::RECORD_DURATIONS) (R62).
    PowerPeak {
        /// How long.
        duration: Duration,
        /// The new best.
        power: Watts,
    },
    /// The rider held a hard step of a structured workout on its target to its end.
    StepHeld {
        /// Index of the step in the workout.
        index: usize,
        /// The rider's average power over it.
        power: Watts,
    },
    /// The rider has never ridden this course before.
    FirstRide {
        /// The course's name.
        course: String,
    },
    /// The rider's rides this week, Monday to Sunday, up to this one.
    Streak {
        /// How many, this one included.
        rides: u32,
    },
    /// Rides this week against the number the rider plans for one.
    WeekPlan {
        /// This ride's number in the week.
        ride: u32,
        /// How many the rider plans.
        planned: u32,
    },
}

/// Finds the milestones of a ride as the rider passes them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Milestones {
    /// The route's length; a ride without a route only has the "so far" milestones.
    length: Option<Meters>,
    /// Where the rider was at the last look.
    passed: Meters,
    /// The rider's kilometre or mile.
    unit: Meters,
}

impl Milestones {
    pub(crate) fn new(length: Option<Meters>, units: UnitSystem) -> Self {
        Self {
            length,
            passed: Meters(0.0),
            unit: Meters(match units {
                UnitSystem::Metric => 1_000.0,
                UnitSystem::Imperial => METERS_PER_MILE,
            }),
        }
    }

    /// The milestones between the last look and `distance`, in the order the rider met them.
    pub(crate) fn reach(&mut self, distance: Meters) -> Vec<Moment> {
        let from = self.passed.0;
        let to = distance.0;
        if to <= from {
            return Vec::new();
        }
        self.passed = distance;
        let crossed = |at: f64| from < at && at <= to;
        let mut found: Vec<(f64, Moment)> = Vec::new();
        let unit = self.unit.0;
        let every = MILESTONE_STEPS * unit;
        let length = self.length.filter(|l| l.0 > MIN_MILESTONE_ROUTE.0);
        if let Some(length) = length {
            let half = length.0 / 2.0;
            if crossed(half) {
                found.push((
                    half,
                    Moment::Halfway {
                        distance: Meters(half),
                    },
                ));
            }
            let last = length.0 - unit;
            if crossed(last) {
                found.push((last, Moment::OneToGo));
            }
        }
        // Whole multiples only: the steps between looks are far shorter than the gap.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first = (from / every).floor() as u64 + 1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let last = (to / every).floor() as u64;
        for n in first..=last {
            #[allow(clippy::cast_precision_loss)] // a few multiples of 25 km
            let at = n as f64 * every;
            let crowded = length.is_some_and(|length| {
                (at - length.0 / 2.0).abs() < MILESTONE_GAP.0
                    || at > length.0 - unit - MILESTONE_GAP.0
            });
            if !crowded {
                found.push((at, Moment::Distance(Meters(at))));
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, moment)| moment).collect()
    }

    /// The rider jumped to `distance`: what lies between counts as passed, untold.
    pub(crate) fn skip_to(&mut self, distance: Meters) {
        self.passed = distance;
    }
}

/// Finds the rider's power records while they ride them: once an average over one of the
/// [`RECORD_DURATIONS`] beats the best before, it is told when the effort tops out, so a long
/// sprint is one moment rather than one a second.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PowerPeaks {
    /// Per duration: the best before this ride, raised by each record told; none for a rider
    /// without records, whose every effort would be one.
    bests: [Option<Watts>; RECORD_DURATIONS.len()],
    /// Per duration: the record being ridden, the highest average so far.
    rising: [Option<Watts>; RECORD_DURATIONS.len()],
    /// `sums[i]`: the power of the first `i` samples added up.
    sums: Vec<f64>,
}

impl PowerPeaks {
    pub(crate) fn new(bests: [Option<Watts>; RECORD_DURATIONS.len()]) -> Self {
        Self {
            bests,
            rising: [None; RECORD_DURATIONS.len()],
            sums: vec![0.0],
        }
    }

    /// The records topped out with the samples the ride recorded since the last look; `samples`
    /// are all of them, one a second.
    pub(crate) fn follow(&mut self, samples: &[Sample]) -> Vec<Moment> {
        let mut told = Vec::new();
        for sample in samples.iter().skip(self.sums.len() - 1) {
            let sum = self.sums.last().copied().unwrap_or_default();
            // Like the power curve, a second without power counts as none.
            self.sums.push(sum + sample.power.map_or(0.0, |p| p.0));
            told.extend(self.look());
        }
        told
    }

    fn look(&mut self) -> Vec<Moment> {
        let seconds = self.sums.len() - 1;
        let mut told = Vec::new();
        for (i, duration) in RECORD_DURATIONS.into_iter().enumerate() {
            let Some(window) = usize::try_from(duration.as_secs())
                .ok()
                .filter(|w| *w <= seconds)
            else {
                continue;
            };
            #[allow(clippy::cast_precision_loss)] // at most an hour of seconds
            let average = Watts((self.sums[seconds] - self.sums[seconds - window]) / window as f64);
            let Some(best) = self.bests[i] else {
                continue;
            };
            let bar = self.rising[i].map_or(best.0, |rising| rising.0);
            if average.0 > bar {
                self.rising[i] = Some(average);
            } else if let Some(peak) = self.rising[i].take() {
                self.bests[i] = Some(peak);
                told.push(Moment::PowerPeak {
                    duration,
                    power: peak,
                });
            }
        }
        told
    }
}

/// Finds the hard steps of a structured workout the rider held on their target.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StepsHeld {
    ftp: Watts,
    /// Samples looked at so far.
    seen: usize,
    step: Option<StepRun>,
}

/// A step under way.
#[derive(Debug, Clone, PartialEq)]
struct StepRun {
    index: usize,
    seconds: u32,
    /// The rider's power and the target, added up over its seconds.
    power: f64,
    target: f64,
    /// Every second had a power to hold, none of them all out.
    steady: bool,
    left: Duration,
}

impl StepsHeld {
    pub(crate) fn new(ftp: Watts) -> Self {
        Self {
            ftp,
            seen: 0,
            step: None,
        }
    }

    /// Takes the samples recorded since the last look, with the workout as it stands now; the
    /// step held, if one just ended.
    pub(crate) fn follow(
        &mut self,
        samples: &[Sample],
        workout: Option<&WorkoutState>,
    ) -> Option<Moment> {
        let new = samples.get(self.seen..).unwrap_or_default();
        self.seen = samples.len();
        let progress = workout.and_then(|w| w.progress.as_ref());
        let told = if self.step.as_ref().map(|s| s.index) == progress.map(|p| p.step) {
            None
        } else {
            let ended = self.step.take().and_then(|run| self.held(&run));
            self.step = progress.map(|p| StepRun {
                index: p.step,
                seconds: 0,
                power: 0.0,
                target: 0.0,
                steady: true,
                left: p.step_left,
            });
            ended
        };
        if let (Some(run), Some(progress)) = (&mut self.step, progress) {
            let target = workout.and_then(|w| w.target_power);
            for sample in new {
                run.seconds += 1;
                run.power += sample.power.map_or(0.0, |p| p.0);
                run.target += target.map_or(0.0, |t| t.0);
            }
            run.steady &= target.is_some() && !progress.all_out;
            run.left = progress.step_left;
        }
        told
    }

    fn held(&self, run: &StepRun) -> Option<Moment> {
        let seconds = f64::from(run.seconds);
        let (power, target) = (run.power / seconds, run.target / seconds);
        let held = run.steady
            && run.left <= STEP_SKIPPED
            && Duration::from_secs(u64::from(run.seconds)) >= STEP_MIN
            && target >= self.ftp.0 * STEP_MIN_SHARE
            && (power - target).abs() <= target * STEP_TOLERANCE;
        held.then_some(Moment::StepHeld {
            index: run.index,
            power: Watts(power),
        })
    }
}

/// What the summary of the ride `record` says of it beyond its figures and records (R124):
/// that it was the rider's first on its course, or else how far into their week it is (Monday
/// to Sunday in local time, `utc_offset_s` ahead of UTC), against the `planned` rides a week if
/// they set a number. Nothing for a lone ride without a plan, nor for a simulated one, which
/// counts towards nothing; neither do the simulated ones among `others`, the rider's other
/// rides.
pub(crate) fn recap(
    record: &RideRecord,
    others: &[&RideRecord],
    planned: Option<u8>,
    utc_offset_s: i64,
) -> Option<Moment> {
    if record.simulated {
        return None;
    }
    let before: Vec<&RideRecord> = others
        .iter()
        .copied()
        .filter(|other| !other.simulated && other.start < record.start)
        .collect();
    if let Some(key) = &record.route_key
        && !before
            .iter()
            .any(|other| other.route_key.as_ref() == Some(key))
    {
        return Some(Moment::FirstRide {
            course: record.route.clone(),
        });
    }
    let week =
        |r: &RideRecord| fitness::week_start(fitness::day_of(unix_seconds(r.start), utc_offset_s));
    let earlier = before
        .iter()
        .filter(|other| week(other) == week(record))
        .count();
    let rides = u32::try_from(earlier + 1).unwrap_or(u32::MAX);
    match planned.filter(|p| *p > 0) {
        Some(planned) => Some(Moment::WeekPlan {
            ride: rides,
            planned: u32::from(planned),
        }),
        None => (rides >= 2).then_some(Moment::Streak { rides }),
    }
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The moment of topping a climb in `elapsed`, with the best time before this ride; a ride
/// that does not count towards records (`real` false) sets none.
pub(crate) fn climb_topped(
    index: usize,
    elapsed: Duration,
    grade: GradePercent,
    previous_best: Option<Duration>,
    real: bool,
) -> Moment {
    match previous_best.filter(|best| real && elapsed < *best) {
        Some(best) => Moment::ClimbRecord {
            index,
            elapsed,
            gain: best.saturating_sub(elapsed),
        },
        None => Moment::ClimbTopped {
            index,
            elapsed,
            grade,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rides `milestones` from the start to `to` in steps of `step`, collecting what it tells.
    fn ride(milestones: &mut Milestones, to: f64, step: f64) -> Vec<Moment> {
        let mut told = Vec::new();
        let mut at = 0.0;
        while at < to {
            at = (at + step).min(to);
            told.extend(milestones.reach(Meters(at)));
        }
        told
    }

    #[test]
    fn a_long_route_tells_each_milestone_once_in_order() {
        let mut milestones = Milestones::new(Some(Meters(80_000.0)), UnitSystem::Metric);

        let told = ride(&mut milestones, 80_000.0, 7.5);

        assert_eq!(
            told,
            vec![
                Moment::Distance(Meters(25_000.0)),
                Moment::Halfway {
                    distance: Meters(40_000.0)
                },
                Moment::Distance(Meters(50_000.0)),
                Moment::Distance(Meters(75_000.0)),
                Moment::OneToGo,
            ]
        );
    }

    #[test]
    fn a_distance_milestone_next_to_halfway_or_the_finish_is_left_out() {
        let mut milestones = Milestones::new(Some(Meters(50_400.0)), UnitSystem::Metric);

        let told = ride(&mut milestones, 50_400.0, 10.0);

        // 25 km is 200 m from halfway, 50 km lies in the last kilometre.
        assert_eq!(
            told,
            vec![
                Moment::Halfway {
                    distance: Meters(25_200.0)
                },
                Moment::OneToGo,
            ]
        );
    }

    #[test]
    fn a_short_route_only_tells_of_halfway_and_the_last_kilometre_when_long_enough() {
        let mut short = Milestones::new(Some(Meters(1_500.0)), UnitSystem::Metric);
        assert_eq!(ride(&mut short, 1_500.0, 5.0), []);

        let mut three = Milestones::new(Some(Meters(3_000.0)), UnitSystem::Metric);
        assert_eq!(
            ride(&mut three, 3_000.0, 5.0),
            vec![
                Moment::Halfway {
                    distance: Meters(1_500.0)
                },
                Moment::OneToGo,
            ]
        );
    }

    #[test]
    fn a_ride_without_a_route_counts_the_distance_only() {
        let mut milestones = Milestones::new(None, UnitSystem::Metric);

        let told = ride(&mut milestones, 60_000.0, 9.0);

        assert_eq!(
            told,
            vec![
                Moment::Distance(Meters(25_000.0)),
                Moment::Distance(Meters(50_000.0)),
            ]
        );
    }

    #[test]
    fn a_rider_in_miles_gets_milestones_in_miles() {
        let mile = 1_609.344;
        let every = 25.0 * mile;
        let mut milestones = Milestones::new(Some(Meters(100.0 * mile)), UnitSystem::Imperial);

        let told = ride(&mut milestones, 100.0 * mile, 10.0);

        assert_eq!(
            told,
            vec![
                Moment::Distance(Meters(every)),
                Moment::Halfway {
                    distance: Meters(50.0 * mile)
                },
                Moment::Distance(Meters(3.0 * every)),
                Moment::OneToGo,
            ],
            "50 mi is halfway, and 1 mi to go comes at 99 mi"
        );
        let mut again = Milestones::new(Some(Meters(100.0 * mile)), UnitSystem::Imperial);
        let last_mile = ride(&mut again, 99.0 * mile - 5.0, 10.0);
        assert!(!last_mile.contains(&Moment::OneToGo));
    }

    #[test]
    fn jumping_passes_milestones_without_telling_them() {
        let mut milestones = Milestones::new(Some(Meters(80_000.0)), UnitSystem::Metric);
        milestones.skip_to(Meters(60_000.0));

        let told = ride_from(&mut milestones, 60_000.0, 80_000.0);

        assert_eq!(
            told,
            vec![Moment::Distance(Meters(75_000.0)), Moment::OneToGo]
        );
    }

    fn ride_from(milestones: &mut Milestones, from: f64, to: f64) -> Vec<Moment> {
        let mut told = Vec::new();
        let mut at = from;
        while at < to {
            at = (at + 10.0).min(to);
            told.extend(milestones.reach(Meters(at)));
        }
        told
    }

    /// One sample a second with these powers.
    fn samples(powers: &[f64]) -> Vec<Sample> {
        powers
            .iter()
            .enumerate()
            .map(|(i, &p)| Sample {
                elapsed: Duration::from_secs(i as u64),
                location: None,
                distance: Meters(0.0),
                speed: orqwa_domain::units::MetersPerSecond(0.0),
                power: Some(Watts(p)),
                cadence: None,
                heart_rate: None,
            })
            .collect()
    }

    /// Bests of `five_s` over 5 s and far out of reach over the longer durations.
    fn bests(five_s: Option<f64>) -> [Option<Watts>; RECORD_DURATIONS.len()] {
        let mut bests = [Some(Watts(2_000.0)); RECORD_DURATIONS.len()];
        bests[0] = five_s.map(Watts);
        bests
    }

    #[test]
    fn a_power_record_is_told_once_when_the_effort_tops_out() {
        let mut peaks = PowerPeaks::new(bests(Some(400.0)));
        let mut powers = vec![200.0; 30];
        powers.extend([450.0; 8]);
        powers.extend([200.0; 30]);
        let ride = samples(&powers);

        // Fed bit by bit, as the ride records them.
        let mut told = Vec::new();
        for end in (1..=ride.len()).step_by(3) {
            told.extend(peaks.follow(&ride[..end]));
        }
        told.extend(peaks.follow(&ride));

        assert_eq!(
            told,
            vec![Moment::PowerPeak {
                duration: Duration::from_secs(5),
                power: Watts(450.0)
            }]
        );
    }

    #[test]
    fn only_an_effort_beyond_the_last_record_is_told_again() {
        let mut peaks = PowerPeaks::new(bests(Some(400.0)));
        let mut powers = vec![200.0; 10];
        for sprint in [450.0, 430.0, 500.0] {
            powers.extend([sprint; 6]);
            powers.extend([200.0; 20]);
        }

        let told = peaks.follow(&samples(&powers));

        let five_s = |w: f64| Moment::PowerPeak {
            duration: Duration::from_secs(5),
            power: Watts(w),
        };
        assert_eq!(told, vec![five_s(450.0), five_s(500.0)]);
    }

    #[test]
    fn a_rider_without_records_or_power_is_told_of_none() {
        let mut powers = vec![200.0; 10];
        powers.extend([450.0; 8]);
        powers.extend([200.0; 10]);
        assert_eq!(PowerPeaks::new(bests(None)).follow(&samples(&powers)), []);

        let mut quiet = samples(&[0.0; 30]);
        for sample in &mut quiet {
            sample.power = None;
        }
        assert_eq!(PowerPeaks::new(bests(Some(100.0))).follow(&quiet), []);
    }

    /// A structured workout at `step`, `left` to go in it, asking for `target`.
    fn workout(step: usize, left: u64, target: Option<f64>) -> WorkoutState {
        WorkoutState {
            target_power: target.map(Watts),
            target_heart_rate: None,
            progress: Some(orqwa_session::workout::Progress {
                step,
                steps: 3,
                elapsed: Duration::ZERO,
                step_left: Duration::from_secs(left),
                left: Duration::ZERO,
                cadence: None,
                next: None,
                cue: None,
                all_out: false,
            }),
            intensity: orqwa_domain::units::Percent(100.0),
            paused: false,
        }
    }

    /// Rides a step of `seconds` at `power` against `target`, then moves to the next step;
    /// what was told.
    fn ride_step(
        held: &mut StepsHeld,
        ride: &mut Vec<Sample>,
        (step, seconds): (usize, u64),
        power: f64,
        target: Option<f64>,
        skipped: bool,
    ) -> Vec<Moment> {
        let mut told = Vec::new();
        for s in 0..seconds {
            ride.extend(samples(&[power]));
            let left = if skipped { 100 } else { seconds - s - 1 };
            told.extend(held.follow(ride, Some(&workout(step, left, target))));
        }
        told
    }

    #[test]
    fn a_hard_step_ridden_on_target_to_its_end_is_held() {
        let mut held = StepsHeld::new(Watts(250.0));
        let mut ride = Vec::new();
        let mut told = Vec::new();

        told.extend(ride_step(
            &mut held,
            &mut ride,
            (0, 300),
            120.0,
            Some(125.0),
            false,
        ));
        told.extend(ride_step(
            &mut held,
            &mut ride,
            (1, 240),
            262.0,
            Some(260.0),
            false,
        ));
        told.extend(ride_step(
            &mut held,
            &mut ride,
            (2, 30),
            120.0,
            Some(125.0),
            false,
        ));

        assert_eq!(
            told,
            vec![Moment::StepHeld {
                index: 1,
                power: Watts(262.0)
            }],
            "the warm-up is too easy to praise"
        );
    }

    #[test]
    fn a_step_off_target_short_free_or_skipped_is_not_held() {
        let cases: [(u64, f64, Option<f64>, bool, &str); 4] = [
            (240, 230.0, Some(260.0), false, "off target"),
            (40, 260.0, Some(260.0), false, "too short"),
            (240, 260.0, None, false, "a free step"),
            (240, 260.0, Some(260.0), true, "skipped"),
        ];
        for (seconds, power, target, skipped, what) in cases {
            let mut held = StepsHeld::new(Watts(250.0));
            let mut ride = Vec::new();
            ride_step(&mut held, &mut ride, (0, seconds), power, target, skipped);
            let told = ride_step(&mut held, &mut ride, (1, 5), 100.0, Some(100.0), false);
            assert_eq!(told, [], "{what}");
        }
    }

    const DAY: Duration = Duration::from_hours(24);
    /// A Monday, noon UTC: 2026-10-19.
    const MONDAY: u64 = 20_745 * 86_400 + 12 * 3_600;

    /// A real ride on "Gurten" `days` after a Monday.
    fn gurten(days: u32) -> RideRecord {
        RideRecord {
            route: "Gurten".to_owned(),
            start: UNIX_EPOCH + Duration::from_secs(MONDAY) + DAY * days,
            summary: orqwa_domain::recording::RideSummary::default(),
            route_key: Some("gurten".to_owned()),
            route_time: None,
            climbs: Vec::new(),
            name: None,
            ftp_estimate: None,
            rider_mass: None,
            simulated: false,
            assisted: false,
            analysis: None,
            uploads: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn the_first_ride_on_a_course_is_told_before_the_week() {
        let first = gurten(0);
        let mut simulated = gurten(1);
        simulated.simulated = true;
        let second = gurten(2);
        let course = Some(Moment::FirstRide {
            course: "Gurten".to_owned(),
        });

        assert_eq!(
            recap(&first, &[&second], Some(3), 0),
            course,
            "later rides do not count"
        );
        assert_eq!(
            recap(&simulated, &[&first], None, 0),
            None,
            "simulated rides count for nothing"
        );
        let mut elsewhere = gurten(3);
        elsewhere.route_key = Some("bantiger".to_owned());
        assert_eq!(
            recap(&second, &[&first, &simulated, &elsewhere], None, 0),
            Some(Moment::Streak { rides: 2 })
        );
    }

    #[test]
    fn the_week_counts_rides_from_monday_against_the_riders_plan() {
        let mut sunday = gurten(0);
        sunday.start -= DAY;
        let monday = gurten(0);
        let mut simulated = gurten(1);
        simulated.simulated = true;
        let wednesday = gurten(2);
        let others = [&sunday, &monday, &simulated];

        assert_eq!(
            recap(&wednesday, &others, None, 0),
            Some(Moment::Streak { rides: 2 })
        );
        assert_eq!(
            recap(&wednesday, &others, Some(4), 0),
            Some(Moment::WeekPlan {
                ride: 2,
                planned: 4
            })
        );
        assert_eq!(
            recap(&monday, &[&sunday], Some(3), 0),
            Some(Moment::WeekPlan {
                ride: 1,
                planned: 3
            })
        );
        assert_eq!(
            recap(&monday, &[&sunday], None, 0),
            None,
            "one ride is no streak"
        );
        // Thirteen hours ahead of UTC, Sunday noon is already Monday.
        assert_eq!(
            recap(&monday, &[&sunday], None, 13 * 3_600),
            Some(Moment::Streak { rides: 2 })
        );
    }

    #[test]
    fn a_climb_is_a_record_only_when_faster_than_a_real_earlier_time() {
        let grade = GradePercent(8.0);
        let minutes = |m: u64| Duration::from_secs(m * 60);

        assert_eq!(
            climb_topped(1, minutes(14), grade, Some(minutes(15)), true),
            Moment::ClimbRecord {
                index: 1,
                elapsed: minutes(14),
                gain: minutes(1)
            }
        );
        let topped = Moment::ClimbTopped {
            index: 1,
            elapsed: minutes(14),
            grade,
        };
        assert_eq!(
            climb_topped(1, minutes(14), grade, None, true),
            topped,
            "a first time"
        );
        assert_eq!(
            climb_topped(1, minutes(14), grade, Some(minutes(13)), true),
            topped,
            "slower"
        );
        assert_eq!(
            climb_topped(1, minutes(14), grade, Some(minutes(15)), false),
            topped,
            "simulated"
        );
    }
}
