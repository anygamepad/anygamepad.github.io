//! The wired Xbox 360 Controller.
//!
//! It is not a HID device: its USB interface is vendor-specific, which
//! Windows drives through XInput and Linux through `xpad`. Its input report
//! is the 20-byte packet of its interrupt endpoint, whose payload is the
//! XInput gamepad state that ViGEmBus takes (`XUSB_REPORT`); a uinput
//! output turns the same state into `xpad`'s evdev events.
//!
//! Sources: `xpad` (note 05) and the XInput `XINPUT_GAMEPAD` layout. Not
//! checked against a recorded pad.

use anygamepad_model::{
    Axis, Button, Capabilities, GamepadState, Output, OutputCommand, OutputSet, Rumble,
};

use crate::{Target, VirtualIdentity, VirtualPad, motor, trigger_byte};

pub(crate) const IDENTITY: VirtualIdentity = VirtualIdentity {
    vendor_id: 0x045e,
    product_id: 0x028e,
    version: 0x0114,
    name: "Microsoft X-Box 360 pad",
};

/// Length of the input packet: type, length, 18 bytes of state.
const INPUT_LEN: usize = 20;

/// `wButtons` bits of the XInput state.
const BUTTONS: [(u16, Button); 15] = [
    (0x0001, Button::DPadUp),
    (0x0002, Button::DPadDown),
    (0x0004, Button::DPadLeft),
    (0x0008, Button::DPadRight),
    (0x0010, Button::Start),
    (0x0020, Button::Select),
    (0x0040, Button::LeftStick),
    (0x0080, Button::RightStick),
    (0x0100, Button::LeftShoulder),
    (0x0200, Button::RightShoulder),
    (0x0400, Button::Home),
    (0x1000, Button::South),
    (0x2000, Button::East),
    (0x4000, Button::West),
    (0x8000, Button::North),
];

/// An emulated Xbox 360 Controller.
#[derive(Clone, Debug)]
pub struct Xbox360 {
    report: [u8; INPUT_LEN],
}

impl Xbox360 {
    /// A pad with nothing pressed.
    pub fn new() -> Self {
        let mut report = [0; INPUT_LEN];
        report[1] = INPUT_LEN as u8;
        Self { report }
    }
}

impl Default for Xbox360 {
    fn default() -> Self {
        Self::new()
    }
}

