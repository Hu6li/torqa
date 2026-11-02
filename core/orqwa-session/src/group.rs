//! Fixed-size, membership-bound remote motion and individual group assistance (R164, R166).
//! No received measurement is substituted for the local rider's telemetry or ERG target.

use orqwa_domain::units::{GradePercent, Meters, MetersPerSecond, Rpm, Watts};
use orqwa_physics::GroupEffect;
use std::time::{Duration, Instant};

/// The group limit, checked against the transport's limit by the app integration.
pub const MAX_RIDERS: usize = 8;
/// Stop showing or assisting from a rider after two seconds without a fresh sample.
pub const STALE: Duration = Duration::from_secs(2);
const DELAY: Duration = Duration::from_millis(100);
const EXTRAPOLATE: Duration = Duration::from_millis(300);

/// One validated remote measurement. Heart rate deliberately has no representation here.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Elapsed time since the shared start, for rejecting reordered samples.
    pub elapsed: Duration,
    /// Distance along the shared course.
    pub distance: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// That rider's measured power, for their avatar and group display only.
    pub power: Watts,
    /// That rider's measured cadence, for their avatar only.
    pub cadence: Rpm,
}

impl Sample {
    fn valid(self) -> bool {
        [
            (self.distance.0, 10_000_000.0),
            (self.speed.0, 150.0),
            (self.power.0, 5_000.0),
            (self.cadence.0, 300.0),
        ]
        .iter()
        .all(|&(value, max)| value.is_finite() && (0.0..=max).contains(&value))
    }
}

/// A remote rider's smoothed position, with freshness for fading assistance.
#[derive(Clone, Copy, Debug)]
pub struct Rider {
    /// Membership generation; replacing a rider in a slot invalidates the old motion.
    pub generation: u64,
    /// Smoothed route distance.
    pub distance: Meters,
    /// Latest measured virtual speed.
    pub speed: MetersPerSecond,
    /// Latest measured peer power; never an input to local recording or ERG.
    pub power: Watts,
    /// Latest measured peer cadence.
    pub cadence: Rpm,
    /// Age since the newest sample arrived locally.
    pub age: Duration,
}

#[derive(Clone, Copy, Debug)]
struct Received {
    sample: Sample,
    at: Instant,
}
#[derive(Clone, Copy, Debug)]
struct Slot {
    generation: u64,
    previous: Option<Received>,
    latest: Received,
}

/// Exactly eight motion histories, independent of networking, device work and rendering.
#[derive(Debug, Default)]
pub struct Riders {
    slots: [Option<Slot>; MAX_RIDERS],
}

impl Riders {
    /// Receives one sample from the authenticated current membership. Invalid, duplicate or
    /// reordered input is ignored; a new generation discards the previous rider's history.
    pub fn observe(&mut self, id: usize, generation: u64, sample: Sample, at: Instant) -> bool {
        let Some(slot) = self.slots.get_mut(id) else {
            return false;
        };
        if generation == 0 || !sample.valid() {
            return false;
        }
        let mut previous = None;
        if let Some(old) = slot.filter(|s| s.generation == generation) {
            if at <= old.latest.at || sample.elapsed <= old.latest.sample.elapsed {
                return false;
            }
            if at.duration_since(old.latest.at) < STALE
                && sample.distance.0 >= old.latest.sample.distance.0
            {
                previous = Some(old.latest);
            }
        }
        *slot = Some(Slot {
            generation,
            previous,
            latest: Received { sample, at },
        });
        true
    }

