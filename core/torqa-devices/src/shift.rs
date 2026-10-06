//! Shift inputs (R7) behind [`ShiftInput`]: the keyboard here, whose keys the front end passes
//! on; Bluetooth controllers follow.

use std::sync::mpsc;

use torqa_domain::shifting::{Shift, ShiftInput};

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
    fn keys_pressed_come_out_in_order_once() {
        let (keys, mut keyboard) = keyboard();

        keys.press(Shift::Up);
        keys.press(Shift::Up);
        keys.press(Shift::Down);

        assert_eq!(keyboard.poll(), [Shift::Up, Shift::Up, Shift::Down]);
        assert_eq!(keyboard.poll(), []);
    }
}
