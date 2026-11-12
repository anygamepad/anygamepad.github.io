//! Controller information: what a DualSense or DualSense Edge tells about
//! its firmware, its hardware revision and how much it was used, and a
//! DualShock 4 about its firmware.
//!
//! [`InfoReader`] is sans-IO: it says what to send and read
//! ([`InfoReader::step`]), takes the answers ([`InfoReader::answer`]) and
//! ends with a [`ControllerInfo`]. It reads:
//!
//! - the firmware information ([`firmware`]), and feature
//!   report `0x22` for the version of the Bluetooth patch;
//! - the answers to a few of the pad's *test commands*: a command goes out
//!   in feature report `0x80` (a device and an action), and feature report
//!   `0x81` is read until it echoes them with a result, 56 bytes a page.
//!   They give the board revision and the colour, the model of the adaptive
//!   trigger motors, the touchpad's firmware (USB only), the battery
//!   voltage, and the usage counters the pad keeps (time in use, charges,
//!   connections, presses of each button, stick travel).
//!
//! # Identifiers
//!
//! The pad also answers with identifiers of the unit: its serial number,
//! Bluetooth address, board and processor IDs, and the barcodes of its
//! battery and motors. anygamepad never collects those (AGENTS.md), so the
//! reader does not ask for them, with one exception: the board revision and
//! the colour are two characters of the serial number, which is read,
//! reduced to those and dropped at once. The usage block and the trigger
//! answers also carry serial numbers; they are skipped. Nothing in a
//! [`ControllerInfo`] tells one unit from another of the same model,
//! although the usage counters are specific enough to recognize a unit seen
//! twice: show them, do not store them.
//!
//! # Caveats
//!
//! Only read commands are sent. Like every feature report, the first read
//! switches a Bluetooth pad to its full report (see the
//! [parent module](super)). Layouts are from `daidr/dualsense-tester`
//! (`5bc7ece`, MIT), which reads them on real pads; they are not checked on
//! a pad here yet. Answers are not checksummed on Bluetooth: the echo of
//! the command tells an answer from noise.

use anygamepad_model::{Axis, Button, Transport};

use super::super::crc32;
use super::Model;
use super::firmware::{self, FirmwareInfo};

/// The feature report a test command goes out in.
pub const COMMAND_REPORT: u8 = 0x80;
/// The feature report the answer to a test command is read from.
pub const ANSWER_REPORT: u8 = 0x81;
/// The feature report holding the version of the Bluetooth patch.
pub const PATCH_REPORT: u8 = 0x22;
/// Data bytes of [`COMMAND_REPORT`] as the DualSense's descriptors declare
/// it (the report ID not counted).
pub const COMMAND_SIZE: usize = 63;

/// Bytes of an answer's page.
const PAGE: usize = 56;
/// How long the pad may take to answer a command, or to give its next
/// page, in milliseconds.
const TIMEOUT_MS: u64 = 1000;
/// The pause between two reads of an answer that is not there yet.
const RETRY_MS: u32 = 10;

/// Hardware versions from which the pad keeps its serial number and the
/// trigger motors' information (daidr's tester).
const TRACEABLE_HARDWARE: u32 = 777;
/// Main firmware versions from which it answers for them.
const TRACEABLE_FIRMWARE: u32 = 65_655;
/// Update versions above which the usage block is complete; older firmware
/// only keeps the time in use.
const USAGE_UPDATE: u16 = 1106;
const USAGE_UPDATE_EDGE: u16 = 374;

/// What the backend does next for an [`InfoReader`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Send this feature report (report ID first), then ask again; call
    /// [`InfoReader::failed`] first if it could not be sent.
    Send(Vec<u8>),
    /// Read the feature report with this ID and pass it to
    /// [`InfoReader::answer`], or call [`InfoReader::failed`].
    Read(u8),
    /// Ask again in this many milliseconds.
    Wait(u32),
    /// Nothing left: [`InfoReader::finish`] gives the information.
    Done,
}

/// A colour or edition of the DualSense, as its serial number codes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour([u8; 2]);

impl Colour {
    /// The two characters of the code (`00`, `Z3`).
    pub fn code(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("")
    }

    /// The colour's name, for the codes known so far.
    pub fn name(&self) -> Option<&'static str> {
        Some(match &self.0 {
            b"00" => "White",
            b"01" => "Midnight Black",
            b"02" => "Cosmic Red",
            b"03" => "Nova Pink",
            b"04" => "Galactic Purple",
            b"05" => "Starlight Blue",
            b"06" => "Grey Camouflage",
            b"07" => "Volcanic Red",
            b"08" => "Sterling Silver",
            b"09" => "Cobalt Blue",
            b"10" => "Chroma Teal",
            b"11" => "Chroma Indigo",
            b"12" => "Chroma Pearl",
            b"13" => "Techno Red",
            b"30" => "30th Anniversary",
            b"Z1" => "God of War Ragnarök",
            b"Z2" => "Marvel's Spider-Man 2",
            b"Z3" => "Astro Bot",
            b"Z4" => "Fortnite",
            b"Z5" => "Monster Hunter Wilds",
            b"Z6" => "The Last of Us",
            b"Z7" => "Ghost of Yōtei",
            b"ZB" => "Icon Blue",
            b"ZC" => "Astro Bot Joyful",
            b"ZE" => "Genshin Impact",
            _ => return None,
        })
    }
}

/// How far a DualSense Edge's trigger was pulled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stroke {
    /// The short stop.
    Short,
    /// The middle stop.
    Middle,
    /// The full travel.
    Full,
}

