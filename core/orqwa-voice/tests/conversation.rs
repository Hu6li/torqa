//! Real pinned QUIC and Opus with simulated audio devices; no microphone or public server.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use orqwa_domain::units::{Kilograms, Meters};
use orqwa_domain::voice::{CapturedVoice, PcmFrame, VoiceAudio, VoiceDeviceError};
use orqwa_net::code::RouteHash;
use orqwa_net::discovery::GatherOptions;
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{HostLobby, JoinerLobby, LobbyEvent, Rider};
use orqwa_net::policy::AddressPolicy;
use orqwa_net::state::{RiderState, STATE_EVERY};
use orqwa_net::voice::{FRAME_TIME, VoiceFrame};
use orqwa_voice::microphone_test::MicrophoneTest;
use orqwa_voice::{VoiceChat, VoiceEvent};

#[derive(Default)]
struct Probe {
    opened: AtomicUsize,
    microphone: AtomicBool,
    heard: AtomicUsize,
    dropped: AtomicBool,
    stall: Mutex<Option<mpsc::Receiver<()>>>,
    stalled: AtomicBool,
    invalidated: AtomicBool,
    recovered: AtomicUsize,
    played: AtomicUsize,
    captures: Mutex<Option<mpsc::Receiver<CapturedVoice>>>,
    captured: AtomicUsize,
}
struct Audio {
    probe: Arc<Probe>,
    activation: Option<u32>,
    opened: Option<Instant>,
    produced: u32,
    phase: f32,
    frequency: f32,
    deafened: bool,
}
fn audio(probe: Arc<Probe>, frequency: f32) -> Audio {
    Audio {
        probe,
        activation: None,
        opened: None,
        produced: 0,
        phase: 0.0,
        frequency,
        deafened: false,
    }
}
impl VoiceAudio for Audio {
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        if activation.is_some() && self.activation != activation {
            self.probe.opened.fetch_add(1, Ordering::Relaxed);
        }
        if self.activation != activation {
            self.opened = None;
            self.produced = 0;
        }
        self.activation = activation;
        self.probe
            .microphone
            .store(activation.is_some(), Ordering::Release);
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        let activation = self.activation?;
        if let Some(queue) = self.probe.captures.lock().unwrap().as_ref() {
            let frame = queue.try_recv().ok()?;
            self.probe.captured.fetch_add(1, Ordering::Release);
            return Some(frame);
        }
        // Like a real microphone, one frame per 20 ms; a stalled reader loses the oldest.
        let opened = *self.opened.get_or_insert_with(Instant::now);
        let due = u32::try_from(opened.elapsed().as_millis() / 20)
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        if self.produced >= due {
            return None;
        }
        self.produced = self.produced.max(due.saturating_sub(4)) + 1;
        let samples = std::array::from_fn(|_| {
            let sample = self.phase.sin() * 0.2;
            self.phase = (self.phase + self.frequency * std::f32::consts::TAU / 48_000.0)
                % std::f32::consts::TAU;
            sample
        });
        Some(CapturedVoice {
            activation,
            at: Instant::now(),
            samples,
        })
    }
    fn play(&mut self, samples: PcmFrame) {
        self.probe.played.fetch_add(1, Ordering::Relaxed);
        let stall = self.probe.stall.lock().unwrap().take();
        if let Some(stall) = stall {
            self.probe.stalled.store(true, Ordering::Release);
            let _ = stall.recv();
        }
        assert!(
            samples
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 0.9)
        );
        if !self.deafened && samples.iter().any(|sample| sample.abs() > 0.005) {
            self.probe.heard.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn deafen(&mut self, deafened: bool) {
        self.deafened = deafened;
    }
    fn healthy(&self) -> bool {
        !self.probe.invalidated.load(Ordering::Acquire)
    }
    fn recover(&mut self) -> Result<bool, VoiceDeviceError> {
        self.probe.invalidated.store(false, Ordering::Release);
        self.probe.recovered.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }
}

