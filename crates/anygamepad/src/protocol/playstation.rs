//! Sony PlayStation controllers: the input reports of the DualShock 4, the
//! DualSense and the DualSense Edge (C15).
//!
//! [`decode`] turns one input report into the canonical state: buttons,
//! sticks, triggers, touchpad and battery, plus the motion sensors as raw
//! counts ([`RawImu`]). Converting motion to SI units needs the pad's
//! calibration, which is C7's.
//!
//! # Report forms
//!
//! Lengths are without the report ID, as WebHID gives the data.
//!
//! | Pad | USB | Bluetooth, reduced | Bluetooth, full |
//! |-----|-----|--------------------|-----------------|
//! | DualShock 4 | `0x01`, 63 bytes | `0x01`, 9 bytes | `0x11`, 77 bytes, CRC-32 |
//! | DualSense, DualSense Edge | `0x01`, 63 bytes | `0x01`, 9 bytes | `0x31`, 77 bytes, CRC-32 |
//!
//! Bluetooth pads start with the reduced report: sticks, buttons and
//! triggers only. Reading a feature report, such as [`CALIBRATION_REPORT`],
//! switches a pad to the full report until it is power-cycled, for every
//! application on the host, and generic HID readers (DirectInput games, the
//! Gamepad API in some browsers) then get no input. macOS switches the
//! DualSense itself.
//!
//! # Sources
//!
//! `docs/research/09-dualsense-hid-protocol.md` (from a WebHID tester), the
//! Linux `hid-playstation` driver for the DualShock 4 battery and touch
//! layout, and SDL for the reduced reports. Nothing here has been checked
//! against reports recorded from a real pad yet. Not decoded: the DualSense
//! Edge's function and back buttons, whose bits are not confirmed.

use core::fmt;

use anygamepad_model::hid::HidCollection;
use anygamepad_model::{
    Axis, Button, Family, GamepadState, Power, PowerState, TouchPoint, TouchpadInfo, Transport,
};

use super::crc32;

/// A PlayStation controller model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Model {
    /// DualShock 4 (both versions).
    DualShock4,
    /// DualSense.
    DualSense,
    /// DualSense Edge.
    DualSenseEdge,
}

impl Model {
    /// The model of a controller family, if it is a PlayStation one.
    pub const fn from_family(family: Family) -> Option<Self> {
        match family {
            Family::DualShock4 => Some(Self::DualShock4),
            Family::DualSense => Some(Self::DualSense),
            Family::DualSenseEdge => Some(Self::DualSenseEdge),
            _ => None,
        }
    }

    /// The model of a USB vendor and product ID.
    pub const fn from_ids(vendor_id: u16, product_id: u16) -> Option<Self> {
        match Family::from_ids(vendor_id, product_id) {
            Some(family) => Self::from_family(family),
            None => None,
        }
    }

    /// The controller family.
    pub const fn family(self) -> Family {
        match self {
            Self::DualShock4 => Family::DualShock4,
            Self::DualSense => Family::DualSense,
            Self::DualSenseEdge => Family::DualSenseEdge,
        }
    }

    /// The touchpad: two fingers, its resolution in device units.
    pub const fn touchpad(self) -> TouchpadInfo {
        TouchpadInfo {
            max_fingers: 2,
            width: 1920,
            height: match self {
                Self::DualShock4 => 942,
                Self::DualSense | Self::DualSenseEdge => 1080,
            },
        }
    }
}

/// The feature report holding the motion calibration. Reading it over
/// Bluetooth switches the pad to the full report (see the
/// [module documentation](self)).
pub const CALIBRATION_REPORT: u8 = 0x05;

/// Which form of input report a pad sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportForm {
    /// USB: always the full report.
    Usb,
    /// Bluetooth, before the switch: no touchpad, motion or battery.
    BluetoothReduced,
    /// Bluetooth, after the switch.
    BluetoothFull,
}

