//! Audio device plugin boundary. Capture and playback have no trainer or recording access.

use std::time::Instant;

/// Voice processing uses mono 48 kHz, independently of the hardware's native rate.
pub const SAMPLE_RATE: u32 = 48_000;
/// One twenty-millisecond mono frame.
pub const FRAME_SAMPLES: usize = 960;
/// Normalized mono samples, finite and limited to the range -1 through 1.
pub type PcmFrame = [f32; FRAME_SAMPLES];

/// A captured frame bound to one explicit microphone activation.
pub struct CapturedVoice {
    /// The activation requested when this microphone stream was opened.
    pub activation: u32,
    /// When the complete frame was captured; stale frames must not be transmitted.
    pub at: Instant,
    /// Twenty milliseconds of microphone samples.
    pub samples: PcmFrame,
}

/// A local device error, containing no audio samples.
#[derive(Debug, Clone)]
pub struct VoiceDeviceError {
    operation: &'static str,
    details: String,
}
impl VoiceDeviceError {
    /// Retains the backend's original cause under the operation that failed, without audio data.
    pub fn new(operation: &'static str, cause: impl std::fmt::Display) -> Self {
        Self {
            operation,
            details: cause.to_string(),
        }
    }
}
impl std::fmt::Display for VoiceDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.operation, self.details)
    }
}
impl std::error::Error for VoiceDeviceError {}

/// Replaceable local audio backend. All methods run on its owning audio thread.
/// Implementations start with no microphone open, have bounded queues and release streams
/// when dropped. Device callbacks must not allocate per sample or wait for network work.
pub trait VoiceAudio {
    /// Opens this activation, or closes capture and discards queued samples for `None`.
    ///
    /// # Errors
    /// Returns [`VoiceDeviceError`] when capture cannot be opened safely.
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError>;
    /// Takes one bounded captured frame, without waiting for one.
    fn capture(&mut self) -> Option<CapturedVoice>;
    /// Queues one playback frame; a full queue drops it instead of blocking.
    fn play(&mut self, frame: PcmFrame);
    /// Silences output and clears pending speech; reopening must never replay it.
    fn deafen(&mut self, deafened: bool);
    /// Reports asynchronous device failures without panicking in callbacks.
    fn healthy(&self) -> bool;
    /// Takes the diagnostic for an asynchronous device failure. Backends without a more
    /// specific cause still report the operation without exposing microphone samples.
    fn failure(&mut self) -> VoiceDeviceError {
        VoiceDeviceError::new(
            "voice audio callback",
            "the audio device stopped responding",
        )
    }
}
