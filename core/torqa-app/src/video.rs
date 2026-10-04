//! Video courses (R17): a route ridden along a video — a GoPro recording with its own GPS, an
//! Incyclist route video (control file + GPX + video), or any video placed on a GPX by hand
//! (where the route starts and ends in it). The rider's distance decides the moment of the
//! video, through the same matching used for ghosts (R20).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use torqa_domain::units::Meters;
use torqa_routes::Route;
use torqa_session::ghost::Ghost;
use torqa_video::{Frame, Video, gps_track, gpx_from_track, incyclist};

/// File extensions of the videos Torqa reads directly (GoPro and similar).
pub const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "mkv"];

/// What a video course is prepared from.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoSource {
    /// The video file.
    pub video: PathBuf,
    /// Display name.
    pub name: String,
    /// The route as GPX; its timestamps (from the first point) are the video's timeline.
    pub gpx: String,
    /// Where the route's first point sits in the video.
    pub offset: Duration,
    /// Where the route's last point sits in the video, for videos aligned by hand: the video
    /// is spread evenly between `offset` and this. `None` when timestamps pair them.
    pub end: Option<Duration>,
}

/// A video without GPS placed on the GPX route at `gpx` by hand: the route starts at `start`
/// and ends at `end` in the video. The GPX's own timestamps, from another recording, are not
/// used.
///
/// # Errors
/// A readable message if the GPX cannot be read.
pub fn aligned_source(
    video: &Path,
    gpx: &Path,
    start: Duration,
    end: Duration,
) -> Result<VideoSource, String> {
    let xml =
        std::fs::read_to_string(gpx).map_err(|e| format!("cannot read {}: {e}", gpx.display()))?;
    let name = video
        .file_stem()
        .map_or_else(|| "Video".to_owned(), |s| s.to_string_lossy().into_owned());
    Ok(VideoSource {
        video: video.to_owned(),
        name,
        gpx: xml,
        offset: start,
        end: Some(end),
    })
}

/// What the import needs to know about a video first: its length, and whether it carries GPS
/// (then it pairs itself with its route) or must be placed on a GPX by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// The video's length.
    pub duration: Duration,
    /// Whether it records GPS.
    pub has_gps: bool,
}

/// Looks at a video before importing it.
///
/// # Errors
/// A readable message if it cannot be opened as a video.
pub fn probe(path: &Path) -> Result<Probe, String> {
    let unreadable = |e: &dyn std::fmt::Display| format!("cannot read {}: {e}", path.display());
    let duration = Video::open(path)
        .map_err(|e| unreadable(&e))?
        .info()
        .duration;
    let has_gps = torqa_video::has_gps(path).map_err(|e| unreadable(&e))?;
    Ok(Probe { duration, has_gps })
}

/// Reads what a video course needs from an Incyclist control file (`.xml`) or a video with
/// GPS (GoPro).
///
/// # Errors
/// A readable message if the files cannot be read, or the video has no GPS.
pub fn source(path: &Path) -> Result<VideoSource, String> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let unreadable = |e: &dyn std::fmt::Display| format!("cannot read {}: {e}", path.display());
    if extension == "xml" {
        let xml = std::fs::read_to_string(path).map_err(|e| unreadable(&e))?;
        let route = incyclist::parse(&xml).map_err(|e| unreadable(&e))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let gpx_path = dir.join(&route.gpx_file);
        let gpx = std::fs::read_to_string(&gpx_path)
            .map_err(|e| format!("cannot read {}: {e}", gpx_path.display()))?;
        return Ok(VideoSource {
            video: dir.join(&route.video_file),
            name: route.title.clone(),
            gpx,
            offset: route.video_offset(),
            end: None,
        });
    }
    let track = gps_track(path).map_err(|e| unreadable(&e))?;
    if track.len() < 2 {
        return Err(format!(
            "{} has no GPS track — place it on a GPX route instead",
            path.display()
        ));
    }
    let name = path
        .file_stem()
        .map_or_else(|| "Video".to_owned(), |s| s.to_string_lossy().into_owned());
    Ok(VideoSource {
        video: path.to_owned(),
        gpx: gpx_from_track(&name, &track),
        name,
        offset: Duration::ZERO,
        end: None,
    })
}

/// A route paired with its video.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoCourse {
    /// The video file.
    pub video: PathBuf,
    /// Where the route's first point sits in the video.
    pub offset: Duration,
    /// Where the route's last point sits in the video, if aligned by hand.
    pub end: Option<Duration>,
    /// The video's length.
    pub duration: Duration,
    /// Video time at each distance along the route.
    sync: Ghost,
}

/// Start and end marks must lie in the video, the end after the start.
///
/// # Errors
/// A readable message otherwise.
pub fn check_marks(start: Duration, end: Duration, duration: Duration) -> Result<(), String> {
    // Container durations are rounded; a mark on the last frame may lie a little past them.
    let slack = Duration::from_millis(500);
    if end <= start || end > duration + slack {
        return Err("the route must end after it starts, within the video".to_owned());
    }
    Ok(())
}