#[tokio::test]
async fn a_retired_audio_worker_cannot_disable_its_replacement_or_disconnect_the_group() {
    let mut host = host().await;
    let mut rider = join(&host, "Restarting audio").await;
    let port = rider.take_voice().unwrap();
    let retired = Arc::new(Probe::default());
    let (release, stalled) = mpsc::channel();
    *retired.stall.lock().unwrap() = Some(stalled);
    let probe = retired.clone();
    let old = VoiceChat::with_audio(port.clone(), move |_| Ok(Box::new(audio(probe, 440.0))))
        .await
        .unwrap();
    until(|| retired.stalled.load(Ordering::Acquire)).await;
    drop(old);
    let current = Arc::new(Probe::default());
    let probe = current.clone();
    let replacement =
        VoiceChat::with_audio(port.clone(), move |_| Ok(Box::new(audio(probe, 660.0))))
            .await
            .unwrap();
    replacement.microphone(true).unwrap();
    until(|| current.microphone.load(Ordering::Acquire)).await;
    release.send(()).unwrap();
    until(|| retired.dropped.load(Ordering::Acquire)).await;
    assert!(
        replacement.microphone_enabled(),
        "retired cleanup must not revoke new consent"
    );
    assert!(port.alive(), "audio restarts must preserve group ownership");
    replacement.close().await;
    let start = host.start(Duration::from_secs(1)).await.unwrap();
    tokio::time::sleep_until(start + STATE_EVERY).await;
    rider
        .send_state(RiderState {
            distance: Meters(23.0),
            ..RiderState::default()
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(host.next().await, Some(LobbyEvent::State { id, state }) if id == rider.me() && state.distance == Meters(23.0)) {
                break;
            }
        }
    }).await.unwrap();
    rider.close().await;
    host.close().await;
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.probe.microphone.store(false, Ordering::Release);
        self.probe.dropped.store(true, Ordering::Release);
    }
}
fn options() -> ConnectOptions {
    ConnectOptions {
        gather: GatherOptions {
            policy: AddressPolicy {
                lan: false,
                loopback: true,
            },
            stun_servers: vec![],
            ..GatherOptions::default()
        },
        window: Duration::from_secs(2),
    }
}
async fn host() -> HostLobby {
    HostLobby::open(
        Host::open("Host", "Voice test", RouteHash([9; 32]), &options())
            .await
            .unwrap(),
        Rider {
            name: "Host".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
    )
    .unwrap()
}
async fn join(host: &HostLobby, name: &str) -> JoinerLobby {
    let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
        .await
        .unwrap();
    host.admit(joiner.reply()).await.unwrap();
    JoinerLobby::join(
        joiner,
        Rider {
            name: name.into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
    )
    .await
    .unwrap()
}
async fn chat(lobby: &mut JoinerLobby, probe: &Arc<Probe>, frequency: f32) -> VoiceChat {
    let probe = probe.clone();
    let chat = VoiceChat::with_audio(lobby.take_voice().unwrap(), move |_| {
        Ok(Box::new(audio(probe, frequency)))
    })
    .await
    .unwrap();
    chat.echo_prevention(false);
    chat
}
#[track_caller]
fn until(predicate: impl Fn() -> bool) -> impl std::future::Future<Output = ()> {
    let caller = std::panic::Location::caller();
    async move {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !predicate() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|error| panic!("progress wait at {caller} failed: {error}"));
    }
}

#[tokio::test]
async fn speech_is_opt_in_does_not_echo_and_mute_deafen_and_voice_exit_preserve_the_ride() {
    // The host can relay speech without opening audio hardware itself.
    let mut host = host().await;
    let mut anna = join(&host, "Anna").await;
    let mut ben = join(&host, "Ben").await;
    let a = Arc::new(Probe::default());
    let b = Arc::new(Probe::default());
    let anna_voice = chat(&mut anna, &a, 440.0).await;
    let ben_voice = chat(&mut ben, &b, 660.0).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(a.opened.load(Ordering::Relaxed), 0);
    assert_eq!(b.opened.load(Ordering::Relaxed), 0);
    assert_eq!(b.heard.load(Ordering::Relaxed), 0);
    anna_voice.microphone(true).unwrap();
    until(|| b.heard.load(Ordering::Relaxed) >= 5).await;
    assert_eq!(a.heard.load(Ordering::Relaxed), 0, "no loopback echo");
    ben_voice.mute_peer(anna.me(), true);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let count = b.heard.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(b.heard.load(Ordering::Relaxed), count);
    ben_voice.mute_peer(anna.me(), false);
    until(|| b.heard.load(Ordering::Relaxed) > count + 3).await;
    ben_voice.microphone(true).unwrap();
    until(|| a.heard.load(Ordering::Relaxed) > 3).await;
    ben_voice.deafen(true);
    until(|| !b.microphone.load(Ordering::Acquire)).await;
    assert!(!ben_voice.microphone_enabled());
    ben_voice.deafen(false);
    assert!(!ben_voice.microphone_enabled());
    anna_voice.microphone(false).unwrap();
    until(|| !a.microphone.load(Ordering::Acquire)).await;
    anna_voice.microphone(true).unwrap();
    let count = b.heard.load(Ordering::Relaxed);
    until(|| b.heard.load(Ordering::Relaxed) > count + 3).await;
    assert!(anna_voice.volume(f32::NAN).is_err());
    assert!(anna_voice.volume(2.0).is_err());
    anna_voice.close().await;
    assert!(a.dropped.load(Ordering::Acquire));
    let start = host.start(Duration::from_secs(1)).await.unwrap();
    tokio::time::sleep_until(start + STATE_EVERY).await;
    anna.send_state(RiderState {
        distance: Meters(17.0),
        ..RiderState::default()
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(host.next().await,Some(LobbyEvent::State { id,.. }) if id==anna.me()) {
                break;
            }
        }
    })
    .await
    .unwrap();
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn sustained_joiner_microphone_preserves_an_organizer_that_is_listening() {
    let mut host = host().await;
    let mut peer = join(&host, "Speaking peer").await;
    let output = Arc::new(Probe::default());
    let probe = output.clone();
    let host_voice = VoiceChat::with_audio(host.take_voice().unwrap(), move |_| {
        Ok(Box::new(audio(probe, 440.0)))
    })
    .await
    .unwrap();
    let input = Arc::new(Probe::default());
    let peer_voice = chat(&mut peer, &input, 660.0).await;
    peer_voice.microphone(true).unwrap();
    let mut previous = 0;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(
            peer_voice.microphone_enabled(),
            "peer disconnected during sustained capture"
        );
        let heard = output.heard.load(Ordering::Acquire);
        assert!(
            heard > previous,
            "organizer keeps receiving the peer's audio"
        );
        previous = heard;
    }
    host.start(Duration::from_secs(1)).await.unwrap();
    peer_voice.close().await;
    host_voice.close().await;
    peer.close().await;
    host.close().await;
}

