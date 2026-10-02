//! Bluetooth Heart Rate Service.

use torqa_domain::units::BeatsPerMinute;

use crate::bytes::{ParseError, Reader};

/// Heart Rate service.
pub const SERVICE: u16 = 0x180D;
/// Heart Rate Measurement characteristic (notify).
pub const MEASUREMENT: u16 = 0x2A37;

/// Decodes a Heart Rate Measurement; `None` while the strap reports no skin contact.
///
/// # Errors
/// Returns [`ParseError`] if the value is too short.
pub fn parse_measurement(data: &[u8]) -> Result<Option<BeatsPerMinute>, ParseError> {
    let mut r = Reader::new(data);
    let flags = r.u8()?;
    let wide_value = flags & 0b001 != 0;
    let contact_detected = flags & 0b010 != 0;
    let contact_supported = flags & 0b100 != 0;

    let bpm = if wide_value {
        r.u16()?
    } else {
        u16::from(r.u8()?)
    };
    if (contact_supported && !contact_detected) || bpm == 0 {
        return Ok(None);
    }
    Ok(Some(BeatsPerMinute(f64::from(bpm))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_8_bit_value() {
        assert_eq!(
            parse_measurement(&[0x00, 72]).unwrap(),
            Some(BeatsPerMinute(72.0))
        );
    }

    #[test]
    fn decodes_16_bit_value() {
        assert_eq!(
            parse_measurement(&[0x01, 0x2C, 0x01]).unwrap(),
            Some(BeatsPerMinute(300.0))
        );
    }

    #[test]
    fn no_contact_yields_no_value() {
        // Contact supported (bit 2) but not detected (bit 1 clear).
        assert_eq!(parse_measurement(&[0b100, 80]).unwrap(), None);
        assert_eq!(
            parse_measurement(&[0b110, 80]).unwrap(),
            Some(BeatsPerMinute(80.0))
        );
    }

    #[test]
    fn truncated_value_is_an_error() {
        assert!(parse_measurement(&[0x01, 0x2C]).is_err());
    }
}
