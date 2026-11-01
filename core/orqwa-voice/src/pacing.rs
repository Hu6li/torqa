//! Live media deadlines: missed ticks never become catch-up transmissions.

use std::time::{Duration, Instant};

use orqwa_net::voice::FRAME_TIME;

pub(super) struct Pacer {
    next: Instant,
}

impl Pacer {
    pub(super) fn new(now: Instant) -> Self {
        Self { next: now }
    }

    pub(super) fn due(&mut self, now: Instant) -> bool {
        if now < self.next {
            return false;
        }
        self.defer(now);
        true
    }

    pub(super) fn defer(&mut self, now: Instant) {
        self.next = now + FRAME_TIME;
    }

    pub(super) fn wait(&self, now: Instant) -> Duration {
        self.next.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_wakeups_and_control_wakeups_never_replay_missed_ticks() {
        let now = Instant::now();
        let mut pacer = Pacer::new(now);
        assert!(pacer.due(now));
        assert!(!pacer.due(now + Duration::from_millis(5)));
        assert!(pacer.due(now + Duration::from_millis(39)));
        assert!(!pacer.due(now + Duration::from_millis(40)));
        assert_eq!(
            pacer.wait(now + Duration::from_millis(40)),
            Duration::from_millis(19)
        );
        assert!(pacer.due(now + Duration::from_secs(2)));
        assert!(!pacer.due(now + Duration::from_secs(2)));
        assert!(pacer.due(now + Duration::from_secs(2) + FRAME_TIME));
    }
}
