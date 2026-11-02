//! A narrow, bounded interface to the bundled hardened Opus implementation.

use opusic_c::{Application, Bitrate, Channels, Decoder, Encoder, InbandFec, SampleRate, Signal};
use orqwa_domain::voice::{FRAME_SAMPLES, PcmFrame};
use orqwa_net::state::RiderId;
use orqwa_net::voice::{MAX_AUDIO_BYTES, VoiceFrame};

/// Invalid speech data or an unavailable codec. Audio is never included in diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("The speech frame could not be encoded or decoded.")]
pub struct CodecError;

/// Constant speech bitrate: clear super-wideband voice with room for loss recovery data.
const BITRATE: u32 = 32_000;
/// Hard CBR without DTX gives precisely this many compressed bytes per 20 ms frame.
pub const PACKET_BYTES: usize = 80;
/// Loss the encoder plans recovery data for. Opus adds none at all at 5 % or less.
const EXPECTED_LOSS_PERCENT: u8 = 10;

/// One mono, constant-rate 32 kbit/s encoder, reused across every recipient.
pub struct SpeechEncoder {
    inner: Encoder,
    spurt: u32,
    sequence: u32,
}
impl SpeechEncoder {
    /// Creates the bounded 20 ms voice configuration with native hardening left enabled.
    /// Each packet also carries a coarse copy of the previous frame, so a single lost
    /// packet is rebuilt from the next one instead of being guessed.
    ///
    /// # Errors
    /// Returns [`CodecError`] if the codec cannot be initialized.
    pub fn new() -> Result<Self, CodecError> {
        let mut inner = Encoder::new(Channels::Mono, SampleRate::Hz48000, Application::Voip)
            .map_err(|_| CodecError)?;
        inner
            .set_bitrate(Bitrate::Value(BITRATE))
            .map_err(|_| CodecError)?;
        inner.set_vbr(false).map_err(|_| CodecError)?;
        inner.set_dtx(false).map_err(|_| CodecError)?;
        // One voice stream costs a few percent of a core even at the best analysis.
        inner.set_complexity(10).map_err(|_| CodecError)?;
        inner.set_signal(Signal::Voice).map_err(|_| CodecError)?;
        inner
            .set_inband_fec(InbandFec::Mode1)
            .map_err(|_| CodecError)?;
        inner
            .set_packet_loss(EXPECTED_LOSS_PERCENT)
            .map_err(|_| CodecError)?;
        Ok(Self {
            inner,
            spurt: 0,
            sequence: 0,
        })
    }
    /// Encodes finite samples into a fixed payload. Activation changes reset codec prediction.
    ///
    /// # Errors
    /// Returns [`CodecError`] for invalid samples, activation, exhaustion or codec failure.
    pub fn encode(
        &mut self,
        samples: &PcmFrame,
        speaker: RiderId,
        spurt: u32,
    ) -> Result<VoiceFrame, CodecError> {
        if spurt == 0
            || samples
                .iter()
                .any(|v| !v.is_finite() || !(-1.0..=1.0).contains(v))
        {
            return Err(CodecError);
        }
        if spurt != self.spurt {
            self.inner.reset().map_err(|_| CodecError)?;
            self.spurt = spurt;
            self.sequence = 0;
        }
        let mut bytes = [0; MAX_AUDIO_BYTES];
        let len = self
            .inner
            .encode_float_to_slice(samples, &mut bytes)
            .map_err(|_| CodecError)?;
        if len != PACKET_BYTES {
            return Err(CodecError);
        }
        let frame = VoiceFrame::new(speaker, spurt, self.sequence, &bytes[..len])
            .map_err(|_| CodecError)?;
        self.sequence = self.sequence.checked_add(1).ok_or(CodecError)?;
        Ok(frame)
    }
    /// Spends one sequence number on a captured frame that was too old to send, so listeners
    /// see the gap and conceal it instead of splicing the speech either side together.
    ///
    /// # Errors
    /// Returns [`CodecError`] once the activation's sequence counter is exhausted.
    pub fn skip(&mut self, spurt: u32) -> Result<(), CodecError> {
        if spurt == self.spurt {
            self.sequence = self.sequence.checked_add(1).ok_or(CodecError)?;
        }
        Ok(())
    }
}