impl ReportForm {
    /// The transport this form implies.
    pub const fn transport(self) -> Transport {
        match self {
            Self::Usb => Transport::Usb,
            Self::BluetoothReduced | Self::BluetoothFull => Transport::Bluetooth,
        }
    }
}

/// Motion sensor readings as the pad reports them, before calibration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawImu {
    /// Gyroscope, in report order (the pad's X, Y, Z).
    pub gyro: [i16; 3],
    /// Accelerometer, in report order (the pad's X, Y, Z).
    pub accel: [i16; 3],
    /// The pad's sensor clock: 32 bits on the DualSense, 16 bits on the
    /// DualShock 4. Only differences are meaningful.
    pub timestamp: u32,
}

/// One decoded input report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    /// The report's form, which also gives the transport.
    pub form: ReportForm,
    /// Buttons, sticks, triggers, touch points and power. Motion is left
    /// out (see [`Input::imu`]).
    pub state: GamepadState,
    /// Motion sensors, in full reports.
    pub imu: Option<RawImu>,
}

/// Why a report was not decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not an input report this model sends, or too short.
    UnknownReport {
        /// The report ID.
        report_id: u8,
        /// The data length, without the report ID.
        len: usize,
    },
    /// A Bluetooth report whose CRC-32 does not match: corrupted.
    Checksum,
    /// A report that carries no controller state (DualSense microphone
    /// audio, which shares the full report's ID).
    NotState,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownReport { report_id, len } => {
                write!(f, "unknown input report 0x{report_id:02x} of {len} bytes")
            }
            Self::Checksum => f.write_str("input report checksum mismatch"),
            Self::NotState => f.write_str("input report without controller state"),
        }
    }
}

impl std::error::Error for DecodeError {}

const USB_LEN: usize = 63;
const REDUCED_LEN: usize = 9;
const BLUETOOTH_LEN: usize = 77;
/// The Bluetooth HID header of an input report (DATA | Input), which the
/// CRC-32 covers before the report ID.
const INPUT_CRC_SEED: u8 = 0xa1;
/// Low nibble of a DualSense Bluetooth full report's first byte for audio.
const AUDIO_PAYLOAD: u8 = 0x02;

/// Decodes the input report `report_id` of a `model` pad; `data` is the
/// report without its ID.
pub fn decode(model: Model, report_id: u8, data: &[u8]) -> Result<Input, DecodeError> {
    let bluetooth_id = match model {
        Model::DualShock4 => 0x11,
        Model::DualSense | Model::DualSenseEdge => 0x31,
    };
    if report_id == 0x01 && data.len() >= USB_LEN {
        return Ok(full(model, ReportForm::Usb, data));
    }
    if report_id == 0x01 && data.len() >= REDUCED_LEN {
        return Ok(reduced(data));
    }
    if report_id != bluetooth_id || data.len() < BLUETOOTH_LEN {
        return Err(DecodeError::UnknownReport {
            report_id,
            len: data.len(),
        });
    }
    let (body, rest) = data.split_at(BLUETOOTH_LEN - 4);
    if model != Model::DualShock4 && body[0] & 0x0f == AUDIO_PAYLOAD {
        return Err(DecodeError::NotState);
    }
    let crc = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
    if crc32(&[INPUT_CRC_SEED, report_id], body) != crc {
        return Err(DecodeError::Checksum);
    }
    // The common layout starts after a tag byte (DualSense) or two flag
    // bytes (DualShock 4).
    let skip = if model == Model::DualShock4 { 2 } else { 1 };
    Ok(full(model, ReportForm::BluetoothFull, &body[skip..]))
}

