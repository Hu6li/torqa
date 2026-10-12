//! The rider's power across rides (R62): the best power held over every duration among the
//! rides of a period, compared with all rides or the period before, and the records it sets;
//! the power profile of a period against the rider's best (R63); fitness, fatigue and form and
//! the weekly totals (R64); the best efforts and FTP week by week or month by month (R65).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use torqa_domain::profile::Profile;
use torqa_domain::recording::{CURVE_DURATIONS, RECORD_DURATIONS};
use torqa_domain::units::{BeatsPerMinute, Kilograms, Meters, Watts};
use torqa_session::fitness::{self, Load};
use torqa_storage::rides::RideRecord;

use crate::HistoryEntry;

/// What a period's power curve is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// All of the rider's rides.
    AllTime,
    /// The rides of the period just before, as long as the period.
    PreviousPeriod,
}

/// The best effort over one duration among a set of rides.
#[derive(Debug, Clone, PartialEq)]
pub struct BestEffort {
    /// How long the effort lasted.
    pub duration: Duration,
    /// Its average power.
    pub power: Watts,
    /// Its average power per kilogram of the rider's body mass on that ride.
    pub watts_per_kg: f64,
    /// The FIT file of the ride.
    pub fit: PathBuf,
    /// When the ride started.
    pub start: SystemTime,
}

/// A period's power curve and the one it is compared with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PowerCurves {
    /// The best efforts among the period's rides, shortest first.
    pub period: Vec<BestEffort>,
    /// The best efforts among the rides compared with, shortest first.
    pub compare: Vec<BestEffort>,
}

/// One axis of the power profile (R63): the best power over one of the [`RECORD_DURATIONS`]
/// among a period's rides, against the rider's best of all rides.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileAxis {
    /// How long the efforts lasted.
    pub duration: Duration,
    /// The period's best effort, if a ride of it lasted that long.
    pub period: Option<BestEffort>,
    /// The rider's best effort of all rides.
    pub best: Option<BestEffort>,
    /// The period's best as a share of the rider's best, from 0 to 1 (per kilogram if so
    /// asked); `None` without both.
    pub share: Option<f64>,
}

/// The power profile of the rides in `history` that started within `period` before `now`
/// against all of them, over each of the [`RECORD_DURATIONS`]; `mass` and `per_kg` as for
/// [`power_curves`].
#[must_use]
pub fn power_profile(
    history: &[HistoryEntry],
    period: Option<Duration>,
    now: SystemTime,
    mass: Kilograms,
    per_kg: bool,
) -> Vec<ProfileAxis> {
    let curves = power_curves(history, period, Comparison::AllTime, now, mass, per_kg);
    let at = |efforts: &[BestEffort], duration: Duration| {
        efforts.iter().find(|e| e.duration == duration).cloned()
    };
    let value = |e: &BestEffort| if per_kg { e.watts_per_kg } else { e.power.0 };
    RECORD_DURATIONS
        .into_iter()
        .map(|duration| {
            let period = at(&curves.period, duration);
            let best = at(&curves.compare, duration);
            let share = period
                .as_ref()
                .zip(best.as_ref())
                .filter(|(_, b)| value(b) > 0.0)
                .map(|(p, b)| value(p) / value(b));
            ProfileAxis {
                duration,
                period,
                best,
                share,
            }
        })
        .collect()
}

/// The training load of one local day (R64).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayLoad {
    /// The day, counted from 1970-01-01.
    pub day: i64,
    /// Its stress, and the fitness, fatigue and form it leaves.
    pub load: Load,
}

