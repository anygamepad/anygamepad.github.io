//! [`HidDriver`]: the protocol drivers behind one sans-IO interface (C7),
//! for every backend that moves raw HID reports: hidraw on Linux and WebHID
//! in the browser now, Windows HID and IOKit later.
//!
//! The backend feeds every input report to [`HidDriver::receive`], sends
//! what [`HidDriver::transmit`] asks for, and answers the feature reports it
//! asks to read with [`HidDriver::feature`]. The driver keeps the pad's
//! canonical state and motion samples, and turns [`OutputCommand`]s into
//! output reports.
//!
//! A new driver only decodes: it sends nothing until [`HidDriver::start`],
//! which makes the pad send its full reports (PlayStation calibration read,
//! Switch Pro set-up). Reading a PlayStation pad's calibration over
//! Bluetooth switches it to its full report for every application on the
//! host (see [`playstation`]), so backends call `start` only when the user
//! asked for it or when nothing else reads the pad.
//!
//! Reports start with their report ID, as hidraw gives them.

use std::collections::VecDeque;

use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad_model::{
    Axis, Button, GamepadState, MotionSample, Output, OutputCommand, OutputSet, Rumble, Sensor,
    SensorSet, Transport,
};

use super::playstation::{self, Calibration, DecodeError, Model, MotionTracker, OutputReport};
use super::switch_pro::{self, Setup, Writer};

/// How long the driver waits for the Switch Pro Controller to answer a
/// set-up message before sending it again, in microseconds.
pub const RESEND_US: u64 = 100_000;

/// Motion samples kept until [`HidDriver::take_motion`] collects them; older
/// ones are dropped beyond this (a second of a Switch Pro Controller).
pub const MAX_QUEUED_SAMPLES: usize = 200;

/// Something the backend must do for the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Write this output report (report ID first).
    Output(Vec<u8>),
    /// Read the feature report with this ID and pass it to
    /// [`HidDriver::feature`].
    ReadFeature(u8),
}

/// A pad the library drives over raw HID (see the
/// [module documentation](self)).
#[derive(Clone, Debug)]
pub struct HidDriver {
    transport: Transport,
    protocol: Protocol,
    state: GamepadState,
    motion: VecDeque<MotionSample>,
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
enum Protocol {
    PlayStation(PlayStation),
    SwitchPro(SwitchPro),
}

#[derive(Clone, Debug)]
struct PlayStation {
    model: Model,
    output: OutputReport,
    /// Commands recorded since the last output report.
    dirty: bool,
    /// Bluetooth output report sequence (DualSense).
    sequence: u8,
    tracker: MotionTracker,
    /// `start` was called: read the calibration once the transport is
    /// known.
    started: bool,
    /// The calibration report asked for and not answered yet.
    reading: Option<u8>,
    /// Whether the read was handed to the backend.
    asked: bool,
}

#[derive(Clone, Debug)]
struct SwitchPro {
    /// The set-up exchange while it runs.
    setup: Option<Setup>,
    /// The progress and time of the last set-up message sent.
    sent: Option<(usize, u64)>,
    writer: Writer,
    calibration: switch_pro::Calibration,
    tracker: switch_pro::MotionTracker,
    /// Commands waiting for the set-up to end, or for `transmit`.
    player: Option<u8>,
    rumble: Option<Rumble>,
}

impl HidDriver {
    /// A driver for the pad with these USB IDs, if the library has one.
    /// The Switch Pro Controller needs its transport (its set-up differs);
    /// PlayStation pads tell it with their first input report.
    pub fn new(vendor_id: u16, product_id: u16, transport: Transport) -> Option<Self> {
        let protocol = if let Some(model) = Model::from_ids(vendor_id, product_id) {
            Protocol::PlayStation(PlayStation {
                model,
                output: OutputReport::new(model),
                dirty: false,
                sequence: 0,
                tracker: MotionTracker::new(model, Calibration::NOMINAL),
                started: false,
                reading: None,
                asked: false,
            })
        } else if (vendor_id, product_id) == SWITCH_PRO_IDS && transport != Transport::Unknown {
            Protocol::SwitchPro(SwitchPro {
                setup: None,
                sent: None,
                writer: Writer::new(),
                calibration: switch_pro::Calibration::default(),
                tracker: switch_pro::MotionTracker::default(),
                player: None,
                rumble: None,
            })
        } else {
            return None;
        };
        Some(Self {
            transport,
            protocol,
            state: GamepadState::new(),
            motion: VecDeque::new(),
        })
    }

