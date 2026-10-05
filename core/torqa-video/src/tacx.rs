//! Tacx Real Life Video (#42): a `.rlv` file pairing video frames with the distance covered,
//! a `.pgmf` file with the course's slope profile, and the video itself. Neither file knows
//! where the course is — there is no GPS — only how far and how steep.
//!
//! Both are little-endian files of blocks: a file header (fingerprint, version, block count),
//! then per block a type, version, record count and record size, then the records.

use std::time::Duration;

/// Reading an RLV or PGMF file failed.
#[derive(Debug, thiserror::Error)]
pub enum TacxError {
    /// Not the kind of file expected, or cut short.
    #[error("not a Tacx {0} file, or a damaged one")]
    Invalid(&'static str),
    /// A program of watts or heart rate rather than a course of slopes.
    #[error("this Tacx program sets power or heart rate, not a course of slopes")]
    NotACourse,
}

/// What an `.rlv` file says about its video.
#[derive(Debug, Clone, PartialEq)]
pub struct Rlv {
    /// The video's file name as the file gives it (often a Windows path).
    pub video_file: String,
    /// Frames per second.
    pub frame_rate: f64,
    /// From each frame on, the distance covered per frame in metres: how fast the camera
    /// moved there.
    pub speeds: Vec<(u32, f64)>,
    /// The course's end in metres, if the file gives one.
    pub end_m: Option<f64>,
}

impl Rlv {
    /// Distance along the course and moment of the video at each speed change, from the
    /// start, and at the course's end if the file gives one past them.
    #[must_use]
    pub fn sync_points(&self) -> Vec<(f64, Duration)> {
        let rate = self.frame_rate.max(1.0);
        let time = |frame: f64| Duration::from_secs_f64((frame / rate).max(0.0));
        let mut points = vec![(0.0, Duration::ZERO)];
        let (mut frame, mut distance) = (0_u32, 0.0);
        let mut per_frame = self.speeds.first().map_or(0.0, |s| s.1);
        for &(at, speed) in &self.speeds {
            if at > frame {
                distance += per_frame * f64::from(at - frame);
                frame = at;
                points.push((distance, time(f64::from(frame))));
            }
            per_frame = speed;
        }
        // Past the last change the camera keeps its speed to the end.
        if let Some(end) = self.end_m
            && end > distance
            && per_frame > 0.0
        {
            points.push((end, time(f64::from(frame) + (end - distance) / per_frame)));
        }
        points
    }

    /// The video's file name without any directory, for finding it next to the `.rlv`.
    #[must_use]
    pub fn video_file_name(&self) -> &str {
        self.video_file
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&self.video_file)
    }
}

/// What a `.pgmf` file says about its course.
#[derive(Debug, Clone, PartialEq)]
pub struct Pgmf {
    /// The course's name.
    pub name: String,
    /// Altitude at the start, in metres.
    pub start_altitude: f64,
    /// The course in pieces: length in metres and slope in percent.
    pub segments: Vec<(f64, f64)>,
}

/// Reads an `.rlv` file.
///
/// # Errors
/// [`TacxError::Invalid`] if it is not one.
pub fn parse_rlv(bytes: &[u8]) -> Result<Rlv, TacxError> {
    let invalid = || TacxError::Invalid("RLV");
    let mut reader = Reader { bytes, at: 0 };
    let (fingerprint, _version, blocks) = reader.file_header().ok_or_else(invalid)?;
    if fingerprint != 2000 {
        return Err(invalid());
    }
    let mut rlv = Rlv {
        video_file: String::new(),
        frame_rate: 0.0,
        speeds: Vec::new(),
        end_m: None,
    };
    for _ in 0..blocks {
        let (kind, records, size) = reader.block_header().ok_or_else(invalid)?;
        match kind {
            2010 => {
                rlv.video_file = text(reader.take(522).ok_or_else(invalid)?);
                rlv.frame_rate = f64::from(reader.f32().ok_or_else(invalid)?);
                // Original rider weight and a frame offset, not needed for riding.
                reader.take(8).ok_or_else(invalid)?;
            }
            2020 => {
                for _ in 0..records {
                    let frame = reader.u32().ok_or_else(invalid)?;
                    let per_frame = f64::from(reader.f32().ok_or_else(invalid)?);
                    rlv.speeds.push((frame, per_frame));
                }
            }
            2040 => {
                for _ in 0..records {
                    let _start = reader.f32().ok_or_else(invalid)?;
                    let end = f64::from(reader.f32().ok_or_else(invalid)?);
                    rlv.end_m = Some(rlv.end_m.map_or(end, |e: f64| e.max(end)));
                    // Segment name and text file.
                    reader.take(66 + 522).ok_or_else(invalid)?;
                }
            }
            // Info boxes and anything newer: skipped by their declared size.
            _ => {
                reader
                    .take(records as usize * size as usize)
                    .ok_or_else(invalid)?;
            }
        }
    }
    if rlv.frame_rate <= 0.0 || rlv.speeds.is_empty() {
        return Err(invalid());
    }
    Ok(rlv)
}

