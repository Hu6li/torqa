//! Ride analysis (R31): summary figures, time in training zones and the power curve (R62),
//! computed from the 1 Hz samples of a recorded ride.

use std::ops::Range;
use std::time::Duration;

use torqa_domain::profile::Profile;
use torqa_domain::recording::{CURVE_DURATIONS, PowerCurve, RideAnalysis, RideSummary, Sample};
use torqa_domain::units::{BeatsPerMinute, Joules, Meters, MetersPerSecond, Rpm, Watts};

/// Normalized power smooths power over this many samples (30 s at 1 Hz).
const NP_WINDOW: usize = 30;
/// Each sample stands for this much time.
const SAMPLE_TIME: Duration = Duration::from_secs(1);
/// Gaps between samples up to this many seconds are bridged with the power before them, as a
/// device recording only changes leaves them; longer ones are pauses no effort spans.
const MAX_HOLD_S: u64 = 10;

/// Summarises a ride; intensity and training stress are relative to `ftp`.
#[must_use]
pub fn summarize(samples: &[Sample], ftp: Watts) -> RideSummary {
    let Some(last) = samples.last() else {
        return RideSummary {
            ftp,
            ..RideSummary::default()
        };
    };
    let elapsed = last.elapsed;
    let seconds = elapsed.as_secs_f64();
    let powers: Vec<f64> = samples
        .iter()
        .filter_map(|s| s.power.map(|p| p.0))
        .collect();
    // Cadence averages conventionally leave out coasting.
    let cadences: Vec<f64> = samples
        .iter()
        .filter_map(|s| s.cadence.map(|c| c.0))
        .filter(|&c| c > 0.0)
        .collect();
    let heart_rates: Vec<f64> = samples
        .iter()
        .filter_map(|s| s.heart_rate.map(|h| h.0))
        .collect();
    let normalized_power = normalized_power(samples);
    let intensity_factor = normalized_power
        .filter(|_| ftp.0 > 0.0)
        .map(|np| np.0 / ftp.0);
    let training_stress = normalized_power
        .zip(intensity_factor)
        .map(|(np, intensity)| seconds * np.0 * intensity / (ftp.0 * 3600.0) * 100.0);
    RideSummary {
        elapsed,
        distance: last.distance,
        elevation_gain: Meters(
            samples
                .windows(2)
                .filter_map(|w| Some((w[0].location?, w[1].location?)))
                .map(|(a, b)| (b.elevation.0 - a.elevation.0).max(0.0))
                .sum(),
        ),
        avg_speed: MetersPerSecond(if seconds > 0.0 {
            last.distance.0 / seconds
        } else {
            0.0
        }),
        max_speed: MetersPerSecond(samples.iter().map(|s| s.speed.0).fold(0.0, f64::max)),
        avg_power: average(&powers).map(Watts),
        max_power: maximum(&powers).map(Watts),
        normalized_power,
        intensity_factor,
        training_stress,
        work: (!powers.is_empty())
            .then(|| Joules(powers.iter().sum::<f64>() * SAMPLE_TIME.as_secs_f64())),
        avg_cadence: average(&cadences).map(Rpm),
        avg_heart_rate: average(&heart_rates).map(BeatsPerMinute),
        max_heart_rate: maximum(&heart_rates).map(BeatsPerMinute),
        ftp,
    }
}

/// The best average power over `window` (whole seconds of 1 Hz samples), e.g. the best minute;
/// seconds without power count as 0 W. `None` if the ride is shorter.
#[must_use]
pub fn best_average_power(samples: &[Sample], window: Duration) -> Option<Watts> {
    let length = usize::try_from(window.as_secs()).ok()?.max(1);
    let powers: Vec<f64> = samples
        .iter()
        .map(|s| s.power.map_or(0.0, |p| p.0))
        .collect();
    best_mean(&powers, length).map(Watts)
}