    /// The transport, once known.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// The outputs [`HidDriver::send`] drives.
    pub fn outputs(&self) -> OutputSet {
        match &self.protocol {
            Protocol::PlayStation(ps) if ps.model == Model::DualShock4 => {
                OutputSet::from([Output::Rumble, Output::Lightbar])
            }
            Protocol::PlayStation(_) => {
                OutputSet::from([Output::Rumble, Output::Lightbar, Output::PlayerLeds])
            }
            Protocol::SwitchPro(_) => OutputSet::from([Output::Rumble, Output::PlayerLeds]),
        }
    }

    /// The raw layout [`HidDriver::fill`] fills: canonical sources, as the
    /// driver decodes the pad itself.
    pub fn layout(&self) -> RawLayout {
        let (buttons, axes, touchpads): (&[Button], &[Axis], _) = match &self.protocol {
            Protocol::PlayStation(ps) => (
                if ps.model == Model::DualShock4 {
                    &PLAYSTATION_BUTTONS[..PLAYSTATION_BUTTONS.len() - 1]
                } else {
                    &PLAYSTATION_BUTTONS
                },
                &Axis::ALL,
                vec![ps.model.touchpad()],
            ),
            Protocol::SwitchPro(_) => (&SWITCH_PRO_BUTTONS, &Axis::ALL[..4], Vec::new()),
        };
        RawLayout {
            buttons: buttons.iter().copied().map(RawSource::Button).collect(),
            axes: axes
                .iter()
                .map(|&axis| RawAxisInfo {
                    source: RawSource::Axis(axis),
                    min: if axis.is_trigger() { 0.0 } else { -1.0 },
                    max: 1.0,
                })
                .collect(),
            hats: Vec::new(),
            sensors: SensorSet::from([Sensor::Gyroscope, Sensor::Accelerometer]),
            touchpads,
            battery: true,
        }
    }

    /// Makes the pad send its full reports (see the
    /// [module documentation](self)). Calling it again does nothing.
    pub fn start(&mut self) {
        match &mut self.protocol {
            Protocol::PlayStation(ps) => {
                if !ps.started {
                    ps.started = true;
                    ps.request_calibration(self.transport);
                }
            }
            Protocol::SwitchPro(pro) => {
                if pro.setup.is_none() && pro.sent.is_none() {
                    pro.setup = Some(Setup::new(self.transport, pro.player.unwrap_or(1)));
                }
            }
        }
    }

    /// Whether the pad needs nothing more to send its full reports: the
    /// calibration was read, or the set-up exchange ended.
    pub fn is_ready(&self) -> bool {
        match &self.protocol {
            Protocol::PlayStation(ps) => ps.started && ps.reading.is_none(),
            Protocol::SwitchPro(pro) => pro.setup.is_none() && pro.sent.is_some(),
        }
    }

    /// Takes an input report. `Ok(true)` when it carried the pad's state;
    /// `Ok(false)` for reports without it (answers, audio, the Switch Pro
    /// Controller's simple report).
    pub fn receive(&mut self, report: &[u8]) -> Result<bool, DecodeError> {
        let Some((&report_id, data)) = report.split_first() else {
            return Ok(false);
        };
        match &mut self.protocol {
            Protocol::PlayStation(ps) => {
                let input = match playstation::decode(ps.model, report_id, data) {
                    Ok(input) => input,
                    Err(DecodeError::NotState) => return Ok(false),
                    Err(error) => return Err(error),
                };
                if self.transport == Transport::Unknown {
                    self.transport = input.form.transport();
                    if ps.started {
                        ps.request_calibration(self.transport);
                    }
                }
                self.state = input.state;
                if let Some(imu) = input.imu {
                    let sample = ps.tracker.sample(&imu);
                    self.state.motion = Some(sample);
                    push(&mut self.motion, sample);
                }
                Ok(true)
            }
            Protocol::SwitchPro(pro) => {
                if let Some(setup) = &mut pro.setup {
                    setup.receive(report);
                    if setup.is_done() {
                        pro.calibration = *setup.calibration();
                        let setup = pro.setup.take().expect("set-up running");
                        pro.writer = setup.into_writer();
                    }
                }
                let Some(input) = switch_pro::decode(report, &pro.calibration) else {
                    return Ok(false);
                };
                self.state = input.state;
                let samples = pro.tracker.samples(&pro.calibration, &input);
                self.state.motion = samples.last().copied();
                for sample in samples {
                    push(&mut self.motion, sample);
                }
                Ok(true)
            }
        }
    }

