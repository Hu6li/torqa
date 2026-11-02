//! Normalized structured workouts for sharing without converting absolute watts into FTP shares.

use std::time::Duration;

use orqwa_domain::units::{Rpm, Watts};
use orqwa_domain::workout::{Cue, Intensity, Plan, Step, Target};

use crate::activity::{ActivityError, Input, MAX_WORKOUT_BYTES};
use crate::code::valid_name;

const MAGIC: &[u8; 4] = b"OQW1";

/// Plugin adapter for normalized group workouts retained in the app library.
pub struct SharedWorkoutParser;
impl orqwa_domain::workout::WorkoutParser for SharedWorkoutParser {
    fn extensions(&self) -> &'static [&'static str] {
        &["oqw"]
    }
    fn parse(
        &self,
        bytes: &[u8],
        _name: &str,
    ) -> Result<Plan, orqwa_domain::workout::WorkoutFileError> {
        decode(bytes).map_err(|e| orqwa_domain::workout::WorkoutFileError(e.to_string()))
    }
}
/// Maximum steps or cues in one shared workout.
pub const MAX_ITEMS: usize = 4096;
/// Longest shared workout, matching the networking session limit.
pub const MAX_DURATION: Duration = Duration::from_hours(48);
const MAX_DESCRIPTION: usize = 4096;
const MAX_CUE: usize = 1024;

fn text(value: &str, max: usize) -> bool {
    value.len() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}
fn intensity(value: Intensity) -> bool {
    match value {
        Intensity::Ftp(share) => share.is_finite() && (0.0..=5.0).contains(&share),
        Intensity::Watts(watts) => watts.0.is_finite() && (0.0..=5000.0).contains(&watts.0),
    }
}

/// Checks finite targets, positive millisecond durations and all allocation/text limits.
///
/// # Errors
/// Returns [`ActivityError::Invalid`] if a workout is unsupported or exceeds its bounds.
pub fn validate(plan: &Plan) -> Result<(), ActivityError> {
    if !valid_name(&plan.name)
        || !text(&plan.description, MAX_DESCRIPTION)
        || plan.steps.is_empty()
        || plan.steps.len() > MAX_ITEMS
        || plan.cues.len() > MAX_ITEMS
    {
        return Err(ActivityError::Invalid);
    }
    let mut total = Duration::ZERO;
    for step in &plan.steps {
        if step.duration.is_zero()
            || step.duration.subsec_nanos() % 1_000_000 != 0
            || step.duration > MAX_DURATION
            || step
                .cadence
                .is_some_and(|rpm| !rpm.0.is_finite() || !(0.0..=300.0).contains(&rpm.0))
            || matches!(step.target,Target::Power{from,to} if !intensity(from) || !intensity(to))
        {
            return Err(ActivityError::Invalid);
        }
        total = total
            .checked_add(step.duration)
            .ok_or(ActivityError::Invalid)?;
        if total > MAX_DURATION {
            return Err(ActivityError::Invalid);
        }
    }
    for cue in &plan.cues {
        if cue.at > total || cue.at.subsec_nanos() % 1_000_000 != 0 || !text(&cue.text, MAX_CUE) {
            return Err(ActivityError::Invalid);
        }
    }
    Ok(())
}

