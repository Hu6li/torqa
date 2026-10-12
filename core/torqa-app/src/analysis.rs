//! The rider's power across rides (R62): the best power held over every duration among the
//! rides of a period, compared with all rides or the period before, and the records it sets;
//! the power profile of a period against the rider's best (R63).

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use torqa_domain::recording::{CURVE_DURATIONS, RECORD_DURATIONS};
use torqa_domain::units::{Kilograms, Watts};
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
