//! Temporary local microphone and speaker checks, without recording or network access.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use orqwa_domain::voice::{VoiceAudio, VoiceDeviceError};
use tokio::sync::{mpsc, oneshot};

use crate::speech_activity::SpeechActivity;
use crate::{
    VoiceError,
    device::{DeviceOptions, NativeAudio},
};

/// One explicitly requested local audio check; only one device direction is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TestKind {
    /// Capture for ten seconds, displaying level and estimated speech activity.
    Microphone = 1,
    /// Play two short tones for two seconds, without opening capture.
    Speakers = 2,
}

struct Controls {
    stop: Arc<AtomicBool>,
    level: AtomicU32,
    voice: AtomicBool,
    heard: AtomicBool,
    volume: AtomicU32,
}

/// A bounded local audio check. Microphone checks never play captured samples;
/// speaker checks never open capture. Dropping it stops its device immediately.
pub struct AudioTest {
    controls: Arc<Controls>,
    thread: Option<JoinHandle<()>>,
    errors: mpsc::Receiver<VoiceDeviceError>,
}

impl AudioTest {
    /// Opens only the selected microphone after the rider explicitly requests a test.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for device permission, worker or startup failures.
    pub async fn open(options: DeviceOptions) -> Result<Self, VoiceError> {
        Self::with_audio(move |stop| Ok(Box::new(NativeAudio::capture_only(options, stop)))).await
    }

    /// Uses a replaceable capture backend; useful for tests without a real microphone.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for device, worker or startup failures.
    pub async fn with_audio(
        factory: impl FnOnce(Arc<AtomicBool>) -> Result<Box<dyn VoiceAudio>, VoiceDeviceError>
        + Send
        + 'static,
    ) -> Result<Self, VoiceError> {
        Self::start(TestKind::Microphone, 0.6, factory).await
    }

    /// Plays two short tones through the selected speakers at the voice volume.
    /// No microphone permission or input device is required.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for invalid gain, device or startup failures.
    pub async fn speakers(options: DeviceOptions, volume: f32) -> Result<Self, VoiceError> {
        Self::with_speaker_audio(volume, move |stop| {
            Ok(Box::new(NativeAudio::open(options, stop)?))
        })
        .await
    }

    /// Runs the speaker check with a replaceable output-only audio backend.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for invalid gain, device or startup failures.
    pub async fn with_speaker_audio(
        volume: f32,
        factory: impl FnOnce(Arc<AtomicBool>) -> Result<Box<dyn VoiceAudio>, VoiceDeviceError>
        + Send
        + 'static,
    ) -> Result<Self, VoiceError> {
        Self::start(TestKind::Speakers, volume, factory).await
    }

