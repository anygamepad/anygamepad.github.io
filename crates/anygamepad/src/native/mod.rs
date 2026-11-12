//! The native provider (ADR 0002): anygamepad's own backends for each OS.
//!
//! - [`evdev`]: Linux input devices, sans-IO (testable on any OS).
//! - `linux`: `LinuxProvider`, on Linux.
//! - `hidraw`: `HidrawProvider`, on Linux: the pads the protocol drivers
//!   drive through raw HID.
//! - [`hid`]: [`HidPad`](hid::HidPad), a pad read through a native raw HID
//!   API, sans-IO.
//! - [`iokit`] and [`game_controller`]: what the macOS providers read,
//!   sans-IO (testable on any OS).
//! - `macos`: `IoHidProvider` and `GameControllerProvider`, on macOS.
//!
//! Windows (C5) comes later.

pub mod evdev;
pub mod game_controller;
pub mod hid;
#[cfg(target_os = "linux")]
pub mod hidraw;
pub mod iokit;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
