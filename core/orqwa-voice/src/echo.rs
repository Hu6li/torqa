//! Speaker mode uses a bounded half-duplex gate rather than claiming acoustic cancellation.
//!
//! The gate listens for a friend's *speech* in what is about to play: voiced sound, with a
//! pitch, standing above that playback's own background noise. An open microphone on the
//! other side always carries some room or fan noise; holding the local microphone for that
//! noise would let both riders' gates close each other's microphones in turn.

use orqwa_domain::voice::{FRAME_SAMPLES, PcmFrame};
use std::time::{Duration, Instant};

/// Playback quieter than this is never treated as speech (about -46 dBFS).
const SPEECH_FLOOR: f32 = 0.005;
/// Below this (about -70 dBFS) playback is digital silence: nobody speaking, or a friend
/// whose own gate is holding. It says nothing about their background noise.
const DIGITAL_SILENCE: f32 = 0.0003;
/// Speech stands about 10 dB above the playback's background noise.
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
/// Consecutive loud frames that make speech. The gate decides while a frame is still
/// queued, about 40 ms before it plays, so waiting for a second frame lets no echo through.
const ONSET: u8 = 2;
/// Blocks remembered: the background follows a change in steady noise within 1.5 s.
const BLOCKS: usize = 5;
/// After speech stops its echo still has to leave the queue, the speaker and the room.
const TAIL: Duration = Duration::from_millis(300);
/// Gain change per sample: closing within 5 ms, opening over 20 ms, without clicks.
const CLOSE_STEP: f32 = 1.0 / 240.0;
const OPEN_STEP: f32 = 1.0 / 960.0;

pub(crate) struct SpeakerGate {
    blocks: [f32; BLOCKS],
    block: usize,
    quietest: [f32; OUTLIERS + 1],
    frames: u8,
    loud: u8,
    pitch: [f32; 2 * PITCH_SAMPLES],
    until: Option<Instant>,
    gain: f32,
}

impl SpeakerGate {
    pub(crate) fn new() -> Self {
        Self {
            blocks: [0.0; BLOCKS],
            block: 0,
            quietest: [f32::MAX; OUTLIERS + 1],
            frames: 0,
            loud: 0,
            pitch: [0.0; 2 * PITCH_SAMPLES],
            until: None,
            gain: 1.0,
        }
    }