/// Fitness, fatigue and form for every local day from `first` to `last` (R64), from the TSS
/// of the rides in `history`, local time `utc_offset_s` ahead of UTC. The model runs from the
/// rider's first ride, so a period starting later starts from the load built up before it.
#[must_use]
pub fn day_loads(
    history: &[HistoryEntry],
    first: i64,
    last: i64,
    utc_offset_s: i64,
) -> Vec<DayLoad> {
    let rides: Vec<(i64, f64)> = history
        .iter()
        .filter(|entry| !entry.record.simulated)
        .map(|entry| {
            let day = fitness::day_of(unix_seconds(entry.record.start), utc_offset_s);
            (day, entry.record.summary.training_stress.unwrap_or(0.0))
        })
        .collect();
    let start = rides
        .iter()
        .map(|(day, _)| *day)
        .min()
        .unwrap_or(first)
        .min(first);
    let length = usize::try_from(last - start + 1).unwrap_or(0);
    let mut stress = vec![0.0; length];
    for (day, tss) in rides {
        if let Some(total) = usize::try_from(day - start)
            .ok()
            .and_then(|i| stress.get_mut(i))
        {
            *total += tss;
        }
    }
    fitness::loads(&stress)
        .into_iter()
        .zip(start..)
        .filter(|(_, day)| *day >= first)
        .map(|(load, day)| DayLoad { day, load })
        .collect()
}

/// The durations whose best the progress follows (R65): anaerobic capacity, maximal aerobic
/// power and threshold.
pub const PROGRESS_DURATIONS: [Duration; 3] = [
    Duration::from_mins(1),
    Duration::from_mins(5),
    Duration::from_mins(20),
];

/// The best efforts the FTP estimate looks at (R65): recent enough to be the rider's form now,
/// long enough to hold some maximal ones.
pub const FTP_ESTIMATE_PERIOD: Duration = Duration::from_hours(90 * 24);

/// Periods up to this many days are followed week by week, longer ones month by month.
const WEEKS_UP_TO_DAYS: i64 = 91;

/// One week or month of the rider's progress (R65), in watts or watts per kilogram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgressPoint {
    /// Its first day, counted from 1970-01-01.
    pub start: i64,
    /// The best power over each of the [`PROGRESS_DURATIONS`] among its rides.
    pub bests: [Option<f64>; 3],
    /// The FTP of its last ride, as the ride was saved with.
    pub ftp: Option<f64>,
    /// The highest FTP an FTP test of it showed.
    pub test: Option<f64>,
}

/// The rider's progress over a period of local days.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Progress {
    /// Whether the points are months (else weeks).
    pub monthly: bool,
    /// Every week or month from the one holding the first day to the one holding the last.
    pub points: Vec<ProgressPoint>,
}

/// The rides of `history` from local day `first` to `last` week by week, or month by month for
/// a period longer than 13 weeks (R65): their best 1, 5 and 20 minutes, the FTP they were saved
/// with and the FTP tests. With `per_kg` everything is per kilogram of the rider's mass on each
/// ride (`mass` where a ride did not keep it). Simulated rides are left out.
#[must_use]
pub fn progress(
    history: &[HistoryEntry],
    first: i64,
    last: i64,
    utc_offset_s: i64,
    mass: Kilograms,
    per_kg: bool,
) -> Progress {
    let monthly = last - first + 1 > WEEKS_UP_TO_DAYS;
    let bucket_start = |day: i64| {
        if monthly {
            fitness::month_start(day)
        } else {
            fitness::week_start(day)
        }
    };
    let mut points: Vec<ProgressPoint> = Vec::new();
    let mut start = bucket_start(first);
    while start <= last {
        points.push(ProgressPoint {
            start,
            ..ProgressPoint::default()
        });
        start = if monthly {
            fitness::month_start(start + 31)
        } else {
            start + 7
        };
    }
    // Oldest first, so each point's FTP ends up the one of its last ride.
    let mut rides: Vec<&HistoryEntry> = history
        .iter()
        .filter(|entry| !entry.record.simulated)
        .collect();
    rides.sort_by_key(|entry| entry.record.start);
    for entry in rides {
        let record = &entry.record;
        let day = fitness::day_of(unix_seconds(record.start), utc_offset_s);
        let Some(point) = points
            .iter_mut()
            .find(|point| point.start == bucket_start(day))
            .filter(|_| (first..=last).contains(&day))
        else {
            continue;
        };
        let rider_mass = record.rider_mass.unwrap_or(mass).0;
        let value = |watts: Watts| {
            if !per_kg {
                watts.0
            } else if rider_mass > 0.0 {
                watts.0 / rider_mass
            } else {
                0.0
            }
        };
        if let Some(analysis) = &record.analysis {
            for (best, duration) in point.bests.iter_mut().zip(PROGRESS_DURATIONS) {
                if let Some(power) = analysis.power_curve.at(duration).map(value)
                    && best.is_none_or(|held| power > held)
                {
                    *best = Some(power);
                }
            }
        }
        point.ftp = Some(value(record.summary.ftp));
        if let Some(test) = record.ftp_estimate.map(value)
            && point.test.is_none_or(|held| test > held)
        {
            point.test = Some(test);
        }
    }
    Progress { monthly, points }
}

