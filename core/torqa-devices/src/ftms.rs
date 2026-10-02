//! Bluetooth Fitness Machine Service (FTMS) for indoor bikes: decoding Indoor Bike Data and
//! encoding Fitness Machine Control Point commands (FTMS v1.0).

use torqa_domain::telemetry::{Telemetry, TrainerControl};
use torqa_domain::units::{BeatsPerMinute, MetersPerSecond, Percent, Rpm, Watts};

use crate::bytes::{ParseError, Reader};

/// Fitness Machine service.
pub const SERVICE: u16 = 0x1826;
/// Indoor Bike Data characteristic (notify).
pub const INDOOR_BIKE_DATA: u16 = 0x2AD2;
/// Supported Resistance Level Range characteristic (read).
pub const SUPPORTED_RESISTANCE_LEVEL_RANGE: u16 = 0x2AD6;
/// Fitness Machine Control Point characteristic (write, indicate).
pub const CONTROL_POINT: u16 = 0x2AD9;

const OP_REQUEST_CONTROL: u8 = 0x00;
const OP_SET_TARGET_RESISTANCE: u8 = 0x04;
const OP_SET_TARGET_POWER: u8 = 0x05;
const OP_START_OR_RESUME: u8 = 0x07;
const OP_SET_INDOOR_BIKE_SIMULATION: u8 = 0x11;
const OP_RESPONSE: u8 = 0x80;

/// Largest resistance level the uint8 control point parameter (0.1 resolution) can express.
///
/// FTMS v1.0 is inconsistent here: the supported range is a sint16 and may exceed this.
const MAX_ENCODABLE_RESISTANCE: f64 = 25.5;

/// Decodes an Indoor Bike Data notification.
///
/// # Errors
/// Returns [`ParseError`] if the value is shorter than its flags announce.
pub fn parse_indoor_bike_data(data: &[u8]) -> Result<Telemetry, ParseError> {
    let mut r = Reader::new(data);
    let flags = r.u16()?;
    let has = |bit: u16| flags & (1 << bit) != 0;
    let mut telemetry = Telemetry::default();

    // Bit 0 ("More Data") is inverted: instantaneous speed is present when it is clear.
    if !has(0) {
        let centi_kmh = r.u16()?;
        telemetry.speed = Some(MetersPerSecond::from_kilometers_per_hour(
            f64::from(centi_kmh) * 0.01,
        ));
    }
    if has(1) {
        r.skip(2)?; // average speed
    }
    if has(2) {
        telemetry.cadence = Some(Rpm(f64::from(r.u16()?) * 0.5));
    }
    if has(3) {
        r.skip(2)?; // average cadence
    }
    if has(4) {
        r.skip(3)?; // total distance
    }
    if has(5) {
        r.skip(2)?; // resistance level
    }
    if has(6) {
        telemetry.power = Some(Watts(f64::from(r.i16()?)));
    }
    if has(7) {
        r.skip(2)?; // average power
    }
    if has(8) {
        r.skip(5)?; // expended energy: total, per hour, per minute
    }
    if has(9) {
        // Trainers without a paired strap report 0 rather than omitting the field.
        let bpm = r.u8()?;
        telemetry.heart_rate = (bpm > 0).then(|| BeatsPerMinute(f64::from(bpm)));
    }
    // Remaining fields (metabolic equivalent, elapsed and remaining time) are not used.
    Ok(telemetry)
}

/// The resistance levels a trainer supports, in its own unitless scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResistanceRange {
    /// Lowest supported level.
    pub min: f64,
    /// Highest supported level.
    pub max: f64,
}

impl Default for ResistanceRange {
    /// The full range the control point can express, for trainers that do not report theirs.
    fn default() -> Self {
        Self {
            min: 0.0,
            max: MAX_ENCODABLE_RESISTANCE,
        }
    }
}

impl ResistanceRange {
    /// Decodes the Supported Resistance Level Range characteristic.
    ///
    /// # Errors
    /// Returns [`ParseError`] if the value is too short.
    pub fn parse(data: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(data);
        let min = f64::from(r.i16()?) * 0.1;
        let max = f64::from(r.i16()?) * 0.1;
        Ok(Self { min, max })
    }

