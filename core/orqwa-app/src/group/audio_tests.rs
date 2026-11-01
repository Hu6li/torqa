use super::*;
use orqwa_domain::units::Kilograms;
use orqwa_domain::voice::{CapturedVoice, PcmFrame, VoiceAudio, VoiceDeviceError};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Capture {
    live: Arc<AtomicUsize>,
    activation: Option<u32>,
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
        self.activation = activation;
        Ok(())
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        let activation = self.activation?;
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
#[allow(clippy::too_many_lines)] // one lifecycle checks exclusive capture and retained controls across start/stop/leave
async fn microphone_testing_restores_listening_muted_and_group_leave_releases_capture() {
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
    let request = Request {
        audio: AudioBackend {
            factory: Some(Arc::new(move |_| {
                Ok(Box::new(Capture {
                    live: live.clone(),
                    activation: None,
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
        },
        options,
        data: directory.join("data"),
        temporary: directory.join("temporary"),
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
    wait(&mut updates, |status| {
        status.voice && !status.microphone_test
    })
    .await;
    let status = updates.borrow().clone();
    assert!(!status.microphone && !status.echo_prevention);
    assert!(status.microphone_voice_heard && status.microphone_checked);
    assert!((status.voice_volume - 0.4).abs() < f32::EPSILON);
    assert_eq!(status.input_device, "test-input");
    assert_eq!(status.output_device, "test-output");
    assert_eq!(capture.load(Ordering::Acquire), 0);
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