/// What the analysis across rides keeps of a ride (R62).
#[must_use]
pub fn analyse(samples: &[Sample]) -> RideAnalysis {
    RideAnalysis {
        power_curve: power_curve(samples),
    }
}

/// The ride's best average power over each of the [`CURVE_DURATIONS`] it lasted (R62). Samples
/// are put on a 1 s grid first, so activities recorded at another rate or only on changes give
/// the same curve; no effort spans a pause. Empty for a ride without power.
#[must_use]
pub fn power_curve(samples: &[Sample]) -> PowerCurve {
    if samples.iter().all(|s| s.power.is_none()) {
        return PowerCurve::default();
    }
    let stretches = power_stretches(samples);
    PowerCurve(
        CURVE_DURATIONS
            .iter()
            .filter_map(|&duration| {
                let window = usize::try_from(duration.as_secs()).ok()?;
                stretches
                    .iter()
                    .filter_map(|stretch| best_mean(stretch, window))
                    .reduce(f64::max)
                    // A tenth of a watt is finer than power meters measure, and keeps the
                    // stored curve short and the same as the one computed.
                    .map(|best| (duration, Watts((best * 10.0).round() / 10.0)))
            })
            .collect(),
    )
}

/// The ride's power once per second, in stretches between pauses: samples within the same
/// second are averaged, short gaps hold the power before them, and seconds without power count
/// as 0 W.
fn power_stretches(samples: &[Sample]) -> Vec<Vec<f64>> {
    let mut stretches = Vec::new();
    let mut stretch: Vec<f64> = Vec::new();
    // The second being filled: when, the sum of its powers and how many there were.
    let mut second: Option<(u64, f64, u32)> = None;
    for sample in samples {
        let at = sample.elapsed.as_secs();
        let power = sample.power.map_or(0.0, |p| p.0);
        if let Some((current, sum, count)) = &mut second
            && at <= *current
        {
            *sum += power;
            *count += 1;
            continue;
        }
        if let Some((current, sum, count)) = second {
            let average = sum / f64::from(count);
            stretch.push(average);
            let gap = at - current;
            if gap > MAX_HOLD_S {
                stretches.push(std::mem::take(&mut stretch));
            } else {
                let held = usize::try_from(gap - 1).unwrap_or_default();
                stretch.extend(std::iter::repeat_n(average, held));
            }
        }
        second = Some((at, power, 1));
    }
    if let Some((_, sum, count)) = second {
        stretch.push(sum / f64::from(count));
        stretches.push(stretch);
    }
    stretches
}

/// The highest mean of `window` consecutive values, as a running sum; `None` if there are
/// fewer.
fn best_mean(values: &[f64], window: usize) -> Option<f64> {
    if window == 0 || values.len() < window {
        return None;
    }
    let mut sum: f64 = values[..window].iter().sum();
    let mut best = sum;
    for (entering, leaving) in values[window..].iter().zip(values) {
        sum += entering - leaving;
        best = best.max(sum);
    }
    #[allow(clippy::cast_precision_loss)] // a window of seconds
    Some(best / window as f64)
}

/// Share of the best minute of a ramp test that the rider can hold for an hour: the usual
/// estimate of FTP from such a test.
const RAMP_TEST_SHARE: f64 = 0.75;

/// The FTP a ramp test (R22) shows: 75 % of the rider's best minute in it. `None` if it lasted
/// less than a minute.
#[must_use]
pub fn ramp_test_ftp(samples: &[Sample]) -> Option<Watts> {
    best_average_power(samples, Duration::from_mins(1))
        .map(|best| Watts((best.0 * RAMP_TEST_SHARE).round()))
}

