//! The DualShock 4 and the DualSense, in their USB form.
//!
//! Input reports follow the layout the `anygamepad` crate decodes (C15,
//! `protocol::playstation`), and the tests decode every report with it.
//! Report descriptors have the real pads' structure (the same reports, IDs
//! and sizes, sticks and triggers on the same usages, hat and buttons in
//! the same bits) without being byte-identical to them.
//!
//! Motion: the pads report raw counts that the host scales with the
//! calibration feature report. Every virtual pad reports the same nominal
//! calibration ([`GYRO_COUNTS_PER_DEGREE`], [`ACCEL_COUNTS_PER_G`]), and
//! scales the canonical SI values with it.
//!
//! Sources: research note 09, `hid-playstation` (note 05) for the
//! calibration, pairing and firmware feature reports and the output report
//! fields. Not checked against a real pad or the Linux driver yet (A5).

use anygamepad_model::{
    Axis, AxisSet, Button, ButtonSet, Capabilities, GamepadState, Output, OutputCommand, OutputSet,
    PowerState, Rgb, Rumble, STANDARD_GRAVITY, Sensor, SensorSet, TouchpadInfo, Vec3,
};

use crate::descriptor::{
    BUTTON, Builder, GAME_PAD, GENERIC_DESKTOP, HAT_SWITCH, RX, RY, RZ, ReportKind, VARIABLE,
    VARIABLE_NULL, VENDOR, X, Y, Z,
};
use crate::{Target, VirtualIdentity, VirtualPad, motor, trigger_byte, unsigned_byte, virtual_mac};

pub(crate) const DUALSHOCK4_IDENTITY: VirtualIdentity = VirtualIdentity {
    vendor_id: 0x054c,
    product_id: 0x09cc,
    version: 0x0100,
    name: "Sony Interactive Entertainment Wireless Controller",
};

pub(crate) const DUALSENSE_IDENTITY: VirtualIdentity = VirtualIdentity {
    vendor_id: 0x054c,
    product_id: 0x0ce6,
    version: 0x0100,
    name: "Sony Interactive Entertainment DualSense Wireless Controller",
};

/// Gyroscope counts per degree per second of the nominal calibration
/// (±2000 °/s on 16 bits).
pub const GYRO_COUNTS_PER_DEGREE: f32 = 16.0;
/// Accelerometer counts per standard gravity of the nominal calibration
/// (±4 g on 16 bits).
pub const ACCEL_COUNTS_PER_G: f32 = 8192.0;

/// Gyroscope speed, in °/s, that the calibration report pairs with
/// `GYRO_COUNTS_PER_DEGREE` times it.
const CALIBRATION_SPEED: i16 = 540;

/// Input report: ID and data length (USB).
const INPUT_ID: u8 = 0x01;
const INPUT_LEN: usize = 63;

/// Touchpads, as C15 decodes them.
const DUALSHOCK4_TOUCHPAD: TouchpadInfo = TouchpadInfo {
    max_fingers: 2,
    width: 1920,
    height: 942,
};
const DUALSENSE_TOUCHPAD: TouchpadInfo = TouchpadInfo {
    max_fingers: 2,
    width: 1920,
    height: 1080,
};

/// Feature reports of the DualShock 4: calibration, pairing information
/// (the Bluetooth address), firmware information. Lengths without the ID.
const DS4_CALIBRATION: (u8, u32) = (0x02, 36);
const DS4_PAIRING: (u8, u32) = (0x12, 15);
const DS4_FIRMWARE: (u8, u32) = (0xa3, 48);
/// Output report of the DualShock 4.
const DS4_OUTPUT: (u8, u32) = (0x05, 31);

/// Feature reports of the DualSense, as for the DualShock 4.
const DS_CALIBRATION: (u8, u32) = (0x05, 40);
const DS_PAIRING: (u8, u32) = (0x09, 19);
const DS_FIRMWARE: (u8, u32) = (0x20, 63);
/// Output report of the DualSense.
const DS_OUTPUT: (u8, u32) = (0x02, 47);

/// Player LED patterns of the DualSense for players 1-5 (`hid-playstation`).
const PLAYER_LEDS: [u8; 5] = [0x04, 0x0a, 0x15, 0x1b, 0x1f];

/// Bits of the three button bytes after the hat (C15's table).
const BUTTONS: [(usize, u8, Button); 15] = [
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

/// What tells the two pads apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    DualShock4,
    DualSense,
}

