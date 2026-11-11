//! [`HidPad`]: a game controller read through a native raw HID API,
//! sans-IO. IOKit HID on macOS uses it (C6); Windows HID (C5) will.
//!
//! A pad the library has a protocol driver for (DualShock 4, DualSense,
//! Switch Pro Controller) goes through its [`HidDriver`]; any other pad is
//! read through its report descriptor ([`anygamepad_hid::RawReader`]), with
//! its buttons, axes and hats named by HID usage for device profiles to
//! map. The backend moves the bytes: it feeds every input report to
//! [`HidPad::receive`], and sends what [`HidPad::transmit`] asks for.
//!
//! Reports start with their report ID when the device uses report IDs, as
//! hidraw, IOKit and Windows HID give them.

use anygamepad_hid::{Descriptor, HidField, RawReader};
use anygamepad_model::raw::{RawLayout, RawState};
use anygamepad_model::{OutputCommand, OutputSet, Transport};

use crate::protocol::driver::{HidDriver, Request};

/// Extended usages of the top-level collections of game controllers:
/// Generic Desktop Joystick, Game Pad and Multi-axis Controller.
const GAME_CONTROLLERS: [u32; 3] = [0x0001_0004, 0x0001_0005, 0x0001_0008];

/// A game controller read through raw HID (see the
/// [module documentation](self)).
#[derive(Clone, Debug)]
pub enum HidPad {
    /// A pad the library drives with a protocol driver.
    Driven(HidDriver),
    /// Any other HID game controller, read through its report descriptor.
    Generic(GenericPad),
}

/// A HID game controller read through its report descriptor.
#[derive(Clone, Debug)]
pub struct GenericPad {
    reader: RawReader,
    /// The latest value of every element: a report updates only the
    /// elements it carries.
    latest: RawState,
}

impl GenericPad {
    /// A pad read through this report descriptor, if it has a game
    /// controller collection (Joystick, Game Pad or Multi-axis Controller)
    /// with at least one button, axis or hat.
    pub fn new(descriptor: &[u8]) -> Option<Self> {
        let descriptor = Descriptor::parse(descriptor).ok()?;
        let game_controller = |field: &HidField| GAME_CONTROLLERS.contains(&field.application);
        if !descriptor.fields.iter().any(game_controller) {
            return None;
        }
        let reader = RawReader::new(&descriptor);
        let layout = reader.layout();
        if layout.buttons.is_empty() && layout.axes.is_empty() && layout.hats.is_empty() {
            return None;
        }
        let latest = RawState::for_layout(layout);
        Some(Self { reader, latest })
    }

    /// The elements the pad reports.
    pub fn layout(&self) -> &RawLayout {
        self.reader.layout()
    }
}

impl HidPad {
    /// The pad with these USB IDs on this transport: driven when the
    /// library has a driver for it, otherwise generic when its report
    /// descriptor describes a game controller. `None` otherwise.
    pub fn new(
        vendor_id: u16,
        product_id: u16,
        transport: Transport,
        descriptor: Option<&[u8]>,
    ) -> Option<Self> {
        if let Some(driver) = HidDriver::new(vendor_id, product_id, transport) {
            return Some(Self::Driven(driver));
        }
        descriptor.and_then(GenericPad::new).map(Self::Generic)
    }

    /// Whether a protocol driver drives the pad.
    pub fn is_driven(&self) -> bool {
        matches!(self, Self::Driven(_))
    }

    /// The elements [`HidPad::fill`] writes.
    pub fn layout(&self) -> RawLayout {
        match self {
            Self::Driven(driver) => driver.layout(),
            Self::Generic(pad) => pad.layout().clone(),
        }
    }

    /// The outputs [`HidPad::send`] drives: none for a generic pad.
    pub fn outputs(&self) -> OutputSet {
        match self {
            Self::Driven(driver) => driver.outputs(),
            Self::Generic(_) => OutputSet::default(),
        }
    }

    /// Starts the driver's set-up ([`HidDriver::start`]); nothing for a
    /// generic pad.
    pub fn start(&mut self) {
        if let Self::Driven(driver) = self {
            driver.start();
        }
    }

    /// Takes an input report. Reports that do not decode are skipped: a
    /// corrupted Bluetooth report is followed by the next one.
    pub fn receive(&mut self, report: &[u8]) {
        match self {
            Self::Driven(driver) => {
                let _ = driver.receive(report);
            }
            Self::Generic(pad) => {
                pad.reader.read(report, &mut pad.latest);
            }
        }
    }

    /// What the backend must send or read next, until `None`.
    pub fn transmit(&mut self, now_us: u64) -> Option<Request> {
        match self {
            Self::Driven(driver) => driver.transmit(now_us),
            Self::Generic(_) => None,
        }
    }

