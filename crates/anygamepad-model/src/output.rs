//! Outputs a game can drive: rumble, lights, trigger effects.

use crate::macros::{enum_set, named_enum};

named_enum! {
    /// An output a device may support.
    pub enum Output: "output" {
        /// Two body motors; see [`Rumble`].
        Rumble = "rumble",
        /// Motors in the triggers (Xbox One and Series); see
        /// [`TriggerRumble`].
        TriggerRumble = "trigger_rumble",
        /// High-definition (voice coil) rumble, as on Switch Pro Controllers
        /// and DualSense. Its commands are defined with the protocol drivers.
        HdRumble = "hd_rumble",
        /// RGB light; see [`Rgb`].
        Lightbar = "lightbar",
        /// Player number indicator (player LEDs).
        PlayerLeds = "player_leds",
        /// Adaptive trigger resistance (DualSense). Its commands are defined
        /// with the protocol drivers.
        AdaptiveTriggers = "adaptive_triggers",
    }
}

enum_set! {
    /// A set of [`Output`]s: the outputs a device supports.
    pub struct OutputSet(u8): Output;
}

/// Body rumble, each motor's strength in `[0.0, 1.0]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Rumble {
    /// Low-frequency (strong, usually left) motor.
    pub low_frequency: f32,
    /// High-frequency (weak, usually right) motor.
    pub high_frequency: f32,
}

/// Trigger rumble, each motor's strength in `[0.0, 1.0]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TriggerRumble {
    /// Left trigger motor.
    pub left: f32,
    /// Right trigger motor.
    pub right: f32,
}

/// A lightbar color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

/// A command for one of a device's [`Output`]s.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum OutputCommand {
    /// Sets the body motors ([`Output::Rumble`]); zero stops them.
    Rumble(Rumble),
    /// Sets the trigger motors ([`Output::TriggerRumble`]).
    TriggerRumble(TriggerRumble),
    /// Sets the lightbar color ([`Output::Lightbar`]).
    Lightbar(Rgb),
    /// Shows a player number, from 1 ([`Output::PlayerLeds`]); 0 turns the
    /// indicator off. Each device draws the number its own way.
    PlayerNumber(u8),
}

impl OutputCommand {
    /// The output this command drives.
    pub const fn output(&self) -> Output {
        match self {
            Self::Rumble(_) => Output::Rumble,
            Self::TriggerRumble(_) => Output::TriggerRumble,
            Self::Lightbar(_) => Output::Lightbar,
            Self::PlayerNumber(_) => Output::PlayerLeds,
        }
    }
}
