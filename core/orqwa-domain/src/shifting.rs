//! Shifting (R7, R9): inputs that shift the virtual gears, behind the [`ShiftInput`] plugin
//! interface — the keyboard, Shimano Di2 buttons and, later, others — and what the rider gave
//! each button to do (#139).

use crate::battery::Battery;

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
    /// Moves the camera around the rider to the left.
    CameraLeft,
    /// Moves the camera around the rider to the right.
    CameraRight,
    /// Moves the camera closer to the rider.
    CameraNearer,
    /// Moves the camera farther from the rider.
    CameraFurther,
    /// Into the overlay, or back out of it.
    Overlay,
    /// Plays or pauses the rider's music.
    PlayPause,
    /// The next track.
    NextTrack,
    /// The previous track.
    PreviousTrack,
    /// Pauses the ride, or goes on with it.
    Pause,
    /// A harder trainer difficulty: more of the road's gradient.
    DifficultyUp,
    /// An easier trainer difficulty.
    DifficultyDown,
    /// A workout's powers a little harder.
    IntensityUp,
    /// A workout's powers a little easier.
    IntensityDown,
    /// On to the next step of a structured workout.
    SkipStep,
    /// Ends the ride and saves it, as the ride settings' "Finish & save" does.
    FinishRide,
}

/// A step through ORQWA's screens, as a TV remote's arrows, OK and back take it (#228): the
/// front end moves its focus, presses what has it, or goes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    /// Focus up.
    Up,
    /// Focus down.
    Down,
    /// Focus left.
    Left,
    /// Focus right.
    Right,
    /// Presses what has the focus.
    Accept,
    /// Closes a dialog, or leaves the screen for the one before.
    Back,
}

impl Navigation {
    /// Every step, in the order a remote shows them.
    pub const ALL: [Self; 6] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Accept,
        Self::Back,
    ];

    /// Its name for the front end and the remote's page.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Accept => "ok",
            Self::Back => "back",
        }
    }

    /// The step called `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.name() == name)
    }
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
    pub const ALL: [Self; 20] = [
        Self::ShiftUp,
        Self::ShiftDown,
        Self::ShiftUpTwo,
        Self::ShiftDownTwo,
        Self::Control(Control::NextCamera),
        Self::Control(Control::CameraLeft),
        Self::Control(Control::CameraRight),
        Self::Control(Control::CameraNearer),
        Self::Control(Control::CameraFurther),
        Self::Control(Control::Overlay),
        Self::Control(Control::PlayPause),
        Self::Control(Control::NextTrack),
        Self::Control(Control::PreviousTrack),
        Self::Control(Control::Pause),
        Self::Control(Control::DifficultyUp),
        Self::Control(Control::DifficultyDown),
        Self::Control(Control::IntensityUp),
        Self::Control(Control::IntensityDown),
        Self::Control(Control::SkipStep),
        Self::Control(Control::FinishRide),
    ];

    /// Its name in the settings files, for the front end and the phone remote (#228).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ShiftUp => "shift_up",
            Self::ShiftDown => "shift_down",
            Self::ShiftUpTwo => "shift_up_two",
            Self::ShiftDownTwo => "shift_down_two",
            Self::Control(Control::NextCamera) => "next_camera",
            Self::Control(Control::CameraLeft) => "camera_left",
            Self::Control(Control::CameraRight) => "camera_right",
            Self::Control(Control::CameraNearer) => "camera_nearer",
            Self::Control(Control::CameraFurther) => "camera_further",
            Self::Control(Control::Overlay) => "overlay",
            Self::Control(Control::PlayPause) => "play_pause",
            Self::Control(Control::NextTrack) => "next_track",
            Self::Control(Control::PreviousTrack) => "previous_track",
            Self::Control(Control::Pause) => "pause",
            Self::Control(Control::DifficultyUp) => "difficulty_up",
            Self::Control(Control::DifficultyDown) => "difficulty_down",
            Self::Control(Control::IntensityUp) => "intensity_up",
            Self::Control(Control::IntensityDown) => "intensity_down",
            Self::Control(Control::SkipStep) => "skip_step",
            Self::Control(Control::FinishRide) => "finish_ride",
        }
    }

    /// The action called `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }

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

/// The buttons a controller can have assigned: a Di2 has four D-Fly channels, a Zwift Click v2
/// ten buttons on its two pods.
pub const BUTTONS: u8 = 10;

/// Which shifter a controller is: their buttons and their protocols differ, so each has its
/// own [`ButtonMap`] (R84).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerModel {
    /// Shimano Di2 (R59): four D-Fly channels, each telling presses, holds and double presses.
    Di2,
    /// The Zwift Click (R75): a + and a − button, each telling presses and holds.
    ZwiftClick,
    /// The Zwift Click v2 (R77): two pods, + and − and eight more buttons, presses and holds.
    ZwiftClickV2,
    /// The phone remote (#228): ten buttons on a page in the phone's browser, two big ones
    /// first, each acting the moment it is pressed; no holds.
    PhoneRemote,
}

