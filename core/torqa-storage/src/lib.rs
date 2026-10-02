//! Storage for Torqa: recorded rides as FIT activities (R28) and course files (R32–R35).

pub mod course;

use std::io::Cursor;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use embedded_io_adapters::std::FromStd;
use rustyfit::Encoder;
use rustyfit::profile::{mesgdef, typedef};
use rustyfit::proto::{FIT, Message};
use torqa_domain::recording::Sample;

/// FIT export failed.
#[derive(Debug, thiserror::Error)]
pub enum FitError {
    /// There is nothing to export.
    #[error("ride has no samples")]
    Empty,
    /// The encoder rejected the data.
    #[error("FIT encoding failed: {0}")]
    Encode(String),
}

/// Encodes a recorded ride as a FIT activity file.
///
/// The session is tagged as a virtual cycling activity, so platforms such as Strava file it
/// as a virtual ride.
///
/// # Errors
/// [`FitError::Empty`] without samples, [`FitError::Encode`] if encoding fails.
pub fn encode_fit(start: SystemTime, samples: &[Sample]) -> Result<Vec<u8>, FitError> {
    let last = samples.last().ok_or(FitError::Empty)?;
    let start_secs = start
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let at = |elapsed: Duration| {
        typedef::DateTime::from_unix_timestamp(
            start_secs + i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
        )
    };
    let start_time = at(Duration::ZERO);
    let end_time = at(last.elapsed);
    let summary = Summary::of(samples);

    let mut messages = Vec::with_capacity(samples.len() + 6);
    messages.push(Message::from(file_id(start_time)));
    messages.push(Message::from(timer_event(
        start_time,
        typedef::EventType::START,
    )));
    messages.extend(
        samples
            .iter()
            .map(|s| Message::from(record(at(s.elapsed), s))),
    );
    messages.push(Message::from(timer_event(
        end_time,
        typedef::EventType::STOP_ALL,
    )));
    messages.push(Message::from(lap(start_time, end_time, &summary)));
    messages.push(Message::from(session(start_time, end_time, &summary)));
    messages.push(Message::from(activity(end_time, &summary)));

    let mut fit = FIT {
        messages,
        ..Default::default()
    };
    let mut buffer = Cursor::new(Vec::new());
    Encoder::new()
        .encode(FromStd::new(&mut buffer), &mut fit)
        .map_err(|e| FitError::Encode(format!("{e:?}")))?;
    Ok(buffer.into_inner())
}

/// Aggregates for lap and session messages.
struct Summary {
    elapsed: f64,
    distance: f64,
    ascent: f64,
    avg_speed: f64,
    max_speed: f64,
    avg_power: Option<f64>,
    max_power: Option<f64>,
    avg_cadence: Option<f64>,
    avg_heart_rate: Option<f64>,
    max_heart_rate: Option<f64>,
}

impl Summary {
    fn of(samples: &[Sample]) -> Self {
        let last = samples[samples.len() - 1];
        let elapsed = last.elapsed.as_secs_f64();
        let average = |values: Vec<f64>| {
            #[allow(clippy::cast_precision_loss)]
            let count = values.len() as f64;
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / count)
        };
        let maximum = |values: Vec<f64>| values.into_iter().reduce(f64::max);
        let powers: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.power.map(|p| p.0))
            .collect();
        // Average cadence conventionally excludes time spent not pedalling.
        let cadences: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.cadence.map(|c| c.0))
            .filter(|&c| c > 0.0)
            .collect();
        let heart_rates: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.heart_rate.map(|h| h.0))
            .collect();
        Self {
            elapsed,
            distance: last.distance.0,
            ascent: samples
                .windows(2)
                .map(|w| (w[1].elevation.0 - w[0].elevation.0).max(0.0))
                .sum(),
            avg_speed: if elapsed > 0.0 {
                last.distance.0 / elapsed
            } else {
                0.0
            },
            max_speed: samples.iter().map(|s| s.speed.0).fold(0.0, f64::max),
            avg_power: average(powers.clone()),
            max_power: maximum(powers),
            avg_cadence: average(cadences),
            avg_heart_rate: average(heart_rates.clone()),
            max_heart_rate: maximum(heart_rates),
        }
    }
}

