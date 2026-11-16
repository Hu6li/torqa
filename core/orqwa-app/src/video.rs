//! Video courses (R17): a route ridden along a video — a GoPro recording with its own GPS, an
//! Incyclist route video (control file + GPX + video), or any video added to a GPX course by
//! hand (where the route starts and ends in it). The rider's distance decides the moment of the
//! video, through the same matching used for ghosts (R20).

use crate::failure::{Failure, Kind};
use crate::import::text;
use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use orqwa_domain::units::Meters;
use orqwa_routes::Route;
use orqwa_session::ghost::Ghost;
use orqwa_video::audio::{Audio, Stereo, Stretcher};
use orqwa_video::{Frame, Video, gps_track, gpx_from_track, incyclist, tacx};

/// File extensions of the videos ORQWA reads directly (GoPro and similar, and AVI as Tacx
/// courses come with).
pub const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "mov", "m4v", "mkv", "avi"];

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
    /// For videos aligned by hand: where route positions sit in the video, from the start to
    /// the end of the route; the video follows the distance evenly between neighbours. Empty
    /// when timestamps pair them.
    pub marks: Vec<SyncMark>,
    /// Whether the GPX is a real place; `false` for a Tacx RLV course drawn from its slopes.
    pub located: bool,
}

/// A position on the route and the moment of the video showing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncMark {
    /// Distance along the route.
    pub distance: Meters,
    /// Moment in the video.
    pub time: Duration,
}

/// A Tacx Real Life Video (#42) from its `.rlv` file: the video and the `.pgmf` course next to
/// it (found by name, ignoring case). The course has no place, only distance and slope, so its
/// GPX is drawn from the slopes and the video is paired by the RLV's speed changes.
///
/// # Errors
/// A readable message if a file is missing or cannot be read.
fn tacx_source(path: &Path) -> Result<VideoSource, Failure> {
    let unreadable = |file: &Path, e: &dyn std::fmt::Display| {
        Failure::new(
            Kind::ReadFile,
            format!("cannot read {}: {e}", file.display()),
        )
        .arg(file.display())
    };
    let bytes = std::fs::read(path).map_err(|e| unreadable(path, &e))?;
    let rlv = tacx::parse_rlv(&bytes).map_err(|e| unreadable(path, &e))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let video = find_file(dir, |name| name.eq_ignore_ascii_case(rlv.video_file_name()))
        .or_else(|| {
            // Courses copied about often keep the video next to the RLV under its own name.
            find_file(dir, |name| {
                let file = Path::new(name);
                file.file_stem()
                    .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(&stem))
                    && !file.extension().is_some_and(|e| {
                        ["rlv", "pgmf"].contains(&e.to_string_lossy().to_lowercase().as_str())
                    })
            })
        })
        .ok_or_else(|| {
            Failure::new(
                Kind::MissingVideo,
                format!(
                    "video {} not found — put it next to {}",
                    rlv.video_file_name(),
                    path.display()
                ),
            )
            .arg(rlv.video_file_name())
        })?;
    let pgmf_name = format!("{stem}.pgmf");
    let pgmf_path =
        find_file(dir, |name| name.eq_ignore_ascii_case(&pgmf_name)).ok_or_else(|| {
            Failure::new(
                Kind::MissingSlopes,
                format!(
                    "{pgmf_name} not found — it holds the course's slopes, put it next to the RLV"
                ),
            )
            .arg(&pgmf_name)
        })?;
    let pgmf_bytes = std::fs::read(&pgmf_path).map_err(|e| unreadable(&pgmf_path, &e))?;
    let pgmf = tacx::parse_pgmf(&pgmf_bytes).map_err(|e| unreadable(&pgmf_path, &e))?;
    let duration = Video::open(&video)
        .map_err(|e| unreadable(&video, &e))?
        .info()
        .duration;
    // The RLV's end may lie a few frames past the video's; the video ends where it ends.
    let mut marks: Vec<SyncMark> = Vec::new();
    for (distance, time) in rlv.sync_points() {
        let time = time.min(duration);
        if marks.last().is_none_or(|m| time > m.time) {
            marks.push(SyncMark {
                distance: Meters(distance),
                time,
            });
        }
    }
    let name = tacx_name(&pgmf, &stem);
    Ok(VideoSource {
        video,
        gpx: tacx::gpx_from_profile(&name, &pgmf),
        name,
        offset: Duration::ZERO,
        marks,
        located: false,
    })
}

