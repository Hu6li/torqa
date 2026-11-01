//! Speaker checks use output only, even when no microphone or permission is available.
use orqwa_domain::voice::{CapturedVoice, PcmFrame, VoiceAudio, VoiceDeviceError};
use orqwa_voice::audio_test::AudioTest;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct Probe {
    frames: AtomicUsize,
    heard: AtomicUsize,
    dropped: AtomicBool,
}
struct Output(Arc<Probe>);
impl VoiceAudio for Output {
    fn microphone(&mut self, _activation: Option<u32>) -> Result<(), VoiceDeviceError> {
        panic!("speaker test must not touch microphone capture")
    }
    fn capture(&mut self) -> Option<CapturedVoice> {
        panic!("speaker test must not read microphone samples")
    }
    fn play(&mut self, samples: PcmFrame) {
        assert!(
            samples
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 0.0361)
        );
        self.0.frames.fetch_add(1, Ordering::Relaxed);
        if samples.iter().any(|value| value.abs() > 0.001) {
            self.0.heard.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn deafen(&mut self, _deafened: bool) {}
    fn healthy(&self) -> bool {
        true
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn speakers_play_at_selected_gain_and_finish_without_a_microphone() {
    let probe = Arc::new(Probe::default());
    let output = probe.clone();
    let test = AudioTest::with_speaker_audio(0.3, move |_| Ok(Box::new(Output(output))))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while test.running() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(probe.frames.load(Ordering::Relaxed) > 40);
    assert!(probe.heard.load(Ordering::Relaxed) > 20);
    assert!(probe.dropped.load(Ordering::Acquire));
    assert!(!test.voice_detected() && !test.voice_heard());
}

#[tokio::test]
async fn stop_releases_output_and_invalid_gain_never_opens_it() {
    let probe = Arc::new(Probe::default());
    let output = probe.clone();
    let test = AudioTest::with_speaker_audio(0.3, move |_| Ok(Box::new(Output(output))))
        .await
        .unwrap();
    test.close().await;
    assert!(probe.dropped.load(Ordering::Acquire));
    let opened = Arc::new(AtomicBool::new(false));
    let output = opened.clone();
    let error = AudioTest::with_speaker_audio(f32::NAN, move |_| {
        output.store(true, Ordering::Release);
        Ok(Box::new(Output(Arc::default())))
    })
    .await
    .err()
    .unwrap();
    assert!(error.to_string().contains("volume"));
    assert!(!opened.load(Ordering::Acquire));
}