    /// Removes a departed membership immediately, including its interpolation history.
    pub fn clear(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            *slot = None;
        }
    }

    /// Renders 100 ms behind arrival time, extrapolates for at most 300 ms, then holds the
    /// position until stale. Future local timestamps and stale slots never appear.
    #[must_use]
    pub fn view(&self, now: Instant) -> [Option<Rider>; MAX_RIDERS] {
        std::array::from_fn(|id| {
            let slot = self.slots[id]?;
            let age = now.checked_duration_since(slot.latest.at)?;
            if age >= STALE {
                return None;
            }
            let sample = slot.latest.sample;
            let target = now.checked_sub(DELAY).unwrap_or(now);
            let distance = if target >= slot.latest.at {
                sample.distance.0
                    + sample.speed.0
                        * target
                            .duration_since(slot.latest.at)
                            .min(EXTRAPOLATE)
                            .as_secs_f64()
            } else if let Some(previous) = slot.previous {
                let span = slot.latest.at.duration_since(previous.at).as_secs_f64();
                let fraction = target.saturating_duration_since(previous.at).as_secs_f64() / span;
                previous.sample.distance.0
                    + (sample.distance.0 - previous.sample.distance.0) * fraction.clamp(0.0, 1.0)
            } else {
                sample.distance.0
            };
            Some(Rider {
                generation: slot.generation,
                distance: Meters(distance),
                speed: sample.speed,
                power: sample.power,
                cadence: sample.cadence,
                age,
            })
        })
    }
}

