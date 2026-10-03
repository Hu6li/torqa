//! Video courses (R17): a route ridden along a video — a GoPro recording with its own GPS, or
//! an Incyclist route video (control file + GPX + video). The rider's distance decides the
//! moment of the video, through the same matching used for ghosts (R20).

use std::path::{Path, PathBuf};
use std::time::Duration;

use torqa_domain::units::Meters;
use torqa_routes::Route;
use torqa_session::ghost::Ghost;
use torqa_video::{Video, gps_track, gpx_from_track, incyclist};

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
        });
    }
    let track = gps_track(path).map_err(|e| unreadable(&e))?;
    if track.len() < 2 {
        return Err(format!(
            "{} has no GPS track — record with GPS on, or use a route video with a GPX",
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
    })
}

/// A route paired with its video.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoCourse {
    /// The video file.
    pub video: PathBuf,
    /// Where the route's first point sits in the video.
    pub offset: Duration,
    /// The video's length.
    pub duration: Duration,
    /// Video time at each distance along the route.
    sync: Ghost,
}

impl VideoCourse {
    /// Pairs `route` (imported from `source.gpx`) with the video. The GPX timestamps give the
    /// video time at each point; without them, the video is spread evenly over the route.
    ///
    /// # Errors
    /// A readable message if the video cannot be opened.
    pub fn new(route: &Route, source: &VideoSource) -> Result<Self, String> {
        let duration = Video::open(&source.video)
            .map_err(|e| format!("cannot open {}: {e}", source.video.display()))?
            .info()
            .duration;
        // Matching counts from the route start; the offset places that in the video.
        let points = torqa_routes::timed_points(&source.gpx).unwrap_or_default();
        let sync = Ghost::from_activity("video", route, &points)
            .or_else(|| {
                let rest = duration.saturating_sub(source.offset).as_secs_f64();
                Ghost::from_trace("video", [(0.0, 0.0), (route.length().0, rest)].into_iter())
            })
            .ok_or_else(|| "the video and the route do not match".to_owned())?;
        Ok(Self {
            video: source.video.clone(),
            offset: source.offset,
            duration,
            sync,
        })
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
