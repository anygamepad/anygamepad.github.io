//! Serde representation: names in human-readable formats, indices in binary
//! formats.
#![cfg(feature = "serde")]

use anygamepad_model::{
    Axis, AxisSet, Button, ButtonSet, Capabilities, FaceLayout, GamepadState, MotionSample, Output,
    Sensor, TouchPoint, TouchpadInfo, Vec3,
};

fn sample_capabilities() -> Capabilities {
    let mut caps = Capabilities::standard();
    caps.buttons.insert(Button::TouchpadClick);
    caps.buttons.insert(Button::extra(2).unwrap());
    caps.sensors.insert(Sensor::Gyroscope);
    caps.outputs.insert(Output::Lightbar);
    caps.touchpads.push(TouchpadInfo {
        max_fingers: 2,
        width: 1920,
        height: 943,
    });
    caps.battery = true;
    caps
}

fn sample_state() -> GamepadState {
    let mut state = GamepadState::new();
    state.set_pressed(Button::East, true);
    state.set_pressed(Button::extra(31).unwrap(), true);
    state.set_axis(Axis::RightStickY, -0.5);
    state.touches[0] = Some(TouchPoint {
        touchpad: 0,
        id: 3,
        x: 0.5,
        y: 0.25,
    });
    state.motion = Some(MotionSample {
        timestamp_us: 123_456,
        gyro: Some(Vec3::new(0.1, -0.2, 0.3)),
        accel: None,
    });
    state
}

#[test]
fn json_uses_snake_case_names() {
    assert_eq!(
        serde_json::to_string(&Button::DPadUp).unwrap(),
        r#""dpad_up""#
    );
    assert_eq!(
        serde_json::to_string(&Button::extra(4).unwrap()).unwrap(),
        r#""extra_4""#
    );
    assert_eq!(
        serde_json::to_string(&AxisSet::from([Axis::LeftStickX, Axis::RightTrigger])).unwrap(),
        r#"["left_stick_x","right_trigger"]"#
    );
    assert_eq!(
        serde_json::to_string(&FaceLayout::NINTENDO).unwrap(),
        r#"{"south":"b","east":"a","west":"y","north":"x"}"#
    );
}