fn write_text(out: &mut Vec<u8>, text: &str) -> Result<(), ActivityError> {
    out.extend_from_slice(
        &u16::try_from(text.len())
            .map_err(|_| ActivityError::Invalid)?
            .to_be_bytes(),
    );
    out.extend_from_slice(text.as_bytes());
    Ok(())
}
fn write_duration(out: &mut Vec<u8>, duration: Duration) -> Result<(), ActivityError> {
    out.extend_from_slice(
        &u64::try_from(duration.as_millis())
            .map_err(|_| ActivityError::Invalid)?
            .to_be_bytes(),
    );
    Ok(())
}
fn write_intensity(out: &mut Vec<u8>, intensity: Intensity) {
    let (tag, value) = match intensity {
        Intensity::Ftp(share) => (0, share),
        Intensity::Watts(watts) => (1, watts.0),
    };
    out.push(tag);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Encodes a normalized plan, retaining absolute watts, FTP shares, ramps, free steps and cues.
///
/// # Errors
/// Returns [`ActivityError::Invalid`] when the plan or encoded length exceeds its bounds.
pub fn encode(plan: &Plan) -> Result<Vec<u8>, ActivityError> {
    validate(plan)?;
    let encoded_size = 4
        + 2
        + plan.name.len()
        + 2
        + plan.description.len()
        + 2
        + 2
        + plan
            .steps
            .iter()
            .map(|step| {
                8 + 1
                    + usize::from(matches!(step.target, Target::Power { .. })) * 18
                    + 1
                    + usize::from(step.cadence.is_some()) * 8
            })
            .sum::<usize>()
        + plan
            .cues
            .iter()
            .map(|cue| 8 + 2 + cue.text.len())
            .sum::<usize>();
    if encoded_size as u64 > MAX_WORKOUT_BYTES {
        return Err(ActivityError::Invalid);
    }
    let mut out = Vec::with_capacity(encoded_size);
    out.extend_from_slice(MAGIC);
    write_text(&mut out, &plan.name)?;
    write_text(&mut out, &plan.description)?;
    out.extend_from_slice(
        &u16::try_from(plan.steps.len())
            .map_err(|_| ActivityError::Invalid)?
            .to_be_bytes(),
    );
    for step in &plan.steps {
        write_duration(&mut out, step.duration)?;
        match step.target {
            Target::Free => out.push(0),
            Target::Power { from, to } => {
                out.push(1);
                write_intensity(&mut out, from);
                write_intensity(&mut out, to);
            }
        }
        match step.cadence {
            None => out.push(0),
            Some(rpm) => {
                out.push(1);
                out.extend_from_slice(&rpm.0.to_be_bytes());
            }
        }
    }
    out.extend_from_slice(
        &u16::try_from(plan.cues.len())
            .map_err(|_| ActivityError::Invalid)?
            .to_be_bytes(),
    );
    for cue in &plan.cues {
        write_duration(&mut out, cue.at)?;
        write_text(&mut out, &cue.text)?;
    }
    if out.len() as u64 > MAX_WORKOUT_BYTES {
        return Err(ActivityError::Invalid);
    }
    Ok(out)
}

fn read_text(input: &mut Input<'_>, max: usize) -> Result<String, ActivityError> {
    let len = usize::from(u16::from_be_bytes(input.take()?));
    if len > max {
        return Err(ActivityError::Invalid);
    }
    let value = std::str::from_utf8(input.0.get(..len).ok_or(ActivityError::Invalid)?)
        .map_err(|_| ActivityError::Invalid)?;
    input.0 = &input.0[len..];
    Ok(value.into())
}
fn read_intensity(input: &mut Input<'_>) -> Result<Intensity, ActivityError> {
    let tag = input.byte()?;
    let value = f64::from_be_bytes(input.take()?);
    match tag {
        0 => Ok(Intensity::Ftp(value)),
        1 => Ok(Intensity::Watts(Watts(value))),
        _ => Err(ActivityError::Invalid),
    }
}
fn count(input: &mut Input<'_>) -> Result<usize, ActivityError> {
    let count = usize::from(u16::from_be_bytes(input.take()?));
    if count > MAX_ITEMS {
        return Err(ActivityError::Invalid);
    }
    Ok(count)
}

/// Decodes only bounded normalized plans, checking size before any peer-dependent allocation.
///
/// # Errors
/// Returns [`ActivityError::Invalid`] for invalid versions, values, lengths or trailing bytes.
pub fn decode(bytes: &[u8]) -> Result<Plan, ActivityError> {
    if bytes.len() as u64 > MAX_WORKOUT_BYTES {
        return Err(ActivityError::Invalid);
    }
    let mut input = Input(bytes);
    if &input.take::<4>()? != MAGIC {
        return Err(ActivityError::Invalid);
    }
    let name = read_text(&mut input, crate::code::MAX_NAME_BYTES)?;
    let description = read_text(&mut input, MAX_DESCRIPTION)?;
    let steps = count(&mut input)?;
    let mut plan = Plan {
        name,
        description,
        steps: Vec::with_capacity(steps),
        cues: Vec::new(),
    };
    for _ in 0..steps {
        let duration = Duration::from_millis(u64::from_be_bytes(input.take()?));
        let target = match input.byte()? {
            0 => Target::Free,
            1 => Target::Power {
                from: read_intensity(&mut input)?,
                to: read_intensity(&mut input)?,
            },
            _ => return Err(ActivityError::Invalid),
        };
        let cadence = match input.byte()? {
            0 => None,
            1 => Some(Rpm(f64::from_be_bytes(input.take()?))),
            _ => return Err(ActivityError::Invalid),
        };
        plan.steps.push(Step {
            duration,
            target,
            cadence,
        });
    }
    let cues = count(&mut input)?;
    for _ in 0..cues {
        let at = Duration::from_millis(u64::from_be_bytes(input.take()?));
        plan.cues.push(Cue {
            at,
            text: read_text(&mut input, MAX_CUE)?,
        });
    }
    if !input.0.is_empty() {
        return Err(ActivityError::Invalid);
    }
    validate(&plan)?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        Plan {
            name: "Together".into(),
            description: "A shared workout\nAt your own FTP".into(),
            steps: vec![
                Step {
                    duration: Duration::from_secs(60),
                    target: Target::Power {
                        from: Intensity::Ftp(0.5),
                        to: Intensity::Ftp(0.9),
                    },
                    cadence: Some(Rpm(90.0)),
                },
                Step {
                    duration: Duration::from_secs(30),
                    target: Target::steady(Intensity::Watts(Watts(180.0))),
                    cadence: None,
                },
                Step {
                    duration: Duration::from_secs(10),
                    target: Target::Free,
                    cadence: None,
                },
            ],
            cues: vec![Cue {
                at: Duration::from_secs(50),
                text: "Almost there".into(),
            }],
        }
    }
    #[test]
    fn normalized_workouts_keep_absolute_watts_and_personal_ftp() {
        let original = plan();
        let encoded = encode(&original).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(
            decoded.at(Duration::ZERO, Watts(200.0)).unwrap().power,
            Some(Watts(100.0))
        );
        assert_eq!(
            decoded.at(Duration::ZERO, Watts(300.0)).unwrap().power,
            Some(Watts(150.0))
        );
        assert_eq!(
            decoded
                .at(Duration::from_secs(61), Watts(300.0))
                .unwrap()
                .power,
            Some(Watts(180.0))
        );
        for end in 0..encoded.len() {
            assert!(decode(&encoded[..end]).is_err());
        }
        let mut trailing = encoded;
        trailing.push(0);
        assert!(decode(&trailing).is_err());
    }
    #[test]
    fn malformed_targets_counts_and_durations_are_rejected() {
        for watts in [f64::NAN, f64::INFINITY, -1.0, 5001.0] {
            let mut p = plan();
            p.steps[0].target = Target::steady(Intensity::Watts(Watts(watts)));
            assert!(encode(&p).is_err());
        }
        for duration in [
            Duration::ZERO,
            MAX_DURATION + Duration::from_secs(1),
            Duration::from_nanos(1),
        ] {
            let mut p = plan();
            p.steps[0].duration = duration;
            assert!(encode(&p).is_err());
        }
        let mut p = plan();
        p.cues[0].at = MAX_DURATION;
        assert!(encode(&p).is_err());
        let mut p = plan();
        p.steps = vec![p.steps[0]; MAX_ITEMS + 1];
        assert!(encode(&p).is_err());
        assert!(decode(&vec![0; usize::try_from(MAX_WORKOUT_BYTES).unwrap() + 1]).is_err());
    }
}
