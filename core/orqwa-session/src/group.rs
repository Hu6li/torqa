//! Fixed-size, membership-bound group playback and individual assistance (R164, R166, R193).
//! No received measurement is substituted for the local rider's telemetry or ERG target.

use orqwa_domain::units::{GradePercent, Meters, MetersPerSecond, Rpm, Watts};
use orqwa_physics::GroupEffect;
use std::time::{Duration, Instant};

/// The group limit, checked against the transport's limit by the app integration.
pub const MAX_RIDERS: usize = 8;
/// Stop using a rider's source state after two seconds without a fresh sample.
pub const STALE: Duration = Duration::from_secs(2);
/// Shared presentation delay, covering publication, host fan-out and receiver coalescing
/// (three 200 ms periods), with 200 ms left for network jitter. Local avatars use it too.
pub const DELAY: Duration = Duration::from_millis(800);
const EXTRAPOLATE: Duration = Duration::from_millis(300);
// More than the complete freshness window at 5 Hz; independent of session length.
const HISTORY: usize = 16;

/// One validated remote measurement. Heart rate deliberately has no representation here.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Elapsed time since the shared start, for playback and rejecting reordered samples.
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
    /// Virtual speed at the presented position.
    pub speed: MetersPerSecond,
    /// Presented peer power; never an input to local recording or ERG.
    pub power: Watts,
    /// Presented peer cadence.
    pub cadence: Rpm,
    /// The greater of arrival age and source age on the shared clock.
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
    samples: [Option<Received>; HISTORY],
    len: usize,
}
impl Slot {
    fn latest(&self) -> Received {
        self.samples[self.len - 1].expect("a motion history is never empty")
    }

    fn sample(&self, target: Duration) -> Sample {
        let mut previous = self.samples[0]
            .expect("a motion history is never empty")
            .sample;
        for next in self.samples[1..self.len].iter().flatten() {
            if target <= next.sample.elapsed {
                return interpolate(previous, next.sample, target);
            }
            previous = next.sample;
        }
        let ahead = target.saturating_sub(previous.elapsed).min(EXTRAPOLATE);
        Sample {
            distance: Meters(previous.distance.0 + previous.speed.0 * ahead.as_secs_f64()),
            ..previous
        }
    }
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
        if let Some(old) = slot.as_mut().filter(|s| s.generation == generation) {
            let latest = old.latest();
            if at < latest.at || sample.elapsed <= latest.sample.elapsed {
                return false;
            }
            if at.duration_since(latest.at) < STALE
                && sample.elapsed.saturating_sub(latest.sample.elapsed) < STALE
                && sample.distance.0 >= latest.sample.distance.0
            {
                if old.len == HISTORY {
                    old.samples.copy_within(1..HISTORY, 0);
                    old.len -= 1;
                }
                old.samples[old.len] = Some(Received { sample, at });
                old.len += 1;
                return true;
            }
        }
        let mut samples = [None; HISTORY];
        samples[0] = Some(Received { sample, at });
        *slot = Some(Slot {
            generation,
            samples,
            len: 1,
        });
        true
    }

    /// Removes a departed membership immediately, including its interpolation history.
    pub fn clear(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            *slot = None;
        }
    }

    /// Plays every rider, including the local rider's accepted echo, at the same position
    /// on the shared clock. Packet arrival times affect freshness only, never motion speed.
    #[must_use]
    pub fn view(&self, now: Instant, elapsed: Duration) -> [Option<Rider>; MAX_RIDERS] {
        self.at(now, elapsed, elapsed.saturating_sub(DELAY))
    }

    /// Predicts current positions for assistance against the live local simulation. The
    /// presentation delay must never make a rider draft their own echo or lag the group.
    #[must_use]
    pub fn current(&self, now: Instant, elapsed: Duration) -> [Option<Rider>; MAX_RIDERS] {
        self.at(now, elapsed, elapsed)
    }

    fn at(&self, now: Instant, elapsed: Duration, target: Duration) -> [Option<Rider>; MAX_RIDERS] {
        std::array::from_fn(|id| {
            let slot = self.slots[id].as_ref()?;
            let latest = slot.latest();
            let age = now
                .checked_duration_since(latest.at)?
                .max(elapsed.saturating_sub(latest.sample.elapsed));
            if age >= STALE {
                return None;
            }
            let sample = slot.sample(target);
            Some(Rider {
                generation: slot.generation,
                distance: sample.distance,
                speed: sample.speed,
                power: sample.power,
                cadence: sample.cadence,
                age,
            })
        })
    }
}

