//! Touchpads.

/// Number of simultaneous touch points a [`GamepadState`](crate::GamepadState)
/// can hold, across all touchpads of the device.
pub const MAX_TOUCH_POINTS: usize = 4;

/// A finger on a touchpad.
///
/// Coordinates are normalized to `[0.0, 1.0]` with the origin at the top left
/// corner of the touchpad as seen by the player: `x` grows to the right, `y`
/// grows towards the player. Use [`TouchpadInfo`] for the aspect ratio.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchPoint {
    /// Index of the touchpad in
    /// [`Capabilities::touchpads`](crate::Capabilities::touchpads).
    pub touchpad: u8,
    /// Identifier of the finger, stable while the finger stays down.
    pub id: u8,
    /// Horizontal position: 0 left edge, 1 right edge.
    pub x: f32,
    /// Vertical position: 0 far edge, 1 edge closest to the player.
    pub y: f32,
}

/// Description of a touchpad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchpadInfo {
    /// Number of fingers it tracks at once.
    pub max_fingers: u8,
    /// Width in device units (its resolution).
    pub width: u16,
    /// Height in device units (its resolution).
    pub height: u16,
}

impl TouchpadInfo {
    /// Width divided by height; 0 when the size is unknown.
    pub fn aspect_ratio(self) -> f32 {
        if self.height == 0 {
            0.0
        } else {
            f32::from(self.width) / f32::from(self.height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratio() {
        let pad = TouchpadInfo {
            max_fingers: 2,
            width: 1920,
            height: 960,
        };
        assert_eq!(pad.aspect_ratio(), 2.0);
        assert_eq!(TouchpadInfo { height: 0, ..pad }.aspect_ratio(), 0.0);
    }
}