/// The local days `(first, last)` of the `period` before `now`, or for `None` from the first
/// of the rides in `history` (today without rides); local time `utc_offset_s` ahead of UTC.
#[must_use]
pub fn period_days(
    history: &[HistoryEntry],
    period: Option<Duration>,
    now: SystemTime,
    utc_offset_s: i64,
) -> (i64, i64) {
    let today = fitness::day_of(unix_seconds(now), utc_offset_s);
    let first = match period {
        Some(length) => {
            let days = i64::try_from(length.as_secs() / 86_400).unwrap_or(i64::MAX);
            today.saturating_sub(days) + 1
        }
        None => history
            .iter()
            .filter(|entry| !entry.record.simulated)
            .map(|entry| fitness::day_of(unix_seconds(entry.record.start), utc_offset_s))
            .min()
            .unwrap_or(today),
    };
    (first.min(today), today)
}

/// What the rider rode in one week, Monday to Sunday (R64).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeekTotals {
    /// The Monday the week starts on, counted from 1970-01-01.
    pub start: i64,
    /// How many rides.
    pub rides: u32,
    /// Their riding time.
    pub time: Duration,
    /// Their distance.
    pub distance: Meters,
    /// Their climbing.
    pub climbing: Meters,
    /// Their training stress (TSS).
    pub stress: f64,
    /// Time in each power zone of the rider's zones today.
    pub power_zones: [Duration; 7],
    /// Time in each heart-rate zone of the rider's zones today.
    pub heart_rate_zones: [Duration; 5],
}

