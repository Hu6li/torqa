//! Native audio streams with bounded callback queues and no network work in callbacks.

#[cfg(all(test, target_os = "linux"))]
#[path = "device/linux_tests.rs"]
mod linux_tests;

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use orqwa_domain::voice::{
    CapturedVoice, FRAME_SAMPLES, PcmFrame, SAMPLE_RATE, VoiceAudio, VoiceDeviceError,
};

use crate::resample::Resampler;

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
    /// Whether input is advertised, or an ALSA route needs validation when selected.
    pub input: bool,
    /// Whether output is advertised, or an ALSA route needs validation when selected.
    pub output: bool,
}
/// Lists local audio devices without opening capture or playback.
///
/// # Errors
/// Returns [`VoiceDeviceError`] if the OS cannot enumerate devices.
pub fn devices() -> Result<Vec<AudioDevice>, VoiceDeviceError> {
    let host = cpal::default_host();
    let mut devices = Vec::new();
    for device in host
        .devices()
        .map_err(|e| VoiceDeviceError::new("enumerating audio devices", e))?
    {
        let id = device
            .id()
            .map_err(|e| VoiceDeviceError::new("reading audio device ID", e))?;
        #[cfg(target_os = "linux")]
        if id.host() == cpal::HostId::Alsa && id.id() == "null" {
            continue;
        }
        devices.push(AudioDevice {
            id: id.to_string(),
            name: device.to_string(),
            input: supports_direction(&device, true),
            output: supports_direction(&device, false),
        });
    }
    #[cfg(target_os = "linux")]
    devices.sort_by_key(|device| {
        device
            .id
            .parse::<cpal::DeviceId>()
            .map_or(3, |id| match (id.host(), id.id()) {
                (cpal::HostId::Alsa, "pipewire") => 0,
                (cpal::HostId::Alsa, "pulse") => 1,
                (cpal::HostId::Alsa, "default") => 2,
                _ => 3,
            })
    });
    Ok(devices)
}

