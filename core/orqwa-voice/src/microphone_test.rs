//! Explicit, temporary microphone metering without playback, a codec or a network port.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use orqwa_domain::voice::{VoiceAudio, VoiceDeviceError};
use tokio::sync::{mpsc, oneshot};

use crate::{
    VoiceError,
    device::{DeviceOptions, NativeAudio},
};

struct Controls {
    stop: Arc<AtomicBool>,
    level: AtomicU32,
}

/// A ten-second, local-only microphone level test. Dropping it stops capture.
/// No playback or network interface is available to the test worker.
pub struct MicrophoneTest {
    controls: Arc<Controls>,
    thread: Option<JoinHandle<()>>,
    errors: mpsc::Receiver<VoiceDeviceError>,
}

impl MicrophoneTest {
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
        let controls = Arc::new(Controls {
            stop: Arc::new(AtomicBool::new(false)),
            level: AtomicU32::new(0.0_f32.to_bits()),
        });
        let worker = controls.clone();
        let (ready, receive) = oneshot::channel();
        let (errors, events) = mpsc::channel(1);
        let thread = std::thread::Builder::new()
            .name("orqwa-mic-test".into())
            .spawn(move || {
                let mut ready = Some(ready);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut audio = factory(worker.stop.clone())?;
                    audio.microphone(Some(1))?;
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(Ok(()));
                    }
                    let until = Instant::now() + Duration::from_secs(10);
                    while !worker.stop.load(Ordering::Acquire) && Instant::now() < until {
                        if !audio.healthy() && !audio.recover()? {
                            return Err(audio.failure());
                        }
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
                        std::thread::park_timeout(Duration::from_millis(20));
                    }
                    audio.microphone(None)?;
                    Ok::<(), VoiceDeviceError>(())
                }));
                worker.level.store(0.0_f32.to_bits(), Ordering::Relaxed);
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some(VoiceDeviceError::new(
                        "testing microphone",
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
                    || VoiceError::Startup("microphone test did not start".into()),
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

    /// Whether the bounded worker is still capturing.
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

    /// Stops capture and waits up to one second for native handles to close.
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

impl Drop for MicrophoneTest {
    fn drop(&mut self) {
        self.stop();
    }
}
