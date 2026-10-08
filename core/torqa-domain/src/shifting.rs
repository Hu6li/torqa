//! Shifting (R7, R9): inputs that shift the virtual gears, behind the [`ShiftInput`] plugin
//! interface — the keyboard, Shimano Di2 buttons and, later, others — and what the rider gave
//! each button to do (#139).

/// What a shift input asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    /// One gear harder.
    Up,
    /// One gear easier.
    Down,
    /// Straight to a gear, counted from 1.
    To(usize),
}

/// A ride control besides shifting that a button can be given (#139); the front end carries
/// it out as it does the control's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// The next camera.
    NextCamera,
    /// Into the overlay, or back out of it.
    Overlay,
    /// Plays or pauses the rider's music.
    PlayPause,
    /// The next track.
    NextTrack,
    /// The previous track.
    PreviousTrack,
}

/// What a button press does (#139).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    /// One gear harder.
    ShiftUp,
    /// One gear easier.
    ShiftDown,
    /// Two gears harder.
    ShiftUpTwo,
    /// Two gears easier.
    ShiftDownTwo,
    /// A control besides shifting.
    Control(Control),
}

impl ButtonAction {
    /// Every action a button can be given, in the order to offer them.
    pub const ALL: [Self; 9] = [
        Self::ShiftUp,
        Self::ShiftDown,
        Self::ShiftUpTwo,
        Self::ShiftDownTwo,
        Self::Control(Control::NextCamera),
        Self::Control(Control::Overlay),
        Self::Control(Control::PlayPause),
        Self::Control(Control::NextTrack),
        Self::Control(Control::PreviousTrack),
    ];

    /// The shifts it asks for; none for a control.
    #[must_use]
    pub fn shifts(self) -> &'static [Shift] {
        match self {
            Self::ShiftUp => &[Shift::Up],
            Self::ShiftDown => &[Shift::Down],
            Self::ShiftUpTwo => &[Shift::Up, Shift::Up],
            Self::ShiftDownTwo => &[Shift::Down, Shift::Down],
            Self::Control(_) => &[],
        }
    }
}

const PRESSES: usize = 3;

/// How a button was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Pressed once.
    Short,
    /// Held down; one press however long.
    Long,
    /// Pressed twice quickly.
    Double,
}

impl Press {
    /// Every kind of press, in the order to offer them.
    pub const ALL: [Self; PRESSES] = [Self::Short, Self::Long, Self::Double];

    fn index(self) -> usize {
        match self {
            Self::Short => 0,
            Self::Long => 1,
            Self::Double => 2,
        }
    }
}

/// A press of a controller's button, e.g. one assigned to a Di2 D-Fly channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonPress {
    /// The button (for a Di2, the D-Fly channel), from 1 to [`BUTTONS`].
    pub channel: u8,
    /// How it was pressed.
    pub press: Press,
}

/// The buttons a controller can have assigned, as a Di2 has four D-Fly channels.
pub const BUTTONS: u8 = 4;

/// What each press of each button does (#139).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonMap {
    /// By button (from 0 here), then by [`Press::index`].
    actions: [[Option<ButtonAction>; PRESSES]; BUTTONS as usize],
}

impl ButtonMap {
    /// No button does anything.
    pub const NONE: Self = Self {
        actions: [[None; PRESSES]; BUTTONS as usize],
    };

    /// Button `up` shifts up and `down` down: a press or a hold one gear, a double press two.
    #[must_use]
    pub fn shifting(up: u8, down: u8) -> Self {
        let mut map = Self::NONE;
        for (button, one, two) in [
            (up, ButtonAction::ShiftUp, ButtonAction::ShiftUpTwo),
            (down, ButtonAction::ShiftDown, ButtonAction::ShiftDownTwo),
        ] {
            map.assign(button, Press::Short, Some(one));
            map.assign(button, Press::Long, Some(one));
            map.assign(button, Press::Double, Some(two));
        }
        map
    }

