use std::path::PathBuf;

use ffmpeg::codec;
use ffmpeg::format::Pixel;

use super::*;

const FPS: i32 = 10;
const FRAMES: i32 = 40;

/// A 4 s, 10 fps MPEG-4 video whose frame `i` is a flat grey of brightness `40 + 4·i`, so each
/// decoded frame tells which one it is.
fn test_video(name: &str, width: u32, height: u32) -> PathBuf {
    init();
    let path = std::env::temp_dir().join(format!("torqa-video-{name}-{}.mp4", std::process::id()));
    let codec = ffmpeg::encoder::find(codec::Id::MPEG4).expect("MPEG-4 encoder built in");
    let mut output = ffmpeg::format::output(&path).unwrap();
    let global_header = output
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
    let mut stream = output.add_stream(codec).unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap();
    encoder.set_width(width);
    encoder.set_height(height);
    encoder.set_format(Pixel::YUV420P);
    encoder.set_time_base((1, FPS));
    encoder.set_frame_rate(Some((FPS, 1)));
    encoder.set_gop(10);
    encoder.set_bit_rate(2_000_000);
    if global_header {
        encoder.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let mut encoder = encoder.open_as(codec).unwrap();
    stream.set_parameters(&encoder);
    stream.set_time_base((1, FPS));
    output.write_header().unwrap();
    let stream_time_base = output.stream(0).unwrap().time_base();

    let write_packets = |encoder: &mut ffmpeg::encoder::Video,
                         output: &mut ffmpeg::format::context::Output| {
        let mut packet = ffmpeg::Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(0);
            // Without durations the MP4 edit list ends at the last frame's start and cuts it.
            packet.set_duration(1);
            packet.rescale_ts((1, FPS), stream_time_base);
            packet.write_interleaved(output).unwrap();
        }
    };
    for i in 0..FRAMES {
        let mut frame = ffmpeg::frame::Video::new(Pixel::YUV420P, width, height);
        let luma = u8::try_from(40 + 4 * i).unwrap();
        frame.data_mut(0).fill(luma);
        frame.data_mut(1).fill(128);
        frame.data_mut(2).fill(128);
        frame.set_pts(Some(i64::from(i)));
        encoder.send_frame(&frame).unwrap();
        write_packets(&mut encoder, &mut output);
    }
    encoder.send_eof().unwrap();
    write_packets(&mut encoder, &mut output);
    output.write_trailer().unwrap();
    path
}

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

/// Parameters of a GoPro metadata track. `ffmpeg-next` has no setter for the codec tag the
/// MP4 muxer needs, so this test fixture sets the field directly.
#[allow(unsafe_code)] // test fixture only: writes one plain integer field of owned parameters
fn gpmd_parameters() -> ffmpeg::codec::Parameters {
    let mut parameters = ffmpeg::codec::Parameters::new();
    parameters.set_medium(ffmpeg::media::Type::Data);
    // SAFETY: `parameters` is owned and valid; `codec_tag` is a plain integer field.
    unsafe {
        (*parameters.as_mut_ptr()).codec_tag = u32::from_le_bytes(*b"gpmd");
    }
    parameters
}

/// A 3 s video with a GoPro-style GPMF data track: one payload per second, 4 positions each,
/// heading north 10 m per position.
fn gopro_video(name: &str) -> PathBuf {
    init();
    // MOV rather than MP4: FFmpeg's MP4 muxer only accepts registered tags, while both are read
    // by the same demuxer GoPro files go through.
    let path = std::env::temp_dir().join(format!("torqa-gopro-{name}-{}.mov", std::process::id()));
    let video_codec = ffmpeg::encoder::find(codec::Id::MPEG4).unwrap();
    let mut output = ffmpeg::format::output_as(&path, "mov").unwrap();
    let mut video = output.add_stream(video_codec).unwrap();
    let mut encoder = codec::context::Context::new_with_codec(video_codec)
        .encoder()
        .video()
        .unwrap();
    encoder.set_width(64);
    encoder.set_height(48);
    encoder.set_format(Pixel::YUV420P);
    encoder.set_time_base((1, FPS));
    encoder.set_flags(codec::Flags::GLOBAL_HEADER);
    let mut encoder = encoder.open_as(video_codec).unwrap();
    video.set_parameters(&encoder);
    video.set_time_base((1, FPS));
    let mut data = output
        .add_stream(ffmpeg::encoder::find(codec::Id::None))
        .unwrap();
    data.set_time_base((1, 1000));
    data.set_parameters(gpmd_parameters());
    output.write_header().unwrap();
    let video_tb = output.stream(0).unwrap().time_base();
    let data_tb = output.stream(1).unwrap().time_base();

    let write_video = |encoder: &mut ffmpeg::encoder::Video,
                       output: &mut ffmpeg::format::context::Output| {
        let mut packet = ffmpeg::Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(0);
            packet.set_duration(1);
            packet.rescale_ts((1, FPS), video_tb);
            packet.write_interleaved(output).unwrap();
        }
    };
    for second in 0..3u16 {
        for i in 0..FPS {
            let mut frame = ffmpeg::frame::Video::new(Pixel::YUV420P, 64, 48);
            frame.data_mut(0).fill(100);
            frame.data_mut(1).fill(128);
            frame.data_mut(2).fill(128);
            frame.set_pts(Some(i64::from(second) * i64::from(FPS) + i64::from(i)));
            encoder.send_frame(&frame).unwrap();
            write_video(&mut encoder, &mut output);
        }
        let points: Vec<(f64, f64, f64, f64)> = (0..4)
            .map(|k| {
                let metres = f64::from(second * 4 + k) * 10.0;
                (46.0 + metres / 111_195.0, 7.0, 500.0, 10.0)
            })
            .collect();
        let payload = crate::gpmf::tests::gps5_payload(&points, 3);
        let mut packet = ffmpeg::Packet::copy(&payload);
        packet.set_stream(1);
        packet.set_pts(Some(i64::from(second) * 1000));
        packet.set_dts(Some(i64::from(second) * 1000));
        packet.set_duration(1000);
        packet.rescale_ts((1, 1000), data_tb);
        packet.write_interleaved(&mut output).unwrap();
    }
    encoder.send_eof().unwrap();
    write_video(&mut encoder, &mut output);
    output.write_trailer().unwrap();
    path
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
