//! Shimano Di2 over Bluetooth (12-speed Di2, or the EW-WU111 wireless unit): the D-Fly
//! channels. In E-TUBE the hood buttons (or any switch) are assigned to channels 1–4; the
//! unit indicates each press, which Torqa can use to shift the virtual gears (R7).
//!
//! The service and its button characteristic sit in Shimano's own UUID base, which spells
//! `SHIMANO_BLE`. The unit must be paired before it sends the presses.

use crate::bytes::ParseError;

/// The D-Fly service.
pub const SERVICE: &str = "000018ef-5348-494d-414e-4f5f424c4500";
/// The button characteristic (indicate), as its short form leads the full UUID.
pub const BUTTONS_PREFIX: &str = "00002ac2-";
/// Shimano's Bluetooth company identifier, in the advertising data of most units.
pub const MANUFACTURER: u16 = 0x044A;

/// A channel not assigned to any button in E-TUBE.
const UNASSIGNED: u8 = 0xF0;
const SHORT: u8 = 0x10;
const LONG: u8 = 0x20;
const DOUBLE: u8 = 0x40;

/// How a button was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Pressed once.
    Short,
    /// Held; the unit repeats it while the button stays down.
    Long,
    /// Pressed twice quickly.
    Double,
}

/// A press of a button assigned to a D-Fly channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonPress {
    /// The D-Fly channel, 1–4.
    pub channel: u8,
    /// How it was pressed.
    pub press: Press,
}

/// Turns the unit's button indications into presses, each once: an indication carries a
/// counter and the state of the four channels, and a release (no flags) presses nothing.
#[derive(Debug, Clone, Default)]
pub struct Buttons {
    counter: Option<u8>,
}

impl Buttons {
    /// The presses one indication tells of; none if it repeats the last one.
    ///
    /// # Errors
    /// [`ParseError`] if it is shorter than a counter and four channels.
    pub fn presses(&mut self, data: &[u8]) -> Result<Vec<ButtonPress>, ParseError> {
        let [counter, channels @ ..] = data else {
            return Err(ParseError::TooShort {
                needed: 5,
                actual: 0,
            });
        };
        if channels.len() < 4 {
            return Err(ParseError::TooShort {
                needed: 5,
                actual: data.len(),
            });
        }
        if self.counter.replace(*counter) == Some(*counter) {
            return Ok(Vec::new());
        }
        Ok(channels[..4]
            .iter()
            .zip(1..)
            .filter(|&(&state, _)| state != UNASSIGNED)
            .filter_map(|(&state, channel)| {
                let press = if state & DOUBLE != 0 {
                    Press::Double
                } else if state & LONG != 0 {
                    Press::Long
                } else if state & SHORT != 0 {
                    Press::Short
                } else {
                    return None;
                };
                Some(ButtonPress { channel, press })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(channel: u8, press: Press) -> ButtonPress {
        ButtonPress { channel, press }
    }

    #[test]
    fn presses_come_once_and_releases_press_nothing() {
        let mut buttons = Buttons::default();

        // Channel 1 pressed (2 and 4 unassigned), then released.
        let pressed = buttons.presses(&[7, 0x10, 0xF0, 0x00, 0xF0]).unwrap();
        let again = buttons.presses(&[7, 0x10, 0xF0, 0x00, 0xF0]).unwrap();
        let released = buttons.presses(&[8, 0x00, 0xF0, 0x00, 0xF0]).unwrap();

        assert_eq!(pressed, [press(1, Press::Short)]);
        assert_eq!(again, [], "the same indication twice is one press");
        assert_eq!(released, []);
    }

    #[test]
    fn long_and_double_presses_on_any_channel() {
        let mut buttons = Buttons::default();

        let held = buttons.presses(&[1, 0x00, 0x20, 0x00, 0x00]).unwrap();
        let still_held = buttons.presses(&[2, 0x00, 0x20, 0x00, 0x00]).unwrap();
        let double = buttons.presses(&[3, 0x00, 0x00, 0x00, 0x40]).unwrap();
        let both = buttons.presses(&[4, 0x10, 0x00, 0x10, 0x00]).unwrap();

        assert_eq!(held, [press(2, Press::Long)]);
        assert_eq!(still_held, [press(2, Press::Long)], "held: repeated");
        assert_eq!(double, [press(4, Press::Double)]);
        assert_eq!(both, [press(1, Press::Short), press(3, Press::Short)]);
    }

    #[test]
    fn short_indications_are_errors() {
        assert!(Buttons::default().presses(&[]).is_err());
        assert!(Buttons::default().presses(&[1, 0x10, 0x00]).is_err());
    }
}
