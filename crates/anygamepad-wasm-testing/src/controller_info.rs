//! `ControllerReader`: reads what a WebHID PlayStation pad tells about
//! itself (firmware, board, colour, usage counters, battery voltage), with
//! the library's sans-IO [`InfoReader`]. The page moves the bytes, with the
//! tester module's `HidDevices.sendFeatureReport()` and
//! `receiveFeatureReport()`. It lives in this module, which the tester
//! loads when asked, to keep the tester's own module in its size budget.
//!
//! No identifier of the unit comes out: see
//! `anygamepad::protocol::playstation::info`.

use anygamepad::model::Transport;
use anygamepad::protocol::playstation::Model;
use anygamepad::protocol::playstation::info::{ControllerInfo, InfoReader, Step};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::to_js;

/// What the page does next for a `ControllerReader`.
#[derive(Clone, Debug, Serialize, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReaderStep {
    /// Send this feature report (`sendFeatureReport`), then call `step()`
    /// again; `failed()` first if it could not be sent.
    Send {
        /// Report ID.
        report_id: u8,
        /// Data, without the report ID.
        #[tsify(type = "number[]")]
        data: Vec<u8>,
    },
    /// Read this feature report and pass it to `answer()`, or call
    /// `failed()`.
    Read {
        /// Report ID.
        report_id: u8,
    },
    /// Call `step()` again in this many milliseconds.
    Wait {
        /// Milliseconds.
        ms: u32,
    },
    /// Nothing left: `finish()` gives the information.
    Done,
}

/// A pad's firmware information.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct FirmwareFacts {
    /// Build date and time (`Jun 24 2024 11:16:21`).
    pub build: String,
    /// Hardware version.
    pub hardware: u32,
    /// Main firmware version.
    pub firmware: u32,
    /// Update version (`0x0520`, shown `5.20`).
    pub update: Option<u16>,
    /// Firmware type.
    pub kind: Option<u16>,
    /// Software series.
    pub series: Option<u16>,
    /// Bootloader firmware version.
    pub sbl: Option<u32>,
    /// DSP firmware version.
    pub dsp: Option<u32>,
    /// MCU DSP firmware version.
    pub mcu_dsp: Option<u32>,
}

/// One usage counter.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct UsageFact {
    /// Stable name: `active_seconds`, `presses.south`,
    /// `stick_cycles.left_stick_x`.
    pub name: String,
    /// Its value; durations in seconds.
    pub value: u32,
}

/// What a pad told about itself. Nothing in it tells one unit from another
/// of the same model.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct ControllerFacts {
    /// `undefined` when the pad answered no firmware information.
    pub firmware: Option<FirmwareFacts>,
    /// Version of the Bluetooth patch.
    pub bluetooth_patch: Option<u32>,
    /// Board name (`BDM-020`).
    pub board: Option<String>,
    /// Colour code (`00`).
    pub colour_code: Option<String>,
    /// Colour name, for the known codes.
    pub colour: Option<String>,
    /// Model of the left and right adaptive trigger motors.
    pub trigger_motors: [Option<String>; 2],
    /// Touchpad controller firmware, in hex.
    pub touchpad_firmware: Option<String>,
    /// Battery voltage, in millivolts.
    pub battery_millivolts: Option<u16>,
    /// Usage counters, in the pad's order.
    pub usage: Vec<UsageFact>,
}

impl From<ControllerInfo> for ControllerFacts {
    fn from(info: ControllerInfo) -> Self {
        let board = info.board_name();
        Self {
            firmware: info.firmware.map(|f| FirmwareFacts {
                build: f.build,
                hardware: f.hardware,
                firmware: f.firmware,
                update: f.update,
                kind: f.kind,
                series: f.series,
                sbl: f.sbl,
                dsp: f.dsp,
                mcu_dsp: f.mcu_dsp,
            }),
            bluetooth_patch: info.bluetooth_patch,
            board,
            colour_code: info.colour.map(|c| c.code().to_owned()),
            colour: info.colour.and_then(|c| c.name()).map(str::to_owned),
            trigger_motors: info.trigger_motors,
            touchpad_firmware: info
                .touchpad_firmware
                .map(|bytes| bytes.iter().map(|b| format!("{b:02X}")).collect()),
            battery_millivolts: info.battery_millivolts,
            usage: info
                .usage
                .into_iter()
                .map(|(stat, value)| UsageFact {
                    name: stat.name(),
                    value,
                })
                .collect(),
        }
    }
}

/// Reads what a PlayStation pad tells about itself; no identifier of the
/// unit comes out (see `anygamepad::protocol::playstation::info`).
#[wasm_bindgen]
pub struct ControllerReader {
    reader: Option<InfoReader>,
}

#[wasm_bindgen]
impl ControllerReader {
    /// A reader for the PlayStation pad with these USB IDs on this
    /// transport (`HidDeviceInfo.transport`); `command_size` is the data
    /// bytes of its feature report `0x80` as its descriptor declares them,
    /// when it does. Throws for other pads. Like every feature report, its
    /// first read switches a Bluetooth pad to its full report until it is
    /// turned off: ask first.
    #[wasm_bindgen(constructor)]
    pub fn new(
        vendor_id: u16,
        product_id: u16,
        #[wasm_bindgen(unchecked_param_type = "Transport")] transport: JsValue,
        command_size: Option<u32>,
    ) -> Result<ControllerReader, JsError> {
        let model = Model::from_ids(vendor_id, product_id)
            .ok_or_else(|| JsError::new("not a PlayStation pad"))?;
        let transport: Transport =
            serde_wasm_bindgen::from_value(transport).unwrap_or(Transport::Unknown);
        let mut reader = InfoReader::new(model, transport);
        if let Some(size) = command_size {
            reader = reader.command_size(size as usize);
        }
        Ok(ControllerReader {
            reader: Some(reader),
        })
    }

    /// What to do next, at `now` (`performance.now()`).
    #[wasm_bindgen(unchecked_return_type = "ReaderStep")]
    pub fn step(&mut self, now: f64) -> Result<JsValue, JsError> {
        let Some(reader) = &mut self.reader else {
            return to_js(&ReaderStep::Done);
        };
        to_js(&convert(reader.step(now.max(0.0) as u64)))
    }

    /// The feature report a `read` step asked for, report ID first.
    /// Returns the next step when the answer decides it.
    #[wasm_bindgen(unchecked_return_type = "ReaderStep | undefined")]
    pub fn answer(&mut self, report: &[u8]) -> Result<JsValue, JsError> {
        match self
            .reader
            .as_mut()
            .and_then(|reader| reader.answer(report))
        {
            Some(step) => to_js(&convert(step)),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The last `send` or `read` failed.
    pub fn failed(&mut self) {
        if let Some(reader) = &mut self.reader {
            reader.failed();
        }
    }

    /// The information read; the reader is spent.
    #[wasm_bindgen(unchecked_return_type = "ControllerFacts")]
    pub fn finish(&mut self) -> Result<JsValue, JsError> {
        let info = self
            .reader
            .take()
            .map(InfoReader::finish)
            .unwrap_or_default();
        to_js(&ControllerFacts::from(info))
    }
}

fn convert(step: Step) -> ReaderStep {
    match step {
        Step::Send(report) => {
            let (&report_id, data) = report.split_first().unwrap_or((&0, &[]));
            ReaderStep::Send {
                report_id,
                data: data.to_vec(),
            }
        }
        Step::Read(report_id) => ReaderStep::Read { report_id },
        Step::Wait(ms) => ReaderStep::Wait { ms },
        Step::Done => ReaderStep::Done,
    }
}