    /// Maps a share of the range onto a level that the control point can express.
    ///
    /// The top of the range is capped at what the uint8 parameter can encode, so 0–100 %
    /// always spans usable levels instead of saturating early on trainers that advertise more.
    fn level(self, share: Percent) -> f64 {
        let max = self.max.min(MAX_ENCODABLE_RESISTANCE);
        let min = self.min.clamp(0.0, max);
        min + (max - min) * share.0.clamp(0.0, 100.0) / 100.0
    }
}

/// Command that must be acknowledged before the trainer accepts any other control.
#[must_use]
pub fn request_control() -> Vec<u8> {
    vec![OP_REQUEST_CONTROL]
}

/// Command that starts or resumes the training session on the trainer.
#[must_use]
pub fn start_or_resume() -> Vec<u8> {
    vec![OP_START_OR_RESUME]
}

/// Encodes a resistance control as a control point command.
#[must_use]
pub fn encode_control(control: &TrainerControl, range: ResistanceRange) -> Vec<u8> {
    match control {
        TrainerControl::Simulation(p) => {
            let mut command = vec![OP_SET_INDOOR_BIKE_SIMULATION];
            command.extend(scaled_i16(p.wind_speed.0, 0.001).to_le_bytes());
            command.extend(scaled_i16(p.grade.0, 0.01).to_le_bytes());
            command.push(scaled_u8(p.crr, 0.0001));
            command.push(scaled_u8(p.cw.0, 0.01));
            command
        }
        TrainerControl::TargetPower(power) => {
            let mut command = vec![OP_SET_TARGET_POWER];
            command.extend(scaled_i16(power.0, 1.0).to_le_bytes());
            command
        }
        TrainerControl::Resistance(share) => {
            vec![
                OP_SET_TARGET_RESISTANCE,
                scaled_u8(range.level(*share), 0.1),
            ]
        }
    }
}

/// Outcome reported by the trainer for a control point command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlResult {
    /// The command was executed.
    Success,
    /// The trainer does not support the command.
    NotSupported,
    /// A parameter was out of range.
    InvalidParameter,
    /// The trainer failed to execute the command.
    Failed,
    /// Control was not granted (missing or lost Request Control).
    ControlNotPermitted,
    /// A result code not defined by FTMS v1.0.
    Unknown(u8),
}

/// A control point indication answering a previous command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlResponse {
    /// Op code of the command being answered.
    pub request: u8,
    /// Outcome of that command.
    pub result: ControlResult,
}

/// Decodes a control point indication; `None` if it is not a response.
#[must_use]
pub fn parse_control_response(data: &[u8]) -> Option<ControlResponse> {
    match *data {
        [OP_RESPONSE, request, code, ..] => Some(ControlResponse {
            request,
            result: match code {
                0x01 => ControlResult::Success,
                0x02 => ControlResult::NotSupported,
                0x03 => ControlResult::InvalidParameter,
                0x04 => ControlResult::Failed,
                0x05 => ControlResult::ControlNotPermitted,
                other => ControlResult::Unknown(other),
            },
        }),
        _ => None,
    }
}

