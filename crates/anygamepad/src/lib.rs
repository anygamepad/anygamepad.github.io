//! Normalized gamepad input for Rust games.
//!
//! anygamepad hides controller modes (XInput, DirectInput, Switch Pro,
//! DualShock 4/DualSense emulation, vendor modes), transports and operating
//! system differences behind one canonical gamepad model.
//!
//! This crate is the public facade that games depend on. It is not usable yet:
//! the project is in its first development phase.
//!
//! Providers ([`provider`]) read the raw elements of each device; the
//! mapping engine ([`mapping`]) applies the device's profile
//! ([`db`]) to turn them into the canonical model ([`model`]). The guided
//! mapping wizard ([`wizard`]) records a device's mapping by asking the
//! person to use each control in turn.
//!
//! # Features
//!
//! - `web` (default): the browser provider ([`web`]), active on
//!   `wasm32-unknown-unknown`.
//! - `native` (default): anygamepad's own OS backends ([`native`]); on
//!   Linux, the evdev provider.
#![forbid(unsafe_code)]

pub mod mapping;
#[cfg(feature = "native")]
pub mod native;
pub mod protocol;
pub mod provider;
#[cfg(feature = "web")]
pub mod web;
pub mod wizard;

pub use anygamepad_db as db;
pub use anygamepad_model as model;

/// Version of the anygamepad library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