/// The transport of a WebHID device, which WebHID does not report, from the
/// size of its largest input report in its Game Pad collection: 504 bits on
/// USB, 616 bits for the DualSense on Bluetooth.
pub fn transport_from_collections(model: Model, collections: &[HidCollection]) -> Transport {
    let bits = collections
        .iter()
        .filter(|collection| collection.usage_page == 0x01 && collection.usage == 0x05)
        .flat_map(|collection| &collection.input_reports)
        .map(|report| {
            report
                .items
                .iter()
                .map(|item| u32::from(item.report_size) * u32::from(item.report_count))
                .sum::<u32>()
        })
        .max();
    match (model, bits) {
        (_, Some(504)) => Transport::Usb,
        (Model::DualShock4, Some(_)) => Transport::Bluetooth,
        (_, Some(616)) => Transport::Bluetooth,
        _ => Transport::Unknown,
    }
}

/// The reduced Bluetooth report: sticks, three button bytes, triggers.
fn reduced(data: &[u8]) -> Input {
    let mut state = GamepadState::new();
    sticks(&mut state, &data[0..4]);
    // The third byte's upper bits are a counter, not the mute button.
    buttons(&mut state, [data[4], data[5], data[6]], false);
    triggers(&mut state, data[7], data[8]);
    Input {
        form: ReportForm::BluetoothReduced,
        state,
        imu: None,
    }
}

/// A full report, from the start of its common layout.
fn full(model: Model, form: ReportForm, d: &[u8]) -> Input {
    let mut state = GamepadState::new();
    sticks(&mut state, &d[0..4]);
    let imu = match model {
        Model::DualSense | Model::DualSenseEdge => {
            triggers(&mut state, d[4], d[5]);
            buttons(&mut state, [d[7], d[8], d[9]], true);
            touches(&mut state, model.touchpad(), &d[32..40]);
            state.power = dualsense_power(d[52]);
            RawImu {
                gyro: [i16_at(d, 15), i16_at(d, 17), i16_at(d, 19)],
                accel: [i16_at(d, 21), i16_at(d, 23), i16_at(d, 25)],
                timestamp: u32::from_le_bytes([d[27], d[28], d[29], d[30]]),
            }
        }
        Model::DualShock4 => {
            buttons(&mut state, [d[4], d[5], d[6]], false);
            triggers(&mut state, d[7], d[8]);
            // d[32] counts touch reports; the first one is a timestamp byte
            // and two points.
            touches(&mut state, model.touchpad(), &d[34..42]);
            state.power = dualshock4_power(d[29]);
            RawImu {
                gyro: [i16_at(d, 12), i16_at(d, 14), i16_at(d, 16)],
                accel: [i16_at(d, 18), i16_at(d, 20), i16_at(d, 22)],
                timestamp: u32::from(u16::from_le_bytes([d[9], d[10]])),
            }
        }
    };
    Input {
        form,
        state,
        imu: Some(imu),
    }
}

fn i16_at(data: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([data[at], data[at + 1]])
}

/// Sticks are 0-255, centre 128, Y growing downwards; canonical Y grows
/// upwards (ADR 0001).
fn sticks(state: &mut GamepadState, raw: &[u8]) {
    let axis = |value: u8| f32::from(value) / 127.5 - 1.0;
    state.set_axis(Axis::LeftStickX, axis(raw[0]));
    state.set_axis(Axis::LeftStickY, -axis(raw[1]));
    state.set_axis(Axis::RightStickX, axis(raw[2]));
    state.set_axis(Axis::RightStickY, -axis(raw[3]));
}

fn triggers(state: &mut GamepadState, left: u8, right: u8) {
    state.set_axis(Axis::LeftTrigger, f32::from(left) / 255.0);
    state.set_axis(Axis::RightTrigger, f32::from(right) / 255.0);
}