// FIT fields are fixed-size integers; values are rounded and clamped to the field range first.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u8(value: f64) -> u8 {
    value.round().clamp(0.0, f64::from(u8::MAX - 1)) as u8
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u16(value: f64) -> u16 {
    value.round().clamp(0.0, f64::from(u16::MAX - 1)) as u16
}

#[allow(clippy::cast_possible_truncation)]
fn semicircles(degrees: f64) -> i32 {
    (degrees * (f64::from(1u32 << 31) / 180.0)).round() as i32
}

fn file_id(created: typedef::DateTime) -> mesgdef::FileId {
    let mut file_id = mesgdef::FileId::new();
    file_id.r#type = typedef::File::ACTIVITY;
    file_id.manufacturer = typedef::Manufacturer::DEVELOPMENT;
    file_id.product = 0;
    file_id.time_created = created;
    "Torqa".clone_into(&mut file_id.product_name);
    file_id
}

fn timer_event(at: typedef::DateTime, event_type: typedef::EventType) -> mesgdef::Event {
    let mut event = mesgdef::Event::new();
    event.timestamp = at;
    event.event = typedef::Event::TIMER;
    event.event_type = event_type;
    event
}

fn record(at: typedef::DateTime, sample: &Sample) -> mesgdef::Record {
    let mut record = mesgdef::Record::new();
    record.timestamp = at;
    record.position_lat = semicircles(sample.lat);
    record.position_long = semicircles(sample.lon);
    record.set_altitude_scaled(sample.elevation.0);
    record.set_distance_scaled(sample.distance.0);
    record.set_speed_scaled(sample.speed.0);
    record.set_grade_scaled(sample.grade.0);
    if let Some(power) = sample.power {
        record.power = to_u16(power.0);
    }
    if let Some(cadence) = sample.cadence {
        record.cadence = to_u8(cadence.0);
    }
    if let Some(heart_rate) = sample.heart_rate {
        record.heart_rate = to_u8(heart_rate.0);
    }
    record
}

fn lap(start: typedef::DateTime, end: typedef::DateTime, s: &Summary) -> mesgdef::Lap {
    let mut lap = mesgdef::Lap::new();
    lap.message_index = typedef::MessageIndex(0);
    lap.timestamp = end;
    lap.start_time = start;
    lap.event = typedef::Event::LAP;
    lap.event_type = typedef::EventType::STOP;
    lap.lap_trigger = typedef::LapTrigger::SESSION_END;
    lap.sport = typedef::Sport::CYCLING;
    lap.sub_sport = typedef::SubSport::VIRTUAL_ACTIVITY;
    lap.set_total_elapsed_time_scaled(s.elapsed)
        .set_total_timer_time_scaled(s.elapsed)
        .set_total_distance_scaled(s.distance)
        .set_avg_speed_scaled(s.avg_speed)
        .set_max_speed_scaled(s.max_speed);
    lap.total_ascent = to_u16(s.ascent);
    if let Some(v) = s.avg_power {
        lap.avg_power = to_u16(v);
    }
    if let Some(v) = s.max_power {
        lap.max_power = to_u16(v);
    }
    if let Some(v) = s.avg_cadence {
        lap.avg_cadence = to_u8(v);
    }
    if let Some(v) = s.avg_heart_rate {
        lap.avg_heart_rate = to_u8(v);
    }
    if let Some(v) = s.max_heart_rate {
        lap.max_heart_rate = to_u8(v);
    }
    lap
}

fn session(start: typedef::DateTime, end: typedef::DateTime, s: &Summary) -> mesgdef::Session {
    let mut session = mesgdef::Session::new();
    session.message_index = typedef::MessageIndex(0);
    session.timestamp = end;
    session.start_time = start;
    session.event = typedef::Event::SESSION;
    session.event_type = typedef::EventType::STOP;
    session.sport = typedef::Sport::CYCLING;
    session.sub_sport = typedef::SubSport::VIRTUAL_ACTIVITY;
    session.first_lap_index = 0;
    session.num_laps = 1;
    session
        .set_total_elapsed_time_scaled(s.elapsed)
        .set_total_timer_time_scaled(s.elapsed)
        .set_total_distance_scaled(s.distance)
        .set_avg_speed_scaled(s.avg_speed)
        .set_max_speed_scaled(s.max_speed);
    session.total_ascent = to_u16(s.ascent);
    if let Some(v) = s.avg_power {
        session.avg_power = to_u16(v);
    }
    if let Some(v) = s.max_power {
        session.max_power = to_u16(v);
    }
    if let Some(v) = s.avg_cadence {
        session.avg_cadence = to_u8(v);
    }
    if let Some(v) = s.avg_heart_rate {
        session.avg_heart_rate = to_u8(v);
    }
    if let Some(v) = s.max_heart_rate {
        session.max_heart_rate = to_u8(v);
    }
    session
}

