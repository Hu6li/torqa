//! Native audio streams with bounded callback queues and no network work in callbacks.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use orqwa_domain::voice::{
    CapturedVoice, FRAME_SAMPLES, PcmFrame, SAMPLE_RATE, VoiceAudio, VoiceDeviceError,
};

/// Device IDs from [`devices`]. Unspecified IDs use the operating system's defaults.
#[derive(Clone, Debug, Default)]
pub struct DeviceOptions {
    /// Microphone ID; no input stream opens until explicit unmute.
    pub input: Option<String>,
    /// Speaker/headphone output ID.
    pub output: Option<String>,
}
/// One available local audio device; enumeration does not capture microphone samples.
#[derive(Debug)]
pub struct AudioDevice {
    /// Stable CPAL device ID used by voice input/output options.
    pub id: String,
    /// Human-readable local device name.
    pub name: String,
    /// Whether the device has input configurations.
    pub input: bool,
    /// Whether the device has output configurations.
    pub output: bool,
}
/// Lists local audio devices without opening capture or playback.
///
/// # Errors
/// Returns [`VoiceDeviceError`] if the OS cannot enumerate devices.
pub fn devices() -> Result<Vec<AudioDevice>, VoiceDeviceError> {
    let host = cpal::default_host();
    host.devices()
        .map_err(|e| VoiceDeviceError::new("enumerating audio devices", e))?
        .map(|device| {
            Ok(AudioDevice {
                id: device
                    .id()
                    .map_err(|e| VoiceDeviceError::new("reading audio device ID", e))?
                    .to_string(),
                name: device.to_string(),
                input: device.supports_input(),
                output: device.supports_output(),
            })
        })
        .collect()
}

struct Gates {
    stop: Arc<AtomicBool>,
    activation: AtomicU32,
    deafened: AtomicBool,
    revision: AtomicU32,
    invalidated: AtomicU8,
    failure: Mutex<Option<VoiceDeviceError>>,
}
impl Gates {
    fn fail(&self, error: cpal::Error, stream: u8) {
        // CPAL recovers buffer underruns/overruns itself. A dropped audio buffer is not
        // a lost device and must not tear down the whole conversation during a slow frame.
        if matches!(
            error.kind(),
            cpal::ErrorKind::Xrun
                | cpal::ErrorKind::DeviceChanged
                | cpal::ErrorKind::RealtimeDenied
        ) {
            return;
        }
        let reason = if error.kind() == cpal::ErrorKind::StreamInvalidated {
            stream
        } else {
            PERMANENT
        };
        // Error callbacks never wait on the audio owner. Keep at most one diagnostic,
        // with no samples or credentials, and retire the stream through its normal gate.
        if let Ok(mut failure) = self.failure.try_lock()
            && (failure.is_none() || reason == PERMANENT)
        {
            *failure = Some(VoiceDeviceError::new("voice audio callback", error));
        }
        self.invalidated.fetch_or(reason, Ordering::AcqRel);
    }
}

const INPUT: u8 = 1;
const OUTPUT: u8 = 2;
const PERMANENT: u8 = 4;

struct RecoveryBudget {
    since: Instant,
    attempts: u8,
}
impl RecoveryBudget {
    fn attempt(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.since) >= Duration::from_secs(5) {
            self.since = now;
            self.attempts = 0;
        }
        if self.attempts == 3 {
            return false;
        }
        self.attempts += 1;
        true
    }
}

struct Output {
    samples: PcmFrame,
    at: Instant,
    revision: u32,
}

