//! Virtual pads on Linux (A5): uinput for the Xbox 360 Controller, UHID
//! for the HID pads, so that the kernel's own drivers (`hid-playstation`,
//! `hid-nintendo`) bind to them as to the real pads.
//!
//! Both need write access to `/dev/uinput` or `/dev/uhid`; see
//! `docs/linux-permissions.md` for the udev rules.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use anygamepad_model::{GamepadState, OutputCommand, Rumble};
use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventSummary, FFEffectCode, FFEffectKind,
    InputEvent, InputId, KeyCode, UInputCode, UinputAbsSetup,
};
use nix::fcntl::{FcntlArg, OFlag, fcntl};

use crate::uhid::{self, Request};
use crate::xbox360::{EVDEV_AXES, EVDEV_KEYS, evdev_events};
use crate::{Target, VirtualPad};

/// A virtual pad the kernel sees as a device.
pub struct LinuxVirtualPad {
    pad: Box<dyn VirtualPad>,
    output: Output,
}

enum Output {
    Uinput {
        device: VirtualDevice,
        /// Rumble effects the host uploaded, by effect ID.
        effects: BTreeMap<i16, Rumble>,
    },
    Uhid {
        file: File,
        buffer: Box<[u8; uhid::EVENT_SIZE]>,
    },
}

impl LinuxVirtualPad {
    /// Creates a virtual pad of `target`; `index` as in [`Target::create`].
    ///
    /// The Xbox 360 Controller goes through uinput (it is not a HID
    /// device), every other target through UHID.
    pub fn create(target: Target, index: u8) -> io::Result<Self> {
        let pad = target.create(index);
        let output = match pad.report_descriptor() {
            None => uinput(target)?,
            Some(descriptor) => {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(OFlag::O_NONBLOCK.bits() | OFlag::O_CLOEXEC.bits())
                    .open("/dev/uhid")?;
                file.write_all(&uhid::create(pad.identity(), descriptor))?;
                Output::Uhid {
                    file,
                    buffer: Box::new([0; uhid::EVENT_SIZE]),
                }
            }
        };
        Ok(Self { pad, output })
    }

    /// The emulated controller type.
    pub fn target(&self) -> Target {
        self.pad.target()
    }

