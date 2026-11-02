//! Local voice capture and Opus speech; audio is never recorded or sent to a voice service.

pub mod audio_test;
pub mod codec;
pub mod device;
mod echo;
pub mod microphone_test;
mod pacing;
mod resample;
mod speech_activity;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use orqwa_domain::voice::{FRAME_SAMPLES, PcmFrame, VoiceAudio, VoiceDeviceError};
use orqwa_net::state::{MAX_RIDERS, RiderId};
use orqwa_net::voice::{Playout, VoicePort};
use tokio::sync::{mpsc, oneshot};

use codec::{SpeechDecoder, SpeechEncoder};

/// Playback frames kept queued ahead of the speaker, covering a late worker wakeup.
const PLAYBACK_QUEUE: usize = 2;
/// How often the worker tops up playback and sends captured speech.
const POLL: Duration = Duration::from_millis(5);
/// Captured speech older than this would arrive too late to belong to the conversation.
const CAPTURE_FRESH: Duration = Duration::from_millis(120);
/// Frames handled per wakeup in each direction, whatever a backend reports.
const MAX_FRAMES_PER_WAKE: usize = 8;

/// Bounded local status messages, without audio or credentials.
#[derive(Debug)]
pub enum VoiceEvent {
    /// A device or microphone could not be used. Conversation/ride state is unaffected.
    DeviceUnavailable {
        /// Local operation and backend diagnostic, without microphone samples.
        error: VoiceDeviceError,
    },
    /// Invalid compressed audio silenced only this speaker locally.
    PeerMuted {
        /// Offending speaker number.
        peer: RiderId,
    },
    /// The voice owner or group connection ended and audio streams were closed.
    Stopped,
}

/// Voice setup failed. Microphone permission is requested only after explicit unmute.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// The selected local hardware is unavailable or unsupported.
    #[error(transparent)]
    Device(#[from] VoiceDeviceError),
    /// Local worker or codec startup failed.
    #[error("Voice could not start: {0}")]
    Startup(String),
    /// Volume is finite and between zero and one.
    #[error("Voice volume must be from 0 to 1.")]
    Volume,
    /// Voice is closed or microphone transmission is currently forbidden.
    #[error(transparent)]
    Network(#[from] orqwa_net::voice::VoiceError),
}

struct Controls {
    stop: Arc<AtomicBool>,
    volume: AtomicU32,
    flush: AtomicU32,
    echo_prevention: AtomicBool,
}

/// Owns one local voice thread and its audio devices. Starts listening with the mic muted.
/// Dropping it closes transmission immediately; the owner thread then releases audio devices.
pub struct VoiceChat {
    port: VoicePort,
    controls: Arc<Controls>,
    thread: Option<JoinHandle<()>>,
    events: mpsc::Receiver<VoiceEvent>,
}

impl VoiceChat {
    /// Opens the selected/default native audio devices, initially without a microphone stream.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for device, codec or worker startup failures.
    pub async fn open(port: VoicePort, options: device::DeviceOptions) -> Result<Self, VoiceError> {
        Self::with_audio(port, move |stop| {
            Ok(Box::new(device::NativeAudio::open(options, stop)?))
        })
        .await
    }
    /// Runs the same codec/mixing path through a replaceable local audio backend.
    /// The factory executes on the owning thread, so native stream handles need not be Send.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for backend/codec initialization, timeout or thread failures.
    pub async fn with_audio(
        port: VoicePort,
        factory: impl FnOnce(Arc<AtomicBool>) -> Result<Box<dyn VoiceAudio>, VoiceDeviceError>
        + Send
        + 'static,
    ) -> Result<Self, VoiceError> {
        let port = port.claim()?;
        let controls = Arc::new(Controls {
            stop: Arc::new(AtomicBool::new(false)),
            volume: AtomicU32::new(0.6_f32.to_bits()),
            flush: AtomicU32::new(0),
            echo_prevention: AtomicBool::new(false),
        });
        let (ready, receive) = oneshot::channel();
        let (send, events) = mpsc::channel(8);
        let owner_port = port.clone();
        let owner_controls = controls.clone();
        let thread = std::thread::Builder::new()
            .name("orqwa-voice".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    factory(owner_controls.stop.clone())
                }));
                match result {
                    Ok(Ok(audio)) => run(&owner_port, &owner_controls, audio, ready, &send),
                    Ok(Err(error)) => {
                        let _ = ready.send(Err(VoiceError::Device(error)));
                    }
                    Err(panic) => {
                        let cause = panic
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic.downcast_ref::<&str>().copied())
                            .unwrap_or("non-string panic");
                        let _ = ready.send(Err(VoiceError::Startup(format!(
                            "opening native audio devices panicked: {cause}"
                        ))));
                    }
                }
            })
            .map_err(|error| VoiceError::Startup(format!("starting voice worker: {error}")))?;
        let mut chat = Self {
            port,
            controls,
            thread: Some(thread),
            events,
        };
        match tokio::time::timeout(Duration::from_secs(5), receive).await {
            Ok(Ok(Ok(()))) => Ok(chat),
            Ok(Ok(Err(error))) => {
                chat.stop();
                Err(error)
            }
            Ok(Err(error)) => {
                chat.stop();
                Err(VoiceError::Startup(format!(
                    "voice worker ended before reporting readiness: {error}"
                )))
            }
            Err(error) => {
                chat.stop();
                Err(VoiceError::Startup(format!(
                    "waiting for native audio devices timed out: {error}"
                )))
            }
        }
    }
    /// Explicitly opens/closes microphone transmission. Muting invalidates buffered speech.
    ///
    /// # Errors
    /// Returns [`VoiceError`] if the session is closed, deafened or unavailable.
    pub fn microphone(&self, enabled: bool) -> Result<(), VoiceError> {
        self.port.microphone(enabled)?;
        self.wake();
        Ok(())
    }
    /// Current transmission gate; backend failures turn it off again automatically.
    #[must_use]
    pub fn microphone_enabled(&self) -> bool {
        self.port.transmitting().is_some()
    }
    /// Prevents speaker feedback by holding outgoing speech while a friend's speech is audible,
    /// including a short echo tail; background noise does not hold it. Disable for
    /// full-duplex conversation with headphones.
    pub fn echo_prevention(&self, enabled: bool) {
        self.controls
            .echo_prevention
            .store(enabled, Ordering::Release);
        self.wake();
    }
    /// Silences playback and mutes the microphone; undeafening keeps the microphone muted.
    pub fn deafen(&self, deafened: bool) {
        self.port.deafen(deafened);
        self.controls.flush.fetch_add(1, Ordering::Relaxed);
        self.wake();
    }
    /// Silences one speaker locally and discards mixed playback already queued for that speaker.
    pub fn mute_peer(&self, peer: RiderId, muted: bool) {
        self.port.mute_peer(peer, muted);
        self.controls.flush.fetch_add(1, Ordering::Relaxed);
        self.wake();
    }
    /// Sets normalized playback gain without touching trainer or activity settings.
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
        self.wake();
        Ok(())
    }
    /// Waits for the next bounded local status event.
    pub async fn next(&mut self) -> Option<VoiceEvent> {
        self.events.recv().await
    }
    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
    fn stop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
        self.port.disable();
        self.wake();
    }
    /// Stops capture/transmission and waits for up to one second for normal device cleanup.
    /// A backend stalled in the OS remains gated off and releases its handles when it returns.
    pub async fn close(mut self) {
        self.stop();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}