/// The three button bytes: D-pad hat and face buttons, shoulders and menus,
/// system buttons. `mute` reads the DualSense's mute button.
fn buttons(state: &mut GamepadState, bytes: [u8; 3], mute: bool) {
    // Hat: 0 up, then clockwise to 7 up-left; 8 and above released.
    let hat = bytes[0] & 0x0f;
    state.set_pressed(Button::DPadUp, matches!(hat, 0 | 1 | 7));
    state.set_pressed(Button::DPadRight, matches!(hat, 1..=3));
    state.set_pressed(Button::DPadDown, matches!(hat, 3..=5));
    state.set_pressed(Button::DPadLeft, matches!(hat, 5..=7));
    let bits: [(usize, u8, Button); 15] = [
        (0, 0x10, Button::West),
        (0, 0x20, Button::South),
        (0, 0x40, Button::East),
        (0, 0x80, Button::North),
        (1, 0x01, Button::LeftShoulder),
        (1, 0x02, Button::RightShoulder),
        (1, 0x04, Button::LeftTrigger),
        (1, 0x08, Button::RightTrigger),
        (1, 0x10, Button::Select),
        (1, 0x20, Button::Start),
        (1, 0x40, Button::LeftStick),
        (1, 0x80, Button::RightStick),
        (2, 0x01, Button::Home),
        (2, 0x02, Button::TouchpadClick),
        (2, 0x04, Button::Mute),
    ];
    for (byte, mask, button) in bits {
        if button == Button::Mute && !mute {
            continue;
        }
        state.set_pressed(button, bytes[byte] & mask != 0);
    }
}

/// Two touch points of 4 bytes: bit 7 of the first byte set when no finger
/// is down, its low bits the finger's id, then X and Y on 12 bits each.
fn touches(state: &mut GamepadState, touchpad: TouchpadInfo, raw: &[u8]) {
    let scale = |value: u16, size: u16| (f32::from(value) / f32::from(size - 1)).clamp(0.0, 1.0);
    for (slot, point) in raw.chunks_exact(4).enumerate() {
        if point[0] & 0x80 != 0 {
            continue;
        }
        let x = u16::from(point[1]) | (u16::from(point[2] & 0x0f) << 8);
        let y = u16::from(point[2] >> 4) | (u16::from(point[3]) << 4);
        state.touches[slot] = Some(TouchPoint {
            touchpad: 0,
            id: point[0] & 0x7f,
            x: scale(x, touchpad.width),
            y: scale(y, touchpad.height),
        });
    }
}

/// Low nibble: level 0-10; high nibble: 0 on battery, 1 charging, 2 full,
/// other values a fault.
fn dualsense_power(status: u8) -> Power {
    let percent = ((status & 0x0f).min(10) * 10 + 5).min(100);
    match status >> 4 {
        0 => Power {
            state: PowerState::OnBattery,
            percent: Some(percent),
        },
        1 => Power {
            state: PowerState::Charging,
            percent: Some(percent),
        },
        2 => Power {
            state: PowerState::Charged,
            percent: Some(100),
        },
        _ => Power::default(),
    }
}