/// Every week from the one holding local day `first` to the one holding `last`, with the
/// rides of `history` that fall in it (R64); time in zones uses `profile`'s zones, as the
/// summary does. Local time is `utc_offset_s` ahead of UTC; simulated rides are left out.
#[must_use]
pub fn weekly(
    history: &[HistoryEntry],
    profile: &Profile,
    first: i64,
    last: i64,
    utc_offset_s: i64,
) -> Vec<WeekTotals> {
    let first_week = fitness::week_start(first);
    let mut weeks: Vec<WeekTotals> = (first_week..=last)
        .step_by(7)
        .map(|start| WeekTotals {
            start,
            ..WeekTotals::default()
        })
        .collect();
    for entry in history.iter().filter(|entry| !entry.record.simulated) {
        let record = &entry.record;
        let day = fitness::day_of(unix_seconds(record.start), utc_offset_s);
        let Some(week) = usize::try_from((fitness::week_start(day) - first_week) / 7)
            .ok()
            .filter(|_| day <= last)
            .and_then(|i| weeks.get_mut(i))
        else {
            continue;
        };
        let summary = &record.summary;
        week.rides += 1;
        week.time += summary.elapsed;
        week.distance.0 += summary.distance.0;
        week.climbing.0 += summary.elevation_gain.0;
        week.stress += summary.training_stress.unwrap_or(0.0);
        if let Some(analysis) = &record.analysis {
            let power: [Duration; 7] = analysis
                .power_histogram
                .zones(|watts| profile.power_zone(Watts(watts)));
            let heart: [Duration; 5] = analysis
                .heart_rate_histogram
                .zones(|bpm| profile.heart_rate_zone(BeatsPerMinute(bpm)));
            for (total, zone) in week.power_zones.iter_mut().zip(power) {
                *total += zone;
            }
            for (total, zone) in week.heart_rate_zones.iter_mut().zip(heart) {
                *total += zone;
            }
        }
    }
    weeks
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The power curves of the rides in `history` that started within `period` before `now` (all
/// rides for `None`), and of the rides `comparison` names. With `per_kg`, each duration's best
/// effort is the one with the most power per kilogram, rides that did not keep the rider's mass
/// counting with `mass`.
#[must_use]
pub fn power_curves(
    history: &[HistoryEntry],
    period: Option<Duration>,
    comparison: Comparison,
    now: SystemTime,
    mass: Kilograms,
    per_kg: bool,
) -> PowerCurves {
    let ago = |length: Duration| now.checked_sub(length).unwrap_or(SystemTime::UNIX_EPOCH);
    let between = |from: Option<SystemTime>, until: Option<SystemTime>| {
        history.iter().filter(move |entry| {
            from.is_none_or(|from| entry.record.start >= from)
                && until.is_none_or(|until| entry.record.start < until)
        })
    };
    let period_start = period.map(ago);
    let compare = match (comparison, period) {
        (Comparison::AllTime, _) => best_efforts(history, mass, per_kg),
        (Comparison::PreviousPeriod, Some(length)) => best_efforts(
            between(Some(ago(length.saturating_mul(2))), period_start),
            mass,
            per_kg,
        ),
        (Comparison::PreviousPeriod, None) => Vec::new(),
    };
    PowerCurves {
        period: best_efforts(between(period_start, None), mass, per_kg),
        compare,
    }
}

/// The best effort over each of the [`CURVE_DURATIONS`] among `rides`, leaving out simulated
/// ones; see [`power_curves`] for `mass` and `per_kg`.
pub fn best_efforts<'a>(
    rides: impl IntoIterator<Item = &'a HistoryEntry>,
    mass: Kilograms,
    per_kg: bool,
) -> Vec<BestEffort> {
    let mut best: Vec<Option<BestEffort>> = vec![None; CURVE_DURATIONS.len()];
    for entry in rides {
        let record = &entry.record;
        let Some(analysis) = record.analysis.as_ref().filter(|_| !record.simulated) else {
            continue;
        };
        let mass = record.rider_mass.unwrap_or(mass);
        for &(duration, power) in &analysis.power_curve.0 {
            let Some(slot) = CURVE_DURATIONS
                .iter()
                .position(|d| *d == duration)
                .and_then(|i| best.get_mut(i))
            else {
                continue;
            };
            let effort = BestEffort {
                duration,
                power,
                watts_per_kg: if mass.0 > 0.0 { power.0 / mass.0 } else { 0.0 },
                fit: entry.fit.clone(),
                start: record.start,
            };
            let value = |e: &BestEffort| if per_kg { e.watts_per_kg } else { e.power.0 };
            if slot
                .as_ref()
                .is_none_or(|held| value(&effort) > value(held))
            {
                *slot = Some(effort);
            }
        }
    }
    best.into_iter().flatten().collect()
}

/// The rider's best power over each of the [`RECORD_DURATIONS`] among `rides`, leaving out
/// simulated ones.
pub(crate) fn record_powers<'a>(
    rides: impl IntoIterator<Item = &'a RideRecord>,
) -> [Option<Watts>; RECORD_DURATIONS.len()] {
    let mut best = [None; RECORD_DURATIONS.len()];
    for record in rides.into_iter().filter(|r| !r.simulated) {
        let Some(analysis) = &record.analysis else {
            continue;
        };
        for (held, duration) in best.iter_mut().zip(RECORD_DURATIONS) {
            if let Some(power) = analysis.power_curve.at(duration)
                && held.is_none_or(|h: Watts| power.0 > h.0)
            {
                *held = Some(power);
            }
        }
    }
    best
}

