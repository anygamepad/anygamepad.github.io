//! The Nintendo Switch Pro Controller, in its USB form.
//!
//! The input report is the standard full report (`0x30`) that the pad
//! sends once a host has set it up. Sticks are 12-bit values that the host
//! scales with the factory calibration it reads from the pad's SPI flash;
//! every virtual pad uses the same nominal calibration
//! ([`STICK_CENTER`], [`STICK_RANGE`]).
//!
//! The set-up exchange is answered (A5): USB commands (`0x80`) with `0x81`
//! reports, subcommands (`0x01`) with `0x21` reports, through
//! [`VirtualPad::pending_input`]. SPI flash reads return the nominal stick
//! calibration and erased user calibration; other subcommands are
//! acknowledged without data. Not emulated yet: HD rumble, whose
//! amplitudes are encoded, and motion sensors (reported still).
//!
//! The report descriptor has the real pad's report IDs and sizes, every
//! report as vendor data.
//!
//! Sources: `hid-nintendo` (note 05) and the community's reverse-engineered
//! protocol notes it follows. Not checked against a real pad yet.

use std::collections::VecDeque;

use anygamepad_model::{
    Axis, AxisSet, Button, Capabilities, GamepadState, Output, OutputCommand, OutputSet, PowerState,
};

use crate::descriptor::{Builder, GAME_PAD, GENERIC_DESKTOP, ReportKind};
use crate::{Target, VirtualIdentity, VirtualPad};

pub(crate) const IDENTITY: VirtualIdentity = VirtualIdentity {
    vendor_id: 0x057e,
    product_id: 0x2009,
    version: 0x0200,
    name: "Nintendo Co., Ltd. Pro Controller",
};

/// Stick value at rest, on 12 bits.
pub const STICK_CENTER: u16 = 2048;
/// Distance from the centre to a stick's full deflection.
pub const STICK_RANGE: u16 = 1600;

const INPUT_ID: u8 = 0x30;
const INPUT_LEN: usize = 63;
/// Input reports: standard full report, subcommand reply, USB reply.
const INPUTS: [(u8, u32); 3] = [(INPUT_ID, 63), (0x21, 63), (0x81, 63)];
/// Output reports: rumble and subcommand, rumble only, USB commands.
const OUTPUTS: [(u8, u32); 4] = [(0x01, 48), (0x10, 48), (0x80, 63), (0x82, 63)];

/// Subcommand that sets the player lights.
const SET_PLAYER_LIGHTS: u8 = 0x30;
/// Subcommand that asks for the device information.
const DEVICE_INFO: u8 = 0x02;
/// Subcommand that reads the SPI flash.
const SPI_READ: u8 = 0x10;

/// The SPI flash regions the virtual pad holds; every other byte reads as
/// erased (`0xff`).
fn flash(address: u32) -> u8 {
    // Factory sticks at 0x603d: left above, centre, below; right centre,
    // below, above. Every value is centre or range on both axes.
    let pack = |v: u16| {
        [
            v as u8,
            ((v >> 8) as u8 & 0x0f) | ((v as u8 & 0x0f) << 4),
            (v >> 4) as u8,
        ]
    };
    let (r, c) = (pack(STICK_RANGE), pack(STICK_CENTER));
    let sticks = [r, c, r, c, r, r].concat();
    // Factory IMU at 0x6020: accelerometer origin and sensitivity, then
    // gyroscope origin and sensitivity, per axis.
    let imu: Vec<u8> = [
        0i16, 0, 0, 16384, 16384, 16384, 0, 0, 0, 13371, 13371, 13371,
    ]
    .iter()
    .flat_map(|v| v.to_le_bytes())
    .collect();
    // Colours at 0x6050: body, buttons (dark grey and white).
    let colours = [0x32, 0x32, 0x32, 0xff, 0xff, 0xff];
    let regions: [(u32, &[u8]); 3] = [(0x6020, &imu), (0x603d, &sticks), (0x6050, &colours)];
    regions
        .iter()
        .find_map(|(start, bytes)| {
            let offset = address.checked_sub(*start)?;
            bytes.get(offset as usize).copied()
        })
        .unwrap_or(0xff)
}

