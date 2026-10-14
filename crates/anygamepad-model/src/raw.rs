//! Raw input: what a provider reads from a device before a profile maps it to
//! the canonical model (mapping engine, C3).
//!
//! A provider describes the elements it reads once, in a [`RawLayout`], and
//! fills a [`RawState`] on every poll. Each element names its
//! [`RawSource`], so profiles can refer to elements independently of the
//! order a backend lists them in.

use core::fmt;
use core::str::FromStr;

use crate::{
    Axis, Button, MAX_TOUCH_POINTS, MotionSample, ParseNameError, Power, Sensor, SensorSet,
    TouchPoint, TouchpadInfo,
};

/// Where a raw element comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
#[non_exhaustive]
pub enum RawSource {
    /// Position in the backend's own list, when nothing else identifies the
    /// element (Gamepad API, DirectInput, SDL joystick indices).
    Index(u16),
    /// Linux input event code (`BTN_*`/`KEY_*` for buttons, `ABS_*` for axes
    /// and hats).
    Evdev(u16),
    /// HID usage page and usage.
    Hid {
        /// Usage page (`0x01` Generic Desktop, `0x09` Button, ...).
        page: u16,
        /// Usage within the page.
        usage: u16,
    },
    /// Already a canonical button: the backend normalizes it (XInput, SDL
    /// gamepad API, gilrs).
    Button(Button),
    /// Already a canonical axis.
    Axis(Axis),
}

/// Text form, used by device profiles: `3` (an index), `evdev:0x0130`,
/// `hid:0x0001:0x0030`, `button:south`, `axis:left_stick_x`. Codes are
/// written as four lowercase hexadecimal digits.
impl fmt::Display for RawSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(index) => write!(f, "{index}"),
            Self::Evdev(code) => write!(f, "evdev:{code:#06x}"),
            Self::Hid { page, usage } => write!(f, "hid:{page:#06x}:{usage:#06x}"),
            Self::Button(button) => write!(f, "button:{button}"),
            Self::Axis(axis) => write!(f, "axis:{axis}"),
        }
    }
}

impl FromStr for RawSource {
    type Err = ParseNameError;

    /// Parses the text form of [`Display`](fmt::Display). Indices are
    /// decimal without leading zeros; codes are `0x` and one to four
    /// hexadecimal digits in either case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseNameError::new("raw source", s);
        let hex = |code: &str| {
            let digits = code.strip_prefix("0x")?;
            if (1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                u16::from_str_radix(digits, 16).ok()
            } else {
                None
            }
        };
        let source = if let Some(code) = s.strip_prefix("evdev:") {
            hex(code).map(Self::Evdev)
        } else if let Some(codes) = s.strip_prefix("hid:") {
            codes.split_once(':').and_then(|(page, usage)| {
                Some(Self::Hid {
                    page: hex(page)?,
                    usage: hex(usage)?,
                })
            })
        } else if let Some(name) = s.strip_prefix("button:") {
            name.parse().ok().map(Self::Button)
        } else if let Some(name) = s.strip_prefix("axis:") {
            name.parse().ok().map(Self::Axis)
        } else if s == "0" || (!s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())) {
            s.parse().ok().map(Self::Index)
        } else {
            None
        };
        source.ok_or_else(error)
    }
}

/// A raw axis and its range.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct RawAxisInfo {
    /// Where the axis comes from.
    pub source: RawSource,
    /// Minimum value the device reports.
    pub min: f32,
    /// Maximum value the device reports.
    pub max: f32,
}

/// The elements a provider reads from a device.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct RawLayout {
    /// Buttons, in the order of [`RawState::buttons`].
    pub buttons: Vec<RawSource>,
    /// Axes, in the order of [`RawState::axes`].
    pub axes: Vec<RawAxisInfo>,
    /// Hat switches, in the order of [`RawState::hats`].
    pub hats: Vec<RawSource>,
    /// Motion sensors, delivered in SI units in [`RawState::motion`].
    pub sensors: SensorSet,
    /// Touchpads, delivered normalized in [`RawState::touches`].
    pub touchpads: Vec<TouchpadInfo>,
    /// Whether [`RawState::power`] is reported.
    pub battery: bool,
}

impl RawLayout {
    /// Whether the device has `sensor`.
    pub fn has_sensor(&self, sensor: Sensor) -> bool {
        self.sensors.contains(sensor)
    }
}

/// A hat switch position: a set of directions (diagonals have two).
///
/// Bits follow SDL: 1 up, 2 right, 4 down, 8 left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct Hat(pub u8);

impl Hat {
    /// No direction.
    pub const CENTERED: Self = Self(0);
    /// Up.
    pub const UP: u8 = 1;
    /// Right.
    pub const RIGHT: u8 = 2;
    /// Down.
    pub const DOWN: u8 = 4;
    /// Left.
    pub const LEFT: u8 = 8;

    /// Whether the hat points towards `direction` (one of the direction
    /// bits).
    pub const fn has(self, direction: u8) -> bool {
        self.0 & direction != 0
    }