/// The name of a Tacx course: its PGMF's, unless missing or cut short by the field's size,
/// then the RLV's file name (`stem`).
fn tacx_name(pgmf: &tacx::Pgmf, stem: &str) -> String {
    let name = pgmf.name.trim();
    if name.is_empty() || name.chars().count() >= PGMF_NAME_CHARS {
        stem.to_owned()
    } else {
        name.to_owned()
    }
}

/// The name a Tacx course from the `.rlv` at `path` gets, if its PGMF can be read.
#[must_use]
pub fn tacx_course_name(path: &Path) -> Option<String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let pgmf_name = format!("{stem}.pgmf");
    let pgmf = find_file(dir, |name| name.eq_ignore_ascii_case(&pgmf_name))?;
    let pgmf = tacx::parse_pgmf(&std::fs::read(pgmf).ok()?).ok()?;
    Some(tacx_name(&pgmf, &stem))
}

/// Characters a PGMF course name can hold (34 bytes of UTF-16).
const PGMF_NAME_CHARS: usize = 17;

/// `reference`, a path a control file gives relative to its folder `dir`, if it stays inside:
/// the file is the rider's, so one naming an absolute path or `..` may not read others.
fn within(dir: &Path, reference: &str) -> Option<PathBuf> {
    let relative = Path::new(reference);
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        .then(|| dir.join(relative))
}

/// The file in `dir` whose name `matches`, if any.
fn find_file(dir: &Path, matches: impl Fn(&str) -> bool) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| matches(&name.to_string_lossy()))
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
pub fn probe(path: &Path) -> Result<Probe, Failure> {
    let unreadable = |e: &dyn std::fmt::Display| {
        Failure::new(
            Kind::ReadVideo,
            format!("cannot read {}: {e}", path.display()),
        )
    };
    let duration = Video::open(path)
        .map_err(|e| unreadable(&e))?
        .info()
        .duration;
    let has_gps = orqwa_video::has_gps(path).map_err(|e| unreadable(&e))?;
    Ok(Probe { duration, has_gps })
}

/// Reads what a video course needs from an Incyclist control file (`.xml`) or a video with
/// GPS (GoPro).
///
/// # Errors
/// A readable message if the files cannot be read, or the video has no GPS.
pub fn source(path: &Path) -> Result<VideoSource, Failure> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let unreadable = |e: &dyn std::fmt::Display| {
        Failure::new(
            Kind::ReadVideo,
            format!("cannot read {}: {e}", path.display()),
        )
    };
    if extension == "rlv" {
        return tacx_source(path);
    }
    if extension == "xml" {
        let xml = std::fs::read(path).map(text).map_err(|e| unreadable(&e))?;
        let route = incyclist::parse(&xml).map_err(|e| unreadable(&e))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let inside = |reference: &str| {
            within(dir, reference).ok_or_else(|| {
                Failure::new(
                    Kind::ReadFile,
                    format!("{} points outside its folder: {reference}", path.display()),
                )
                .arg(path.display())
            })
        };
        let gpx_path = inside(&route.gpx_file)?;
        let video = inside(&route.video_file)?;
        let gpx = std::fs::read(&gpx_path)
            .map(text)
            .map_err(|e| Failure::new(Kind::ReadFile, e).arg(gpx_path.display()))?;
        return Ok(VideoSource {
            video,
            name: route.title.clone(),
            gpx,
            offset: route.video_offset(),
            marks: Vec::new(),
            located: true,
        });
    }
    let track = gps_track(path).map_err(|e| unreadable(&e))?;
    if track.len() < 2 {
        return Err(Failure::new(
            Kind::VideoGps,
            format!(
                "{} has no GPS track — import its GPX route, then add the video on the course page",
                path.display()
            ),
        )
        .arg(path.display()));
    }
    let name = path
        .file_stem()
        .map_or_else(|| "Video".to_owned(), |s| s.to_string_lossy().into_owned());
    Ok(VideoSource {
        video: path.to_owned(),
        gpx: gpx_from_track(&name, &track),
        name,
        offset: Duration::ZERO,
        marks: Vec::new(),
        located: true,
    })
}

