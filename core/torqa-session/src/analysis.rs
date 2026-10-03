//! Ride analysis (R31): summary figures and time in training zones, computed from the 1 Hz
//! samples of a recorded ride.

use std::time::Duration;

use torqa_domain::profile::Profile;
use torqa_domain::recording::{RideSummary, Sample};
use torqa_domain::units::{BeatsPerMinute, Joules, Meters, MetersPerSecond, Rpm, Watts};

/// Normalized power smooths power over this many samples (30 s at 1 Hz).
const NP_WINDOW: usize = 30;
/// Each sample stands for this much time.
const SAMPLE_TIME: Duration = Duration::from_secs(1);

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
                .map(|w| (w[1].elevation.0 - w[0].elevation.0).max(0.0))
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
                    lat: 46.0,
                    lon: 7.0,
                    elevation: Meters(500.0 + at),
                    distance: Meters(8.0 * at),
                    speed: MetersPerSecond(8.0),
                    grade: GradePercent(1.0),
                    power: power.map(Watts),
                    cadence: Some(Rpm(90.0)),
                    heart_rate: Some(BeatsPerMinute(140.0)),
                }
            })
            .collect()
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