/// The [`RECORD_DURATIONS`] over which `record` holds the rider's best power, `records` as
/// [`record_powers`] found them.
pub(crate) fn power_records(
    record: &RideRecord,
    records: &[Option<Watts>; RECORD_DURATIONS.len()],
) -> Vec<Duration> {
    let Some(analysis) = record.analysis.as_ref().filter(|_| !record.simulated) else {
        return Vec::new();
    };
    RECORD_DURATIONS
        .into_iter()
        .zip(records)
        .filter(|(duration, best)| best.is_some() && analysis.power_curve.at(*duration) == **best)
        .map(|(duration, _)| duration)
        .collect()
}

#[cfg(test)]
mod tests {
    use torqa_domain::recording::{PowerCurve, RideAnalysis, RideSummary};

    use super::*;

    const DAY: Duration = Duration::from_hours(24);

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + DAY * 20_000
    }

    /// A ride `days_ago` with the given best powers per duration in seconds.
    fn ride(days_ago: u32, curve: &[(u64, f64)], mass: Option<f64>) -> HistoryEntry {
        HistoryEntry {
            fit: PathBuf::from(format!("{days_ago}.fit")),
            record: RideRecord {
                route: "Gurten".to_owned(),
                start: now() - DAY * days_ago,
                summary: RideSummary::default(),
                route_key: None,
                route_time: None,
                climbs: Vec::new(),
                name: None,
                ftp_estimate: None,
                rider_mass: mass.map(Kilograms),
                simulated: false,
                uploads: std::collections::BTreeMap::new(),
                analysis: Some(RideAnalysis {
                    power_curve: PowerCurve(
                        curve
                            .iter()
                            .map(|&(s, w)| (Duration::from_secs(s), Watts(w)))
                            .collect(),
                    ),
                    ..RideAnalysis::default()
                }),
            },
            route_record: false,
            climb_records: Vec::new(),
            power_records: Vec::new(),
        }
    }

    fn powers(efforts: &[BestEffort]) -> Vec<(u64, f64)> {
        efforts
            .iter()
            .map(|e| (e.duration.as_secs(), e.power.0))
            .collect()
    }

    #[test]
    fn each_duration_keeps_its_own_best_ride() {
        let history = [
            ride(2, &[(5, 700.0), (300, 280.0)], None),
            ride(9, &[(5, 820.0), (300, 260.0), (1200, 240.0)], None),
        ];

        let curves = power_curves(
            &history,
            None,
            Comparison::AllTime,
            now(),
            Kilograms(70.0),
            false,
        );

        assert_eq!(
            powers(&curves.period),
            [(5, 820.0), (300, 280.0), (1200, 240.0)]
        );
        assert_eq!(curves.period[0].fit, PathBuf::from("9.fit"));
        assert_eq!(curves.period[1].fit, PathBuf::from("2.fit"));
    }

    #[test]
    fn a_period_is_compared_with_all_rides_or_the_period_before() {
        let history = [
            ride(10, &[(60, 400.0)], None),
            ride(50, &[(60, 450.0)], None),
            ride(100, &[(60, 500.0)], None),
        ];
        let six_weeks = Some(DAY * 42);
        let curves = |comparison| {
            power_curves(
                &history,
                six_weeks,
                comparison,
                now(),
                Kilograms(70.0),
                false,
            )
        };

        let all_time = curves(Comparison::AllTime);
        let previous = curves(Comparison::PreviousPeriod);

        assert_eq!(powers(&all_time.period), [(60, 400.0)]);
        assert_eq!(powers(&all_time.compare), [(60, 500.0)]);
        assert_eq!(
            powers(&previous.compare),
            [(60, 450.0)],
            "42 to 84 days ago"
        );
        let whole = power_curves(
            &history,
            None,
            Comparison::PreviousPeriod,
            now(),
            Kilograms(70.0),
            false,
        );
        assert_eq!(whole.compare, [], "nothing before all time");
    }

    #[test]
    fn per_kilogram_the_lighter_rider_can_hold_the_best() {
        // 300 W at 60 kg beats 320 W at 80 kg; a ride without a mass counts with today's.
        let history = [
            ride(1, &[(1200, 300.0)], Some(60.0)),
            ride(2, &[(1200, 320.0)], Some(80.0)),
            ride(3, &[(1200, 290.0)], None),
        ];
        let best = |per_kg| best_efforts(&history, Kilograms(50.0), per_kg);

        assert_eq!(powers(&best(false)), [(1200, 320.0)]);
        let per_kg = best(true);
        assert_eq!(powers(&per_kg), [(1200, 290.0)]);
        assert!((per_kg[0].watts_per_kg - 5.8).abs() < 1e-9);
    }

    #[test]
    fn the_profile_sets_a_period_against_the_riders_best() {
        // Sprints were better two months ago; the 5 and 20 minutes are this month's.
        let history = [
            ride(3, &[(5, 800.0), (300, 350.0), (1200, 280.0)], None),
            ride(60, &[(5, 1000.0), (300, 330.0)], None),
        ];

        let profile = power_profile(&history, Some(DAY * 42), now(), Kilograms(70.0), false);

        let shares: Vec<(u64, Option<f64>)> = profile
            .iter()
            .map(|axis| (axis.duration.as_secs(), axis.share))
            .collect();
        assert_eq!(
            shares,
            [
                (5, Some(0.8)),
                (60, None),
                (300, Some(1.0)),
                (1200, Some(1.0)),
                (3600, None)
            ]
        );
        assert_eq!(profile[0].best.as_ref().unwrap().power, Watts(1000.0));
        assert_eq!(profile[0].period.as_ref().unwrap().power, Watts(800.0));
    }

    #[test]
    fn a_period_starts_from_the_load_built_up_before_it() {
        // A ride of 100 TSS a day for two weeks, then two days off.
        let today = fitness::day_of(unix_seconds(now()), 0);
        let mut history: Vec<HistoryEntry> = (2..16).map(|ago| ride(ago, &[], None)).collect();
        for entry in &mut history {
            entry.record.summary.training_stress = Some(100.0);
        }

        let all = day_loads(&history, today - 15, today, 0);
        let last_week = day_loads(&history, today - 6, today, 0);

        assert_eq!(all.len(), 16);
        assert_eq!(last_week.len(), 7);
        assert_eq!(
            last_week.first(),
            all.get(9),
            "the same model, only the last days shown"
        );
        assert_eq!(all[0].load.form, 0.0, "rested before the first ride");
        let rested = last_week.last().unwrap().load;
        assert_eq!(rested.stress, 0.0);
        assert!(rested.form > all[13].load.form, "two days off: fresher");
    }

    #[test]
    fn weeks_add_up_their_rides_in_the_riders_zones_today() {
        use std::collections::BTreeMap;
        use torqa_domain::recording::Histogram;

        // Today is a Friday: two rides this week, one last week, none the week before.
        let today = fitness::day_of(unix_seconds(now()), 0);
        assert_eq!(today - fitness::week_start(today), 4, "a Friday");
        let mut history = vec![ride(0, &[], None), ride(1, &[], None), ride(9, &[], None)];
        for entry in &mut history {
            let record = &mut entry.record;
            record.summary.elapsed = Duration::from_hours(1);
            record.summary.distance = Meters(30_000.0);
            record.summary.training_stress = Some(60.0);
            if let Some(analysis) = record.analysis.as_mut() {
                // Half an hour at 130 W, half an hour at 230 W.
                analysis.power_histogram =
                    Histogram(BTreeMap::from([(130, 1800.0), (230, 1800.0)]));
            }
        }
        let zones_at = |ftp: f64| Profile {
            ftp: Watts(ftp),
            ..Profile::default()
        };

        let weeks = weekly(&history, &zones_at(200.0), today - 13, today, 0);

        let rides: Vec<u32> = weeks.iter().map(|week| week.rides).collect();
        assert_eq!(rides, [0, 1, 2]);
        let this_week = &weeks[2];
        assert_eq!(this_week.time, Duration::from_hours(2));
        assert_eq!(this_week.distance, Meters(60_000.0));
        assert!((this_week.stress - 120.0).abs() < 1e-9);
        // 130 W is endurance and 230 W VO2max at an FTP of 200 W (65 % and 115 %) ...
        assert_eq!(this_week.power_zones[1], Duration::from_hours(1));
        assert_eq!(this_week.power_zones[4], Duration::from_hours(1));
        // ... but recovery and threshold at 250 W (52 % and 92 %).
        let fitter = weekly(&history, &zones_at(250.0), today - 13, today, 0);
        assert_eq!(fitter[2].power_zones[0], Duration::from_hours(1));
        assert_eq!(fitter[2].power_zones[3], Duration::from_hours(1));
    }

    #[test]
    fn progress_goes_week_by_week_and_month_by_month_for_long_periods() {
        let today = fitness::day_of(unix_seconds(now()), 0);
        let mut history = vec![
            ride(1, &[(60, 400.0), (300, 300.0)], Some(80.0)),
            ride(3, &[(60, 420.0), (1200, 250.0)], None),
            ride(20, &[(300, 280.0)], None),
            ride(120, &[(1200, 230.0)], None),
        ];
        for (entry, ftp) in history.iter_mut().zip([240.0, 235.0, 230.0, 220.0]) {
            entry.record.summary.ftp = Watts(ftp);
        }
        history[0].record.ftp_estimate = Some(Watts(245.0));

        let weeks = progress(&history, today - 41, today, 0, Kilograms(70.0), false);
        let months = progress(&history, today - 364, today, 0, Kilograms(70.0), false);

        assert!(!weeks.monthly && months.monthly);
        assert_eq!(weeks.points.len(), 7, "six weeks and the one begun");
        let this_week = weeks.points.last().unwrap();
        assert_eq!(this_week.bests, [Some(420.0), Some(300.0), Some(250.0)]);
        assert_eq!(this_week.ftp, Some(240.0), "the last ride's");
        assert_eq!(this_week.test, Some(245.0));
        assert_eq!(months.points.len(), 13, "twelve months and the one begun");
        assert!(
            months
                .points
                .iter()
                .all(|p| fitness::civil_date(p.start).2 == 1)
        );
        let four_months_ago = months.points.iter().find(|p| p.ftp == Some(220.0));
        assert_eq!(four_months_ago.unwrap().bests[2], Some(230.0));

        let per_kg = progress(&history, today - 41, today, 0, Kilograms(70.0), true);
        let this_week = per_kg.points.last().unwrap();
        assert!(
            (this_week.bests[1].unwrap() - 3.75).abs() < 1e-9,
            "300 W at 80 kg"
        );
        assert!(
            (this_week.bests[0].unwrap() - 6.0).abs() < 1e-9,
            "420 W at 70 kg today"
        );
    }

    #[test]
    fn records_belong_to_the_best_ride_and_simulated_rides_hold_none() {
        let mut simulated = ride(1, &[(5, 1500.0), (300, 400.0)], None);
        simulated.record.simulated = true;
        let older = ride(20, &[(5, 900.0), (300, 300.0)], None);
        let newer = ride(2, &[(5, 850.0), (300, 320.0), (1200, 280.0)], None);
        let records = record_powers([&simulated.record, &older.record, &newer.record]);

        assert_eq!(
            records,
            [
                Some(Watts(900.0)),
                None,
                Some(Watts(320.0)),
                Some(Watts(280.0)),
                None
            ]
        );
        assert_eq!(
            power_records(&older.record, &records),
            [Duration::from_secs(5)]
        );
        assert_eq!(
            power_records(&newer.record, &records),
            [Duration::from_mins(5), Duration::from_mins(20)]
        );
        assert_eq!(power_records(&simulated.record, &records), []);
        assert_eq!(
            best_efforts([&simulated], Kilograms(70.0), false),
            [],
            "not the rider's own effort"
        );
    }
}
