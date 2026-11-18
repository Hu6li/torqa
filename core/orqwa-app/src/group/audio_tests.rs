use super::*;
use orqwa_domain::units::Kilograms;
use orqwa_domain::voice::{CapturedVoice, PcmFrame, VoiceAudio, VoiceDeviceError};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Capture {
    live: Arc<AtomicUsize>,
    activation: Option<u32>,
    opened: Option<std::time::Instant>,
    produced: u32,
    phase: f32,
    played: Arc<AtomicUsize>,
}
impl VoiceAudio for Capture {
    fn microphone(&mut self, activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        if self.activation.is_none() && activation.is_some() {
            self.live.fetch_add(1, Ordering::AcqRel);
        }
        if self.activation.is_some() && activation.is_none() {
            self.live.fetch_sub(1, Ordering::AcqRel);
        }
        if self.activation != activation {
            self.opened = None;
            self.produced = 0;
        }
        self.activation = activation;
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        let activation = self.activation?;
        // Like a real microphone, one frame per 20 ms.
        let opened = *self.opened.get_or_insert_with(std::time::Instant::now);
        let due = u32::try_from(opened.elapsed().as_millis() / 20)
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        if self.produced >= due {
            return None;
        }
        self.produced = self.produced.max(due.saturating_sub(4)) + 1;
        let samples = std::array::from_fn(|_| {
            let sample = self.phase.sin() * 0.1 + (self.phase * 3.0).sin() * 0.04;
            self.phase =
                (self.phase + std::f32::consts::TAU * 180.0 / 48_000.0) % std::f32::consts::TAU;
            sample
        });
        Some(CapturedVoice {
            activation,
            at: std::time::Instant::now(),
            samples,
        })
    }
    fn play(&mut self, samples: PcmFrame) {
        if samples.iter().any(|sample| sample.abs() > 0.001) {
            self.played.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn deafen(&mut self, _deafened: bool) {}
    fn healthy(&self) -> bool {
        true
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.microphone(None);
    }
}

async fn wait(updates: &mut watch::Receiver<Snapshot>, predicate: impl Fn(&Snapshot) -> bool) {
    tokio::time::timeout(Duration::from_secs(3), updates.wait_for(predicate))
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn joining_voice_supersedes_an_audio_check_even_when_its_stop_command_is_stale() {
    for kind in [TestKind::Microphone, TestKind::Speakers] {
        for mic in [false, true] {
            join_after_check(kind, mic, false).await;
        }
    }
}

#[tokio::test]
async fn joining_voice_after_a_check_reports_device_failure_without_ending_the_group() {
    for kind in [TestKind::Microphone, TestKind::Speakers] {
        join_after_check(kind, false, true).await;
    }
}

#[allow(clippy::too_many_lines)] // one lifecycle keeps check, consent, startup failure and cleanup assertions together
async fn join_after_check(kind: TestKind, mic: bool, voice_fails: bool) {
    let directory = std::env::temp_dir().join(format!(
        "orqwa-join-after-check-{:x?}",
        orqwa_net::code::RideId::generate().unwrap().0
    ));
    let live = Arc::new(AtomicUsize::new(0));
    let capture = live.clone();
    let opened = Arc::new(AtomicUsize::new(0));
    let opening = opened.clone();
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    let controls = Arc::new(VoiceControls::default());
    let (_choices, choices) = watch::channel(None);
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                assert_eq!(
                    capture.load(Ordering::Acquire),
                    0,
                    "release test capture first"
                );
                if opening.fetch_add(1, Ordering::AcqRel) > 0 && voice_fails {
                    return Err(VoiceDeviceError::new(
                        "opening output",
                        "selected device unavailable",
                    ));
                }
                Ok(Box::new(Capture {
                    live: capture.clone(),
                    activation: None,
                    opened: None,
                    produced: 0,
                    phase: 0.0,
                    played: Arc::default(),
                }))
            })),
        },
        voice_controls: controls.clone(),
        invite: None,
        rider: Rider {
            name: "Solo organizer".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
        options,
        data: directory.join("data"),
        temporary: directory.join("temporary"),
        choices,
    };
    let (commands, receive) = mpsc::channel(8);
    let (send, mut updates) = watch::channel(Snapshot::default());
    let (_local, local) = watch::channel(None);
    let task = tokio::spawn(run(request, receive, send, local));
    wait(&mut updates, |s| s.phase == Phase::Waiting).await;
    commands
        .send(Command::AudioTest(0, Some(kind)))
        .await
        .unwrap();
    wait(&mut updates, |s| match kind {
        TestKind::Microphone => s.microphone_test && s.microphone_level > 0.0,
        TestKind::Speakers => s.speaker_test && opened.load(Ordering::Acquire) > 0,
    })
    .await;
    // The UI stops the check then joins immediately, advancing the shared generation twice
    // before the actor reads either command. Its stale stop must not swallow the join.
    controls.generation.store(2, Ordering::Release);
    controls.audio_test.store(0, Ordering::Release);
    commands.send(Command::AudioTest(1, None)).await.unwrap();
    commands
        .send(Command::Voice(2, true, mic, false, 0.4))
        .await
        .unwrap();
    wait(&mut updates, |s| s.voice || !s.error.is_empty()).await;
    let status = updates.borrow().clone();
    if voice_fails {
        assert!(!status.voice && !status.voice_pending && !status.microphone);
        assert_eq!(status.failure.unwrap().kind, crate::Kind::GroupVoice);
        assert!(status.error.contains("selected device unavailable"));
    } else {
        assert!(status.voice, "voice startup failed: {}", status.error);
        assert_eq!(
            status.microphone, mic,
            "preserve the explicit microphone choice"
        );
    }
    assert!(
        status.phase == Phase::Waiting,
        "audio failure preserves the group"
    );
    assert!(!status.microphone_test && !status.speaker_test);
    assert_eq!(status.roster.iter().flatten().count(), 1);
    drop(commands);
    task.await.unwrap();
    // Dropping the owner signals the audio thread; its capture closes on that thread.
    tokio::time::timeout(Duration::from_secs(1), async {
        while live.load(Ordering::Acquire) > 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("leaving releases capture within the cleanup deadline");
    assert_eq!(live.load(Ordering::Acquire), 0, "leaving releases capture");
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn joiner_listening_keeps_the_group_connected_and_clears_roster_when_removed() {
    let directory = std::env::temp_dir().join(format!(
        "orqwa-peer-audio-{:x?}",
        orqwa_net::code::RideId::generate().unwrap().0
    ));
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    let host = Host::lobby("Organizer", &options).await.unwrap();
    let host = HostLobby::open(
        host,
        Rider {
            name: "Organizer".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
    )
    .unwrap();
    let live = Arc::new(AtomicUsize::new(0));
    let capture = live.clone();
    let choices = watch::channel(None);
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                Ok(Box::new(Capture {
                    live: capture.clone(),
                    activation: None,
                    opened: None,
                    produced: 0,
                    phase: 0.0,
                    played: Arc::default(),
                }))
            })),
        },
        voice_controls: Arc::default(),
        invite: Some((
            host.invite().to_code(options.gather.policy).unwrap(),
            orqwa_net::link::Credential::generate().unwrap(),
        )),
        rider: Rider {
            name: "Listening peer".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
        options: options.clone(),
        data: directory.join("data"),
        temporary: directory.join("temporary"),
        choices: choices.1,
    };
    let (commands, receive) = mpsc::channel(8);
    let (send, mut updates) = watch::channel(Snapshot::default());
    let (_local, local) = watch::channel(None);
    let task = tokio::spawn(run(request, receive, send, local));
    wait(&mut updates, |status| !status.reply.is_empty()).await;
    let reply = Reply::from_code(&updates.borrow().reply, options.gather.policy).unwrap();
    host.admit(&reply).await.unwrap();
    wait(&mut updates, |status| status.phase == Phase::Waiting).await;
    commands
        .send(Command::Voice(0, true, false, false, 0.6))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice || !status.error.is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    let status = updates.borrow().clone();
    assert!(status.voice, "listening failed: {}", status.error);
    assert!(status.phase == Phase::Waiting, "{}", status.error);
    assert_eq!(
        live.load(Ordering::Acquire),
        0,
        "listening opens no microphone"
    );
    assert_eq!(status.roster.iter().flatten().count(), 2);
    host.remove(RiderId::new(status.me).unwrap()).unwrap();
    wait(&mut updates, |status| status.phase == Phase::Ended).await;
    assert_eq!(updates.borrow().roster.iter().flatten().count(), 1);
    drop(commands);
    task.await.unwrap();
    host.close().await;
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one lifecycle checks exclusive capture and retained controls across start/stop/leave
async fn audio_checks_restore_voice_with_the_microphone_as_it_was_and_leaving_releases_capture() {
    let directory = std::env::temp_dir().join(format!(
        "orqwa-mic-ui-{:x?}",
        orqwa_net::code::RideId::generate().unwrap().0
    ));
    let capture = Arc::new(AtomicUsize::new(0));
    let live = capture.clone();
    let played = Arc::new(AtomicUsize::new(0));
    let output = played.clone();
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    let choices = watch::channel(None);
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                Ok(Box::new(Capture {
                    live: live.clone(),
                    activation: None,
                    opened: None,
                    produced: 0,
                    phase: 0.0,
                    played: output.clone(),
                }))
            })),
        },
        voice_controls: Arc::default(),
        invite: None,
        rider: Rider {
            name: "Testing audio".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
        options,
        data: directory.join("data"),
        temporary: directory.join("temporary"),
        choices: choices.1,
    };
    let (commands, receive) = mpsc::channel(8);
    let (send, mut updates) = watch::channel(Snapshot::default());
    let (_local, local) = watch::channel(None);
    let task = tokio::spawn(run(request, receive, send, local));
    wait(&mut updates, |status| status.phase == Phase::Waiting).await;
    commands
        .send(Command::Voice(0, true, false, false, 0.4))
        .await
        .unwrap();
    wait(&mut updates, |status| status.voice).await;
    commands
        .send(Command::ChooseDevices(
            0,
            "test-input".into(),
            "test-output".into(),
            false,
        ))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice && status.input_device == "test-input"
    })
    .await;
    commands.send(Command::EchoPrevention(false)).await.unwrap();
    commands
        .send(Command::Voice(0, true, true, false, 0.4))
        .await
        .unwrap();
    wait(&mut updates, |status| status.microphone).await;
    commands
        .send(Command::AudioTest(0, Some(TestKind::Microphone)))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.microphone_test && status.microphone_level > 0.1 && status.microphone_voice_detected
    })
    .await;
    assert!(!updates.borrow().voice && !updates.borrow().microphone);
    assert_eq!(
        capture.load(Ordering::Acquire),
        1,
        "only the local test owns capture"
    );
    commands.send(Command::AudioTest(0, None)).await.unwrap();
    // The microphone was on before the check, so voice sends again afterwards (R188, R194).
    wait(&mut updates, |status| {
        status.voice && !status.microphone_test && status.microphone
    })
    .await;
    let status = updates.borrow().clone();
    assert!(!status.echo_prevention);
    assert!(status.microphone_voice_heard && status.microphone_checked);
    assert!((status.voice_volume - 0.4).abs() < f32::EPSILON);
    assert_eq!(status.input_device, "test-input");
    assert_eq!(status.output_device, "test-output");
    // The voice worker opens capture as it takes the restored microphone up.
    tokio::time::timeout(Duration::from_secs(2), async {
        while capture.load(Ordering::Acquire) != 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("voice owns capture again");
    commands
        .send(Command::AudioTest(0, Some(TestKind::Speakers)))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.speaker_test && played.load(Ordering::Relaxed) > 2
    })
    .await;
    assert_eq!(
        capture.load(Ordering::Acquire),
        0,
        "speaker testing never opens capture"
    );
    assert!(!updates.borrow().microphone_voice_detected);
    commands.send(Command::AudioTest(0, None)).await.unwrap();
    wait(&mut updates, |status| {
        status.voice && !status.speaker_test && status.microphone
    })
    .await;
    // Muted by the rider, the microphone stays off after a check.
    commands
        .send(Command::Voice(0, true, false, false, 0.4))
        .await
        .unwrap();
    wait(&mut updates, |status| !status.microphone).await;
    commands
        .send(Command::AudioTest(0, Some(TestKind::Speakers)))
        .await
        .unwrap();
    wait(&mut updates, |status| status.speaker_test).await;
    commands.send(Command::AudioTest(0, None)).await.unwrap();
    wait(&mut updates, |status| status.voice && !status.speaker_test).await;
    assert!(!updates.borrow().microphone);
    commands
        .send(Command::AudioTest(0, Some(TestKind::Microphone)))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.microphone_test && status.microphone_level > 0.1
    })
    .await;
    drop(commands);
    task.await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while capture.load(Ordering::Acquire) > 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn joining_with_the_microphone_on_sends_at_once_and_shows_its_level() {
    let directory = std::env::temp_dir().join(format!(
        "orqwa-mic-join-{:x?}",
        orqwa_net::code::RideId::generate().unwrap().0
    ));
    let capture = Arc::new(AtomicUsize::new(0));
    let live = capture.clone();
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    let choices = watch::channel(None);
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                Ok(Box::new(Capture {
                    live: live.clone(),
                    activation: None,
                    opened: None,
                    produced: 0,
                    phase: 0.0,
                    played: Arc::default(),
                }))
            })),
        },
        voice_controls: Arc::default(),
        invite: None,
        rider: Rider {
            name: "Talking at once".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
        options,
        data: directory.join("data"),
        temporary: directory.join("temporary"),
        choices: choices.1,
    };
    let (commands, receive) = mpsc::channel(8);
    let (send, mut updates) = watch::channel(Snapshot::default());
    let (_local, local) = watch::channel(None);
    let task = tokio::spawn(run(request, receive, send, local));
    wait(&mut updates, |status| status.phase == Phase::Waiting).await;
    // Chosen in the join window (R194): voice opens with the microphone sending.
    commands
        .send(Command::Voice(0, true, true, false, 0.6))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice
            && status.microphone
            && status.microphone_level > 0.1
            && status.microphone_voice_detected
    })
    .await;
    assert!(!updates.borrow().microphone_test, "no separate check runs");
    assert_eq!(capture.load(Ordering::Acquire), 1);
    commands
        .send(Command::Voice(0, true, false, false, 0.6))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice && !status.microphone && status.microphone_level < 0.001
    })
    .await;
    assert!(!updates.borrow().microphone_voice_detected);
    drop(commands);
    task.await.unwrap();
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