/// One of the usage counters a pad keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stat {
    /// Records written to the pad's usage log.
    Records,
    /// Time in use, in seconds.
    ActiveSeconds,
    /// Time charging, in seconds.
    ChargingSeconds,
    /// Time its haptic actuators ran, in seconds.
    HapticsSeconds,
    /// Time the left adaptive trigger ran, in seconds.
    LeftTriggerSeconds,
    /// Time the right adaptive trigger ran, in seconds.
    RightTriggerSeconds,
    /// Battery charges started.
    ChargeCycles,
    /// Battery charges that reached full.
    FullCharges,
    /// Times a charger was connected.
    ChargerConnections,
    /// USB ports found to be standard downstream ports.
    UsbSdp,
    /// USB ports found to be charging downstream ports.
    UsbCdp,
    /// USB ports found to be dedicated charging ports.
    UsbDcp,
    /// USB Type-C sources of 1.5 A.
    UsbTypeC1500,
    /// USB Type-C sources of 3 A.
    UsbTypeC3000,
    /// USB ports of an unknown kind.
    UsbUnknown,
    /// Charges stopped by the battery's voltage.
    VoltageErrors,
    /// Charges stopped by the battery's temperature.
    TemperatureErrors,
    /// Errors of the power management chip.
    PmicErrors,
    /// Headsets (with a microphone) plugged in.
    HeadsetDetections,
    /// Headphones plugged in.
    HeadphoneDetections,
    /// USB connections.
    UsbConnections,
    /// Bluetooth connections.
    BluetoothConnections,
    /// Bluetooth connections lost to a timeout.
    BluetoothTimeouts,
    /// Start-up errors of the secondary processor.
    SubCpuErrors,
    /// Authentication challenges from a console.
    AuthChallenges,
    /// Authentications that succeeded.
    AuthSuccesses,
    /// Authentications that failed.
    AuthFailures,
    /// Presses of a button.
    Presses(Button),
    /// Touches of the touchpad.
    TouchpadTouches,
    /// Round trips of a stick axis.
    StickCycles(Axis),
    /// The range a stick axis covered, in the pad's own units.
    StickRange(Axis),
    /// Pulls of a DualSense Edge's trigger ([`Axis::LeftTrigger`] or
    /// [`Axis::RightTrigger`]) to a stop.
    TriggerStrokes(Axis, Stroke),
    /// Presses of a DualSense Edge's function button, left (`false`) or
    /// right (`true`).
    FunctionPresses(bool),
}

impl Stat {
    /// A stable name: `active_seconds`, `presses.south`,
    /// `stick_cycles.left_stick_x`, `trigger_strokes.left_trigger.full`.
    pub fn name(self) -> String {
        let plain = match self {
            Self::Records => "records",
            Self::ActiveSeconds => "active_seconds",
            Self::ChargingSeconds => "charging_seconds",
            Self::HapticsSeconds => "haptics_seconds",
            Self::LeftTriggerSeconds => "left_trigger_seconds",
            Self::RightTriggerSeconds => "right_trigger_seconds",
            Self::ChargeCycles => "charge_cycles",
            Self::FullCharges => "full_charges",
            Self::ChargerConnections => "charger_connections",
            Self::UsbSdp => "usb_sdp",
            Self::UsbCdp => "usb_cdp",
            Self::UsbDcp => "usb_dcp",
            Self::UsbTypeC1500 => "usb_type_c_1500",
            Self::UsbTypeC3000 => "usb_type_c_3000",
            Self::UsbUnknown => "usb_unknown",
            Self::VoltageErrors => "voltage_errors",
            Self::TemperatureErrors => "temperature_errors",
            Self::PmicErrors => "pmic_errors",
            Self::HeadsetDetections => "headset_detections",
            Self::HeadphoneDetections => "headphone_detections",
            Self::UsbConnections => "usb_connections",
            Self::BluetoothConnections => "bluetooth_connections",
            Self::BluetoothTimeouts => "bluetooth_timeouts",
            Self::SubCpuErrors => "sub_cpu_errors",
            Self::AuthChallenges => "auth_challenges",
            Self::AuthSuccesses => "auth_successes",
            Self::AuthFailures => "auth_failures",
            Self::TouchpadTouches => "touchpad_touches",
            Self::Presses(button) => return format!("presses.{button}"),
            Self::StickCycles(axis) => return format!("stick_cycles.{axis}"),
            Self::StickRange(axis) => return format!("stick_range.{axis}"),
            Self::TriggerStrokes(axis, stroke) => {
                let stroke = match stroke {
                    Stroke::Short => "short",
                    Stroke::Middle => "middle",
                    Stroke::Full => "full",
                };
                return format!("trigger_strokes.{axis}.{stroke}");
            }
            Self::FunctionPresses(right) => {
                return format!("function_presses.{}", if right { "right" } else { "left" });
            }
        };
        plain.to_owned()
    }
}

/// What a pad told about itself (see the [module documentation](self)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerInfo {
    /// The firmware information; `None` when the pad did not answer with
    /// one, as many compatible pads do not.
    pub firmware: Option<FirmwareInfo>,
    /// Version of the Bluetooth patch.
    pub bluetooth_patch: Option<u32>,
    /// Board revision, 1 to 5: `BDM-010` to `BDM-050`.
    pub board: Option<u8>,
    /// Colour or edition.
    pub colour: Option<Colour>,
    /// Model of the left and right adaptive trigger motors.
    pub trigger_motors: [Option<String>; 2],
    /// The touchpad controller's firmware version, as it answers it (USB
    /// only).
    pub touchpad_firmware: Option<[u8; 8]>,
    /// Battery voltage, in millivolts.
    pub battery_millivolts: Option<u16>,
    /// Usage counters, in the pad's order; empty when it keeps none.
    pub usage: Vec<(Stat, u32)>,
}

impl ControllerInfo {
    /// The board's name (`BDM-020`).
    pub fn board_name(&self) -> Option<String> {
        self.board.map(|revision| format!("BDM-0{revision}0"))
    }

    /// The value of a usage counter.
    pub fn stat(&self, stat: Stat) -> Option<u32> {
        self.usage
            .iter()
            .find_map(|&(s, value)| (s == stat).then_some(value))
    }
}

