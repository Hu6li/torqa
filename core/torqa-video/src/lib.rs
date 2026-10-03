//! Video decoding for the video ride mode (R17), built on FFmpeg (ADR 0010).
//!
//! A ride does not play a video at its own pace: the rider's position decides which moment of
//! the video is shown, so the speed changes all the time and the rider may even stop. [`Video`]
//! therefore hands out the frame for any moment, decoding forward when that is cheap and
//! seeking otherwise, scaled down to at most [`MAX_WIDTH`] for display.

use std::path::Path;
use std::sync::Once;
use std::time::Duration;

use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg_next as ffmpeg;

/// Frames are scaled down to at most this width (1080p); larger videos gain nothing on screen.
pub const MAX_WIDTH: u32 = 1920;
/// Further ahead than this, seeking to the next keyframe beats decoding every frame up to it.
const SEEK_AHEAD: Duration = Duration::from_secs(3);

/// Opening or decoding a video failed.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// FFmpeg could not read the file or decode it.
    #[error("cannot read video: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    /// The file has no video stream.
    #[error("the file contains no video")]
    NoVideo,
    /// No frame could be decoded at all.
    #[error("the video has no frames")]
    Empty,
}

/// What a video is like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    /// Length.
    pub duration: Duration,
    /// Frames per second.
    pub frame_rate: f64,
    /// Width of the frames handed out (after scaling), in pixels.
    pub width: u32,
    /// Height of the frames handed out, in pixels.
    pub height: u32,
}

/// One decoded picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// When it is shown in the video.
    pub time: Duration,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels row by row, four bytes (red, green, blue, alpha) each.
    pub rgba: Vec<u8>,
}

/// An open video, handing out frames for any moment.
pub struct Video {
    input: ffmpeg::format::context::Input,
    stream: usize,
    decoder: ffmpeg::decoder::Video,
    scaler: scaling::Context,
    /// Seconds per unit of the stream's timestamps.
    time_base: f64,
    info: VideoInfo,
    /// The frame decoded last; the next one requested is usually just after it.
    current: Option<Frame>,
    /// A frame decoded beyond the one requested, kept for the next request.
    ahead: Option<Frame>,
    ended: bool,
}

impl Video {
    /// Opens a video file.
    ///
    /// # Errors
    /// [`VideoError`] if the file cannot be read or holds no video.
    pub fn open(path: &Path) -> Result<Self, VideoError> {
        init();
        let input = ffmpeg::format::input(path)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(VideoError::NoVideo)?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        // The stream's own rate; the average is frames over duration and comes out uneven.
        let rate = f64::from(stream.rate());
        let frame_rate = if rate > 0.0 {
            rate
        } else {
            f64::from(stream.avg_frame_rate()).max(1.0)
        };
        let duration = if stream.duration() > 0 {
            Duration::from_secs_f64(f64_from(stream.duration()) * time_base)
        } else {
            Duration::from_secs_f64(
                f64_from(input.duration().max(0)) / f64::from(ffmpeg::ffi::AV_TIME_BASE),
            )
        };
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;
        let (width, height) = display_size(decoder.width(), decoder.height());
        let scaler = scaling::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGBA,
            width,
            height,
            scaling::Flags::BILINEAR,
        )?;
        Ok(Self {
            input,
            stream: index,
            decoder,
            scaler,
            time_base,
            info: VideoInfo {
                duration,
                frame_rate,
                width,
                height,
            },
            current: None,
            ahead: None,
            ended: false,
        })
    }

    /// What the video is like.
    #[must_use]
    pub fn info(&self) -> VideoInfo {
        self.info
    }

    /// The frame shown at `time` (clamped to the video): the last frame starting at or before it.
    ///
    /// # Errors
    /// [`VideoError`] if decoding fails.
    pub fn frame_at(&mut self, time: Duration) -> Result<&Frame, VideoError> {
        let time = time.min(self.info.duration);
        let behind = self.current.as_ref().is_some_and(|f| time < f.time);
        let far_ahead = self
            .current
            .as_ref()
            .is_some_and(|f| time > f.time + SEEK_AHEAD);
        if behind || far_ahead || self.current.is_none() {
            self.seek(time)?;
        }
        // Decode forward until the next frame would be past `time`.
        loop {
            if self.ahead.is_none() {
                self.ahead = self.decode_next()?;
            }
            match &self.ahead {
                Some(next) if next.time <= time || self.current.is_none() => {
                    self.current = self.ahead.take();
                }
                _ => break,
            }
        }
        self.current.as_ref().ok_or(VideoError::Empty)
    }

    /// Jumps to the keyframe at or before `time`.
    fn seek(&mut self, time: Duration) -> Result<(), VideoError> {
        #[allow(clippy::cast_possible_truncation)] // microseconds of a video fit i64
        let target = time.as_micros() as i64;
        // The keyframe at or before `target`: nothing later than it.
        self.input.seek(target, ..target + 1)?;
        self.decoder.flush();
        self.current = None;
        self.ahead = None;
        self.ended = false;
        Ok(())
    }

    /// The next frame of the stream, or `None` at its end.
    fn decode_next(&mut self) -> Result<Option<Frame>, VideoError> {
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => return Ok(Some(self.convert(&decoded)?)),
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other {
                    errno: ffmpeg::error::EAGAIN,
                }) => {}
                Err(error) => return Err(error.into()),
            }
            if self.ended {
                return Ok(None);
            }
            // The decoder wants more data: feed it the next packet of our stream.
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
                self.ended = true;
            }
        }
    }

    fn convert(&mut self, decoded: &ffmpeg::frame::Video) -> Result<Frame, VideoError> {
        let mut rgba = ffmpeg::frame::Video::empty();
        self.scaler.run(decoded, &mut rgba)?;
        let (width, height) = (self.info.width, self.info.height);
        let row = width as usize * 4;
        let stride = rgba.stride(0);
        let data = rgba.data(0);
        // Rows in FFmpeg's buffer may be padded; hand out tightly packed rows.
        let mut pixels = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            pixels.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        let timestamp = decoded.timestamp().or(decoded.pts()).unwrap_or(0).max(0);
        Ok(Frame {
            time: Duration::from_secs_f64(f64_from(timestamp) * self.time_base),
            width,
            height,
            rgba: pixels,
        })
    }
}

/// Sets up FFmpeg once per process.
fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = ffmpeg::init();
        ffmpeg::log::set_level(ffmpeg::log::Level::Error);
    });
}

/// The size frames are handed out in: at most [`MAX_WIDTH`] wide, same aspect, even sides.
fn display_size(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_WIDTH {
        return (width, height);
    }
    let scaled = u64::from(height) * u64::from(MAX_WIDTH) / u64::from(width);
    let even = u32::try_from(scaled).unwrap_or(u32::MAX) & !1;
    (MAX_WIDTH, even.max(2))
}

#[allow(clippy::cast_precision_loss)] // timestamps far below 2^52
fn f64_from(value: i64) -> f64 {
    value as f64
}

#[cfg(test)]
mod tests;
