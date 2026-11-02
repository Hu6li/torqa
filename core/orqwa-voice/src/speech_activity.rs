//! A local speech-activity estimate; no recognition, recording or network access.

use orqwa_domain::voice::{FRAME_SAMPLES, PcmFrame};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct SpeechActivity {
    previous: f32,
    high_pass: f32,
    band: f32,
    candidate: Option<Instant>,
    last_voice: Option<Instant>,
    seen: bool,
}

impl SpeechActivity {
    pub(crate) fn observe(&mut self, samples: &PcmFrame, now: Instant) {
        let mut energy = 0.0_f32;
        let mut crossings = 0;
        for sample in samples {
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.high_pass = 0.98 * (self.high_pass + sample - self.previous);
            self.previous = sample;
            let previous = self.band;
            self.band += 0.4 * (self.high_pass - self.band);
            energy += self.band * self.band;
            if previous.is_sign_positive() != self.band.is_sign_positive() {
                crossings += 1;
            }
        }
        #[allow(clippy::cast_precision_loss)] // fixed frame length is exactly representable
        let rms = (energy / FRAME_SAMPLES as f32).sqrt();
        // Reject silence, DC, low mains hum and isolated clicks; this remains an estimate,
        // so other sounds with speech-like energy can also trigger the indicator.
        if rms >= 0.003 && (3..=320).contains(&crossings) {
            let since = *self.candidate.get_or_insert(now);
            if now.saturating_duration_since(since) >= Duration::from_millis(40) {
                self.last_voice = Some(now);
                self.seen = true;
            }
        } else {
            self.candidate = None;
        }
    }
    pub(crate) fn detected(&self, now: Instant) -> bool {
        self.last_voice
            .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(300))
    }
    pub(crate) fn seen(&self) -> bool {
        self.seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speech_activity_is_visible_then_releases_while_the_result_is_kept() {
        let mut detector = SpeechActivity::default();
        let now = Instant::now();
        for frame in 0..10 {
            let samples = std::array::from_fn(|sample| {
                #[allow(clippy::cast_precision_loss)] // short synthetic speech-like fixture
                let time = (frame * FRAME_SAMPLES + sample) as f32 / 48_000.0;
                0.04 * (time * 180.0 * std::f32::consts::TAU).sin()
                    + 0.02 * (time * 540.0 * std::f32::consts::TAU).sin()
            });
            detector.observe(
                &samples,
                now + Duration::from_millis(u64::try_from(frame).unwrap() * 20),
            );
        }
        assert!(detector.detected(now + Duration::from_millis(200)));
        assert!(!detector.detected(now + Duration::from_secs(1)));
        assert!(detector.seen());
    }
    #[test]
    fn silence_dc_hum_and_invalid_samples_do_not_report_voice() {
        let now = Instant::now();
        for constant in [0.0, 0.2, f32::NAN] {
            let mut detector = SpeechActivity::default();
            for frame in 0..50 {
                detector.observe(
                    &[constant; FRAME_SAMPLES],
                    now + Duration::from_millis(frame * 20),
                );
            }
            assert!(!detector.seen());
        }
        let mut detector = SpeechActivity::default();
        for frame in 0..50 {
            let samples = std::array::from_fn(|sample| {
                #[allow(clippy::cast_precision_loss)]
                let time = (frame * FRAME_SAMPLES + sample) as f32 / 48_000.0;
                0.2 * (time * 50.0 * std::f32::consts::TAU).sin()
            });
            detector.observe(
                &samples,
                now + Duration::from_millis(u64::try_from(frame).unwrap() * 20),
            );
        }
        assert!(!detector.seen());
    }
}