// Values are clamped to the field's range first, so the casts cannot truncate.
#[allow(clippy::cast_possible_truncation)]
fn scaled_i16(value: f64, resolution: f64) -> i16 {
    (value / resolution)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scaled_u8(value: f64, resolution: f64) -> u8 {
    (value / resolution).round().clamp(0.0, f64::from(u8::MAX)) as u8
}

#[cfg(test)]
mod tests {
    use torqa_domain::telemetry::SimulationParameters;
    use torqa_domain::units::{GradePercent, KilogramsPerMeter};

    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn decodes_speed_cadence_power_and_heart_rate() {
        // Flags: cadence (bit 2), power (bit 6), heart rate (bit 9); speed present (bit 0 clear).
        let flags: u16 = (1 << 2) | (1 << 6) | (1 << 9);
        let mut data = flags.to_le_bytes().to_vec();
        data.extend(3250u16.to_le_bytes()); // 32.50 km/h
        data.extend(180u16.to_le_bytes()); // 90 rpm
        data.extend(250i16.to_le_bytes()); // 250 W
        data.push(142); // 142 bpm

        let t = parse_indoor_bike_data(&data).unwrap();

        assert_close(t.speed.unwrap().as_kilometers_per_hour(), 32.5);
        assert_eq!(t.cadence, Some(Rpm(90.0)));
        assert_eq!(t.power, Some(Watts(250.0)));
        assert_eq!(t.heart_rate, Some(BeatsPerMinute(142.0)));
    }

    #[test]
    fn skips_unused_fields_to_reach_power() {
        // Average speed, average cadence, distance, resistance precede power.
        let flags: u16 = 1 | (1 << 1) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6);
        let mut data = flags.to_le_bytes().to_vec();
        data.extend([0u8; 2 + 2 + 3 + 2]);
        data.extend((-5i16).to_le_bytes());

        let t = parse_indoor_bike_data(&data).unwrap();

        assert_eq!(t.speed, None, "speed absent when More Data is set");
        assert_eq!(t.power, Some(Watts(-5.0)));
    }

    #[test]
    fn heart_rate_zero_means_no_strap() {
        let flags: u16 = 1 | (1 << 9);
        let mut data = flags.to_le_bytes().to_vec();
        data.push(0);

        assert_eq!(parse_indoor_bike_data(&data).unwrap().heart_rate, None);
    }

    #[test]
    fn truncated_data_is_an_error() {
        let flags: u16 = 1 << 6;
        let data = flags.to_le_bytes().to_vec(); // speed and power announced but missing

        assert!(parse_indoor_bike_data(&data).is_err());
    }

    #[test]
    fn encodes_simulation_parameters() {
        let control = TrainerControl::Simulation(SimulationParameters {
            grade: GradePercent(-2.5),
            wind_speed: MetersPerSecond(1.5),
            crr: 0.004,
            cw: KilogramsPerMeter(0.51),
        });

        let command = encode_control(&control, ResistanceRange::default());

        let mut expected = vec![0x11];
        expected.extend(1500i16.to_le_bytes());
        expected.extend((-250i16).to_le_bytes());
        expected.extend([40, 51]);
        assert_eq!(command, expected);
    }

    #[test]
    fn encodes_target_power() {
        let command = encode_control(
            &TrainerControl::TargetPower(Watts(275.0)),
            ResistanceRange::default(),
        );

        assert_eq!(command, [0x05, 19, 1]); // 275 = 0x0113
    }

    #[test]
    fn resistance_share_spans_the_supported_range() {
        let range = ResistanceRange {
            min: 1.0,
            max: 21.0,
        };

        let half = encode_control(&TrainerControl::Resistance(Percent(50.0)), range);
        let full = encode_control(&TrainerControl::Resistance(Percent(100.0)), range);

        assert_eq!(half, [0x04, 110]); // level 11.0
        assert_eq!(full, [0x04, 210]); // level 21.0
    }

    #[test]
    fn resistance_range_beyond_encodable_levels_is_capped() {
        // A trainer advertising 0–100 must still get distinct levels across 0–100 %.
        let range = ResistanceRange::parse(&[0, 0, 0xE8, 0x03, 10, 0]).unwrap();

        let half = encode_control(&TrainerControl::Resistance(Percent(50.0)), range);
        let full = encode_control(&TrainerControl::Resistance(Percent(100.0)), range);

        assert_eq!(half, [0x04, 128]);
        assert_eq!(full, [0x04, 255]);
    }

    #[test]
    fn decodes_control_responses() {
        assert_eq!(
            parse_control_response(&[0x80, 0x11, 0x01]),
            Some(ControlResponse {
                request: 0x11,
                result: ControlResult::Success
            })
        );
        assert_eq!(
            parse_control_response(&[0x80, 0x05, 0x05]).map(|r| r.result),
            Some(ControlResult::ControlNotPermitted)
        );
        assert_eq!(parse_control_response(&[0x11, 0x00]), None);
    }
}