    /// What `press` of `button` (1 to [`BUTTONS`]) does; nothing for other buttons.
    #[must_use]
    pub fn action(&self, button: u8, press: Press) -> Option<ButtonAction> {
        let index = usize::from(button.checked_sub(1)?);
        self.actions.get(index)?[press.index()]
    }

    /// Gives `press` of `button` (1 to [`BUTTONS`]) `action`, or nothing; other buttons are
    /// left alone.
    pub fn assign(&mut self, button: u8, press: Press, action: Option<ButtonAction>) {
        let Some(index) = button.checked_sub(1).map(usize::from) else {
            return;
        };
        if let Some(presses) = self.actions.get_mut(index) {
            presses[press.index()] = action;
        }
    }

    /// What `presses` ask for, in order.
    #[must_use]
    pub fn actions(&self, presses: &[ButtonPress]) -> Vec<ButtonAction> {
        presses
            .iter()
            .filter_map(|p| self.action(p.channel, p.press))
            .collect()
    }
}

impl Default for ButtonMap {
    /// The first two buttons shift up and down, until the rider gives them something else.
    fn default() -> Self {
        Self::shifting(1, 2)
    }
}

/// A source of shifts (R4, R7) and of the other controls its buttons were given (#139).
pub trait ShiftInput: Send {
    /// What the rider knows it by, e.g. "Keyboard".
    fn name(&self) -> &str;

    /// The shifts asked for since the last call, oldest first.
    fn poll(&mut self) -> Vec<Shift>;

    /// The other controls asked for up to the last [`Self::poll`], oldest first; most inputs
    /// only shift.
    fn controls(&mut self) -> Vec<Control> {
        Vec::new()
    }

    /// Whether it can shift now: a wireless controller may drop out; a keyboard always can.
    fn connected(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(channel: u8, press: Press) -> ButtonPress {
        ButtonPress { channel, press }
    }

    #[test]
    fn shifting_buttons_shift_one_gear_a_press_and_two_a_double_press() {
        let map = ButtonMap::shifting(2, 1);

        let actions = map.actions(&[
            press(2, Press::Short),
            press(1, Press::Long),
            press(3, Press::Short),
            press(2, Press::Double),
            press(1, Press::Double),
        ]);

        assert_eq!(
            actions,
            [
                ButtonAction::ShiftUp,
                ButtonAction::ShiftDown,
                ButtonAction::ShiftUpTwo,
                ButtonAction::ShiftDownTwo
            ],
            "button 3 does nothing"
        );
        assert_eq!(ButtonAction::ShiftUpTwo.shifts(), [Shift::Up, Shift::Up]);
    }

    #[test]
    fn each_press_of_a_button_can_do_something_else() {
        let mut map = ButtonMap::shifting(1, 2);
        let camera = ButtonAction::Control(Control::NextCamera);
        let music = ButtonAction::Control(Control::PlayPause);

        map.assign(3, Press::Short, Some(camera));
        map.assign(3, Press::Double, Some(music));
        map.assign(1, Press::Long, None);

        assert_eq!(map.action(3, Press::Short), Some(camera));
        assert_eq!(map.action(3, Press::Long), None);
        assert_eq!(map.action(3, Press::Double), Some(music));
        assert_eq!(map.action(1, Press::Short), Some(ButtonAction::ShiftUp));
        assert_eq!(map.action(1, Press::Long), None);
    }

    #[test]
    fn buttons_beyond_the_four_do_nothing() {
        let mut map = ButtonMap::NONE;

        map.assign(0, Press::Short, Some(ButtonAction::ShiftUp));
        map.assign(5, Press::Short, Some(ButtonAction::ShiftUp));

        assert_eq!(map, ButtonMap::NONE);
        assert_eq!(map.action(0, Press::Short), None);
        assert_eq!(map.action(5, Press::Short), None);
    }
}