/// What a test command's answer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Serial,
    /// The right trigger's when `true`.
    TriggerMotor(bool),
    TouchpadFirmware,
    Battery,
    Usage,
}

#[derive(Clone, Copy, Debug)]
struct Command {
    kind: Kind,
    device: u8,
    action: u8,
    parameter: Option<u8>,
    /// Bytes of the answer.
    length: usize,
    /// The usage block: every page says more follow, and the pad stops
    /// echoing the command after the last.
    open_ended: bool,
}

#[derive(Clone, Copy, Debug)]
enum Task {
    Firmware,
    Patch,
    Command(Command),
}

#[derive(Clone, Debug)]
enum Phase {
    /// Nothing sent or read for the task yet.
    Start,
    /// A read is with the backend.
    Reading,
    /// The answer was not there: wait, then read again.
    Waiting,
}

/// Reads a pad's [`ControllerInfo`] (see the [module documentation](self)).
#[derive(Clone, Debug)]
pub struct InfoReader {
    model: Model,
    transport: Transport,
    command_size: usize,
    /// Tasks left, the next one last.
    tasks: Vec<Task>,
    phase: Phase,
    /// The answer to the command in progress, so far.
    data: Vec<u8>,
    /// When the command in progress last moved on.
    since: Option<u64>,
    now: u64,
    info: ControllerInfo,
}

impl InfoReader {
    /// A reader for a pad of this model on this transport. With an unknown
    /// transport it reads the firmware information only: test commands need
    /// a checksum on Bluetooth and none on USB.
    pub fn new(model: Model, transport: Transport) -> Self {
        Self {
            model,
            transport,
            command_size: COMMAND_SIZE,
            tasks: vec![Task::Firmware],
            phase: Phase::Start,
            data: Vec::new(),
            since: None,
            now: 0,
            info: ControllerInfo::default(),
        }
    }

    /// Sets the data bytes of [`COMMAND_REPORT`], when the pad's report
    /// descriptor tells them ([`COMMAND_SIZE`] otherwise).
    #[must_use]
    pub fn command_size(mut self, bytes: usize) -> Self {
        if bytes >= 8 {
            self.command_size = bytes;
        }
        self
    }

    /// What to do next, at `now_ms` on any steady clock.
    pub fn step(&mut self, now_ms: u64) -> Step {
        self.now = now_ms;
        loop {
            let Some(&task) = self.tasks.last() else {
                return Step::Done;
            };
            match (task, &self.phase) {
                (Task::Firmware, Phase::Start) => {
                    self.phase = Phase::Reading;
                    return Step::Read(firmware::report_id(self.model));
                }
                (Task::Patch, Phase::Start) => {
                    self.phase = Phase::Reading;
                    return Step::Read(PATCH_REPORT);
                }
                (Task::Command(command), Phase::Start) => {
                    self.data.clear();
                    self.since = Some(now_ms);
                    self.phase = Phase::Waiting;
                    return Step::Send(self.command_report(command));
                }
                (Task::Command(_), Phase::Waiting) => {
                    let since = *self.since.get_or_insert(now_ms);
                    if now_ms.saturating_sub(since) >= TIMEOUT_MS {
                        self.complete();
                        continue;
                    }
                    self.phase = Phase::Reading;
                    return Step::Read(ANSWER_REPORT);
                }
                // The backend asks again without having answered: the read
                // is still its to do.
                (Task::Command(_), Phase::Reading) => return Step::Read(ANSWER_REPORT),
                (Task::Firmware, _) => return Step::Read(firmware::report_id(self.model)),
                (Task::Patch, _) => return Step::Read(PATCH_REPORT),
            }
        }
    }

    /// The feature report [`Step::Read`] asked for, report ID first.
    pub fn answer(&mut self, report: &[u8]) -> Option<Step> {
        let &task = self.tasks.last()?;
        if !matches!(self.phase, Phase::Reading) {
            return None;
        }
        match task {
            Task::Firmware => {
                self.info.firmware = firmware::parse(self.model, report);
                self.tasks.clear();
                self.phase = Phase::Start;
                self.plan();
            }
            Task::Patch => {
                if report.first() == Some(&PATCH_REPORT) && report.len() >= 35 {
                    self.info.bluetooth_patch = Some(le32(report, 31));
                }
                self.next_task();
            }
            Task::Command(command) => return self.page(command, report),
        }
        None
    }

    /// The last [`Step::Send`] or [`Step::Read`] failed: the reader gives
    /// up on what it was reading and goes on with the rest.
    pub fn failed(&mut self) {
        match self.tasks.last() {
            // Without the firmware information nothing else is asked.
            Some(Task::Firmware) => self.tasks.clear(),
            Some(Task::Command(_)) => {
                self.complete();
                return;
            }
            _ => {}
        }
        self.next_task();
    }

    /// The information read so far.
    pub fn finish(self) -> ControllerInfo {
        self.info
    }

    /// Queues what this pad answers, once its firmware is known.
    fn plan(&mut self) {
        let Some(firmware) = &self.info.firmware else {
            return;
        };
        if self.model == Model::DualShock4
            || !matches!(self.transport, Transport::Usb | Transport::Bluetooth)
            // Retail firmware (daidr's tester asks nothing of the others).
            || !matches!(firmware.kind, Some(2 | 3))
        {
            return;
        }
        let hardware = firmware.hardware & 0xffff;
        let traceable = hardware >= TRACEABLE_HARDWARE;
        let command = |kind, device, action, parameter, length| {
            Task::Command(Command {
                kind,
                device,
                action,
                parameter,
                length,
                open_ended: false,
            })
        };
        let mut tasks = Vec::new();
        if traceable && firmware.firmware >= TRACEABLE_FIRMWARE {
            tasks.push(Task::Patch);
            tasks.push(command(Kind::Serial, 1, 19, None, 32));
        }
        if traceable {
            tasks.push(command(Kind::TriggerMotor(false), 7, 37, Some(1), 43));
            tasks.push(command(Kind::TriggerMotor(true), 7, 37, Some(2), 43));
        }
        // The touchpad only answers over USB.
        if self.transport == Transport::Usb {
            tasks.push(command(Kind::TouchpadFirmware, 5, 4, None, 8));
        }
        tasks.push(command(Kind::Battery, 4, 3, None, 4));
        let pages = if self.model == Model::DualSenseEdge {
            6
        } else {
            4
        };
        tasks.push(Task::Command(Command {
            kind: Kind::Usage,
            device: 0x70,
            action: 1,
            parameter: None,
            length: pages * PAGE,
            open_ended: true,
        }));
        tasks.reverse();
        self.tasks = tasks;
    }

