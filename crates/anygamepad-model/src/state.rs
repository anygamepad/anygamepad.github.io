//! The state of a gamepad at one point in time.

use crate::{Axis, Button, ButtonSet, MAX_TOUCH_POINTS, MotionSample, Power, Stick, TouchPoint};

/// Normalized state of a gamepad.
///
/// `Copy` and fixed-size, so a frame's snapshot is cheap to keep and to pass
/// across the WebAssembly boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct GamepadState {
    /// Buttons currently pressed.
    pub buttons: ButtonSet,
    /// Axis values, in the order of [`Axis::ALL`]; always within
    /// [`Axis::range`] when set through [`GamepadState::set_axis`].
    ///
    /// Serialized as an object keyed by axis name in human-readable formats.
    #[cfg_attr(feature = "serde", serde(with = "axes_serde"))]
    #[cfg_attr(feature = "schemars", schemars(schema_with = "axes_serde::schema"))]
    #[cfg_attr(feature = "tsify", tsify(type = "Record<Axis, number>"))]
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

/// Serde form of [`GamepadState::axes`]: an object keyed by axis name in
/// human-readable formats, the array in binary formats. Values are clamped
/// into their axis range when read.
#[cfg(feature = "serde")]
mod axes_serde {
    use core::fmt;

    use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
    use serde::ser::{Serialize, SerializeStruct, Serializer};

    use crate::Axis;

    type Axes = [f32; Axis::COUNT];

    const NAMES: [&str; Axis::COUNT] = {
        let mut names = [""; Axis::COUNT];
        let mut i = 0;
        while i < Axis::COUNT {
            names[i] = Axis::ALL[i].as_str();
            i += 1;
        }
        names
    };

    pub(super) fn serialize<S: Serializer>(axes: &Axes, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            // A struct, not a map, so that every serializer writes a plain
            // object (serde-wasm-bindgen turns maps into JavaScript `Map`s).
            let mut object = serializer.serialize_struct("Axes", Axis::COUNT)?;
            for axis in Axis::ALL {
                object.serialize_field(axis.as_str(), &axes[axis.index()])?;
            }
            object.end()
        } else {
            axes.serialize(serializer)
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Axes, D::Error> {
        let mut axes = if deserializer.is_human_readable() {
            deserializer.deserialize_struct("Axes", &NAMES, AxesVisitor)?
        } else {
            Axes::deserialize(deserializer)?
        };
        for axis in Axis::ALL {
            axes[axis.index()] = axis.clamp(axes[axis.index()]);
        }
        Ok(axes)
    }

    /// Missing axes are at rest (0); unknown names are rejected.
    struct AxesVisitor;

    impl<'de> Visitor<'de> for AxesVisitor {
        type Value = Axes;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object from axis names to values")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Axes, A::Error> {
            let mut axes = [0.0; Axis::COUNT];
            while let Some(axis) = map.next_key::<Axis>()? {
                axes[axis.index()] = map.next_value()?;
            }
            Ok(axes)
        }
    }

    #[cfg(feature = "schemars")]
    pub(super) fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let number = schemars::json_schema!({ "type": "number" });
        let properties: std::collections::BTreeMap<&str, schemars::Schema> = NAMES
            .into_iter()
            .map(|name| (name, number.clone()))
            .collect();
        schemars::json_schema!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        })
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
