//! The FTP the rider's best efforts show (R65), offered on the Analysis tab and never applied by
//! itself: the critical power of Morton's three-parameter model fitted to the best efforts of
//! 2 to 20 minutes that look maximal, or 95 % of the best 20 minutes where the model does not
//! fit.

use std::time::Duration;

use torqa_domain::units::{Joules, Watts};

/// The shortest effort the model is fitted to: shorter ones are anaerobic, not governed by
/// critical power.
const SHORTEST: Duration = Duration::from_mins(2);
/// The longest effort the model is fitted to, and the one the fallback takes.
const LONGEST: Duration = Duration::from_mins(20);
/// Maximal efforts of different durations the fit needs at least, one per parameter.
const ENOUGH_EFFORTS: usize = 3;
/// Share of the best 20 minutes taken as FTP without a model (Allen and Coggan).
const TWENTY_MINUTE_SHARE: f64 = 0.95;
/// What the model may find for a person on a bike; outside it, the efforts were not maximal.
const CRITICAL_POWER_W: (f64, f64) = (50.0, 600.0);
const W_PRIME_J: (f64, f64) = (3_000.0, 40_000.0);

/// How an estimate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The critical power of Morton's three-parameter model.
    CriticalPower,
    /// 95 % of the best 20 minutes.
    TwentyMinutes,
}

/// The FTP the best efforts show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FtpEstimate {
    /// The estimate, to the watt.
    pub ftp: Watts,
    /// How it was found.
    pub method: Method,
    /// The work the rider can do above critical power (W′), with the model.
    pub w_prime: Option<Joules>,
    /// The power the model gives for an instant, with the model.
    pub max_power: Option<Watts>,
}

/// The FTP shown by `efforts`, the best average power over each duration, e.g. a power curve of
/// the last 90 days. Morton's model `P(t) = CP + W′ / (t − k)` (with `k < 0`, so power stays
/// finite at the start) is fitted to the efforts of 2 to 20 minutes that look maximal by least
/// squares: for each `k` on a fine grid, `CP` and `W′` follow linearly, and the `k` fitting best
/// is kept. `None` without an effort of 20 minutes.
#[must_use]
pub fn estimate_ftp(efforts: &[(Duration, Watts)]) -> Option<FtpEstimate> {
    let twenty = efforts
        .iter()
        .find(|(d, _)| *d == LONGEST)
        .map(|(_, p)| *p)?;
    let mut points: Vec<(f64, f64)> = efforts
        .iter()
        .filter(|(d, _)| (SHORTEST..=LONGEST).contains(d))
        .map(|(d, p)| (d.as_secs_f64(), p.0))
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    critical_power(&maximal(&points))
        .filter(|fit| (CRITICAL_POWER_W.0..=CRITICAL_POWER_W.1).contains(&fit.critical_power))
        .filter(|fit| (W_PRIME_J.0..=W_PRIME_J.1).contains(&fit.w_prime))
        // Critical power is held longer than 20 minutes, so it lies under their power.
        .filter(|fit| fit.critical_power < twenty.0)
        .map(|fit| FtpEstimate {
            ftp: Watts(fit.critical_power.round()),
            method: Method::CriticalPower,
            w_prime: Some(Joules(fit.w_prime)),
            max_power: Some(Watts(fit.critical_power - fit.w_prime / fit.k)),
        })
        .or(Some(FtpEstimate {
            ftp: Watts((twenty.0 * TWENTY_MINUTE_SHARE).round()),
            method: Method::TwentyMinutes,
            w_prime: None,
            max_power: None,
        }))
}

/// The efforts of `points` (`(seconds, watts)`, shortest first) that may have been all-out: those
/// on the upper hull of work over time. Work only adds up as an effort lasts, so an effort under
/// the line between a shorter and a longer one was held below what the rider showed they could,
/// e.g. a best 10 minutes from a ride that never went hard for 10 minutes.
fn maximal(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut hull: Vec<(f64, f64)> = Vec::new();
    for &(t, p) in points {
        while let [.., (t1, p1), (t2, p2)] = hull[..] {
            let (w1, w2, w) = (p1 * t1, p2 * t2, p * t);
            // The middle effort lies on or under the line from the one before to this one.
            if (t2 - t1) * (w - w1) - (w2 - w1) * (t - t1) >= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push((t, p));
    }
    hull
}

/// Morton's model fitted to `(seconds, watts)` points.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    critical_power: f64,
    w_prime: f64,
    k: f64,
}