/// CPAL audio backend. Its streams remain on the owning audio thread.
pub struct NativeAudio {
    options: DeviceOptions,
    gates: Arc<Gates>,
    output: Option<Stream>,
    input: Option<Stream>,
    capture_send: SyncSender<CapturedVoice>,
    capture_receive: Receiver<CapturedVoice>,
    play_send: SyncSender<Output>,
    recovery: RecoveryBudget,
}
impl NativeAudio {
    /// Opens output only. Capture is constructed lazily by [`VoiceAudio::microphone`].
    ///
    /// # Errors
    /// Returns [`VoiceDeviceError`] for unavailable devices or unsupported configurations.
    pub fn open(options: DeviceOptions, stop: Arc<AtomicBool>) -> Result<Self, VoiceDeviceError> {
        let gates = Arc::new(Gates {
            stop,
            activation: AtomicU32::new(0),
            deafened: AtomicBool::new(false),
            revision: AtomicU32::new(0),
            invalidated: AtomicU8::new(0),
            failure: Mutex::new(None),
        });
        let (output, play_send) = speakers(&options, &gates)?;
        let (capture_send, capture_receive) = sync_channel(4);
        Ok(Self {
            options,
            gates,
            output: Some(output),
            input: None,
            capture_send,
            capture_receive,
            play_send,
            recovery: RecoveryBudget {
                since: Instant::now(),
                attempts: 0,
            },
        })
    }
}

fn speakers(
    options: &DeviceOptions,
    gates: &Arc<Gates>,
) -> Result<(Stream, SyncSender<Output>), VoiceDeviceError> {
    let host = cpal::default_host();
    let device = choose(&host, options.output.as_deref(), false)?;
    let supported = device
        .default_output_config()
        .map_err(|e| VoiceDeviceError::new("reading speaker configuration", e))?;
    let format = supported.sample_format();
    let config = configuration(&supported);
    validate(&config)?;
    let (play_send, receive) = sync_channel(4);
    let output = match format {
        SampleFormat::F32 => output::<f32>(&device, config, receive, gates.clone()),
        SampleFormat::F64 => output::<f64>(&device, config, receive, gates.clone()),
        SampleFormat::I8 => output::<i8>(&device, config, receive, gates.clone()),
        SampleFormat::I16 => output::<i16>(&device, config, receive, gates.clone()),
        SampleFormat::I32 => output::<i32>(&device, config, receive, gates.clone()),
        SampleFormat::I64 => output::<i64>(&device, config, receive, gates.clone()),
        SampleFormat::U8 => output::<u8>(&device, config, receive, gates.clone()),
        SampleFormat::U16 => output::<u16>(&device, config, receive, gates.clone()),
        SampleFormat::U32 => output::<u32>(&device, config, receive, gates.clone()),
        SampleFormat::U64 => output::<u64>(&device, config, receive, gates.clone()),
        _ => {
            return Err(VoiceDeviceError::new(
                "opening voice audio stream",
                "unsupported sample format",
            ));
        }
    }?;
    output
        .play()
        .map_err(|e| VoiceDeviceError::new("starting speaker playback", e))?;
    Ok((output, play_send))
}