    fn next_task(&mut self) {
        self.tasks.pop();
        self.phase = Phase::Start;
        self.data.clear();
        self.since = None;
    }

    /// The feature report of a test command: the device, the action and its
    /// parameter, padded, with the CRC-32 Bluetooth asks for.
    fn command_report(&self, command: Command) -> Vec<u8> {
        let mut report = vec![0u8; 1 + self.command_size];
        report[0] = COMMAND_REPORT;
        report[1] = command.device;
        report[2] = command.action;
        if let Some(parameter) = command.parameter {
            report[3] = parameter;
        }
        if self.transport == Transport::Bluetooth {
            // The Bluetooth HID header of a feature report being set.
            let end = report.len() - 4;
            let crc = crc32(&[0x53], &report[..end]);
            report[end..].copy_from_slice(&crc.to_le_bytes());
        }
        report
    }

    /// One read of the answer report while a command is in progress.
    fn page(&mut self, command: Command, report: &[u8]) -> Option<Step> {
        let echoed = report.len() >= 4
            && report[0] == ANSWER_REPORT
            && report[1] == command.device
            && report[2] == command.action;
        // 2: complete; 3: complete, more pages follow.
        let status = if echoed { report[3] } else { 0 };
        if status == 2 || status == 3 {
            let page = report.get(4..).unwrap_or(&[]);
            let page = &page[..page.len().min(PAGE)];
            let room = command.length.saturating_sub(self.data.len());
            let take = if status == 3 { PAGE } else { room }.min(room);
            self.data.extend_from_slice(&page[..take.min(page.len())]);
            // A short page: zeros stand for what it did not carry.
            let missing = take.saturating_sub(page.len());
            self.data.extend(core::iter::repeat_n(0, missing));
            self.since = Some(self.now);
            if status == 2 || self.data.len() >= command.length {
                self.complete();
            } else {
                // The next page is there to read.
                self.phase = Phase::Waiting;
            }
            return None;
        }
        if command.open_ended && !echoed && !self.data.is_empty() {
            // The usage block ends when the pad stops echoing the command.
            self.complete();
            return None;
        }
        self.phase = Phase::Waiting;
        Some(Step::Wait(RETRY_MS))
    }

    /// Ends the command in progress with the bytes it gave.
    fn complete(&mut self) {
        if let Some(&Task::Command(command)) = self.tasks.last() {
            let mut data = core::mem::take(&mut self.data);
            let whole = data.len() >= command.length || (command.open_ended && !data.is_empty());
            if whole {
                data.resize(command.length, 0);
                self.apply(command.kind, &data);
            }
            // The serial number is not kept beyond `apply`.
            data.fill(0);
        }
        self.next_task();
    }

    fn apply(&mut self, kind: Kind, data: &[u8]) {
        match kind {
            Kind::Serial => {
                // The second character is the board revision, the fifth
                // and sixth the colour; the rest tells the unit.
                if let revision @ b'1'..=b'5' = data[1] {
                    self.info.board = Some(revision - b'0');
                }
                let code = [data[4], data[5]];
                if code.iter().all(u8::is_ascii_alphanumeric) {
                    self.info.colour = Some(Colour(code));
                }
            }
            Kind::TriggerMotor(right) => {
                // After a status byte and the motor's serial number.
                if data[0] == 0 {
                    self.info.trigger_motors[usize::from(right)] = ascii(&data[19..27]);
                }
            }
            Kind::TouchpadFirmware => {
                let mut version = [0u8; 8];
                version.copy_from_slice(&data[..8]);
                self.info.touchpad_firmware = Some(version);
            }
            Kind::Battery => {
                self.info.battery_millivolts = Some(le16(data, 0));
            }
            Kind::Usage => {
                let update = self.info.firmware.as_ref().and_then(|f| f.update);
                let edge = self.model == Model::DualSenseEdge;
                let complete = if edge {
                    USAGE_UPDATE_EDGE
                } else {
                    USAGE_UPDATE
                };
                // A status byte comes first.
                let block = &data[1..];
                self.info.usage = if update.is_some_and(|update| update > complete) {
                    usage(block, edge)
                } else {
                    vec![(Stat::ActiveSeconds, low24(block, 21))]
                };
            }
        }
    }
}

/// ASCII text with its NUL bytes removed; `None` when empty or not text.
fn ascii(bytes: &[u8]) -> Option<String> {
    let text: Vec<u8> = bytes.iter().copied().filter(|&b| b != 0).collect();
    (!text.is_empty() && text.iter().all(|b| (0x20..0x7f).contains(b)))
        .then(|| String::from_utf8_lossy(&text).trim().to_owned())
}

fn le16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn le32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

// The counters live in flash memory: an erased or factory-marked field
// reads as high values, which must not show as counts.

/// A count on 16 bits whose upper half may be erased.
fn count(data: &[u8], at: usize) -> u32 {
    let (low, high) = (u32::from(le16(data, at)), le16(data, at + 2));
    if high != 0 && high >= 0xff00 && low >= 0xff00 {
        0
    } else {
        low
    }
}

/// A count kept in the upper half of its field.
fn count_high(data: &[u8], at: usize) -> u32 {
    let value = le16(data, at + 2);
    if value < 0xff00 { u32::from(value) } else { 0 }
}

