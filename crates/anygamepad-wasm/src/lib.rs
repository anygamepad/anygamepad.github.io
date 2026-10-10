//! WebAssembly bindings of anygamepad, used by the web app.
//!
//! Built with `wasm-bindgen`; the full API is defined by roadmap task C11.

use wasm_bindgen::prelude::*;

/// Returns the version of the anygamepad library compiled into this module.
#[wasm_bindgen]
pub fn version() -> String {
    anygamepad::VERSION.to_owned()
}