/// Bits of the three button bytes: right side, shared, left side. Face
/// buttons by position: Nintendo `B` is south, `A` east.
const BUTTONS: [(usize, u8, Button); 18] = [
    (0, 0x01, Button::West),
    (0, 0x02, Button::North),
    (0, 0x04, Button::South),
    (0, 0x08, Button::East),
    (0, 0x40, Button::RightShoulder),
    (0, 0x80, Button::RightTrigger),
    (1, 0x01, Button::Select),
    (1, 0x02, Button::Start),
    (1, 0x04, Button::RightStick),
    (1, 0x08, Button::LeftStick),
    (1, 0x10, Button::Home),
    (1, 0x20, Button::Capture),
    (2, 0x01, Button::DPadDown),
    (2, 0x02, Button::DPadUp),
    (2, 0x04, Button::DPadRight),
    (2, 0x08, Button::DPadLeft),
    (2, 0x40, Button::LeftShoulder),
    (2, 0x80, Button::LeftTrigger),
];

/// Player light patterns and the player each shows: one light per player,
/// as the console does, and `hid-nintendo`'s patterns for players 1-8.
const PLAYER_LIGHTS: [(u8, u8); 11] = [
    (0x1, 1),
    (0x2, 2),
    (0x4, 3),
    (0x8, 4),
    (0x3, 2),
    (0x7, 3),
    (0xf, 4),
    (0x9, 5),
    (0x5, 6),
    (0xd, 7),
    (0x6, 8),
];

/// An emulated Switch Pro Controller.
#[derive(Clone, Debug)]
pub struct SwitchPro {
    descriptor: Vec<u8>,
    report: [u8; 1 + INPUT_LEN],
    replies: VecDeque<Vec<u8>>,
}

impl SwitchPro {
    /// A pad with nothing pressed.
    pub fn new() -> Self {
        let mut b = Builder::new();
        b.usage_page(GENERIC_DESKTOP).usage(GAME_PAD).application();
        for (id, len) in INPUTS {
            b.vendor_bytes(ReportKind::Input, id, 0x01, len);
        }
        for (id, len) in OUTPUTS {
            b.vendor_bytes(ReportKind::Output, id, 0x02, len);
        }
        b.end_collection();
        let mut report = [0; 1 + INPUT_LEN];
        report[0] = INPUT_ID;
        Self {
            descriptor: b.finish(),
            report,
            replies: VecDeque::new(),
        }
    }

    /// A `0x21` reply: the current state, the ACK byte, the subcommand and
    /// its data.
    fn reply(&mut self, ack: u8, subcommand: u8, data: &[u8]) {
        self.report[1] = self.report[1].wrapping_add(1);
        let mut reply = vec![0; 1 + INPUT_LEN];
        reply[0] = 0x21;
        reply[1..12].copy_from_slice(&self.report[1..12]);
        reply[13] = ack;
        reply[14] = subcommand;
        let len = data.len().min(INPUT_LEN - 14);
        reply[15..15 + len].copy_from_slice(&data[..len]);
        self.replies.push_back(reply);
    }

    /// Answers a subcommand.
    fn subcommand(&mut self, id: u8, args: &[u8]) {
        match id {
            DEVICE_INFO => {
                // Firmware 3.72, Pro Controller, the made-up address, colours
                // in SPI.
                let mut info = vec![0x03, 0x48, 0x03, 0x02];
                info.extend(crate::virtual_mac(0));
                info.extend([0x01, 0x01]);
                self.reply(0x82, id, &info);
            }
            SPI_READ if args.len() >= 5 => {
                let address = u32::from_le_bytes([args[0], args[1], args[2], args[3]]);
                let len = args[4].min(0x1d);
                let mut data = args[..5].to_vec();
                data[4] = len;
                data.extend((0..u32::from(len)).map(|i| flash(address + i)));
                self.reply(0x90, id, &data);
            }
            _ => self.reply(0x80, id, &[]),
        }
    }
}

impl Default for SwitchPro {
    fn default() -> Self {
        Self::new()
    }
}

