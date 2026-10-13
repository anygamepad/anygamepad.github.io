//! Browser provider (ADR 0002): the Gamepad API, on
//! `wasm32-unknown-unknown`.
//!
//! [`WebProvider`] implements [`Provider`](crate::provider::Provider) on top
//! of a [`GamepadSource`]. In a browser, [`WebProvider::new`] reads
//! `navigator.getGamepads()`. On other targets it fails, and only the
//! platform-independent parts work: user agent parsing ([`detect`]), the
//! [`standard`] layout and the provider logic over a source the caller
//! supplies.
//!
//! On wasm32, the `hid` module gives WebHID access (Chromium only).
//!
//! The Gamepad API is poll-based: call
//! [`Provider::update`](crate::provider::Provider::update) once per frame,
//! from `requestAnimationFrame`. Rust games compiled to WebAssembly use this
//! provider the same way as the web app.

mod browser;
#[cfg(target_arch = "wasm32")]
pub mod hid;
mod provider;
pub mod standard;
#[cfg(target_arch = "wasm32")]
mod sys;

pub use browser::detect;
pub use provider::{
    ButtonSnapshot, GamepadSnapshot, GamepadSource, HapticEffect, HapticKind, WebProvider,
};

#[cfg(not(target_arch = "wasm32"))]
impl WebProvider {
    /// The browser's gamepads. Outside WebAssembly there is no browser: this
    /// always fails with
    /// [`ProviderError::Unsupported`](crate::provider::ProviderError::Unsupported).
    pub fn new() -> Result<Self, crate::provider::ProviderError> {
        Err(crate::provider::ProviderError::Unsupported)
    }
}
