//! The state of a gamepad at one point in time.

use crate::{Axis, Button, ButtonSet, MAX_TOUCH_POINTS, MotionSample, Power, Stick, TouchPoint};

/// Normalized state of a gamepad.
///
/// `Copy` and fixed-size, so a frame's snapshot is cheap to keep and to pass
/// across the WebAssembly boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GamepadState {
    /// Buttons currently pressed.
    pub buttons: ButtonSet,
    /// Axis values, in the order of [`Axis::ALL`]; always within
    /// [`Axis::range`] when set through [`GamepadState::set_axis`].
    axes: [f32; Axis::COUNT],
    /// Fingers on the touchpads; `None` entries are free slots.
    pub touches: [Option<TouchPoint>; MAX_TOUCH_POINTS],
    /// Latest motion reading, if the device has motion sensors.
    pub motion: Option<MotionSample>,
    /// Power status.
    pub power: Power,
}

impl GamepadState {
    /// A state with nothing pressed, sticks centered and triggers released.
    pub const fn new() -> Self {
        Self {
            buttons: ButtonSet::EMPTY,
            axes: [0.0; Axis::COUNT],
            touches: [None; MAX_TOUCH_POINTS],
            motion: None,
            power: Power {
                state: crate::PowerState::Unknown,
                percent: None,
            },
        }
    }

    /// Whether `button` is pressed.
    pub const fn pressed(&self, button: Button) -> bool {
        self.buttons.contains(button)
    }

    /// Presses or releases `button`.
    pub fn set_pressed(&mut self, button: Button, pressed: bool) {
        self.buttons.set(button, pressed);
    }

    /// Value of `axis`, within [`Axis::range`].
    pub const fn axis(&self, axis: Axis) -> f32 {
        self.axes[axis.index()]
    }

    /// Sets `axis`, clamped into [`Axis::range`] (NaN becomes 0).
    pub fn set_axis(&mut self, axis: Axis, value: f32) {
        self.axes[axis.index()] = axis.clamp(value);
    }

    /// Position of the left stick.
    pub const fn left_stick(&self) -> Stick {
        Stick {
            x: self.axis(Axis::LeftStickX),
            y: self.axis(Axis::LeftStickY),
        }
    }

    /// Position of the right stick.
    pub const fn right_stick(&self) -> Stick {
        Stick {
            x: self.axis(Axis::RightStickX),
            y: self.axis(Axis::RightStickY),
        }
    }

    /// Fingers currently down, on any touchpad.
    pub fn active_touches(&self) -> impl Iterator<Item = TouchPoint> + '_ {
        self.touches.iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_neutral() {
        let state = GamepadState::new();
        assert_eq!(state, GamepadState::default());
        assert!(state.buttons.is_empty());
        assert_eq!(state.left_stick(), Stick::default());
        assert_eq!(state.active_touches().count(), 0);
    }

    #[test]
    fn axes_are_clamped() {
        let mut state = GamepadState::new();
        state.set_axis(Axis::LeftStickY, 1.5);
        state.set_axis(Axis::RightTrigger, -0.2);
        state.set_axis(Axis::LeftStickX, f32::NAN);
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);
        assert_eq!(state.axis(Axis::RightTrigger), 0.0);
        assert_eq!(state.axis(Axis::LeftStickX), 0.0);
    }

    #[test]
    fn buttons_and_touches() {
        let mut state = GamepadState::new();
        state.set_pressed(Button::South, true);
        assert!(state.pressed(Button::South));
        state.set_pressed(Button::South, false);
        assert!(!state.pressed(Button::South));

        let finger = TouchPoint {
            touchpad: 0,
            id: 7,
            x: 0.25,
            y: 0.5,
        };
        state.touches[2] = Some(finger);
        assert_eq!(state.active_touches().collect::<Vec<_>>(), [finger]);
    }
}
