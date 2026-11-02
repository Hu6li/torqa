//! What the organizer is choosing, made ready for friends to see before it is shared (#418).
//! Friends fetch it over the group link, whose flow control makes every kilobyte take time,
//! so the map picture goes at half its size.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use orqwa_domain::units::Meters;
use orqwa_domain::workout::Plan;
use orqwa_net::preview::{CoursePreview, MAX_PICTURE_BYTES, MAX_POINTS, Preview};
use orqwa_storage::course;

/// Pictures wider than this go at half their size: wider than friends' preview shows them.
const FULL_WIDTH: u32 = 640;

/// What the organizer's activity picker has: a library course, a workout or both.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Choice {
    pub(super) course: Option<PathBuf>,
    pub(super) plan: Option<Plan>,
}

impl Choice {
    pub(super) const fn is_empty(&self) -> bool {
        self.course.is_none() && self.plan.is_none()
    }
}

/// The preview of `choice`, reading its course file: blocking.
pub(super) fn build(choice: &Choice) -> Result<Preview, String> {
    let course = choice.course.as_deref().map(course_preview).transpose()?;
    // Friends see the workout's steps; its cues would only make it take longer to arrive.
    let workout = choice.plan.clone().map(|plan| Plan {
        cues: Vec::new(),
        ..plan
    });
    let preview = Preview { course, workout };
    preview.validate().map_err(|error| error.to_string())?;
    Ok(preview)
}

fn course_preview(path: &Path) -> Result<CoursePreview, String> {
    let manifest = course::read_manifest(path).map_err(|error| error.to_string())?;
    let picture = course::preview(path)
        .and_then(|png| smaller(&png))
        .filter(|png| png.len() <= MAX_PICTURE_BYTES)
        .unwrap_or_default();
    Ok(CoursePreview {
        name: shown_name(&manifest.name),
        length: Meters(manifest.length_m),
        climbing: Meters(manifest.elevation_gain_m),
        steepest: manifest.max_grade_percent,
        track: thin(&manifest.track),
        profile: thin(&manifest.profile),
        picture,
    })
}

/// The course name as a group may carry it: printable, at most 64 bytes.
fn shown_name(name: &str) -> String {
    let mut shown: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    while shown.len() > orqwa_net::code::MAX_NAME_BYTES {
        shown.pop();
    }
    shown.trim().to_owned()
}

/// At most [`MAX_POINTS`] of `points`, evenly spread, the last one kept.
fn thin(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if points.len() <= MAX_POINTS {
        return points.to_vec();
    }
    let step = points.len().div_ceil(MAX_POINTS - 1);
    points
        .iter()
        .step_by(step)
        .chain(points.last())
        .copied()
        .collect()
}

/// The map picture `png` at half its size when it is wider than [`FULL_WIDTH`]: a quarter of
/// the pixels, as drawn for the gallery's cards at twice their size. `None` if it is no
/// picture ORQWA can read.
fn smaller(png: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    if info.width <= FULL_WIDTH {
        return Some(png.to_vec());
    }
    let (width, height) = (info.width / 2, info.height / 2);
    let channels = info.line_size / usize::try_from(info.width).ok()?;
    let color = match channels {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        4 => png::ColorType::Rgba,
        _ => return None,
    };
    let row = info.line_size;
    let (columns, rows) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    let mut half = Vec::with_capacity(columns * rows * channels);
    for y in 0..rows {
        for x in 0..columns {
            for channel in 0..channels {
                let at = |dx: usize, dy: usize| {
                    u16::from(pixels[(2 * y + dy) * row + (2 * x + dx) * channels + channel])
                };
                let sum = at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1);
                half.push(u8::try_from((sum + 2) / 4).unwrap_or(u8::MAX));
            }
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&half).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|n| {
                let (x, y) = (n % width, n / width);
                // Two-pixel stripes: halving them averages each pair into one grey.
                let value = if x % 2 == 0 { 200 } else { 100 };
                [value, u8::try_from((y / 2) % 256).unwrap(), 0]
            })
            .collect();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        out
    }

    fn decoded(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        (info.width, info.height, pixels)
    }

    #[test]
    fn a_large_map_picture_goes_at_half_its_size_and_a_small_one_as_it_is() {
        let large = picture(1072, 640);
        let half = smaller(&large).unwrap();
        let (width, height, pixels) = decoded(&half);
        assert_eq!((width, height), (536, 320));
        assert_eq!(
            &pixels[..3],
            &[150, 0, 0],
            "the average of each two-by-two block"
        );
        assert!(half.len() < large.len());
        let small = picture(536, 320);
        assert_eq!(smaller(&small).unwrap(), small);
        assert_eq!(smaller(b"PNG"), None);
    }

    #[test]
    fn a_preview_carries_a_printable_name_and_at_most_its_points() {
        assert_eq!(
            shown_name(" Twann\n– Chasseral\u{202e} "),
            "Twann– Chasseral"
        );
        let long = "Ä".repeat(40);
        assert!(shown_name(&long).len() <= 64);
        let points: Vec<[f32; 2]> = (0..1000_u16).map(|n| [f32::from(n), 0.0]).collect();
        let thinned = thin(&points);
        assert!(thinned.len() <= MAX_POINTS);
        assert_eq!(thinned.first(), points.first());
        assert_eq!(thinned.last(), points.last());
        assert_eq!(thin(&points[..10]), &points[..10]);
    }

    #[test]
    fn a_workout_is_previewed_without_its_cues() {
        let mut plan = orqwa_workouts::load("builtin:recovery-30").unwrap();
        plan.cues.push(orqwa_domain::workout::Cue {
            at: std::time::Duration::from_mins(1),
            text: "Easy now".into(),
        });
        let preview = build(&Choice {
            course: None,
            plan: Some(plan.clone()),
        })
        .unwrap();
        let shown = preview.workout.unwrap();
        assert_eq!(shown.cues, Vec::new());
        assert_eq!(shown.steps, plan.steps);
        assert!(preview.course.is_none());
    }
}