    /// The answer to a [`Request::ReadFeature`], report ID first.
    pub fn feature(&mut self, report: &[u8]) {
        if let Self::Driven(driver) = self {
            driver.feature(report);
        }
    }

    /// A [`Request::ReadFeature`] that failed.
    pub fn feature_failed(&mut self) {
        if let Self::Driven(driver) = self {
            driver.feature_failed();
        }
    }

    /// Records an output command; `false` when the pad has no such output.
    pub fn send(&mut self, command: OutputCommand) -> bool {
        match self {
            Self::Driven(driver) => driver.send(command),
            Self::Generic(_) => false,
        }
    }

    /// Writes the latest values into `state`, which must fit the layout,
    /// and the motion samples received since the previous call.
    pub fn fill(&mut self, state: &mut RawState) {
        match self {
            Self::Driven(driver) => driver.fill(state),
            Self::Generic(pad) => {
                state.buttons.copy_from_slice(&pad.latest.buttons);
                state.axes.copy_from_slice(&pad.latest.axes);
                state.hats.copy_from_slice(&pad.latest.hats);
                state.motion.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::raw::{Hat, RawSource};
    use anygamepad_model::{Output, Rumble};

    use super::*;

    /// A game pad: report 1 with two buttons and a hat, report 2 with an
    /// 8-bit X axis.
    fn descriptor() -> Vec<u8> {
        vec![
            0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, // Generic Desktop, Game Pad, Application
            0x85, 0x01, // Report ID 1
            0x05, 0x09, 0x19, 0x01, 0x29, 0x02, // Button 1 to 2
            0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02, // 2 x 1 bit
            0x75, 0x02, 0x95, 0x01, 0x81, 0x01, // 2 bits of padding
            0x05, 0x01, 0x09, 0x39, 0x25, 0x07, 0x75, 0x04, 0x81, 0x42, // Hat, null state
            0x85, 0x02, // Report ID 2
            0x09, 0x30, 0x26, 0xff, 0x00, 0x75, 0x08, 0x81, 0x02, // X, 0 to 255
            0xc0,
        ]
    }

    #[test]
    fn generic_pad_keeps_the_latest_value_of_every_report() {
        let mut pad = HidPad::new(0x1234, 0x5678, Transport::Usb, Some(&descriptor())).unwrap();
        assert!(!pad.is_driven());
        let layout = pad.layout();
        assert_eq!(
            layout.buttons,
            [
                RawSource::Hid {
                    page: 0x09,
                    usage: 1
                },
                RawSource::Hid {
                    page: 0x09,
                    usage: 2
                },
            ]
        );
        assert_eq!(layout.axes.len(), 1);
        assert_eq!(layout.hats.len(), 1);
        assert!(pad.outputs().is_empty());

        let mut state = RawState::for_layout(&layout);
        // Button 2 and the hat pointing right (2).
        pad.receive(&[0x01, 0b0010_0010]);
        pad.receive(&[0x02, 200]);
        pad.fill(&mut state);
        assert_eq!(state.buttons, [0.0, 1.0]);
        assert_eq!(state.axes, [200.0]);
        assert_eq!(state.hats, [Hat(Hat::RIGHT)]);

        // Report 1 again: the axis keeps its value.
        pad.receive(&[0x01, 0b1111_0001]);
        pad.fill(&mut state);
        assert_eq!(state.buttons, [1.0, 0.0]);
        assert_eq!(state.axes, [200.0]);
        assert_eq!(state.hats, [Hat::CENTERED]);

        assert_eq!(pad.transmit(0), None);
        assert!(!pad.send(OutputCommand::Rumble(Rumble::default())));
    }

    #[test]
    fn known_pads_get_their_driver() {
        let mut pad = HidPad::new(0x054c, 0x0ce6, Transport::Unknown, None).unwrap();
        assert!(pad.is_driven());
        assert!(pad.outputs().contains(Output::Lightbar));
        // The driver asks nothing before it is started.
        assert_eq!(pad.transmit(0), None);
        pad.start();
    }

    #[test]
    fn devices_that_are_not_pads_are_refused() {
        // A keyboard: Generic Desktop, Keyboard.
        let keyboard = [
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00,
            0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0xc0,
        ];
        assert!(HidPad::new(0x1234, 0x0001, Transport::Usb, Some(&keyboard)).is_none());
        assert!(HidPad::new(0x1234, 0x0001, Transport::Usb, Some(&[0xff])).is_none());
        assert!(HidPad::new(0x1234, 0x0001, Transport::Usb, None).is_none());
    }
}
