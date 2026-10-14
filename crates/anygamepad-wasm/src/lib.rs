//! WebAssembly bindings of anygamepad, used by the web app (ADR 0005).
//!
//! [`Gamepads`] exposes the library's web provider to JavaScript, and
//! `HidDevices` (wasm32 only) its WebHID access. Values are
//! converted with serde-wasm-bindgen, and the generated `.d.ts` declares
//! their TypeScript types (through tsify), so the frontend never writes the
//! canonical model by hand.
//!
//! ```js
//! import init, { Gamepads } from "anygamepad-wasm";
//!
//! await init();
//! const gamepads = new Gamepads();
//! function frame() {
//!   for (const event of gamepads.update()) {
//!     if (event.type === "added") gamepads.open(event.device);
//!   }
//!   for (const device of gamepads.devices()) {
//!     const state = gamepads.state(device); // canonical, standard layout only
//!     const raw = gamepads.poll(device); // raw Gamepad API values
//!   }
//!   requestAnimationFrame(frame);
//! }
//! requestAnimationFrame(frame);
//! ```

use wasm_bindgen::prelude::*;

mod gamepads;
#[cfg(target_arch = "wasm32")]
mod hid_devices;

pub use gamepads::{
    ButtonSnapshot, CanonicalLayout, DeviceEvent, DeviceInfo, Environment, GamepadSnapshot,
    Gamepads,
};
#[cfg(target_arch = "wasm32")]
pub use hid_devices::{
    DecodedInput, HidDeviceInfo, HidDevices, HidFilter, HidInputReport, HidPoll, RawImu, ReportForm,
};

/// Returns the version of the anygamepad library compiled into this module.
#[wasm_bindgen]
pub fn version() -> String {
    anygamepad::VERSION.to_owned()
}
