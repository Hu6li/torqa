//! Band-limited conversion between native device rates and 48 kHz speech.
//!
//! Picking the nearest sample, or blending two neighbours, folds everything above the slower
//! rate's Nyquist frequency back into the audible band: Opus' upper band rang metallic on
//! 16/24 kHz Bluetooth headsets. A windowed-sinc kernel filters before it decimates.

use std::f64::consts::PI;

/// Kernel rows between two input samples; neighbouring rows are blended linearly.
const PHASES: usize = 64;
/// Kernel half-width in input samples at equal or rising rates.
const HALF_WIDTH: f64 = 24.0;
/// Keeps the transition band just below the slower rate's Nyquist frequency.
const CUTOFF: f64 = 0.9;

/// Streams mono samples from one rate to another. All memory is allocated by [`Self::new`],
/// so audio callbacks only read and write fixed buffers.
pub(crate) struct Resampler {
    kernel: Option<Kernel>,
}

struct Kernel {
    step: f64,
    table: Box<[f32]>,
    history: Box<[f32]>,
    pushed: u64,
    base: u64,
    fraction: f64,
}

impl Resampler {
    /// Equal rates pass samples through untouched.
    pub(crate) fn new(from: u32, to: u32) -> Self {
        if from == to || from == 0 || to == 0 {
            return Self { kernel: None };
        }
        let step = f64::from(from) / f64::from(to);
        let stretch = step.max(1.0);
        let cutoff = CUTOFF / stretch;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // validated device rates keep the width between 24 and 576 samples
        let half = (HALF_WIDTH * stretch).ceil() as usize;
        let taps = 2 * half;
        let mut table = vec![0.0; (PHASES + 1) * taps].into_boxed_slice();
        for (phase, row) in table.chunks_exact_mut(taps).enumerate() {
            #[allow(clippy::cast_precision_loss)] // small table indices
            let fraction = phase as f64 / PHASES as f64;
            let mut sum = 0.0;
            for (tap, value) in row.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)] // small table indices
                let distance = fraction + (half - 1) as f64 - tap as f64;
                #[allow(clippy::cast_precision_loss)]
                let x = distance / half as f64;
                let window = if x.abs() >= 1.0 {
                    0.0
                } else {
                    0.42 + 0.5 * (PI * x).cos() + 0.08 * (2.0 * PI * x).cos()
                };
                let coefficient = cutoff * sinc(cutoff * distance) * window;
                sum += coefficient;
                #[allow(clippy::cast_possible_truncation)] // coefficients are below one
                {
                    *value = coefficient as f32;
                }
            }
            // Unity gain at every phase keeps steady tones free of a phase-dependent ripple.
            #[allow(clippy::cast_possible_truncation)]
            let sum = sum as f32;
            for value in row {
                *value /= sum;
            }
        }
        Self {
            kernel: Some(Kernel {
                step,
                table,
                history: vec![0.0; taps].into_boxed_slice(),
                pushed: 0,
                base: 0,
                fraction: 0.0,
            }),
        }
    }

    /// Feeds one input sample and emits every output sample it completes.
    pub(crate) fn push(&mut self, sample: f32, mut emit: impl FnMut(f32)) {
        let Some(kernel) = &mut self.kernel else {
            emit(sample);
            return;
        };
        kernel.store(sample);
        while kernel.ready() {
            emit(kernel.next());
        }
    }

    /// Produces one output sample, taking as many input samples as it needs.
    pub(crate) fn pull(&mut self, mut source: impl FnMut() -> f32) -> f32 {
        let Some(kernel) = &mut self.kernel else {
            return source();
        };
        while !kernel.ready() {
            kernel.store(source());
        }
        kernel.next()
    }

    /// Forgets earlier samples, so silence or a new stream never rings with old speech.
    pub(crate) fn reset(&mut self) {
        if let Some(kernel) = &mut self.kernel {
            kernel.history.fill(0.0);
            kernel.pushed = 0;
            kernel.base = 0;
            kernel.fraction = 0.0;
        }
    }
}