/// The state shared by both pads.
#[derive(Clone, Debug)]
struct Pad {
    model: Model,
    index: u8,
    descriptor: Vec<u8>,
    report: [u8; 1 + INPUT_LEN],
    counter: u8,
}

impl Pad {
    fn new(model: Model, index: u8) -> Self {
        let mut report = [0; 1 + INPUT_LEN];
        report[0] = INPUT_ID;
        Self {
            model,
            index,
            descriptor: descriptor(model),
            report,
            counter: 0,
        }
    }

    fn touchpad(&self) -> TouchpadInfo {
        match self.model {
            Model::DualShock4 => DUALSHOCK4_TOUCHPAD,
            Model::DualSense => DUALSENSE_TOUCHPAD,
        }
    }

    fn capabilities(&self) -> Capabilities {
        let mut buttons = Capabilities::standard().buttons;
        buttons.insert(Button::TouchpadClick);
        let mut outputs = OutputSet::from([Output::Rumble, Output::Lightbar]);
        if self.model == Model::DualSense {
            buttons.insert(Button::Mute);
            outputs.insert(Output::PlayerLeds);
        }
        Capabilities {
            buttons,
            axes: AxisSet::ALL,
            sensors: SensorSet::from([Sensor::Gyroscope, Sensor::Accelerometer]),
            touchpads: vec![self.touchpad()],
            outputs,
            battery: true,
        }
    }

    fn input_report(&mut self, state: &GamepadState) -> &[u8] {
        let model = self.model;
        let touchpad = self.touchpad();
        self.counter = self.counter.wrapping_add(1);
        let counter = self.counter;
        let d = &mut self.report[1..];
        d.fill(0);
        d[0] = unsigned_byte(state.axis(Axis::LeftStickX));
        d[1] = unsigned_byte(-state.axis(Axis::LeftStickY));
        d[2] = unsigned_byte(state.axis(Axis::RightStickX));
        d[3] = unsigned_byte(-state.axis(Axis::RightStickY));
        let left = trigger_byte(state.axis(Axis::LeftTrigger));
        let right = trigger_byte(state.axis(Axis::RightTrigger));
        let mut buttons = buttons(state.buttons);
        let motion = state.motion.unwrap_or_default();
        let gyro = motion.gyro.unwrap_or_default();
        // A pad lying flat and still when the state has no motion.
        let accel = motion
            .accel
            .unwrap_or(Vec3::new(0.0, STANDARD_GRAVITY, 0.0));
        match model {
            Model::DualSense => {
                d[4] = left;
                d[5] = right;
                d[6] = counter;
                d[7..10].copy_from_slice(&buttons);
                imu(&mut d[15..27], gyro, accel);
                // The sensor clock counts thirds of a microsecond.
                let clock = motion.timestamp_us.wrapping_mul(3) as u32;
                d[27..31].copy_from_slice(&clock.to_le_bytes());
                touches(&mut d[32..40], touchpad, state);
                d[52] = dualsense_power(state);
            }
            Model::DualShock4 => {
                // No mute button; the counter fills the upper six bits.
                buttons[2] = (buttons[2] & 0x03) | (counter << 2);
                d[4..7].copy_from_slice(&buttons);
                d[7] = left;
                d[8] = right;
                // The sensor clock counts units of 16/3 µs.
                let clock = (motion.timestamp_us.wrapping_mul(3) / 16) as u16;
                d[9..11].copy_from_slice(&clock.to_le_bytes());
                imu(&mut d[12..24], gyro, accel);
                d[29] = dualshock4_power(state);
                // One touch report: a timestamp byte and two points.
                d[32] = 1;
                d[33] = counter;
                touches(&mut d[34..42], touchpad, state);
            }
        }
        &self.report
    }

