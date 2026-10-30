//! The native provider (ADR 0002): anygamepad's own backends for each OS.
//!
//! - [`evdev`]: Linux input devices, sans-IO (testable on any OS).
//! - [`linux`]: [`LinuxProvider`](linux::LinuxProvider), on Linux.
//!
//! Windows (C5) and macOS (C6) come later.

pub mod evdev;
#[cfg(target_os = "linux")]
pub mod linux;
