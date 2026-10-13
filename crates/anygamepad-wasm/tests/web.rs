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
    assert_eq!(gamepads.timestamp(1), None);
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

mod hid {
    use anygamepad::model::hid::{HidItemFlag, HidUnitSystem, HidUsageRange};
    use anygamepad::web::hid::parse_collections;
    use anygamepad_wasm::HidDevices;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::*;

    /// `HIDDevice.collections` of a game pad, as Chromium describes it.
    const COLLECTIONS: &str = r#"[{
        "usagePage": 1, "usage": 5, "type": 1,
        "children": [{
            "usagePage": 1, "usage": 1, "type": 0, "children": [],
            "inputReports": [{ "reportId": 1, "items": [{
                "usages": [65584, 65585], "isRange": false,
                "reportSize": 8, "reportCount": 2,
                "logicalMinimum": 0, "logicalMaximum": 255,
                "isAbsolute": true, "isArray": false, "isConstant": false,
                "isLinear": true, "hasPreferredState": true, "hasNull": false,
                "unitSystem": "none", "unitExponent": 0
            }]}],
            "outputReports": [], "featureReports": []
        }],
        "inputReports": [{ "reportId": 1, "items": [
            {
                "usages": [], "isRange": true,
                "usageMinimum": 589825, "usageMaximum": 589838,
                "reportSize": 1, "reportCount": 14,
                "logicalMinimum": 0, "logicalMaximum": 1,
                "isAbsolute": true, "isArray": false, "isLinear": true,
                "hasPreferredState": true
            },
            {
                "usages": [65593], "isRange": false,
                "reportSize": 4, "reportCount": 1,
                "logicalMinimum": 0, "logicalMaximum": 7,
                "physicalMinimum": 0, "physicalMaximum": 315,
                "isAbsolute": true, "isArray": false, "isLinear": true,
                "hasPreferredState": true, "hasNull": true,
                "unitSystem": "english-rotation", "unitExponent": 0
            },
            { "reportSize": 2, "reportCount": 1, "isConstant": true, "isArray": true,
              "isAbsolute": true, "isLinear": true, "hasPreferredState": true }
        ]}],
        "outputReports": [{ "reportId": 5, "items": [] }],
        "featureReports": []
    }]"#;

    #[wasm_bindgen_test]
    fn collections_are_parsed() {
        let collections = parse_collections(&js_sys::JSON::parse(COLLECTIONS).unwrap());
        assert_eq!(collections.len(), 1);
        let gamepad = &collections[0];
        assert_eq!((gamepad.usage_page, gamepad.usage, gamepad.kind), (1, 5, 1));
        assert_eq!(gamepad.output_reports[0].report_id, 5);

        let items = &gamepad.input_reports[0].items;
        assert_eq!(gamepad.input_reports[0].report_id, 1);
        assert_eq!(
            items[0].usage_range,
            Some(HidUsageRange {
                minimum: 0x0009_0001,
                maximum: 0x0009_000e
            })
        );
        assert_eq!((items[0].report_size, items[0].report_count), (1, 14));
        assert_eq!(
            items[0].flags.iter().collect::<Vec<_>>(),
            [HidItemFlag::Variable]
        );

        let hat = &items[1];
        assert_eq!(hat.usages, [0x0001_0039]);
        assert_eq!(hat.usage_range, None);
        assert_eq!(hat.physical_maximum, 315);
        assert_eq!(hat.unit_system, HidUnitSystem::EnglishRotation);
        assert!(hat.flags.contains(HidItemFlag::NullState));
        assert_eq!(hat.flags.bits(), 0x42);

        let padding = &items[2];
        assert_eq!(
            padding.flags.iter().collect::<Vec<_>>(),
            [HidItemFlag::Constant]
        );
        assert_eq!(padding.unit_system, HidUnitSystem::None);

        let sticks = &gamepad.children[0].input_reports[0].items[0];
        assert_eq!(sticks.usages, [0x0001_0030, 0x0001_0031]);
        assert_eq!(sticks.logical_maximum, 255);
    }

    #[wasm_bindgen_test]
    fn malformed_collections_read_as_empty_or_zero() {
        let parsed = |json: &str| parse_collections(&js_sys::JSON::parse(json).unwrap());
        assert!(parsed("null").is_empty());
        assert!(parsed(r#"{"usage": 5}"#).is_empty());
        let odd = parsed(r#"[{"usagePage": "x", "usage": 1e9, "children": 3}]"#);
        assert_eq!((odd[0].usage_page, odd[0].usage), (0, u16::MAX));
        assert!(odd[0].children.is_empty());
    }

    #[wasm_bindgen_test]
    async fn devices_need_permission() {
        if !HidDevices::is_supported() {
            // Firefox and Safari.
            assert!(HidDevices::new().is_err());
            return;
        }
        let mut hid = HidDevices::new().unwrap();
        hid.update().unwrap();
        assert!(hid.devices().is_empty());
        assert!(hid.info(1).unwrap().is_undefined());
        assert!(hid.reports(1).is_err());
        assert_eq!(hid.dropped_reports(1), None);
        assert!(JsFuture::from(hid.open(1)).await.is_err());
        assert!(
            JsFuture::from(hid.send_report(1, 0, &[1, 2]))
                .await
                .is_err()
        );
        // Without a user gesture the browser refuses to show its chooser.
        let request = hid.request(wasm_bindgen::JsValue::UNDEFINED).unwrap();
        assert!(JsFuture::from(request).await.is_err());
        // Bad filters fail before reaching the browser.
        let filters = js_sys::JSON::parse(r#"[{"vendor_id": "sony"}]"#).unwrap();
        assert!(hid.request(filters).is_err());
    }
}
