//! The W3C "standard" gamepad layout, which browsers apply to the devices
//! they know (`Gamepad.mapping == "standard"`): fixed positions for 17
//! buttons and 4 axes, the same positions as the canonical model.
//!
//! See <https://w3c.github.io/gamepad/#remapping>.

use anygamepad_model::raw::RawState;
use anygamepad_model::{Axis, Button, Capabilities, GamepadState, OutputSet};

/// `Gamepad.mapping` of devices in the standard layout.
pub const MAPPING: &str = "standard";

/// The canonical button at each standard button index.
pub const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::Select,
    Button::Start,
    Button::LeftStick,
    Button::RightStick,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Home,
];

/// The canonical axis at each standard axis index. The Gamepad API's Y axes
/// grow downwards, so [`map`] inverts them (ADR 0001: Y positive = up).
pub const AXES: [Axis; 4] = [
    Axis::LeftStickX,
    Axis::LeftStickY,
    Axis::RightStickX,
    Axis::RightStickY,
];

/// Indices of the analog trigger buttons, whose values are also the trigger
/// axes.
const TRIGGERS: [(usize, Axis); 2] = [(6, Axis::LeftTrigger), (7, Axis::RightTrigger)];

pub use crate::mapping::PRESS_THRESHOLD;

/// The canonical button at a button index of the standard layout: the 17
/// standard buttons, then extra buttons for the ones browsers append
/// (Chromium adds the touchpad click of PlayStation pads, the capture button
/// of Switch pads or the share button of Xbox Series pads as button 17).
/// Profiles say what each extra button is.
pub fn button(index: usize) -> Option<Button> {
    match index.checked_sub(BUTTONS.len()) {
        None => Some(BUTTONS[index]),
        Some(extra) => u8::try_from(extra).ok().and_then(Button::extra),
    }
}

/// What a device in the standard layout has, given how many buttons and axes
/// the browser reports and the outputs it can drive.
pub fn capabilities(buttons: usize, axes: usize, outputs: OutputSet) -> Capabilities {
    let mut capabilities = Capabilities {
        buttons: (0..buttons).filter_map(button).collect(),
        axes: AXES.into_iter().take(axes).collect(),
        outputs,
        ..Capabilities::default()
    };
    for (index, axis) in TRIGGERS {
        if index < buttons {
            capabilities.axes.insert(axis);
        }
    }
    capabilities
}

/// Writes the canonical buttons, sticks and triggers of a device in the
/// standard layout, from its raw Gamepad API values. Touches, motion and
/// power are left as they are.
pub fn map(raw: &RawState, state: &mut GamepadState) {
    for (index, &value) in raw.buttons.iter().enumerate() {
        if let Some(button) = button(index) {
            state.set_pressed(button, value >= PRESS_THRESHOLD);
        }
    }
    for (&axis, &value) in AXES.iter().zip(&raw.axes) {
        let value = match axis {
            // `0.0 - value`, not `-value`, which would turn 0 into -0.
            Axis::LeftStickY | Axis::RightStickY => 0.0 - value,
            _ => value,
        };
        state.set_axis(axis, value);
    }
    for (index, axis) in TRIGGERS {
        if let Some(&value) = raw.buttons.get(index) {
            state.set_axis(axis, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::Output;

    use super::*;

    fn raw(buttons: usize, axes: usize) -> RawState {
        RawState {
            buttons: vec![0.0; buttons],
            axes: vec![0.0; axes],
            ..RawState::default()
        }
    }

    #[test]
    fn standard_buttons_then_extras() {
        assert_eq!(button(0), Some(Button::South));
        assert_eq!(button(16), Some(Button::Home));
        assert_eq!(button(17), Button::extra(0));
        assert_eq!(button(48), Button::extra(31));
        assert_eq!(button(49), None);
        assert_eq!(button(usize::MAX), None);
    }

    #[test]
    fn capabilities_follow_what_the_browser_reports() {
        let full = capabilities(17, 4, OutputSet::from([Output::Rumble]));
        let standard = Capabilities::standard();
        assert_eq!(full.buttons, standard.buttons);
        assert_eq!(full.axes, standard.axes);
        assert!(full.has_output(Output::Rumble));

        let dualshock = capabilities(18, 4, OutputSet::EMPTY);
        assert!(dualshock.has_button(Button::extra(0).unwrap()));

        // A pad without analog triggers or right stick.
        let small = capabilities(6, 2, OutputSet::EMPTY);
        assert!(!small.has_axis(Axis::LeftTrigger));
        assert!(!small.has_axis(Axis::RightStickX));
        assert!(small.has_axis(Axis::LeftStickY));
        assert!(!small.has_button(Button::LeftTrigger));
    }

    #[test]
    fn maps_buttons_axes_and_triggers() {
        let mut raw = raw(18, 4);
        raw.buttons[0] = 1.0; // South
        raw.buttons[7] = 0.75; // Right trigger
        raw.buttons[6] = 0.1; // Left trigger, below the threshold
        raw.buttons[17] = 1.0; // First extra button
        raw.axes = vec![0.5, -1.0, 0.0, 0.25];

        let mut state = GamepadState::new();
        map(&raw, &mut state);
        assert!(state.pressed(Button::South));
        assert!(state.pressed(Button::RightTrigger));
        assert!(!state.pressed(Button::LeftTrigger));
        assert!(state.pressed(Button::extra(0).unwrap()));
        assert!(!state.pressed(Button::East));
        assert_eq!(state.axis(Axis::RightTrigger), 0.75);
        assert_eq!(state.axis(Axis::LeftTrigger), 0.1);
        assert_eq!(state.axis(Axis::LeftStickX), 0.5);
        // Pushed up in the browser (-1) is up in the canonical model (+1).
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);
        assert_eq!(state.axis(Axis::RightStickY), -0.25);
        assert!(state.axis(Axis::RightStickX).is_sign_positive());

        // Releasing updates the state.
        raw.buttons[0] = 0.0;
        map(&raw, &mut state);
        assert!(!state.pressed(Button::South));
    }

    #[test]
    fn odd_values_are_contained() {
        let mut raw = raw(17, 6);
        raw.buttons[1] = f32::NAN;
        raw.axes[0] = 3.0;
        raw.axes[5] = 1.0; // No canonical axis beyond the fourth.
        let mut state = GamepadState::new();
        map(&raw, &mut state);
        assert!(!state.pressed(Button::East));
        assert_eq!(state.axis(Axis::LeftStickX), 1.0);
        assert_eq!(state.axis(Axis::RightTrigger), 0.0);
    }
}
