//! The sound of a video (R26): stereo samples from any moment, decoded forward when read in
//! order and found by seeking otherwise — like [`crate::Video`] does for frames.

use std::collections::VecDeque;
use std::path::Path;

use ffmpeg::format::Sample;
use ffmpeg::format::sample::Type;
use ffmpeg_next as ffmpeg;

use crate::{VideoError, f64_from, init};

/// A stereo sample: left, right, from −1 to 1.
pub type Stereo = [f32; 2];

/// Reading further ahead than this decodes forward; further still, seeking is faster.
const SEEK_AHEAD_S: f64 = 2.0;
/// Decoded sound kept behind the read position, so small steps back need no seek.
const KEEP_BEHIND_S: f64 = 1.0;

/// The sound track of a video.
pub struct Audio {
    input: ffmpeg::format::context::Input,
    stream: usize,
    decoder: ffmpeg::decoder::Audio,
    /// Seconds per unit of the stream's timestamps.
    time_base: f64,
    rate: u32,
    /// Decoded samples, the first being sample number `start` of the track.
    buffer: VecDeque<Stereo>,
    start: i64,
    ended: bool,
}

impl Audio {
    /// Opens the sound of the video at `path`; `None` if it has none.
    ///
    /// # Errors
    /// [`VideoError`] if the file cannot be read or its sound cannot be decoded.
    pub fn open(path: &Path) -> Result<Option<Self>, VideoError> {
        init();
        let input = ffmpeg::format::input(path)?;
        let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) else {
            return Ok(None);
        };
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().audio()?;
        let rate = decoder.rate();
        Ok(Some(Self {
            input,
            stream: index,
            decoder,
            time_base,
            rate,
            buffer: VecDeque::new(),
            start: 0,
            ended: false,
        }))
    }

    /// Samples per second.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Fills `out` with the samples from sample number `from` on; silence before the track's
    /// start and after its end.
    ///
    /// # Errors
    /// [`VideoError`] if decoding fails.
    pub fn read(&mut self, from: i64, out: &mut [Stereo]) -> Result<(), VideoError> {
        let rate = f64::from(self.rate);
        let end = self.start + len_i64(self.buffer.len());
        #[allow(clippy::cast_possible_truncation)] // seconds of samples fit i64
        let seek_ahead = (SEEK_AHEAD_S * rate) as i64;
        if self.buffer.is_empty() || from < self.start || from > end + seek_ahead {
            self.seek(from)?;
        }
        let wanted = from + len_i64(out.len());
        while self.start + len_i64(self.buffer.len()) < wanted && !self.ended {
            self.decode_next()?;
        }
        for (i, sample) in out.iter_mut().enumerate() {
            let at = from + len_i64(i) - self.start;
            *sample = usize::try_from(at)
                .ok()
                .and_then(|at| self.buffer.get(at).copied())
                .unwrap_or_default();
        }
        #[allow(clippy::cast_possible_truncation)]
        let keep = (KEEP_BEHIND_S * rate) as i64;
        let drop = usize::try_from(from - keep - self.start)
            .unwrap_or(0)
            .min(self.buffer.len());
        self.buffer.drain(..drop);
        self.start += len_i64(drop);
        Ok(())
    }

    /// Jumps to the packet at or before sample number `from`.
    fn seek(&mut self, from: i64) -> Result<(), VideoError> {
        let target = from.max(0) * i64::from(ffmpeg::ffi::AV_TIME_BASE) / i64::from(self.rate);
        self.input.seek(target, ..target + 1)?;
        self.decoder.flush();
        self.buffer.clear();
        self.ended = false;
        // Until the first decoded samples say where they belong.
        self.start = from;
        Ok(())
    }

    /// Decodes the next frame of sound into the buffer; at the end, marks it ended.
    fn decode_next(&mut self) -> Result<(), VideoError> {
        let mut decoded = ffmpeg::frame::Audio::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let samples = stereo(&decoded);
                    if self.buffer.is_empty() {
                        let timestamp = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
                        #[allow(clippy::cast_possible_truncation)]
                        let first = (f64_from(timestamp) * self.time_base * f64::from(self.rate))
                            .round() as i64;
                        // After a seek the first samples may start before or after the one
                        // asked for; silence fills a gap.
                        if first > self.start {
                            let gap = usize::try_from(first - self.start).unwrap_or(0);
                            self.buffer.extend(std::iter::repeat_n([0.0; 2], gap));
                        } else {
                            self.start = first;
                        }
                    }
                    self.buffer.extend(samples);
                    return Ok(());
                }
                Err(ffmpeg::Error::Eof) => {
                    self.ended = true;
                    return Ok(());
                }
                Err(ffmpeg::Error::Other {
                    errno: ffmpeg::error::EAGAIN,
                }) => {}
                Err(error) => return Err(error.into()),
            }
            let mut fed = false;
            for (stream, packet) in self.input.packets() {
                if stream.index() == self.stream {
                    self.decoder.send_packet(&packet)?;
                    fed = true;
                    break;
                }
            }
            if !fed {
                self.decoder.send_eof()?;
            }
        }
    }
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// A decoded frame as stereo: mono is doubled, more channels than two keep the first two.
fn stereo(frame: &ffmpeg::frame::Audio) -> Vec<Stereo> {
    let samples = frame.samples();
    let channels = usize::from(frame.channels()).max(1);
    let (size, read): (usize, fn(&[u8]) -> f32) = match frame.format() {
        Sample::U8(_) => (1, |b| (f32::from(b[0]) - 128.0) / 128.0),
        Sample::I16(_) => (2, |b| {
            f32::from(i16::from_ne_bytes([b[0], b[1]])) / 32_768.0
        }),
        #[allow(clippy::cast_precision_loss)] // 24 bits of a 32-bit sample are plenty
        Sample::I32(_) => (4, |b| {
            i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
        }),
        Sample::F32(_) => (4, |b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
        #[allow(clippy::cast_possible_truncation)]
        Sample::F64(_) => (8, |b| {
            f64::from_ne_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32
        }),
        Sample::I64(_) | Sample::None => return vec![[0.0; 2]; samples],
    };
    let planar = matches!(
        frame.format(),
        Sample::U8(Type::Planar)
            | Sample::I16(Type::Planar)
            | Sample::I32(Type::Planar)
            | Sample::F32(Type::Planar)
            | Sample::F64(Type::Planar)
    );
    let value = |sample: usize, channel: usize| -> f32 {
        let channel = channel.min(channels - 1);
        let (plane, offset) = if planar {
            (channel, sample * size)
        } else {
            (0, (sample * channels + channel) * size)
        };
        frame
            .data(plane)
            .get(offset..offset + size)
            .map_or(0.0, read)
    };
    (0..samples).map(|i| [value(i, 0), value(i, 1)]).collect()
}

/// Grain length in seconds: long enough for low tones, short enough to follow changes.
const GRAIN_S: f64 = 0.04;
/// How far a grain may move to line up with the previous one, in seconds.
const TOLERANCE_S: f64 = 0.01;

/// Plays sound faster or slower without changing its pitch (WSOLA: waveform-similarity
/// overlap-add). Each step reads a grain near the wanted moment, shifted a little to continue
/// the previous grain's waveform, and fades it into the one before.
pub struct Stretcher {
    grain: usize,
    hop: usize,
    tolerance: i64,
    window: Vec<f32>,
    /// The second half of the previous grain, to be faded into the next one.
    tail: Vec<Stereo>,
    /// Where the previous grain started in the track.
    previous: Option<i64>,
}

impl Stretcher {
    /// A stretcher for sound at `rate` samples per second.
    #[must_use]
    pub fn new(rate: u32) -> Self {
        let rate = f64::from(rate);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let hop = ((GRAIN_S * rate / 2.0) as usize).max(16);
        let grain = hop * 2;
        // A periodic Hann window: half-overlapping grains add up to exactly one.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let window = (0..grain)
            .map(|n| (0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / grain as f64).cos()) as f32)
            .collect();
        #[allow(clippy::cast_possible_truncation)]
        let tolerance = (TOLERANCE_S * rate) as i64;
        Self {
            grain,
            hop,
            tolerance,
            window,
            tail: vec![[0.0; 2]; hop],
            previous: None,
        }
    }

    /// Samples produced by each [`Stretcher::step`].
    #[must_use]
    pub fn hop(&self) -> usize {
        self.hop
    }

    /// The next [`Stretcher::hop`] samples of output, for the track around sample number
    /// `wanted`. Moving `wanted` on by `hop × speed` per step plays at that speed.
    ///
    /// # Errors
    /// [`VideoError`] if decoding fails.
    pub fn step(&mut self, audio: &mut Audio, wanted: i64) -> Result<Vec<Stereo>, VideoError> {
        let start = match self.previous {
            Some(previous) => self.aligned(audio, previous + len_i64(self.hop), wanted)?,
            None => wanted,
        };
        let mut grain = vec![[0.0; 2]; self.grain];
        audio.read(start, &mut grain)?;
        let (rise, fall) = self.window.split_at(self.hop);
        let (first, second) = grain.split_at(self.hop);
        let out = first
            .iter()
            .zip(rise)
            .zip(&self.tail)
            .map(|((s, w), t)| [t[0] + s[0] * w, t[1] + s[1] * w])
            .collect();
        for ((tail, s), w) in self.tail.iter_mut().zip(second).zip(fall) {
            *tail = [s[0] * w, s[1] * w];
        }
        self.previous = Some(start);
        Ok(out)
    }

    /// Forgets the waveform played so far, e.g. after a jump: the next grain starts afresh.
    pub fn reset(&mut self) {
        self.previous = None;
        self.tail.fill([0.0; 2]);
    }

    /// The start near `wanted` whose first half best continues the sound that naturally
    /// follows the previous grain (at `natural`).
    fn aligned(&self, audio: &mut Audio, natural: i64, wanted: i64) -> Result<i64, VideoError> {
        let mut reference = vec![[0.0; 2]; self.hop];
        audio.read(natural, &mut reference)?;
        let span = usize::try_from(self.tolerance * 2).unwrap_or(0) + self.hop;
        let mut around = vec![[0.0; 2]; span];
        audio.read(wanted - self.tolerance, &mut around)?;
        let mono = |s: &Stereo| s[0] + s[1];
        let mut best = (f32::MIN, wanted);
        // Every other offset and sample: plenty for matching, a quarter of the work.
        for offset in (0..=usize::try_from(self.tolerance * 2).unwrap_or(0)).step_by(2) {
            let score: f32 = (0..self.hop)
                .step_by(2)
                .map(|i| mono(&reference[i]) * mono(&around[offset + i]))
                .sum();
            if score > best.0 {
                best = (score, wanted - self.tolerance + len_i64(offset));
            }
        }
        Ok(best.1)
    }
}
