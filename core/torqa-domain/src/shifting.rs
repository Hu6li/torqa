//! Shifting (R7, R9): inputs that shift the virtual gears, behind the [`ShiftInput`] plugin
//! interface — the keyboard, Shimano Di2 buttons and, later, others.

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

/// A source of shifts (R4, R7).
pub trait ShiftInput: Send {
    /// What the rider knows it by, e.g. "Keyboard".
    fn name(&self) -> &str;

    /// The shifts asked for since the last call, oldest first.
    fn poll(&mut self) -> Vec<Shift>;

    /// Whether it can shift now: a wireless controller may drop out; a keyboard always can.
    fn connected(&self) -> bool {
        true
    }
}
