//! Whether one voice is speaking: voiced sound, with a pitch, standing above that voice's own
//! background noise. An open microphone always carries some room or fan noise; counting it
//! as speech would let the echo gates of two riders close each other's microphones in turn,
//! and show a rider speaking while they only sit by a fan.

use orqwa_domain::voice::{FRAME_SAMPLES, PcmFrame};
use std::time::{Duration, Instant};

/// Sound quieter than this is never treated as speech (about -46 dBFS).
const SPEECH_FLOOR: f32 = 0.005;
/// Below this (about -70 dBFS) a voice is digital silence: nobody speaking, or a friend
/// whose own gate is holding. It says nothing about their background noise.
const DIGITAL_SILENCE: f32 = 0.0003;
/// Speech stands about 10 dB above the voice's background noise.
const ABOVE_NOISE: f32 = 3.2;
/// Frames per noise-tracking block; the background is the quietest of the recent blocks.
const BLOCK_FRAMES: u8 = 15;
/// Each block ignores its quietest few frames: the fades and codec ramps around a friend's
/// gate opening or closing are briefly quieter than the room behind them.
const OUTLIERS: usize = 3;
/// Voiced sound repeats with its pitch: normalized correlation at one period, from 0 to 1.
const VOICED: f32 = 0.45;
/// Samples averaged into one for the pitch search: 8 kHz is plenty for 60–400 Hz voices.
const DECIMATE: usize = 6;
const PITCH_SAMPLES: usize = FRAME_SAMPLES / DECIMATE;
/// Pitch periods searched, in 8 kHz samples: 400 Hz down to 60 Hz.
const PERIODS: std::ops::RangeInclusive<usize> = 20..=133;
/// Consecutive loud frames that make speech, so a click is none. The echo gate decides
/// while a frame is still queued, about 40 ms before it plays, so waiting for a second
/// frame lets no echo through.
const ONSET: u8 = 2;
/// Blocks remembered: the background follows a change in steady noise within 1.5 s.
const BLOCKS: usize = 5;
/// A rider stays shown speaking between words, rather than flickering with each syllable.
const SPEAKING_HOLD: Duration = Duration::from_millis(500);

/// Follows one voice, 20 ms at a time.
pub(crate) struct Speech {
    blocks: [f32; BLOCKS],
    block: usize,
    quietest: [f32; OUTLIERS + 1],
    frames: u8,
    loud: u8,
    pitch: [f32; 2 * PITCH_SAMPLES],
}

impl Default for Speech {
    fn default() -> Self {
        Self {
            blocks: [0.0; BLOCKS],
            block: 0,
            quietest: [f32::MAX; OUTLIERS + 1],
            frames: 0,
            loud: 0,
            pitch: [0.0; 2 * PITCH_SAMPLES],
        }
    }
}

impl Speech {
    /// Notes the voice's next frame; whether it is speaking now.
    pub(crate) fn observe(&mut self, frame: &PcmFrame) -> bool {
        let energy: f32 = frame.iter().map(|sample| sample * sample).sum();
        #[allow(clippy::cast_precision_loss)] // the frame length is exactly representable
        let level = (energy / FRAME_SAMPLES as f32).sqrt();
        if !level.is_finite() {
            return false;
        }
        if level >= DIGITAL_SILENCE {
            if let Some(slot) = self.quietest.iter().position(|quiet| level < *quiet) {
                self.quietest.copy_within(slot..OUTLIERS, slot + 1);
                self.quietest[slot] = level;
            }
            self.frames += 1;
            if self.frames == BLOCK_FRAMES {
                self.blocks[self.block] = self.quietest[OUTLIERS];
                self.block = (self.block + 1) % BLOCKS;
                self.quietest = [f32::MAX; OUTLIERS + 1];
                self.frames = 0;
            }
        }
        let noise = self
            .blocks
            .iter()
            .copied()
            .fold(self.quietest[OUTLIERS], f32::min);
        self.remember(frame);
        // The pitch search is the costly part, and only loud frames need it.
        if level > SPEECH_FLOOR.max(noise * ABOVE_NOISE) && self.voiced() {
            self.loud = self.loud.saturating_add(1);
        } else {
            self.loud = 0;
        }
        self.loud >= ONSET
    }