/// A stick axis in `[-1, 1]` to 12 bits around [`STICK_CENTER`].
fn stick(value: f32) -> u16 {
    let offset = (value.clamp(-1.0, 1.0) * f32::from(STICK_RANGE)).round() as i16;
    STICK_CENTER.wrapping_add_signed(offset)
}

/// Two 12-bit values in three bytes, X first; Y grows upwards.
fn pack_stick(out: &mut [u8], x: f32, y: f32) {
    let (x, y) = (stick(x), stick(y));
    out[0] = x as u8;
    out[1] = ((x >> 8) as u8 & 0x0f) | ((y as u8 & 0x0f) << 4);
    out[2] = (y >> 4) as u8;
}

/// Battery byte: level (0, 2, 4, 6, 8 for full) and charging bit in the
/// high nibble; the low nibble says a Pro Controller powered over USB.
fn battery(state: &GamepadState) -> u8 {
    let level = |percent: Option<u8>| match percent {
        Some(p) => ((u16::from(p.min(100)) * 4 + 50) / 100) as u8 * 2,
        None => 8,
    };
    let power = state.power;
    let high = match power.state {
        PowerState::OnBattery => level(power.percent),
        PowerState::Charging => level(power.percent) | 1,
        _ => 8,
    };
    (high << 4) | 0x01
}

impl VirtualPad for SwitchPro {
    fn target(&self) -> Target {
        Target::SwitchPro
    }

    fn report_descriptor(&self) -> Option<&[u8]> {
        Some(&self.descriptor)
    }

    /// Digital triggers: no trigger axes, and the trigger buttons are what
    /// ZL and ZR report.
    fn capabilities(&self) -> Capabilities {
        let buttons = BUTTONS.iter().map(|(_, _, button)| *button).collect();
        Capabilities {
            buttons,
            axes: AxisSet::from([
                Axis::LeftStickX,
                Axis::LeftStickY,
                Axis::RightStickX,
                Axis::RightStickY,
            ]),
            outputs: OutputSet::from([Output::PlayerLeds]),
            battery: true,
            ..Capabilities::default()
        }
    }

    fn input_report(&mut self, state: &GamepadState) -> &[u8] {
        let d = &mut self.report[1..];
        d[0] = d[0].wrapping_add(1);
        d[1] = battery(state);
        d[2..5].fill(0);
        for (byte, mask, button) in BUTTONS {
            if state.pressed(button) {
                d[2 + byte] |= mask;
            }
        }
        pack_stick(
            &mut d[5..8],
            state.axis(Axis::LeftStickX),
            state.axis(Axis::LeftStickY),
        );
        pack_stick(
            &mut d[8..11],
            state.axis(Axis::RightStickX),
            state.axis(Axis::RightStickY),
        );
        &self.report
    }

