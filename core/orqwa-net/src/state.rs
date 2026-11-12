//! Validated state datagrams and a fixed-size latest-state mailbox (R159, R161).

use std::time::{Duration, Instant};

use orqwa_domain::units::{Meters, MetersPerSecond, Rpm, Watts};

/// Riders including the host; also the hard bound on every mailbox/roster array.
pub const MAX_RIDERS: usize = 8;
/// The maximum state publication rate, independently enforced at the receiving host.
pub const STATE_EVERY: Duration = Duration::from_millis(200);
/// A small arrival burst allows normal network jitter without permitting a sustained flood.
const STATE_BURST: u8 = 10;
/// How far a peer's estimate of the shared clock may run ahead of ours.
pub(crate) const MAX_FUTURE: Duration = Duration::from_secs(2);
const MAX_SESSION_TIME: Duration = Duration::from_hours(48);
const FORMAT: u8 = 1;
const DATAGRAM_BYTES: usize = 22;

/// A rider number assigned by the host, never trusted merely because a peer sent it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RiderId(u8);

impl RiderId {
    /// The initial organizer's number; handover preserves identities when authority changes.
    pub const HOST: Self = Self(0);

    /// Checks a wire/roster number against the fixed rider limit.
    ///
    /// # Errors
    /// Returns [`StateError::Rider`] for a number outside the roster.
    pub fn new(value: u8) -> Result<Self, StateError> {
        if usize::from(value) >= MAX_RIDERS {
            return Err(StateError::Rider);
        }
        Ok(Self(value))
    }

    /// Its compact wire representation.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// A rider's own physical state, in SI units. No heart-rate field is transmitted.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RiderState {
    /// Elapsed time since the common start.
    pub at: Duration,
    /// Distance along the common course.
    pub distance: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// Rider's power.
    pub power: Watts,
    /// Rider's cadence.
    pub cadence: Rpm,
}

/// Invalid state must be discarded before queueing or fan-out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    /// Unknown rider, reused slot, stale connection or a forged sender number.
    #[error("The state is not from the current rider connection.")]
    Rider,
    /// Wrong version, size, number range or timestamp.
    #[error("The rider state is invalid.")]
    Invalid,
    /// Peer traffic exceeded the bounded jitter allowance and sustained 5 Hz rate.
    #[error("The rider is sending too many state packets.")]
    Flood,
}

impl RiderState {
    /// Encodes validated state in a fixed-size datagram; callers supply their assigned number.
    ///
    /// # Errors
    /// Invalid physical values or excessive elapsed time return [`StateError::Invalid`].
    pub fn datagram(&self, rider: RiderId) -> Result<[u8; DATAGRAM_BYTES], StateError> {
        self.validate(self.at)?;
        let mut bytes = [0; DATAGRAM_BYTES];
        bytes[0] = FORMAT;
        bytes[1] = rider.0;
        let millis = u32::try_from(self.at.as_millis()).map_err(|_| StateError::Invalid)?;
        bytes[2..6].copy_from_slice(&millis.to_be_bytes());
        for (target, value) in bytes[6..].as_chunks_mut::<4>().0.iter_mut().zip([
            self.distance.0,
            self.speed.0,
            self.power.0,
            self.cadence.0,
        ]) {
            #[allow(clippy::cast_possible_truncation)] // finite, range-checked physical values
            target.copy_from_slice(&(value as f32).to_be_bytes());
        }
        Ok(bytes)
    }