fn choose(
    host: &cpal::Host,
    id: Option<&str>,
    input: bool,
) -> Result<cpal::Device, VoiceDeviceError> {
    let device = match id {
        Some(id) => host.device_by_id(
            &id.parse()
                .map_err(|e| VoiceDeviceError::new("reading selected audio device ID", e))?,
        ),
        None if input => host.default_input_device(),
        None => host.default_output_device(),
    }
    .ok_or_else(|| {
        VoiceDeviceError::new(
            "selecting audio device",
            "selected or default device is unavailable",
        )
    })?;
    if (input && !device.supports_input()) || (!input && !device.supports_output()) {
        return Err(VoiceDeviceError::new(
            "selecting audio device",
            "the device does not support the selected direction",
        ));
    }
    Ok(device)
}
fn validate(config: &StreamConfig) -> Result<(), VoiceDeviceError> {
    if !(1..=8).contains(&config.channels) || !(8000..=192_000).contains(&config.sample_rate) {
        return Err(VoiceDeviceError::new(
            "validating native audio configuration",
            format!(
                "unsupported {} channels at {} Hz",
                config.channels, config.sample_rate
            ),
        ));
    }
    Ok(())
}
fn configuration(supported: &cpal::SupportedStreamConfig) -> StreamConfig {
    let mut config = supported.config();
    if let cpal::SupportedBufferSize::Range { min, max } = supported.buffer_size()
        && min <= max
    {
        config.buffer_size = cpal::BufferSize::Fixed((config.sample_rate / 100).clamp(*min, *max));
    }
    config
}
impl VoiceAudio for NativeAudio {
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        self.gates.activation.store(0, Ordering::Release);
        self.input = None;
        while self.capture_receive.try_recv().is_ok() {}
        let Some(activation) = activation else {
            return Ok(());
        };
        if activation == 0 || self.gates.stop.load(Ordering::Acquire) {
            return Err(VoiceDeviceError::new(
                "opening voice audio stream",
                "unsupported configuration or activation is no longer valid",
            ));
        }
        let host = cpal::default_host();
        let device = choose(&host, self.options.input.as_deref(), true)?;
        let supported = device
            .default_input_config()
            .map_err(|e| VoiceDeviceError::new("reading microphone configuration", e))?;
        let format = supported.sample_format();
        let config = configuration(&supported);
        validate(&config)?;
        let send = self.capture_send.clone();
        let stream = match format {
            SampleFormat::F32 => {
                input::<f32>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::F64 => {
                input::<f64>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::I8 => input::<i8>(&device, config, send, self.gates.clone(), activation),
            SampleFormat::I16 => {
                input::<i16>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::I32 => {
                input::<i32>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::I64 => {
                input::<i64>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::U8 => input::<u8>(&device, config, send, self.gates.clone(), activation),
            SampleFormat::U16 => {
                input::<u16>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::U32 => {
                input::<u32>(&device, config, send, self.gates.clone(), activation)
            }
            SampleFormat::U64 => {
                input::<u64>(&device, config, send, self.gates.clone(), activation)
            }
            _ => {
                return Err(VoiceDeviceError::new(
                    "opening voice audio stream",
                    "unsupported sample format",
                ));
            }
        }?;
        if self.gates.stop.load(Ordering::Acquire) {
            return Err(VoiceDeviceError::new(
                "opening voice audio stream",
                "unsupported configuration or activation is no longer valid",
            ));
        }
        self.gates.activation.store(activation, Ordering::Release);
        stream.play().map_err(|error| {
            self.gates.activation.store(0, Ordering::Release);
            VoiceDeviceError::new("starting microphone capture", error)
        })?;
        self.input = Some(stream);
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        self.capture_receive.try_recv().ok()
    }
    fn play(&mut self, frame: PcmFrame) {
        if !self.gates.deafened.load(Ordering::Relaxed) {
            let _ = self.play_send.try_send(Output {
                samples: frame,
                at: Instant::now(),
                revision: self.gates.revision.load(Ordering::Relaxed),
            });
        }
    }
    fn deafen(&mut self, deafened: bool) {
        self.gates.deafened.store(deafened, Ordering::Release);
        self.gates.revision.fetch_add(1, Ordering::AcqRel);
    }
    fn healthy(&self) -> bool {
        self.gates.invalidated.load(Ordering::Acquire) == 0
    }
    fn recover(&mut self) -> Result<bool, VoiceDeviceError> {
        let invalidated = self.gates.invalidated.load(Ordering::Acquire);
        if invalidated & PERMANENT != 0
            || self.gates.stop.load(Ordering::Acquire)
            || !self.recovery.attempt(Instant::now())
        {
            return Ok(false);
        }
        // Bluetooth may change both hardware rates when its microphone opens. Keep an
        // unaffected capture stream open so the headset does not switch profiles again.
        let activation = self.gates.activation.load(Ordering::Acquire);
        if invalidated & INPUT != 0 {
            self.microphone(None)?;
        }
        self.output = None;
        self.gates.revision.fetch_add(1, Ordering::AcqRel);
        let invalidated = self.gates.invalidated.swap(0, Ordering::AcqRel);
        if invalidated & PERMANENT != 0 {
            self.gates
                .invalidated
                .fetch_or(invalidated, Ordering::AcqRel);
            return Ok(false);
        }
        self.gates
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        std::thread::sleep(Duration::from_millis(100));
        if self.gates.stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        if invalidated & INPUT != 0 && activation != 0 {
            self.microphone(Some(activation))?;
        }
        let (output, send) = speakers(&self.options, &self.gates)?;
        self.output = Some(output);
        self.play_send = send;
        Ok(true)
    }
    fn failure(&mut self) -> VoiceDeviceError {
        self.gates
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| {
                VoiceDeviceError::new("voice audio callback", "device stopped responding")
            })
    }
}
impl Drop for NativeAudio {
    fn drop(&mut self) {
        self.gates.activation.store(0, Ordering::Release);
        self.gates.deafened.store(true, Ordering::Release);
    }
}

fn input<T>(
    device: &cpal::Device,
    config: StreamConfig,
    send: SyncSender<CapturedVoice>,
    gates: Arc<Gates>,
    activation: u32,
) -> Result<Stream, VoiceDeviceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut resampler = Resampler::new(config.sample_rate);
    let sample_rate = config.sample_rate;
    let mut samples = [0.0; FRAME_SAMPLES];
    let mut count = 0;
    let failures = gates.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if gates.stop.load(Ordering::Acquire)
                    || gates.invalidated.load(Ordering::Acquire) != 0
                    || gates.activation.load(Ordering::Acquire) != activation
                {
                    count = 0;
                    return;
                }
                let frames = data.len() / channels;
                let callback_at = Instant::now();
                for (index, frame) in data.chunks_exact(channels).enumerate() {
                    let mut mono = 0.0_f32;
                    for sample in frame {
                        mono += sample.to_sample::<f32>();
                    }
                    #[allow(clippy::cast_precision_loss)] // channel count is bounded to eight
                    {
                        mono /= channels as f32;
                    }
                    if !mono.is_finite() {
                        mono = 0.0;
                    }
                    mono = mono.clamp(-1.0, 1.0);
                    resampler.push(mono, |sample| {
                        samples[count] = sample;
                        count += 1;
                        if count == FRAME_SAMPLES {
                            #[allow(clippy::cast_precision_loss)] // callback sizes are below 2^53
                            let remaining = (frames - index - 1) as f64 / f64::from(sample_rate);
                            let at = callback_at
                                .checked_sub(std::time::Duration::from_secs_f64(remaining))
                                .unwrap_or(callback_at);
                            if at.elapsed() <= std::time::Duration::from_millis(60) {
                                let _ = send.try_send(CapturedVoice {
                                    samples,
                                    activation,
                                    at,
                                });
                            }
                            count = 0;
                        }
                    });
                }
            },
            move |error| {
                failures.fail(error, INPUT);
            },
            None,
        )
        .map_err(|error| VoiceDeviceError::new("building native audio stream", error))
}

struct Resampler {
    previous: f32,
    position: f64,
    step: f64,
}
impl Resampler {
    fn new(rate: u32) -> Self {
        Self {
            previous: 0.0,
            position: 1.0,
            step: f64::from(rate) / f64::from(SAMPLE_RATE),
        }
    }
    fn push(&mut self, mono: f32, mut emit: impl FnMut(f32)) {
        while self.position <= 1.0 {
            #[allow(clippy::cast_possible_truncation)] // interpolation fraction is in [0,1]
            emit(self.previous + (mono - self.previous) * self.position as f32);
            self.position += self.step;
        }
        self.position -= 1.0;
        self.previous = mono;
    }
}

#[allow(clippy::cast_precision_loss)] // frame length is exactly 960, representable as f64
fn output<T>(
    device: &cpal::Device,
    config: StreamConfig,
    receive: Receiver<Output>,
    gates: Arc<Gates>,
) -> Result<Stream, VoiceDeviceError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let step = f64::from(SAMPLE_RATE) / f64::from(config.sample_rate);
    let mut samples = [0.0; FRAME_SAMPLES];
    let mut position = FRAME_SAMPLES as f64;
    let mut revision = 0;
    let failures = gates.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let current = gates.revision.load(Ordering::Acquire);
                if gates.stop.load(Ordering::Acquire)
                    || gates.invalidated.load(Ordering::Acquire) != 0
                    || gates.deafened.load(Ordering::Acquire)
                    || revision != current
                {
                    while receive.try_recv().is_ok() {}
                    samples.fill(0.0);
                    position = FRAME_SAMPLES as f64;
                    revision = current;
                    data.fill(T::from_sample(0.0));
                    return;
                }
                for frame in data.chunks_exact_mut(channels) {
                    if position >= FRAME_SAMPLES as f64 {
                        position -= FRAME_SAMPLES as f64;
                        samples.fill(0.0);
                        while let Ok(next) = receive.try_recv() {
                            if next.revision == revision
                                && next.at.elapsed() <= std::time::Duration::from_millis(100)
                            {
                                samples = next.samples;
                                break;
                            }
                        }
                    }
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    // phase is maintained in [0,960)
                    let index = position as usize;
                    let value = samples[index];
                    for sample in frame {
                        *sample = T::from_sample(value);
                    }
                    position += step;
                }
            },
            move |error| {
                failures.fail(error, OUTPUT);
            },
            None,
        )
        .map_err(|error| VoiceDeviceError::new("building native audio stream", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffer_underruns_recover_but_device_errors_keep_the_original_cause() {
        let gates = Gates {
            stop: Arc::new(AtomicBool::new(false)),
            activation: AtomicU32::new(0),
            deafened: AtomicBool::new(false),
            revision: AtomicU32::new(0),
            invalidated: AtomicU8::new(0),
            failure: Mutex::new(None),
        };
        gates.fail(
            cpal::Error::with_message(cpal::ErrorKind::Xrun, "buffer underrun"),
            OUTPUT,
        );
        for kind in [
            cpal::ErrorKind::DeviceChanged,
            cpal::ErrorKind::RealtimeDenied,
        ] {
            gates.fail(cpal::Error::new(kind), INPUT);
        }
        assert_eq!(gates.invalidated.load(Ordering::Acquire), 0);
        gates.fail(
            cpal::Error::with_message(
                cpal::ErrorKind::StreamInvalidated,
                "Device sample rate changed",
            ),
            OUTPUT,
        );
        assert_eq!(gates.invalidated.load(Ordering::Acquire), OUTPUT);
        assert!(
            gates
                .failure
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .to_string()
                .contains("Device sample rate changed")
        );
        gates.fail(
            cpal::Error::with_message(
                cpal::ErrorKind::DeviceNotAvailable,
                "AAudio device disconnected",
            ),
            INPUT,
        );
        assert_ne!(gates.invalidated.load(Ordering::Acquire) & PERMANENT, 0);
        let cause = gates.failure.lock().unwrap().take().unwrap().to_string();
        assert!(
            cause.contains("voice audio callback") && cause.contains("AAudio device disconnected")
        );
    }
    #[test]
    fn repeated_route_failures_have_a_finite_recovery_budget() {
        let now = Instant::now();
        let mut budget = RecoveryBudget {
            since: now,
            attempts: 0,
        };
        for _ in 0..3 {
            assert!(budget.attempt(now));
        }
        assert!(!budget.attempt(now + Duration::from_secs(4)));
        assert!(budget.attempt(now + Duration::from_secs(5)));
    }
    #[test]
    fn device_rates_produce_forty_eight_thousand_finite_mono_samples_per_second() {
        for rate in [8000, 16_000, 44_100, 48_000, 96_000, 192_000] {
            let mut resampler = Resampler::new(rate);
            let mut count = 0_u32;
            for _ in 0..rate {
                resampler.push(0.25, |sample| {
                    assert!(sample.is_finite() && (sample - 0.25).abs() < 0.001);
                    count += 1;
                });
            }
            assert!(
                count.abs_diff(SAMPLE_RATE) <= 6,
                "native rate {rate}, count {count}"
            );
        }
    }
}
