//! Admission budgets taken before allocating a connection task or starting TLS (R161).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Maximum simultaneous pending and established transport connections at a host.
pub const MAX_CONNECTIONS: usize = 16;
/// One public address may contain several riders behind the same home router.
pub const MAX_PER_SOURCE: usize = 8;
/// Maximum burst of new handshakes; sustained replenishment is four per second.
pub const HANDSHAKE_BURST: u32 = 16;
const REFILL: Duration = Duration::from_millis(250);

/// Shared, fixed-capacity connection and handshake-rate limits.
///
/// Obtain a permit before accepting an incoming transport handshake. Keep it for the whole
/// connection lifetime, including while a successful connection waits in a bounded event queue.
/// Dropping the permit releases capacity even after an error or cancelled operation.
#[derive(Clone, Debug)]
pub struct ConnectionBudget(Arc<Mutex<State>>);

#[derive(Debug)]
struct State {
    active: usize,
    sources: HashMap<IpAddr, usize>,
    tokens: u32,
    refilled: Instant,
}

/// Capacity owned by one transport connection, automatically returned on drop.
#[must_use = "dropping the permit immediately returns the connection capacity"]
#[derive(Debug)]
pub struct ConnectionPermit {
    budget: ConnectionBudget,
    source: IpAddr,
}

impl ConnectionBudget {
    /// Starts an empty connection pool with a bounded initial handshake burst.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self(Arc::new(Mutex::new(State {
            active: 0,
            sources: HashMap::with_capacity(MAX_CONNECTIONS),
            tokens: HANDSHAKE_BURST,
            refilled: now,
        })))
    }

    /// Tries to reserve capacity without waiting, allocating a task or growing an unbounded
    /// source-address table. A refusal consumes no connection capacity.
    #[must_use]
    pub fn try_acquire(&self, source: IpAddr, now: Instant) -> Option<ConnectionPermit> {
        let mut state = self.state();
        if state.active >= MAX_CONNECTIONS
            || state.sources.get(&source).copied().unwrap_or(0) >= MAX_PER_SOURCE
        {
            return None;
        }
        let added = u32::try_from(
            now.saturating_duration_since(state.refilled).as_nanos() / REFILL.as_nanos(),
        )
        .unwrap_or(HANDSHAKE_BURST);
        if added > 0 {
            state.tokens = state.tokens.saturating_add(added).min(HANDSHAKE_BURST);
            let remainder =
                now.saturating_duration_since(state.refilled).as_nanos() % REFILL.as_nanos();
            #[allow(clippy::cast_possible_truncation)] // remainder is below 250 million ns
            let remainder = Duration::from_nanos(remainder as u64);
            state.refilled = now.checked_sub(remainder).unwrap_or(now);
        }
        if state.tokens == 0 {
            return None;
        }
        state.tokens -= 1;
        state.active += 1;
        *state.sources.entry(source).or_default() += 1;
        Some(ConnectionPermit {
            budget: self.clone(),
            source,
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // No peer input or user callback runs while holding this lock. If an unrelated panic
        // poisoned it, retaining the existing counters preserves the capacity limit.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut state = self.budget.state();
        state.active -= 1;
        if let Some(count) = state.sources.get_mut(&self.source) {
            *count -= 1;
            if *count == 0 {
                state.sources.remove(&self.source);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(n: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, n])
    }

    #[test]
    fn connections_require_capacity_before_any_peer_task_can_be_started() {
        let now = Instant::now();
        let budget = ConnectionBudget::new(now);
        let permits: Vec<_> = (0..MAX_CONNECTIONS)
            .map(|n| {
                budget
                    .try_acquire(source(u8::try_from(n).unwrap()), now)
                    .unwrap()
            })
            .collect();
        for n in 32..200 {
            assert!(
                budget
                    .try_acquire(source(n), now + Duration::from_secs(60))
                    .is_none()
            );
        }
        assert!(budget.state().sources.len() <= MAX_CONNECTIONS);
        drop(permits);
        assert!(budget.state().sources.is_empty());
        assert!(
            budget
                .try_acquire(source(1), now + Duration::from_secs(60))
                .is_some()
        );
    }

    #[test]
    fn one_source_cannot_use_the_entire_host_but_a_household_can_join() {
        let now = Instant::now();
        let budget = ConnectionBudget::new(now);
        let permits: Vec<_> = (0..MAX_PER_SOURCE)
            .map(|_| budget.try_acquire(source(1), now).unwrap())
            .collect();
        assert!(budget.try_acquire(source(1), now).is_none());
        assert!(budget.try_acquire(source(2), now).is_some());
        drop(permits);
        assert!(budget.try_acquire(source(1), now).is_some());
    }

    #[test]
    fn fast_connect_disconnect_cycles_do_not_bypass_the_handshake_rate() {
        let now = Instant::now();
        let budget = ConnectionBudget::new(now);
        for _ in 0..HANDSHAKE_BURST {
            drop(budget.try_acquire(source(1), now).unwrap());
        }
        assert!(budget.try_acquire(source(1), now).is_none());
        assert!(budget.try_acquire(source(1), now + REFILL / 2).is_none());
        drop(budget.try_acquire(source(1), now + REFILL).unwrap());
        assert!(budget.try_acquire(source(1), now + REFILL).is_none());
        assert!(budget.state().sources.is_empty());
    }

    #[test]
    fn concurrent_attempts_share_one_capacity_limit_and_release_every_source() {
        let now = Instant::now();
        let budget = ConnectionBudget::new(now);
        let barrier = std::sync::Barrier::new(64);
        let accepted = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for n in 0..64 {
                let (budget, barrier, accepted) = (&budget, &barrier, &accepted);
                scope.spawn(move || {
                    let permit = budget.try_acquire(source(n), now);
                    if permit.is_some() {
                        accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    // Successful permits stay live until all attempts have completed.
                    barrier.wait();
                    drop(permit);
                });
            }
        });
        assert_eq!(
            accepted.load(std::sync::atomic::Ordering::Relaxed),
            MAX_CONNECTIONS
        );
        assert!(budget.state().sources.is_empty());
        assert_eq!(budget.state().active, 0);
    }
}