    /// Decodes state from an authenticated connection, verifying its host-assigned sender.
    ///
    /// # Errors
    /// Returns [`StateError`] for any malformed, forged or invalid state.
    pub fn from_datagram(
        bytes: &[u8],
        expected: RiderId,
        elapsed: Duration,
    ) -> Result<Self, StateError> {
        if bytes.len() != DATAGRAM_BYTES || bytes[0] != FORMAT {
            return Err(StateError::Invalid);
        }
        if RiderId::new(bytes[1])? != expected {
            return Err(StateError::Rider);
        }
        let millis = u32::from_be_bytes(bytes[2..6].try_into().map_err(|_| StateError::Invalid)?);
        let mut values = [0.0; 4];
        for (value, source) in values.iter_mut().zip(bytes[6..].as_chunks::<4>().0) {
            *value = f64::from(f32::from_be_bytes(*source));
        }
        let state = Self {
            at: Duration::from_millis(u64::from(millis)),
            distance: Meters(values[0]),
            speed: MetersPerSecond(values[1]),
            power: Watts(values[2]),
            cadence: Rpm(values[3]),
        };
        state.validate(elapsed)?;
        Ok(state)
    }

    fn validate(&self, elapsed: Duration) -> Result<(), StateError> {
        if self.at > MAX_SESSION_TIME
            || self.at > elapsed.saturating_add(MAX_FUTURE)
            || ![
                (self.distance.0, 10_000_000.0),
                (self.speed.0, 150.0),
                (self.power.0, 5_000.0),
                (self.cadence.0, 300.0),
            ]
            .iter()
            .all(|&(value, max)| value.is_finite() && (0.0..=max).contains(&value))
        {
            return Err(StateError::Invalid);
        }
        Ok(())
    }
}

/// A roster assignment bound to the lifetime of one connection, including when IDs are reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Membership {
    rider: RiderId,
    generation: u64,
}

impl Membership {
    /// The host-assigned sender number for this connection only.
    #[must_use]
    pub const fn rider(self) -> RiderId {
        self.rider
    }
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    active: bool,
    latest: Option<Duration>,
    pending: Option<RiderState>,
    tokens: u8,
    refilled: Instant,
}

/// A host-owned mailbox with exactly eight slots, no per-packet allocation or event queue.
///
/// Admission creates a [`Membership`] for the connection. Ingest charges the traffic budget
/// before parsing, rejects invalid state and replaces one pending value. Draining emits at
/// most one latest value per rider per 200 ms; membership/control events belong in a separate
/// bounded channel. Excess delivery is discarded before parsing; the transport separately
/// detects sustained raw traffic abuse without mistaking a delayed batch for a flood.
#[derive(Debug)]
pub struct StateInbox {
    slots: [Slot; MAX_RIDERS],
    next_publish: Instant,
}

