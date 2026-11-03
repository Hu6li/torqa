//! Estimated metabolic energy from recorded riding, with power preferred for each second.
//!
//! Power uses 24% gross efficiency. Heart rate uses Keytel et al.'s equation without `VO₂max`;
//! the optional effort fallback uses the 2024 Compendium's stationary-cycling MET values.
//! See `docs/adr/0038-calories.md` for sources, assumptions and the meaning of this estimate.

use orqwa_domain::profile::{CalorieEffort, CalorieProfile, CalorieSex, Profile};
use orqwa_domain::recording::Sample;
use orqwa_domain::units::{Joules, Kilograms};

/// Thermochemical kilocalorie in joules; only presentation converts the accumulated energy.
pub const JOULES_PER_KCAL: f64 = 4_184.0;
const EFFICIENCY: f64 = 0.24;

/// The information used across the estimated seconds of a ride.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Measured power from a trainer or power meter.
    Power,
    /// Heart rate with the rider's optional age, sex and weight.
    HeartRate,
    /// An effort the rider explicitly assumed.
    AssumedEffort,
    /// Different sources were used for different recorded seconds.
    Mixed,
}

/// Estimated energy so far; unavailable intervals contribute nothing and are disclosed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Estimate {
    /// Metabolic energy, absent until at least one recorded second could be estimated.
    pub energy: Option<Joules>,
    /// The source of the estimate, absent when no second could be estimated.
    pub basis: Option<Basis>,
    /// Some recorded seconds could not be estimated; the total covers only the others.
    pub incomplete: bool,
}

/// Incremental calorie estimate, refreshed only for new samples or changed rider inputs.
#[derive(Debug, Default)]
pub struct Calories {
    settings: Option<(CalorieProfile, Kilograms, u16)>,
    sampled: usize,
    estimate: Estimate,
}

impl Calories {
    /// Adds new one-second ride samples. Changed calorie inputs recompute the ride so far;
    /// `year` is the local calendar year and also invalidates the cache when it changes.
    /// Pauses add no samples, and timestamp gaps never invent riding.
    pub fn update(&mut self, samples: &[Sample], profile: &Profile, year: u16) {
        let settings = (profile.calories, profile.rider_mass, year);
        if self.settings != Some(settings) || samples.len() < self.sampled {
            *self = Self {
                settings: Some(settings),
                ..Self::default()
            };
        }
        for sample in &samples[self.sampled..] {
            if let Some((energy, basis)) = energy(sample, profile, year) {
                self.estimate.energy = Some(Joules(
                    self.estimate.energy.unwrap_or_default().0 + energy.0,
                ));
                self.estimate.basis = Some(match self.estimate.basis {
                    None => basis,
                    Some(previous) if previous == basis => basis,
                    Some(_) => Basis::Mixed,
                });
            } else {
                self.estimate.incomplete = true;
            }
        }
        self.sampled = samples.len();
    }

    /// The cached estimate; reading it never scans the ride's samples.
    #[must_use]
    pub const fn estimate(&self) -> Estimate {
        self.estimate
    }
}