    /// Takes a feature report the driver asked to read (report ID first).
    /// Reports it did not ask for are ignored.
    pub fn feature(&mut self, report: &[u8]) {
        let Protocol::PlayStation(ps) = &mut self.protocol else {
            return;
        };
        let Some((&report_id, data)) = report.split_first() else {
            return;
        };
        if ps.reading == Some(report_id) {
            ps.reading = None;
            let calibration = Calibration::parse(ps.model, self.transport, data);
            ps.tracker.set_calibration(calibration);
        }
    }

    /// Gives up on the feature report the driver asked to read: the
    /// nominal calibration stays.
    pub fn feature_failed(&mut self) {
        if let Protocol::PlayStation(ps) = &mut self.protocol {
            ps.reading = None;
        }
    }

    /// Records a command; `false` for outputs this pad does not have. The
    /// report goes out with the next [`HidDriver::transmit`], which merges
    /// the commands recorded since the last one.
    pub fn send(&mut self, command: OutputCommand) -> bool {
        match &mut self.protocol {
            Protocol::PlayStation(ps) => {
                let taken = ps.output.set(command);
                ps.dirty |= taken;
                taken
            }
            Protocol::SwitchPro(pro) => {
                match command {
                    OutputCommand::Rumble(rumble) => pro.rumble = Some(rumble),
                    OutputCommand::PlayerNumber(player) => pro.player = Some(player),
                    _ => return false,
                }
                true
            }
        }
    }

    /// The next thing to send, at `now_us` (any monotonic clock in
    /// microseconds); `None` when there is nothing. Call it until it
    /// returns `None`, after every [`HidDriver::receive`] and
    /// [`HidDriver::send`], and regularly while the set-up runs.
    pub fn transmit(&mut self, now_us: u64) -> Option<Request> {
        match &mut self.protocol {
            Protocol::PlayStation(ps) => {
                if let (Some(id), false) = (ps.reading, ps.asked) {
                    ps.asked = true;
                    return Some(Request::ReadFeature(id));
                }
                if ps.dirty && self.transport != Transport::Unknown {
                    ps.dirty = false;
                    let report = ps.output.encode(self.transport, ps.sequence);
                    ps.sequence = (ps.sequence + 1) & 0x0f;
                    return Some(Request::Output(report));
                }
                None
            }
            Protocol::SwitchPro(pro) => {
                if let Some(setup) = &mut pro.setup {
                    let progress = setup.progress();
                    let due = match pro.sent {
                        Some((sent, at)) => {
                            sent != progress || now_us.saturating_sub(at) >= RESEND_US
                        }
                        None => true,
                    };
                    if !due {
                        return None;
                    }
                    pro.sent = Some((progress, now_us));
                    return setup.message().map(Request::Output);
                }
                // Without `start`, the pad belongs to whoever set it up.
                pro.sent?;
                if let Some(player) = pro.player.take() {
                    return Some(Request::Output(pro.writer.player_lights(player)));
                }
                if let Some(rumble) = pro.rumble.take() {
                    return Some(Request::Output(pro.writer.rumble(rumble)));
                }
                None
            }
        }
    }

    /// The pad's state from the newest report that carried it.
    pub fn state(&self) -> &GamepadState {
        &self.state
    }

    /// Moves the motion samples received since the last call to `out`,
    /// oldest first.
    pub fn take_motion(&mut self, out: &mut Vec<MotionSample>) {
        out.extend(self.motion.drain(..));
    }

