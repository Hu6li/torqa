//! FIT workout files (`.fit` of type workout), as Garmin Connect, `TrainingPeaks` and others
//! export them: steps by time with a power range, a power zone or none, and repeats of earlier
//! steps.

use std::io::Cursor;
use std::time::Duration;

use embedded_io_adapters::std::FromStd;
use rustyfit::Decoder;
use rustyfit::profile::{mesgdef, typedef};
use torqa_domain::units::{Rpm, Watts};
use torqa_domain::workout::{Cue, Intensity, Plan, Step, Target, WorkoutFileError, WorkoutParser};

use crate::zone_share;

/// FIT power targets up to this are a percentage of FTP; above, watts plus this.
const WATTS_OFFSET: u32 = 1000;

/// Reads FIT workout files.
#[derive(Debug, Clone, Copy, Default)]
pub struct FitParser;

impl WorkoutParser for FitParser {
    fn extensions(&self) -> &'static [&'static str] {
        &["fit"]
    }

    fn parse(&self, bytes: &[u8], name: &str) -> Result<Plan, WorkoutFileError> {
        let fit = Decoder::new()
            .decode(FromStd::new(Cursor::new(bytes)))
            .map_err(|e| WorkoutFileError(format!("not a readable FIT file: {e:?}")))?
            .ok_or_else(|| WorkoutFileError("empty FIT file".to_owned()))?;
        let of = |num| fit.messages.iter().filter(move |m| m.num == num);
        let workout = of(typedef::MesgNum::WORKOUT)
            .next()
            .map(mesgdef::Workout::from)
            .ok_or_else(|| {
                WorkoutFileError("a FIT file, but not a workout (e.g. a ride)".to_owned())
            })?;
        let steps: Vec<mesgdef::WorkoutStep> = of(typedef::MesgNum::WORKOUT_STEP)
            .map(mesgdef::WorkoutStep::from)
            .collect();
        let mut plan = Plan {
            name: if workout.wkt_name.trim().is_empty() {
                name.to_owned()
            } else {
                workout.wkt_name.trim().to_owned()
            },
            description: workout.wkt_description.trim().to_owned(),
            ..Plan::default()
        };
        add_steps(&mut plan, &steps)?;
        if plan.steps.is_empty() {
            return Err(WorkoutFileError("the workout has no steps".to_owned()));
        }
        Ok(plan)
    }
}

fn add_steps(plan: &mut Plan, steps: &[mesgdef::WorkoutStep]) -> Result<(), WorkoutFileError> {
    // Where each FIT step's own plan steps (and cues) begin, for repeats back to it.
    let mut begins: Vec<(usize, usize)> = Vec::with_capacity(steps.len());
    for step in steps {
        begins.push((plan.steps.len(), plan.cues.len()));
        match step.duration_type {
            typedef::WktStepDuration::TIME => {
                let label = if step.wkt_step_name.trim().is_empty() {
                    step.notes.trim()
                } else {
                    step.wkt_step_name.trim()
                };
                if !label.is_empty() {
                    plan.cues.push(Cue {
                        at: plan.duration(),
                        text: label.to_owned(),
                    });
                }
                let (target, cadence) = target(step);
                plan.steps.push(Step {
                    duration: Duration::from_millis(u64::from(step.duration_value)),
                    target,
                    cadence,
                });
            }
            typedef::WktStepDuration::REPEAT_UNTIL_STEPS_CMPLT => {
                let first = usize::try_from(step.duration_value).unwrap_or(usize::MAX);
                let &(from_step, from_cue) = begins.get(first).ok_or_else(|| {
                    WorkoutFileError(format!("a repeat back to step {first}, which is not there"))
                })?;
                let block = plan.steps[from_step..].to_vec();
                let block_time: Duration = block.iter().map(|s| s.duration).sum();
                let cues = plan.cues[from_cue..].to_vec();
                // The block has run once already; `target_value` is how often in all.
                for round in 1..step.target_value.clamp(1, 1000) {
                    let shift = block_time * round;
                    plan.steps.extend_from_slice(&block);
                    plan.cues.extend(cues.iter().map(|cue| Cue {
                        at: cue.at + shift,
                        text: cue.text.clone(),
                    }));
                }
            }
            other => {
                return Err(WorkoutFileError(format!(
                    "steps that end by {} are not supported, only by time",
                    duration_kind(other)
                )));
            }
        }
    }
    Ok(())
}

/// What a FIT step asks for: a power range is held at its middle; other targets (heart rate,
/// speed, open) leave the power free, keeping a cadence target.
fn target(step: &mesgdef::WorkoutStep) -> (Target, Option<Rpm>) {
    let (low, high) = (step.custom_target_value_low, step.custom_target_value_high);
    let custom = low != 0 && high != 0 && low != u32::MAX && high != u32::MAX;
    match step.target_type {
        typedef::WktStepTarget::POWER if custom => {
            let intensity = if low > WATTS_OFFSET && high > WATTS_OFFSET {
                Intensity::Watts(Watts(f64::midpoint(
                    f64::from(low - WATTS_OFFSET),
                    f64::from(high - WATTS_OFFSET),
                )))
            } else {
                Intensity::Ftp(f64::midpoint(f64::from(low), f64::from(high)) / 100.0)
            };
            (Target::steady(intensity), None)
        }
        typedef::WktStepTarget::POWER if (1..=7).contains(&step.target_value) => (
            Target::steady(Intensity::Ftp(zone_share(f64::from(step.target_value)))),
            None,
        ),
        typedef::WktStepTarget::CADENCE if custom => (
            Target::Free,
            Some(Rpm(f64::midpoint(f64::from(low), f64::from(high)))),
        ),
        _ => (Target::Free, None),
    }
}

fn duration_kind(kind: typedef::WktStepDuration) -> &'static str {
    match kind {
        typedef::WktStepDuration::DISTANCE => "distance",
        typedef::WktStepDuration::OPEN => "the lap button",
        typedef::WktStepDuration::HR_LESS_THAN | typedef::WktStepDuration::HR_GREATER_THAN => {
            "heart rate"
        }
        typedef::WktStepDuration::CALORIES => "calories",
        _ => "something else",
    }
}
