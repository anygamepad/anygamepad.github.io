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
//! The [`catalog`] lists controller models as sold, with their modes and
//! the ways to connect them, and the [`lab`] inventory lists the physical
//! controllers of a test lab (ADR 0008). Both are embedded at build time
//! ([`Catalog::embedded`], [`Inventory::embedded`]); [`detect`] tells which
//! model, mode and unit a connected device is. The test [`plan`]s the test
//! modes run are data too ([`TestPlan::embedded`]).
//!
//! # Features
//!
//! - `toml`: [`Profile::from_toml`] and [`Profile::to_toml`], and the same
//!   for catalog models and inventories.
//! - `schemars`: [`json_schema`], [`catalog::json_schema`] and
//!   [`lab::json_schema`].
#![forbid(unsafe_code)]

pub mod catalog;
pub mod database;
pub mod detect;
mod embedded;
mod input;
pub mod lab;
pub mod plan;
mod profile;
pub mod sdl;
pub mod wire;

pub use catalog::Catalog;
pub use database::Database;
pub use input::{AxisRange, Input, ParseInputError};
pub use lab::Inventory;
pub use plan::TestPlan;
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