    /// Keeps the last 40 ms at 8 kHz for the pitch search.
    fn remember(&mut self, frame: &PcmFrame) {
        self.pitch.copy_within(PITCH_SAMPLES.., 0);
        for (low, chunk) in self.pitch[PITCH_SAMPLES..]
            .iter_mut()
            .zip(frame.as_chunks::<DECIMATE>().0)
        {
            #[allow(clippy::cast_precision_loss)] // a small constant
            let average = chunk.iter().sum::<f32>() / DECIMATE as f32;
            *low = average;
        }
    }

    /// Whether the last 40 ms repeat at a voice's pitch, unlike fans, wind or hiss.
    fn voiced(&self) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let mean = self.pitch.iter().sum::<f32>() / self.pitch.len() as f32;
        let mut centred = [0.0; 2 * PITCH_SAMPLES];
        for (out, value) in centred.iter_mut().zip(self.pitch) {
            *out = value - mean;
        }
        PERIODS.into_iter().any(|period| {
            let (earlier, later) = (&centred[..centred.len() - period], &centred[period..]);
            let (mut dot, mut first, mut second) = (0.0, 0.0, 0.0);
            for (a, b) in earlier.iter().zip(later) {
                dot += a * b;
                first += a * a;
                second += b * b;
            }
            dot > VOICED * (first * second).sqrt() && dot > 0.0
        })
    }
}

/// Whether a rider is shown speaking, held between their words.
#[derive(Default)]
pub(crate) struct Speaking {
    speech: Speech,
    until: Option<Instant>,
}

impl Speaking {
    /// Notes the rider's frame heard or sent at `at`.
    pub(crate) fn observe(&mut self, frame: &PcmFrame, at: Instant) {
        if self.speech.observe(frame) {
            self.until = Some(at + SPEAKING_HOLD);
        }
    }

    /// Whether the rider spoke within the hold before `now`.
    pub(crate) fn at(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }
}

/// Sounds for the tests of what counts as speech.
#[cfg(test)]
pub(crate) mod sounds {
    use super::{FRAME_SAMPLES, PcmFrame};

    /// A voice or a motor's whine: a 220 Hz pitch with its harmonics.
    pub(crate) fn voice(amplitude: f32, frame: u32) -> PcmFrame {
        std::array::from_fn(|index| {
            #[allow(clippy::cast_precision_loss)]
            let t = (frame as usize * FRAME_SAMPLES + index) as f32 / 48_000.0;
            let phase = t * 220.0 * std::f32::consts::TAU;
            amplitude * (phase.sin() * 0.7 + (2.0 * phase).sin() * 0.3)
        })
    }
    /// A fan: broadband noise without a pitch.
    pub(crate) fn fan(amplitude: f32, frame: u32) -> PcmFrame {
        let mut noise = frame.wrapping_mul(747_796_405).wrapping_add(1);
        std::array::from_fn(|_| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            #[allow(clippy::cast_precision_loss)]
            let hiss = (noise >> 8) as f32 / (1 << 24) as f32 - 0.5;
            amplitude * hiss
        })
    }
}

#[cfg(test)]
mod tests {
    use super::sounds::{fan, voice};
    use super::*;

    fn at(start: Instant, frame: u32) -> Instant {
        start + Duration::from_millis(20) * frame
    }

    #[test]
    fn a_rider_is_shown_speaking_through_their_words_then_soon_not() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        let mut frame = 0;
        // Three words of 300 ms, 200 ms apart, over a quiet room.
        for _ in 0..3 {
            for _ in 0..15 {
                speaking.observe(&voice(0.15, frame), at(start, frame));
                frame += 1;
            }
            for _ in 0..10 {
                speaking.observe(&fan(0.01, frame), at(start, frame));
                frame += 1;
                assert!(
                    speaking.at(at(start, frame)),
                    "between words, frame {frame}"
                );
            }
        }
        for _ in 0..25 {
            speaking.observe(&fan(0.01, frame), at(start, frame));
            frame += 1;
        }
        assert!(
            !speaking.at(at(start, frame)),
            "half a second after the last word"
        );
    }

    #[test]
    fn a_click_or_a_fan_however_loud_is_nobody_speaking() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        speaking.observe(&voice(0.2, 0), at(start, 0));
        speaking.observe(&[0.0; FRAME_SAMPLES], at(start, 1));
        assert!(!speaking.at(at(start, 2)), "a single loud frame");
        let mut speaking = Speaking::default();
        for frame in 0..200 {
            speaking.observe(&fan(0.35, frame), at(start, frame));
            assert!(!speaking.at(at(start, frame)), "fan, frame {frame}");
        }
    }
}