impl ControllerModel {
    /// Every model whose buttons the rider assigns.
    pub const ALL: [Self; 4] = [
        Self::Di2,
        Self::ZwiftClick,
        Self::ZwiftClickV2,
        Self::PhoneRemote,
    ];

    /// The Bluetooth shifters, which shared one assignment before R84.
    pub const SHIFTERS: [Self; 3] = [Self::Di2, Self::ZwiftClick, Self::ZwiftClickV2];

    /// The model's name as the front end and the settings files know it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Di2 => "di2",
            Self::ZwiftClick => "zwift_click",
            Self::ZwiftClickV2 => "zwift_click_v2",
            Self::PhoneRemote => "phone_remote",
        }
    }

    /// What its buttons do until the rider gives them something else: a shifter's first two
    /// shift; the phone remote's big ones shift and the others work the ride.
    #[must_use]
    pub fn default_buttons(self) -> ButtonMap {
        match self {
            Self::PhoneRemote => {
                let mut map = ButtonMap::NONE;
                for (button, action) in (1..).zip([
                    ButtonAction::ShiftUp,
                    ButtonAction::ShiftDown,
                    ButtonAction::Control(Control::Pause),
                    ButtonAction::Control(Control::NextCamera),
                    ButtonAction::Control(Control::DifficultyDown),
                    ButtonAction::Control(Control::DifficultyUp),
                    ButtonAction::Control(Control::IntensityDown),
                    ButtonAction::Control(Control::IntensityUp),
                    ButtonAction::Control(Control::SkipStep),
                    ButtonAction::Control(Control::PlayPause),
                ]) {
                    map.assign(button, Press::Short, Some(action));
                }
                map
            }
            _ => ButtonMap::default(),
        }
    }

    /// The model called `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.name() == name)
    }
}

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

    /// The steps through ORQWA's screens asked for up to the last [`Self::poll`], oldest
    /// first, with or without a ride; only a remote has them (#228).
    fn navigation(&mut self) -> Vec<Navigation> {
        Vec::new()
    }

    /// Whether it can shift now: a wireless controller may drop out; a keyboard always can.
    fn connected(&self) -> bool {
        true
    }

    /// Why the last attempt to connect it failed, while it keeps trying (R80); a keyboard
    /// never fails.
    fn failure(&self) -> Option<&str> {
        None
    }

    /// How full its batteries are, as far as it told while connected (R97); a keyboard has
    /// none.
    fn battery(&self) -> Battery {
        Battery::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_navigation_step_has_its_own_name() {
        for step in Navigation::ALL {
            assert_eq!(Navigation::from_name(step.name()), Some(step));
        }
        assert_eq!(Navigation::from_name("home"), None);
    }

    #[test]
    fn every_button_action_has_its_own_name() {
        for action in ButtonAction::ALL {
            assert_eq!(ButtonAction::from_name(action.name()), Some(action));
        }
        assert_eq!(ButtonAction::from_name("launch"), None);
    }

    #[test]
    fn the_phone_remote_shifts_with_its_big_buttons_and_works_the_ride_with_the_rest() {
        let map = ControllerModel::PhoneRemote.default_buttons();
        let pressed = |button| map.action(button, Press::Short);

        assert_eq!(pressed(1), Some(ButtonAction::ShiftUp));
        assert_eq!(pressed(2), Some(ButtonAction::ShiftDown));
        assert!(
            (3..=BUTTONS).all(|button| matches!(pressed(button), Some(ButtonAction::Control(_))))
        );
        assert!(
            (1..=BUTTONS).all(|button| map.action(button, Press::Long).is_none()),
            "a page has no holds"
        );
        assert_eq!(
            ControllerModel::from_name("phone_remote"),
            Some(ControllerModel::PhoneRemote)
        );
        assert_eq!(ControllerModel::Di2.default_buttons(), ButtonMap::default());
    }

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
    fn buttons_beyond_the_ten_do_nothing() {
        let mut map = ButtonMap::NONE;

        map.assign(0, Press::Short, Some(ButtonAction::ShiftUp));
        map.assign(11, Press::Short, Some(ButtonAction::ShiftUp));

        assert_eq!(map, ButtonMap::NONE);
        assert_eq!(map.action(0, Press::Short), None);
        assert_eq!(map.action(11, Press::Short), None);
        map.assign(10, Press::Short, Some(ButtonAction::ShiftUp));
        assert_eq!(map.action(10, Press::Short), Some(ButtonAction::ShiftUp));
    }
}