fn supports_direction(device: &cpal::Device, input: bool) -> bool {
    #[cfg(target_os = "linux")]
    if device.id().is_ok_and(|id| id.host() == cpal::HostId::Alsa)
        && device
            .description()
            .is_ok_and(|description| description.direction() == cpal::DeviceDirection::Unknown)
    {
        // ALSA's default route deliberately has unknown metadata. Only the requested
        // configuration can validate it; probing every plugin can leak native handles.
        return true;
    }
    if input {
        device.supports_input()
    } else {
        device.supports_output()
    }
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

/// The worker's end of the playback queue. The callback counts every frame it takes, so the
/// worker can mix exactly as fast as the speaker plays instead of by its own timer.
struct Speaker {
    send: SyncSender<Output>,
    sent: u64,
    taken: Arc<AtomicU64>,
    largest: Arc<AtomicUsize>,
    rate: u32,
}
impl Speaker {
    fn closed() -> Self {
        Self {
            send: sync_channel(1).0,
            sent: 0,
            taken: Arc::new(AtomicU64::new(0)),
            largest: Arc::new(AtomicUsize::new(0)),
            rate: SAMPLE_RATE,
        }
    }
}

/// Playback frames queued, less those one device callback will take beyond the first: a
/// device that plays large blocks needs that much more in hand, or each block runs dry.
fn queued_ahead(queued: usize, largest_callback: usize, rate: u32) -> usize {
    let block = u64::try_from(largest_callback).unwrap_or(u64::MAX);
    let frame = u64::from(rate) * FRAME_SAMPLES as u64;
    let frames = (block.saturating_mul(u64::from(SAMPLE_RATE))).div_ceil(frame.max(1));
    queued.saturating_sub(usize::try_from(frames.saturating_sub(1)).unwrap_or(usize::MAX))
}

/// CPAL audio backend. Its streams remain on the owning audio thread.
pub struct NativeAudio {
    options: DeviceOptions,
    gates: Arc<Gates>,
    output: Option<Stream>,
    input: Option<Stream>,
    capture_send: SyncSender<CapturedVoice>,
    capture_receive: Receiver<CapturedVoice>,
    speaker: Speaker,
    recovery: RecoveryBudget,
}
impl NativeAudio {
    pub(crate) fn capture_only(options: DeviceOptions, stop: Arc<AtomicBool>) -> Self {
        let gates = Arc::new(Gates {
            stop,
            activation: AtomicU32::new(0),
            deafened: AtomicBool::new(true),
            revision: AtomicU32::new(0),
            invalidated: AtomicU8::new(0),
            failure: Mutex::new(None),
        });
        let (capture_send, capture_receive) = sync_channel(4);
        Self {
            options,
            gates,
            output: None,
            input: None,
            capture_send,
            capture_receive,
            speaker: Speaker::closed(),
            recovery: RecoveryBudget {
                since: Instant::now(),
                attempts: 0,
            },
        }
    }
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
        let (output, speaker) = speakers(&options, &gates)?;
        let (capture_send, capture_receive) = sync_channel(4);
        Ok(Self {
            options,
            gates,
            output: Some(output),
            input: None,
            capture_send,
            capture_receive,
            speaker,
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
) -> Result<(Stream, Speaker), VoiceDeviceError> {
    let host = cpal::default_host();
    let (device, supported) = configured(&host, options.output.as_deref(), false)?;
    let format = supported.sample_format();
    let config = configuration(&supported);
    validate(&config)?;
    // Room for large device blocks; the worker keeps only what the device needs queued.
    let (send, receive) = sync_channel(8);
    let taken = Arc::new(AtomicU64::new(0));
    let largest = Arc::new(AtomicUsize::new(0));
    let rate = config.sample_rate;
    let queue = Queue {
        receive,
        taken: taken.clone(),
        largest: largest.clone(),
    };
    let gates = gates.clone();
    let output = match format {
        SampleFormat::F32 => output::<f32>(&device, config, queue, gates),
        SampleFormat::F64 => output::<f64>(&device, config, queue, gates),
        SampleFormat::I8 => output::<i8>(&device, config, queue, gates),
        SampleFormat::I16 => output::<i16>(&device, config, queue, gates),
        SampleFormat::I32 => output::<i32>(&device, config, queue, gates),
        SampleFormat::I64 => output::<i64>(&device, config, queue, gates),
        SampleFormat::U8 => output::<u8>(&device, config, queue, gates),
        SampleFormat::U16 => output::<u16>(&device, config, queue, gates),
        SampleFormat::U32 => output::<u32>(&device, config, queue, gates),
        SampleFormat::U64 => output::<u64>(&device, config, queue, gates),
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
    Ok((
        output,
        Speaker {
            send,
            sent: 0,
            taken,
            largest,
            rate,
        },
    ))
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
    if !supports_direction(&device, input) {
        return Err(VoiceDeviceError::new(
            "selecting audio device",
            "the device does not support the selected direction",
        ));
    }
    Ok(device)
}

fn read_configuration(
    device: &cpal::Device,
    input: bool,
) -> Result<cpal::SupportedStreamConfig, cpal::Error> {
    if input {
        device.default_input_config()
    } else {
        device.default_output_config()
    }
}

fn configured(
    host: &cpal::Host,
    id: Option<&str>,
    input: bool,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig), VoiceDeviceError> {
    let operation = if input {
        "reading microphone configuration"
    } else {
        "reading speaker configuration"
    };
    #[cfg(target_os = "linux")]
    if id.is_none() && host.id() == cpal::HostId::Alsa {
        // Desktop sound servers own Bluetooth routing. Prefer their ALSA bridges to
        // raw hardware; explicit device choices never fall through to another device.
        for name in ["pipewire", "pulse"] {
            if let Some(device) = host.device_by_id(&cpal::DeviceId::new(cpal::HostId::Alsa, name))
                && supports_direction(&device, input)
            {
                match read_configuration(&device, input) {
                    Ok(config) => return Ok((device, config)),
                    Err(error)
                        if matches!(
                            error.kind(),
                            cpal::ErrorKind::PermissionDenied | cpal::ErrorKind::DeviceBusy
                        ) =>
                    {
                        return Err(VoiceDeviceError::new(operation, error));
                    }
                    Err(_) => {}
                }
            }
        }
    }
    let device = choose(host, id, input)?;
    let config = read_configuration(&device, input)
        .map_err(|error| VoiceDeviceError::new(operation, error))?;
    Ok((device, config))
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
        let (device, supported) = configured(&host, self.options.input.as_deref(), true)?;
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
        // Deafened output still takes (and discards) frames, keeping the worker on its clock.
        if self
            .speaker
            .send
            .try_send(Output {
                samples: frame,
                at: Instant::now(),
                revision: self.gates.revision.load(Ordering::Relaxed),
            })
            .is_ok()
        {
            self.speaker.sent += 1;
        }
    }
    fn playback_queued(&self) -> Option<usize> {
        self.output.as_ref()?;
        let taken = self.speaker.taken.load(Ordering::Acquire);
        let queued = usize::try_from(self.speaker.sent.saturating_sub(taken)).unwrap_or(usize::MAX);
        Some(queued_ahead(
            queued,
            self.speaker.largest.load(Ordering::Relaxed),
            self.speaker.rate,
        ))
    }
    fn deafen(&mut self, deafened: bool) {
        self.gates.deafened.store(deafened, Ordering::Release);
        self.gates.revision.fetch_add(1, Ordering::AcqRel);
    }
    fn healthy(&self) -> bool {
        self.gates.invalidated.load(Ordering::Acquire) == 0
    }
    fn recover(&mut self) -> Result<bool, VoiceDeviceError> {
        let output_open = self.output.is_some();
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
        if output_open {
            let (output, speaker) = speakers(&self.options, &self.gates)?;
            self.output = Some(output);
            self.speaker = speaker;
        }
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
    let mut resampler = Resampler::new(config.sample_rate, SAMPLE_RATE);
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

struct Queue {
    receive: Receiver<Output>,
    taken: Arc<AtomicU64>,
    largest: Arc<AtomicUsize>,
}
impl Queue {
    fn take(&self) -> Option<Output> {
        let next = self.receive.try_recv().ok()?;
        self.taken.fetch_add(1, Ordering::Release);
        Some(next)
    }
}

fn output<T>(
    device: &cpal::Device,
    config: StreamConfig,
    queue: Queue,
    gates: Arc<Gates>,
) -> Result<Stream, VoiceDeviceError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let mut resampler = Resampler::new(SAMPLE_RATE, config.sample_rate);
    let mut samples = [0.0; FRAME_SAMPLES];
    let mut position = FRAME_SAMPLES;
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
                    while queue.take().is_some() {}
                    position = FRAME_SAMPLES;
                    revision = current;
                    resampler.reset();
                    data.fill(T::from_sample(0.0));
                    return;
                }
                queue
                    .largest
                    .fetch_max(data.len() / channels, Ordering::Relaxed);
                // An empty queue plays silence for the rest of this callback only; the next
                // frame continues exactly where it arrives instead of a whole frame later.
                let mut starved = false;
                for frame in data.chunks_exact_mut(channels) {
                    let value = resampler.pull(|| {
                        while position == FRAME_SAMPLES && !starved {
                            match queue.take() {
                                Some(next)
                                    if next.revision == revision
                                        && next.at.elapsed() <= Duration::from_millis(250) =>
                                {
                                    samples = next.samples;
                                    position = 0;
                                }
                                Some(_) => {}
                                None => starved = true,
                            }
                        }
                        if position < FRAME_SAMPLES {
                            position += 1;
                            samples[position - 1]
                        } else {
                            0.0
                        }
                    });
                    for sample in frame {
                        *sample = T::from_sample(value);
                    }
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
    fn devices_playing_large_blocks_keep_enough_speech_in_hand() {
        // 10 ms callbacks need nothing extra; one 85 ms Bluetooth block takes five frames.
        assert_eq!(queued_ahead(2, 480, 48_000), 2);
        assert_eq!(queued_ahead(2, 441, 44_100), 2);
        assert_eq!(queued_ahead(2, 960, 48_000), 2);
        assert_eq!(queued_ahead(6, 4096, 48_000), 2);
        assert_eq!(queued_ahead(2, 4096, 48_000), 0);
        assert_eq!(queued_ahead(3, 1365, 16_000), 0);
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
}