fn energy(sample: &Sample, profile: &Profile, year: u16) -> Option<(Joules, Basis)> {
    // Zero power is a measurement too: coasting must not fall back to elevated heart rate.
    if let Some(power) = sample.power.filter(|p| p.0.is_finite() && p.0 >= 0.0) {
        return Some((Joules(power.0 / EFFICIENCY), Basis::Power));
    }
    let weight = profile.rider_mass.0;
    if !weight.is_finite() || !(30.0..=200.0).contains(&weight) {
        return None;
    }
    if let Some(heart_rate) = sample
        .heart_rate
        .filter(|h| h.0.is_finite() && (30.0..=250.0).contains(&h.0))
        && let Some(age) = profile
            .calories
            .age(year)
            .filter(|age| (18..=100).contains(age))
        && let Some(sex) = profile.calories.sex
    {
        // Published regression gives kJ/min, not kcal/min. No resting allowance is added.
        let kj_per_min = match sex {
            CalorieSex::Male => {
                -55.0969 + 0.6309 * heart_rate.0 + 0.1988 * weight + 0.2017 * f64::from(age)
            }
            CalorieSex::Female => {
                -20.4022 + 0.4472 * heart_rate.0 - 0.1263 * weight + 0.074 * f64::from(age)
            }
        };
        return Some((
            Joules(kj_per_min.max(0.0) * 1_000.0 / 60.0),
            Basis::HeartRate,
        ));
    }
    let met = match profile.calories.effort? {
        CalorieEffort::Easy => 3.5,
        CalorieEffort::Moderate => 6.8,
        CalorieEffort::Hard => 10.8,
    };
    Some((
        Joules(met * weight * JOULES_PER_KCAL / 3_600.0),
        Basis::AssumedEffort,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_domain::units::{BeatsPerMinute, Meters, MetersPerSecond, Watts};
    use std::time::Duration;

    fn sample(power: Option<f64>, heart: Option<f64>) -> Sample {
        Sample {
            elapsed: Duration::ZERO,
            location: None,
            distance: Meters(0.0),
            speed: MetersPerSecond(0.0),
            power: power.map(Watts),
            cadence: None,
            heart_rate: heart.map(BeatsPerMinute),
        }
    }

    fn rider() -> Profile {
        Profile {
            calories: CalorieProfile {
                birth_year: Some(1986),
                sex: Some(CalorieSex::Male),
                effort: Some(CalorieEffort::Moderate),
            },
            ..Profile::default()
        }
    }

    fn estimate(samples: &[Sample], profile: &Profile) -> Estimate {
        let mut calories = Calories::default();
        calories.update(samples, profile, 2026);
        calories.estimate()
    }

    #[test]
    fn an_hour_at_200_watts_uses_power_and_keeps_work_distinct() {
        let samples = vec![sample(Some(200.0), Some(180.0)); 3_600];
        let estimate = estimate(&samples, &rider());
        assert!((estimate.energy.unwrap().0 - 3_000_000.0).abs() < 0.001);
        assert_eq!(estimate.basis, Some(Basis::Power));
        assert!(!estimate.incomplete);
        let work = crate::analysis::summarize(&samples, Watts(200.0)).work;
        assert_eq!(work, Some(Joules(720_000.0)));
    }

    #[test]
    fn heart_rate_uses_the_chosen_equation_and_requires_optional_inputs() {
        let samples = vec![sample(None, Some(150.0)); 60];
        let mut profile = rider();
        let male = estimate(&samples, &profile);
        assert!((male.energy.unwrap().0 - 62_516.1).abs() < 0.001);
        assert_eq!(male.basis, Some(Basis::HeartRate));
        profile.calories.sex = Some(CalorieSex::Female);
        assert!((estimate(&samples, &profile).energy.unwrap().0 - 40_165.3).abs() < 0.001);
        profile.calories = CalorieProfile::default();
        assert_eq!(estimate(&samples, &profile).energy, None);
        profile.calories.sex = Some(CalorieSex::Female);
        assert_eq!(
            estimate(&samples, &profile).energy,
            None,
            "age is required too"
        );
    }

    #[test]
    fn no_sensors_need_explicit_effort_and_never_use_virtual_speed_or_ftp() {
        let samples = vec![sample(None, None); 3_600];
        let mut profile = Profile::default();
        assert_eq!(estimate(&samples, &profile).energy, None);
        for (effort, kcal) in [
            (CalorieEffort::Easy, 262.5),
            (CalorieEffort::Moderate, 510.0),
            (CalorieEffort::Hard, 810.0),
        ] {
            profile.calories.effort = Some(effort);
            let total = estimate(&samples, &profile);
            assert!((total.energy.unwrap().0 / JOULES_PER_KCAL - kcal).abs() < 0.001);
            assert_eq!(total.basis, Some(Basis::AssumedEffort));
        }
    }

    #[test]
    fn zero_power_wins_and_sources_are_chosen_per_second_without_double_counting() {
        let mut calories = Calories::default();
        let profile = rider();
        let mut samples = vec![sample(Some(0.0), Some(150.0))];
        calories.update(&samples, &profile, 2026);
        assert_eq!(calories.estimate().energy, Some(Joules(0.0)));
        samples.push(sample(Some(240.0), Some(150.0)));
        calories.update(&samples, &profile, 2026);
        assert_eq!(calories.estimate().energy, Some(Joules(1_000.0)));
        samples.push(sample(None, Some(150.0)));
        samples.push(sample(None, None));
        calories.update(&samples, &profile, 2026);
        let total = calories.estimate();
        assert_eq!(total.basis, Some(Basis::Mixed));
        calories.update(&samples, &profile, 2026);
        assert_eq!(
            calories.estimate(),
            total,
            "paused or reread: no extra calories"
        );
        samples.push(Sample {
            elapsed: Duration::from_secs(3_600),
            ..sample(Some(240.0), None)
        });
        calories.update(&samples, &profile, 2026);
        assert!(
            (calories.estimate().energy.unwrap().0 - total.energy.unwrap().0 - 1_000.0).abs()
                < 0.001
        );
    }

    #[test]
    fn missing_intervals_are_disclosed_and_profile_edits_recompute_without_accumulating() {
        let samples = [sample(Some(240.0), None), sample(None, Some(150.0))];
        let mut calories = Calories::default();
        let mut profile = Profile::default();
        calories.update(&samples, &profile, 2026);
        assert_eq!(calories.estimate().energy, Some(Joules(1_000.0)));
        assert!(calories.estimate().incomplete);
        profile.calories = rider().calories;
        calories.update(&samples, &profile, 2026);
        assert!(!calories.estimate().incomplete);
        assert!((calories.estimate().energy.unwrap().0 - 2_041.935).abs() < 0.001);
    }

    #[test]
    fn new_year_refreshes_the_cached_heart_rate_estimate_without_new_samples() {
        let mut profile = rider();
        profile.calories.effort = None;
        let samples = vec![sample(None, Some(150.0)); 60];
        let mut calories = Calories::default();
        calories.update(&samples, &profile, 2026);
        let before = calories.estimate();
        calories.update(&samples, &profile, 2027);
        let after = calories.estimate();
        assert_eq!(after.basis, Some(Basis::HeartRate));
        assert!((after.energy.unwrap().0 - before.energy.unwrap().0 - 201.7).abs() < 0.001);
        calories.update(&samples, &profile, 2027);
        assert_eq!(calories.estimate(), after, "a year is applied only once");
        calories.update(&samples, &profile, 2026);
        assert_eq!(
            calories.estimate(),
            before,
            "clock rollback derives the original age"
        );
    }

    #[test]
    fn invalid_inputs_do_not_create_nan_or_negative_energy() {
        let samples = [sample(Some(f64::NAN), Some(f64::INFINITY))];
        assert_eq!(estimate(&samples, &Profile::default()).energy, None);
        let mut profile = rider();
        profile.rider_mass = Kilograms(f64::NAN);
        assert_eq!(estimate(&samples, &profile).energy, None);
        profile = rider();
        assert_eq!(
            estimate(&[sample(None, Some(30.0))], &profile).energy,
            Some(Joules(0.0))
        );
        profile.calories.birth_year = Some(2014);
        profile.calories.effort = None;
        assert_eq!(
            estimate(&[sample(None, Some(150.0))], &profile).energy,
            None
        );
        profile.calories.birth_year = Some(1925);
        assert_eq!(
            estimate(&[sample(None, Some(150.0))], &profile).energy,
            None
        );
    }
}