/// The FTP an FTP test of all-out efforts (R22, #125) shows: `share` of the efforts' average
/// power, each effort a stretch of time from the start of the ride (seconds without power
/// count as 0 W). `None` unless the ride lasted through the last effort.
#[must_use]
pub fn effort_test_ftp(
    samples: &[Sample],
    efforts: &[Range<Duration>],
    share: f64,
) -> Option<Watts> {
    let last = samples.last()?.elapsed;
    let mut averages = Vec::with_capacity(efforts.len());
    for effort in efforts {
        if last + SAMPLE_TIME < effort.end {
            return None;
        }
        let powers: Vec<f64> = samples
            .iter()
            .filter(|s| effort.contains(&s.elapsed))
            .map(|s| s.power.map_or(0.0, |p| p.0))
            .collect();
        #[allow(clippy::cast_precision_loss)] // seconds of a test
        let count = powers.len() as f64;
        averages.push(powers.iter().sum::<f64>() / count);
    }
    if averages.is_empty() || averages.iter().any(|a| a.is_nan()) {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // one or two efforts
    let count = averages.len() as f64;
    Some(Watts(
        (averages.iter().sum::<f64>() / count * share).round(),
    ))
}

/// The rider's time and power over one stretch of the route, e.g. a climb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effort {
    /// Time from `start` to `end`.
    pub elapsed: Duration,
    /// Average power over the stretch, if power was recorded.
    pub avg_power: Option<Watts>,
}

/// When the rider passed `distance`, interpolated between samples; `None` if not reached.
#[must_use]
pub fn time_at(samples: &[Sample], distance: Meters) -> Option<Duration> {
    let after = samples.iter().position(|s| s.distance.0 >= distance.0)?;
    if after == 0 {
        return Some(samples[0].elapsed);
    }
    let (a, b) = (&samples[after - 1], &samples[after]);
    let span = b.distance.0 - a.distance.0;
    let fraction = if span > 0.0 {
        (distance.0 - a.distance.0) / span
    } else {
        0.0
    };
    Some(a.elapsed + b.elapsed.saturating_sub(a.elapsed).mul_f64(fraction))
}

/// The effort between two distances along the route, if the rider covered all of it.
#[must_use]
pub fn effort(samples: &[Sample], start: Meters, end: Meters) -> Option<Effort> {
    let started = time_at(samples, start)?;
    let ended = time_at(samples, end)?;
    let powers: Vec<f64> = samples
        .iter()
        .filter(|s| s.elapsed >= started && s.elapsed <= ended)
        .filter_map(|s| s.power.map(|p| p.0))
        .collect();
    Some(Effort {
        elapsed: ended.saturating_sub(started),
        avg_power: average(&powers).map(Watts),
    })
}

/// Time spent in each of the rider's seven power zones. Each interval between samples counts
/// for the zone at its end, so the zones add up to the ride's duration where power was known.
#[must_use]
pub fn time_in_power_zones(samples: &[Sample], profile: &Profile) -> [Duration; 7] {
    time_in_zones(samples, |s| s.power.map(|p| profile.power_zone(p)))
}

/// Time spent in each of the rider's five heart-rate zones, counted like the power zones.
#[must_use]
pub fn time_in_heart_rate_zones(samples: &[Sample], profile: &Profile) -> [Duration; 5] {
    time_in_zones(samples, |s| {
        s.heart_rate.map(|h| profile.heart_rate_zone(h))
    })
}

fn time_in_zones<const N: usize>(
    samples: &[Sample],
    zone: impl Fn(&Sample) -> Option<u8>,
) -> [Duration; N] {
    let mut zones = [Duration::ZERO; N];
    for pair in samples.windows(2) {
        if let Some(z) = zone(&pair[1]) {
            zones[usize::from(z.max(1) - 1).min(N - 1)] +=
                pair[1].elapsed.saturating_sub(pair[0].elapsed);
        }
    }
    zones
}

