use super::*;
use crate::testing::{FRAMES, gopro_video, test_video};

fn brightness(frame: &Frame) -> f64 {
    let sum: u64 = frame.rgba.chunks(4).map(|p| u64::from(p[0])).sum();
    #[allow(clippy::cast_precision_loss)]
    let mean = sum as f64 / (frame.rgba.len() / 4) as f64;
    mean
}

#[test]
fn knows_length_rate_and_size() {
    let path = test_video("info", 320, 180);

    let info = Video::open(&path).unwrap().info();

    assert!((info.duration.as_secs_f64() - 4.0).abs() < 0.15, "{info:?}");
    assert!((info.frame_rate - 10.0).abs() < 1e-6, "{info:?}");
    assert_eq!((info.width, info.height), (320, 180));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn hands_out_the_frame_for_any_moment_forwards_and_backwards() {
    let path = test_video("frames", 160, 96);
    let mut video = Video::open(&path).unwrap();
    // Each frame's brightness, decoded in order.
    let in_order: Vec<f64> = (0..FRAMES)
        .map(|i| {
            let t = Duration::from_millis(u64::try_from(i).unwrap() * 100 + 50);
            brightness(video.frame_at(t).unwrap())
        })
        .collect();
    assert!(in_order.windows(2).all(|w| w[1] > w[0]), "{in_order:?}");

    // Jumping back and far ahead lands on the same frames as decoding in order.
    for (seconds, index) in [(3.25, 32), (0.0, 0), (1.04, 10), (3.99, 39), (2.5, 25)] {
        let frame = video.frame_at(Duration::from_secs_f64(seconds)).unwrap();
        assert!(
            (brightness(frame) - in_order[index]).abs() < 1.0,
            "at {seconds} s: {} vs frame {index} {}",
            brightness(frame),
            in_order[index]
        );
        assert!(
            (frame.time.as_secs_f64() - f64::from(u8::try_from(index).unwrap()) / 10.0).abs()
                < 1e-6
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn large_videos_are_scaled_to_1080p() {
    assert_eq!(display_size(3840, 2160), (1920, 1080));
    assert_eq!(display_size(2704, 1520), (1920, 1078));
    assert_eq!(display_size(1280, 720), (1280, 720));
}

#[test]
fn files_without_video_are_refused() {
    let path = std::env::temp_dir().join(format!("torqa-video-none-{}.txt", std::process::id()));
    std::fs::write(&path, b"not a video").unwrap();

    assert!(Video::open(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reads_the_gps_track_with_video_times() {
    let path = gopro_video("gps");

    let track = gps_track(&path).unwrap();

    assert_eq!(track.len(), 12, "{track:?}");
    // The second payload's third position: 1.5 s into the video, 60 m north.
    let sample = track[6];
    assert!((sample.time.as_secs_f64() - 1.5).abs() < 1e-3, "{sample:?}");
    assert!((sample.point.lat - (46.0 + 60.0 / 111_195.0)).abs() < 1e-7);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn tells_videos_with_gps_from_plain_ones() {
    let gopro = gopro_video("probe");
    let plain = test_video("probe", 64, 48);

    assert!(has_gps(&gopro).unwrap());
    assert!(!has_gps(&plain).unwrap());
    std::fs::remove_file(gopro).unwrap();
    std::fs::remove_file(plain).unwrap();
}

#[test]
fn videos_without_gps_have_no_track() {
    let path = test_video("nogps", 64, 48);

    assert_eq!(gps_track(&path).unwrap(), []);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_track_becomes_a_gpx_with_video_times() {
    let point = |lat: f64| gpmf::GpsPoint {
        lat,
        lon: 7.4,
        altitude: 540.25,
        speed: 8.0,
    };
    let track = [
        TimedGps {
            time: Duration::ZERO,
            point: point(46.9),
        },
        TimedGps {
            time: Duration::from_millis(3_725_500),
            point: point(46.9001),
        },
    ];

    let gpx = gpx_from_track("Gurten <climb>", &track);

    assert!(gpx.contains("<name>Gurten &lt;climb&gt;</name>"), "{gpx}");
    assert!(gpx.contains(r#"<trkpt lat="46.9001000" lon="7.4000000"><ele>540.25</ele>"#));
    assert!(
        gpx.contains("<time>1970-01-01T01:02:05.500Z</time>"),
        "{gpx}"
    );
}

/// Loudness (peak-like: √2 × RMS) and zero crossings per second of one channel.
fn tone(samples: &[audio::Stereo], channel: usize, rate: f64) -> (f64, f64) {
    let values: Vec<f64> = samples.iter().map(|s| f64::from(s[channel])).collect();
    #[allow(clippy::cast_precision_loss)]
    let count = values.len() as f64;
    let rms = (values.iter().map(|v| v * v).sum::<f64>() / count).sqrt();
    #[allow(clippy::cast_precision_loss)]
    let crossings = values
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count() as f64;
    (
        rms * std::f64::consts::SQRT_2,
        crossings / 2.0 / (count / rate),
    )
}

#[test]
fn hands_out_the_sound_of_any_moment_with_its_pitch() {
    use crate::testing::{SOUND_RATE, TONES, sound_video};

    let path = sound_video("audio");
    let mut sound = audio::Audio::open(&path).unwrap().unwrap();
    let rate = f64::from(sound.rate());
    assert_eq!(sound.rate(), u32::try_from(SOUND_RATE).unwrap());
    let mut read_at = |seconds: f64| {
        let mut out = vec![[0.0; 2]; 4_800];
        #[allow(clippy::cast_possible_truncation)]
        sound.read((seconds * rate) as i64, &mut out).unwrap();
        out
    };

    // In order, back, and far ahead: loudness grows 0.2 per second, pitch stays.
    for seconds in [1.0, 1.1, 0.5, 3.5, 2.0] {
        let samples = read_at(seconds);
        for (channel, wanted) in TONES.iter().enumerate() {
            let (loudness, hertz) = tone(&samples, channel, rate);
            let expected = 0.8 * (seconds + 0.05) / 4.0;
            assert!(
                (loudness - expected).abs() < 0.02,
                "{seconds} s: {loudness} vs {expected}"
            );
            assert!(
                (hertz - wanted).abs() < 15.0,
                "{seconds} s: {hertz} Hz vs {wanted}"
            );
        }
    }
    // Past the end: silence.
    assert!(
        read_at(5.0)
            .iter()
            .flatten()
            .all(|v| v.abs() < f32::EPSILON)
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn videos_without_sound_have_no_audio() {
    let path = test_video("silent", 64, 48);

    assert!(audio::Audio::open(&path).unwrap().is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn slower_and_faster_sound_keeps_its_pitch() {
    use crate::testing::{TONES, sound_video};

    let path = sound_video("stretch");
    let mut sound = audio::Audio::open(&path).unwrap().unwrap();
    let rate = f64::from(sound.rate());
    for speed in [0.5, 1.0, 1.7] {
        let mut stretcher = audio::Stretcher::new(sound.rate());
        let hop = stretcher.hop();
        let mut out = Vec::new();
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let mut position = rate * 1.0;
        // One second of output, from 1 s into the track.
        while out.len() < 48_000 {
            #[allow(clippy::cast_possible_truncation)]
            out.extend(stretcher.step(&mut sound, position as i64).unwrap());
            #[allow(clippy::cast_precision_loss)]
            let advance = hop as f64 * speed;
            position += advance;
        }
        // Past the first grain, which fades in.
        let steady = &out[4_800..];
        for (channel, wanted) in TONES.iter().enumerate() {
            let (_, hertz) = tone(steady, channel, rate);
            assert!(
                (hertz - wanted).abs() < 20.0,
                "×{speed}: {hertz} Hz vs {wanted}"
            );
        }
        // The track moved on by the speed: louder the faster (loudness grows with time).
        let (end_loudness, _) = tone(&out[out.len() - 4_800..], 0, rate);
        let expected = 0.8 * (1.0 + speed * 0.95) / 4.0;
        assert!(
            (end_loudness - expected).abs() < 0.03,
            "×{speed}: {end_loudness} vs {expected}"
        );
    }
    std::fs::remove_file(path).unwrap();
}
