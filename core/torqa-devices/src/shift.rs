//! Shift inputs (R7) behind [`ShiftInput`]: the keyboard, whose keys the front end passes on,
//! and the D-Fly buttons of a Shimano Di2 shifter, which do what the rider gave them (#139).

use std::sync::{Arc, Mutex, PoisonError, mpsc};

use torqa_domain::shifting::{ButtonAction, ButtonMap, Control, Shift, ShiftInput};

use crate::{DeviceEvent, DeviceHandle};

/// What a Di2 shifter's buttons do, shared with the settings, so a change applies at once.
#[derive(Debug, Default)]
pub struct Assignments(Mutex<ButtonMap>);

impl Assignments {
    /// The buttons doing what `map` gives them.
    #[must_use]
    pub fn new(map: ButtonMap) -> Self {
        Self(Mutex::new(map))
    }

    /// Gives the buttons what `map` says.
    pub fn set(&self, map: ButtonMap) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = map;
    }

    /// What the buttons do.
    #[must_use]
    pub fn get(&self) -> ButtonMap {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Shifts, and the other controls its buttons were given, from a Di2 shifter connected with
/// [`crate::ble::Bluetooth::connect`].
#[derive(Debug)]
pub struct Controller {
    handle: DeviceHandle,
    assignments: Arc<Assignments>,
    connected: bool,
    /// Controls pressed for, until [`ShiftInput::controls`] takes them.
    controls: Vec<Control>,
}

impl Controller {
    /// Shifts from the shifter behind `handle`, its buttons doing what `assignments` say.
    #[must_use]
    pub fn new(handle: DeviceHandle, assignments: Arc<Assignments>) -> Self {
        Self {
            handle,
            assignments,
            connected: false,
            controls: Vec::new(),
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
                DeviceEvent::Buttons(presses) => {
                    for action in self.assignments.get().actions(&presses) {
                        shifts.extend_from_slice(action.shifts());
                        if let ButtonAction::Control(control) = action {
                            self.controls.push(control);
                        }
                    }
                }
                DeviceEvent::Telemetry(_) => {}
            }
        }
        shifts
    }

    fn controls(&mut self) -> Vec<Control> {
        std::mem::take(&mut self.controls)
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
    use torqa_domain::shifting::{ButtonPress, Press};

    use super::*;

    #[tokio::test]
    async fn presses_shift_or_ask_for_the_controls_their_buttons_were_given() {
        let mut map = ButtonMap::shifting(2, 1);
        map.assign(
            3,
            Press::Short,
            Some(ButtonAction::Control(Control::NextCamera)),
        );
        let press = |channel, press| ButtonPress { channel, press };
        let presses = vec![
            press(2, Press::Short),
            press(3, Press::Short),
            press(1, Press::Double),
            press(4, Press::Short),
        ];
        let handle = DeviceHandle::spawn("RDR8150".to_owned(), false, |channels| async move {
            let _ = channels.events.send(DeviceEvent::Connected).await;
            let _ = channels.events.send(DeviceEvent::Buttons(presses)).await;
        });
        let mut controller = Controller::new(handle, Arc::new(Assignments::new(map)));
        tokio::task::yield_now().await;

        let shifts = controller.poll();

        assert_eq!(shifts, [Shift::Up, Shift::Down, Shift::Down]);
        assert_eq!(controller.controls(), [Control::NextCamera]);
        assert_eq!(controller.controls(), [], "taken once");
        assert!(controller.connected());
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
