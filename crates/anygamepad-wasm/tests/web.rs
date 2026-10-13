//! Tests that run in a headless browser through `wasm-bindgen-test`.
//!
//! Headless browsers have no gamepads: the provider logic is tested natively
//! in `anygamepad::web`; these tests check the browser glue and the
//! JavaScript form of the values.
#![cfg(target_arch = "wasm32")]

use anygamepad::model::identity::{Browser, BrowserEngine, DeviceIdentity, Os};
use anygamepad::model::{Axis, Button, GamepadState};
use anygamepad_wasm::Gamepads;
use js_sys::{Array, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).unwrap()
}

#[wasm_bindgen_test]
fn version_is_exported() {
    assert_eq!(anygamepad_wasm::version(), env!("CARGO_PKG_VERSION"));
}

#[wasm_bindgen_test]
fn gamepads_start_empty() {
    let mut gamepads = Gamepads::new().unwrap();
    let events = gamepads.update().unwrap();
    assert_eq!(events.dyn_into::<Array>().unwrap().length(), 0);
    assert!(gamepads.devices().is_empty());
    // A second provider works alongside the first.
    let mut other = Gamepads::new().unwrap();
    assert!(other.update().is_ok());
}

#[wasm_bindgen_test]
fn environment_comes_from_the_user_agent() {
    let gamepads = Gamepads::new().unwrap();
    let environment = gamepads.environment().unwrap();
    let browser = get(&environment, "browser");
    let engine = get(&browser, "engine").as_string().unwrap();
    assert!(
        ["blink", "gecko", "webkit"].contains(&engine.as_str()),
        "{engine}"
    );
    assert!(get(&browser, "major_version").as_f64().unwrap() > 0.0);
    let os = get(&environment, "os").as_string().unwrap();
    assert_ne!(os, "unknown");

    let navigator = get(&js_sys::global(), "navigator");
    let user_agent = get(&navigator, "userAgent").as_string().unwrap();
    let (detected, _) = anygamepad::web::detect(&user_agent, 0);
    assert_eq!(detected.engine.as_str(), engine);
}

#[wasm_bindgen_test]
fn unknown_devices_are_refused() {
    let mut gamepads = Gamepads::new().unwrap();
    gamepads.update().unwrap();
    assert!(gamepads.info(1).unwrap().is_undefined());
    assert!(gamepads.snapshot(1).unwrap().is_undefined());
    assert!(gamepads.open(1, None).is_err());
    assert!(gamepads.poll(1).is_err());
    assert!(gamepads.state(1).is_err());
    let rumble = js_sys::JSON::parse(r#"{"rumble":{"low_frequency":1,"high_frequency":0}}"#);
    assert!(gamepads.send(1, rumble.unwrap()).is_err());
    assert!(gamepads.send(1, JsValue::from_str("rumble")).is_err());
    gamepads.close(1);
}

#[wasm_bindgen_test]
fn values_have_their_declared_javascript_form() {
    // The bindings convert with `serde_wasm_bindgen::to_value`; the `.d.ts`
    // declares these shapes.
    let mut state = GamepadState::new();
    state.set_pressed(Button::South, true);
    state.set_axis(Axis::LeftStickY, 1.0);
    let js = serde_wasm_bindgen::to_value(&state).unwrap();
    // `Record<Axis, number>`: a plain object, not a `Map`.
    let axes = get(&js, "axes");
    assert!(!axes.is_instance_of::<js_sys::Map>());
    assert_eq!(get(&axes, "left_stick_y").as_f64(), Some(1.0));
    let buttons = get(&js, "buttons").dyn_into::<Array>().unwrap();
    assert_eq!(buttons.to_vec(), [JsValue::from_str("south")]);
    assert!(get(&js, "motion").is_undefined());
    assert_eq!(get(&js, "touches").dyn_into::<Array>().unwrap().length(), 4);

    let browser = Browser {
        engine: BrowserEngine::Gecko,
        major_version: Some(143),
    };
    let identity =
        DeviceIdentity::from_gamepad_api("054c-09cc-Wireless Controller", "", browser, Os::Linux);
    let js = serde_wasm_bindgen::to_value(&identity).unwrap();
    assert_eq!(get(&js, "backend").as_string().unwrap(), "gamepad_api");
    assert_eq!(get(&js, "vendor_id").as_f64(), Some(f64::from(0x054c)));
    assert!(get(&js, "descriptor_hash").is_undefined());
    assert_eq!(get(&get(&js, "web"), "mapping").as_string().unwrap(), "");
}