    fn output_report(&self, report: &[u8]) -> Vec<OutputCommand> {
        let (id, len) = match self.model {
            Model::DualShock4 => DS4_OUTPUT,
            Model::DualSense => DS_OUTPUT,
        };
        let Some((&report_id, data)) = report.split_first() else {
            return Vec::new();
        };
        if report_id != id || data.len() < len as usize {
            return Vec::new();
        }
        let mut commands = Vec::new();
        match self.model {
            Model::DualShock4 => {
                // Flags: bit 0 rumble, bit 1 light bar.
                if data[0] & 0x01 != 0 {
                    commands.push(OutputCommand::Rumble(Rumble {
                        low_frequency: motor(data[4]),
                        high_frequency: motor(data[3]),
                    }));
                }
                if data[0] & 0x02 != 0 {
                    let [r, g, b] = [data[5], data[6], data[7]];
                    commands.push(OutputCommand::Lightbar(Rgb { r, g, b }));
                }
            }
            Model::DualSense => {
                // Flags 0: bits 0-1 rumble (compatible or haptics mode).
                if data[0] & 0x03 != 0 {
                    commands.push(OutputCommand::Rumble(Rumble {
                        low_frequency: motor(data[3]),
                        high_frequency: motor(data[2]),
                    }));
                }
                // Flags 1: bit 2 light bar, bit 4 player LEDs.
                if data[1] & 0x04 != 0 {
                    let [r, g, b] = [data[44], data[45], data[46]];
                    commands.push(OutputCommand::Lightbar(Rgb { r, g, b }));
                }
                if data[1] & 0x10 != 0 {
                    // Bit 5 only makes the change instant.
                    let leds = data[43] & 0x1f;
                    let player = match leds {
                        0 => Some(0),
                        _ => PLAYER_LEDS
                            .iter()
                            .position(|&pattern| pattern == leds)
                            .map(|i| i as u8 + 1),
                    };
                    commands.extend(player.map(OutputCommand::PlayerNumber));
                }
            }
        }
        commands
    }

    fn feature_report(&self, report_id: u8) -> Option<Vec<u8>> {
        let (calibration, pairing, firmware) = match self.model {
            Model::DualShock4 => (DS4_CALIBRATION, DS4_PAIRING, DS4_FIRMWARE),
            Model::DualSense => (DS_CALIBRATION, DS_PAIRING, DS_FIRMWARE),
        };
        let (_, len) = [calibration, pairing, firmware]
            .into_iter()
            .find(|(id, _)| *id == report_id)?;
        let mut report = vec![0; 1 + len as usize];
        report[0] = report_id;
        if report_id == calibration.0 {
            write_calibration(&mut report[1..]);
        } else if report_id == pairing.0 {
            // The address comes least significant byte first.
            let mut mac = virtual_mac(self.index);
            mac.reverse();
            report[1..7].copy_from_slice(&mac);
        }
        // Firmware information stays zero: no version a driver could match
        // a quirk to.
        Some(report)
    }
}

/// The hat and the three button bytes (the mute bit included; the
/// DualShock 4 overwrites it with its counter).
fn buttons(pressed: ButtonSet) -> [u8; 3] {
    let up = pressed.contains(Button::DPadUp);
    let down = pressed.contains(Button::DPadDown);
    let left = pressed.contains(Button::DPadLeft);
    let right = pressed.contains(Button::DPadRight);
    // 0 up, then clockwise to 7 up-left; 8 released. Opposite directions
    // cancel out.
    let hat = match (up && !down, down && !up, left && !right, right && !left) {
        (true, _, false, false) => 0,
        (true, _, _, true) => 1,
        (false, false, false, true) => 2,
        (_, true, _, true) => 3,
        (_, true, false, false) => 4,
        (_, true, true, _) => 5,
        (false, false, true, _) => 6,
        (true, _, true, _) => 7,
        _ => 8,
    };
    let mut bytes = [hat, 0, 0];
    for (byte, mask, button) in BUTTONS {
        if pressed.contains(button) {
            bytes[byte] |= mask;
        }
    }
    bytes
}

/// Gyroscope then accelerometer, three little-endian `i16` each, in the
/// device frame (the pad's axes are the canonical ones).
fn imu(out: &mut [u8], gyro: Vec3, accel: Vec3) {
    let gyro_scale = GYRO_COUNTS_PER_DEGREE * 180.0 / core::f32::consts::PI;
    let accel_scale = ACCEL_COUNTS_PER_G / STANDARD_GRAVITY;
    let values = [
        gyro.x * gyro_scale,
        gyro.y * gyro_scale,
        gyro.z * gyro_scale,
        accel.x * accel_scale,
        accel.y * accel_scale,
        accel.z * accel_scale,
    ];
    for (chunk, value) in out.chunks_exact_mut(2).zip(values) {
        // `as` saturates, and NaN becomes 0.
        chunk.copy_from_slice(&(value.round() as i16).to_le_bytes());
    }
}

