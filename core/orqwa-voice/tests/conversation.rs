//! Real pinned QUIC and Opus with simulated audio devices; no microphone or public server.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
use orqwa_voice::{VoiceChat, VoiceEvent};

#[derive(Default)]
struct Probe {
    opened: AtomicUsize,
    microphone: AtomicBool,
    heard: AtomicUsize,
    dropped: AtomicBool,
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
        true
    }
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
    VoiceChat::with_audio(lobby.take_voice().unwrap(), move |_| {
        Ok(Box::new(Audio {
            probe,
            activation: None,
            phase: 0.0,
            frequency,
            deafened: false,
        }))
    })
    .await
    .unwrap()
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