    /// Answers USB commands and subcommands (see the module
    /// documentation), and reads the player lights from subcommand `0x30`
    /// (lights on in the low nibble of its argument). Rumble is not decoded
    /// yet.
    fn output_report(&mut self, report: &[u8]) -> Vec<OutputCommand> {
        match report {
            // USB commands: status, handshake, baud rate. 0x04 (no
            // Bluetooth timeout) has no answer.
            [0x80, command @ (0x01..=0x03), ..] => {
                let mut reply = vec![0; 1 + INPUT_LEN];
                reply[0] = 0x81;
                reply[1] = *command;
                if *command == 0x01 {
                    // Pro Controller, the made-up address.
                    reply[3] = 0x03;
                    let mut mac = crate::virtual_mac(0);
                    mac.reverse();
                    reply[4..10].copy_from_slice(&mac);
                }
                self.replies.push_back(reply);
            }
            [0x01, _, _, _, _, _, _, _, _, _, id, args @ ..] => self.subcommand(*id, args),
            _ => {}
        }
        // ID, counter, 8 bytes of rumble, subcommand, argument.
        match report {
            [
                0x01,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                SET_PLAYER_LIGHTS,
                lights,
                ..,
            ] => {
                let lights = lights & 0x0f;
                let player = match lights {
                    0 => Some(0),
                    _ => PLAYER_LIGHTS
                        .iter()
                        .find(|(pattern, _)| *pattern == lights)
                        .map(|(_, player)| *player),
                };
                player
                    .map(OutputCommand::PlayerNumber)
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn feature_report(&mut self, _report_id: u8) -> Option<Vec<u8>> {
        None
    }

    fn pending_input(&mut self) -> Option<Vec<u8>> {
        self.replies.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::Power;

    use super::*;
    use crate::descriptor::{ReportLayout, reports};

    fn unpack(raw: &[u8]) -> (u16, u16) {
        let x = u16::from(raw[0]) | (u16::from(raw[1] & 0x0f) << 8);
        let y = u16::from(raw[1] >> 4) | (u16::from(raw[2]) << 4);
        (x, y)
    }

    #[test]
    fn input_report() {
        let mut pad = SwitchPro::new();
        let report = pad.input_report(&GamepadState::new()).to_vec();
        assert_eq!(report.len(), 64);
        assert_eq!(report[..2], [0x30, 1]);
        assert_eq!(report[2], 0x81);
        assert_eq!(report[3..6], [0, 0, 0]);
        assert_eq!(unpack(&report[6..9]), (2048, 2048));

        let mut state = GamepadState::new();
        // Nintendo B, A, ZL, capture, d-pad up.
        for button in [
            Button::South,
            Button::East,
            Button::LeftTrigger,
            Button::Capture,
            Button::DPadUp,
        ] {
            state.set_pressed(button, true);
        }
        state.set_axis(Axis::LeftStickY, 1.0);
        state.set_axis(Axis::RightStickX, -1.0);
        state.power = Power {
            state: PowerState::OnBattery,
            percent: Some(40),
        };
        let report = pad.input_report(&state);
        assert_eq!(report[1], 2);
        assert_eq!(report[2], 0x41);
        assert_eq!(report[3..6], [0x0c, 0x20, 0x82]);
        assert_eq!(unpack(&report[6..9]), (2048, 2048 + 1600));
        assert_eq!(unpack(&report[9..12]), (2048 - 1600, 2048));
    }

    #[test]
    fn battery_levels() {
        let with = |state, percent| {
            let mut s = GamepadState::new();
            s.power = Power { state, percent };
            battery(&s) >> 4
        };
        assert_eq!(with(PowerState::OnBattery, Some(100)), 8);
        assert_eq!(with(PowerState::OnBattery, Some(10)), 0);
        assert_eq!(with(PowerState::OnBattery, Some(60)), 4);
        assert_eq!(with(PowerState::Charging, Some(80)), 7);
        assert_eq!(with(PowerState::Unknown, None), 8);
    }

    #[test]
    fn player_lights() {
        let mut pad = SwitchPro::new();
        let lights = |pattern: u8| {
            let mut report = [0u8; 49];
            report[0] = 0x01;
            report[10] = SET_PLAYER_LIGHTS;
            report[11] = pattern;
            report
        };
        assert_eq!(
            pad.output_report(&lights(0x03)),
            [OutputCommand::PlayerNumber(2)]
        );
        assert_eq!(
            pad.output_report(&lights(0x04)),
            [OutputCommand::PlayerNumber(3)]
        );
        // Flashing lights in the high nibble.
        assert_eq!(
            pad.output_report(&lights(0xf0)),
            [OutputCommand::PlayerNumber(0)]
        );
        assert!(pad.output_report(&lights(0x0b)).is_empty());
        assert!(pad.output_report(&[0x10, 0, 0]).is_empty());
    }

    #[test]
    fn descriptor_declares_every_report() {
        let pad = SwitchPro::new();
        let layouts = reports(pad.report_descriptor().unwrap()).unwrap();
        assert_eq!(layouts.len(), INPUTS.len() + OUTPUTS.len());
        assert!(layouts.contains(&ReportLayout {
            kind: ReportKind::Input,
            report_id: INPUT_ID,
            bits: INPUT_LEN as u32 * 8,
        }));
        let caps = pad.capabilities();
        assert_eq!(caps.buttons.len(), BUTTONS.len());
        assert!(!caps.has_axis(Axis::LeftTrigger));
    }
}