impl StateInbox {
    /// Creates a mailbox with no admitted riders. Joining the host uses the same validation.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot {
                generation: 0,
                active: false,
                latest: None,
                pending: None,
                tokens: STATE_BURST,
                refilled: now,
            }),
            next_publish: now,
        }
    }

    /// Reserves a roster slot for one connection. Remove the old membership before reuse.
    ///
    /// # Errors
    /// Returns [`StateError::Rider`] for an occupied or exhausted slot.
    pub fn join(&mut self, rider: RiderId, now: Instant) -> Result<Membership, StateError> {
        let slot = &mut self.slots[usize::from(rider.0)];
        if slot.active {
            return Err(StateError::Rider);
        }
        slot.generation = slot.generation.checked_add(1).ok_or(StateError::Rider)?;
        slot.active = true;
        slot.latest = None;
        slot.pending = None;
        slot.tokens = STATE_BURST;
        slot.refilled = now;
        Ok(Membership {
            rider,
            generation: slot.generation,
        })
    }

    /// Removes only this connection's membership; a late old task cannot remove its successor.
    pub fn remove(&mut self, membership: Membership) {
        let slot = &mut self.slots[usize::from(membership.rider.0)];
        if slot.generation == membership.generation {
            slot.active = false;
            slot.latest = None;
            slot.pending = None;
        }
    }

    /// Clears activity motion while preserving memberships and ingress budgets.
    pub(crate) fn clear_states(&mut self) {
        for slot in &mut self.slots {
            slot.latest = None;
            slot.pending = None;
        }
    }

    /// Charges ingress before parsing, then keeps only a newer, validated state from the
    /// current connection. `Ok(false)` discards a reordered or duplicate sample.
    ///
    /// # Errors
    /// Returns [`StateError`] for a stale/forged connection or malformed packet.
    pub fn ingest(
        &mut self,
        membership: Membership,
        bytes: &[u8],
        elapsed: Duration,
        now: Instant,
    ) -> Result<bool, StateError> {
        let slot = &mut self.slots[usize::from(membership.rider.0)];
        if !slot.active || slot.generation != membership.generation {
            return Err(StateError::Rider);
        }
        let added = u8::try_from(
            now.saturating_duration_since(slot.refilled).as_nanos() / STATE_EVERY.as_nanos(),
        )
        .unwrap_or(STATE_BURST);
        if added > 0 {
            slot.tokens = slot.tokens.saturating_add(added).min(STATE_BURST);
            // Preserve fractional refill time: normal jitter around 200 ms must not
            // gradually exhaust a correctly paced rider's allowance.
            let remainder =
                now.saturating_duration_since(slot.refilled).as_nanos() % STATE_EVERY.as_nanos();
            #[allow(clippy::cast_possible_truncation)] // remainder is below 200 million ns
            let remainder = Duration::from_nanos(remainder as u64);
            slot.refilled = now.checked_sub(remainder).unwrap_or(now);
        }
        if slot.tokens == 0 {
            return Ok(false);
        }
        slot.tokens -= 1;
        let state = RiderState::from_datagram(bytes, membership.rider, elapsed)?;
        if slot.latest.is_some_and(|at| at >= state.at) {
            return Ok(false);
        }
        slot.latest = Some(state.at);
        slot.pending = Some(state);
        Ok(true)
    }

    /// Takes at most one latest state for each rider, at most 5 Hz. Calls between publication
    /// times return an empty fixed-size array and leave pending states coalescing in place.
    #[must_use]
    pub fn drain(&mut self, now: Instant) -> [Option<RiderState>; MAX_RIDERS] {
        // Callers tick every `STATE_EVERY`, each tick a little late or less. Due exactly a
        // period after the last call, a tick handled sooner than the last one published
        // nothing: 200/400 ms apart (#591). The schedule keeps its own beat and a tick counts
        // from half a period before it; a beat comes at least half a period after the call
        // that met the last one, so it still publishes at most 5 Hz.
        if now + STATE_EVERY / 2 <= self.next_publish {
            return [None; MAX_RIDERS];
        }
        self.next_publish = (self.next_publish + STATE_EVERY).max(now + STATE_EVERY / 2);
        std::array::from_fn(|i| self.slots[i].pending.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(ms: u64) -> RiderState {
        RiderState {
            at: Duration::from_millis(ms),
            distance: Meters(187_654.32),
            speed: MetersPerSecond(9.87),
            power: Watts(245.0),
            cadence: Rpm(88.0),
        }
    }

    #[test]
    fn datagrams_keep_route_precision_and_check_the_authenticated_sender() {
        let id = RiderId::new(1).unwrap();
        let original = state(1000);
        let bytes = original.datagram(id).unwrap();
        let received = RiderState::from_datagram(&bytes, id, original.at).unwrap();
        assert!((received.distance.0 - original.distance.0).abs() < 0.02);
        assert!((received.speed.0 - original.speed.0).abs() < 1e-5);
        assert_eq!(
            RiderState::from_datagram(&bytes, RiderId::HOST, original.at),
            Err(StateError::Rider)
        );
        assert_eq!(RiderId::new(255), Err(StateError::Rider));
    }

    #[test]
    fn every_non_finite_negative_or_excessive_wire_value_is_refused() {
        let id = RiderId::new(1).unwrap();
        for field in 0..4 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, f32::MAX] {
                let mut bytes = state(1000).datagram(id).unwrap();
                bytes[6 + field * 4..10 + field * 4].copy_from_slice(&value.to_be_bytes());
                assert_eq!(
                    RiderState::from_datagram(&bytes, id, Duration::from_secs(1)),
                    Err(StateError::Invalid)
                );
            }
        }
        let bytes = state(10_000).datagram(id).unwrap();
        assert_eq!(
            RiderState::from_datagram(&bytes, id, Duration::from_secs(1)),
            Err(StateError::Invalid)
        );
        assert!(
            RiderState {
                distance: Meters(f64::NAN),
                ..state(1000)
            }
            .datagram(id)
            .is_err()
        );
    }

    #[test]
    fn a_future_packet_cannot_poison_ordering_for_legitimate_packets() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        let future = state(100_000).datagram(member.rider()).unwrap();
        assert_eq!(
            inbox.ingest(member, &future, Duration::from_secs(1), now),
            Err(StateError::Invalid)
        );
        let valid = state(1000).datagram(member.rider()).unwrap();
        assert_eq!(
            inbox.ingest(member, &valid, Duration::from_secs(1), now),
            Ok(true)
        );
    }

    #[test]
    fn slow_consumers_keep_one_latest_sample_and_publish_at_most_five_times_per_second() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        assert!(inbox.drain(now).iter().all(Option::is_none));
        for ms in 1..=u64::from(STATE_BURST) {
            let bytes = state(ms).datagram(member.rider()).unwrap();
            assert_eq!(
                inbox.ingest(member, &bytes, Duration::from_secs(1), now),
                Ok(true)
            );
        }
        assert!(
            inbox
                .drain(now + STATE_EVERY / 2)
                .iter()
                .all(Option::is_none)
        );
        let latest = inbox.drain(now + STATE_EVERY);
        assert_eq!(latest.iter().flatten().count(), 1);
        assert_eq!(
            latest[0].unwrap().at,
            Duration::from_millis(u64::from(STATE_BURST))
        );
        assert!(inbox.drain(now + STATE_EVERY).iter().all(Option::is_none));
    }

    #[test]
    fn a_tick_a_moment_sooner_than_the_last_late_one_still_publishes() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        let datagram = |ms| state(ms).datagram(RiderId::HOST).unwrap();
        let late = now + STATE_EVERY + Duration::from_millis(1);
        inbox
            .ingest(member, &datagram(200), Duration::from_millis(200), late)
            .unwrap();
        assert!(inbox.drain(late)[0].is_some());
        let on_time = now + 2 * STATE_EVERY;
        inbox
            .ingest(member, &datagram(400), Duration::from_millis(400), on_time)
            .unwrap();
        assert!(inbox.drain(on_time)[0].is_some());
        // Ticks handled up to most of a period late, in any order, each publish once.
        for (tick, late_ms) in [(3_u32, 150), (4, 0), (5, 199), (6, 20), (7, 180), (8, 1)] {
            let at = now + tick * STATE_EVERY + Duration::from_millis(late_ms);
            let ms = u64::from(tick) * 200;
            inbox
                .ingest(member, &datagram(ms), Duration::from_millis(ms), at)
                .unwrap();
            assert!(inbox.drain(at)[0].is_some(), "tick {tick}");
            assert!(inbox.drain(at)[0].is_none(), "once per tick");
        }
    }

    #[test]
    fn malformed_duplicates_and_valid_packets_all_consume_the_ingress_budget() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        for _ in 0..STATE_BURST {
            assert_eq!(
                inbox.ingest(member, &[], Duration::ZERO, now),
                Err(StateError::Invalid)
            );
        }
        let bytes = state(0).datagram(member.rider()).unwrap();
        assert_eq!(inbox.ingest(member, &bytes, Duration::ZERO, now), Ok(false));
        assert_eq!(
            inbox.ingest(member, &bytes, Duration::ZERO, now + STATE_EVERY),
            Ok(true)
        );
        assert_eq!(
            inbox.ingest(member, &bytes, Duration::ZERO, now + STATE_EVERY),
            Ok(false)
        );
        assert_eq!(
            inbox.ingest(member, &bytes, Duration::ZERO, now + 2 * STATE_EVERY),
            Ok(false)
        );
    }

    #[test]
    fn delayed_state_bursts_resume_without_replacing_the_current_membership() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        for ms in 0..50 {
            let bytes = state(ms).datagram(member.rider()).unwrap();
            assert_eq!(
                inbox.ingest(member, &bytes, Duration::from_secs(1), now),
                Ok(ms < u64::from(STATE_BURST))
            );
        }
        let bytes = state(200).datagram(member.rider()).unwrap();
        assert_eq!(
            inbox.ingest(member, &bytes, Duration::from_secs(1), now + STATE_EVERY),
            Ok(true)
        );
        assert_eq!(
            inbox.drain(now + STATE_EVERY)[0].unwrap().at,
            Duration::from_millis(200)
        );
    }

    #[test]
    fn a_departed_connection_cannot_submit_for_or_remove_a_reused_rider_slot() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let old = inbox.join(RiderId::HOST, now).unwrap();
        inbox.remove(old);
        let next = inbox.join(RiderId::HOST, now).unwrap();
        let bytes = state(0).datagram(old.rider()).unwrap();
        assert_eq!(
            inbox.ingest(old, &bytes, Duration::ZERO, now),
            Err(StateError::Rider)
        );
        inbox.remove(old);
        assert_eq!(inbox.ingest(next, &bytes, Duration::ZERO, now), Ok(true));
        inbox.remove(next);
        assert!(inbox.drain(now).iter().all(Option::is_none));
    }

    #[test]
    fn malformed_lengths_versions_and_timestamps_are_refused() {
        let id = RiderId::HOST;
        let bytes = state(1000).datagram(id).unwrap();
        for len in 0..bytes.len() {
            assert!(RiderState::from_datagram(&bytes[..len], id, Duration::from_secs(1)).is_err());
        }
        let mut padded = bytes.to_vec();
        padded.push(0);
        assert!(RiderState::from_datagram(&padded, id, Duration::from_secs(1)).is_err());
        let mut version = bytes;
        version[0] += 1;
        assert!(RiderState::from_datagram(&version, id, Duration::from_secs(1)).is_err());
        assert!(
            RiderState {
                at: MAX_SESSION_TIME + Duration::from_secs(1),
                ..state(0)
            }
            .datagram(id)
            .is_err()
        );
    }

    #[test]
    fn all_eight_riders_have_independent_fixed_slots_and_a_ninth_is_invalid() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        for id in 0..u8::try_from(MAX_RIDERS).unwrap() {
            let member = inbox.join(RiderId::new(id).unwrap(), now).unwrap();
            let bytes = state(u64::from(id)).datagram(member.rider()).unwrap();
            assert_eq!(
                inbox.ingest(member, &bytes, Duration::from_secs(1), now),
                Ok(true)
            );
            assert_eq!(inbox.join(member.rider(), now), Err(StateError::Rider));
        }
        assert!(RiderId::new(u8::try_from(MAX_RIDERS).unwrap()).is_err());
        assert_eq!(inbox.drain(now).iter().flatten().count(), MAX_RIDERS);
        assert!(inbox.drain(now + STATE_EVERY).iter().all(Option::is_none));
    }

    #[test]
    fn sustained_five_hertz_with_network_jitter_does_not_exhaust_the_allowance() {
        let now = Instant::now();
        let mut inbox = StateInbox::new(now);
        let member = inbox.join(RiderId::HOST, now).unwrap();
        for sample in 1..=1000_u64 {
            let millis = sample * 200 + u64::from(sample % 2 == 0);
            let state = state(millis);
            assert!(
                inbox
                    .ingest(
                        member,
                        &state.datagram(RiderId::HOST).unwrap(),
                        state.at,
                        now + state.at
                    )
                    .is_ok()
            );
        }
    }
}