/// Reads a `.pgmf` file.
///
/// # Errors
/// [`TacxError::Invalid`] if it is not one, [`TacxError::NotACourse`] for programs of power
/// or heart rate.
pub fn parse_pgmf(bytes: &[u8]) -> Result<Pgmf, TacxError> {
    let invalid = || TacxError::Invalid("PGMF");
    let mut reader = Reader { bytes, at: 0 };
    let (fingerprint, _version, blocks) = reader.file_header().ok_or_else(invalid)?;
    if fingerprint != 1000 {
        return Err(invalid());
    }
    let mut pgmf = Pgmf {
        name: String::new(),
        start_altitude: 0.0,
        segments: Vec::new(),
    };
    for _ in 0..blocks {
        let (kind, records, size) = reader.block_header().ok_or_else(invalid)?;
        match kind {
            1010 => {
                let _checksum = reader.u32().ok_or_else(invalid)?;
                pgmf.name = text(reader.take(34).ok_or_else(invalid)?);
                let watt_slope_pulse = reader.u32().ok_or_else(invalid)?;
                let time_or_distance = reader.u32().ok_or_else(invalid)?;
                // Slope over distance is a course; power or heart rate, or time, is a workout.
                if watt_slope_pulse != 1 || time_or_distance != 1 {
                    return Err(TacxError::NotACourse);
                }
                // Total distance and energy, then the start altitude and the brake category.
                reader.take(16).ok_or_else(invalid)?;
                pgmf.start_altitude = f64::from(reader.f32().ok_or_else(invalid)?);
                reader.take(4).ok_or_else(invalid)?;
            }
            1020 => {
                for _ in 0..records {
                    let length = f64::from(reader.f32().ok_or_else(invalid)?);
                    let slope = f64::from(reader.f32().ok_or_else(invalid)?);
                    let _friction = reader.f32().ok_or_else(invalid)?;
                    pgmf.segments.push((length, slope));
                }
            }
            _ => {
                reader
                    .take(records as usize * size as usize)
                    .ok_or_else(invalid)?;
            }
        }
    }
    if pgmf.segments.is_empty() {
        return Err(invalid());
    }
    Ok(pgmf)
}