impl VideoCourse {
    /// Pairs `route` (imported from `source.gpx`) with the video: by the GPX timestamps, or
    /// evenly between the start and end marks of a video aligned by hand. Without either, the
    /// video is spread evenly from its offset to its end.
    ///
    /// # Errors
    /// A readable message if the video cannot be opened or the marks do not fit it.
    pub fn new(route: &Route, source: &VideoSource) -> Result<Self, String> {
        let duration = Video::open(&source.video)
            .map_err(|e| format!("cannot open {}: {e}", source.video.display()))?
            .info()
            .duration;
        let evenly = |end: Duration| {
            let span = end.saturating_sub(source.offset).as_secs_f64();
            Ghost::from_trace("video", [(0.0, 0.0), (route.length().0, span)].into_iter())
        };
        let sync = match source.end {
            Some(end) => {
                check_marks(source.offset, end, duration)?;
                evenly(end)
            }
            // Matching counts from the route start; the offset places that in the video.
            None => torqa_routes::timed_points(&source.gpx)
                .ok()
                .and_then(|points| Ghost::from_activity("video", route, &points))
                .or_else(|| evenly(duration)),
        }
        .ok_or_else(|| "the video and the route do not match".to_owned())?;
        Ok(Self {
            video: source.video.clone(),
            offset: source.offset,
            end: source.end,
            duration,
            sync,
        })
    }

    /// Whether the video was placed on the route by hand (start and end marks), so the marks
    /// can be moved.
    #[must_use]
    pub fn aligned_by_hand(&self) -> bool {
        self.end.is_some()
    }

    /// The moment of the video at `distance` along the route (the end beyond it).
    #[must_use]
    pub fn time_at(&self, distance: Meters) -> Duration {
        let along = self
            .sync
            .time_at(distance)
            .unwrap_or_else(|| self.sync.total_time());
        (self.offset + along).min(self.duration)
    }
}

/// Decodes a video course's frames on its own thread, so riding never waits for the decoder:
/// [`VideoPlayer::show`] asks for a moment, [`VideoPlayer::frame`] hands out the newest frame
/// once decoded.
pub struct VideoPlayer {
    shared: Arc<(Mutex<PlayerState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct PlayerState {
    wanted: Option<Duration>,
    ready: Option<Frame>,
    error: Option<String>,
    stop: bool,
}

impl VideoPlayer {
    /// Opens `video` for playback.
    ///
    /// # Errors
    /// A readable message if the video cannot be opened.
    pub fn open(video: &Path) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(PlayerState::default()), Condvar::new()));
        let state = Arc::clone(&shared);
        let path = video.to_owned();
        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        // The decoder stays on its thread: FFmpeg's scaler may not move between threads.
        let thread = std::thread::Builder::new()
            .name("video".to_owned())
            .spawn(move || {
                let mut video = match Video::open(&path) {
                    Ok(video) => {
                        let _ = opened_tx.send(Ok(()));
                        video
                    }
                    Err(error) => {
                        let _ =
                            opened_tx.send(Err(format!("cannot open {}: {error}", path.display())));
                        return;
                    }
                };
                // The frame after the moment shown, so the view can blend towards it (R17).
                let step = Duration::from_secs_f64(1.0 / video.info().frame_rate.max(1.0));
                let (lock, wake) = &*state;
                let mut shown: Option<Duration> = None;
                loop {
                    let wanted = {
                        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
                        while s.wanted.is_none() && !s.stop {
                            s = wake.wait(s).unwrap_or_else(PoisonError::into_inner);
                        }
                        if s.stop {
                            return;
                        }
                        s.wanted.take()
                    };
                    let Some(wanted) = wanted else { continue };
                    let decoded = video.frame_at(wanted + step);
                    let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
                    match decoded {
                        Ok(frame) if shown != Some(frame.time) => {
                            shown = Some(frame.time);
                            s.ready = Some(frame.clone());
                        }
                        Ok(_) => {}
                        Err(error) => s.error = Some(error.to_string()),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        opened_rx
            .recv()
            .map_err(|_| "the video player stopped".to_owned())??;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Asks for the frames around `time`; a newer request replaces one not yet started.
    pub fn show(&self, time: Duration) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(PoisonError::into_inner).wanted = Some(time);
        wake.notify_one();
    }

    /// The newest decoded frame not handed out yet: the one following the moment asked for.
    ///
    /// # Errors
    /// The decoder's message if the video could not be decoded there.
    pub fn frame(&self) -> Result<Option<Frame>, String> {
        let (lock, _) = &*self.shared;
        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
        match s.error.take() {
            Some(error) => Err(error),
            None => Ok(s.ready.take()),
        }
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for VideoPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPlayer").finish_non_exhaustive()
    }
}
