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
//! person to use each control in turn. The test modes find the pad the
//! person presses among several ([`detect`]).
//!
//! # Features
//!
//! - `web` (default): the browser provider ([`web`]), active on
//!   `wasm32-unknown-unknown`.
//! - `native` (default): anygamepad's own OS backends ([`native`]); on
//!   Linux, the evdev and hidraw providers; on macOS, the IOKit HID and
//!   GameController providers.
//! - `testing`: the test runner of the apps' test modes (`testing`), which
//!   writes test reports.
//!
//! # Unsafe code
//!
//! Denied everywhere but in two places: the hidraw provider's feature
//! report ioctl (`HIDIOCGFEATURE`), which has no safe wrapper, and the
//! macOS providers (`native::macos`), whose IOKit and GameController
//! calls are all `unsafe` in their bindings. Each block says why it is
//! sound.
#![deny(unsafe_code)]

pub mod detect;
pub mod mapping;
#[cfg(feature = "native")]
pub mod native;
pub mod protocol;
pub mod provider;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(feature = "web")]
pub mod web;
pub mod wizard;

pub use anygamepad_db as db;
pub use anygamepad_model as model;

/// Version of the anygamepad library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
