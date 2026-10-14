//! Normalized gamepad input for Rust games.
//!
//! anygamepad hides controller modes (XInput, DirectInput, Switch Pro,
//! DualShock 4/DualSense emulation, vendor modes), transports and operating
//! system differences behind one canonical gamepad model.
//!
//! This crate is the public facade that games depend on. It is not usable yet:
//! the project is in its first development phase.
//!
//! # Features
//!
//! - `web` (default): the browser provider ([`web`]), active on
//!   `wasm32-unknown-unknown`.
#![forbid(unsafe_code)]

pub mod protocol;
pub mod provider;
#[cfg(feature = "web")]
pub mod web;

pub use anygamepad_db as db;
pub use anygamepad_model as model;

/// Version of the anygamepad library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
