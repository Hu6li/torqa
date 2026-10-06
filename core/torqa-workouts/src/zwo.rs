//! Zwift workout files (`.zwo`): XML with steps whose power is a share of FTP. Zwift never
//! published the format; this follows the community reference
//! (github.com/h4l/zwift-workout-file-reference) and the files found in the wild.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use torqa_domain::units::{Rpm, Watts};
use torqa_domain::workout::{Cue, Intensity, Plan, Step, Target, WorkoutFileError, WorkoutParser};

use crate::zone_share;

/// Reads `.zwo` files.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZwoParser;

impl WorkoutParser for ZwoParser {
    fn extensions(&self) -> &'static [&'static str] {
        &["zwo"]
    }

    fn parse(&self, bytes: &[u8], name: &str) -> Result<Plan, WorkoutFileError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| WorkoutFileError("the workout is not UTF-8 text".to_owned()))?;
        parse(text, name)
    }
}

fn parse(xml: &str, fallback_name: &str) -> Result<Plan, WorkoutFileError> {
    let mut reader = Reader::from_str(xml);
    let mut plan = Plan::default();
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    // When the element under way inside <workout> started: its text cues count from there.
    let mut element_start = Duration::ZERO;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| WorkoutFileError(format!("not a readable ZWO file: {e}")))?;
        let (element, empty) = match &event {
            Event::Start(e) => (Some(e), false),
            Event::Empty(e) => (Some(e), true),
            _ => (None, false),
        };
        if let Some(e) = element {
            let name = lower(e);
            if path.is_empty() && name != "workout_file" {
                return Err(WorkoutFileError("not a ZWO workout file".to_owned()));
            }
            if path.is_empty()
                && let Some(kind) = attributes(e)?.get("durationtype")
                && kind != "time"
            {
                return Err(WorkoutFileError(
                    "workouts by distance are not supported".to_owned(),
                ));
            }
            if name == "textevent" {
                add_cue(&mut plan, e, element_start)?;
            } else if path.last().is_some_and(|parent| parent == "workout") {
                element_start = plan.duration();
                add_element(&mut plan, e)?;
            }
            // An empty element has no end event to pop it.
            if !empty {
                path.push(name);
                text.clear();
            }
            continue;
        }
        match event {
            Event::Text(t) => text.push_str(&t.xml10_content()),
            // Entities in text (e.g. `&amp;` in a name) come as events of their own.
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else {
                    text.push_str(match r.to_string().as_str() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                match (path.len(), path.last().map(String::as_str)) {
                    (2, Some("name")) => text.trim().clone_into(&mut plan.name),
                    (2, Some("description")) => text.trim().clone_into(&mut plan.description),
                    (2, Some("sporttype")) if !text.trim().eq_ignore_ascii_case("bike") => {
                        return Err(WorkoutFileError(format!(
                            "not a cycling workout ({})",
                            text.trim()
                        )));
                    }
                    _ => {}
                }
                path.pop();
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if plan.steps.is_empty() {
        return Err(WorkoutFileError(
            "not a ZWO workout file with steps".to_owned(),
        ));
    }
    if plan.name.is_empty() {
        fallback_name.clone_into(&mut plan.name);
    }
    plan.cues.sort_by_key(|cue| cue.at);
    Ok(plan)
}

/// A text cue, `timeoffset` seconds after the start of the element it is in.
fn add_cue(
    plan: &mut Plan,
    e: &BytesStart<'_>,
    element_start: Duration,
) -> Result<(), WorkoutFileError> {
    let fields = attributes(e)?;
    let offset = fields
        .get("timeoffset")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0);
    // "mssage" is in some of Zwift's own files.
    if let Some(message) = fields.get("message").or_else(|| fields.get("mssage")) {
        plan.cues.push(Cue {
            at: element_start + Duration::from_secs_f64(offset),
            text: message.trim().to_owned(),
        });
    }
    Ok(())
}

/// Appends the steps of one element of `<workout>`.
fn add_element(plan: &mut Plan, e: &BytesStart<'_>) -> Result<(), WorkoutFileError> {
    let kind = lower(e);
    let fields = attributes(e)?;
    let number = |key: &str| fields.get(key).and_then(|v| v.trim().parse::<f64>().ok());
    let seconds = |key: &str| number(key).map(|s| Duration::from_secs_f64(s.max(0.0)));
    let cadence = |key: &str| number(key).filter(|&c| c > 0.0).map(Rpm);
    // A steady power: `Power`, the middle of `PowerLow`–`PowerHigh`, or a zone's middle.
    let steady = |single: &str, low: &str, high: &str| {
        number(single)
            .or_else(|| Some(f64::midpoint(number(low)?, number(high)?)))
            .or_else(|| number("zone").map(zone_share))
            .map(Intensity::Ftp)
    };
    let mut step = |duration: Duration, target: Target, cadence: Option<Rpm>| {
        if !duration.is_zero() {
            plan.steps.push(Step {
                duration,
                target,
                cadence,
            });
        }
    };
    match kind.as_str() {
        "warmup" | "cooldown" | "ramp" => {
            let duration = seconds("duration").unwrap_or_default();
            let (Some(low), Some(high)) = (number("powerlow"), number("powerhigh")) else {
                // A ramp with a single power is a steady step.
                let power = steady("power", "powerlow", "powerhigh")
                    .ok_or_else(|| missing(&kind, "power"))?;
                step(duration, Target::steady(power), cadence("cadence"));
                return Ok(());
            };
            // Files disagree on which of the two comes first in a cool-down; a warm-up always
            // rises and a cool-down always falls, whatever the order.
            let (from, to) = match kind.as_str() {
                "warmup" => (low.min(high), low.max(high)),
                "cooldown" => (low.max(high), low.min(high)),
                _ => (low, high),
            };
            step(
                duration,
                Target::Power {
                    from: Intensity::Ftp(from),
                    to: Intensity::Ftp(to),
                },
                cadence("cadence"),
            );
        }
        "steadystate" | "solidstate" => {
            let power =
                steady("power", "powerlow", "powerhigh").ok_or_else(|| missing(&kind, "power"))?;
            step(
                seconds("duration").unwrap_or_default(),
                Target::steady(power),
                cadence("cadence"),
            );
        }
        "intervalst" => {
            let on = steady("onpower", "poweronlow", "poweronhigh")
                .ok_or_else(|| missing(&kind, "OnPower"))?;
            let off = steady("offpower", "powerofflow", "poweroffhigh")
                .ok_or_else(|| missing(&kind, "OffPower"))?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped first
            let repeat = number("repeat").map_or(1, |r| r.round().clamp(1.0, 1000.0) as u32);
            for _ in 0..repeat {
                step(
                    seconds("onduration").unwrap_or_default(),
                    Target::steady(on),
                    cadence("cadence"),
                );
                step(
                    seconds("offduration").unwrap_or_default(),
                    Target::steady(off),
                    cadence("cadenceresting"),
                );
            }
        }
        "freeride" | "maxeffort" => {
            step(
                seconds("duration").unwrap_or_default(),
                Target::Free,
                cadence("cadence"),
            );
        }
        // Text cues and anything newer Zwift may add take no time.
        _ => {}
    }
    Ok(())
}

fn missing(element: &str, attribute: &str) -> WorkoutFileError {
    WorkoutFileError(format!("a {element} step without {attribute}"))
}

fn lower(e: &BytesStart<'_>) -> String {
    e.local_name().as_ref().to_ascii_lowercase()
}

/// The attributes of an element, by lowercase name (files differ in case).
fn attributes(e: &BytesStart<'_>) -> Result<HashMap<String, String>, WorkoutFileError> {
    let mut fields = HashMap::new();
    for attribute in e.attributes() {
        let attribute =
            attribute.map_err(|err| WorkoutFileError(format!("invalid attribute: {err}")))?;
        let key = attribute.key.local_name().as_ref().to_ascii_lowercase();
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| WorkoutFileError(format!("invalid attribute: {err}")))?;
        fields.insert(key, value.into_owned());
    }
    Ok(fields)
}

