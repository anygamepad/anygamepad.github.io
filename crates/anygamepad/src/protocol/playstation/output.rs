//! Output reports of the DualShock 4 and DualSense (C7): rumble, light bar,
//! player LEDs and the DualSense's mute LED, over USB and Bluetooth.
//!
//! An [`OutputReport`] keeps what was last asked for each output and
//! writes the report that sets it. Only the outputs a report carries are
//! flagged valid, so the pad leaves the others as they are.
//!
//! Layouts from research note 09 and `hid-playstation` (note 05). Adaptive
//! triggers and HD haptics are not written yet.

use anygamepad_model::{OutputCommand, Rgb, Rumble, Transport};

use super::Model;
use crate::protocol::crc32;

/// The Bluetooth HID header of an output report (DATA | Output), which the
/// CRC-32 covers before the report ID.
const OUTPUT_CRC_SEED: u8 = 0xa2;
/// Length of a Bluetooth output report, report ID and CRC included.
const BLUETOOTH_LEN: usize = 78;

/// Player LED patterns of the DualSense for players 1-5 (`hid-playstation`).
const PLAYER_LEDS: [u8; 5] = [0x04, 0x0a, 0x15, 0x1b, 0x1f];

/// The DualSense's five player LEDs for `player` (bit 0 the leftmost):
/// 0 turns them off, players above 5 show 5.
pub(crate) fn player_pattern(player: u8) -> u8 {
    match player {
        0 => 0,
        n => PLAYER_LEDS[usize::from(n.min(PLAYER_LEDS.len() as u8)) - 1],
    }
}

/// What to set on a pad's outputs.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputReport {
    model: Model,
    rumble: Option<Rumble>,
    lightbar: Option<Rgb>,
    player: Option<u8>,
    mute_led: Option<bool>,
}

impl OutputReport {
    /// Nothing to set yet.
    pub const fn new(model: Model) -> Self {
        Self {
            model,
            rumble: None,
            lightbar: None,
            player: None,
            mute_led: None,
        }
    }

    /// Records a command; `false` for commands this pad does not take
    /// (player numbers on the DualShock 4, which shows them with its light
    /// bar, and trigger rumble).
    pub fn set(&mut self, command: OutputCommand) -> bool {
        match command {
            OutputCommand::Rumble(rumble) => self.rumble = Some(rumble),
            OutputCommand::Lightbar(rgb) => self.lightbar = Some(rgb),
            OutputCommand::PlayerNumber(player) if self.model != Model::DualShock4 => {
                self.player = Some(player.min(PLAYER_LEDS.len() as u8));
            }
            _ => return false,
        }
        true
    }

    /// Lights the DualSense's mute button LED.
    pub fn set_mute_led(&mut self, on: bool) {
        self.mute_led = Some(on);
    }

    /// The report that sets everything recorded, report ID first. On
    /// Bluetooth, `sequence` (0-15) numbers DualSense reports; the pad
    /// ignores reports whose sequence does not change.
    pub fn encode(&self, transport: Transport, sequence: u8) -> Vec<u8> {
        let bluetooth = transport != Transport::Usb;
        let mut report = if bluetooth {
            vec![0; BLUETOOTH_LEN]
        } else {
            match self.model {
                Model::DualShock4 => vec![0; 32],
                Model::DualSense | Model::DualSenseEdge => vec![0; 48],
            }
        };
        let common = match (self.model, bluetooth) {
            (Model::DualShock4, false) => {
                report[0] = 0x05;
                1
            }
            (Model::DualShock4, true) => {
                report[0] = 0x11;
                // HID report, with CRC.
                report[1] = 0xc0;
                3
            }
            (_, false) => {
                report[0] = 0x02;
                1
            }
            (_, true) => {
                report[0] = 0x31;
                report[1] = (sequence & 0x0f) << 4;
                // Tag of the output payload.
                report[2] = 0x10;
                3
            }
        };
        let c = &mut report[common..];
        let motor = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        match self.model {
            Model::DualShock4 => {
                if let Some(rumble) = self.rumble {
                    c[0] |= 0x01;
                    c[3] = motor(rumble.high_frequency);
                    c[4] = motor(rumble.low_frequency);
                }
                if let Some(rgb) = self.lightbar {
                    c[0] |= 0x02;
                    c[5..8].copy_from_slice(&[rgb.r, rgb.g, rgb.b]);
                }
            }
            Model::DualSense | Model::DualSenseEdge => {
                if let Some(rumble) = self.rumble {
                    // Compatible vibration, haptics select.
                    c[0] |= 0x03;
                    c[2] = motor(rumble.high_frequency);
                    c[3] = motor(rumble.low_frequency);
                }
                if let Some(on) = self.mute_led {
                    c[1] |= 0x01;
                    c[8] = u8::from(on);
                }
                if let Some(rgb) = self.lightbar {
                    c[1] |= 0x04;
                    c[44..47].copy_from_slice(&[rgb.r, rgb.g, rgb.b]);
                }
                if let Some(player) = self.player {
                    c[1] |= 0x10;
                    c[43] = player_pattern(player);
                }
            }
        }
        if bluetooth {
            let (body, crc) = report.split_at_mut(BLUETOOTH_LEN - 4);
            crc.copy_from_slice(&crc32(&[OUTPUT_CRC_SEED], body).to_le_bytes());
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dualsense_usb() {
        let mut report = OutputReport::new(Model::DualSense);
        assert!(report.set(OutputCommand::Rumble(Rumble {
            low_frequency: 1.0,
            high_frequency: 0.5,
        })));
        assert!(report.set(OutputCommand::PlayerNumber(2)));
        report.set_mute_led(true);
        let bytes = report.encode(Transport::Usb, 0);
        assert_eq!(bytes.len(), 48);
        assert_eq!(bytes[..5], [0x02, 0x03, 0x11, 128, 255]);
        assert_eq!(bytes[9], 1);
        assert_eq!(bytes[44], 0x0a);
        // No light bar asked for: left alone.
        assert_eq!(bytes[45..48], [0, 0, 0]);
    }

    #[test]
    fn bluetooth_reports_carry_a_crc() {
        for (model, id) in [(Model::DualSense, 0x31), (Model::DualShock4, 0x11)] {
            let mut report = OutputReport::new(model);
            report.set(OutputCommand::Lightbar(Rgb { r: 1, g: 2, b: 3 }));
            let bytes = report.encode(Transport::Bluetooth, 5);
            assert_eq!(bytes.len(), 78);
            assert_eq!(bytes[0], id);
            let crc = u32::from_le_bytes([bytes[74], bytes[75], bytes[76], bytes[77]]);
            assert_eq!(crc, crc32(&[0xa2], &bytes[..74]));
        }
        let bytes = OutputReport::new(Model::DualSense).encode(Transport::Bluetooth, 5);
        assert_eq!(bytes[1..3], [0x50, 0x10]);
    }

    #[test]
    fn dualshock4_has_no_player_leds() {
        let mut report = OutputReport::new(Model::DualShock4);
        assert!(!report.set(OutputCommand::PlayerNumber(1)));
        report.set(OutputCommand::Lightbar(Rgb { r: 9, g: 8, b: 7 }));
        let bytes = report.encode(Transport::Usb, 0);
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[..2], [0x05, 0x02]);
        assert_eq!(bytes[6..9], [9, 8, 7]);
    }
}