/// A stick axis in `[-1, 1]` to XInput's signed 16 bits, Y up as in the
/// canonical model.
fn stick(value: f32) -> [u8; 2] {
    let value = (value.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    value.to_le_bytes()
}

impl VirtualPad for Xbox360 {
    fn target(&self) -> Target {
        Target::Xbox360
    }

    fn report_descriptor(&self) -> Option<&[u8]> {
        None
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            outputs: OutputSet::from([Output::Rumble, Output::PlayerLeds]),
            ..Capabilities::standard()
        }
    }

    fn input_report(&mut self, state: &GamepadState) -> &[u8] {
        let buttons = BUTTONS
            .iter()
            .filter(|(_, button)| state.pressed(*button))
            .fold(0u16, |bits, (mask, _)| bits | mask);
        let r = &mut self.report;
        r[2..4].copy_from_slice(&buttons.to_le_bytes());
        r[4] = trigger_byte(state.axis(Axis::LeftTrigger));
        r[5] = trigger_byte(state.axis(Axis::RightTrigger));
        r[6..8].copy_from_slice(&stick(state.axis(Axis::LeftStickX)));
        r[8..10].copy_from_slice(&stick(state.axis(Axis::LeftStickY)));
        r[10..12].copy_from_slice(&stick(state.axis(Axis::RightStickX)));
        r[12..14].copy_from_slice(&stick(state.axis(Axis::RightStickY)));
        &self.report
    }

    /// Two packets: rumble (`00 08 00 <strong> <weak> ...`) and the ring
    /// light (`01 03 <pattern>`), whose patterns 2-5 (blink, then on) and
    /// 6-9 (on) show players 1-4, and 0 turns it off. The other patterns
    /// (rotating, blinking) show no player number.
    fn output_report(&mut self, report: &[u8]) -> Vec<OutputCommand> {
        match report {
            [0x00, 0x08, _, strong, weak, ..] => vec![OutputCommand::Rumble(Rumble {
                low_frequency: motor(*strong),
                high_frequency: motor(*weak),
            })],
            [0x01, 0x03, pattern, ..] => match pattern {
                0 => vec![OutputCommand::PlayerNumber(0)],
                2..=5 => vec![OutputCommand::PlayerNumber(pattern - 1)],
                6..=9 => vec![OutputCommand::PlayerNumber(pattern - 5)],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn feature_report(&mut self, _report_id: u8) -> Option<Vec<u8>> {
        None
    }
}

/// An evdev axis of the uinput pad: code, minimum, maximum, fuzz, flat.
pub type EvdevAxis = (u16, i32, i32, i32, i32);

/// Key codes of the uinput pad, as `xpad` reports a wired Xbox 360
/// Controller: `BTN_A`, `BTN_B`, `BTN_X`, `BTN_Y` (the X button, west, is
/// `BTN_X`, code `0x133`), shoulders, menus, guide, stick clicks.
pub const EVDEV_KEYS: [u16; 11] = [
    0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x13a, 0x13b, 0x13c, 0x13d, 0x13e,
];

/// Absolute axes of the uinput pad, with `xpad`'s ranges: sticks
/// (`ABS_X/Y`, `ABS_RX/RY`), triggers (`ABS_Z/RZ`), d-pad (`ABS_HAT0X/Y`).
pub const EVDEV_AXES: [EvdevAxis; 8] = [
    (0x00, -32768, 32767, 16, 128),
    (0x01, -32768, 32767, 16, 128),
    (0x03, -32768, 32767, 16, 128),
    (0x04, -32768, 32767, 16, 128),
    (0x02, 0, 255, 0, 0),
    (0x05, 0, 255, 0, 0),
    (0x10, -1, 1, 0, 0),
    (0x11, -1, 1, 0, 0),
];

/// The evdev events `xpad` sends for an input packet ([`Xbox360`]'s
/// report): `(type, code, value)`, `EV_KEY` then `EV_ABS`, without the
/// closing `SYN_REPORT`. Y axes grow downwards, as `xpad` reports them.
pub fn evdev_events(report: &[u8]) -> Vec<(u16, u16, i32)> {
    const EV_KEY: u16 = 0x01;
    const EV_ABS: u16 = 0x03;
    if report.len() < INPUT_LEN {
        return Vec::new();
    }
    let low = report[2];
    let high = report[3];
    let bit = |byte: u8, mask: u8| i32::from(byte & mask != 0);
    let stick = |at: usize| i32::from(i16::from_le_bytes([report[at], report[at + 1]]));
    vec![
        (EV_KEY, 0x130, bit(high, 0x10)), // A
        (EV_KEY, 0x131, bit(high, 0x20)), // B
        (EV_KEY, 0x133, bit(high, 0x40)), // X
        (EV_KEY, 0x134, bit(high, 0x80)), // Y
        (EV_KEY, 0x136, bit(high, 0x01)), // LB
        (EV_KEY, 0x137, bit(high, 0x02)), // RB
        (EV_KEY, 0x13c, bit(high, 0x04)), // guide
        (EV_KEY, 0x13b, bit(low, 0x10)),  // start
        (EV_KEY, 0x13a, bit(low, 0x20)),  // back
        (EV_KEY, 0x13d, bit(low, 0x40)),  // left stick
        (EV_KEY, 0x13e, bit(low, 0x80)),  // right stick
        (EV_ABS, 0x00, stick(6)),
        (EV_ABS, 0x01, !stick(8)),
        (EV_ABS, 0x03, stick(10)),
        (EV_ABS, 0x04, !stick(12)),
        (EV_ABS, 0x02, i32::from(report[4])),
        (EV_ABS, 0x05, i32::from(report[5])),
        (EV_ABS, 0x10, bit(low, 0x08) - bit(low, 0x04)),
        (EV_ABS, 0x11, bit(low, 0x02) - bit(low, 0x01)),
    ]
}

#[cfg(test)]
mod tests {
    use anygamepad_model::AxisSet;

    use super::*;

    #[test]
    fn input_packet() {
        let mut pad = Xbox360::new();
        assert_eq!(
            pad.input_report(&GamepadState::new())[..6],
            [0, 20, 0, 0, 0, 0]
        );

        let mut state = GamepadState::new();
        state.set_pressed(Button::South, true);
        state.set_pressed(Button::DPadLeft, true);
        state.set_pressed(Button::Home, true);
        state.set_axis(Axis::LeftStickX, -1.0);
        state.set_axis(Axis::LeftStickY, 1.0);
        state.set_axis(Axis::RightStickY, -0.5);
        state.set_axis(Axis::RightTrigger, 1.0);
        let report = pad.input_report(&state);
        assert_eq!(report.len(), 20);
        assert_eq!(report[2..4], [0x04, 0x14]);
        assert_eq!(report[4..6], [0, 255]);
        assert_eq!(i16::from_le_bytes([report[6], report[7]]), -32767);
        assert_eq!(i16::from_le_bytes([report[8], report[9]]), 32767);
        assert_eq!(i16::from_le_bytes([report[12], report[13]]), -16384);
        assert_eq!(report[14..], [0; 6]);
    }

    #[test]
    fn every_button_has_its_own_bit() {
        let bits = BUTTONS.iter().fold(0u16, |bits, (mask, _)| {
            assert_eq!(bits & mask, 0);
            bits | mask
        });
        assert_eq!(bits, 0xf7ff);
        let pad = Xbox360::new();
        let buttons = BUTTONS.iter().map(|(_, button)| *button);
        assert!(buttons.clone().all(|b| pad.capabilities().has_button(b)));
        assert_eq!(pad.capabilities().buttons.len(), BUTTONS.len() + 2);
        assert_eq!(pad.capabilities().axes, AxisSet::ALL);
    }

    #[test]
    fn output_packets() {
        let mut pad = Xbox360::new();
        assert_eq!(
            pad.output_report(&[0x00, 0x08, 0x00, 0xff, 0x00, 0, 0, 0]),
            [OutputCommand::Rumble(Rumble {
                low_frequency: 1.0,
                high_frequency: 0.0
            })]
        );
        assert_eq!(
            pad.output_report(&[0x01, 0x03, 0x07]),
            [OutputCommand::PlayerNumber(2)]
        );
        assert_eq!(
            pad.output_report(&[0x01, 0x03, 0x02]),
            [OutputCommand::PlayerNumber(1)]
        );
        assert_eq!(
            pad.output_report(&[0x01, 0x03, 0x00]),
            [OutputCommand::PlayerNumber(0)]
        );
        assert!(pad.output_report(&[0x01, 0x03, 0x0a]).is_empty());
        assert!(pad.output_report(&[0x02]).is_empty());
        assert!(pad.output_report(&[]).is_empty());
        assert_eq!(pad.feature_report(0x01), None);
        assert_eq!(pad.report_descriptor(), None);
    }

    #[test]
    fn evdev_events_like_xpad() {
        let mut pad = Xbox360::new();
        let mut state = GamepadState::new();
        state.set_pressed(Button::West, true);
        state.set_pressed(Button::DPadUp, true);
        state.set_pressed(Button::DPadRight, true);
        state.set_axis(Axis::LeftStickY, 1.0);
        state.set_axis(Axis::LeftTrigger, 1.0);
        let events = evdev_events(pad.input_report(&state));
        let value = |kind: u16, code: u16| {
            events
                .iter()
                .find(|(k, c, _)| *k == kind && *c == code)
                .map(|e| e.2)
        };
        // West is BTN_X.
        assert_eq!(value(1, 0x133), Some(1));
        assert_eq!(value(1, 0x134), Some(0));
        // Up is negative.
        assert_eq!(value(3, 0x01), Some(-32768));
        assert_eq!(value(3, 0x02), Some(255));
        assert_eq!((value(3, 0x10), value(3, 0x11)), (Some(1), Some(-1)));
        for (code, min, max, _, _) in EVDEV_AXES {
            let v = value(3, code).unwrap();
            assert!((min..=max).contains(&v));
        }
        let keys: Vec<u16> = events.iter().filter(|e| e.0 == 1).map(|e| e.1).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, EVDEV_KEYS);
        assert!(evdev_events(&[0; 4]).is_empty());
    }
}