/// Writes `plan` as a ZWO file, the format of the workouts the editor makes. Steps in watts
/// become shares of `ftp`; cues are kept in the step they fall in.
#[must_use]
pub fn write(plan: &Plan, ftp: Watts) -> String {
    let share = |intensity: Intensity| match intensity {
        Intensity::Ftp(share) => share,
        Intensity::Watts(watts) => watts.0 / ftp.0.max(1.0),
    };
    let mut xml = String::from("<workout_file>\n");
    let _ = writeln!(xml, "    <author>Torqa</author>");
    let _ = writeln!(xml, "    <name>{}</name>", escape(&plan.name));
    let _ = writeln!(
        xml,
        "    <description>{}</description>",
        escape(&plan.description)
    );
    xml.push_str("    <sportType>bike</sportType>\n    <workout>\n");
    let mut start = Duration::ZERO;
    for (index, step) in plan.steps.iter().enumerate() {
        let end = start + step.duration;
        let last = index + 1 == plan.steps.len();
        let duration = number(step.duration.as_secs_f64());
        let cadence = step
            .cadence
            .map(|c| format!(" Cadence=\"{}\"", number(c.0)))
            .unwrap_or_default();
        let element = match step.target {
            Target::Power { from, to } if from == to => format!(
                "SteadyState Duration=\"{duration}\" Power=\"{}\"{cadence}",
                number(share(from))
            ),
            Target::Power { from, to } => format!(
                "Ramp Duration=\"{duration}\" PowerLow=\"{}\" PowerHigh=\"{}\"{cadence}",
                number(share(from)),
                number(share(to))
            ),
            Target::Free => format!("FreeRide Duration=\"{duration}\"{cadence}"),
        };
        let cues: Vec<&Cue> = plan
            .cues
            .iter()
            .filter(|cue| cue.at >= start && (cue.at < end || last))
            .collect();
        if cues.is_empty() {
            let _ = writeln!(xml, "        <{element}/>");
        } else {
            let _ = writeln!(xml, "        <{element}>");
            for cue in cues {
                let _ = writeln!(
                    xml,
                    "            <textevent timeoffset=\"{}\" message=\"{}\"/>",
                    number(cue.at.saturating_sub(start).as_secs_f64()),
                    escape(&cue.text)
                );
            }
            let tag = element.split(' ').next().unwrap_or_default();
            let _ = writeln!(xml, "        </{tag}>");
        }
        start = end;
    }
    xml.push_str("    </workout>\n</workout_file>\n");
    xml
}

/// A number as short as it can be: whole numbers without decimals, others to 3 places.
fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
