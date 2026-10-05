//! Test videos for Torqa's own tests (feature `testing`): small MPEG-4 videos generated on the
//! fly, optionally with a GoPro-style GPS metadata track, so no sample footage is needed.

// Fixtures: a failure to build a test video is a broken test environment, so they panic.
#![allow(clippy::missing_panics_doc)]

use std::path::PathBuf;

use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg_next as ffmpeg;

use crate::init;

/// Frames per second of the test videos.
pub const FPS: i32 = 10;
/// Frames of [`test_video`].
pub const FRAMES: i32 = 40;

/// A 4 s, 10 fps MPEG-4 video whose frame `i` is a flat grey of brightness `40 + 4·i`, so each
/// decoded frame tells which one it is.
#[must_use]
pub fn test_video(name: &str, width: u32, height: u32) -> PathBuf {
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

/// Sample rate of [`sound_video`].
pub const SOUND_RATE: i32 = 48_000;
/// The tones of [`sound_video`], left and right, in hertz.
pub const TONES: [f64; 2] = [440.0, 660.0];
/// Samples per frame written by [`sound_video`].
const SOUND_CHUNK: usize = 1024;

/// A 4 s, 10 fps video like [`test_video`] (64 × 48, MOV) with sound: a steady tone on each
/// channel ([`TONES`]) whose loudness rises from silence to full over the 4 s, so read samples
/// tell their moment (loudness) and pitch.
#[must_use]
pub fn sound_video(name: &str) -> PathBuf {
    use ffmpeg::ChannelLayout;
    use ffmpeg::format::Sample;
    use ffmpeg::format::sample::Type;

    init();
    let path = std::env::temp_dir().join(format!("torqa-sound-{name}-{}.mov", std::process::id()));
    let video_codec = ffmpeg::encoder::find(codec::Id::MPEG4).unwrap();
    let audio_codec = ffmpeg::encoder::find(codec::Id::PCM_S16LE).unwrap();
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
    let mut audio = output.add_stream(audio_codec).unwrap();
    let mut sound = codec::context::Context::new_with_codec(audio_codec)
        .encoder()
        .audio()
        .unwrap();
    sound.set_rate(SOUND_RATE);
    sound.set_format(Sample::I16(Type::Packed));
    sound.set_channel_layout(ChannelLayout::STEREO);
    sound.set_time_base((1, SOUND_RATE));
    let mut sound = sound.open_as(audio_codec).unwrap();
    audio.set_parameters(&sound);
    audio.set_time_base((1, SOUND_RATE));
    output.write_header().unwrap();
    let video_tb = output.stream(0).unwrap().time_base();
    let audio_tb = output.stream(1).unwrap().time_base();

    let mut packet = ffmpeg::Packet::empty();
    for i in 0..FRAMES {
        let mut frame = ffmpeg::frame::Video::new(Pixel::YUV420P, 64, 48);
        frame.data_mut(0).fill(u8::try_from(40 + 4 * i).unwrap());
        frame.data_mut(1).fill(128);
        frame.data_mut(2).fill(128);
        frame.set_pts(Some(i64::from(i)));
        encoder.send_frame(&frame).unwrap();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(0);
            packet.set_duration(1);
            packet.rescale_ts((1, FPS), video_tb);
            packet.write_interleaved(&mut output).unwrap();
        }
    }
    encoder.send_eof().unwrap();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        packet.set_duration(1);
        packet.rescale_ts((1, FPS), video_tb);
        packet.write_interleaved(&mut output).unwrap();
    }

    let total = usize::try_from(SOUND_RATE).unwrap() * usize::try_from(FRAMES / FPS).unwrap();
    let rate = f64::from(SOUND_RATE);
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    for first in (0..total).step_by(SOUND_CHUNK) {
        let mut frame = ffmpeg::frame::Audio::new(
            Sample::I16(Type::Packed),
            SOUND_CHUNK,
            ChannelLayout::STEREO,
        );
        frame.set_rate(u32::try_from(SOUND_RATE).unwrap());
        let data = frame.data_mut(0);
        for i in 0..SOUND_CHUNK {
            let t = (first + i) as f64 / rate;
            let loudness = (first + i) as f64 / total as f64;
            for (channel, tone) in TONES.iter().enumerate() {
                let value =
                    (loudness * 0.8 * (std::f64::consts::TAU * tone * t).sin() * 32_767.0) as i16;
                let at = (i * 2 + channel) * 2;
                data[at..at + 2].copy_from_slice(&value.to_ne_bytes());
            }
        }
        frame.set_pts(Some(i64::try_from(first).unwrap()));
        sound.send_frame(&frame).unwrap();
        while sound.receive_packet(&mut packet).is_ok() {
            packet.set_stream(1);
            packet.rescale_ts((1, SOUND_RATE), audio_tb);
            packet.write_interleaved(&mut output).unwrap();
        }
    }
    sound.send_eof().unwrap();
    while sound.receive_packet(&mut packet).is_ok() {
        packet.set_stream(1);
        packet.rescale_ts((1, SOUND_RATE), audio_tb);
        packet.write_interleaved(&mut output).unwrap();
    }
    output.write_trailer().unwrap();
    path
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
#[must_use]
pub fn gopro_video(name: &str) -> PathBuf {
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
        let payload = gps5_payload(&points, 3);
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

/// One GPMF entry, padded to four bytes.
#[must_use]
pub fn entry(key: [u8; 4], kind: u8, sample_size: u8, repeat: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = key.to_vec();
    bytes.push(kind);
    bytes.push(sample_size);
    bytes.extend(repeat.to_be_bytes());
    bytes.extend(data);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}

fn be(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A HERO-style payload: a device with an accelerometer stream and a `GPS5` stream.
#[must_use]
pub fn gps5_payload(points: &[(f64, f64, f64, f64)], fix: u32) -> Vec<u8> {
    let scales = be(&[10_000_000, 10_000_000, 1000, 1000, 100]);
    let mut samples = Vec::new();
    #[allow(clippy::cast_possible_truncation)]
    for &(lat, lon, alt, speed) in points {
        samples.extend(be(&[
            (lat * 1e7).round() as i32,
            (lon * 1e7).round() as i32,
            (alt * 1000.0).round() as i32,
            (speed * 1000.0).round() as i32,
            (speed * 100.0).round() as i32,
        ]));
    }
    let mut gps = entry(*b"STNM", b'c', 1, 9, b"GPS (Lat.");
    gps.extend(entry(*b"GPSF", b'L', 4, 1, &fix.to_be_bytes()));
    gps.extend(entry(*b"SCAL", b'l', 4, 5, &scales));
    gps.extend(entry(
        *b"GPS5",
        b'l',
        20,
        u16::try_from(points.len()).unwrap(),
        &samples,
    ));
    let accel = entry(*b"ACCL", b's', 6, 2, &[0; 12]);
    let mut device = entry(*b"DVID", b'L', 4, 1, &1u32.to_be_bytes());
    device.extend(entry(
        *b"STRM",
        0,
        1,
        u16::try_from(accel.len()).unwrap(),
        &accel,
    ));
    device.extend(entry(
        *b"STRM",
        0,
        1,
        u16::try_from(gps.len()).unwrap(),
        &gps,
    ));
    entry(
        *b"DEVC",
        0,
        1,
        u16::try_from(device.len()).unwrap(),
        &device,
    )
}

/// Writes Tacx files for tests: blocks of records after a file header.
struct Writer(Vec<u8>);

impl Writer {
    fn new(fingerprint: u16, blocks: u32) -> Self {
        let mut w = Self(Vec::new());
        w.u16(fingerprint).u16(100).u32(blocks);
        w
    }

    fn block(&mut self, kind: u16, records: u32, size: u32) -> &mut Self {
        self.u16(kind).u16(100).u32(records).u32(size)
    }

    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }

    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }

    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }

    fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }

    /// A text field of `size` bytes in UTF-16.
    fn text(&mut self, text: &str, size: usize) -> &mut Self {
        let mut field: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        field.resize(size, 0);
        self.0.extend(field);
        self
    }
}