/// A 24-bit value.
fn low24(data: &[u8], at: usize) -> u32 {
    let value = le32(data, at) & 0x00ff_ffff;
    if value < 0x00ff_ff00 { value } else { 0 }
}

/// A 16-bit counter whose upper byte may be erased.
fn short(data: &[u8], at: usize) -> u32 {
    if data[at + 1] < 0xe0 {
        u32::from(le16(data, at))
    } else {
        u32::from(data[at])
    }
}

/// The DualSense Edge's record count, above a factory offset.
fn edge_records(data: &[u8], at: usize) -> u32 {
    const OFFSET: u32 = 0xedf0_0000;
    match le32(data, at) {
        u32::MAX => 0,
        value => value.saturating_sub(OFFSET),
    }
}

/// A DualSense Edge trigger stroke count.
fn edge_strokes(data: &[u8], at: usize) -> u32 {
    let value = le32(data, at);
    let (low, high) = (value & 0xffff, value >> 16);
    if low >= 0xff00 {
        let decoded = i64::from(value) - i64::from(high * 0x1_0000 + 0xff00);
        if (0..100_000).contains(&decoded) {
            return decoded as u32;
        }
    }
    if high == 0 { low } else { value }
}

/// The usage block's counters. Its first 17 bytes are the serial number,
/// which is not read; neither are the serial numbers of the DualSense
/// Edge's stick modules.
fn usage(data: &[u8], edge: bool) -> Vec<(Stat, u32)> {
    use Stat::*;
    let u16_at = |at| u32::from(le16(data, at));
    let mut stats = vec![
        (
            Records,
            if edge {
                edge_records(data, 17)
            } else {
                le32(data, 17)
            },
        ),
        (ActiveSeconds, low24(data, 21)),
        (ChargingSeconds, low24(data, 25)),
        (HapticsSeconds, low24(data, 29)),
        (LeftTriggerSeconds, count(data, 33)),
        (RightTriggerSeconds, count(data, 37)),
        (UsbConnections, u16_at(66)),
        (BluetoothConnections, u16_at(68)),
        (BluetoothTimeouts, u16_at(70)),
        (UsbSdp, u16_at(43)),
        (UsbCdp, short(data, 45)),
        (UsbDcp, u16_at(47)),
        (UsbTypeC1500, u16_at(49)),
        (UsbTypeC3000, u16_at(51)),
        (UsbUnknown, u16_at(53)),
        (ChargerConnections, u16_at(55)),
        (ChargeCycles, short(data, 41)),
        (FullCharges, u16_at(60)),
        (HeadsetDetections, u16_at(62)),
        (HeadphoneDetections, u16_at(64)),
        (VoltageErrors, u32::from(data[57])),
        (TemperatureErrors, u32::from(data[58])),
        (PmicErrors, u32::from(data[59])),
        (SubCpuErrors, u16_at(72)),
        (AuthChallenges, count(data, 74)),
        (AuthSuccesses, count(data, 78)),
        (AuthFailures, count(data, 82)),
    ];
    let sticks = |stats: &mut Vec<(Stat, u32)>, axes: [Axis; 2], at: usize| {
        for (i, axis) in axes.into_iter().enumerate() {
            stats.push((StickCycles(axis), le32(data, at + 6 * i)));
            stats.push((StickRange(axis), u16_at(at + 6 * i + 4)));
        }
    };
    const FACE: [Button; 9] = [
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
        Button::North,
        Button::South,
        Button::West,
        Button::East,
        Button::LeftShoulder,
    ];
    if edge {
        for (i, button) in FACE.into_iter().enumerate() {
            stats.push((Presses(button), count(data, 86 + 4 * i)));
        }
        stats.push((Presses(Button::RightShoulder), count(data, 134)));
        for (axis, at) in [(Axis::LeftTrigger, 122), (Axis::RightTrigger, 138)] {
            for (i, stroke) in [Stroke::Short, Stroke::Middle, Stroke::Full]
                .into_iter()
                .enumerate()
            {
                stats.push((TriggerStrokes(axis, stroke), edge_strokes(data, at + 4 * i)));
            }
        }
        stats.push((TouchpadTouches, count(data, 150)));
        stats.push((Presses(Button::TouchpadClick), count(data, 154)));
        stats.push((Presses(Button::Start), count(data, 158)));
        stats.push((Presses(Button::Select), count(data, 162)));
        stats.push((Presses(Button::Home), count(data, 166)));
        stats.push((Presses(Button::Mute), count_high(data, 170)));
        stats.push((Presses(Button::LeftPaddle1), count_high(data, 174)));
        stats.push((Presses(Button::RightPaddle1), count_high(data, 178)));
        // Each stick module: 17 bytes of serial number, then its counters.
        let modules = [
            (199, [Axis::LeftStickX, Axis::LeftStickY], Button::LeftStick),
            (
                236,
                [Axis::RightStickX, Axis::RightStickY],
                Button::RightStick,
            ),
        ];
        for (right, (at, axes, click)) in modules.into_iter().enumerate() {
            sticks(&mut stats, axes, at);
            stats.push((Presses(click), le32(data, at + 12)));
            stats.push((FunctionPresses(right == 1), le32(data, at + 16)));
        }
    } else {
        sticks(&mut stats, [Axis::LeftStickX, Axis::LeftStickY], 86);
        sticks(&mut stats, [Axis::RightStickX, Axis::RightStickY], 98);
        const REST: [Option<Button>; 11] = [
            Some(Button::LeftTrigger),
            Some(Button::LeftStick),
            Some(Button::RightShoulder),
            Some(Button::RightTrigger),
            Some(Button::RightStick),
            None,
            Some(Button::TouchpadClick),
            Some(Button::Start),
            Some(Button::Select),
            Some(Button::Home),
            Some(Button::Mute),
        ];
        let buttons = FACE.into_iter().map(Some).chain(REST);
        for (i, button) in buttons.enumerate() {
            let stat = button.map_or(TouchpadTouches, Presses);
            stats.push((stat, count(data, 110 + 4 * i)));
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command's answer: device, action, parameter, bytes, and whether
    /// it is the open-ended usage block.
    type Answer = (u8, u8, Option<u8>, Vec<u8>, bool);

    /// A pad that answers the reader as a DualSense does.
    struct Pad {
        bluetooth: bool,
        firmware: Vec<u8>,
        answers: Vec<Answer>,
        /// Reads of the answer report that say "running" first.
        delay: usize,
        current: Option<(usize, usize)>,
        waited: usize,
        /// The commands received.
        commands: Vec<Vec<u8>>,
        /// Feature reports read.
        reads: Vec<u8>,
    }

    const SERIAL: &[u8; 17] = b"A21600XYZ12345678";

    fn firmware(update: u16) -> Vec<u8> {
        let mut report = vec![0u8; 64];
        report[0] = 0x20;
        report[1..12].copy_from_slice(b"Jun 24 2024");
        report[12..20].copy_from_slice(b"11:16:21");
        report[20..22].copy_from_slice(&2u16.to_le_bytes());
        report[22..24].copy_from_slice(&4u16.to_le_bytes());
        report[24..28].copy_from_slice(&0x0000_0410u32.to_le_bytes());
        report[28..32].copy_from_slice(&0x010f_0000u32.to_le_bytes());
        report[44..46].copy_from_slice(&update.to_le_bytes());
        report[48..52].copy_from_slice(&0x0001_002au32.to_le_bytes());
        report[52..56].copy_from_slice(&0x0002_000au32.to_le_bytes());
        report[56..60].copy_from_slice(&0x0000_0006u32.to_le_bytes());
        report
    }

    fn usage_block() -> Vec<u8> {
        // A status byte, then the block.
        let mut block = vec![0u8; 1 + 4 * PAGE];
        let data = &mut block[1..];
        data[..17].copy_from_slice(SERIAL);
        data[17..21].copy_from_slice(&49u32.to_le_bytes());
        data[21..25].copy_from_slice(&64_334u32.to_le_bytes());
        data[25..29].copy_from_slice(&16_408u32.to_le_bytes());
        data[29..33].copy_from_slice(&811u32.to_le_bytes());
        data[33..37].copy_from_slice(&9354u32.to_le_bytes());
        data[37..41].copy_from_slice(&9311u32.to_le_bytes());
        data[41..43].copy_from_slice(&23u16.to_le_bytes());
        data[43..45].copy_from_slice(&1u16.to_le_bytes());
        data[45..47].copy_from_slice(&10u16.to_le_bytes());
        data[47..49].copy_from_slice(&1u16.to_le_bytes());
        data[66..68].copy_from_slice(&12u16.to_le_bytes());
        data[68..70].copy_from_slice(&37u16.to_le_bytes());
        data[70..72].copy_from_slice(&10u16.to_le_bytes());
        data[74..78].copy_from_slice(&8u32.to_le_bytes());
        data[78..82].copy_from_slice(&26u32.to_le_bytes());
        // An erased field.
        data[82..86].copy_from_slice(&[0xff; 4]);
        data[86..90].copy_from_slice(&21_935u32.to_le_bytes());
        data[90..92].copy_from_slice(&2500u16.to_le_bytes());
        data[92..96].copy_from_slice(&2033u32.to_le_bytes());
        data[96..98].copy_from_slice(&2518u16.to_le_bytes());
        data[104..108].copy_from_slice(&178u32.to_le_bytes());
        data[108..110].copy_from_slice(&2353u16.to_le_bytes());
        data[110..114].copy_from_slice(&528u32.to_le_bytes());
        data[130..134].copy_from_slice(&1209u32.to_le_bytes());
        data[158..162].copy_from_slice(&1252u32.to_le_bytes());
        data[166..170].copy_from_slice(&153u32.to_le_bytes());
        data[170..174].copy_from_slice(&45u32.to_le_bytes());
        data[186..190].copy_from_slice(&9u32.to_le_bytes());
        block
    }

    impl Pad {
        fn dualsense(bluetooth: bool) -> Self {
            let mut serial = vec![0u8; 32];
            serial[..17].copy_from_slice(SERIAL);
            let mut motor = vec![0u8; 43];
            motor[12..19].copy_from_slice(&[0x31, 0x12, 0x42, 0x60, 0x04, 0x32, 0x51]);
            motor[19..27].copy_from_slice(b"M1418AB3");
            Self {
                bluetooth,
                firmware: firmware(0x0520),
                answers: vec![
                    (1, 19, None, serial, false),
                    (7, 37, Some(1), motor.clone(), false),
                    (7, 37, Some(2), motor, false),
                    (5, 4, None, vec![1, 2, 3, 4, 5, 6, 7, 8], false),
                    (4, 3, None, 3120u16.to_le_bytes().to_vec(), false),
                    (0x70, 1, None, usage_block(), true),
                ],
                delay: 2,
                current: None,
                waited: 0,
                commands: Vec::new(),
                reads: Vec::new(),
            }
        }

        fn send(&mut self, report: &[u8]) {
            assert_eq!(report[0], COMMAND_REPORT);
            assert_eq!(report.len(), 1 + COMMAND_SIZE);
            if self.bluetooth {
                let end = report.len() - 4;
                let crc = crc32(&[0x53], &report[..end]);
                assert_eq!(report[end..], crc.to_le_bytes(), "the command's CRC-32");
            } else {
                assert!(report[4..].iter().all(|&b| b == 0));
            }
            self.commands.push(report[1..4].to_vec());
            self.current = self
                .answers
                .iter()
                .position(|(device, action, parameter, ..)| {
                    *device == report[1]
                        && *action == report[2]
                        && parameter.is_none_or(|p| p == report[3])
                })
                .map(|index| (index, 0));
            self.waited = 0;
        }

        fn read(&mut self, report_id: u8) -> Vec<u8> {
            self.reads.push(report_id);
            let mut report = vec![0u8; 64];
            report[0] = report_id;
            match report_id {
                0x20 => return self.firmware.clone(),
                PATCH_REPORT => {
                    report[31..35].copy_from_slice(&0x18u32.to_le_bytes());
                    return report;
                }
                _ => {}
            }
            let Some((index, sent)) = self.current else {
                // Nothing in progress: the header is clear.
                return report;
            };
            let (device, action, _, data, open_ended) = &self.answers[index];
            report[1] = *device;
            report[2] = *action;
            if self.waited < self.delay {
                self.waited += 1;
                report[3] = 1;
                return report;
            }
            let rest = &data[sent.min(data.len())..];
            let last = rest.len() <= PAGE;
            report[3] = if last && !*open_ended { 2 } else { 3 };
            let page = &rest[..rest.len().min(PAGE)];
            report[4..4 + page.len()].copy_from_slice(page);
            self.current = (!last).then_some((index, sent + PAGE));
            report
        }
    }

    /// Runs a reader against a pad, on a clock that only waits move.
    fn run(mut reader: InfoReader, pad: &mut Pad) -> ControllerInfo {
        let mut now = 0u64;
        let mut pending = None;
        for _ in 0..10_000 {
            let step = pending.take().unwrap_or_else(|| reader.step(now));
            match step {
                Step::Send(report) => pad.send(&report),
                Step::Read(id) => pending = reader.answer(&pad.read(id)),
                Step::Wait(ms) => now += u64::from(ms),
                Step::Done => return reader.finish(),
            }
        }
        panic!("the reader does not end");
    }

    #[test]
    fn dualsense_over_usb() {
        let mut pad = Pad::dualsense(false);
        let info = run(InfoReader::new(Model::DualSense, Transport::Usb), &mut pad);
        let firmware = info.firmware.as_ref().unwrap();
        assert_eq!(firmware.build, "Jun 24 2024 11:16:21");
        assert_eq!(firmware.update, Some(0x0520));
        assert_eq!(info.bluetooth_patch, Some(0x18));
        assert_eq!(info.board_name().as_deref(), Some("BDM-020"));
        let colour = info.colour.unwrap();
        assert_eq!((colour.code(), colour.name()), ("00", Some("White")));
        assert_eq!(info.trigger_motors[0].as_deref(), Some("M1418AB3"));
        assert_eq!(info.trigger_motors[1].as_deref(), Some("M1418AB3"));
        assert_eq!(info.touchpad_firmware, Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(info.battery_millivolts, Some(3120));
        assert_eq!(info.stat(Stat::Records), Some(49));
        assert_eq!(info.stat(Stat::ActiveSeconds), Some(64_334));
        assert_eq!(info.stat(Stat::ChargingSeconds), Some(16_408));
        assert_eq!(info.stat(Stat::LeftTriggerSeconds), Some(9354));
        assert_eq!(info.stat(Stat::ChargeCycles), Some(23));
        assert_eq!(info.stat(Stat::UsbCdp), Some(10));
        assert_eq!(info.stat(Stat::BluetoothConnections), Some(37));
        assert_eq!(info.stat(Stat::AuthSuccesses), Some(26));
        // An erased field is no count.
        assert_eq!(info.stat(Stat::AuthFailures), Some(0));
        assert_eq!(info.stat(Stat::StickCycles(Axis::LeftStickX)), Some(21_935));
        assert_eq!(info.stat(Stat::StickRange(Axis::LeftStickY)), Some(2518));
        assert_eq!(info.stat(Stat::StickCycles(Axis::RightStickY)), Some(178));
        assert_eq!(info.stat(Stat::Presses(Button::DPadUp)), Some(528));
        assert_eq!(info.stat(Stat::Presses(Button::South)), Some(1209));
        assert_eq!(info.stat(Stat::Presses(Button::RightTrigger)), Some(1252));
        assert_eq!(info.stat(Stat::TouchpadTouches), Some(153));
        assert_eq!(info.stat(Stat::Presses(Button::TouchpadClick)), Some(45));
        assert_eq!(info.stat(Stat::Presses(Button::Mute)), Some(9));
        // The commands sent, in order; none asks for an identifier alone.
        assert_eq!(
            pad.commands,
            [
                [1, 19, 0],
                [7, 37, 1],
                [7, 37, 2],
                [5, 4, 0],
                [4, 3, 0],
                [0x70, 1, 0]
            ]
        );
    }

    #[test]
    fn dualsense_over_bluetooth() {
        let mut pad = Pad::dualsense(true);
        let info = run(
            InfoReader::new(Model::DualSense, Transport::Bluetooth),
            &mut pad,
        );
        assert_eq!(info.board, Some(2));
        assert_eq!(info.battery_millivolts, Some(3120));
        assert_eq!(info.stat(Stat::Presses(Button::South)), Some(1209));
        // The touchpad is not asked over Bluetooth.
        assert_eq!(info.touchpad_firmware, None);
        assert!(!pad.commands.contains(&vec![5, 4, 0]));
    }

    #[test]
    fn nothing_identifies_the_unit() {
        let mut pad = Pad::dualsense(false);
        let info = run(InfoReader::new(Model::DualSense, Transport::Usb), &mut pad);
        let shown = format!("{info:?}");
        // Neither the serial number nor the motors' serial numbers.
        assert!(!shown.contains("XYZ"));
        assert!(!shown.contains("12345678"));
        assert!(!shown.contains("49, 18, 66"));
        let names: Vec<String> = info.usage.iter().map(|(stat, _)| stat.name()).collect();
        assert!(names.contains(&"presses.south".to_owned()));
        assert!(names.contains(&"stick_cycles.left_stick_x".to_owned()));
        assert!(names.iter().all(|name| !name.contains("serial")));
    }

    #[test]
    fn older_firmware_keeps_the_time_in_use_only() {
        let mut pad = Pad::dualsense(false);
        pad.firmware = firmware(0x0400);
        let info = run(InfoReader::new(Model::DualSense, Transport::Usb), &mut pad);
        assert_eq!(info.usage, [(Stat::ActiveSeconds, 64_334)]);
    }

    #[test]
    fn a_pad_that_answers_no_firmware_is_asked_nothing_else() {
        let mut pad = Pad::dualsense(false);
        pad.firmware = vec![0x20; 8];
        let info = run(InfoReader::new(Model::DualSense, Transport::Usb), &mut pad);
        assert_eq!(info, ControllerInfo::default());
        assert!(pad.commands.is_empty());
        assert_eq!(pad.reads, [0x20]);
    }

    #[test]
    fn firmware_only_for_the_dualshock4_and_unknown_transports() {
        let mut pad = Pad::dualsense(false);
        let reader = InfoReader::new(Model::DualSense, Transport::Unknown);
        let info = run(reader, &mut pad);
        assert!(info.firmware.is_some());
        assert!(pad.commands.is_empty());

        let mut reader = InfoReader::new(Model::DualShock4, Transport::Usb);
        assert_eq!(reader.step(0), Step::Read(0xa3));
        let mut report = vec![0u8; 49];
        report[0] = 0xa3;
        report[1..12].copy_from_slice(b"Sep 21 2018");
        report[17..25].copy_from_slice(b"04:50:51");
        assert_eq!(reader.answer(&report), None);
        assert_eq!(reader.step(0), Step::Done);
        assert!(reader.finish().firmware.is_some());
    }

    #[test]
    fn a_silent_pad_times_out_command_by_command() {
        let mut pad = Pad::dualsense(false);
        // It answers "running" for ever.
        pad.delay = usize::MAX;
        let info = run(InfoReader::new(Model::DualSense, Transport::Usb), &mut pad);
        assert!(info.firmware.is_some());
        assert_eq!(info.bluetooth_patch, Some(0x18));
        assert_eq!(info.board, None);
        assert_eq!(info.battery_millivolts, None);
        assert!(info.usage.is_empty());
        assert_eq!(pad.commands.len(), 6);
    }

    #[test]
    fn failures_skip_what_was_being_read() {
        let mut reader = InfoReader::new(Model::DualSense, Transport::Usb);
        assert_eq!(reader.step(0), Step::Read(0x20));
        reader.failed();
        assert_eq!(reader.step(0), Step::Done);

        let mut reader = InfoReader::new(Model::DualSense, Transport::Usb);
        reader.step(0);
        reader.answer(&firmware(0x0520));
        assert_eq!(reader.step(0), Step::Read(PATCH_REPORT));
        reader.failed();
        // The serial number's command: it cannot be sent.
        assert!(matches!(reader.step(0), Step::Send(_)));
        reader.failed();
        let Step::Send(report) = reader.step(0) else {
            panic!("the next command");
        };
        assert_eq!(report[1..4], [7, 37, 1]);
    }

    #[test]
    fn command_size_from_the_descriptor() {
        let mut reader = InfoReader::new(Model::DualSense, Transport::Bluetooth).command_size(47);
        reader.step(0);
        reader.answer(&firmware(0x0520));
        reader.step(0);
        reader.answer(&[PATCH_REPORT; 64]);
        let Step::Send(report) = reader.step(0) else {
            panic!("a command");
        };
        assert_eq!(report.len(), 48);
        let crc = crc32(&[0x53], &report[..44]);
        assert_eq!(report[44..], crc.to_le_bytes());
    }

    #[test]
    fn dualsense_edge_usage() {
        let mut data = vec![0u8; 6 * PAGE];
        data[17..21].copy_from_slice(&(0xedf0_0000u32 + 7).to_le_bytes());
        data[21..25].copy_from_slice(&100u32.to_le_bytes());
        data[106..110].copy_from_slice(&33u32.to_le_bytes());
        data[130..134].copy_from_slice(&0x0003_ff05u32.to_le_bytes());
        data[172..174].copy_from_slice(&4u16.to_le_bytes());
        data[176..178].copy_from_slice(&0xffffu16.to_le_bytes());
        data[182..199].copy_from_slice(b"STICKMODULE000001");
        data[199..203].copy_from_slice(&500u32.to_le_bytes());
        data[203..205].copy_from_slice(&2400u16.to_le_bytes());
        data[211..215].copy_from_slice(&12u32.to_le_bytes());
        data[252..256].copy_from_slice(&3u32.to_le_bytes());
        let stats = usage(&data, true);
        let stat = |stat| stats.iter().find_map(|&(s, v)| (s == stat).then_some(v));
        assert_eq!(stat(Stat::Records), Some(7));
        assert_eq!(stat(Stat::ActiveSeconds), Some(100));
        assert_eq!(stat(Stat::Presses(Button::South)), Some(33));
        assert_eq!(
            stat(Stat::TriggerStrokes(Axis::LeftTrigger, Stroke::Full)),
            Some(5)
        );
        assert_eq!(stat(Stat::Presses(Button::Mute)), Some(4));
        assert_eq!(stat(Stat::Presses(Button::LeftPaddle1)), Some(0));
        assert_eq!(stat(Stat::StickCycles(Axis::LeftStickX)), Some(500));
        assert_eq!(stat(Stat::StickRange(Axis::LeftStickX)), Some(2400));
        assert_eq!(stat(Stat::Presses(Button::LeftStick)), Some(12));
        assert_eq!(stat(Stat::FunctionPresses(true)), Some(3));
        assert_eq!(
            Stat::TriggerStrokes(Axis::LeftTrigger, Stroke::Full).name(),
            "trigger_strokes.left_trigger.full"
        );
        assert_eq!(Stat::FunctionPresses(false).name(), "function_presses.left");
    }
}
