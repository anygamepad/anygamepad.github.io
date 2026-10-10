//! Tests that run in a headless browser through `wasm-bindgen-test`.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn version_is_exported() {
    assert_eq!(anygamepad_wasm::version(), env!("CARGO_PKG_VERSION"));
}