    /// Notes one frame that is about to play.
    pub(crate) fn observe(&mut self, playback: &PcmFrame, now: Instant) {
        let energy: f32 = playback.iter().map(|sample| sample * sample).sum();
        #[allow(clippy::cast_precision_loss)] // the frame length is exactly representable
        let level = (energy / FRAME_SAMPLES as f32).sqrt();
        if !level.is_finite() {
            return;
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
        let voiced = self.voiced(playback);
        if voiced && level > SPEECH_FLOOR.max(noise * ABOVE_NOISE) {
            self.loud = self.loud.saturating_add(1);
            if self.loud >= ONSET {
                self.until = Some(now + TAIL);
            }
        } else {
            self.loud = 0;
        }
    }

    /// Whether the last 40 ms repeat at a voice's pitch, unlike fans, wind or hiss.
    fn voiced(&mut self, playback: &PcmFrame) -> bool {
        self.pitch.copy_within(PITCH_SAMPLES.., 0);
        for (low, chunk) in self.pitch[PITCH_SAMPLES..]
            .iter_mut()
            .zip(playback.as_chunks::<DECIMATE>().0)
        {
            #[allow(clippy::cast_precision_loss)] // a small constant
            let average = chunk.iter().sum::<f32>() / DECIMATE as f32;
            *low = average;
        }
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

    /// A newly opened microphone fades in, and stays closed if a friend is already talking.
    pub(crate) fn restart(&mut self) {
        self.gain = 0.0;
    }

    /// Fades captured samples out while a friend's speech or its echo is audible.
    pub(crate) fn apply(&mut self, samples: &mut PcmFrame, captured: Instant) {
        let held = self.until.is_some_and(|until| captured < until);
        for sample in samples {
            self.gain = if held {
                (self.gain - CLOSE_STEP).max(0.0)
            } else {
                (self.gain + OPEN_STEP).min(1.0)
            };
            *sample *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voice or a motor's whine: a 220 Hz pitch with its harmonics.
    fn voice(amplitude: f32, frame: u32) -> PcmFrame {
        std::array::from_fn(|index| {
            #[allow(clippy::cast_precision_loss)]
            let t = (frame as usize * FRAME_SAMPLES + index) as f32 / 48_000.0;
            let phase = t * 220.0 * std::f32::consts::TAU;
            amplitude * (phase.sin() * 0.7 + (2.0 * phase).sin() * 0.3)
        })
    }
    /// A fan: broadband noise without a pitch.
    fn fan(amplitude: f32, frame: u32) -> PcmFrame {
        let mut noise = frame.wrapping_mul(747_796_405).wrapping_add(1);
        std::array::from_fn(|_| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            #[allow(clippy::cast_precision_loss)]
            let hiss = (noise >> 8) as f32 / (1 << 24) as f32 - 0.5;
            amplitude * hiss
        })
    }
    fn passes(gate: &mut SpeakerGate, at: Instant) -> bool {
        let mut samples = [0.5; FRAME_SAMPLES];
        gate.apply(&mut samples, at);
        samples[FRAME_SAMPLES - 1] > 0.49
    }
    fn at(start: Instant, frame: u32) -> Instant {
        start + Duration::from_millis(20) * frame
    }

    #[test]
    fn a_friends_speech_and_its_echo_tail_are_never_retransmitted() {
        let start = Instant::now();
        let mut gate = SpeakerGate::new();
        for index in 0..10 {
            gate.observe(&[0.0; FRAME_SAMPLES], at(start, index));
            assert!(passes(&mut gate, at(start, index)));
        }
        // A single loud frame, such as a click, does not hold the microphone.
        gate.observe(&voice(0.2, 10), at(start, 10));
        assert!(passes(&mut gate, at(start, 10)));
        gate.observe(&voice(0.2, 11), at(start, 11));
        let mut samples = [0.5; FRAME_SAMPLES];
        gate.apply(&mut samples, at(start, 11));
        assert!(samples[FRAME_SAMPLES - 1].abs() < f32::EPSILON);
        // No click: the gain moves by a small step per sample.
        assert!(
            samples
                .windows(2)
                .all(|pair| (pair[0] - pair[1]).abs() <= 0.5 / 200.0)
        );
        gate.observe(&[0.0; FRAME_SAMPLES], at(start, 12));
        assert!(!passes(&mut gate, at(start, 25)));
        // Opening fades in over one frame.
        let _ = passes(&mut gate, at(start, 27));
        assert!(passes(&mut gate, at(start, 28)));
    }

    #[test]
    fn a_friends_fan_never_closes_the_microphone_however_loud() {
        let start = Instant::now();
        let mut gate = SpeakerGate::new();
        for index in 0..100 {
            // From silence straight to a fan at about -20 dBFS.
            let frame = if index < 10 {
                [0.0; FRAME_SAMPLES]
            } else {
                fan(0.35, index)
            };
            gate.observe(&frame, at(start, index));
            assert!(passes(&mut gate, at(start, index)), "frame {index}");
        }
    }

    #[test]
    fn a_steady_whine_is_learned_as_background_and_speech_still_stands_out() {
        let start = Instant::now();
        let mut gate = SpeakerGate::new();
        for index in 0..50 {
            gate.observe(&[0.0; FRAME_SAMPLES], at(start, index));
        }
        // A trainer's whine has a pitch, so at first it holds like speech; within 1.5 s it
        // is known as background.
        for index in 50..200 {
            gate.observe(&voice(0.02, index), at(start, index));
        }
        assert!(passes(&mut gate, at(start, 200)));
        assert!(passes(&mut gate, at(start, 201)));
        // Their speech over the same whine still holds the microphone.
        gate.observe(&voice(0.15, 201), at(start, 201));
        gate.observe(&voice(0.15, 202), at(start, 202));
        assert!(!passes(&mut gate, at(start, 203)));
        // While their gate holds for us, silence arrives; their whine afterwards is still
        // known, so we can answer at once.
        for index in 203..300 {
            gate.observe(&[0.0; FRAME_SAMPLES], at(start, index));
        }
        // Their gate fades in: a frame quieter than their room, then the room itself.
        gate.observe(&voice(0.004, 300), at(start, 300));
        for index in 301..320 {
            gate.observe(&voice(0.02, index), at(start, index));
        }
        assert!(passes(&mut gate, at(start, 320)));
    }
}