/// Computes independent shelter and an elastic band around the organizer. The organizer
/// gets no band. Keep together acts only while the local rider pedals. Assistance fades
/// after 500 ms without updates and is gone at the same two-second cutoff as the avatar.
#[must_use]
pub fn assistance(
    riders: &[Option<Rider>; MAX_RIDERS],
    distance: Meters,
    speed: MetersPerSecond,
    pedaling: bool,
    organizer: Option<usize>,
    drafting: bool,
    keep_together: bool,
) -> GroupEffect {
    let freshness =
        |rider: &Rider| ((STALE.as_secs_f64() - rider.age.as_secs_f64()) / 1.5).clamp(0.0, 1.0);
    let mut shelter: f64 = 0.0;
    if drafting && pedaling {
        for rider in riders.iter().flatten() {
            let gap = rider.distance.0 - distance.0;
            if (0.0..15.0).contains(&gap) {
                shelter = shelter.max(0.3 * (1.0 - gap / 15.0).powi(2) * freshness(rider));
            }
        }
    }
    let band = organizer
        .and_then(|id| riders.get(id))
        .copied()
        .flatten()
        .filter(|_| keep_together && pedaling)
        .map_or(0.0, |host| {
            let gap = host.distance.0 - distance.0;
            let outside = (gap.abs() - 5.0).max(0.0) * gap.signum();
            GroupEffect::band(Meters(outside), MetersPerSecond(speed.0 - host.speed.0)).0
                * freshness(&host)
        });
    GroupEffect::new(1.0 - shelter, GradePercent(band))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(seconds: u64, distance: f64) -> Sample {
        Sample {
            elapsed: Duration::from_secs(seconds),
            distance: Meters(distance),
            speed: MetersPerSecond(10.0),
            power: Watts(200.0),
            cadence: Rpm(85.0),
        }
    }
    #[test]
    fn smooths_packets_limits_extrapolation_and_retires_stale_members() {
        let at = Instant::now();
        let mut riders = Riders::default();
        assert!(riders.observe(1, 1, sample(1, 10.0), at));
        assert!(riders.observe(1, 1, sample(2, 12.0), at + Duration::from_millis(200)));
        let view = riders.view(at + Duration::from_millis(200))[1].unwrap();
        assert!((view.distance.0 - 11.0).abs() < 1e-9);
        let held = riders.view(at + Duration::from_secs(1))[1].unwrap();
        assert!((held.distance.0 - 15.0).abs() < 1e-9);
        assert!(riders.view(at + Duration::from_secs(3))[1].is_none());
        assert!(!riders.observe(1, 1, sample(1, 99.0), at + Duration::from_millis(300)));
        assert!(riders.observe(1, 2, sample(1, 30.0), at + Duration::from_millis(400)));
        assert_eq!(
            riders.view(at + Duration::from_millis(400))[1]
                .unwrap()
                .distance,
            Meters(30.0)
        );
        riders.clear(1);
        assert!(riders.view(at + Duration::from_millis(400))[1].is_none());
    }
    #[test]
    fn rejects_invalid_and_future_local_data_and_keeps_a_fixed_budget() {
        let at = Instant::now();
        let mut riders = Riders::default();
        assert!(!riders.observe(8, 1, sample(1, 0.0), at));
        assert!(!riders.observe(1, 0, sample(1, 0.0), at));
        assert!(!riders.observe(1, 1, sample(1, f64::NAN), at));
        riders.observe(1, 1, sample(1, 0.0), at + Duration::from_secs(1));
        assert!(riders.view(at)[1].is_none());
    }
    #[test]
    fn rules_are_independent_local_and_fade_without_new_telemetry() {
        let at = Instant::now();
        let mut riders = Riders::default();
        riders.observe(0, 1, sample(1, 60.0), at);
        riders.observe(2, 1, sample(1, 2.0), at);
        let view = riders.view(at);
        let both = assistance(
            &view,
            Meters(0.0),
            MetersPerSecond(10.0),
            true,
            Some(0),
            true,
            true,
        );
        assert!(both.drag_share() < 1.0 && both.grade_offset().0 < 0.0);
        assert_eq!(
            assistance(
                &view,
                Meters(0.0),
                MetersPerSecond(10.0),
                false,
                Some(0),
                true,
                true
            ),
            GroupEffect::default(),
            "coasting supplies no assistance"
        );
        assert_eq!(
            assistance(
                &view,
                Meters(0.0),
                MetersPerSecond(10.0),
                true,
                None,
                true,
                true
            )
            .grade_offset(),
            GradePercent(0.0)
        );
        assert_eq!(
            assistance(
                &view,
                Meters(0.0),
                MetersPerSecond(10.0),
                true,
                Some(0),
                false,
                true
            )
            .drag_share(),
            1.0
        );
        assert_eq!(
            assistance(
                &view,
                Meters(0.0),
                MetersPerSecond(10.0),
                true,
                Some(0),
                true,
                false
            )
            .grade_offset(),
            GradePercent(0.0)
        );
        assert!(
            assistance(
                &view,
                Meters(120.0),
                MetersPerSecond(10.0),
                true,
                Some(0),
                true,
                true
            )
            .grade_offset()
            .0 > 0.0
        );
        let stale = assistance(
            &riders.view(at + STALE),
            Meters(0.0),
            MetersPerSecond(10.0),
            true,
            Some(0),
            true,
            true,
        );
        assert_eq!(stale, GroupEffect::default());
        let fading = assistance(
            &riders.view(at + Duration::from_millis(1500)),
            Meters(0.0),
            MetersPerSecond(10.0),
            true,
            Some(0),
            true,
            true,
        );
        assert!(fading.grade_offset().0 > both.grade_offset().0);
    }

    #[test]
    fn drafting_uses_only_the_strongest_shelter_and_never_brakes_or_stacks() {
        let source = Rider {
            generation: 1,
            distance: Meters(1.0),
            speed: MetersPerSecond(10.0),
            power: Watts(200.0),
            cadence: Rpm(85.0),
            age: Duration::ZERO,
        };
        let mut single = [None; MAX_RIDERS];
        single[1] = Some(source);
        let many = [Some(source); MAX_RIDERS];
        let effect = |riders: &_| {
            assistance(
                riders,
                Meters(0.0),
                MetersPerSecond(10.0),
                true,
                None,
                true,
                false,
            )
        };
        assert_eq!(effect(&single), effect(&many));
        assert!(effect(&many).drag_share() >= 0.7);
        let behind = [Some(Rider {
            distance: Meters(-1.0),
            ..source
        }); MAX_RIDERS];
        assert_eq!(effect(&behind), GroupEffect::default());
    }
}