    /// Fills `raw`, laid out as [`HidDriver::layout`], with the newest
    /// state; its motion samples are the ones received since the last
    /// call.
    pub fn fill(&mut self, raw: &mut RawState) {
        let layout_buttons = match &self.protocol {
            Protocol::PlayStation(ps) if ps.model == Model::DualShock4 => {
                &PLAYSTATION_BUTTONS[..PLAYSTATION_BUTTONS.len() - 1]
            }
            Protocol::PlayStation(_) => &PLAYSTATION_BUTTONS[..],
            Protocol::SwitchPro(_) => &SWITCH_PRO_BUTTONS[..],
        };
        for (value, &button) in raw.buttons.iter_mut().zip(layout_buttons) {
            *value = if self.state.pressed(button) { 1.0 } else { 0.0 };
        }
        for (value, axis) in raw.axes.iter_mut().zip(Axis::ALL) {
            *value = self.state.axis(axis);
        }
        raw.touches = self.state.touches;
        raw.power = self.state.power;
        raw.motion.clear();
        self.take_motion(&mut raw.motion);
    }
}

impl PlayStation {
    fn request_calibration(&mut self, transport: Transport) {
        if transport != Transport::Unknown {
            self.reading = Some(Calibration::report_id(self.model, transport));
            self.asked = false;
        }
    }
}

fn push(queue: &mut VecDeque<MotionSample>, sample: MotionSample) {
    if queue.len() == MAX_QUEUED_SAMPLES {
        queue.pop_front();
    }
    queue.push_back(sample);
}

/// Nintendo Switch Pro Controller.
const SWITCH_PRO_IDS: (u16, u16) = (0x057e, 0x2009);

/// The buttons of PlayStation pads; the DualShock 4 lacks the last (mute).
const PLAYSTATION_BUTTONS: [Button; 18] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftStick,
    Button::RightStick,
    Button::Select,
    Button::Start,
    Button::Home,
    Button::TouchpadClick,
];

const SWITCH_PRO_BUTTONS: [Button; 18] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftStick,
    Button::RightStick,
    Button::Select,
    Button::Start,
    Button::Home,
    Button::Capture,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_pads_have_no_driver() {
        assert!(HidDriver::new(0x045e, 0x028e, Transport::Usb).is_none());
        // The Switch Pro Controller's set-up depends on the transport.
        assert!(HidDriver::new(0x057e, 0x2009, Transport::Unknown).is_none());
        assert!(HidDriver::new(0x057e, 0x2009, Transport::Bluetooth).is_some());
    }

    #[test]
    fn nothing_is_sent_before_start() {
        let mut driver = HidDriver::new(0x054c, 0x0ce6, Transport::Bluetooth).unwrap();
        assert_eq!(driver.transmit(0), None);
        driver.start();
        assert_eq!(driver.transmit(0), Some(Request::ReadFeature(0x05)));
        assert_eq!(driver.transmit(0), None, "asked once");
        assert!(!driver.is_ready());
        driver.feature_failed();
        assert!(driver.is_ready());
    }

    #[test]
    fn playstation_outputs_wait_for_the_transport() {
        let mut driver = HidDriver::new(0x054c, 0x05c4, Transport::Unknown).unwrap();
        driver.start();
        assert!(!driver.send(OutputCommand::PlayerNumber(1)));
        assert!(driver.send(OutputCommand::Rumble(Rumble {
            low_frequency: 1.0,
            high_frequency: 0.0,
        })));
        assert_eq!(driver.transmit(0), None);
        // A USB report (63 bytes after the ID) tells the transport.
        let mut report = vec![0u8; 64];
        report[0] = 0x01;
        report[5] = 0x08;
        assert_eq!(driver.receive(&report), Ok(true));
        assert_eq!(driver.transport(), Transport::Usb);
        assert_eq!(driver.transmit(0), Some(Request::ReadFeature(0x02)));
        let Some(Request::Output(output)) = driver.transmit(0) else {
            panic!("no output report");
        };
        assert_eq!(output[..2], [0x05, 0x01]);
        assert_eq!(driver.transmit(0), None, "commands are sent once");
    }

    #[test]
    fn layouts_match_the_decoded_controls() {
        let ds4 = HidDriver::new(0x054c, 0x09cc, Transport::Usb).unwrap();
        let layout = ds4.layout();
        assert_eq!(layout.buttons.len(), 17);
        assert_eq!(layout.axes.len(), 6);
        assert_eq!(layout.touchpads.len(), 1);
        let pro = HidDriver::new(0x057e, 0x2009, Transport::Usb).unwrap();
        let layout = pro.layout();
        assert!(layout.buttons.contains(&RawSource::Button(Button::Capture)));
        assert_eq!(layout.axes.len(), 4);
        assert!(layout.touchpads.is_empty());
    }

    #[test]
    fn switch_pro_resends_unanswered_messages() {
        let mut driver = HidDriver::new(0x057e, 0x2009, Transport::Usb).unwrap();
        driver.start();
        let first = driver.transmit(0);
        assert_eq!(first, Some(Request::Output(vec![0x80, 0x02])));
        assert_eq!(driver.transmit(RESEND_US - 1), None);
        assert_eq!(driver.transmit(RESEND_US), first);
        driver.receive(&[0x81, 0x02]).unwrap();
        assert_eq!(
            driver.transmit(RESEND_US + 1),
            Some(Request::Output(vec![0x80, 0x03]))
        );
    }
}
