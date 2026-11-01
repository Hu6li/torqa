//! Speaker checks use output only, even when no microphone or permission is available.
use orqwa_domain::voice::{CapturedVoice, PcmFrame, VoiceAudio, VoiceDeviceError};
use orqwa_voice::audio_test::AudioTest;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct Probe {
    low_tone: AtomicBool,
    high_tone: AtomicBool,
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
        if samples.iter().any(|value| value.abs() > 0.001) {
            // Pitch, gain and output-only operation are the contract. Callback
            // throughput is scheduler-dependent, especially on native CI runners.
            let cycles = samples
                .windows(2)
                .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
                .count();
            if (12..=14).contains(&cycles) {
                self.0.low_tone.store(true, Ordering::Release);
            } else if (17..=19).contains(&cycles) {
                self.0.high_tone.store(true, Ordering::Release);
            }
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
    assert!(
        probe.low_tone.load(Ordering::Acquire),
        "the first audible tone must reach output"
    );
    assert!(
        probe.high_tone.load(Ordering::Acquire),
        "the second audible tone must reach output"
    );
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