// Monotone cubic Hermite interpolation preserves endpoint speeds without overshooting a
// stopped rider. Limiting both tangents also handles rounded wire positions and pauses.
fn interpolate(a: Sample, b: Sample, target: Duration) -> Sample {
    let span = b.elapsed.saturating_sub(a.elapsed).as_secs_f64();
    let t = (target.saturating_sub(a.elapsed).as_secs_f64() / span).clamp(0.0, 1.0);
    let distance = b.distance.0 - a.distance.0;
    let mut from = a.speed.0 * span;
    let mut to = b.speed.0 * span;
    let length = from.hypot(to);
    if length > 3.0 * distance {
        let scale = 3.0 * distance / length;
        from *= scale;
        to *= scale;
    }
    Sample {
        elapsed: target,
        distance: Meters(
            a.distance.0
                + (-2.0 * t + 3.0) * t * t * distance
                + ((t - 2.0) * t + 1.0) * t * from
                + (t - 1.0) * t * t * to,
        ),
        speed: MetersPerSecond(a.speed.0 + (b.speed.0 - a.speed.0) * t),
        power: Watts(a.power.0 + (b.power.0 - a.power.0) * t),
        cadence: Rpm(a.cadence.0 + (b.cadence.0 - a.cadence.0) * t),
    }
}

/// Computes shelter and a band around the moving group, including live local motion (R211).
/// The caller excludes the local echo and rate-limits the applied band. Organizer identity
/// has no role. Sources fade after 500 ms and stop contributing at two seconds.
#[must_use]
pub fn assistance(
    riders: &[Option<Rider>; MAX_RIDERS],
    distance: Meters,
    speed: MetersPerSecond,
    pedaling: bool,
    drafting: bool,
    keep_together: bool,
) -> GroupEffect {
    let mut shelter: f64 = 0.0;
    if drafting && pedaling {
        for rider in riders.iter().flatten() {
            let gap = rider.distance.0 - distance.0;
            if (0.0..15.0).contains(&gap) {
                shelter = shelter.max(0.3 * (1.0 - gap / 15.0).powi(2) * freshness(rider));
            }
        }
    }
    let band = if keep_together && pedaling {
        reference(riders, distance, speed).map_or(0.0, |(center, pace, strength)| {
            let gap = center.0 - distance.0;
            let outside = (gap.abs() - 5.0).max(0.0) * gap.signum();
            GroupEffect::band(Meters(outside), MetersPerSecond(speed.0 - pace.0)).0 * strength
        })
    } else {
        0.0
    };
    GroupEffect::new(1.0 - shelter, GradePercent(band))
}

fn freshness(rider: &Rider) -> f64 {
    ((STALE.as_secs_f64() - rider.age.as_secs_f64()) / 1.5).clamp(0.0, 1.0)
}

// Bounded influence limits one sprint's pull without choosing a leader. Smooth influence
// avoids a reference jump when the first peer starts to fade. Both arrays stay fixed-size.
fn reference(
    riders: &[Option<Rider>; MAX_RIDERS],
    distance: Meters,
    speed: MetersPerSecond,
) -> Option<(Meters, MetersPerSecond, f64)> {
    const MOVING: f64 = 0.5;
    let mut positions = [(0.0, 0.0); MAX_RIDERS + 1];
    let mut speeds = positions;
    let mut count = 0;
    let mut strength: f64 = 0.0;
    for rider in riders.iter().flatten().filter(|r| r.speed.0 > MOVING) {
        let weight = freshness(rider);
        if weight > 0.0 {
            positions[count] = (rider.distance.0, weight);
            speeds[count] = (rider.speed.0, weight);
            count += 1;
            strength = strength.max(weight);
        }
    }
    if count == 0 {
        return None;
    }
    if speed.0 > MOVING {
        positions[count] = (distance.0, 1.0);
        speeds[count] = (speed.0, 1.0);
        count += 1;
    }
    Some((
        Meters(robust_mean(&positions[..count], 12.5)),
        MetersPerSecond(robust_mean(&speeds[..count], 0.5)),
        strength,
    ))
}

fn robust_mean(values: &[(f64, f64)], scale: f64) -> f64 {
    let (mut low, mut high) = values.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(low, high), &(value, _)| (low.min(value), high.max(value)),
    );
    // Each rider's pull approaches its weight continuously, even at a distant sprint.
    // The balance is strictly decreasing, so bisection has one root and bounded work.
    for _ in 0..64 {
        let center = low.midpoint(high);
        let balance: f64 = values
            .iter()
            .map(|&(value, weight)| {
                let offset = value - center;
                weight * offset / offset.hypot(scale)
            })
            .sum();
        if balance.abs() < 1e-12 {
            return center;
        }
        if balance > 0.0 {
            low = center;
        } else {
            high = center;
        }
    }
    low.midpoint(high)
}

