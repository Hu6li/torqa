//! Shift inputs (R7) behind [`ShiftInput`]: the keyboard, whose keys the front end passes on,
//! and the D-Fly buttons of a Shimano Di2 shifter.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};

use torqa_domain::shifting::{Shift, ShiftInput};

use crate::di2::{ButtonPress, Press};
use crate::{DeviceEvent, DeviceHandle};

/// Which D-Fly channels shift up and down, as the rider assigned their buttons in E-TUBE;
/// shared with the settings, so a change applies at once.
#[derive(Debug)]
pub struct Channels {
    up: AtomicU8,
    down: AtomicU8,
}

impl Channels {
    /// Channel `up` shifts up (harder), `down` down (easier), each 1–4.
    #[must_use]
    pub fn new(up: u8, down: u8) -> Self {
        Self {
            up: AtomicU8::new(up),
            down: AtomicU8::new(down),
        }
    }

    /// Assigns the channels anew.
    pub fn set(&self, up: u8, down: u8) {
        self.up.store(up, Ordering::Relaxed);
        self.down.store(down, Ordering::Relaxed);
    }

    /// The channels shifting up and down.
    #[must_use]
    pub fn get(&self) -> (u8, u8) {
        (
            self.up.load(Ordering::Relaxed),
            self.down.load(Ordering::Relaxed),
        )
    }

    /// The shifts `presses` ask for: a short or long press one gear, a double press two; other
    /// channels shift nothing.
    #[must_use]
    pub fn shifts(&self, presses: &[ButtonPress]) -> Vec<Shift> {
        let (up, down) = self.get();
        presses
            .iter()
            .flat_map(|press| {
                let shift = if press.channel == up {
                    Some(Shift::Up)
                } else if press.channel == down {
                    Some(Shift::Down)
                } else {
                    None
                };
                let times = if press.press == Press::Double { 2 } else { 1 };
                std::iter::repeat_n(shift, times).flatten()
            })
            .collect()
    }
}

/// Shifts from a Di2 shifter connected with [`crate::ble::Bluetooth::connect`].
#[derive(Debug)]
pub struct Controller {
    handle: DeviceHandle,
    channels: Arc<Channels>,
    connected: bool,
}

impl Controller {
    /// Shifts from the shifter behind `handle`, with its buttons on `channels`.
    #[must_use]
    pub fn new(handle: DeviceHandle, channels: Arc<Channels>) -> Self {
        Self {
            handle,
            channels,
            connected: false,
        }
    }
}

impl ShiftInput for Controller {
    fn name(&self) -> &str {
        self.handle.name()
    }

    fn poll(&mut self) -> Vec<Shift> {
        let mut shifts = Vec::new();
        while let Ok(Some(event)) = self.handle.try_next_event() {
            match event {
                DeviceEvent::Connected => self.connected = true,
                DeviceEvent::Disconnected => self.connected = false,
                DeviceEvent::Buttons(presses) => shifts.extend(self.channels.shifts(&presses)),
                DeviceEvent::Telemetry(_) => {}
            }
        }
        shifts
    }

    fn connected(&self) -> bool {
        self.connected
    }
}

/// Shifts from the keyboard, as passed on through its [`KeyboardKeys`].
#[derive(Debug)]
pub struct Keyboard {
    shifts: mpsc::Receiver<Shift>,
}

/// Passes key presses on to a [`Keyboard`].
#[derive(Debug, Clone)]
pub struct KeyboardKeys {
    shifts: mpsc::Sender<Shift>,
}

/// A keyboard shift input and the handle the front end presses its keys through.
#[must_use]
pub fn keyboard() -> (KeyboardKeys, Keyboard) {
    let (shifts, received) = mpsc::channel();
    (KeyboardKeys { shifts }, Keyboard { shifts: received })
}

impl KeyboardKeys {
    /// A shift key was pressed.
    pub fn press(&self, shift: Shift) {
        // Only fails once the keyboard input is gone, when no one shifts any more.
        let _ = self.shifts.send(shift);
    }
}

impl ShiftInput for Keyboard {
    fn name(&self) -> &'static str {
        "Keyboard"
    }

    fn poll(&mut self) -> Vec<Shift> {
        self.shifts.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_assigned_channels_shift_up_and_down() {
        let channels = Channels::new(2, 1);
        let press = |channel, press| ButtonPress { channel, press };

        let shifts = channels.shifts(&[
            press(2, Press::Short),
            press(1, Press::Long),
            press(3, Press::Short),
            press(2, Press::Double),
        ]);

        assert_eq!(shifts, [Shift::Up, Shift::Down, Shift::Up, Shift::Up]);
        channels.set(3, 4);
        assert_eq!(channels.shifts(&[press(3, Press::Short)]), [Shift::Up]);
    }

    #[test]
    fn keys_pressed_come_out_in_order_once() {
        let (keys, mut keyboard) = keyboard();

        keys.press(Shift::Up);
        keys.press(Shift::Up);
        keys.press(Shift::Down);

        assert_eq!(keyboard.poll(), [Shift::Up, Shift::Up, Shift::Down]);
        assert_eq!(keyboard.poll(), []);
    }
}
