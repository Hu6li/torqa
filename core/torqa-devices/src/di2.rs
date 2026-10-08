//! Shimano Di2 over Bluetooth (12-speed Di2, or the EW-WU111 wireless unit): the D-Fly
//! channels. In E-TUBE the hood buttons (or any switch) are assigned to channels 1–4; the
//! unit indicates each press, which Torqa turns into shifts and other controls (R7, #139).
//!
//! The service and its button characteristic sit in Shimano's own UUID base, which spells
//! `SHIMANO_BLE`. The unit must be paired before it sends the presses.

use torqa_domain::shifting::{ButtonPress, Press};

use crate::bytes::ParseError;

/// The D-Fly service.
pub const SERVICE: &str = "000018ef-5348-494d-414e-4f5f424c4500";
/// The button characteristic (indicate), as its short form leads the full UUID.
pub const BUTTONS_PREFIX: &str = "00002ac2-";
/// Shimano's Bluetooth company identifier, in the advertising data of most units.
pub const MANUFACTURER: u16 = 0x044A;

const CHANNELS: usize = 4;
/// A channel not assigned to any button in E-TUBE.
const UNASSIGNED: u8 = 0xF0;
const SHORT: u8 = 0x10;
const LONG: u8 = 0x20;
const DOUBLE: u8 = 0x40;

/// Turns the unit's button indications into presses, each once.
///
/// An indication carries a counter and a byte per channel. A channel's byte keeps its last
/// press until the next press changes it (only a long press is released), so every indication
/// still shows the other channels' earlier presses: a press is a channel whose byte changed.
#[derive(Debug, Clone, Default)]
pub struct Buttons {
    /// Each channel's byte in the last indication; unknown before the first.
    last: Option<[u8; CHANNELS]>,
    /// Channels held down after a long press, until let go.
    held: [bool; CHANNELS],
}

impl Buttons {
    /// Starts from where the channels stand, as read from the unit, so that the first press
    /// indicated counts too.
    ///
    /// # Errors
    /// [`ParseError`] if the value is shorter than a counter and four channels.
    pub fn start_from(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.last = Some(channels(data)?);
        self.held = [false; CHANNELS];
        Ok(())
    }

    /// The presses one indication tells of. Without a [`Self::start_from`], the first one
    /// only shows where the channels stand.
    ///
    /// # Errors
    /// [`ParseError`] if it is shorter than a counter and four channels.
    pub fn presses(&mut self, data: &[u8]) -> Result<Vec<ButtonPress>, ParseError> {
        let now = channels(data)?;
        let Some(before) = self.last.replace(now) else {
            return Ok(Vec::new());
        };
        let mut presses = Vec::new();
        for (((&state, &was), held), channel) in
            now.iter().zip(&before).zip(&mut self.held).zip(1..)
        {
            if state == was || state == UNASSIGNED {
                continue;
            }
            let press = if state & DOUBLE != 0 {
                Press::Double
            } else if state & LONG != 0 {
                Press::Long
            } else if state & SHORT != 0 {
                Press::Short
            } else {
                *held = false;
                continue;
            };
            // Held after a long press, the channel reads as a short one until let go.
            if press == Press::Short && *held {
                continue;
            }
            *held = press == Press::Long;
            presses.push(ButtonPress { channel, press });
        }
        Ok(presses)
    }
}

/// The channels' bytes, after the counter.
fn channels(data: &[u8]) -> Result<[u8; CHANNELS], ParseError> {
    data.get(1..=CHANNELS)
        .and_then(|channels| channels.try_into().ok())
        .ok_or(ParseError::TooShort {
            needed: CHANNELS + 1,
            actual: data.len(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(channel: u8, press: Press) -> ButtonPress {
        ButtonPress { channel, press }
    }

    #[test]
    fn earlier_presses_on_other_channels_do_not_press_again() {
        let mut buttons = Buttons::default();
        buttons.start_from(&[0, 0x00, 0x00, 0xF0, 0xF0]).unwrap();

        // Channel 2 pressed, then channel 1, while channel 2 still shows its press.
        let second = buttons.presses(&[1, 0x00, 0x10, 0xF0, 0xF0]).unwrap();
        let first = buttons.presses(&[2, 0x10, 0x10, 0xF0, 0xF0]).unwrap();
        let second_again = buttons.presses(&[3, 0x10, 0x11, 0xF0, 0xF0]).unwrap();
        let second_double = buttons.presses(&[4, 0x10, 0x40, 0xF0, 0xF0]).unwrap();
        let repeated = buttons.presses(&[4, 0x10, 0x40, 0xF0, 0xF0]).unwrap();

        assert_eq!(second, [press(2, Press::Short)]);
        assert_eq!(first, [press(1, Press::Short)]);
        assert_eq!(second_again, [press(2, Press::Short)]);
        assert_eq!(second_double, [press(2, Press::Double)]);
        assert_eq!(repeated, [], "the same indication twice is one press");
    }

    #[test]
    fn the_first_indication_only_shows_where_the_channels_stand() {
        let mut buttons = Buttons::default();

        let unknown = buttons.presses(&[7, 0x10, 0x40, 0x00, 0xF0]).unwrap();
        let pressed = buttons.presses(&[8, 0x11, 0x40, 0x00, 0xF0]).unwrap();

        assert_eq!(unknown, [], "which of them is new cannot be told");
        assert_eq!(pressed, [press(1, Press::Short)]);
    }

    #[test]
    fn a_long_press_counts_once_however_long_it_is_held() {
        let mut buttons = Buttons::default();
        buttons.start_from(&[0, 0x00, 0x00, 0x00, 0x00]).unwrap();

        let held = buttons.presses(&[1, 0x00, 0x20, 0x00, 0x00]).unwrap();
        let still_held = buttons.presses(&[2, 0x00, 0x10, 0x00, 0x00]).unwrap();
        let let_go = buttons.presses(&[3, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let pressed = buttons.presses(&[4, 0x00, 0x10, 0x00, 0x00]).unwrap();

        assert_eq!(held, [press(2, Press::Long)]);
        assert_eq!(still_held, []);
        assert_eq!(let_go, []);
        assert_eq!(
            pressed,
            [press(2, Press::Short)],
            "a new press after letting go"
        );
    }

    #[test]
    fn channels_pressed_together_both_count() {
        let mut buttons = Buttons::default();
        buttons.start_from(&[0, 0x00, 0x00, 0x00, 0x00]).unwrap();

        let both = buttons.presses(&[1, 0x10, 0x00, 0x40, 0x00]).unwrap();

        assert_eq!(both, [press(1, Press::Short), press(3, Press::Double)]);
    }

    #[test]
    fn short_indications_are_errors() {
        assert!(Buttons::default().presses(&[]).is_err());
        assert!(Buttons::default().presses(&[1, 0x10, 0x00]).is_err());
        assert!(
            Buttons::default()
                .start_from(&[1, 0x10, 0x00, 0x00])
                .is_err()
        );
    }
}