/// Low nibble: level (0-10, 11 full on a cable); bit 4: cable connected.
fn dualshock4_power(status: u8) -> Power {
    let level = status & 0x0f;
    let percent = (level.min(10) * 10 + 5).min(100);
    if status & 0x10 == 0 {
        return Power {
            state: PowerState::OnBattery,
            percent: Some(percent),
        };
    }
    match level {
        0..=10 => Power {
            state: PowerState::Charging,
            percent: Some(percent),
        },
        11 => Power {
            state: PowerState::Charged,
            percent: Some(100),
        },
        _ => Power::default(),
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::hid::{HidReportInfo, HidReportItem};

    use super::*;

    /// Writes the common DualSense layout into `d`: Cross, D-pad up, L1,
    /// Options, PS, touchpad click and mute pressed, one finger down,
    /// charging at 50-55%.
    fn dualsense_common(d: &mut [u8]) {
        d[0..4].copy_from_slice(&[0, 255, 128, 64]);
        d[4] = 255; // L2
        d[5] = 0; // R2
        d[7] = 0x20; // hat 0 (up) + Cross
        d[8] = 0x01 | 0x20; // L1 + Options
        d[9] = 0x01 | 0x02 | 0x04; // PS + touchpad + mute
        d[15..17].copy_from_slice(&(-100i16).to_le_bytes());
        d[21..23].copy_from_slice(&8192i16.to_le_bytes());
        d[27..31].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        // Finger 5 at (959, 540); second slot inactive.
        d[32..36].copy_from_slice(&[0x05, 0xbf, 0xc3, 0x21]);
        d[36] = 0x80;
        d[52] = 0x15; // charging, level 5
    }

    fn check_dualsense(input: &Input) {
        let state = &input.state;
        for button in [
            Button::South,
            Button::DPadUp,
            Button::LeftShoulder,
            Button::Start,
            Button::Home,
            Button::TouchpadClick,
            Button::Mute,
        ] {
            assert!(state.pressed(button), "{button:?}");
        }
        assert_eq!(state.buttons.len(), 7);
        assert_eq!(state.axis(Axis::LeftStickX), -1.0);
        assert_eq!(state.axis(Axis::LeftStickY), -1.0);
        assert!(state.axis(Axis::RightStickX).abs() < 0.01);
        assert!((state.axis(Axis::RightStickY) - 0.498).abs() < 0.01);
        assert_eq!(state.axis(Axis::LeftTrigger), 1.0);
        assert_eq!(state.axis(Axis::RightTrigger), 0.0);
        let touches: Vec<_> = state.active_touches().collect();
        assert_eq!(touches.len(), 1);
        assert_eq!(touches[0].id, 5);
        assert!((touches[0].x - 0.5).abs() < 0.001);
        assert!((touches[0].y - 0.5).abs() < 0.001);
        assert_eq!(state.power.state, PowerState::Charging);
        assert_eq!(state.power.percent, Some(55));
        let imu = input.imu.expect("full report");
        assert_eq!(imu.gyro, [-100, 0, 0]);
        assert_eq!(imu.accel, [8192, 0, 0]);
        assert_eq!(imu.timestamp, 0x0102_0304);
    }

    /// A Bluetooth full report: `body` after `skip` header bytes, then the
    /// CRC-32 over the header, the report ID and the body.
    fn bluetooth(report_id: u8, header: &[u8], common: impl Fn(&mut [u8])) -> Vec<u8> {
        let mut data = vec![0u8; BLUETOOTH_LEN];
        data[..header.len()].copy_from_slice(header);
        common(&mut data[header.len()..BLUETOOTH_LEN - 4]);
        let crc = crc32(&[INPUT_CRC_SEED, report_id], &data[..BLUETOOTH_LEN - 4]);
        data[BLUETOOTH_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        data
    }

    #[test]
    fn models() {
        assert_eq!(Model::from_ids(0x054c, 0x0ce6), Some(Model::DualSense));
        assert_eq!(Model::from_ids(0x054c, 0x09cc), Some(Model::DualShock4));
        assert_eq!(Model::from_ids(0x057e, 0x2009), None);
        assert_eq!(
            Model::from_family(Family::DualSenseEdge),
            Some(Model::DualSenseEdge)
        );
        assert_eq!(Model::DualSense.family(), Family::DualSense);
        assert_eq!(Model::DualShock4.touchpad().height, 942);
    }

    #[test]
    fn dualsense_usb() {
        let mut data = [0u8; USB_LEN];
        dualsense_common(&mut data);
        let input = decode(Model::DualSense, 0x01, &data).unwrap();
        assert_eq!(input.form, ReportForm::Usb);
        assert_eq!(input.form.transport(), Transport::Usb);
        check_dualsense(&input);
    }

    #[test]
    fn dualsense_bluetooth_full() {
        let data = bluetooth(0x31, &[0x01], dualsense_common);
        let input = decode(Model::DualSenseEdge, 0x31, &data).unwrap();
        assert_eq!(input.form, ReportForm::BluetoothFull);
        check_dualsense(&input);

        let mut corrupted = data.clone();
        corrupted[10] ^= 0x01;
        assert_eq!(
            decode(Model::DualSense, 0x31, &corrupted),
            Err(DecodeError::Checksum)
        );

        let audio = bluetooth(0x31, &[0x02], |_| {});
        assert_eq!(
            decode(Model::DualSense, 0x31, &audio),
            Err(DecodeError::NotState)
        );
    }

    #[test]
    fn reduced_reports() {
        // Sticks, buttons (hat 5 = down-left, Triangle; R2; PS + touchpad,
        // with counter bits), triggers.
        let data = [128, 128, 255, 0, 0x85, 0x08, 0xa7, 10, 200];
        for model in [Model::DualSense, Model::DualShock4] {
            let input = decode(model, 0x01, &data).unwrap();
            assert_eq!(input.form, ReportForm::BluetoothReduced);
            assert_eq!(input.form.transport(), Transport::Bluetooth);
            let state = input.state;
            for button in [
                Button::DPadDown,
                Button::DPadLeft,
                Button::North,
                Button::RightTrigger,
                Button::Home,
                Button::TouchpadClick,
            ] {
                assert!(state.pressed(button), "{model:?} {button:?}");
            }
            // Bit 2 of the third byte is the counter here.
            assert!(!state.pressed(Button::Mute));
            assert_eq!(state.buttons.len(), 6);
            assert_eq!(state.axis(Axis::RightStickX), 1.0);
            assert_eq!(state.axis(Axis::RightStickY), 1.0);
            assert!((state.axis(Axis::RightTrigger) - 200.0 / 255.0).abs() < 1e-6);
            assert_eq!(state.active_touches().count(), 0);
            assert_eq!(state.power, Power::default());
            assert_eq!(input.imu, None);
        }
    }

    #[test]
    fn hat_directions() {
        let pressed = |hat: u8| {
            let data = [128, 128, 128, 128, hat, 0, 0, 0, 0];
            let state = decode(Model::DualSense, 0x01, &data).unwrap().state;
            [
                Button::DPadUp,
                Button::DPadRight,
                Button::DPadDown,
                Button::DPadLeft,
            ]
            .map(|button| state.pressed(button))
        };
        assert_eq!(pressed(0), [true, false, false, false]);
        assert_eq!(pressed(1), [true, true, false, false]);
        assert_eq!(pressed(4), [false, false, true, false]);
        assert_eq!(pressed(7), [true, false, false, true]);
        assert_eq!(pressed(8), [false; 4]);
        assert_eq!(pressed(15), [false; 4]);
    }

    /// The DualShock 4 layout: Square, L2 digital, Share and the touchpad
    /// pressed; finger 2 at the far right; on a cable, full.
    fn dualshock4_common(d: &mut [u8]) {
        d[0..4].copy_from_slice(&[128, 128, 128, 128]);
        d[4] = 0x18; // hat 8 (released) + Square
        d[5] = 0x04 | 0x10; // L2 + Share
        d[6] = 0x02 | 0xfc; // touchpad + counter bits
        d[7] = 128; // L2 travel
        d[9..11].copy_from_slice(&0x1234u16.to_le_bytes());
        d[12..14].copy_from_slice(&7i16.to_le_bytes());
        d[22..24].copy_from_slice(&(-8192i16).to_le_bytes());
        d[29] = 0x1b; // cable + level 11
        d[32] = 1;
        d[34..38].copy_from_slice(&[0x02, 0x7f, 0x07, 0x00]);
        d[38] = 0x80;
    }

    fn check_dualshock4(input: &Input) {
        let state = &input.state;
        for button in [
            Button::West,
            Button::LeftTrigger,
            Button::Select,
            Button::TouchpadClick,
        ] {
            assert!(state.pressed(button), "{button:?}");
        }
        assert_eq!(state.buttons.len(), 4);
        assert!((state.axis(Axis::LeftTrigger) - 128.0 / 255.0).abs() < 1e-6);
        let touch = state.active_touches().next().unwrap();
        assert_eq!((touch.id, touch.x, touch.y), (2, 1.0, 0.0));
        assert_eq!(state.power.state, PowerState::Charged);
        assert_eq!(state.power.percent, Some(100));
        let imu = input.imu.unwrap();
        assert_eq!(imu.gyro, [7, 0, 0]);
        assert_eq!(imu.accel, [0, 0, -8192]);
        assert_eq!(imu.timestamp, 0x1234);
    }

    #[test]
    fn dualshock4_usb_and_bluetooth() {
        let mut data = [0u8; USB_LEN];
        dualshock4_common(&mut data);
        let input = decode(Model::DualShock4, 0x01, &data).unwrap();
        assert_eq!(input.form, ReportForm::Usb);
        check_dualshock4(&input);

        let data = bluetooth(0x11, &[0xc0, 0x00], dualshock4_common);
        let input = decode(Model::DualShock4, 0x11, &data).unwrap();
        assert_eq!(input.form, ReportForm::BluetoothFull);
        check_dualshock4(&input);
        // The DualSense's Bluetooth ID is not the DualShock 4's.
        assert!(matches!(
            decode(Model::DualShock4, 0x31, &data),
            Err(DecodeError::UnknownReport {
                report_id: 0x31,
                len: 77
            })
        ));
    }

    #[test]
    fn power() {
        assert_eq!(dualsense_power(0x08).percent, Some(85));
        assert_eq!(dualsense_power(0x0a).percent, Some(100));
        assert_eq!(dualsense_power(0x20).state, PowerState::Charged);
        assert_eq!(dualsense_power(0xf3), Power::default());
        assert_eq!(dualshock4_power(0x03).state, PowerState::OnBattery);
        assert_eq!(dualshock4_power(0x13).state, PowerState::Charging);
        assert_eq!(dualshock4_power(0x1e), Power::default());
    }

    #[test]
    fn unknown_reports() {
        assert_eq!(
            decode(Model::DualSense, 0x01, &[0; 8]),
            Err(DecodeError::UnknownReport {
                report_id: 1,
                len: 8
            })
        );
        assert_eq!(
            decode(Model::DualSense, 0x31, &[0; 40]),
            Err(DecodeError::UnknownReport {
                report_id: 0x31,
                len: 40
            })
        );
        assert_eq!(
            DecodeError::UnknownReport {
                report_id: 0x31,
                len: 40
            }
            .to_string(),
            "unknown input report 0x31 of 40 bytes"
        );
    }

    #[test]
    fn transport_from_report_sizes() {
        let collection = |bits: &[(u16, u16)]| HidCollection {
            usage_page: 0x01,
            usage: 0x05,
            kind: 1,
            input_reports: vec![HidReportInfo {
                report_id: 1,
                items: bits
                    .iter()
                    .map(|&(report_size, report_count)| HidReportItem {
                        report_size,
                        report_count,
                        ..HidReportItem::default()
                    })
                    .collect(),
            }],
            ..HidCollection::default()
        };
        let usb = [collection(&[(8, 63)])];
        let bluetooth = [collection(&[(8, 9), (8, 68)])];
        let other = [collection(&[(8, 10)])];
        assert_eq!(
            transport_from_collections(Model::DualSense, &usb),
            Transport::Usb
        );
        assert_eq!(
            transport_from_collections(Model::DualSense, &bluetooth),
            Transport::Bluetooth
        );
        assert_eq!(
            transport_from_collections(Model::DualSense, &other),
            Transport::Unknown
        );
        assert_eq!(
            transport_from_collections(Model::DualShock4, &other),
            Transport::Bluetooth
        );
        assert_eq!(
            transport_from_collections(Model::DualSense, &[]),
            Transport::Unknown
        );
    }
}
