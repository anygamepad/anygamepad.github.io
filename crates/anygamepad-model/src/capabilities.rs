//! What a device has: the capability set games query (ADR 0001).

use crate::{Axis, AxisSet, Button, ButtonSet, Output, OutputSet, Sensor, SensorSet, TouchpadInfo};

/// Every control and output of a device.
///
/// A control that is not in the capability set never changes in the device's
/// [`GamepadState`](crate::GamepadState).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Capabilities {
    /// Buttons the device has.
    pub buttons: ButtonSet,
    /// Analog axes the device has.
    pub axes: AxisSet,
    /// Motion sensors the device has.
    pub sensors: SensorSet,
    /// Touchpads, in the order [`TouchPoint::touchpad`](crate::TouchPoint::touchpad)
    /// refers to them.
    pub touchpads: Vec<TouchpadInfo>,
    /// Outputs the device supports.
    pub outputs: OutputSet,
    /// Whether the device reports its battery level.
    pub battery: bool,
}

impl Capabilities {
    /// The controls of the W3C "standard" gamepad mapping: four face buttons,
    /// shoulders, analog triggers, menu and home buttons, stick clicks,
    /// d-pad and two sticks. No sensors, touchpads or outputs.
    pub fn standard() -> Self {
        let buttons = ButtonSet::from([
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
        ]);
        Self {
            buttons,
            axes: AxisSet::ALL,
            ..Self::default()
        }
    }

    /// Whether the device has `button`.
    pub fn has_button(&self, button: Button) -> bool {
        self.buttons.contains(button)
    }

    /// Whether the device has `axis`.
    pub fn has_axis(&self, axis: Axis) -> bool {
        self.axes.contains(axis)
    }

    /// Whether the device has `sensor`.
    pub fn has_sensor(&self, sensor: Sensor) -> bool {
        self.sensors.contains(sensor)
    }

    /// Whether the device supports `output`.
    pub fn has_output(&self, output: Output) -> bool {
        self.outputs.contains(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_mapping_has_17_buttons_and_6_axes() {
        let caps = Capabilities::standard();
        assert_eq!(caps.buttons.len(), 17);
        assert_eq!(caps.axes.len(), 6);
        assert!(caps.has_button(Button::Home));
        assert!(!caps.has_button(Button::TouchpadClick));
        assert!(!caps.has_sensor(Sensor::Gyroscope));
        assert!(!caps.has_output(Output::Rumble));
        assert!(caps.touchpads.is_empty());
    }
}
