//! What the organizer is choosing, shown to every rider before it is shared (#418): the
//! course's map picture, elevation and figures, and the workout's steps. A preview is only
//! ever shown, never saved or ridden; riders fetch it from the organizer's memory with
//! [`crate::content::fetch_preview`], and it carries none of the course itself.

use orqwa_domain::units::Meters;
use orqwa_domain::workout::Plan;

use crate::activity::{ActivityError, Input};
use crate::code::{MAX_NAME_BYTES, valid_name};

/// Largest encoded preview: 1 MiB, its picture and a workout of the most steps included.
pub const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;
/// Largest map picture in a preview. A course with a larger one is previewed without it.
pub const MAX_PICTURE_BYTES: usize = 512 * 1024;
/// Most points in a preview's track or elevation profile.
pub const MAX_POINTS: usize = 256;
const VERSION: u8 = 1;
const PNG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
/// Bound for every distance, height and coordinate: ten thousand kilometres.
const MAX_METERS: f64 = 10_000_000.0;
/// Bound for the steepest gradient, in percent.
const MAX_GRADE: f64 = 1000.0;

/// A course as the activity picker shows it, without anything to ride it with.
#[derive(Clone, Debug, PartialEq)]
pub struct CoursePreview {
    /// Printable display name, never a file-system path.
    pub name: String,
    /// Route length.
    pub length: Meters,
    /// Total climbing.
    pub climbing: Meters,
    /// Steepest climbing gradient in percent.
    pub steepest: f64,
    /// Thinned track: metres east/north of the start.
    pub track: Vec<[f32; 2]>,
    /// Thinned elevation profile: distance and elevation in metres.
    pub profile: Vec<[f32; 2]>,
    /// The course's map picture, a PNG with the track on it; empty without one.
    pub picture: Vec<u8>,
}

impl CoursePreview {
    fn validate(&self) -> Result<(), ActivityError> {
        let distance =
            |meters: Meters| meters.0.is_finite() && (0.0..=MAX_METERS).contains(&meters.0);
        let points = |points: &[[f32; 2]]| {
            points.len() <= MAX_POINTS
                && points
                    .iter()
                    .flatten()
                    .all(|&value| value.is_finite() && f64::from(value.abs()) <= MAX_METERS)
        };
        let picture = self.picture.len() <= MAX_PICTURE_BYTES
            && (self.picture.is_empty() || self.picture.starts_with(PNG));
        let valid = valid_name(&self.name)
            && distance(self.length)
            && distance(self.climbing)
            && self.steepest.is_finite()
            && self.steepest.abs() <= MAX_GRADE
            && points(&self.track)
            && points(&self.profile)
            && picture;
        valid.then_some(()).ok_or(ActivityError::Invalid)
    }
}

/// The course, the workout or both that the organizer is choosing.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    /// The course, if one is chosen.
    pub course: Option<CoursePreview>,
    /// The workout, without its cues: only its steps are shown.
    pub workout: Option<Plan>,
}

impl Preview {
    /// Checks every limit and that something is previewed.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] for an empty preview, a workout with cues or values
    /// out of bounds.
    pub fn validate(&self) -> Result<(), ActivityError> {
        if self.course.is_none() && self.workout.is_none() {
            return Err(ActivityError::Invalid);
        }
        if let Some(course) = &self.course {
            course.validate()?;
        }
        if let Some(plan) = &self.workout {
            if !plan.cues.is_empty() {
                return Err(ActivityError::Invalid);
            }
            crate::workout::validate(plan)?;
        }
        Ok(())
    }

    /// Bounded binary representation, served to riders who ask for it.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] if validation fails or it exceeds [`MAX_PREVIEW_BYTES`].
    pub fn encode(&self) -> Result<Vec<u8>, ActivityError> {
        self.validate()?;
        let mut out = vec![
            VERSION,
            u8::from(self.course.is_some()) | (u8::from(self.workout.is_some()) << 1),
        ];
        if let Some(course) = &self.course {
            out.push(u8::try_from(course.name.len()).map_err(|_| ActivityError::Invalid)?);
            out.extend_from_slice(course.name.as_bytes());
            for value in [course.length.0, course.climbing.0, course.steepest] {
                out.extend_from_slice(&value.to_be_bytes());
            }
            for points in [&course.track, &course.profile] {
                out.extend_from_slice(
                    &u16::try_from(points.len())
                        .map_err(|_| ActivityError::Invalid)?
                        .to_be_bytes(),
                );
                for value in points.iter().flatten() {
                    out.extend_from_slice(&value.to_be_bytes());
                }
            }
            write_bytes(&mut out, &course.picture)?;
        }
        if let Some(plan) = &self.workout {
            write_bytes(&mut out, &crate::workout::encode(plan)?)?;
        }
        if out.len() as u64 > MAX_PREVIEW_BYTES {
            return Err(ActivityError::Invalid);
        }
        Ok(out)
    }

