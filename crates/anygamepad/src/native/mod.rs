//! The native provider (ADR 0002): anygamepad's own backends for each OS.
//!
//! - [`evdev`]: Linux input devices, sans-IO (testable on any OS).
//! - [`linux`]: [`LinuxProvider`](linux::LinuxProvider), on Linux.
//! - [`hidraw`]: [`HidrawProvider`](hidraw::HidrawProvider), the pads the
//!   protocol drivers drive through Linux raw HID.
//!
//! Windows (C5) and macOS (C6) come later.

pub mod evdev;
#[cfg(target_os = "linux")]
pub mod hidraw;
#[cfg(target_os = "linux")]
pub mod linux;
