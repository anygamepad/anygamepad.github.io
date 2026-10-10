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
}