    /// Shows `state` on the virtual pad, answers what the host asked since
    /// the last call, and returns the output commands the host sent
    /// (rumble, light bar, player number) for the physical pad.
    pub fn update(&mut self, state: &GamepadState) -> io::Result<Vec<OutputCommand>> {
        let mut commands = Vec::new();
        match &mut self.output {
            Output::Uinput { device, effects } => {
                let report = self.pad.input_report(state);
                let events: Vec<InputEvent> = evdev_events(report)
                    .into_iter()
                    .map(|(kind, code, value)| InputEvent::new(kind, code, value))
                    .collect();
                device.emit(&events)?;
                let pending: Vec<InputEvent> = match device.fetch_events() {
                    Ok(events) => events.collect(),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => Vec::new(),
                    Err(e) => return Err(e),
                };
                for event in pending {
                    match event.destructure() {
                        EventSummary::UInput(event, UInputCode::UI_FF_UPLOAD, _) => {
                            let upload = device.process_ff_upload(event)?;
                            if let FFEffectKind::Rumble {
                                strong_magnitude,
                                weak_magnitude,
                            } = upload.effect().kind
                            {
                                let scale = |m: u16| f32::from(m) / f32::from(u16::MAX);
                                effects.insert(
                                    upload.effect_id(),
                                    Rumble {
                                        low_frequency: scale(strong_magnitude),
                                        high_frequency: scale(weak_magnitude),
                                    },
                                );
                            }
                            // Dropping the upload hands it back to the
                            // kernel, accepted.
                        }
                        EventSummary::UInput(event, UInputCode::UI_FF_ERASE, _) => {
                            let erase = device.process_ff_erase(event)?;
                            effects.remove(&(erase.effect_id() as i16));
                        }
                        EventSummary::ForceFeedback(_, FFEffectCode(id), value) => {
                            let rumble = if value > 0 {
                                effects.get(&(id as i16)).copied().unwrap_or_default()
                            } else {
                                Rumble::default()
                            };
                            commands.push(OutputCommand::Rumble(rumble));
                        }
                        _ => {}
                    }
                }
            }
            Output::Uhid { file, buffer } => {
                file.write_all(&uhid::input(self.pad.input_report(state)))?;
                loop {
                    match file.read(&mut buffer[..]) {
                        Ok(0) => break,
                        Ok(len) => {
                            let Some(request) = uhid::parse(&buffer[..len]) else {
                                continue;
                            };
                            if let Some(reply) = answer(self.pad.as_mut(), request, &mut commands) {
                                file.write_all(&reply)?;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Ok(commands)
    }
}

impl Drop for LinuxVirtualPad {
    fn drop(&mut self) {
        if let Output::Uhid { file, .. } = &mut self.output {
            // Closing the file destroys the device too.
            let _ = file.write_all(&uhid::destroy());
        }
    }
}

/// The reply to a UHID request, if it needs one; output reports add their
/// commands to `commands`.
fn answer(
    pad: &mut dyn VirtualPad,
    request: Request,
    commands: &mut Vec<OutputCommand>,
) -> Option<Vec<u8>> {
    match request {
        Request::Output { data, .. } => {
            commands.extend(pad.output_report(&data));
            None
        }
        Request::GetReport { id, report, .. } => Some(match pad.feature_report(report) {
            Some(data) => uhid::get_report_reply(id, 0, &data),
            None => uhid::get_report_reply(id, uhid::EIO, &[]),
        }),
        Request::SetReport { id, data, .. } => {
            // Feature writes are accepted; those that carry output
            // commands give them.
            commands.extend(pad.output_report(&data));
            Some(uhid::set_report_reply(id, 0))
        }
        Request::Start | Request::Stop | Request::Open | Request::Close => None,
    }
}

/// The uinput Xbox 360 Controller, as `xpad` presents one.
fn uinput(target: Target) -> io::Result<Output> {
    let identity = target.identity();
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in EVDEV_KEYS {
        keys.insert(KeyCode(code));
    }
    let mut ff = AttributeSet::<FFEffectCode>::new();
    ff.insert(FFEffectCode::FF_RUMBLE);
    let mut builder = VirtualDevice::builder()?
        .name(identity.name)
        .input_id(InputId::new(
            BusType::BUS_USB,
            identity.vendor_id,
            identity.product_id,
            identity.version,
        ))
        .with_keys(&keys)?
        .with_ff(&ff)?
        .with_ff_effects_max(16);
    for (code, min, max, fuzz, flat) in EVDEV_AXES {
        let info = AbsInfo::new(0, min, max, fuzz, flat, 0);
        builder = builder.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), info))?;
    }
    let device = builder.build()?;
    let flags = fcntl(device.as_raw_fd(), FcntlArg::F_GETFL)?;
    let flags = OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK;
    fcntl(device.as_raw_fd(), FcntlArg::F_SETFL(flags))?;
    Ok(Output::Uinput {
        device,
        effects: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use anygamepad_model::Rgb;

    use super::*;
    use crate::{DualSense, DualShock4};

    #[test]
    fn answers_feature_reads_and_output_reports() {
        let mut pad = DualShock4::new(0);
        let mut commands = Vec::new();
        let reply = answer(
            &mut pad,
            Request::GetReport {
                id: 4,
                report: 0x02,
                report_type: uhid::ReportType::Feature,
            },
            &mut commands,
        )
        .unwrap();
        // Reply with the 37-byte calibration report.
        assert_eq!(u16::from_ne_bytes([reply[8], reply[9]]), 0);
        assert_eq!(u16::from_ne_bytes([reply[10], reply[11]]), 37);
        assert_eq!(reply[12], 0x02);

        let reply = answer(
            &mut pad,
            Request::GetReport {
                id: 5,
                report: 0x99,
                report_type: uhid::ReportType::Feature,
            },
            &mut commands,
        )
        .unwrap();
        assert_eq!(u16::from_ne_bytes([reply[8], reply[9]]), uhid::EIO);

        let mut output = vec![0u8; 32];
        output[0] = 0x05;
        output[1] = 0x02;
        output[6..9].copy_from_slice(&[9, 8, 7]);
        assert_eq!(
            answer(
                &mut pad,
                Request::Output {
                    data: output,
                    report_type: uhid::ReportType::Output,
                },
                &mut commands,
            ),
            None
        );
        assert_eq!(
            commands,
            [OutputCommand::Lightbar(Rgb { r: 9, g: 8, b: 7 })]
        );
    }

    #[test]
    fn accepts_feature_writes() {
        let mut pad = DualSense::new(0);
        let mut commands = Vec::new();
        let reply = answer(
            &mut pad,
            Request::SetReport {
                id: 11,
                report: 0x08,
                report_type: uhid::ReportType::Feature,
                data: vec![0x08, 0],
            },
            &mut commands,
        )
        .unwrap();
        assert_eq!(
            u32::from_ne_bytes([reply[4], reply[5], reply[6], reply[7]]),
            11
        );
        assert_eq!(u16::from_ne_bytes([reply[8], reply[9]]), 0);
        assert!(commands.is_empty());
        assert_eq!(answer(&mut pad, Request::Open, &mut commands), None);
    }

    #[test]
    fn creating_without_devices_fails_cleanly() {
        // The container running the tests has neither /dev/uhid nor
        // /dev/uinput; on a desktop this creates a real pad, so only the
        // error path is checked, and only when the node is missing.
        if !std::path::Path::new("/dev/uhid").exists() {
            assert!(LinuxVirtualPad::create(Target::DualSense, 0).is_err());
        }
    }
}