    /// A HID hat switch value: 0 up, then clockwise in eighths (1 up-right,
    /// 2 right, ...); anything outside `0..8` (the null state) is centered.
    /// `value` is relative to the logical minimum of the usage.
    pub const fn from_hid(value: i32) -> Self {
        const EIGHTHS: [u8; 8] = [
            Hat::UP,
            Hat::UP | Hat::RIGHT,
            Hat::RIGHT,
            Hat::DOWN | Hat::RIGHT,
            Hat::DOWN,
            Hat::DOWN | Hat::LEFT,
            Hat::LEFT,
            Hat::UP | Hat::LEFT,
        ];
        if value >= 0 && value < 8 {
            Self(EIGHTHS[value as usize])
        } else {
            Self::CENTERED
        }
    }

    /// A pair of evdev hat axes (`ABS_HAT0X`, `ABS_HAT0Y`): negative X is
    /// left, negative Y is up.
    pub const fn from_axes(x: i32, y: i32) -> Self {
        let mut bits = 0;
        if x < 0 {
            bits |= Self::LEFT;
        } else if x > 0 {
            bits |= Self::RIGHT;
        }
        if y < 0 {
            bits |= Self::UP;
        } else if y > 0 {
            bits |= Self::DOWN;
        }
        Self(bits)
    }
}

/// Raw values of every element in a [`RawLayout`], filled by a provider on
/// each poll.
///
/// Allocate it once per device with [`RawState::for_layout`]; polls overwrite
/// it in place, so the frame loop does not allocate.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct RawState {
    /// Button values in `[0.0, 1.0]` (0 or 1 for digital buttons).
    pub buttons: Vec<f32>,
    /// Axis values within the range of their [`RawAxisInfo`].
    pub axes: Vec<f32>,
    /// Hat positions.
    pub hats: Vec<Hat>,
    /// Motion readings received since the previous poll, oldest first.
    pub motion: Vec<MotionSample>,
    /// Fingers on the touchpads.
    pub touches: [Option<TouchPoint>; MAX_TOUCH_POINTS],
    /// Power status.
    pub power: Power,
}

impl RawState {
    /// A neutral state sized for `layout`: buttons released, axes at 0, hats
    /// centered.
    pub fn for_layout(layout: &RawLayout) -> Self {
        Self {
            buttons: vec![0.0; layout.buttons.len()],
            axes: vec![0.0; layout.axes.len()],
            hats: vec![Hat::CENTERED; layout.hats.len()],
            ..Self::default()
        }
    }

    /// Whether this state has one value per element of `layout`.
    pub fn fits(&self, layout: &RawLayout) -> bool {
        self.buttons.len() == layout.buttons.len()
            && self.axes.len() == layout.axes.len()
            && self.hats.len() == layout.hats.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_hat_values() {
        assert_eq!(Hat::from_hid(0), Hat(Hat::UP));
        assert_eq!(Hat::from_hid(3), Hat(Hat::DOWN | Hat::RIGHT));
        assert_eq!(Hat::from_hid(7), Hat(Hat::UP | Hat::LEFT));
        assert_eq!(Hat::from_hid(8), Hat::CENTERED);
        assert_eq!(Hat::from_hid(-1), Hat::CENTERED);
        assert!(Hat::from_hid(1).has(Hat::RIGHT));
        assert!(!Hat::from_hid(1).has(Hat::DOWN));
    }

    #[test]
    fn evdev_hat_axes() {
        assert_eq!(Hat::from_axes(0, -1), Hat(Hat::UP));
        assert_eq!(Hat::from_axes(1, 1), Hat(Hat::DOWN | Hat::RIGHT));
        assert_eq!(Hat::from_axes(0, 0), Hat::CENTERED);
    }

    #[test]
    fn raw_source_text() {
        for (text, source) in [
            ("0", RawSource::Index(0)),
            ("17", RawSource::Index(17)),
            ("evdev:0x0130", RawSource::Evdev(0x130)),
            (
                "hid:0x0001:0x0030",
                RawSource::Hid {
                    page: 0x01,
                    usage: 0x30,
                },
            ),
            ("button:south", RawSource::Button(Button::South)),
            ("axis:left_stick_x", RawSource::Axis(Axis::LeftStickX)),
        ] {
            assert_eq!(text.parse::<RawSource>(), Ok(source));
            assert_eq!(source.to_string(), text);
        }
        assert_eq!("evdev:0x13A".parse(), Ok(RawSource::Evdev(0x13a)));
        for bad in [
            "",
            "01",
            "-1",
            "+1",
            "65536",
            "x",
            "evdev:130",
            "evdev:0x",
            "evdev:0x10000",
            "hid:0x01",
            "hid:0x01:",
            "button:cross",
            "axis:",
            "index:3",
        ] {
            assert!(bad.parse::<RawSource>().is_err(), "{bad}");
        }
    }

    #[test]
    fn state_for_layout() {
        let layout = RawLayout {
            buttons: vec![RawSource::Evdev(0x130), RawSource::Button(Button::East)],
            axes: vec![RawAxisInfo {
                source: RawSource::Hid {
                    page: 0x01,
                    usage: 0x30,
                },
                min: 0.0,
                max: 255.0,
            }],
            hats: vec![RawSource::Index(0)],
            ..RawLayout::default()
        };
        let state = RawState::for_layout(&layout);
        assert!(state.fits(&layout));
        assert_eq!(state.buttons, [0.0, 0.0]);
        assert_eq!(state.hats, [Hat::CENTERED]);
        assert!(!RawState::default().fits(&layout));
    }
}
