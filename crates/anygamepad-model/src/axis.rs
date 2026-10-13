//! Analog axes of the canonical gamepad.

use core::ops::RangeInclusive;

use crate::macros::{enum_set, named_enum};

named_enum! {
    /// An analog control of the canonical gamepad.
    ///
    /// Stick axes are in `[-1.0, 1.0]` with **X positive = right** and
    /// **Y positive = up** (ADR 0001; the browser Gamepad API and SDL3 use
    /// the opposite Y sign). Trigger axes are in `[0.0, 1.0]`, 0 at rest.
    pub enum Axis: "axis" {
        /// Left stick, horizontal: -1 left, +1 right.
        LeftStickX = "left_stick_x",
        /// Left stick, vertical: -1 down, +1 up.
        LeftStickY = "left_stick_y",
        /// Right stick, horizontal: -1 left, +1 right.
        RightStickX = "right_stick_x",
        /// Right stick, vertical: -1 down, +1 up.
        RightStickY = "right_stick_y",
        /// Left trigger: 0 released, 1 fully pressed.
        LeftTrigger = "left_trigger",
        /// Right trigger: 0 released, 1 fully pressed.
        RightTrigger = "right_trigger",
    }
}

impl Axis {
    /// Whether this is a trigger axis, in `[0.0, 1.0]`.
    pub const fn is_trigger(self) -> bool {
        matches!(self, Self::LeftTrigger | Self::RightTrigger)
    }

    /// Value at rest.
    pub const fn min(self) -> f32 {
        if self.is_trigger() { 0.0 } else { -1.0 }
    }

    /// Valid range of values.
    pub const fn range(self) -> RangeInclusive<f32> {
        self.min()..=1.0
    }

    /// Clamps `value` into [`Self::range`]; NaN becomes 0.
    pub fn clamp(self, value: f32) -> f32 {
        if value.is_nan() {
            0.0
        } else {
            value.clamp(self.min(), 1.0)
        }
    }
}

enum_set! {
    /// A set of [`Axis`] values: the axes a device has.
    pub struct AxisSet(u8): Axis;
}

/// Position of a stick, each coordinate in `[-1.0, 1.0]` (Y positive = up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Stick {
    /// Horizontal position: -1 left, +1 right.
    pub x: f32,
    /// Vertical position: -1 down, +1 up.
    pub y: f32,
}

impl Stick {
    /// Distance from the center. Square gates can exceed 1 on the diagonals.
    pub fn magnitude(self) -> f32 {
        self.x.hypot(self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(Axis::LeftStickY.range(), -1.0..=1.0);
        assert_eq!(Axis::RightTrigger.range(), 0.0..=1.0);
        assert_eq!(Axis::LeftTrigger.clamp(-0.5), 0.0);
        assert_eq!(Axis::LeftStickX.clamp(-1.5), -1.0);
        assert_eq!(Axis::RightStickX.clamp(f32::NAN), 0.0);
    }

    #[test]
    fn names_round_trip() {
        for axis in Axis::ALL {
            assert_eq!(axis.as_str().parse::<Axis>(), Ok(axis));
            assert_eq!(Axis::from_bit(axis.bit()), Some(axis));
        }
        assert!("LeftStickX".parse::<Axis>().is_err());
        assert_eq!(AxisSet::ALL.len(), Axis::COUNT);
    }
}