fn activity(end: typedef::DateTime, s: &Summary) -> mesgdef::Activity {
    let mut activity = mesgdef::Activity::new();
    activity.timestamp = end;
    activity.num_sessions = 1;
    activity.r#type = typedef::Activity::MANUAL;
    activity.event = typedef::Event::ACTIVITY;
    activity.event_type = typedef::EventType::STOP;
    activity.set_total_timer_time_scaled(s.elapsed);
    activity
}

#[cfg(test)]
mod tests {
    use rustyfit::Decoder;
    use torqa_domain::units::{BeatsPerMinute, GradePercent, Meters, MetersPerSecond, Rpm, Watts};

    use super::*;

    fn samples(count: u32) -> Vec<Sample> {
        (0..count)
            .map(|i| Sample {
                elapsed: Duration::from_secs(u64::from(i)),
                lat: 46.9 + f64::from(i) * 1e-4,
                lon: 7.4,
                elevation: Meters(500.0 + f64::from(i)),
                distance: Meters(f64::from(i) * 10.0),
                speed: MetersPerSecond(10.0),
                grade: GradePercent(1.0),
                power: Some(Watts(200.0 + f64::from(i % 2) * 100.0)),
                cadence: Some(Rpm(90.0)),
                heart_rate: (i > 0).then_some(BeatsPerMinute(140.0)),
            })
            .collect()
    }

    fn decode(bytes: &[u8]) -> FIT {
        let mut reader = FromStd::new(Cursor::new(bytes));
        Decoder::new()
            .decode(&mut reader)
            .expect("valid FIT with correct CRC")
            .expect("one FIT sequence")
    }

    #[test]
    fn exports_a_virtual_ride_that_decodes_again() {
        let start = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        let fit = decode(&encode_fit(start, &samples(61)).unwrap());

        let of = |num| fit.messages.iter().filter(move |m| m.num == num);
        let file_id = mesgdef::FileId::from(of(typedef::MesgNum::FILE_ID).next().unwrap());
        assert_eq!(file_id.r#type, typedef::File::ACTIVITY);
        assert_eq!(of(typedef::MesgNum::RECORD).count(), 61);

        let session = mesgdef::Session::from(of(typedef::MesgNum::SESSION).next().unwrap());
        assert_eq!(session.sport, typedef::Sport::CYCLING);
        assert_eq!(session.sub_sport, typedef::SubSport::VIRTUAL_ACTIVITY);
        assert_eq!(session.start_time.unix_timestamp(), Some(1_790_000_000));
        assert_eq!(session.total_distance, 600 * 100); // centimetres
        assert_eq!(session.total_elapsed_time, 60 * 1000); // milliseconds
        assert_eq!(session.avg_power, 249); // 31 × 200 W + 30 × 300 W over 61 s
        assert_eq!(session.max_power, 300);
        assert_eq!(session.avg_heart_rate, 140);
        assert_eq!(session.total_ascent, 60);
        assert_eq!(of(typedef::MesgNum::ACTIVITY).count(), 1);
    }

    #[test]
    fn records_carry_position_and_sensor_data() {
        let fit = decode(&encode_fit(UNIX_EPOCH, &samples(2)).unwrap());
        let record = fit
            .messages
            .iter()
            .filter(|m| m.num == typedef::MesgNum::RECORD)
            .map(mesgdef::Record::from)
            .nth(1)
            .unwrap();

        assert_eq!(record.power, 300);
        assert_eq!(record.cadence, 90);
        assert_eq!(record.heart_rate, 140);
        assert_eq!(record.position_lat, semicircles(46.9001));
        assert_eq!(record.speed, 10_000); // mm/s
    }

    #[test]
    fn empty_ride_is_an_error() {
        assert!(matches!(encode_fit(UNIX_EPOCH, &[]), Err(FitError::Empty)));
    }
}