impl Drop for VoiceChat {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    port: &VoicePort,
    controls: &Controls,
    mut audio: Box<dyn VoiceAudio>,
    ready: oneshot::Sender<Result<(), VoiceError>>,
    events: &mpsc::Sender<VoiceEvent>,
) {
    let mut encoder = match SpeechEncoder::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            let _ = ready.send(Err(VoiceError::Startup(format!(
                "opening Opus encoder: {error}"
            ))));
            return;
        }
    };
    if controls.stop.load(Ordering::Acquire) || !port.alive() {
        let _ = ready.send(Err(VoiceError::Startup(
            "group connection ended during voice startup".into(),
        )));
        return;
    }
    port.enable(true);
    let _ = ready.send(Ok(()));
    let mut decoders: [Option<(u64, SpeechDecoder)>; MAX_RIDERS] = std::array::from_fn(|_| None);
    let mut activation = None;
    let mut playback = (false, 0);
    let mut pacer = pacing::Pacer::new(Instant::now());
    let mut speaker_gate = echo::SpeakerGate::new();
    while !controls.stop.load(Ordering::Acquire) && port.alive() {
        let requested = port.transmitting();
        if requested != activation {
            if let Err(error) = audio.microphone(requested) {
                let _ = port.microphone(false);
                let _ = audio.microphone(None);
                let _ = events.try_send(VoiceEvent::DeviceUnavailable { error });
            }
            activation = port.transmitting();
            speaker_gate.restart();
        }
        let output = (port.deafened(), controls.flush.load(Ordering::Relaxed));
        if output != playback {
            audio.deafen(output.0);
            playback = output;
        }
        if !audio.healthy() {
            let error = match audio.recover() {
                Ok(true) => {
                    decoders.fill_with(|| None);
                    pacer.defer(Instant::now());
                    continue;
                }
                Ok(false) => audio.failure(),
                Err(error) => error,
            };
            let _ = events.try_send(VoiceEvent::DeviceUnavailable { error });
            break;
        }
        // The speaker's clock sets the pace: each frame it takes is mixed once, so the
        // microphone, worker and speaker never drift apart into skipped or doubled speech.
        let queued = audio.playback_queued();
        let due = match queued {
            Some(queued) => PLAYBACK_QUEUE.saturating_sub(queued),
            None => usize::from(pacer.due(Instant::now())),
        };
        for _ in 0..due.min(MAX_FRAMES_PER_WAKE) {
            let mixed = mix(
                port,
                &mut decoders,
                f32::from_bits(controls.volume.load(Ordering::Relaxed)),
                events,
            );
            speaker_gate.observe(&mixed, Instant::now());
            audio.play(mixed);
        }
        if let Some(spurt) = activation {
            let gate = controls
                .echo_prevention
                .load(Ordering::Acquire)
                .then_some(&mut speaker_gate);
            send_captures(port, audio.as_mut(), &mut encoder, spurt, gate, events);
        }
        let now = Instant::now();
        pacer.completed(now);
        let wait = if queued.is_some() {
            POLL
        } else {
            pacer.wait(now).min(POLL)
        };
        std::thread::park_timeout(wait);
    }
    port.disable();
    let _ = audio.microphone(None);
    audio.deafen(true);
    drop(audio);
    let _ = events.try_send(VoiceEvent::Stopped);
}