    #[allow(clippy::too_many_lines)] // one owner keeps startup, direction-specific work and cleanup together
    async fn start(
        kind: TestKind,
        volume: f32,
        factory: impl FnOnce(Arc<AtomicBool>) -> Result<Box<dyn VoiceAudio>, VoiceDeviceError>
        + Send
        + 'static,
    ) -> Result<Self, VoiceError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(VoiceError::Volume);
        }
        let controls = Arc::new(Controls {
            stop: Arc::new(AtomicBool::new(false)),
            level: AtomicU32::new(0.0_f32.to_bits()),
            voice: AtomicBool::new(false),
            heard: AtomicBool::new(false),
            volume: AtomicU32::new(volume.to_bits()),
        });
        let worker = controls.clone();
        let (ready, receive) = oneshot::channel();
        let (errors, events) = mpsc::channel(1);
        let thread = std::thread::Builder::new()
            .name("orqwa-audio-test".into())
            .spawn(move || {
                let mut ready = Some(ready);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut audio = factory(worker.stop.clone())?;
                    if kind == TestKind::Microphone {
                        audio.microphone(Some(1))?;
                    }
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(Ok(()));
                    }
                    let started = Instant::now();
                    let until = started
                        + Duration::from_secs(if kind == TestKind::Microphone { 10 } else { 2 });
                    let mut detector = SpeechActivity::default();
                    let mut played = 0;
                    while !worker.stop.load(Ordering::Acquire) && Instant::now() < until {
                        if !audio.healthy() && !audio.recover()? {
                            return Err(audio.failure());
                        }
                        let wait = if kind == TestKind::Speakers {
                            // Tones follow the speaker's clock like speech does, so the
                            // check sounds exactly as clean as a friend will.
                            let due = if let Some(queued) = audio.playback_queued() {
                                crate::PLAYBACK_QUEUE.saturating_sub(queued)
                            } else {
                                // Without a device clock, wall time stands in for it: a late
                                // wakeup skips ahead instead of stretching the check.
                                let now = u32::try_from(started.elapsed().as_millis() / 20)
                                    .unwrap_or(u32::MAX);
                                played = played.max(now);
                                1
                            };
                            for _ in 0..due {
                                audio.play(test_tone(
                                    played,
                                    f32::from_bits(worker.volume.load(Ordering::Relaxed)),
                                ));
                                played += 1;
                            }
                            if audio.playback_queued().is_some() {
                                crate::POLL
                            } else {
                                Duration::from_millis(20)
                            }
                        } else {
                            let mut peak = 0.0_f32;
                            // Capture queues are bounded. Do not let an uncooperative plugin loop forever.
                            for _ in 0..4 {
                                let Some(frame) = audio.capture() else {
                                    break;
                                };
                                if frame.activation != 1
                                    || frame.at.elapsed() > Duration::from_millis(100)
                                {
                                    continue;
                                }
                                detector.observe(&frame.samples, Instant::now());
                                for sample in frame.samples {
                                    if sample.is_finite() {
                                        peak = peak.max(sample.abs().min(1.0));
                                    }
                                }
                            }
                            let level = if peak > 0.0 {
                                ((peak.log10() * 20.0 + 60.0) / 60.0).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            worker.level.store(level.to_bits(), Ordering::Relaxed);
                            worker
                                .voice
                                .store(detector.detected(Instant::now()), Ordering::Release);
                            worker.heard.store(detector.seen(), Ordering::Release);
                            Duration::from_millis(20)
                        };
                        std::thread::park_timeout(wait);
                    }
                    if kind == TestKind::Microphone {
                        audio.microphone(None)?;
                    } else {
                        audio.deafen(true);
                    }
                    Ok::<(), VoiceDeviceError>(())
                }));
                worker.level.store(0.0_f32.to_bits(), Ordering::Relaxed);
                worker.voice.store(false, Ordering::Release);
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some(VoiceDeviceError::new(
                        "testing local audio",
                        "native audio worker panicked",
                    )),
                };
                if let Some(error) = error {
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(Err(error));
                    } else {
                        let _ = errors.try_send(error);
                    }
                }
            })
            .map_err(|error| VoiceError::Startup(error.to_string()))?;
        let mut test = Self {
            controls,
            thread: Some(thread),
            errors: events,
        };
        match tokio::time::timeout(Duration::from_secs(5), receive).await {
            Ok(Ok(Ok(()))) => Ok(test),
            Ok(Ok(Err(error))) => {
                test.stop();
                Err(VoiceError::Device(error))
            }
            _ => {
                test.stop();
                Err(test.take_error().map_or_else(
                    || VoiceError::Startup("audio test did not start".into()),
                    VoiceError::Device,
                ))
            }
        }
    }

    /// Current finite level, mapping -60 through 0 dBFS to zero through one.
    /// Samples are never retained or returned.
    #[must_use]
    pub fn level(&self) -> f32 {
        f32::from_bits(self.controls.level.load(Ordering::Relaxed))
    }

    /// Current speech-activity estimate, briefly held between syllables.
    #[must_use]
    pub fn voice_detected(&self) -> bool {
        self.controls.voice.load(Ordering::Acquire)
    }

    /// Whether this microphone test has detected speech-like activity at least once.
    #[must_use]
    pub fn voice_heard(&self) -> bool {
        self.controls.heard.load(Ordering::Acquire)
    }

    /// Updates speaker-test gain without opening capture or changing any consent.
    ///
    /// # Errors
    /// Returns [`VoiceError::Volume`] for non-finite or out-of-range gain.
    pub fn volume(&self, value: f32) -> Result<(), VoiceError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(VoiceError::Volume);
        }
        self.controls
            .volume
            .store(value.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    /// Whether the bounded local check is still running.
    #[must_use]
    pub fn running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Takes the local diagnostic once, without samples.
    pub fn take_error(&mut self) -> Option<VoiceDeviceError> {
        self.errors.try_recv().ok()
    }

    fn stop(&self) {
        self.controls.stop.store(true, Ordering::Release);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    /// Stops the local check and waits up to one second for native handles to close.
    pub async fn close(mut self) {
        self.stop();
        let until = tokio::time::Instant::now() + Duration::from_secs(1);
        while self.running() && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

#[allow(clippy::cast_precision_loss)] // at most two seconds of 48 kHz samples
fn test_tone(frame: u32, volume: f32) -> orqwa_domain::voice::PcmFrame {
    std::array::from_fn(|index| {
        let sample = frame * 960 + u32::try_from(index).unwrap_or(0);
        let within = sample % 48_000;
        if within >= 38_400 {
            return 0.0;
        }
        let fade = ((within as f32 / 960.0).min((38_400 - within) as f32 / 960.0)).min(1.0);
        let frequency = if sample < 48_000 { 660.0 } else { 880.0 };
        (within as f32 * frequency * std::f32::consts::TAU / 48_000.0).sin() * 0.12 * volume * fade
    })
}

impl Drop for AudioTest {
    fn drop(&mut self) {
        self.stop();
    }
}