#[test]
fn json_rejects_unknown_names() {
    let error = serde_json::from_str::<Button>(r#""a""#).unwrap_err();
    assert!(error.to_string().contains("button name"), "{error}");
    assert!(serde_json::from_str::<ButtonSet>(r#"["south","jump"]"#).is_err());
}

#[test]
fn json_round_trip() {
    let caps = sample_capabilities();
    let json = serde_json::to_string(&caps).unwrap();
    assert_eq!(serde_json::from_str::<Capabilities>(&json).unwrap(), caps);

    let state = sample_state();
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<GamepadState>(&json).unwrap(), state);
}

#[test]
fn json_state_axes_are_keyed_by_name() {
    let json = serde_json::to_value(sample_state()).unwrap();
    let axes = json["axes"].as_object().unwrap();
    assert_eq!(axes.len(), Axis::COUNT);
    assert_eq!(axes["right_stick_y"], -0.5);
    assert_eq!(axes["left_trigger"], 0.0);
    assert_eq!(json["buttons"], serde_json::json!(["east", "extra_31"]));

    // Missing axes are at rest, values are clamped, unknown names fail.
    let state: GamepadState = serde_json::from_value(serde_json::json!({
        "buttons": [],
        "axes": { "left_stick_x": 2.0, "right_trigger": -1.0 },
        "touches": [null, null, null, null],
        "motion": null,
        "power": { "state": "unknown", "percent": null },
    }))
    .unwrap();
    assert_eq!(state.axis(Axis::LeftStickX), 1.0);
    assert_eq!(state.axis(Axis::RightTrigger), 0.0);
    assert_eq!(state.axis(Axis::LeftStickY), 0.0);
    let mut json = serde_json::to_value(sample_state()).unwrap();
    json["axes"]["throttle"] = 1.0.into();
    assert!(serde_json::from_value::<GamepadState>(json).is_err());
}

#[test]
fn postcard_round_trip_is_compact() {
    let caps = sample_capabilities();
    let bytes = postcard::to_allocvec(&caps).unwrap();
    assert_eq!(postcard::from_bytes::<Capabilities>(&bytes).unwrap(), caps);
    // Sets are stored as bits, not names.
    assert!(bytes.len() < 32, "{} bytes", bytes.len());

    let state = sample_state();
    let bytes = postcard::to_allocvec(&state).unwrap();
    assert_eq!(postcard::from_bytes::<GamepadState>(&bytes).unwrap(), state);

    let button = Button::extra(5).unwrap();
    let bytes = postcard::to_allocvec(&button).unwrap();
    assert_eq!(bytes, [37]);
    assert_eq!(postcard::from_bytes::<Button>(&bytes).unwrap(), button);
}

#[test]
fn postcard_rejects_unknown_bits() {
    // Bit 25 is between the standard and the extra buttons.
    let bytes = postcard::to_allocvec(&(1u64 << 25)).unwrap();
    assert!(postcard::from_bytes::<ButtonSet>(&bytes).is_err());
    assert!(postcard::from_bytes::<Button>(&[25]).is_err());
    assert!(postcard::from_bytes::<Axis>(&[Axis::COUNT as u8]).is_err());
}

#[cfg(feature = "schemars")]
#[test]
fn json_schema_lists_names() {
    let schema = schemars::schema_for!(Capabilities);
    let json = serde_json::to_value(&schema).unwrap();
    let text = json.to_string();
    for name in [
        "south",
        "touchpad_click",
        "left_stick_x",
        "gyroscope",
        "lightbar",
    ] {
        assert!(text.contains(&format!("\"{name}\"")), "{name} missing");
    }
    assert!(text.contains("^extra_([0-9]|[12][0-9]|3[01])$"));
    let button = &json["$defs"]["Button"];
    assert_eq!(button["type"], "string");
    assert_eq!(button["anyOf"][0]["enum"].as_array().unwrap().len(), 25);

    let schema = serde_json::to_value(schemars::schema_for!(GamepadState)).unwrap();
    let axes = &schema["properties"]["axes"];
    assert_eq!(axes["type"], "object");
    assert_eq!(axes["additionalProperties"], false);
    assert_eq!(axes["properties"]["left_stick_y"]["type"], "number");
    assert_eq!(axes["properties"].as_object().unwrap().len(), Axis::COUNT);
}

mod identity {
    use anygamepad_model::identity::{
        Backend, Browser, BrowserEngine, DescriptorHash, DeviceIdentity, Fingerprint, Os,
        ProviderKind,
    };
    use anygamepad_model::raw::{Hat, RawAxisInfo, RawLayout, RawSource, RawState};
    use anygamepad_model::sdl::SdlGuid;
    use anygamepad_model::{Button, OutputCommand, Rgb, Transport};

    fn identity() -> DeviceIdentity {
        let browser = Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(141),
        };
        let mut identity = DeviceIdentity::from_gamepad_api(
            "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)",
            "standard",
            browser,
            Os::Windows,
        );
        identity.transport = Transport::Bluetooth;
        identity.descriptor_hash = Some(DescriptorHash(u64::MAX));
        identity
    }

    #[test]
    fn identity_json() {
        let identity = identity();
        let json = serde_json::to_value(&identity).unwrap();
        assert_eq!(json["provider"], "web");
        assert_eq!(json["backend"], "gamepad_api");
        assert_eq!(json["os"], "windows");
        assert_eq!(json["transport"], "bluetooth");
        assert_eq!(json["vendor_id"], 0x054c);
        assert_eq!(json["browser"]["engine"], "blink");
        // 64-bit values are strings, so JavaScript reads them exactly.
        assert_eq!(json["descriptor_hash"], "ffffffffffffffff");
        assert_eq!(
            serde_json::from_value::<DeviceIdentity>(json).unwrap(),
            identity
        );

        let bytes = postcard::to_allocvec(&identity).unwrap();
        assert_eq!(
            postcard::from_bytes::<DeviceIdentity>(&bytes).unwrap(),
            identity
        );
    }

    #[test]
    fn fingerprint_and_guid_strings() {
        let fingerprint = identity().fingerprint();
        let json = serde_json::to_string(&fingerprint).unwrap();
        assert_eq!(json, format!("\"{fingerprint}\""));
        assert_eq!(
            serde_json::from_str::<Fingerprint>(&json).unwrap(),
            fingerprint
        );

        let guid: SdlGuid = "030000004c050000c405000011010000".parse().unwrap();
        let json = serde_json::to_string(&guid).unwrap();
        assert_eq!(json, "\"030000004c050000c405000011010000\"");
        assert_eq!(serde_json::from_str::<SdlGuid>(&json).unwrap(), guid);
        assert!(serde_json::from_str::<SdlGuid>("\"0300\"").is_err());
        let bytes = postcard::to_allocvec(&guid).unwrap();
        assert_eq!(postcard::from_bytes::<SdlGuid>(&bytes).unwrap(), guid);
    }

    #[test]
    fn raw_layout_and_state_json() {
        let layout = RawLayout {
            buttons: vec![RawSource::Evdev(0x130), RawSource::Button(Button::South)],
            axes: vec![RawAxisInfo {
                source: RawSource::Hid {
                    page: 0x01,
                    usage: 0x30,
                },
                min: 0.0,
                max: 255.0,
            }],
            hats: vec![RawSource::Index(0)],
            ..RawLayout::default()
        };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["buttons"][0]["evdev"], 0x130);
        assert_eq!(json["buttons"][1]["button"], "south");
        assert_eq!(json["axes"][0]["source"]["hid"]["usage"], 0x30);
        assert_eq!(serde_json::from_value::<RawLayout>(json).unwrap(), layout);

        let mut state = RawState::for_layout(&layout);
        state.buttons[0] = 1.0;
        state.hats[0] = Hat::from_hid(2);
        let bytes = postcard::to_allocvec(&state).unwrap();
        assert_eq!(postcard::from_bytes::<RawState>(&bytes).unwrap(), state);
    }

    #[test]
    fn output_command_json() {
        let command = OutputCommand::Lightbar(Rgb {
            r: 255,
            g: 0,
            b: 64,
        });
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"lightbar":{"r":255,"g":0,"b":64}}"#
        );
        assert_eq!(
            serde_json::to_string(&OutputCommand::PlayerNumber(2)).unwrap(),
            r#"{"player_number":2}"#
        );
    }

    #[test]
    fn provider_and_backend_names() {
        assert_eq!(
            serde_json::to_string(&ProviderKind::Sdl3).unwrap(),
            "\"sdl3\""
        );
        assert_eq!(
            serde_json::from_str::<Backend>("\"windows_hid\"").unwrap(),
            Backend::WindowsHid
        );
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn identity_schema() {
        let schema = schemars::schema_for!(DeviceIdentity);
        let text = serde_json::to_string(&schema).unwrap();
        for name in [
            "gamepad_api",
            "hidraw",
            "blink",
            "macos",
            "^[0-9a-fA-F]{16}$",
        ] {
            assert!(text.contains(name), "{name} missing");
        }
        let schema = schemars::schema_for!(RawLayout);
        assert!(serde_json::to_string(&schema).unwrap().contains("evdev"));
    }
}