/// A route paired with its video.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoCourse {
    /// The video file.
    pub video: PathBuf,
    /// Where the route's first point sits in the video.
    pub offset: Duration,
    /// The marks of a video aligned by hand, from the route's start to its end; empty
    /// otherwise.
    pub marks: Vec<SyncMark>,
    /// Whether the route is a real place (see [`VideoSource::located`]).
    pub located: bool,
    /// The video's length.
    pub duration: Duration,
    /// Video time at each distance along the route.
    sync: Ghost,
}

/// Puts the first mark at the route's start and the last at its end, then checks that the
/// marks follow each other along the route and in the video, within its length.
///
/// # Errors
/// A readable message if they do not.
pub fn fit_marks(
    marks: &[SyncMark],
    length: Meters,
    duration: Duration,
) -> Result<Vec<SyncMark>, Failure> {
    // Container durations are rounded; a mark on the last frame may lie a little past them.
    let slack = Duration::from_millis(500);
    if marks.len() < 2 {
        return Err(Failure::new(
            Kind::VideoEndpoints,
            "a video needs a start and an end on the route",
        ));
    }
    let mut fitted = marks.to_vec();
    fitted[0].distance = Meters(0.0);
    let last = fitted.len() - 1;
    fitted[last].distance = length;
    let ordered = fitted
        .windows(2)
        .all(|w| w[1].distance.0 > w[0].distance.0 && w[1].time > w[0].time);
    if !ordered {
        return Err(Failure::new(
            Kind::VideoOrder,
            "each point must come after the one before it, on the route and in the video",
        ));
    }
    if fitted[last].time > duration + slack {
        return Err(Failure::new(
            Kind::VideoEnd,
            "the route must end within the video",
        ));
    }
    Ok(fitted)
}

impl VideoCourse {
    /// Pairs `route` (imported from `source.gpx`) with the video: by the GPX timestamps, or
    /// by the marks of a video aligned by hand, evenly in between. Without either, the video
    /// is spread evenly from its offset to its end.
    ///
    /// # Errors
    /// A readable message if the video cannot be opened or the marks do not fit it.
    pub fn new(route: &Route, source: &VideoSource) -> Result<Self, Failure> {
        let duration = Video::open(&source.video)
            .map_err(|e| Failure::new(Kind::OpenFile, e).arg(source.video.display()))?
            .info()
            .duration;
        let marks = if source.marks.is_empty() {
            Vec::new()
        } else {
            fit_marks(&source.marks, route.length(), duration)?
        };
        let offset = marks.first().map_or(source.offset, |m| m.time);
        let sync = if marks.is_empty() {
            // Matching counts from the route start; the offset places that in the video.
            orqwa_routes::timed_points(&source.gpx)
                .ok()
                .and_then(|points| Ghost::from_activity("video", route, &points))
                .or_else(|| {
                    let span = duration.saturating_sub(offset).as_secs_f64();
                    Ghost::from_trace("video", [(0.0, 0.0), (route.length().0, span)].into_iter())
                })
        } else {
            Ghost::from_trace(
                "video",
                marks
                    .iter()
                    .map(|m| (m.distance.0, m.time.saturating_sub(offset).as_secs_f64())),
            )
        }
        .ok_or_else(|| Failure::new(Kind::VideoMismatch, "the video and the route do not match"))?;
        Ok(Self {
            video: source.video.clone(),
            offset,
            marks,
            located: source.located,
            duration,
            sync,
        })
    }

