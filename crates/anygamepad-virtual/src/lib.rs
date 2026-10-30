//! Virtual controllers for the anygamepad desktop app (A4): the controllers
//! it can pretend to be, and how canonical state becomes their reports.
//!
//! A [`VirtualPad`] is one emulated controller of a [`Target`] type (Xbox
//! 360, DualShock 4, DualSense, Switch Pro Controller). It is sans-IO, like
//! the input protocols of the `anygamepad` crate: it turns a
//! [`GamepadState`] into the bytes of the controller's input report, the
//! host's output reports into canonical [`OutputCommand`]s, and answers the
//! feature reports the host reads. The platform outputs move the bytes:
//! uinput and UHID on Linux (A5), ViGEmBus on Windows (A7),
//! `IOHIDUserDevice` on macOS (A8).
//!
//! Every pad emulates its USB form: the same reports on every OS, no
//! Bluetooth CRC, and a cable that keeps the battery charged.
//!
//! ```
//! use anygamepad_model::{Axis, Button, GamepadState, OutputCommand, Rgb};
//! use anygamepad_virtual::Target;
//!
//! let mut pad = Target::DualShock4.create(0);
//! let mut state = GamepadState::new();
//! state.set_pressed(Button::South, true);
//! state.set_axis(Axis::LeftStickY, 1.0);
//!
//! let report = pad.input_report(&state);
//! assert_eq!(report[0], 0x01); // report ID
//! assert_eq!(report[2], 0); // left stick Y: up
//! assert_eq!(report[5] & 0x20, 0x20); // Cross
//!
//! // The host sets the light bar.
//! let mut output = [0u8; 32];
//! output[..9].copy_from_slice(&[0x05, 0x02, 0, 0, 0, 0, 0xff, 0x80, 0x00]);
//! assert_eq!(
//!     pad.output_report(&output),
//!     [OutputCommand::Lightbar(Rgb { r: 0xff, g: 0x80, b: 0 })]
//! );
//! ```
#![forbid(unsafe_code)]

use core::fmt;
use core::str::FromStr;

use anygamepad_model::{Capabilities, Family, GamepadState, OutputCommand};

pub mod descriptor;
mod playstation;
mod switch_pro;
mod xbox360;

pub use playstation::{ACCEL_COUNTS_PER_G, DualSense, DualShock4, GYRO_COUNTS_PER_DEGREE};
pub use switch_pro::{STICK_CENTER, STICK_RANGE, SwitchPro};
pub use xbox360::Xbox360;

/// A controller type the app can emulate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Target {
    /// Microsoft Xbox 360 Controller (wired): the XInput pad every game
    /// supports.
    Xbox360,
    /// Sony DualShock 4 (second version).
    DualShock4,
    /// Sony DualSense.
    DualSense,
    /// Nintendo Switch Pro Controller.
    SwitchPro,
}

impl Target {
    /// Every target, in menu order.
    pub const ALL: [Self; 4] = [
        Self::Xbox360,
        Self::DualShock4,
        Self::DualSense,
        Self::SwitchPro,
    ];

    /// Stable snake_case name, for settings and logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xbox360 => "xbox360",
            Self::DualShock4 => "dualshock4",
            Self::DualSense => "dualsense",
            Self::SwitchPro => "switch_pro",
        }
    }

    /// The family games see.
    pub const fn family(self) -> Family {
        match self {
            Self::Xbox360 => Family::Xbox,
            Self::DualShock4 => Family::DualShock4,
            Self::DualSense => Family::DualSense,
            Self::SwitchPro => Family::SwitchPro,
        }
    }

    /// The USB identity the emulated pad shows.
    pub const fn identity(self) -> &'static VirtualIdentity {
        match self {
            Self::Xbox360 => &xbox360::IDENTITY,
            Self::DualShock4 => &playstation::DUALSHOCK4_IDENTITY,
            Self::DualSense => &playstation::DUALSENSE_IDENTITY,
            Self::SwitchPro => &switch_pro::IDENTITY,
        }
    }

    /// A new emulated pad of this type. `index` tells apart the pads the app
    /// creates at the same time; it goes into the made-up Bluetooth address
    /// that PlayStation drivers ask for (see [`virtual_mac`]).
    pub fn create(self, index: u8) -> Box<dyn VirtualPad> {
        match self {
            Self::Xbox360 => Box::new(Xbox360::new()),
            Self::DualShock4 => Box::new(DualShock4::new(index)),
            Self::DualSense => Box::new(DualSense::new(index)),
            Self::SwitchPro => Box::new(SwitchPro::new()),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown [`Target`] name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseTargetError(String);

impl fmt::Display for ParseTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown virtual pad type `{}`", self.0)
    }
}

