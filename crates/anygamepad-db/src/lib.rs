//! Device profile format and the embedded device database that tells the
//! mapping engine how to turn a specific device into the canonical model.
//!
//! A [`Profile`] says which devices it is for ([`MatchRule`]s on the
//! [`DeviceIdentity`](anygamepad_model::identity::DeviceIdentity) a
//! provider reports) and how their raw elements map to canonical buttons
//! and axes ([`Mapping`]s, which can differ per OS, backend, transport or
//! browser). [`best_match`] picks the profile for a device.
//!
//! Profiles are TOML files in `data/profiles/` (ADR 0003), described by a
//! JSON Schema ([`json_schema`], committed as `data/profiles/schema.json`).
//! [`sdl`] converts SDL GameControllerDB entries into profiles.
//!
//! # Features
//!
//! - `toml`: [`Profile::from_toml`] and [`Profile::to_toml`].
//! - `schemars`: [`json_schema`].
#![forbid(unsafe_code)]

mod input;
mod profile;
pub mod sdl;

pub use input::{AxisRange, Input, ParseInputError};
#[cfg(feature = "toml")]
pub use profile::ProfileError;
pub use profile::{
    AxisMapping, Indices, InvalidMapping, InvalidProfile, Mapping, MatchRule, MatchScore, Profile,
    ProfileSource, SCHEMA_VERSION, best_match,
};

/// The JSON Schema of the profile format, as committed in
/// `data/profiles/schema.json`.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(Profile);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/profile.json".into(),
    );
    schema
}