/// The nominal calibration (`hid-playstation`'s USB layout): gyroscope
/// biases, then the readings at plus and minus the calibration speed per
/// axis, the speeds, then the accelerometer readings at plus and minus one
/// g per axis. Little-endian `i16` each.
fn write_calibration(out: &mut [u8]) {
    let gyro = (f32::from(CALIBRATION_SPEED) * GYRO_COUNTS_PER_DEGREE) as i16;
    let accel = ACCEL_COUNTS_PER_G as i16;
    let values: [i16; 17] = [
        0,
        0,
        0, // biases
        gyro,
        -gyro,
        gyro,
        -gyro,
        gyro,
        -gyro, // pitch, yaw, roll
        CALIBRATION_SPEED,
        CALIBRATION_SPEED, // speeds
        accel,
        -accel,
        accel,
        -accel,
        accel,
        -accel, // X, Y, Z
    ];
    for (chunk, value) in out.chunks_exact_mut(2).zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
}

/// The first two fingers on the touchpad: 4 bytes each, bit 7 of the first
/// set when no finger is down, then X and Y on 12 bits.
fn touches(out: &mut [u8], touchpad: TouchpadInfo, state: &GamepadState) {
    let mut fingers = state.active_touches().filter(|touch| touch.touchpad == 0);
    let scale =
        |value: f32, size: u16| (value.clamp(0.0, 1.0) * f32::from(size - 1)).round() as u16;
    for point in out.chunks_exact_mut(4) {
        let Some(finger) = fingers.next() else {
            point[0] = 0x80;
            continue;
        };
        let x = scale(finger.x, touchpad.width);
        let y = scale(finger.y, touchpad.height);
        point[0] = finger.id & 0x7f;
        point[1] = x as u8;
        point[2] = ((x >> 8) as u8 & 0x0f) | ((y as u8 & 0x0f) << 4);
        point[3] = (y >> 4) as u8;
    }
}

/// Battery level 0-10 of a percentage.
fn level(percent: Option<u8>) -> u8 {
    percent.map_or(10, |p| p.min(100) / 10)
}

/// Level in the low nibble; high nibble 0 on battery, 1 charging, 2 full.
/// Without a battery state, a full pad on its cable.
fn dualsense_power(state: &GamepadState) -> u8 {
    let power = state.power;
    match power.state {
        PowerState::OnBattery => level(power.percent),
        PowerState::Charging => 0x10 | level(power.percent),
        _ => 0x20 | 10,
    }
}

/// Level in the low nibble (11 full on a cable); bit 4 cable connected.
fn dualshock4_power(state: &GamepadState) -> u8 {
    let power = state.power;
    match power.state {
        PowerState::OnBattery => level(power.percent),
        PowerState::Charging => 0x10 | level(power.percent),
        _ => 0x10 | 11,
    }
}

/// The report descriptor of a model (see the module documentation).
fn descriptor(model: Model) -> Vec<u8> {
    let mut b = Builder::new();
    b.usage_page(GENERIC_DESKTOP)
        .usage(GAME_PAD)
        .application()
        .report_id(INPUT_ID);
    // Sticks (and, on the DualSense, triggers) on one byte each.
    b.usage(X).usage(Y).usage(Z).usage(RZ);
    if model == Model::DualSense {
        b.usage(RX).usage(RY);
    }
    let axes = if model == Model::DualSense { 6 } else { 4 };
    b.logical(0, 255).report(8, axes).input(VARIABLE);
    if model == Model::DualSense {
        // Report counter.
        b.usage_page(VENDOR)
            .usage(0x20)
            .report(8, 1)
            .input(VARIABLE);
    }
    // Hat in degrees (English rotation), 8 positions.
    b.usage_page(GENERIC_DESKTOP)
        .usage(HAT_SWITCH)
        .logical(0, 7)
        .physical(0, 315)
        .unit(0x14)
        .report(4, 1)
        .input(VARIABLE_NULL)
        .unit(0)
        .physical(0, 0);
    let buttons = if model == Model::DualSense { 15 } else { 14 };
    b.usage_page(BUTTON)
        .usage_range(1, buttons)
        .logical(0, 1)
        .report(1, buttons.into())
        .input(VARIABLE);
    match model {
        Model::DualShock4 => {
            // Counter, then the triggers, then vendor data.
            b.usage_page(VENDOR)
                .usage(0x20)
                .logical(0, 63)
                .report(6, 1)
                .input(VARIABLE);
            b.usage_page(GENERIC_DESKTOP)
                .usage(RX)
                .usage(RY)
                .logical(0, 255)
                .report(8, 2)
                .input(VARIABLE);
            b.vendor_bytes(ReportKind::Input, INPUT_ID, 0x21, 54);
        }
        Model::DualSense => {
            b.usage_page(VENDOR)
                .usage(0x21)
                .logical(0, 1)
                .report(1, 13)
                .input(VARIABLE);
            b.vendor_bytes(ReportKind::Input, INPUT_ID, 0x22, 52);
        }
    }
    let (output, features) = match model {
        Model::DualShock4 => (DS4_OUTPUT, [DS4_CALIBRATION, DS4_PAIRING, DS4_FIRMWARE]),
        Model::DualSense => (DS_OUTPUT, [DS_CALIBRATION, DS_PAIRING, DS_FIRMWARE]),
    };
    b.vendor_bytes(ReportKind::Output, output.0, 0x23, output.1);
    for (id, len) in features {
        b.vendor_bytes(ReportKind::Feature, id, 0x24, len);
    }
    b.end_collection();
    b.finish()
}

