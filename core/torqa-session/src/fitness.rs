//! Fitness, fatigue and form across rides (R64): the training load model of the performance
//! management chart, and the local calendar days and weeks it counts in.

/// Days of training load that fitness averages over (chronic training load).
pub const FITNESS_DAYS: f64 = 42.0;
/// Days of training load that fatigue averages over (acute training load).
pub const FATIGUE_DAYS: f64 = 7.0;

const SECONDS_PER_DAY: i64 = 86_400;

/// Where a day's training leaves the rider.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Load {
    /// Training stress of the day's rides (TSS).
    pub stress: f64,
    /// The exponentially weighted average of daily stress over [`FITNESS_DAYS`].
    pub fitness: f64,
    /// The same over [`FATIGUE_DAYS`].
    pub fatigue: f64,
    /// Fitness minus fatigue at the start of the day, before its rides: rested above zero,
    /// tired below.
    pub form: f64,
}

/// The load after each day of `daily_stress`, one entry per day in order, starting rested and
/// untrained. Each day's stress moves fitness and fatigue towards it by `1 − e^(−1/days)`, as
/// the performance management chart of `TrainingPeaks` and `intervals.icu` does.
#[must_use]
pub fn loads(daily_stress: &[f64]) -> Vec<Load> {
    let fitness_rate = 1.0 - (-1.0 / FITNESS_DAYS).exp();
    let fatigue_rate = 1.0 - (-1.0 / FATIGUE_DAYS).exp();
    let (mut fitness, mut fatigue) = (0.0, 0.0);
    daily_stress
        .iter()
        .map(|&stress| {
            let form = fitness - fatigue;
            fitness += (stress - fitness) * fitness_rate;
            fatigue += (stress - fatigue) * fatigue_rate;
            Load {
                stress,
                fitness,
                fatigue,
                form,
            }
        })
        .collect()
}

/// The local day, counted from 1970-01-01, of the moment `unix_s` seconds after 1970 in UTC,
/// where local time is `utc_offset_s` ahead of UTC.
#[must_use]
pub fn day_of(unix_s: i64, utc_offset_s: i64) -> i64 {
    (unix_s + utc_offset_s).div_euclid(SECONDS_PER_DAY)
}

/// The Monday starting the week of `day` (ISO weeks).
#[must_use]
pub fn week_start(day: i64) -> i64 {
    // 1970-01-01 was a Thursday, three days after a Monday.
    day - (day + 3).rem_euclid(7)
}

/// The first moment of `day` in seconds after 1970 in UTC, local time `utc_offset_s` ahead.
#[must_use]
pub fn day_start(day: i64, utc_offset_s: i64) -> i64 {
    day * SECONDS_PER_DAY - utc_offset_s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn one_ride_tires_at_once_and_builds_fitness_slowly() {
        let loads = loads(&[100.0, 0.0]);

        assert!(close(loads[0].fatigue, 13.3), "{:?}", loads[0]);
        assert!(close(loads[0].fitness, 2.35), "{:?}", loads[0]);
        assert!(close(loads[0].form, 0.0), "rested before the first ride");
        // The next morning: tired, so form is below zero.
        assert!(close(loads[1].form, 2.35 - 13.3), "{:?}", loads[1]);
    }

    #[test]
    fn the_same_load_every_day_settles_fitness_and_fatigue_on_it() {
        let loads = loads(&[100.0; 400]);

        let last = loads.last().unwrap();
        assert!(
            close(last.fitness, 100.0) && close(last.fatigue, 100.0),
            "{last:?}"
        );
        assert!(close(last.form, 0.0));
        // Fatigue follows within a week or two, fitness takes months.
        assert!(
            loads[20].fatigue > 94.0 && loads[20].fitness < 40.0,
            "{:?}",
            loads[20]
        );
    }

    #[test]
    fn days_and_weeks_follow_the_local_calendar() {
        // 2026-10-12 is a Monday; 23:30 UTC is already Tuesday in Zurich (UTC+2).
        let monday = 20_738;
        let late = day_start(monday, 0) + 23 * 3600 + 1800;

        assert_eq!(day_of(late, 0), monday);
        assert_eq!(day_of(late, 2 * 3600), monday + 1);
        assert_eq!(
            day_of(day_start(monday, -5 * 3600), -5 * 3600),
            monday,
            "New York"
        );
        assert_eq!(week_start(monday), monday);
        assert_eq!(week_start(monday + 6), monday, "Sunday");
        assert_eq!(week_start(monday + 7), monday + 7);
        assert_eq!(week_start(0), -3, "1970-01-01 was a Thursday");
        assert_eq!(day_of(-1, 0), -1, "before 1970");
    }
}
