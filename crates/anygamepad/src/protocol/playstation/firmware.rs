//! Firmware information: feature report `0xA3` on the DualShock 4, `0x20`
//! on the DualSense and DualSense Edge. It holds the firmware's build date
//! and time and its hardware and firmware versions, nothing that identifies
//! a person.
//!
//! Sony's pads answer it; many compatible pads that borrow Sony's USB IDs
//! do not, or answer something else. `daidr/dualsense-tester` calls a
//! DualShock 4 whose answer is not 49 bytes long a fake; [`parse`] makes
//! the same check, so a pad that answers with no firmware information is
//! most likely not a Sony one.
//!
//! Like every feature report, reading it switches a Bluetooth pad to its
//! full report (see the [parent module](super)).
//!
//! Layouts from `hid-playstation` (versions) and daidr's tester (build date
//! and time); not checked on a pad yet.

use super::Model;

/// Bytes of the DualShock 4's answer, report ID included.
const DUALSHOCK4_LENGTH: usize = 49;

/// Bytes of the DualSense's answer read so far: its versions end at byte
/// 46 (it answers 64, the last 4 a CRC-32 over Bluetooth).
const DUALSENSE_MIN_LENGTH: usize = 46;

/// The feature report holding a model's firmware information.
pub const fn report_id(model: Model) -> u8 {
    match model {
        Model::DualShock4 => 0xa3,
        Model::DualSense | Model::DualSenseEdge => 0x20,
    }
}

/// A pad's firmware information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareInfo {
    /// Build date and time as the firmware writes them
    /// (`Sep 21 2018 04:50:51`).
    pub build: String,
    /// Hardware version.
    pub hardware: u32,
    /// Main firmware version.
    pub firmware: u32,
    /// The DualSense's update version, which its firmware updates show as
    /// `0x0630`.
    pub update: Option<u16>,
    /// The DualSense's firmware type (2 and 3 on retail pads).
    pub kind: Option<u16>,
    /// The DualSense's software series.
    pub series: Option<u16>,
    /// The DualSense's bootloader (SBL), DSP and MCU DSP firmware versions,
    /// when its answer holds them.
    pub sbl: Option<u32>,
    /// See `sbl`.
    pub dsp: Option<u32>,
    /// See `sbl`.
    pub mcu_dsp: Option<u32>,
}

/// Reads ASCII text padded with NUL bytes; `None` for anything else.
fn text(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let (text, padding) = bytes.split_at(end);
    let printable = text.iter().all(|b| (0x20..0x7f).contains(b));
    (printable && !text.trim_ascii().is_empty() && padding.iter().all(|&b| b == 0))
        .then(|| String::from_utf8_lossy(text).trim_ascii().to_owned())
}

fn le16(report: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([report[at], report[at + 1]])
}

fn le32(report: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([report[at], report[at + 1], report[at + 2], report[at + 3]])
}

/// The firmware information in a pad's answer to [`report_id`], report ID
/// first; `None` when the answer is not one (see the
/// [module documentation](self)).
pub fn parse(model: Model, report: &[u8]) -> Option<FirmwareInfo> {
    if report.first() != Some(&report_id(model)) {
        return None;
    }
    match model {
        Model::DualShock4 => {
            if report.len() != DUALSHOCK4_LENGTH {
                return None;
            }
            let date = text(&report[1..17])?;
            let time = text(&report[17..25])?;
            Some(FirmwareInfo {
                build: format!("{date} {time}"),
                hardware: u32::from(le16(report, 35)),
                firmware: u32::from(le16(report, 41)),
                update: None,
                kind: None,
                series: None,
                sbl: None,
                dsp: None,
                mcu_dsp: None,
            })
        }
        Model::DualSense | Model::DualSenseEdge => {
            if report.len() < DUALSENSE_MIN_LENGTH {
                return None;
            }
            let date = text(&report[1..12])?;
            let time = text(&report[12..20])?;
            Some(FirmwareInfo {
                build: format!("{date} {time}"),
                hardware: le32(report, 24),
                firmware: le32(report, 28),
                update: Some(le16(report, 44)),
                kind: Some(le16(report, 20)),
                series: Some(le16(report, 22)),
                sbl: (report.len() >= 52).then(|| le32(report, 48)),
                dsp: (report.len() >= 56).then(|| le32(report, 52)),
                mcu_dsp: (report.len() >= 60).then(|| le32(report, 56)),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dualshock4() -> Vec<u8> {
        let mut report = vec![0u8; DUALSHOCK4_LENGTH];
        report[0] = 0xa3;
        report[1..12].copy_from_slice(b"Sep 21 2018");
        report[17..25].copy_from_slice(b"04:50:51");
        report[35..37].copy_from_slice(&0x0100u16.to_le_bytes());
        report[41..43].copy_from_slice(&0x8001u16.to_le_bytes());
        report
    }

    fn dualsense() -> Vec<u8> {
        let mut report = vec![0u8; 64];
        report[0] = 0x20;
        report[1..12].copy_from_slice(b"Jun 12 2023");
        report[12..20].copy_from_slice(b"10:44:25");
        report[24..28].copy_from_slice(&0x0000_0313u32.to_le_bytes());
        report[28..32].copy_from_slice(&0x0110_0005u32.to_le_bytes());
        report[44..46].copy_from_slice(&0x0630u16.to_le_bytes());
        report
    }

    #[test]
    fn report_ids() {
        assert_eq!(report_id(Model::DualShock4), 0xa3);
        assert_eq!(report_id(Model::DualSense), 0x20);
        assert_eq!(report_id(Model::DualSenseEdge), 0x20);
    }

    #[test]
    fn dualshock4_answer() {
        let info = parse(Model::DualShock4, &dualshock4()).unwrap();
        assert_eq!(info.build, "Sep 21 2018 04:50:51");
        assert_eq!((info.hardware, info.firmware), (0x0100, 0x8001));
        assert_eq!(info.update, None);
    }

    #[test]
    fn dualsense_answer() {
        let info = parse(Model::DualSense, &dualsense()).unwrap();
        assert_eq!(info.build, "Jun 12 2023 10:44:25");
        assert_eq!((info.hardware, info.firmware), (0x0313, 0x0110_0005));
        assert_eq!(info.update, Some(0x0630));
    }

    #[test]
    fn answers_that_are_not_firmware_information() {
        // Another length, as compatible pads answer.
        let mut short = dualshock4();
        short.truncate(48);
        assert_eq!(parse(Model::DualShock4, &short), None);
        // Another report ID.
        let mut other = dualshock4();
        other[0] = 0x02;
        assert_eq!(parse(Model::DualShock4, &other), None);
        // Zeros where the build date goes.
        let mut blank = dualshock4();
        blank[1..25].fill(0);
        assert_eq!(parse(Model::DualShock4, &blank), None);
        // Text that is not ASCII.
        let mut garbled = dualsense();
        garbled[3] = 0xff;
        assert_eq!(parse(Model::DualSense, &garbled), None);
        assert_eq!(parse(Model::DualSense, &[]), None);
        assert_eq!(parse(Model::DualSense, &dualsense()[..40]), None);
    }
}