#[cfg(test)]
mod reference_tests;

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
        let mut first = sample(1, 10.0);
        first.elapsed = Duration::ZERO;
        let mut next = sample(1, 12.0);
        next.elapsed = Duration::from_millis(200);
        assert!(riders.observe(1, 1, first, at));
        assert!(riders.observe(1, 1, next, at + Duration::from_millis(200)));
        let view =
            riders.view(at + Duration::from_millis(900), Duration::from_millis(900))[1].unwrap();
        assert!((view.distance.0 - 11.0).abs() < 1e-9);
        let held =
            riders.view(at + Duration::from_secs(1), Duration::from_millis(1500))[1].unwrap();
        assert!((held.distance.0 - 15.0).abs() < 1e-9);
        assert!(riders.view(at + Duration::from_secs(3), Duration::from_secs(3))[1].is_none());
        assert!(!riders.observe(1, 1, first, at + Duration::from_millis(300)));
        assert!(riders.observe(1, 2, sample(1, 30.0), at + Duration::from_millis(400)));
        assert_eq!(
            riders.view(at + Duration::from_millis(400), Duration::from_millis(400))[1]
                .unwrap()
                .distance,
            Meters(30.0)
        );
        riders.clear(1);
        assert!(
            riders.view(at + Duration::from_millis(400), Duration::from_millis(400))[1].is_none()
        );
    }
    #[test]
    fn rejects_invalid_and_future_local_data_and_keeps_a_fixed_budget() {
        let at = Instant::now();
        let mut riders = Riders::default();
        assert!(!riders.observe(8, 1, sample(1, 0.0), at));
        assert!(!riders.observe(1, 0, sample(1, 0.0), at));
        assert!(!riders.observe(1, 1, sample(1, f64::NAN), at));
        riders.observe(1, 1, sample(1, 0.0), at + Duration::from_secs(1));
        assert!(riders.view(at, Duration::ZERO)[1].is_none());
    }

    #[test]
    fn jitter_loss_and_bursts_keep_both_screens_smooth_and_agree_on_overtakes() {
        // Separate local Instant epochs, with the same shared start age. The organizer sees
        // its own accepted state sooner, but must not get an advantage in the displayed gap.
        let starts = [Instant::now(), Instant::now() + Duration::from_secs(100)];
        let mut screens = vec![Riders::default(), Riders::default()].into_boxed_slice();
        let mut packets: [Vec<_>; 2] = std::array::from_fn(|screen| {
            let mut packets = Vec::new();
            for sequence in 0..=50_u64 {
                for id in 0..2 {
                    if screen == 1 && id == 0 && sequence % 9 == 3 {
                        continue; // a lost packet, never replayed
                    }
                    let ms = sequence * 200;
                    let seconds = Duration::from_millis(ms).as_secs_f64();
                    let speed = if id == 0 { 10.0 } else { 10.5 };
                    let distance = if id == 0 { 100.0 } else { 99.0 } + speed * seconds;
                    let delay = if screen == 0 && id == 0 {
                        0
                    } else {
                        [90, 330, 160, 510, 110][usize::try_from(sequence % 5).unwrap()]
                    };
                    let mut state = sample(0, distance);
                    state.elapsed = Duration::from_millis(ms);
                    state.speed = MetersPerSecond(speed);
                    packets.push((ms + delay, id, state));
                }
            }
            packets.sort_by_key(|packet| packet.0);
            packets.reverse();
            packets
        });
        let mut previous = [[None::<f64>; 2]; 2];
        for ms in (0..=9500).step_by(10) {
            let elapsed = Duration::from_millis(ms);
            let mut gaps = [0.0; 2];
            for screen in 0..2 {
                while packets[screen].last().is_some_and(|p| p.0 <= ms) {
                    let (arrival, id, state) = packets[screen].pop().unwrap();
                    screens[screen].observe(
                        id,
                        1,
                        state,
                        starts[screen] + Duration::from_millis(arrival),
                    );
                }
                if ms < 1000 {
                    continue;
                }
                let view = screens[screen].view(starts[screen] + elapsed, elapsed);
                for id in 0..2 {
                    let rider = view[id].unwrap();
                    let speed = if id == 0 { 10.0 } else { 10.5 };
                    let expected = if id == 0 { 100.0 } else { 99.0 }
                        + speed * elapsed.saturating_sub(DELAY).as_secs_f64();
                    assert!(
                        (rider.distance.0 - expected).abs() < 1e-8,
                        "screen {screen}, rider {id}, frame {ms}: {} vs {expected}",
                        rider.distance.0
                    );
                    if let Some(last) = previous[screen][id] {
                        assert!(
                            (rider.distance.0 - last - speed * 0.01).abs() < 1e-8,
                            "no arrival-time speed pulses or backward corrections"
                        );
                    }
                    previous[screen][id] = Some(rider.distance.0);
                }
                gaps[screen] = view[1].unwrap().distance.0 - view[0].unwrap().distance.0;
            }
            assert!((gaps[0] - gaps[1]).abs() < 1e-8, "same order at frame {ms}");
        }
    }

    #[test]
    fn acceleration_and_stopping_are_continuous_without_overshooting() {
        let at = Instant::now();
        let mut riders = Riders::default();
        for ms in [0, 200, 400, 600] {
            let elapsed = Duration::from_millis(ms);
            let t = elapsed.as_secs_f64();
            let mut state = sample(0, 100.0 + 4.0 * t + t * t);
            state.elapsed = elapsed;
            state.speed = MetersPerSecond(4.0 + 2.0 * t);
            riders.observe(0, 1, state, at + elapsed);
        }
        for ms in (800..=1400).step_by(10) {
            let elapsed = Duration::from_millis(ms);
            let t = elapsed.saturating_sub(DELAY).as_secs_f64();
            let view = riders.view(at + elapsed, elapsed)[0].unwrap();
            assert!((view.distance.0 - (100.0 + 4.0 * t + t * t)).abs() < 1e-8);
        }
        let mut stopped = sample(0, 103.0);
        stopped.elapsed = Duration::from_millis(800);
        stopped.speed = MetersPerSecond(0.0);
        riders.observe(0, 1, stopped, at + stopped.elapsed);
        stopped.elapsed = Duration::from_millis(1000);
        riders.observe(0, 1, stopped, at + stopped.elapsed);
        let mut last = 102.76;
        for ms in (1400..=2000).step_by(10) {
            let elapsed = Duration::from_millis(ms);
            let rider = riders.view(at + elapsed, elapsed)[0].unwrap();
            assert!(rider.distance.0 >= last - 1e-8 && rider.distance.0 <= 103.0);
            last = rider.distance.0;
        }
        assert_eq!(last, 103.0);
    }

    #[test]
    fn old_source_times_cannot_revive_a_rider_and_reentry_discards_history() {
        let at = Instant::now();
        let mut riders = Riders::default();
        riders.observe(1, 1, sample(1, 100.0), at);
        assert!(
            riders.view(at, Duration::from_secs(4))[1].is_none(),
            "a delayed old packet is stale even when it just arrived"
        );
        riders.observe(1, 1, sample(4, 140.0), at + Duration::from_secs(3));
        assert_eq!(
            riders.view(at + Duration::from_secs(3), Duration::from_secs(4))[1]
                .unwrap()
                .distance,
            Meters(140.0)
        );
        riders.observe(1, 2, sample(5, 20.0), at + Duration::from_secs(4));
        assert_eq!(
            riders.view(at + Duration::from_secs(4), Duration::from_secs(5))[1]
                .unwrap()
                .distance,
            Meters(20.0)
        );
    }
    #[test]
    fn rules_are_independent_local_and_fade_without_new_telemetry() {
        let at = Instant::now();
        let mut riders = Riders::default();
        riders.observe(0, 1, sample(1, 60.0), at);
        riders.observe(2, 1, sample(1, 2.0), at);
        let view = riders.current(at, Duration::from_secs(1));
        let effect = |pedaling, drafting, keep_together| {
            assistance(
                &view,
                Meters(0.0),
                MetersPerSecond(10.0),
                pedaling,
                drafting,
                keep_together,
            )
        };
        let both = effect(true, true, true);
        assert!(both.drag_share() < 1.0 && both.grade_offset().0 < 0.0);
        assert_eq!(
            effect(false, true, true),
            GroupEffect::default(),
            "coasting supplies no assistance"
        );
        assert_eq!(effect(true, false, true).drag_share(), 1.0);
        assert_eq!(effect(true, true, false).grade_offset(), GradePercent(0.0));
        assert!(
            assistance(
                &view,
                Meters(120.0),
                MetersPerSecond(10.0),
                true,
                true,
                true
            )
            .grade_offset()
            .0 > 0.0
        );
        let stale = assistance(
            &riders.current(at + STALE, Duration::from_secs(1) + STALE),
            Meters(0.0),
            MetersPerSecond(10.0),
            true,
            true,
            true,
        );
        assert_eq!(stale, GroupEffect::default());
        let later = riders.current(
            at + Duration::from_millis(1500),
            Duration::from_millis(2500),
        );
        let fading = assistance(&later, Meters(0.0), MetersPerSecond(10.0), true, true, true);
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
