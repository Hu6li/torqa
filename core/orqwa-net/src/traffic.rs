//! Connection abuse protection, separate from disposable media delivery budgets.

use std::time::{Duration, Instant};

use crate::state::MAX_RIDERS;

// Twice a full room's 50 Hz speech plus 5 Hz state. A single delayed batch is
// disposable congestion; only repeated overloaded windows prove sustained excess.
const PACKETS_PER_SECOND: usize = MAX_RIDERS * 110;
const OVERLOADED_WINDOWS: u8 = 3;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Flood;

pub(crate) struct DatagramIngress {
    window: Instant,
    packets: usize,
    overloaded: u8,
}

impl DatagramIngress {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            window: now,
            packets: 0,
            overloaded: 0,
        }
    }

    pub(crate) fn accept(&mut self, now: Instant) -> Result<bool, Flood> {
        let seconds = now.saturating_duration_since(self.window).as_secs();
        if seconds > 0 {
            self.overloaded = if seconds == 1 && self.packets > PACKETS_PER_SECOND {
                self.overloaded.saturating_add(1)
            } else {
                0
            };
            self.window += Duration::from_secs(seconds);
            self.packets = 0;
        }
        self.packets = self.packets.saturating_add(1);
        if self.packets <= PACKETS_PER_SECOND {
            return Ok(true);
        }
        if self.overloaded >= OVERLOADED_WINDOWS - 1 {
            return Err(Flood);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_delayed_batch_is_dropped_without_ending_the_connection() {
        let now = Instant::now();
        let mut guard = DatagramIngress::new(now);
        for packet in 0..PACKETS_PER_SECOND * 4 {
            assert_eq!(guard.accept(now), Ok(packet < PACKETS_PER_SECOND));
        }
        assert_eq!(guard.accept(now + Duration::from_secs(1)), Ok(true));
        assert_eq!(guard.accept(now + Duration::from_secs(2)), Ok(true));
    }

    #[test]
    fn sustained_excess_is_rejected_but_a_quiet_window_resets_it() {
        let now = Instant::now();
        let mut guard = DatagramIngress::new(now);
        for second in 0..2 {
            for _ in 0..=PACKETS_PER_SECOND {
                assert!(guard.accept(now + Duration::from_secs(second)).is_ok());
            }
        }
        let third = now + Duration::from_secs(2);
        for _ in 0..PACKETS_PER_SECOND {
            assert_eq!(guard.accept(third), Ok(true));
        }
        assert_eq!(guard.accept(third), Err(Flood));
        assert_eq!(guard.accept(now + Duration::from_secs(4)), Ok(true));
    }

    #[test]
    fn a_full_room_keeps_its_connections_through_periodic_delivery_bursts() {
        let now = Instant::now();
        let mut guard = DatagramIngress::new(now);
        for batch in 0..100 {
            let at = now + Duration::from_millis(batch * 500);
            for _ in 0..MAX_RIDERS * 28 {
                assert_eq!(guard.accept(at), Ok(true));
            }
        }
    }

    #[test]
    fn overloads_separated_by_normal_traffic_do_not_accumulate_toward_disconnection() {
        let now = Instant::now();
        let mut guard = DatagramIngress::new(now);
        for second in 0..60 {
            let count = if second % 3 == 2 {
                400
            } else {
                PACKETS_PER_SECOND + 100
            };
            for packet in 0..count {
                assert_eq!(
                    guard.accept(now + Duration::from_secs(second)),
                    Ok(packet < PACKETS_PER_SECOND)
                );
            }
        }
    }
}
