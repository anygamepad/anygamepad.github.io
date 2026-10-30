//! A descriptor parsed, its reports read and mapped to the canonical model
//! with a profile that names elements by HID usage.

use anygamepad::db::{AxisMapping, Mapping};
use anygamepad::mapping::Mapper;
use anygamepad::model::raw::RawState;
use anygamepad::model::{Axis, Button, GamepadState};
use anygamepad_hid::{Descriptor, RawReader};

/// A game pad without report IDs: 4 buttons, 4 bits of padding, then X and
/// Y from 0 to 255.
const GAME_PAD: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, //
    0x05, 0x09, 0x19, 0x01, 0x29, 0x04, //
    0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x04, 0x81, 0x02, //
    0x95, 0x04, 0x81, 0x01, //
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, //
    0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, //
    0xc0,
];

#[test]
fn reports_map_to_the_canonical_model() -> Result<(), Box<dyn std::error::Error>> {
    let descriptor = Descriptor::parse(GAME_PAD)?;
    let reader = RawReader::new(&descriptor);

    let mut mapping = Mapping::default();
    mapping
        .buttons
        .insert(Button::South, "b{hid:0x0009:0x0002}".parse()?);
    mapping.axes.insert(
        Axis::LeftStickX,
        AxisMapping::full("a{hid:0x0001:0x0030}".parse()?),
    );
    mapping.axes.insert(
        Axis::LeftStickY,
        AxisMapping::full("a{hid:0x0001:0x0031}~".parse()?),
    );
    let mapper = Mapper::new(&mapping, reader.layout());
    assert!(mapper.unresolved().is_empty());

    let mut raw = RawState::for_layout(reader.layout());
    let mut state = GamepadState::new();
    // Button 2 pressed, X all the way right, Y all the way up.
    assert!(reader.read(&[0b0010, 0xff, 0x00], &mut raw));
    mapper.apply(&raw, &mut state);
    assert!(state.pressed(Button::South));
    assert_eq!(state.axis(Axis::LeftStickX), 1.0);
    assert_eq!(state.axis(Axis::LeftStickY), 1.0);
    Ok(())
}