fn send_captures(
    port: &VoicePort,
    audio: &mut dyn VoiceAudio,
    encoder: &mut SpeechEncoder,
    spurt: u32,
    mut gate: Option<&mut echo::SpeakerGate>,
    events: &mpsc::Sender<VoiceEvent>,
) {
    // Every fresh frame is sent, in order: dropping any cuts a hole into the words.
    for _ in 0..MAX_FRAMES_PER_WAKE {
        let Some(mut captured) = audio.capture() else {
            break;
        };
        if captured.activation != spurt {
            continue;
        }
        if captured.at.elapsed() > CAPTURE_FRESH {
            // Old captures never become delayed speech; listeners conceal the gap.
            let _ = encoder.skip(spurt);
            continue;
        }
        if let Some(gate) = gate.as_deref_mut() {
            gate.apply(&mut captured.samples, captured.at);
        }
        if let Ok(frame) = encoder.encode(&captured.samples, port.me(), spurt) {
            let _ = port.send(frame);
        } else {
            let _ = port.microphone(false);
            let _ = events.try_send(VoiceEvent::DeviceUnavailable {
                error: VoiceDeviceError::new("encoding voice frame", "Opus encoding failed"),
            });
            break;
        }
    }
}

fn mix(
    port: &VoicePort,
    decoders: &mut [Option<(u64, SpeechDecoder)>; MAX_RIDERS],
    volume: f32,
    events: &mpsc::Sender<VoiceEvent>,
) -> PcmFrame {
    let actions = port.playout();
    let mut mixed = [0.0; FRAME_SAMPLES];
    for (id, playback) in actions.into_iter().enumerate() {
        let action = playback.action;
        if matches!(action, Playout::Silence) {
            decoders[id] = None;
            continue;
        }
        if decoders[id]
            .as_ref()
            .is_none_or(|(epoch, _)| *epoch != playback.epoch)
        {
            decoders[id] = SpeechDecoder::new()
                .ok()
                .map(|decoder| (playback.epoch, decoder));
        }
        let Some((_, decoder)) = &mut decoders[id] else {
            continue;
        };
        let result = match action {
            Playout::Frame(frame) => decoder.decode(&frame),
            Playout::Recover(frame) => decoder.recover(&frame),
            Playout::Conceal => decoder.conceal(),
            Playout::Silence => continue,
        };
        if let Ok(samples) = result {
            for (sum, sample) in mixed.iter_mut().zip(samples) {
                *sum += sample;
            }
        } else {
            let peer = RiderId::new(u8::try_from(id).unwrap_or(0)).unwrap_or(RiderId::HOST);
            port.mute_peer(peer, true);
            decoders[id] = None;
            let _ = events.try_send(VoiceEvent::PeerMuted { peer });
        }
    }
    // Each voice keeps its level whoever else has a microphone open; only loud overlaps
    // are rounded off, never clipped.
    for sample in &mut mixed {
        *sample = limit(*sample * volume);
    }
    mixed
}

fn limit(sample: f32) -> f32 {
    const KNEE: f32 = 0.5;
    const ROOM: f32 = 0.4;
    let level = sample.abs();
    if level <= KNEE {
        sample
    } else {
        (KNEE + ROOM * ((level - KNEE) / ROOM).tanh()).copysign(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_keeps_quiet_speech_exact_and_rounds_off_loud_overlaps_below_full_scale() {
        for sample in [0.0, 0.1, -0.3, 0.5] {
            assert!((limit(sample) - sample).abs() < f32::EPSILON);
        }
        let mut previous = 0.0;
        for step in 0..=400 {
            #[allow(clippy::cast_precision_loss)]
            let value = limit(step as f32 / 100.0);
            assert!(value >= previous && value <= 0.9, "{value}");
            previous = value;
        }
        assert!((limit(-2.0) + limit(2.0)).abs() < f32::EPSILON);
    }
}