/// An emulated DualShock 4 (second version), on USB.
#[derive(Clone, Debug)]
pub struct DualShock4(Pad);

impl DualShock4 {
    /// A pad with nothing pressed; `index` as in [`Target::create`].
    pub fn new(index: u8) -> Self {
        Self(Pad::new(Model::DualShock4, index))
    }
}

/// An emulated DualSense, on USB.
#[derive(Clone, Debug)]
pub struct DualSense(Pad);

impl DualSense {
    /// A pad with nothing pressed; `index` as in [`Target::create`].
    pub fn new(index: u8) -> Self {
        Self(Pad::new(Model::DualSense, index))
    }
}

macro_rules! impl_pad {
    ($pad:ty, $target:expr) => {
        impl VirtualPad for $pad {
            fn target(&self) -> Target {
                $target
            }

            fn report_descriptor(&self) -> Option<&[u8]> {
                Some(&self.0.descriptor)
            }

            fn capabilities(&self) -> Capabilities {
                self.0.capabilities()
            }

            fn input_report(&mut self, state: &GamepadState) -> &[u8] {
                self.0.input_report(state)
            }

            fn output_report(&mut self, report: &[u8]) -> Vec<OutputCommand> {
                self.0.output_report(report)
            }

            fn feature_report(&mut self, report_id: u8) -> Option<Vec<u8>> {
                self.0.feature_report(report_id)
            }
        }
    };
}

impl_pad!(DualShock4, Target::DualShock4);
impl_pad!(DualSense, Target::DualSense);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_positions() {
        let hat =
            |buttons: &[Button]| super::buttons(ButtonSet::from_iter(buttons.iter().copied()))[0];
        assert_eq!(hat(&[]), 8);
        assert_eq!(hat(&[Button::DPadUp]), 0);
        assert_eq!(hat(&[Button::DPadUp, Button::DPadRight]), 1);
        assert_eq!(hat(&[Button::DPadRight]), 2);
        assert_eq!(hat(&[Button::DPadDown, Button::DPadRight]), 3);
        assert_eq!(hat(&[Button::DPadDown]), 4);
        assert_eq!(hat(&[Button::DPadDown, Button::DPadLeft]), 5);
        assert_eq!(hat(&[Button::DPadLeft]), 6);
        assert_eq!(hat(&[Button::DPadUp, Button::DPadLeft]), 7);
        // Opposite directions cancel out.
        assert_eq!(hat(&[Button::DPadUp, Button::DPadDown]), 8);
        assert_eq!(
            hat(&[Button::DPadUp, Button::DPadDown, Button::DPadLeft]),
            6
        );
    }

    #[test]
    fn calibration_layout() {
        let mut pad = DualSense::new(0);
        let report = pad.feature_report(0x05).unwrap();
        assert_eq!(report.len(), 41);
        let at = |i: usize| i16::from_le_bytes([report[i], report[i + 1]]);
        assert_eq!([at(1), at(3), at(5)], [0, 0, 0]);
        assert_eq!([at(7), at(9)], [8640, -8640]);
        assert_eq!([at(19), at(21)], [540, 540]);
        assert_eq!([at(23), at(25), at(33)], [8192, -8192, -8192]);
        assert_eq!(report[35..], [0; 6]);
    }

    #[test]
    fn pairing_report_holds_the_virtual_address() {
        let mut pad = DualShock4::new(7);
        let report = pad.feature_report(0x12).unwrap();
        assert_eq!(report.len(), 16);
        assert_eq!(report[1..7], [7, 0, 0, b'g', b'a', 0x02]);
        assert_eq!(pad.feature_report(0xa3).unwrap().len(), 49);
        assert_eq!(pad.feature_report(0x05), None);
    }
}