#[tokio::test]
async fn a_capture_backlog_sends_current_speech_in_order_but_never_stale_words() {
    let host = host().await;
    let mut anna = join(&host, "Queued capture").await;
    let mut ben = join(&host, "Listener").await;
    let a = Arc::new(Probe::default());
    let b = Arc::new(Probe::default());
    let (captures, queue) = mpsc::sync_channel(4);
    *a.captures.lock().unwrap() = Some(queue);
    let (release, stall) = mpsc::channel();
    *a.stall.lock().unwrap() = Some(stall);
    let anna_voice = chat(&mut anna, &a, 440.0).await;
    let ben_voice = chat(&mut ben, &b, 660.0).await;
    until(|| a.stalled.load(Ordering::Acquire)).await;
    anna_voice.microphone(true).unwrap();
    let tone: PcmFrame = std::array::from_fn(|index| {
        #[allow(clippy::cast_precision_loss)] // fixed 960-sample test frame
        let t = index as f32 / 48_000.0;
        (t * 440.0 * std::f32::consts::TAU).sin() * 0.2
    });
    let at = Instant::now();
    // A word captured before a long stall is too old to belong to the conversation.
    captures
        .send(CapturedVoice {
            activation: 1,
            at: at.checked_sub(Duration::from_millis(500)).unwrap(),
            samples: tone,
        })
        .unwrap();
    for _ in 0..3 {
        captures
            .send(CapturedVoice {
                activation: 1,
                at,
                samples: [0.0; 960],
            })
            .unwrap();
    }
    release.send(()).unwrap();
    until(|| a.captured.load(Ordering::Acquire) == 4).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        b.heard.load(Ordering::Acquire),
        0,
        "a stale word must not replay after a stall"
    );
    // A short backlog of current speech is all sent, none of it dropped for being queued.
    let (release, stall) = mpsc::channel();
    a.stalled.store(false, Ordering::Release);
    *a.stall.lock().unwrap() = Some(stall);
    until(|| a.stalled.load(Ordering::Acquire)).await;
    for _ in 0..4 {
        captures
            .send(CapturedVoice {
                activation: 1,
                at: Instant::now(),
                samples: tone,
            })
            .unwrap();
    }
    release.send(()).unwrap();
    until(|| b.heard.load(Ordering::Acquire) >= 4).await;
    // Fresh words keep crossing the real Opus/QUIC path afterwards.
    for _ in 0..15 {
        captures
            .send(CapturedVoice {
                activation: 1,
                at: Instant::now(),
                samples: tone,
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    until(|| b.heard.load(Ordering::Acquire) > 10).await;
    assert!(anna_voice.microphone_enabled());
    host.start(Duration::from_secs(1)).await.unwrap();
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn malformed_speech_mutes_only_that_source_and_group_drop_closes_live_microphones() {
    let host = host().await;
    let mut anna = join(&host, "Anna").await;
    let mut ben = join(&host, "Ben").await;
    let mut bad = join(&host, "Malformed source").await;
    let a = Arc::new(Probe::default());
    let b = Arc::new(Probe::default());
    let anna_voice = chat(&mut anna, &a, 440.0).await;
    let mut ben_voice = chat(&mut ben, &b, 660.0).await;
    anna_voice.microphone(true).unwrap();
    until(|| b.heard.load(Ordering::Relaxed) > 3).await;
    let bad_port = bad.take_voice().unwrap();
    bad_port.enable(true);
    let activation = bad_port.microphone(true).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Disposable datagrams and late playback may drop a single packet. Keep a
    // paced malformed source audible to the decoder until isolation is observed.
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        let mut tick = tokio::time::interval(FRAME_TIME);
        let mut sequence = 0;
        loop {
            tokio::select! {
                event = ben_voice.next() => break event.unwrap(),
                _ = tick.tick() => {
                    bad_port.send(VoiceFrame::new(bad.me(), activation, sequence, &[0xff]).unwrap()).unwrap();
                    sequence += 1;
                },
            }
        }
    }).await.unwrap();
    assert!(matches!(event,VoiceEvent::PeerMuted { peer } if peer==bad.me()));
    let before = b.heard.load(Ordering::Relaxed);
    until(|| b.heard.load(Ordering::Relaxed) > before + 3).await;
    anna.close().await;
    until(|| a.dropped.load(Ordering::Acquire)).await;
    assert!(!a.microphone.load(Ordering::Acquire));
    anna_voice.close().await;
    bad_port.disable();
    bad.close().await;
    ben_voice.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn a_full_group_can_talk_while_ride_state_keeps_flowing() {
    let mut host = host().await;
    let mut riders = Vec::new();
    let mut chats = Vec::new();
    let mut probes = Vec::new();
    for number in 1..8 {
        let mut rider = join(&host, &format!("Rider {number}")).await;
        let probe = Arc::new(Probe::default());
        let voice = chat(&mut rider, &probe, 440.0).await;
        riders.push(rider);
        chats.push(voice);
        probes.push(probe);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    for voice in &chats {
        voice.microphone(true).unwrap();
    }
    until(|| {
        probes
            .iter()
            .all(|probe| probe.heard.load(Ordering::Relaxed) > 5)
    })
    .await;
    let start = host.start(Duration::from_secs(1)).await.unwrap();
    tokio::time::sleep_until(start + STATE_EVERY).await;
    for rider in &riders {
        rider
            .send_state(RiderState {
                distance: Meters(19.0),
                ..RiderState::default()
            })
            .unwrap();
    }
    let mut received = [false; 8];
    tokio::time::timeout(Duration::from_secs(2), async {
        while received.iter().filter(|received| **received).count() < 7 {
            if let Some(LobbyEvent::State { id, .. }) = host.next().await {
                received[usize::from(id.value())] = true;
            }
        }
    })
    .await
    .unwrap();
    // All live audio workers can stall and resume without replaying a catch-up
    // conversation or revoking consent. Network/state actors keep running.
    let mut releases = Vec::new();
    for probe in &probes {
        let (release, stall) = mpsc::channel();
        *probe.stall.lock().unwrap() = Some(stall);
        releases.push(release);
    }
    until(|| {
        probes
            .iter()
            .all(|probe| probe.stalled.load(Ordering::Acquire))
    })
    .await;
    for rider in &riders {
        rider
            .send_state(RiderState {
                at: STATE_EVERY,
                distance: Meters(27.0),
                ..RiderState::default()
            })
            .unwrap();
    }
    let mut updated = [false; 8];
    tokio::time::timeout(Duration::from_secs(2), async {
        while updated.iter().filter(|updated| **updated).count() < 7 {
            if let Some(LobbyEvent::State { id, state }) = host.next().await
                && state.distance == Meters(27.0)
            {
                updated[usize::from(id.value())] = true;
            }
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let heard: Vec<_> = probes
        .iter()
        .map(|probe| probe.heard.load(Ordering::Acquire))
        .collect();
    for release in releases {
        release.send(()).unwrap();
    }
    until(|| {
        probes
            .iter()
            .zip(&heard)
            .all(|(probe, before)| probe.heard.load(Ordering::Acquire) > before + 3)
    })
    .await;
    assert!(chats.iter().all(VoiceChat::microphone_enabled));
    for voice in chats {
        voice.close().await;
    }
    for rider in riders {
        rider.close().await;
    }
    host.close().await;
}

#[tokio::test]
async fn a_device_rate_change_recovers_without_rejoining_or_reopening_microphone_consent() {
    let host = host().await;
    let mut anna = join(&host, "Anna").await;
    let mut ben = join(&host, "Ben").await;
    let a = Arc::new(Probe::default());
    let b = Arc::new(Probe::default());
    let anna_voice = chat(&mut anna, &a, 440.0).await;
    let ben_voice = chat(&mut ben, &b, 660.0).await;
    anna_voice.microphone(true).unwrap();
    until(|| b.heard.load(Ordering::Relaxed) >= 5).await;
    a.invalidated.store(true, Ordering::Release);
    until(|| a.recovered.load(Ordering::Relaxed) == 1).await;
    let before = b.heard.load(Ordering::Relaxed);
    until(|| b.heard.load(Ordering::Relaxed) >= before + 5).await;
    assert!(anna_voice.microphone_enabled());
    assert_eq!(a.opened.load(Ordering::Relaxed), 1);
    anna_voice.microphone(false).unwrap();
    a.invalidated.store(true, Ordering::Release);
    until(|| a.recovered.load(Ordering::Relaxed) == 2).await;
    assert!(
        !anna_voice.microphone_enabled(),
        "recovery cannot unmute a muted microphone"
    );
    // The worker may rebuild the devices one wakeup before it applies the mute.
    until(|| !a.microphone.load(Ordering::Acquire)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !a.microphone.load(Ordering::Acquire),
        "the muted microphone stays closed after recovery"
    );
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn startup_failures_preserve_their_causes_and_leave_microphones_gated_off() {
    let mut host = host().await;
    let port = host.take_voice().unwrap();
    let failed = VoiceChat::with_audio(port.clone(), |_| {
        Err(VoiceDeviceError::new(
            "opening speakers",
            "AAudio permission denied",
        ))
    })
    .await;
    let error = failed.err().expect("the backend rejected startup");
    assert!(
        error
            .to_string()
            .contains("opening speakers: AAudio permission denied")
    );
    assert!(port.transmitting().is_none());
    let failed = VoiceChat::with_audio(port.clone(), |_| {
        panic!("android context was not initialized")
    })
    .await;
    let error = failed
        .err()
        .expect("the worker panic must become a diagnostic");
    assert!(
        error
            .to_string()
            .contains("android context was not initialized")
    );
    assert!(port.transmitting().is_none());
    host.close().await;
}

#[tokio::test]
async fn speaker_mode_drops_feedback_but_releases_the_microphone_after_the_echo_tail() {
    let host = host().await;
    let mut anna = join(&host, "Anna").await;
    let mut ben = join(&host, "Ben").await;
    let a = Arc::new(Probe::default());
    let b = Arc::new(Probe::default());
    let anna_voice = chat(&mut anna, &a, 440.0).await;
    let ben_voice = chat(&mut ben, &b, 660.0).await;
    ben_voice.echo_prevention(true);
    anna_voice.microphone(true).unwrap();
    until(|| b.heard.load(Ordering::Relaxed) >= 5).await;
    ben_voice.microphone(true).unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        a.heard.load(Ordering::Relaxed),
        0,
        "speaker feedback is not sent back to Anna"
    );
    anna_voice.microphone(false).unwrap();
    until(|| a.heard.load(Ordering::Relaxed) >= 5).await;
    assert!(
        ben_voice.microphone_enabled(),
        "echo prevention does not revoke consent"
    );
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn microphone_test_is_temporary_local_and_never_plays_captured_samples() {
    let probe = Arc::new(Probe::default());
    let device = probe.clone();
    let test = MicrophoneTest::with_audio(move |_| Ok(Box::new(audio(device, 440.0))))
        .await
        .unwrap();
    until(|| test.level() > 0.1).await;
    until(|| test.voice_detected()).await;
    assert!(test.voice_heard());
    assert!(test.level().is_finite() && test.level() <= 1.0);
    assert_eq!(probe.played.load(Ordering::Relaxed), 0);
    test.close().await;
    assert!(probe.dropped.load(Ordering::Acquire));
    assert!(!probe.microphone.load(Ordering::Acquire));
    assert_eq!(probe.played.load(Ordering::Relaxed), 0);
    let failure = MicrophoneTest::with_audio(|_| {
        Err(VoiceDeviceError::new(
            "opening test microphone",
            "permission denied",
        ))
    })
    .await
    .err()
    .unwrap();
    assert!(
        failure
            .to_string()
            .contains("opening test microphone: permission denied")
    );
}

#[tokio::test]
async fn microphone_test_closes_itself_after_ten_seconds() {
    let probe = Arc::new(Probe::default());
    let device = probe.clone();
    let test = MicrophoneTest::with_audio(move |_| Ok(Box::new(audio(device, 440.0))))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(12), async {
        while test.running() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(probe.dropped.load(Ordering::Acquire));
    assert!(!probe.microphone.load(Ordering::Acquire));
    assert_eq!(test.level(), 0.0);
    assert_eq!(probe.played.load(Ordering::Relaxed), 0);
}

/// A simulated sound card whose microphone and speaker each run on their own clock, as real
/// hardware does, so their rates differ slightly from each other and from the other rider's.
struct SoundCard {
    activation: Option<u32>,
    started: Instant,
    capture_period: Duration,
    captured: u32,
    voice: fn(u32) -> PcmFrame,
    play_period: Duration,
    state: std::cell::RefCell<Speaker>,
    heard: Arc<Mutex<Vec<Played>>>,
}
#[derive(Default)]
struct Speaker {
    queue: std::collections::VecDeque<f32>,
    ticks: u32,
    /// When the voice worker last came to the speaker.
    visited: Option<Instant>,
}
/// What a simulated speaker played in one period.
#[derive(Clone, Copy)]
enum Played {
    /// A frame at this RMS level.
    Level(f32),
    /// Nothing was queued although the voice worker kept coming: a gap in the speech.
    Gap,
    /// Nothing was queued after the worker stayed away for longer than the two frames it keeps
    /// queued last: the machine did not run it, as loaded CI machines now and then do not.
    Stall,
}
impl SoundCard {
    fn new(
        capture_period: Duration,
        play_period: Duration,
        voice: fn(u32) -> PcmFrame,
        heard: Arc<Mutex<Vec<Played>>>,
    ) -> Self {
        Self {
            activation: None,
            started: Instant::now(),
            capture_period,
            captured: 0,
            voice,
            play_period,
            state: std::cell::RefCell::default(),
            heard,
        }
    }
    fn ticks(&self, period: Duration) -> u32 {
        u32::try_from(self.started.elapsed().as_nanos() / period.as_nanos()).unwrap()
    }
    /// The speaker plays one queued frame per period; an empty queue is heard as a gap, or a
    /// stall if the worker was away for longer than its queue lasts.
    fn advance(&self) {
        let due = self.ticks(self.play_period);
        let mut speaker = self.state.borrow_mut();
        let visited = speaker
            .visited
            .replace(Instant::now())
            .unwrap_or(self.started);
        while speaker.ticks < due {
            speaker.ticks += 1;
            let at = self.started + self.play_period * speaker.ticks;
            let played = speaker.queue.pop_front().map_or_else(
                || {
                    if at.saturating_duration_since(visited) > self.play_period * 2 {
                        Played::Stall
                    } else {
                        Played::Gap
                    }
                },
                Played::Level,
            );
            self.heard.lock().unwrap().push(played);
        }
    }
}
impl VoiceAudio for SoundCard {
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        if activation.is_some() && self.activation.is_none() {
            self.captured = self.ticks(self.capture_period);
        }
        self.activation = activation;
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        let activation = self.activation?;
        if self.captured >= self.ticks(self.capture_period) {
            return None;
        }
        self.captured += 1;
        Some(CapturedVoice {
            activation,
            at: Instant::now(),
            samples: (self.voice)(self.captured),
        })
    }
    fn play(&mut self, frame: PcmFrame) {
        self.advance();
        let energy: f32 = frame.iter().map(|sample| sample * sample).sum();
        #[allow(clippy::cast_precision_loss)]
        let level = (energy / frame.len() as f32).sqrt();
        self.state.borrow_mut().queue.push_back(level);
    }
    fn playback_queued(&self) -> Option<usize> {
        self.advance();
        Some(self.state.borrow().queue.len())
    }
    fn deafen(&mut self, _deafened: bool) {}
    fn healthy(&self) -> bool {
        true
    }
}

/// A steady 300 Hz voice, continuous from frame to frame, 0.2 at its peak.
fn steady_voice(frame: u32) -> PcmFrame {
    std::array::from_fn(|index| {
        #[allow(clippy::cast_precision_loss)]
        let t = (u64::from(frame) * 960 + index as u64) as f64 / 48_000.0;
        #[allow(clippy::cast_possible_truncation)]
        let value = ((t * 300.0 * std::f64::consts::TAU).sin() * 0.2) as f32;
        value
    })
}
/// A fan behind the rider: broadband noise at about -31 dBFS and no speech.
fn fan(frame: u32) -> PcmFrame {
    let mut state = frame.wrapping_mul(2_654_435_761).wrapping_add(1);
    std::array::from_fn(|_| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        #[allow(clippy::cast_precision_loss)]
        let noise = (state >> 8) as f32 / (1 << 24) as f32 - 0.5;
        noise * 0.1
    })
}

async fn sound_card(
    lobby: &mut JoinerLobby,
    capture_ms: f64,
    play_ms: f64,
    voice: fn(u32) -> PcmFrame,
) -> (VoiceChat, Arc<Mutex<Vec<Played>>>) {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let chat = VoiceChat::with_audio(lobby.take_voice().unwrap(), move |_| {
        Ok(Box::new(SoundCard::new(
            Duration::from_secs_f64(capture_ms / 1000.0),
            Duration::from_secs_f64(play_ms / 1000.0),
            voice,
            output,
        )))
    })
    .await
    .unwrap();
    (chat, heard)
}
/// Output frames heard from `from`.
struct Heard {
    /// Nothing to play although the worker kept coming.
    gaps: usize,
    /// Nothing to play while the machine did not run the worker.
    stalls: usize,
    /// Played, but quiet.
    soft: usize,
    total: usize,
}
fn listened(heard: &Mutex<Vec<Played>>, from: usize, quiet: f32) -> Heard {
    let heard = heard.lock().unwrap();
    let window = &heard[from.min(heard.len())..];
    let count = |test: fn(Played, f32) -> bool| {
        window.iter().filter(|played| test(**played, quiet)).count()
    };
    Heard {
        gaps: count(|played, _| matches!(played, Played::Gap)),
        stalls: count(|played, _| matches!(played, Played::Stall)),
        soft: count(|played, quiet| matches!(played, Played::Level(level) if level < quiet)),
        total: window.len(),
    }
}

#[tokio::test]
async fn speech_stays_continuous_when_every_sound_card_runs_at_its_own_rate() {
    let host = host().await;
    let mut anna = join(&host, "Anna").await;
    let mut ben = join(&host, "Ben").await;
    // Anna's microphone runs fast, Ben's speaker runs faster still: his queue keeps running
    // dry unless playback follows his own speaker and stretches the odd frame.
    let (anna_voice, _) = sound_card(&mut anna, 19.98, 20.02, steady_voice).await;
    let (ben_voice, heard) = sound_card(&mut ben, 20.02, 19.96, steady_voice).await;
    anna_voice.echo_prevention(false);
    ben_voice.echo_prevention(false);
    anna_voice.microphone(true).unwrap();
    until(|| listened(&heard, 0, 0.04).total > 0).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let from = heard.lock().unwrap().len();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let Heard {
        gaps,
        stalls,
        soft,
        total,
    } = listened(&heard, from, 0.04);
    assert!(total >= 240, "Ben's speaker played {total} frames");
    // The steady voice must come through whole: no gaps, no dropped or concealed words. A
    // machine that stops running the worker is beyond what any pacing can make up for.
    assert!(
        gaps + soft <= 3,
        "{gaps} gaps and {soft} quiet frames in {total}, {stalls} more while stalled"
    );
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

#[tokio::test]
async fn background_noise_on_an_open_microphone_does_not_silence_the_other_rider() {
    let host = host().await;
    let mut anna = join(&host, "Anna, by a fan").await;
    let mut ben = join(&host, "Ben, talking").await;
    // Both on speakers with echo prevention, both microphones open. Anna is quiet, but her
    // fan is loud enough that any played sound would have held Ben's microphone forever.
    let (anna_voice, heard) = sound_card(&mut anna, 20.0, 20.0, fan).await;
    let (ben_voice, _) = sound_card(&mut ben, 20.0, 20.0, steady_voice).await;
    anna_voice.echo_prevention(true);
    ben_voice.echo_prevention(true);
    anna_voice.microphone(true).unwrap();
    ben_voice.microphone(true).unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let from = heard.lock().unwrap().len();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let Heard { soft, total, .. } = listened(&heard, from, 0.04);
    assert!(total >= 90, "Anna's speaker played {total} frames");
    assert!(soft <= 3, "Ben was silent in {soft} of {total} frames");
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}

/// A rider talking: words of 300 ms, 200 ms apart, with their quiet room between them.
fn talking(frame: u32) -> PcmFrame {
    if frame % 25 < 15 {
        steady_voice(frame)
    } else {
        fan(frame).map(|sample| sample * 0.1)
    }
}

#[tokio::test]
async fn riders_are_shown_speaking_while_their_words_play_and_never_for_their_fan() {
    let host = host().await;
    let mut anna = join(&host, "Anna, by a fan").await;
    let mut ben = join(&host, "Ben, talking").await;
    let (anna_voice, _) = sound_card(&mut anna, 20.0, 20.0, fan).await;
    let (ben_voice, _) = sound_card(&mut ben, 20.0, 20.0, talking).await;
    // On headphones, so neither microphone is held while the other plays.
    anna_voice.echo_prevention(false);
    ben_voice.echo_prevention(false);
    let (a, b) = (
        usize::from(anna.me().value()),
        usize::from(ben.me().value()),
    );
    assert!(
        !anna_voice.speaking().contains(&true),
        "nobody before a microphone opens"
    );
    anna_voice.microphone(true).unwrap();
    ben_voice.microphone(true).unwrap();
    // Ben sees himself speaking as he is sent, Anna as she hears him.
    until(|| anna_voice.speaking()[b] && ben_voice.speaking()[b]).await;
    // For two seconds Anna's fan plays at Ben's: she is never shown speaking, and Ben stays
    // shown through his words and the pauses between them.
    let mut shown = 0;
    for _ in 0..100 {
        assert!(!ben_voice.speaking()[a], "Anna's fan, as Ben hears it");
        assert!(!anna_voice.speaking()[a], "Anna's fan, as she sends it");
        shown += usize::from(anna_voice.speaking()[b]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(shown >= 95, "Ben shown speaking in {shown} of 100 looks");
    // Muted here, Ben is not shown here; he is again when unmuted.
    anna_voice.mute_peer(ben.me(), true);
    until(|| !anna_voice.speaking()[b]).await;
    anna_voice.mute_peer(ben.me(), false);
    until(|| anna_voice.speaking()[b]).await;
    // A closed microphone is nobody speaking.
    ben_voice.microphone(false).unwrap();
    until(|| !anna_voice.speaking()[b] && !ben_voice.speaking()[b]).await;
    anna_voice.close().await;
    ben_voice.close().await;
    anna.close().await;
    ben.close().await;
    host.close().await;
}
