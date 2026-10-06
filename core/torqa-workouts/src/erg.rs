//! ERG and MRC workout files: plain text with a header and points of (minutes, power) joined
//! by straight lines — in watts (`MINUTES WATTS`, `.erg`) or percent of FTP
//! (`MINUTES PERCENT`, `.mrc`). Two points at the same minute are a step.

use std::time::Duration;

use torqa_domain::units::Watts;
use torqa_domain::workout::{Cue, Intensity, Plan, Step, Target, WorkoutFileError, WorkoutParser};

/// Reads `.erg` and `.mrc` files; the header says which unit the points are in.
#[derive(Debug, Clone, Copy, Default)]
pub struct ErgParser;

impl WorkoutParser for ErgParser {
    fn extensions(&self) -> &'static [&'static str] {
        &["erg", "mrc"]
    }

    fn parse(&self, bytes: &[u8], name: &str) -> Result<Plan, WorkoutFileError> {
        parse(&String::from_utf8_lossy(bytes), name)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    None,
    Header,
    Data,
    Text,
}

#[derive(Clone, Copy)]
enum Unit {
    Watts,
    Percent,
}

fn parse(text: &str, fallback_name: &str) -> Result<Plan, WorkoutFileError> {
    let mut plan = Plan::default();
    let mut section = Section::None;
    let mut unit = None;
    let mut points: Vec<(f64, f64)> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let upper = line.to_ascii_uppercase();
        match upper.as_str() {
            "[COURSE HEADER]" => section = Section::Header,
            "[COURSE DATA]" => section = Section::Data,
            "[COURSE TEXT]" => section = Section::Text,
            _ if upper.starts_with("[END") => section = Section::None,
            _ => match section {
                Section::Header => {
                    let words: Vec<&str> = upper.split_whitespace().collect();
                    if words.first() == Some(&"MINUTES") {
                        unit = match words.get(1).copied() {
                            Some("WATTS") => Some(Unit::Watts),
                            Some("PERCENT") => Some(Unit::Percent),
                            other => {
                                return Err(WorkoutFileError(format!(
                                    "unknown power unit {}",
                                    other.unwrap_or("(none)")
                                )));
                            }
                        };
                    } else if let Some((key, value)) = line.split_once('=') {
                        let value = value.trim().to_owned();
                        match key.trim().to_ascii_uppercase().as_str() {
                            "DESCRIPTION" => plan.description = value,
                            "FILE NAME" => plan.name = value,
                            _ => {}
                        }
                    }
                }
                Section::Data => {
                    let mut numbers = line.split_whitespace().map(str::parse::<f64>);
                    match (numbers.next(), numbers.next()) {
                        (Some(Ok(minutes)), Some(Ok(value))) => points.push((minutes, value)),
                        _ => {
                            return Err(WorkoutFileError(format!(
                                "not a point of minutes and power: {line:?}"
                            )));
                        }
                    }
                }
                Section::Text => {
                    // seconds <tab> text [<tab> how long to show it]
                    let mut parts = line.split('\t');
                    if let (Some(Ok(seconds)), Some(message)) =
                        (parts.next().map(|s| s.trim().parse::<f64>()), parts.next())
                    {
                        plan.cues.push(Cue {
                            at: Duration::from_secs_f64(seconds.max(0.0)),
                            text: message.trim().to_owned(),
                        });
                    }
                }
                Section::None => {}
            },
        }
    }
    let unit = unit.ok_or_else(|| {
        WorkoutFileError("not an ERG or MRC file (no MINUTES WATTS or PERCENT)".to_owned())
    })?;
    let intensity = |value: f64| match unit {
        Unit::Watts => Intensity::Watts(Watts(value)),
        Unit::Percent => Intensity::Ftp(value / 100.0),
    };
    for pair in points.windows(2) {
        let ((start, from), (end, to)) = (pair[0], pair[1]);
        if end > start {
            plan.steps.push(Step {
                duration: Duration::from_secs_f64((end - start) * 60.0),
                target: Target::Power {
                    from: intensity(from),
                    to: intensity(to),
                },
                cadence: None,
            });
        }
    }
    if plan.steps.is_empty() {
        return Err(WorkoutFileError("the workout has no steps".to_owned()));
    }
    if plan.name.is_empty() {
        fallback_name.clone_into(&mut plan.name);
    }
    plan.cues.sort_by_key(|cue| cue.at);
    Ok(plan)
}