/// The system's default output, which it moves as when headphones are plugged in, and the
/// device the rider chose, if any.
struct Outputs {
    default: String,
    chosen: Option<String>,
}

/// Audio that names its own output, like the native backend: the device chosen when it opened,
/// else the system's default as it moves. Voice closing for another device never learns that
/// device's name, so the voice replacing it still tells it.
struct Rerouted {
    outputs: Arc<std::sync::Mutex<Outputs>>,
    chosen: Option<String>,
    told: Option<String>,
}
impl VoiceAudio for Rerouted {
    fn microphone(&mut self, _activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        None
    }
    fn play(&mut self, _samples: PcmFrame) {}
    fn deafen(&mut self, _deafened: bool) {}
    fn healthy(&self) -> bool {
        true
    }
    fn output_changed(&mut self) -> Option<String> {
        let name = self.chosen.clone().unwrap_or_else(|| {
            self.outputs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .default
                .clone()
        });
        if self.told.as_ref() == Some(&name) {
            return None;
        }
        self.told = Some(name.clone());
        Some(name)
    }
}

#[tokio::test]
async fn friends_are_heard_on_the_output_the_system_moves_to_and_the_rider_learns_of_it() {
    let directory = std::env::temp_dir().join(format!(
        "orqwa-rerouted-{:x?}",
        orqwa_net::code::RideId::generate().unwrap().0
    ));
    let outputs = Arc::new(std::sync::Mutex::new(Outputs {
        default: "Laptop speakers".to_owned(),
        chosen: None,
    }));
    let shared = outputs.clone();
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    let choices = watch::channel(None);
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                let chosen = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .chosen
                    .clone();
                Ok(Box::new(Rerouted {
                    outputs: shared.clone(),
                    chosen,
                    told: None,
                }))
            })),
        },
        voice_controls: Arc::default(),
        invite: None,
        rider: Rider {
            name: "Listening".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        },
        options,
        data: directory.join("data"),
        temporary: directory.join("temporary"),
        choices: choices.1,
    };
    let (commands, receive) = mpsc::channel(8);
    let (send, mut updates) = watch::channel(Snapshot::default());
    let (_local, local) = watch::channel(None);
    let task = tokio::spawn(run(request, receive, send, local));
    wait(&mut updates, |status| status.phase == Phase::Waiting).await;
    assert!(
        updates.borrow().output_name.is_empty(),
        "no output before voice"
    );
    commands
        .send(Command::Voice(0, true, false, false, 0.6))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice && status.output_name == "Laptop speakers"
    })
    .await;
    assert!(
        !updates.borrow().ask_listening,
        "joining answered for the first output"
    );
    // Headphones plugged in: the system's default output moves, and so does voice; the rider
    // is asked again how they listen (R194).
    outputs.lock().unwrap().default = "AirPods Pro".to_owned();
    wait(&mut updates, |status| {
        status.output_name == "AirPods Pro" && status.ask_listening
    })
    .await;
    commands.send(Command::EchoPrevention(false)).await.unwrap();
    wait(&mut updates, |status| !status.ask_listening).await;
    // Devices chosen right after being asked need no second question.
    outputs.lock().unwrap().chosen = Some("Desk speakers".to_owned());
    commands
        .send(Command::ChooseDevices(
            0,
            String::new(),
            "desk".into(),
            false,
        ))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        status.voice && status.output_name == "Desk speakers"
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!updates.borrow().ask_listening);
    commands
        .send(Command::Voice(0, false, false, false, 0.6))
        .await
        .unwrap();
    wait(&mut updates, |status| {
        !status.voice && status.output_name.is_empty()
    })
    .await;
    drop(commands);
    task.await.unwrap();
    tokio::fs::remove_dir_all(directory).await.unwrap();
}