/// One speaker's decoder. No decoder state is shared between riders.
pub struct SpeechDecoder {
    inner: Decoder,
    spurt: u32,
}
impl SpeechDecoder {
    /// Initializes one mono decoder with a fixed 960-sample output capacity.
    ///
    /// # Errors
    /// Returns [`CodecError`] when the codec cannot be initialized.
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            inner: Decoder::new(Channels::Mono, SampleRate::Hz48000).map_err(|_| CodecError)?,
            spurt: 0,
        })
    }
    /// Rejects stereo and packets representing anything other than exactly twenty ms before
    /// decoding. Uses fixed slices rather than the wrapper's growable-vector helpers.
    ///
    /// # Errors
    /// Returns [`CodecError`] for malformed/composite-duration/stereo data or decoder failure.
    pub fn decode(&mut self, frame: &VoiceFrame) -> Result<PcmFrame, CodecError> {
        let bytes = checked(frame)?;
        if self.spurt != frame.spurt() {
            self.inner.reset().map_err(|_| CodecError)?;
            self.spurt = frame.spurt();
        }
        self.decode_bytes(bytes, false)
    }
    /// Rebuilds a lost frame from the recovery copy carried by the frame that follows it.
    /// The following frame itself is decoded normally afterwards.
    ///
    /// # Errors
    /// Returns [`CodecError`] for malformed/composite-duration/stereo data or decoder failure.
    pub fn recover(&mut self, following: &VoiceFrame) -> Result<PcmFrame, CodecError> {
        let bytes = checked(following)?;
        if self.spurt != following.spurt() {
            return self.conceal();
        }
        self.decode_bytes(bytes, true)
    }
    /// Conceals one lost twenty-millisecond frame; callers bound concealment to three frames.
    ///
    /// # Errors
    /// Returns [`CodecError`] if the decoder cannot conceal the frame.
    pub fn conceal(&mut self) -> Result<PcmFrame, CodecError> {
        self.decode_bytes(&[], false)
    }
    fn decode_bytes(&mut self, bytes: &[u8], recovery: bool) -> Result<PcmFrame, CodecError> {
        let mut samples = [0.0; FRAME_SAMPLES];
        let len = self
            .inner
            .decode_float_to_slice(bytes, &mut samples, recovery)
            .map_err(|_| CodecError)?;
        if len != FRAME_SAMPLES || samples.iter().any(|s| !s.is_finite()) {
            return Err(CodecError);
        }
        for sample in &mut samples {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(samples)
    }
}

fn checked(frame: &VoiceFrame) -> Result<&[u8], CodecError> {
    let bytes = frame.payload();
    if bytes[0] & 4 != 0
        || opusic_c::utils::get_nb_samples(bytes, SampleRate::Hz48000).map_err(|_| CodecError)?
            != FRAME_SAMPLES
    {
        return Err(CodecError);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_mono_speech_round_trips_and_rejects_wrong_durations_and_stereo() {
        let mut encoder = SpeechEncoder::new().unwrap();
        let mut decoder = SpeechDecoder::new().unwrap();
        let samples = std::array::from_fn(|index| {
            #[allow(clippy::cast_precision_loss)]
            let t = index as f32 / 48_000.0;
            (t * 440.0 * std::f32::consts::TAU).sin() * 0.2
        });
        let frame = encoder.encode(&samples, RiderId::HOST, 1).unwrap();
        assert_eq!(frame.payload().len(), PACKET_BYTES);
        let recovered = decoder.decode(&frame).unwrap();
        assert!(recovered.iter().any(|v| v.abs() > 0.01));
        let mut stereo = frame.payload().to_vec();
        stereo[0] |= 4;
        assert!(
            decoder
                .decode(&VoiceFrame::new(RiderId::HOST, 1, 1, &stereo).unwrap())
                .is_err()
        );
        assert!(
            decoder
                .decode(&VoiceFrame::new(RiderId::HOST, 1, 1, &[0xff]).unwrap())
                .is_err()
        );
        assert!(
            encoder
                .encode(&[f32::NAN; FRAME_SAMPLES], RiderId::HOST, 1)
                .is_err()
        );
    }

    #[test]
    fn a_lost_packet_is_rebuilt_from_the_next_closer_than_a_guess() {
        // Half a second of room hiss, then a voiced sound whose pitch and loudness keep
        // moving, as speech does.
        let mut hiss = 1_u32;
        let speech: Vec<PcmFrame> = (0..60)
            .map(|frame| {
                std::array::from_fn(|index| {
                    hiss = hiss.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    #[allow(clippy::cast_precision_loss)]
                    let noise = ((hiss >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.002;
                    if frame < 25 {
                        return noise;
                    }
                    #[allow(clippy::cast_precision_loss)]
                    let t = (frame * FRAME_SAMPLES + index) as f32 / 48_000.0;
                    let pitch = 140.0 + 60.0 * (t * 3.0).sin();
                    let loudness = 0.15 + 0.1 * (t * 7.0).sin();
                    let phase = t * pitch * std::f32::consts::TAU;
                    #[allow(clippy::cast_precision_loss)]
                    let voice: f32 = (1..=24)
                        .map(|harmonic| (harmonic as f32 * phase).sin() / harmonic as f32)
                        .sum();
                    loudness * voice * 0.5 + noise
                })
            })
            .collect();
        let mut encoder = SpeechEncoder::new().unwrap();
        let packets: Vec<_> = speech
            .iter()
            .map(|samples| encoder.encode(samples, RiderId::HOST, 1).unwrap())
            .collect();
        let lost = 50;
        let error = |rebuild: &dyn Fn(&mut SpeechDecoder) -> PcmFrame| {
            let mut reference = SpeechDecoder::new().unwrap();
            let mut decoder = SpeechDecoder::new().unwrap();
            for packet in &packets[..lost] {
                reference.decode(packet).unwrap();
                decoder.decode(packet).unwrap();
            }
            let expected = reference.decode(&packets[lost]).unwrap();
            let rebuilt = rebuild(&mut decoder);
            expected
                .iter()
                .zip(rebuilt)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f32>()
        };
        let recovered = error(&|decoder| decoder.recover(&packets[lost + 1]).unwrap());
        let concealed = error(&|decoder| decoder.conceal().unwrap());
        assert!(
            recovered * 2.0 < concealed,
            "recovered {recovered}, concealed {concealed}"
        );
    }
}