/// A Tacx `.rlv` for `video` at 10 fps: 1 m per frame, from frame 20 on 0.5 m, to the
/// course's end at 30 m (frame 40, the end of [`test_video`]).
#[must_use]
pub fn rlv_bytes(video: &str) -> Vec<u8> {
    let mut w = Writer::new(2000, 4);
    w.block(2010, 1, 534)
        .text(video, 522)
        .f32(10.0)
        .u32(75)
        .u32(0);
    w.block(2020, 2, 8).u32(0).f32(1.0).u32(20).f32(0.5);
    w.block(2030, 1, 8).u32(5).u32(1);
    w.block(2040, 1, 596)
        .f32(0.0)
        .f32(30.0)
        .text("All", 66)
        .text("", 522);
    w.0
}

/// A Tacx `.pgmf` course named `name`: 15 m at 5 %, then 15 m at −2 %, from 500 m.
#[must_use]
pub fn pgmf_bytes(name: &str) -> Vec<u8> {
    let mut w = Writer::new(1000, 2);
    w.block(1010, 1, 70)
        .u32(0)
        .text(name, 34)
        .u32(1)
        .u32(1)
        .f64(30.0)
        .f64(0.0)
        .f32(500.0)
        .u32(0);
    w.block(1020, 2, 12)
        .f32(15.0)
        .f32(5.0)
        .f32(0.0)
        .f32(15.0)
        .f32(-2.0)
        .f32(0.0);
    w.0
}
