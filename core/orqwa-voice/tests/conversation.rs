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
use orqwa_net::voice::VoiceFrame;
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
}
struct Audio {
    probe: Arc<Probe>,
    activation: Option<u32>,
    phase: f32,
    frequency: f32,
    deafened: bool,
}
impl VoiceAudio for Audio {
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        if activation.is_some() && self.activation != activation {
            self.probe.opened.fetch_add(1, Ordering::Relaxed);
        }
        self.activation = activation;
        self.probe
            .microphone
            .store(activation.is_some(), Ordering::Release);
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        let activation = self.activation?;
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
    let old = VoiceChat::with_audio(port.clone(), move |_| {
        Ok(Box::new(Audio {
            probe,
            activation: None,
            phase: 0.0,
            frequency: 440.0,
            deafened: false,
        }))
    })
    .await
    .unwrap();
    until(|| retired.stalled.load(Ordering::Acquire)).await;
    drop(old);
    let current = Arc::new(Probe::default());
    let probe = current.clone();
    let replacement = VoiceChat::with_audio(port.clone(), move |_| {
        Ok(Box::new(Audio {
            probe,
            activation: None,
            phase: 0.0,
            frequency: 660.0,
            deafened: false,
        }))
    })
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
        },
    )
    .await
    .unwrap()
}
async fn chat(lobby: &mut JoinerLobby, probe: &Arc<Probe>, frequency: f32) -> VoiceChat {
    let probe = probe.clone();
    let chat = VoiceChat::with_audio(lobby.take_voice().unwrap(), move |_| {
        Ok(Box::new(Audio {
            probe,
            activation: None,
            phase: 0.0,
            frequency,
            deafened: false,
        }))
    })
    .await
    .unwrap();
    chat.echo_prevention(false);
    chat
}
async fn until(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
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
    let frame = VoiceFrame::new(bad.me(), activation, 0, &[0xff]).unwrap();
    bad_port.send(frame).unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), ben_voice.next())
        .await
        .unwrap()
        .unwrap();
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
    assert!(!a.microphone.load(Ordering::Acquire));
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
    let audio = probe.clone();
    let test = MicrophoneTest::with_audio(move |_| {
        Ok(Box::new(Audio {
            probe: audio,
            activation: None,
            phase: 0.0,
            frequency: 440.0,
            deafened: false,
        }))
    })
    .await
    .unwrap();
    until(|| test.level() > 0.1).await;
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
    let audio = probe.clone();
    let test = MicrophoneTest::with_audio(move |_| {
        Ok(Box::new(Audio {
            probe: audio,
            activation: None,
            phase: 0.0,
            frequency: 440.0,
            deafened: false,
        }))
    })
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