impl std::error::Error for ParseTargetError {}

impl FromStr for Target {
    type Err = ParseTargetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|target| target.as_str() == s)
            .ok_or_else(|| ParseTargetError(s.to_owned()))
    }
}

/// The USB identity of an emulated pad: what the OS and games see before
/// any report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VirtualIdentity {
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Device release number (`bcdDevice`).
    pub version: u16,
    /// Device name, as Linux drivers name the real pad's input device.
    pub name: &'static str,
}

/// One emulated controller (see the [crate documentation](crate)).
///
/// Reports start with their report ID when the pad uses report IDs, as
/// UHID, `IOHIDUserDevice` and hidraw carry them.
pub trait VirtualPad: Send {
    /// The controller type.
    fn target(&self) -> Target;

    /// The USB identity.
    fn identity(&self) -> &'static VirtualIdentity {
        self.target().identity()
    }

    /// The HID report descriptor, for outputs that emulate the pad as a HID
    /// device. `None` for pads that are not HID devices (the Xbox 360
    /// Controller is a vendor-class USB device; see [`Xbox360`]).
    fn report_descriptor(&self) -> Option<&[u8]>;

    /// What a game can read from and drive on the pad. Controls the
    /// target lacks are dropped from its reports.
    fn capabilities(&self) -> Capabilities;

    /// The input report that shows `state`. Pads with a report counter or
    /// a sensor clock advance it on every call.
    fn input_report(&mut self, state: &GamepadState) -> &[u8];

    /// The commands in an output report from the host. Empty for reports
    /// the pad does not know, or that change nothing canonical (audio,
    /// trigger effects, ...).
    fn output_report(&mut self, report: &[u8]) -> Vec<OutputCommand>;

    /// The reply to the host reading feature report `report_id`, or `None`
    /// when the pad has no such report.
    fn feature_report(&mut self, report_id: u8) -> Option<Vec<u8>>;
}

/// The Bluetooth address a virtual pad gives when the host asks for it:
/// PlayStation drivers read it from every pad, and Linux's
/// `hid-playstation` refuses a pad without one. Made up, never a real
/// pad's: locally administered and unicast (`02:61:67:00:00:<index>`, `ag`
/// for anygamepad), in display order.
pub const fn virtual_mac(index: u8) -> [u8; 6] {
    [0x02, b'a', b'g', 0x00, 0x00, index]
}

/// `value` in `[-1, 1]` to `0..=255` with 128 at the centre.
pub(crate) fn unsigned_byte(value: f32) -> u8 {
    ((value.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8
}

/// `value` in `[0, 1]` to `0..=255`.
pub(crate) fn trigger_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A motor strength byte to `[0, 1]`.
pub(crate) fn motor(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_names_round_trip() {
        for target in Target::ALL {
            assert_eq!(target.as_str().parse::<Target>(), Ok(target));
            assert_eq!(target.create(0).target(), target);
        }
        assert!("XBOX360".parse::<Target>().is_err());
    }

    #[test]
    fn identities_match_families() {
        for target in Target::ALL {
            let id = target.identity();
            assert_eq!(
                Family::from_ids(id.vendor_id, id.product_id),
                Some(target.family()),
                "{target}"
            );
        }
    }

    #[test]
    fn virtual_mac_is_local_unicast() {
        let mac = virtual_mac(3);
        assert_eq!(mac[0] & 0x03, 0x02);
        assert_eq!(mac[5], 3);
    }

    #[test]
    fn byte_scaling() {
        assert_eq!(unsigned_byte(-1.0), 0);
        assert_eq!(unsigned_byte(0.0), 128);
        assert_eq!(unsigned_byte(1.0), 255);
        assert_eq!(unsigned_byte(2.0), 255);
        assert_eq!(trigger_byte(0.5), 128);
        assert_eq!(trigger_byte(-1.0), 0);
        assert_eq!(motor(255), 1.0);
    }
}
