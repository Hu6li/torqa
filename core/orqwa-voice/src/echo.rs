//! Speaker mode uses a bounded half-duplex gate rather than claiming acoustic cancellation.

use orqwa_domain::voice::PcmFrame;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct SpeakerGate {
    until: Option<Instant>,
}

impl SpeakerGate {
    pub(crate) fn allow(&mut self, playback: &PcmFrame, now: Instant) -> bool {
        if playback.iter().any(|sample| sample.abs() > 0.005) {
            // Cover the bounded output queue plus the speaker/room's short echo tail.
            self.until = Some(now + Duration::from_millis(300));
        }
        self.until.is_none_or(|until| now >= until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speaker_playback_and_its_tail_are_never_retransmitted() {
        let now = Instant::now();
        let mut gate = SpeakerGate::default();
        assert!(gate.allow(&[0.0; 960], now));
        assert!(!gate.allow(&[0.1; 960], now));
        assert!(!gate.allow(&[0.0; 960], now + Duration::from_millis(299)));
        assert!(gate.allow(&[0.0; 960], now + Duration::from_millis(300)));
    }
}