    /// Parses a preview, rejecting excessive bytes before allocating any of it.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] for unknown versions, malformed, trailing or
    /// excessive bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ActivityError> {
        if bytes.len() as u64 > MAX_PREVIEW_BYTES {
            return Err(ActivityError::Invalid);
        }
        let mut input = Input(bytes);
        if input.byte()? != VERSION {
            return Err(ActivityError::Invalid);
        }
        let flags = input.byte()?;
        if flags == 0 || flags > 3 {
            return Err(ActivityError::Invalid);
        }
        let course = if flags & 1 == 0 {
            None
        } else {
            let name = input.string(MAX_NAME_BYTES)?;
            let [length, climbing, steepest] =
                std::array::from_fn(|_| input.take().map(f64::from_be_bytes));
            Some(CoursePreview {
                name,
                length: Meters(length?),
                climbing: Meters(climbing?),
                steepest: steepest?,
                track: points(&mut input)?,
                profile: points(&mut input)?,
                picture: read_bytes(&mut input, MAX_PICTURE_BYTES)?.to_vec(),
            })
        };
        let workout = if flags & 2 == 0 {
            None
        } else {
            let limit = usize::try_from(crate::activity::MAX_WORKOUT_BYTES)
                .map_err(|_| ActivityError::Invalid)?;
            Some(crate::workout::decode(read_bytes(&mut input, limit)?)?)
        };
        if !input.0.is_empty() {
            return Err(ActivityError::Invalid);
        }
        let preview = Self { course, workout };
        preview.validate()?;
        Ok(preview)
    }
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ActivityError> {
    out.extend_from_slice(
        &u32::try_from(bytes.len())
            .map_err(|_| ActivityError::Invalid)?
            .to_be_bytes(),
    );
    out.extend_from_slice(bytes);
    Ok(())
}

fn read_bytes<'a>(input: &mut Input<'a>, max: usize) -> Result<&'a [u8], ActivityError> {
    let len =
        usize::try_from(u32::from_be_bytes(input.take()?)).map_err(|_| ActivityError::Invalid)?;
    if len > max {
        return Err(ActivityError::Invalid);
    }
    input.bytes(len)
}

fn points(input: &mut Input<'_>) -> Result<Vec<[f32; 2]>, ActivityError> {
    let count = usize::from(u16::from_be_bytes(input.take()?));
    if count > MAX_POINTS {
        return Err(ActivityError::Invalid);
    }
    (0..count)
        .map(|_| {
            Ok([
                f32::from_be_bytes(input.take()?),
                f32::from_be_bytes(input.take()?),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_domain::workout::{Cue, Intensity, Step, Target};
    use std::time::Duration;

    fn course() -> CoursePreview {
        CoursePreview {
            name: "Twann – Chasseral".into(),
            length: Meters(18_400.0),
            climbing: Meters(1210.0),
            steepest: 12.5,
            track: vec![[0.0, 0.0], [120.0, 450.5], [-30.0, 900.0]],
            profile: vec![[0.0, 434.0], [9000.0, 1100.0], [18_400.0, 1607.0]],
            picture: [PNG.as_slice(), b"rest of the picture"].concat(),
        }
    }

    fn plan() -> Plan {
        Plan {
            name: "Sweet spot".into(),
            description: "Two long efforts".into(),
            steps: vec![Step {
                duration: Duration::from_mins(10),
                target: Target::steady(Intensity::Ftp(0.9)),
                cadence: None,
            }],
            cues: Vec::new(),
        }
    }

    #[test]
    fn a_course_and_a_workout_preview_round_trip() {
        for preview in [
            Preview {
                course: Some(course()),
                workout: Some(plan()),
            },
            Preview {
                course: Some(CoursePreview {
                    picture: Vec::new(),
                    ..course()
                }),
                workout: None,
            },
            Preview {
                course: None,
                workout: Some(plan()),
            },
        ] {
            let bytes = preview.encode().unwrap();
            assert_eq!(Preview::decode(&bytes), Ok(preview));
        }
    }

    #[test]
    fn previews_out_of_bounds_are_neither_sent_nor_accepted() {
        let empty = Preview {
            course: None,
            workout: None,
        };
        let with = |change: fn(&mut CoursePreview)| {
            let mut course = course();
            change(&mut course);
            Preview {
                course: Some(course),
                workout: None,
            }
        };
        let mut cued = plan();
        cued.cues.push(Cue {
            at: Duration::from_mins(1),
            text: "Hold it".into(),
        });
        for invalid in [
            empty,
            with(|c| c.name = "C:\\courses\\secret.oqc\n".into()),
            with(|c| c.length = Meters(f64::NAN)),
            with(|c| c.climbing = Meters(-1.0)),
            with(|c| c.steepest = f64::INFINITY),
            with(|c| c.track = vec![[0.0, 0.0]; MAX_POINTS + 1]),
            with(|c| c.profile = vec![[f32::NAN, 0.0]]),
            with(|c| c.picture = b"GIF89a".to_vec()),
            with(|c| c.picture = [PNG.as_slice(), vec![0; MAX_PICTURE_BYTES].as_slice()].concat()),
            Preview {
                course: None,
                workout: Some(cued),
            },
        ] {
            assert_eq!(invalid.encode(), Err(ActivityError::Invalid));
        }
    }

    #[test]
    fn malformed_trailing_and_oversized_previews_are_rejected_before_allocating() {
        let bytes = Preview {
            course: Some(course()),
            workout: Some(plan()),
        }
        .encode()
        .unwrap();
        for end in 0..bytes.len() {
            assert!(Preview::decode(&bytes[..end]).is_err());
        }
        assert!(Preview::decode(&[bytes.as_slice(), &[0_u8]].concat()).is_err());
        let mut other = bytes.clone();
        other[0] = VERSION + 1;
        assert!(Preview::decode(&other).is_err());
        // A picture length beyond the limit fails before its bytes are read.
        let mut huge = vec![VERSION, 1];
        huge.push(1);
        huge.push(b'A');
        huge.extend_from_slice(&[0; 24]);
        huge.extend_from_slice(&[0; 4]);
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(Preview::decode(&huge), Err(ActivityError::Invalid));
        let oversized = vec![0; usize::try_from(MAX_PREVIEW_BYTES).unwrap() + 1];
        assert_eq!(Preview::decode(&oversized), Err(ActivityError::Invalid));
    }
}