impl Kernel {
    fn taps(&self) -> u64 {
        self.history.len() as u64
    }
    fn store(&mut self, sample: f32) {
        #[allow(clippy::cast_possible_truncation)] // the remainder is below the tap count
        let slot = (self.pushed % self.taps()) as usize;
        self.history[slot] = sample;
        self.pushed += 1;
    }
    fn ready(&self) -> bool {
        // Output needs half a kernel of input after its own position.
        self.pushed > self.base + self.taps() / 2
    }
    fn next(&mut self) -> f32 {
        let taps = self.history.len();
        #[allow(clippy::cast_precision_loss)]
        let position = self.fraction * PHASES as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // the fraction stays in [0, 1)
        let row = (position as usize).min(PHASES - 1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let blend = (position - row as f64) as f32;
        let lower = &self.table[row * taps..(row + 1) * taps];
        let upper = &self.table[(row + 1) * taps..(row + 2) * taps];
        // The ring holds exactly the kernel's window; its oldest sample is at `pushed`.
        #[allow(clippy::cast_possible_truncation)] // the remainder is below the tap count
        let oldest = (self.pushed % self.taps()) as usize;
        let (newer, older) = self.history.split_at(oldest);
        let mut sum = 0.0;
        for ((sample, low), high) in older.iter().chain(newer).zip(lower).zip(upper) {
            sum += sample * (low + (high - low) * blend);
        }
        self.fraction += self.step;
        let whole = self.fraction.floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // at most the rate ratio, which validation bounds
        {
            self.base += whole as u64;
        }
        self.fraction -= whole;
        sum
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn tone(rate: u32, frequency: f64, amplitude: f64, index: usize) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let value =
            (amplitude * (2.0 * PI * frequency * index as f64 / f64::from(rate)).sin()) as f32;
        value
    }

    fn convert(from: u32, to: u32, input: impl Iterator<Item = f32>) -> Vec<f32> {
        let mut resampler = Resampler::new(from, to);
        let mut output = Vec::new();
        for sample in input {
            resampler.push(sample, |value| output.push(value));
        }
        output
    }

    fn rms(samples: &[f32]) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let count = samples.len() as f64;
        (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / count).sqrt()
    }

    #[test]
    fn every_device_rate_converts_to_and_from_48_khz_at_the_right_pace() {
        for rate in [
            8000, 16_000, 22_050, 24_000, 44_100, 48_000, 96_000, 192_000,
        ] {
            let up = convert(rate, 48_000, std::iter::repeat_n(0.25, rate as usize));
            assert!(
                up.len().abs_diff(48_000) <= 600,
                "{rate} Hz in: {}",
                up.len()
            );
            let mut output = Resampler::new(48_000, rate);
            let mut taken = 0_usize;
            for _ in 0..rate {
                let value = output.pull(|| {
                    taken += 1;
                    0.25
                });
                assert!(value.is_finite());
            }
            assert!(taken.abs_diff(48_000) <= 600, "{rate} Hz out: {taken}");
            assert!(
                up[up.len() / 2..].iter().all(|s| (s - 0.25).abs() < 0.001),
                "steady level at {rate} Hz"
            );
        }
    }

    #[test]
    fn speech_band_tones_keep_their_shape_across_rates() {
        for (from, to) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (16_000, 48_000),
            (48_000, 16_000),
        ] {
            let output = convert(
                from,
                to,
                (0..from as usize).map(|i| tone(from, 1000.0, 0.5, i)),
            );
            // Each output sample stands for the input instant `index * from / to`.
            let errors: Vec<f32> = output
                .iter()
                .enumerate()
                .skip(1000)
                .take(output.len() - 2000)
                .map(|(index, value)| value - tone(to, 1000.0, 0.5, index))
                .collect();
            assert!(rms(&errors) < 0.002, "{from} → {to}: {}", rms(&errors));
        }
    }

    #[test]
    fn bluetooth_headset_rates_do_not_fold_high_speech_into_a_metallic_ring() {
        // Opus' super-wideband speech reaches 12 kHz; a 16/24 kHz headset cannot play it.
        for (to, frequency) in [(16_000, 12_000.0), (24_000, 15_000.0), (44_100, 23_000.0)] {
            let output = convert(
                48_000,
                to,
                (0..48_000).map(|i| tone(48_000, frequency, 0.5, i)),
            );
            let level = rms(&output[1000..]);
            assert!(level < 0.005, "{frequency} Hz at {to} Hz output: {level}");
        }
    }

    #[test]
    fn a_reset_never_rings_with_earlier_samples() {
        let mut resampler = Resampler::new(48_000, 44_100);
        for index in 0..4800 {
            resampler.push(tone(48_000, 1000.0, 0.5, index), |_| {});
        }
        resampler.reset();
        let mut output = Vec::new();
        for _ in 0..4800 {
            resampler.push(0.0, |value| output.push(value));
        }
        assert!(output.iter().all(|value| *value == 0.0));
    }
}
