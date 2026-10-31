//! Canonical gamepad model: the device-independent buttons, axes, sensors and
//! output capabilities that every controller is normalized to.
//!
//! Conventions (ADR 0001, `docs/adr/0001-canonical-model-naming.md`):
//!
//! - Controls are named by **position**: [`Button::South`] is the bottom face
//!   button on every controller. [`FaceLayout`] gives the printed label
//!   (`A`, `Cross`, Nintendo `B`, ...) for prompts.
//! - Stick axes are in `[-1.0, 1.0]` with X positive = right and **Y positive
//!   = up**; triggers are in `[0.0, 1.0]`. See [`Axis`].
//! - Motion is in SI units in a right-handed device frame: gyroscope in
//!   rad/s, accelerometer in m/s². See [`motion`].
//! - Touch points are normalized to `[0.0, 1.0]`. See [`TouchPoint`].
//! - Everything a device has is listed in its [`Capabilities`].
//!
//! Below the canonical model, [`identity`] describes which device a provider
//! sees (shared with profiles and reports), [`raw`] describes the raw
//! elements a provider reads before a profile maps them, and [`hid`]
//! describes the report structure of HID devices.
//!
//! Every enum has a stable snake_case name ([`Button::as_str`], ...), which is
//! how profiles, reports and the web bindings refer to it.
//!
//! # Features
//!
//! - `serde`: `Serialize`/`Deserialize` for every type. Human-readable
//!   formats use the snake_case names; binary formats use compact indices
//!   that are only stable within one version of this crate.
//! - `schemars`: JSON Schema of the serde representation (implies `serde`).
//!
//! ```
//! use anygamepad_model::{Axis, Button, FaceLayout, ButtonLabel, GamepadState};
//!
//! let mut state = GamepadState::new();
//! state.set_pressed(Button::South, true);
//! state.set_axis(Axis::LeftStickY, 1.0); // pushed up
//!
//! assert!(state.pressed(Button::South));
//! assert_eq!(state.left_stick().y, 1.0);
//! assert_eq!(FaceLayout::NINTENDO.label(Button::South), ButtonLabel::B);
//! ```
#![forbid(unsafe_code)]

mod macros;

mod axis;
mod button;
mod capabilities;
mod device;
mod family;
mod hash;
pub mod hid;
pub mod identity;
mod label;
pub mod motion;
mod output;
pub mod raw;
pub mod sdl;
mod state;
mod touch;

pub use axis::{Axis, AxisSet, Stick};
pub use button::{Button, ButtonSet, ExtraButton, MAX_EXTRA_BUTTONS};
pub use capabilities::Capabilities;
pub use device::{ControllerMode, Power, PowerState, Transport};
pub use family::Family;
pub use label::{ButtonLabel, FaceLayout};
pub use motion::{MotionSample, STANDARD_GRAVITY, Sensor, SensorSet, Vec3};
pub use output::{Output, OutputCommand, OutputSet, Rgb, Rumble, TriggerRumble};
pub use state::GamepadState;
pub use touch::{MAX_TOUCH_POINTS, TouchPoint, TouchpadInfo};

use core::fmt;

/// Error returned when parsing an unknown name (of a button, axis, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseNameError {
    kind: &'static str,
    name: String,
}

impl ParseNameError {
    pub(crate) fn new(kind: &'static str, name: &str) -> Self {
        Self {
            kind,
            name: name.to_owned(),
        }
    }

    /// What was being parsed (`"button"`, `"axis"`, ...).
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// The name that was not recognized.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for ParseNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} name `{}`", self.kind, self.name)
    }
}

impl std::error::Error for ParseNameError {}