fn critical_power(points: &[(f64, f64)]) -> Option<Fit> {
    if points.len() < ENOUGH_EFFORTS {
        return None;
    }
    // k from −0.5 s to −300 s in half seconds: wide enough for any rider's maximal power.
    (1..=600)
        .map(|step| -0.5 * f64::from(step))
        .filter_map(|k| {
            let (critical_power, w_prime) = least_squares(points, k)?;
            let error: f64 = points
                .iter()
                .map(|&(t, p)| (critical_power + w_prime / (t - k) - p).powi(2))
                .sum();
            Some((
                error,
                Fit {
                    critical_power,
                    w_prime,
                    k,
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, fit)| fit)
}

/// `CP` and `W′` fitting `P = CP + W′ · x` with `x = 1 / (t − k)`, a straight line in `x`.
fn least_squares(points: &[(f64, f64)], k: f64) -> Option<(f64, f64)> {
    #[allow(clippy::cast_precision_loss)] // a handful of points
    let n = points.len() as f64;
    let xs: Vec<f64> = points.iter().map(|&(t, _)| 1.0 / (t - k)).collect();
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_p = points.iter().map(|&(_, p)| p).sum::<f64>() / n;
    let spread: f64 = xs.iter().map(|x| (x - mean_x).powi(2)).sum();
    if spread <= 0.0 {
        return None;
    }
    let w_prime = xs
        .iter()
        .zip(points)
        .map(|(x, &(_, p))| (x - mean_x) * (p - mean_p))
        .sum::<f64>()
        / spread;
    Some((mean_p - w_prime * mean_x, w_prime))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Best efforts of a rider following the model exactly at the curve's durations.
    fn efforts(critical_power: f64, w_prime: f64, max_power: f64) -> Vec<(Duration, Watts)> {
        let k = w_prime / (critical_power - max_power);
        [
            5, 30, 60, 120, 180, 240, 300, 480, 600, 900, 1200, 1800, 3600,
        ]
        .into_iter()
        .map(|s: u32| {
            let power = critical_power + w_prime / (f64::from(s) - k);
            (Duration::from_secs(u64::from(s)), Watts(power))
        })
        .collect()
    }

    #[test]
    fn the_model_finds_the_critical_power_of_a_rider_following_it() {
        let estimate = estimate_ftp(&efforts(250.0, 20_000.0, 1000.0)).unwrap();

        assert_eq!(estimate.method, Method::CriticalPower);
        assert!((estimate.ftp.0 - 250.0).abs() <= 5.0, "{estimate:?}");
        let w_prime = estimate.w_prime.unwrap().0;
        assert!((w_prime - 20_000.0).abs() < 1_000.0, "{estimate:?}");
    }

    #[test]
    fn efforts_that_were_not_all_out_do_not_pull_the_estimate_down() {
        // The same rider, who never went hard for 6 to 15 minutes: those bests are 15 % low.
        let efforts: Vec<(Duration, Watts)> = efforts(250.0, 20_000.0, 1000.0)
            .into_iter()
            .map(|(d, p)| {
                let easy = (360..=900).contains(&d.as_secs());
                (d, Watts(if easy { p.0 * 0.85 } else { p.0 }))
            })
            .collect();

        let estimate = estimate_ftp(&efforts).unwrap();

        assert_eq!(estimate.method, Method::CriticalPower);
        assert!((estimate.ftp.0 - 250.0).abs() <= 8.0, "{estimate:?}");
    }

    #[test]
    fn efforts_the_model_cannot_explain_fall_back_to_the_best_20_minutes() {
        // As hard for 2 minutes as for 20: no anaerobic capacity, so no model.
        let flat: Vec<(Duration, Watts)> = [120, 180, 300, 600, 1200]
            .into_iter()
            .map(|s| (Duration::from_secs(s), Watts(260.0)))
            .collect();

        let estimate = estimate_ftp(&flat).unwrap();

        assert_eq!(estimate.method, Method::TwentyMinutes);
        assert_eq!(estimate.ftp, Watts(247.0), "95 % of 260 W");
        assert_eq!(estimate.w_prime, None);
    }

    #[test]
    fn too_few_efforts_fall_back_and_none_of_20_minutes_give_nothing() {
        let few = [
            (Duration::from_mins(5), Watts(300.0)),
            (Duration::from_mins(20), Watts(250.0)),
        ];
        let short = [(Duration::from_mins(5), Watts(300.0))];

        assert_eq!(estimate_ftp(&few).unwrap().method, Method::TwentyMinutes);
        assert_eq!(estimate_ftp(&short), None);
        assert_eq!(estimate_ftp(&[]), None);
    }
}