/// The fourth root of the mean fourth power of the 30 s rolling average power (Coggan).
/// Gaps without power count as zero, like coasting.
fn normalized_power(samples: &[Sample]) -> Option<Watts> {
    if samples.iter().all(|s| s.power.is_none()) || samples.len() < NP_WINDOW {
        return None;
    }
    let powers: Vec<f64> = samples
        .iter()
        .map(|s| s.power.map_or(0.0, |p| p.0))
        .collect();
    #[allow(clippy::cast_precision_loss)] // window and sample counts are small
    let window = NP_WINDOW as f64;
    let fourth_powers: Vec<f64> = powers
        .windows(NP_WINDOW)
        .map(|w| (w.iter().sum::<f64>() / window).powi(4))
        .collect();
    average(&fourth_powers).map(|mean| Watts(mean.powf(0.25)))
}

fn average(values: &[f64]) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)] // sample counts are far below 2^52
    let count = values.len() as f64;
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / count)
}

fn maximum(values: &[f64]) -> Option<f64> {
    values.iter().copied().reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use torqa_domain::recording::Location;
    use torqa_domain::units::GradePercent;

    use super::*;

    /// One sample per second with the given powers, riding 8 m/s and climbing 1 m per sample.
    fn ride(powers: &[Option<f64>]) -> Vec<Sample> {
        powers
            .iter()
            .enumerate()
            .map(|(i, power)| {
                #[allow(clippy::cast_precision_loss)]
                let at = i as f64;
                Sample {
                    elapsed: Duration::from_secs(i as u64),
                    location: Some(Location {
                        lat: 46.0,
                        lon: 7.0,
                        elevation: Meters(500.0 + at),
                        grade: GradePercent(1.0),
                    }),
                    distance: Meters(8.0 * at),
                    speed: MetersPerSecond(8.0),
                    power: power.map(Watts),
                    cadence: Some(Rpm(90.0)),
                    heart_rate: Some(BeatsPerMinute(140.0)),
                }
            })
            .collect()
    }

    /// Samples at the given times in seconds with the given powers.
    fn timed(points: impl IntoIterator<Item = (f64, f64)>) -> Vec<Sample> {
        points
            .into_iter()
            .map(|(at, power)| Sample {
                elapsed: Duration::from_secs_f64(at),
                location: None,
                distance: Meters(8.0 * at),
                speed: MetersPerSecond(8.0),
                power: Some(Watts(power)),
                cadence: None,
                heart_rate: None,
            })
            .collect()
    }

    /// 10 minutes at 200 W, 5 at 400 W, 10 at 200 W: power for each second of 25 minutes.
    fn block_power(second: f64) -> f64 {
        if (600.0..900.0).contains(&second) {
            400.0
        } else {
            200.0
        }
    }

    fn watts_at(curve: &PowerCurve, seconds: u64) -> Option<f64> {
        curve.at(Duration::from_secs(seconds)).map(|p| p.0)
    }

    #[test]
    fn a_steady_ride_holds_its_power_at_every_duration_it_lasted() {
        let curve = power_curve(&ride(&[Some(250.0); 600]));

        // Every duration of the curve from a second to the ride's 10 minutes.
        assert_eq!(curve.0.len(), 19);
        assert!(curve.0.iter().all(|(_, p)| (p.0 - 250.0).abs() < 1e-9));
        assert_eq!(
            curve.0.last().map(|(d, _)| *d),
            Some(Duration::from_mins(10))
        );
        assert_eq!(watts_at(&curve, 720), None, "longer than the ride");
    }

    #[test]
    fn the_curve_holds_the_hardest_stretch_of_each_length() {
        let samples = timed((0..1500).map(|s| (f64::from(s), block_power(f64::from(s)))));

        let curve = power_curve(&samples);

        assert_eq!(watts_at(&curve, 1), Some(400.0));
        assert_eq!(watts_at(&curve, 300), Some(400.0));
        // The hard 5 minutes with 5 easy ones: (300·400 + 300·200) / 600.
        assert_eq!(watts_at(&curve, 600), Some(300.0));
        assert_eq!(watts_at(&curve, 1200), Some(250.0));
        assert_eq!(watts_at(&curve, 1500), Some(240.0));
    }

    #[test]
    fn the_recording_rate_does_not_change_the_curve() {
        let every_second = power_curve(&timed(
            (0..1500).map(|s| (f64::from(s), block_power(f64::from(s)))),
        ));
        let twice_a_second = power_curve(&timed((0..3000).map(|i| {
            let at = f64::from(i) / 2.0;
            (at, block_power(at.floor()))
        })));
        // Recorded every 4 s, as devices recording only changes do on steady efforts.
        let every_four_seconds = power_curve(&timed(
            (0..375).map(|i| (f64::from(i * 4), block_power(f64::from(i * 4)))),
        ));

        assert_eq!(twice_a_second, every_second);
        for (duration, power) in &every_second.0 {
            if duration.as_secs() <= 1200 {
                assert_eq!(
                    every_four_seconds.at(*duration),
                    Some(*power),
                    "{duration:?}"
                );
            }
        }
    }

    #[test]
    fn no_effort_spans_a_pause() {
        // 5 minutes at 300 W, a 10-minute pause, 5 more minutes at 300 W.
        let samples = timed(
            (0..300)
                .map(|s| (f64::from(s), 300.0))
                .chain((900..1200).map(|s| (f64::from(s), 300.0))),
        );

        let curve = power_curve(&samples);

        assert_eq!(watts_at(&curve, 300), Some(300.0));
        assert_eq!(watts_at(&curve, 360), None);
    }

    #[test]
    fn a_ride_without_power_has_no_curve() {
        assert_eq!(power_curve(&ride(&[None; 120])), PowerCurve::default());
        assert_eq!(power_curve(&[]), PowerCurve::default());
    }

    #[test]
    fn a_ramp_tests_ftp_is_three_quarters_of_the_best_minute() {
        // Steps of a minute from 150 W up by 20 W; the last one held for half a minute.
        let mut powers: Vec<Option<f64>> = (0..8)
            .flat_map(|step| std::iter::repeat_n(Some(150.0 + 20.0 * f64::from(step)), 60))
            .collect();
        powers.extend(std::iter::repeat_n(Some(310.0), 30));
        powers.extend(std::iter::repeat_n(None, 20));

        let samples = ride(&powers);

        // The best minute: half at 290 W and half at 310 W.
        let best = best_average_power(&samples, Duration::from_mins(1)).unwrap();
        assert!((best.0 - 300.0).abs() < 1e-9, "{best:?}");
        assert_eq!(ramp_test_ftp(&samples), Some(Watts(225.0)));
        assert_eq!(ramp_test_ftp(&samples[..59]), None, "shorter than a minute");
    }

    #[test]
    fn an_effort_tests_ftp_is_a_share_of_its_efforts_average() {
        // Two efforts of 8 minutes, at 300 and 280 W, between easy riding at 150 W.
        let mut powers: Vec<Option<f64>> = vec![Some(150.0); 60];
        powers.extend(std::iter::repeat_n(Some(300.0), 480));
        powers.extend(std::iter::repeat_n(Some(150.0), 600));
        powers.extend(std::iter::repeat_n(Some(280.0), 480));
        powers.extend(std::iter::repeat_n(Some(150.0), 60));
        let samples = ride(&powers);
        let efforts = [
            Duration::from_mins(1)..Duration::from_mins(9),
            Duration::from_mins(19)..Duration::from_mins(27),
        ];

        assert_eq!(
            effort_test_ftp(&samples, &efforts, 0.9),
            Some(Watts(261.0)),
            "90 % of 290 W"
        );
        assert_eq!(
            effort_test_ftp(&samples, &efforts[..1], 0.95),
            Some(Watts(285.0)),
            "95 % of one effort"
        );
        assert_eq!(
            effort_test_ftp(&samples[..1500], &efforts, 0.9),
            None,
            "left during the second effort"
        );
        assert_eq!(effort_test_ftp(&samples, &[], 0.9), None, "no efforts");
    }

    #[test]
    fn an_hour_at_ftp_scores_100() {
        let samples = ride(&vec![Some(250.0); 3601]);

        let summary = summarize(&samples, Watts(250.0));

        let np = summary.normalized_power.unwrap().0;
        assert!((np - 250.0).abs() < 1e-6, "NP {np}");
        assert!((summary.intensity_factor.unwrap() - 1.0).abs() < 1e-6);
        assert!((summary.training_stress.unwrap() - 100.0).abs() < 0.1);
        // 250 W for 3601 s.
        assert!((summary.work.unwrap().0 - 900_250.0).abs() < 1e-6);
    }

    #[test]
    fn normalized_power_weights_hard_efforts_above_the_average() {
        // Alternating 5 minutes at 100 W and 400 W: average 250 W, but the hard blocks hurt more.
        let powers: Vec<Option<f64>> = (0..3600)
            .map(|s| Some(if (s / 300) % 2 == 0 { 100.0 } else { 400.0 }))
            .collect();

        let summary = summarize(&ride(&powers), Watts(250.0));

        let average = summary.avg_power.unwrap().0;
        let np = summary.normalized_power.unwrap().0;
        assert!((average - 250.0).abs() < 1e-6);
        assert!(np > 300.0 && np < 340.0, "NP {np}");
    }

    #[test]
    fn short_rides_and_rides_without_power_have_no_normalized_power() {
        assert_eq!(
            summarize(&ride(&[Some(200.0); 10]), Watts(250.0)).normalized_power,
            None
        );
        let no_power = summarize(&ride(&[None; 120]), Watts(250.0));
        assert_eq!(no_power.normalized_power, None);
        assert_eq!(no_power.training_stress, None);
        assert_eq!(no_power.work, None);
    }

    #[test]
    fn summarises_distance_climbing_and_averages() {
        let summary = summarize(&ride(&[Some(100.0), Some(300.0), None]), Watts(250.0));

        assert_eq!(summary.elapsed, Duration::from_secs(2));
        assert_eq!(summary.distance, Meters(16.0));
        assert_eq!(summary.elevation_gain, Meters(2.0));
        assert_eq!(summary.avg_speed, MetersPerSecond(8.0));
        assert_eq!(summary.avg_power, Some(Watts(200.0)));
        assert_eq!(summary.max_power, Some(Watts(300.0)));
        assert_eq!(summary.avg_heart_rate, Some(BeatsPerMinute(140.0)));
    }

    #[test]
    fn efforts_interpolate_between_samples() {
        // 8 m per second: 100 m → 12.5 s, 300 m → 37.5 s.
        let samples = ride(&[Some(200.0); 60]);

        let climb = effort(&samples, Meters(100.0), Meters(300.0)).unwrap();

        assert_eq!(climb.elapsed, Duration::from_secs(25));
        assert_eq!(climb.avg_power, Some(Watts(200.0)));
        assert_eq!(time_at(&samples, Meters(0.0)), Some(Duration::ZERO));
        // Not reached: the ride covered 472 m.
        assert_eq!(effort(&samples, Meters(100.0), Meters(500.0)), None);
    }

    #[test]
    fn counts_seconds_per_zone() {
        let rider = Profile {
            ftp: Watts(200.0),
            max_heart_rate: BeatsPerMinute(200.0),
            ..Profile::default()
        };
        let samples = ride(&[Some(100.0), Some(100.0), Some(210.0), None]);

        let power = time_in_power_zones(&samples, &rider);
        let heart = time_in_heart_rate_zones(&samples, &rider);

        // Three one-second intervals ending at 100 W, 210 W and no power.
        assert_eq!(power[0], Duration::from_secs(1));
        assert_eq!(power[3], Duration::from_secs(1));
        assert_eq!(power.iter().sum::<Duration>(), Duration::from_secs(2));
        // 140 of 200 bpm is the top of zone 2, for the whole ride.
        assert_eq!(heart[1], Duration::from_secs(3));
        assert_eq!(heart.iter().sum::<Duration>(), Duration::from_secs(3));
    }
}