/// A GPX named `name` of a course known only by its slopes: a straight line north from the equator at the
/// prime meridian, a point every 10 m, elevations from the start altitude and the slopes. It
/// has the course's length and climbs, not its place.
#[must_use]
pub fn gpx_from_profile(name: &str, pgmf: &Pgmf) -> String {
    use std::fmt::Write as _;

    const STEP_M: f64 = 10.0;
    const METRES_PER_DEGREE: f64 = 111_195.0;
    let mut gpx = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1" creator="Torqa"><trk><name>"#,
    );
    gpx.push_str(&escape(name));
    gpx.push_str("</name><trkseg>");
    let mut point = |distance: f64, elevation: f64| {
        let _ = write!(
            gpx,
            r#"<trkpt lat="{:.7}" lon="0"><ele>{elevation:.2}</ele></trkpt>"#,
            distance / METRES_PER_DEGREE
        );
    };
    let (mut distance, mut elevation) = (0.0, pgmf.start_altitude);
    point(distance, elevation);
    for &(length, slope) in &pgmf.segments {
        let mut left = length.max(0.0);
        while left > 1e-6 {
            let step = left.min(STEP_M);
            distance += step;
            elevation += step * slope / 100.0;
            left -= step;
            point(distance, elevation);
        }
    }
    gpx.push_str("</trkseg></trk></gpx>");
    gpx
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A fixed-size text field: UTF-16 as Tacx writes it, or 8-bit text in older files; up to the
/// first NUL.
fn text(bytes: &[u8]) -> String {
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .collect();
    let utf16 = String::from_utf16_lossy(&wide);
    // 8-bit text read as UTF-16 pairs up letters into unprintable characters.
    if !utf16.is_empty() && utf16.chars().all(|c| !c.is_control() && c != '\u{fffd}') {
        if wide.iter().all(|&c| c < 0x100) {
            return utf16;
        }
        let narrow = bytes.iter().take_while(|&&b| b != 0).count();
        if narrow <= 1 {
            return utf16;
        }
    }
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

/// Reads little-endian values from the start of `bytes` on.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(slice)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Option<f32> {
        self.take(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Fingerprint, version and block count.
    fn file_header(&mut self) -> Option<(u16, u16, u32)> {
        Some((self.u16()?, self.u16()?, self.u32()?))
    }

    /// Block type, record count and record size.
    fn block_header(&mut self) -> Option<(u16, u32, u32)> {
        let kind = self.u16()?;
        let _version = self.u16()?;
        Some((kind, self.u32()?, self.u32()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{pgmf_bytes, rlv_bytes};

    #[test]
    fn reads_the_videos_speed_and_where_frames_are_on_the_course() {
        let rlv = parse_rlv(&rlv_bytes(r"C:\Tacx\Video\Stelvio.avi")).unwrap();

        assert_eq!(rlv.video_file_name(), "Stelvio.avi");
        assert!((rlv.frame_rate - 10.0).abs() < 1e-9);
        // 20 frames at 1 m, then 0.5 m per frame to the course's end at 30 m (frame 40).
        let points = rlv.sync_points();
        assert_eq!(points.len(), 3, "{points:?}");
        assert!((points[1].0 - 20.0).abs() < 1e-6);
        assert_eq!(points[1].1, Duration::from_secs(2));
        assert!((points[2].0 - 30.0).abs() < 1e-6);
        assert_eq!(points[2].1, Duration::from_secs(4));
    }

    #[test]
    fn reads_the_course_profile_and_draws_it_as_a_gpx() {
        let pgmf = parse_pgmf(&pgmf_bytes("Stelvio & co")).unwrap();

        assert_eq!(pgmf.name, "Stelvio & co");
        assert_eq!(pgmf.segments, [(15.0, 5.0), (15.0, -2.0)]);
        let gpx = gpx_from_profile(&pgmf.name, &pgmf);
        assert!(gpx.contains("<name>Stelvio &amp; co</name>"), "{gpx}");
        // Up 0.75 m over the first 15 m, down 0.3 m over the next 15.
        assert!(gpx.contains("<ele>500.75</ele>"), "{gpx}");
        assert!(
            gpx.ends_with("<ele>500.45</ele></trkpt></trkseg></trk></gpx>"),
            "{gpx}"
        );
    }

    #[test]
    fn refuses_other_files_and_workouts() {
        assert!(parse_rlv(b"<gpx/>").is_err());
        assert!(parse_pgmf(&rlv_bytes("x.avi")).is_err());
        let mut workout = pgmf_bytes("Intervals");
        // Watts instead of slope: after the file and block headers, checksum and name.
        workout[8 + 12 + 4 + 34] = 0;
        assert!(matches!(parse_pgmf(&workout), Err(TacxError::NotACourse)));
    }

    #[test]
    fn reads_older_8_bit_names() {
        let mut field = b"Alpe.avi".to_vec();
        field.resize(522, 0);
        assert_eq!(text(&field), "Alpe.avi");
    }
}