    /// Whether the video was placed on the route by hand (marks), so the marks can be moved.
    #[must_use]
    pub fn aligned_by_hand(&self) -> bool {
        !self.marks.is_empty()
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

/// How far ahead of the moment shown frames are decoded, in seconds of the ride. Decoding comes
/// in bursts — now and then one frame takes as long as several — and a frame decoded only once
/// the ride has reached it holds the picture still, then jumps.
const FRAMES_AHEAD_S: f64 = 0.15;
/// Frames kept decoded ahead at most (a 1080p frame takes 8 MB); when more would come within
/// [`FRAMES_AHEAD_S`], frames further apart are decoded.
const MAX_FRAMES_AHEAD: u32 = 8;
/// The fastest the video is expected to play, in video seconds per second.
const MAX_PLAY_SPEED: f64 = 8.0;

/// Decodes a video course's frames on its own thread, ahead of the ride, so riding never waits
/// for the decoder: [`VideoPlayer::show`] tells the moment of the video shown,
/// [`VideoPlayer::frame`] hands out the next frame to blend towards (R17).
pub struct VideoPlayer {
    shared: Arc<(Mutex<PlayerState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct PlayerState {
    /// The moment of the video shown last, and when.
    shown: Option<(Duration, Instant)>,
    /// Video seconds played per second, estimated from the moments shown.
    speed: f64,
    /// Frames decoded ahead, in order.
    ahead: VecDeque<Frame>,
    /// The frame handed out last, which the view blends towards.
    handed: Option<Duration>,
    /// The moment shown went back: the decoder starts over from there.
    restart: bool,
    error: Option<String>,
    stop: bool,
}

/// The moment of the video to decode the frame of next, while `shown` plays at `speed` with
/// the frame at `last` decoded last and `queued` frames waiting; `None` while enough lies
/// decoded ahead. Frames are found half a frame `step` past where they are expected, so that
/// rounding in their times cannot give the same frame twice.
fn next_moment(
    shown: Duration,
    speed: f64,
    last: Option<Duration>,
    queued: usize,
    step: Duration,
) -> Option<Duration> {
    if queued >= MAX_FRAMES_AHEAD as usize {
        return None;
    }
    let reach = Duration::from_secs_f64(speed.clamp(0.0, MAX_PLAY_SPEED) * FRAMES_AHEAD_S);
    let spacing = step.max(reach / MAX_FRAMES_AHEAD);
    // The frame after the one the view blends towards is wanted too: it is needed the moment
    // the ride reaches that one.
    let horizon = shown + step * 2 + reach;
    let next = match last {
        Some(last) if last >= shown => last + spacing + step / 2,
        // The frame just after the moment shown.
        _ => shown + step,
    };
    (next <= horizon).then_some(next)
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
                let step = Duration::from_secs_f64(1.0 / video.info().frame_rate.max(1.0));
                let (lock, wake) = &*state;
                // The frame decoded last, or the moment looked at last where none came.
                let mut last: Option<Duration> = None;
                loop {
                    let moment = {
                        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
                        loop {
                            if s.stop {
                                return;
                            }
                            if std::mem::take(&mut s.restart) {
                                last = None;
                            }
                            let next = s.shown.and_then(|(shown, _)| {
                                next_moment(shown, s.speed, last, s.ahead.len(), step)
                            });
                            if let Some(next) = next {
                                break next;
                            }
                            s = wake.wait(s).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    let decoded = video.frame_at(moment).cloned();
                    let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
                    if s.restart {
                        continue;
                    }
                    match decoded {
                        Ok(frame) if last.is_none_or(|last| frame.time > last) => {
                            last = Some(frame.time);
                            s.ahead.push_back(frame);
                        }
                        // Nothing new there (the end of the video): look further on next time.
                        Ok(_) => last = Some(moment),
                        Err(error) => {
                            s.error = Some(error.to_string());
                            last = Some(moment);
                        }
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

    /// The moment of the video shown now; called every frame.
    pub fn show(&self, time: Duration) {
        let now = Instant::now();
        let (lock, wake) = &*self.shared;
        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((before, at)) = s.shown {
            if time < before {
                // Moved back (a simulated ride jumps): the frames decoded lie ahead of it.
                s.ahead.clear();
                s.handed = None;
                s.restart = true;
            }
            let elapsed = now.duration_since(at).as_secs_f64();
            if elapsed > 0.0 {
                let speed = (time.as_secs_f64() - before.as_secs_f64()) / elapsed;
                // Frames come unevenly; smooth over a few of them.
                s.speed += (speed.clamp(0.0, MAX_PLAY_SPEED) - s.speed) * 0.2;
            }
        }
        s.shown = Some((time, now));
        wake.notify_one();
    }

    /// The frame to blend towards once the moment `now` has reached the one handed out
    /// before: the first decoded frame after `now`, or while decoding lags behind, the newest
    /// one before it. `None` while the frame handed out last still lies ahead, or none is new.
    #[must_use]
    pub fn frame(&self, now: Duration) -> Option<Frame> {
        let (lock, wake) = &*self.shared;
        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if s.handed.is_some_and(|handed| handed > now) {
            return None;
        }
        let mut passed = None;
        while s.ahead.front().is_some_and(|f| f.time <= now) {
            passed = s.ahead.pop_front();
        }
        let frame = s.ahead.pop_front().or(passed)?;
        // Room in the queue again.
        wake.notify_one();
        if s.handed.is_some_and(|handed| frame.time <= handed) {
            return None;
        }
        s.handed = Some(frame.time);
        Some(frame)
    }

    /// Why the video could not be decoded, once, if it could not.
    #[must_use]
    pub fn error(&self) -> Option<String> {
        let (lock, _) = &*self.shared;
        lock.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .error
            .take()
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

/// Sound queued ahead of playback, in seconds: enough to ride out a slow frame, little enough
/// to follow the rider's speed closely.
const SOUND_AHEAD_S: f64 = 0.12;
/// Below this speed of the video the sound fades out (standing still is silent); it is fully
/// there from `SOUND_FULL_SPEED` on.
const SOUND_SILENT_SPEED: f64 = 0.2;
const SOUND_FULL_SPEED: f64 = 0.5;
/// How far the sound may drift from the picture before it jumps rather than catching up.
const SOUND_MAX_DRIFT_S: f64 = 0.5;

/// The sound of a video course during a ride (R26), on its own thread: it follows the moment
/// of the video ([`SoundPlayer::follow`]) at the speed the rider makes it play, stretched to
/// keep its pitch, and is pulled by the front end ([`SoundPlayer::pull`]).
pub struct SoundPlayer {
    shared: Arc<(Mutex<SoundState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
}

struct SoundState {
    /// The moment of the video shown last, and when.
    target: Option<(Duration, Instant)>,
    /// Video seconds played per second, estimated from the targets.
    speed: f64,
    on: bool,
    queue: VecDeque<Stereo>,
    stop: bool,
}

impl SoundPlayer {
    /// Opens the sound of `video`; `None` if it has none.
    ///
    /// # Errors
    /// A readable message if the video cannot be read.
    pub fn open(video: &Path) -> Result<Option<Self>, String> {
        let shared = Arc::new((
            Mutex::new(SoundState {
                target: None,
                speed: 0.0,
                on: true,
                queue: VecDeque::new(),
                stop: false,
            }),
            Condvar::new(),
        ));
        let state = Arc::clone(&shared);
        let path = video.to_owned();
        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        // Like the picture, the decoder stays on its thread.
        let thread = std::thread::Builder::new()
            .name("video sound".to_owned())
            .spawn(move || {
                let audio = match Audio::open(&path) {
                    Ok(Some(audio)) => {
                        let _ = opened_tx.send(Ok(Some(audio.rate())));
                        audio
                    }
                    Ok(None) => {
                        let _ = opened_tx.send(Ok(None));
                        return;
                    }
                    Err(error) => {
                        let _ =
                            opened_tx.send(Err(format!("cannot open {}: {error}", path.display())));
                        return;
                    }
                };
                play(audio, &state);
            })
            .map_err(|e| e.to_string())?;
        let rate = opened_rx
            .recv()
            .map_err(|_| "the sound player stopped".to_owned())??;
        Ok(rate.map(|rate| Self {
            shared,
            thread: Some(thread),
            rate,
        }))
    }

    /// Samples per second of [`SoundPlayer::pull`].
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The moment of the video shown now; called every frame.
    pub fn follow(&self, time: Duration) {
        let now = Instant::now();
        let (lock, _) = &*self.shared;
        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((before, at)) = s.target {
            let elapsed = now.duration_since(at).as_secs_f64();
            if elapsed > 0.0 {
                let speed = (time.as_secs_f64() - before.as_secs_f64()) / elapsed;
                // Frames come unevenly; smooth over a few of them.
                s.speed += (speed.clamp(0.0, 8.0) - s.speed) * 0.2;
            }
        }
        s.target = Some((time, now));
    }

    /// Switches the sound on or off (it fades).
    pub fn set_on(&self, on: bool) {
        let (lock, _) = &*self.shared;
        lock.lock().unwrap_or_else(PoisonError::into_inner).on = on;
    }

    /// Up to `max` stereo samples to play next.
    #[must_use]
    pub fn pull(&self, max: usize) -> Vec<Stereo> {
        let (lock, wake) = &*self.shared;
        let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let count = max.min(s.queue.len());
        let samples = s.queue.drain(..count).collect();
        wake.notify_one();
        samples
    }
}

/// The sound thread: keeps the queue filled with the stretched sound around the moment shown.
fn play(mut audio: Audio, state: &Arc<(Mutex<SoundState>, Condvar)>) {
    let rate = f64::from(audio.rate());
    let mut stretcher = Stretcher::new(audio.rate());
    let hop = stretcher.hop();
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let ahead = (SOUND_AHEAD_S * rate) as usize;
    let mut position: Option<f64> = None;
    let mut gain = 0.0_f32;
    let (lock, wake) = &**state;
    loop {
        let (target, speed, on, queued) = {
            let mut s = lock.lock().unwrap_or_else(PoisonError::into_inner);
            while !s.stop && (s.target.is_none() || s.queue.len() >= ahead) {
                s = wake
                    .wait_timeout(s, Duration::from_millis(10))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            if s.stop {
                return;
            }
            let Some(target) = s.target else { continue };
            (target, s.speed, s.on, s.queue.len())
        };
        // Where the picture will be when this hop is heard, after what is queued already.
        #[allow(clippy::cast_precision_loss)]
        let lead = target.1.elapsed().as_secs_f64() + queued as f64 / rate;
        let expected = (target.0.as_secs_f64() + lead * speed) * rate;
        #[allow(clippy::cast_precision_loss)]
        let step = hop as f64 * speed;
        let at = match position {
            Some(at) if (expected - at).abs() < SOUND_MAX_DRIFT_S * rate => {
                // Catch up gently rather than jump, which would be heard.
                at + step + (expected - at) * 0.05
            }
            _ => {
                stretcher.reset();
                expected
            }
        };
        position = Some(at);
        #[allow(clippy::cast_possible_truncation)]
        let wanted = if on {
            ((speed - SOUND_SILENT_SPEED) / (SOUND_FULL_SPEED - SOUND_SILENT_SPEED)).clamp(0.0, 1.0)
                as f32
        } else {
            0.0
        };
        let samples = if gain <= 0.0 && wanted <= 0.0 {
            vec![[0.0; 2]; hop]
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let mut samples = stretcher
                .step(&mut audio, at as i64)
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "cannot decode the video's sound");
                    vec![[0.0; 2]; hop]
                });
            // Fade over a few hops so that switching or stopping does not click.
            #[allow(clippy::cast_precision_loss)]
            let change = (wanted - gain).clamp(-0.1, 0.1) / hop as f32;
            for sample in &mut samples {
                gain = (gain + change).clamp(0.0, 1.0);
                sample[0] *= gain;
                sample[1] *= gain;
            }
            samples
        };
        lock.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queue
            .extend(samples);
    }
}

impl Drop for SoundPlayer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for SoundPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoundPlayer")
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_video::testing::test_video;

    #[test]
    fn an_incyclist_control_file_reads_nothing_outside_its_folder() {
        let dir = std::env::temp_dir().join(format!("orqwa-escape-{}", std::process::id()));
        let videos = dir.join("videos");
        std::fs::create_dir_all(&videos).unwrap();
        std::fs::write(dir.join("secret.gpx"), "<gpx><trk><trkseg/></trk></gpx>").unwrap();
        let control = videos.join("climb.xml");
        let secret = dir.join("secret.gpx").display().to_string();
        for (gpx, video) in [
            ("../secret.gpx", "climb.mp4"),
            (secret.as_str(), "climb.mp4"),
            ("climb.gpx", "../climb.mp4"),
        ] {
            std::fs::write(
                &control,
                format!(
                    "<gpx-import><video-file-path>{video}</video-file-path>\
                     <gpx-file-path>{gpx}</gpx-file-path><framerate>30</framerate></gpx-import>"
                ),
            )
            .unwrap();

            assert!(source(&control).is_err(), "{gpx} {video}");
        }

        std::fs::create_dir_all(videos.join("tracks")).unwrap();
        std::fs::write(videos.join("tracks/climb.gpx"), "<gpx/>").unwrap();
        std::fs::write(
            &control,
            "<gpx-import><video-file-path>./climb.mp4</video-file-path>\
             <gpx-file-path>tracks/climb.gpx</gpx-file-path><framerate>30</framerate></gpx-import>",
        )
        .unwrap();
        assert_eq!(source(&control).unwrap().video, videos.join("climb.mp4"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_incyclist_route_video_saved_as_windows_1252_is_read() {
        // Route video libraries ship control files and GPX in Windows-1252 (R17).
        let dir = std::env::temp_dir().join(format!("orqwa-latin1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let control = dir.join("Pr\u{e9}mol.xml");
        std::fs::write(
            &control,
            b"<?xml version=\"1.0\" encoding=\"windows-1252\"?>\n<gpx-import>\
              <title>Col de Pr\xe9mol</title><video-file-path>premol.mp4</video-file-path>\
              <gpx-file-path>premol.gpx</gpx-file-path><framerate>30</framerate></gpx-import>",
        )
        .unwrap();
        std::fs::write(
            dir.join("premol.gpx"),
            b"<gpx><trk><name>Pr\xe9mol</name><trkseg/></trk></gpx>",
        )
        .unwrap();

        let source = source(&control).unwrap();

        assert_eq!(source.video, dir.join("premol.mp4"));
        assert!(source.name.starts_with("Col de Pr"), "{}", source.name);
        assert!(source.gpx.contains("<trkseg/>"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// How often the view refreshes.
    const REFRESH: Duration = Duration::from_millis(16);

    /// Plays the video from `from` at `speed` (video seconds per second) up to `until`, as the
    /// view does each refresh: tells the moment shown, then takes the frame to blend towards.
    /// Returns the refreshes that had a frame ahead of the moment shown to blend towards, and
    /// those that had not.
    fn ride_along(player: &VideoPlayer, from: Duration, speed: f64, until: Duration) -> (u32, u32) {
        // Standing at the start first, as a rider does before pedalling off.
        player.show(from);
        std::thread::sleep(Duration::from_millis(200));
        let mut shown = from;
        let mut blending_towards = player.frame(shown).map(|f| f.time);
        let (mut ahead, mut behind) = (0, 0);
        while shown < until {
            std::thread::sleep(REFRESH);
            shown += REFRESH.mul_f64(speed);
            player.show(shown);
            if let Some(frame) = player.frame(shown) {
                blending_towards = Some(frame.time);
            }
            if blending_towards.is_some_and(|t| t > shown) {
                ahead += 1;
            } else {
                behind += 1;
            }
        }
        (ahead, behind)
    }

    #[test]
    fn frames_are_decoded_before_the_ride_reaches_them() {
        // The view blends from one frame to the next by where the ride is between them: a
        // frame that comes only once the ride has reached the one before holds the picture
        // still, then jumps (R17). Slow, at the video's own pace and fast.
        let video = test_video("ahead", 64, 48);
        for (speed, until) in [(0.3, 0.8), (1.0, 2.5), (3.0, 3.6)] {
            let player = VideoPlayer::open(&video).unwrap();
            let (ahead, behind) = ride_along(
                &player,
                Duration::ZERO,
                speed,
                Duration::from_secs_f64(until),
            );
            assert!(
                behind * 50 <= ahead,
                "at {speed}×, {behind} of {} refreshes had no frame ahead",
                ahead + behind
            );
        }
    }

    #[test]
    fn after_a_jump_back_the_frames_come_from_there() {
        let player = VideoPlayer::open(&test_video("jump", 64, 48)).unwrap();
        ride_along(&player, Duration::ZERO, 1.0, Duration::from_secs(2));

        let (ahead, behind) = ride_along(
            &player,
            Duration::from_millis(500),
            1.0,
            Duration::from_secs(1),
        );

        assert!(
            behind * 50 <= ahead,
            "{behind} of {} behind",
            ahead + behind
        );
        let shown = Duration::from_millis(1_050);
        player.show(shown);
        std::thread::sleep(Duration::from_millis(100));
        let next = player.frame(shown).map_or(Duration::ZERO, |f| f.time);
        assert!(next <= Duration::from_millis(1_200), "{next:?}");
    }
}
