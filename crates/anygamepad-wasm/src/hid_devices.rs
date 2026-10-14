//! The `HidDevices` class: WebHID access for JavaScript (Chromium only).

use std::collections::BTreeMap;

use anygamepad::model::hid::HidCollection;
use anygamepad::model::identity::{DeviceIdentity, Fingerprint};
use anygamepad::model::{FaceLayout, Family, GamepadState, TouchpadInfo, Transport};
use anygamepad::protocol::playstation::{self, Model};
use anygamepad::provider::{DeviceId, ProviderEvent};
use anygamepad::web::hid::{self as web_hid, WebHid};
use js_sys::Promise;
use serde::{Deserialize, Serialize, Serializer};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::gamepads::{DeviceEvent, device_id, error, js_id, to_js};

/// Which devices the chooser offers. A device matches when every field that
/// is set matches.
#[derive(Clone, Copy, Debug, Default, Deserialize, Tsify)]
#[serde(default)]
pub struct HidFilter {
    /// Vendor ID.
    #[tsify(optional)]
    pub vendor_id: Option<u16>,
    /// Product ID (needs `vendor_id`).
    #[tsify(optional)]
    pub product_id: Option<u16>,
    /// Usage page of a top-level collection.
    #[tsify(optional)]
    pub usage_page: Option<u16>,
    /// Usage of a top-level collection (needs `usage_page`).
    #[tsify(optional)]
    pub usage: Option<u16>,
}

impl From<HidFilter> for web_hid::HidFilter {
    fn from(filter: HidFilter) -> Self {
        Self {
            vendor_id: filter.vendor_id,
            product_id: filter.product_id,
            usage_page: filter.usage_page,
            usage: filter.usage,
        }
    }
}

/// What WebHID tells about a device.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct HidDeviceInfo {
    /// Identity: VID, PID, product name, browser and OS.
    pub identity: DeviceIdentity,
    /// Stable hash of the identity.
    pub fingerprint: Fingerprint,
    /// Controller family recognized from the VID/PID; `undefined` when not
    /// recognized.
    pub family: Option<Family>,
    /// What is printed on the face buttons, when the family is known.
    pub face_layout: Option<FaceLayout>,
    /// USB or Bluetooth, for the pads whose report sizes tell (PlayStation);
    /// `unknown` otherwise.
    pub transport: Transport,
    /// Whether `poll()` decodes this device's reports (PlayStation pads).
    pub decoded: bool,
    /// The touchpad of a decoded pad that has one.
    pub touchpad: Option<TouchpadInfo>,
    /// Top-level collections: the structure of the device's reports.
    pub collections: Vec<HidCollection>,
}

/// One input report, as the device sent it.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct HidInputReport<'a> {
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Report data, without the report ID.
    #[serde(serialize_with = "bytes")]
    #[tsify(type = "Uint8Array")]
    pub data: &'a [u8],
    /// When the browser received it, in milliseconds on the
    /// `performance.now()` clock.
    pub timestamp: f64,
}

/// What `poll()` read from a device since the last call.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct HidPoll<'a> {
    /// Input reports received since the last call.
    pub received: u32,
    /// The newest of them; `undefined` when none arrived.
    pub last: Option<HidInputReport<'a>>,
    /// All of them, oldest first, when `poll()` was asked for them (empty
    /// otherwise).
    pub reports: Vec<HidInputReport<'a>>,
    /// For pads the library decodes: their input from the newest report that
    /// decoded, possibly from an earlier call.
    pub input: Option<DecodedInput>,
    /// Why the newest report did not decode, if it did not.
    pub error: Option<String>,
}

/// Which form of input report a PlayStation pad sends.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum ReportForm {
    /// USB: always the full report.
    Usb,
    /// Bluetooth before the switch: no touchpad, motion or battery.
    BluetoothReduced,
    /// Bluetooth after the switch.
    BluetoothFull,
}

/// Motion sensor readings as the pad reports them, before calibration.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
pub struct RawImu {
    /// Gyroscope, in report order.
    pub gyro: [i16; 3],
    /// Accelerometer, in report order.
    pub accel: [i16; 3],
    /// The pad's sensor clock; only differences are meaningful.
    pub timestamp: u32,
}

/// One decoded input report.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
pub struct DecodedInput {
    /// The report's form.
    pub form: ReportForm,
    /// The transport the form implies.
    pub transport: Transport,
    /// Buttons, sticks, triggers, touch points and power (motion stays raw,
    /// in `imu`).
    pub state: GamepadState,
    /// Motion sensors, in full reports.
    pub imu: Option<RawImu>,
}

impl From<playstation::Input> for DecodedInput {
    fn from(input: playstation::Input) -> Self {
        Self {
            form: match input.form {
                playstation::ReportForm::Usb => ReportForm::Usb,
                playstation::ReportForm::BluetoothReduced => ReportForm::BluetoothReduced,
                playstation::ReportForm::BluetoothFull => ReportForm::BluetoothFull,
            },
            transport: input.form.transport(),
            state: input.state,
            imu: input.imu.map(|imu| RawImu {
                gyro: imu.gyro,
                accel: imu.accel,
                timestamp: imu.timestamp,
            }),
        }
    }
}

/// The JavaScript view of a queued report.
fn report_view(report: &web_hid::HidInputReport) -> HidInputReport<'_> {
    HidInputReport {
        report_id: report.report_id,
        data: &report.data,
        timestamp: report.timestamp,
    }
}

/// Serializes bytes as a `Uint8Array` (serde-wasm-bindgen).
fn bytes<S: Serializer>(data: &&[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(data)
}

/// The HID devices the user granted to the page, through WebHID
/// (Chromium-based browsers only; see `HidDevices.isSupported()`).
///
/// `request()` opens the browser's chooser and must be called from a user
/// gesture (a click). `open()` a device to queue its input reports, and
/// collect them with `reports()` or `poll()`, for example once per frame.
#[wasm_bindgen]
pub struct HidDevices {
    hid: WebHid,
    events: Vec<ProviderEvent>,
    reports: Vec<web_hid::HidInputReport>,
    /// The protocol of each device seen by `poll()`, if the library has one.
    models: BTreeMap<DeviceId, Option<Model>>,
    /// The newest decoded input of each device.
    inputs: BTreeMap<DeviceId, playstation::Input>,
}

#[wasm_bindgen]
impl HidDevices {
    /// Whether the browser has WebHID.
    #[wasm_bindgen(js_name = isSupported)]
    pub fn is_supported() -> bool {
        WebHid::is_supported()
    }

    /// Starts tracking the granted devices. Throws where the browser has no
    /// WebHID.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<HidDevices, JsError> {
        Ok(Self {
            hid: WebHid::new().map_err(error)?,
            events: Vec::new(),
            reports: Vec::new(),
            models: BTreeMap::new(),
            inputs: BTreeMap::new(),
        })
    }

    /// Opens the browser's chooser for devices matching any of `filters`
    /// (game pads, joysticks and multi-axis controllers by default; `[]`
    /// for every device). Call it from a user gesture. Resolves with the
    /// number of devices the user picked; `update()` then reports them.
    #[wasm_bindgen(unchecked_return_type = "Promise<number>")]
    pub fn request(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "HidFilter[]")] filters: JsValue,
    ) -> Result<Promise, JsError> {
        let filters: Vec<web_hid::HidFilter> = if filters.is_undefined() {
            web_hid::HidFilter::GAME_CONTROLLERS.to_vec()
        } else {
            serde_wasm_bindgen::from_value::<Vec<HidFilter>>(filters)
                .map_err(|e| JsError::new(&e.to_string()))?
                .into_iter()
                .map(Into::into)
                .collect()
        };
        Ok(self.hid.request(&filters))
    }

    /// Returns which devices were added and removed since the last call.
    #[wasm_bindgen(unchecked_return_type = "DeviceEvent[]")]
    pub fn update(&mut self) -> Result<JsValue, JsError> {
        self.events.clear();
        self.hid.update(&mut self.events);
        for event in &self.events {
            if let ProviderEvent::Removed(id) = event {
                self.models.remove(id);
                self.inputs.remove(id);
            }
        }
        let events: Vec<DeviceEvent> = self.events.iter().map(|&event| event.into()).collect();
        to_js(&events)
    }

    /// Ids of the granted devices that are connected.
    pub fn devices(&self) -> Vec<u32> {
        self.hid.devices().into_iter().map(js_id).collect()
    }

    /// What WebHID tells about a device; `undefined` for an unknown id.
    #[wasm_bindgen(unchecked_return_type = "HidDeviceInfo | undefined")]
    pub fn info(&self, device: u32) -> Result<JsValue, JsError> {
        let Some(info) = self.hid.info(device_id(device)) else {
            return Ok(JsValue::UNDEFINED);
        };
        let family = Family::from_identity(&info.identity);
        let model = family.and_then(Model::from_family);
        to_js(&HidDeviceInfo {
            fingerprint: info.identity.fingerprint(),
            family,
            face_layout: family.map(Family::face_layout),
            transport: model.map_or(Transport::Unknown, |model| {
                playstation::transport_from_collections(model, &info.collections)
            }),
            decoded: model.is_some(),
            touchpad: model.map(Model::touchpad),
            identity: info.identity,
            collections: info.collections,
        })
    }

    /// Opens a device and starts queueing its input reports. Sends nothing
    /// to the device.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn open(&self, device: u32) -> Promise {
        self.hid.open(device_id(device))
    }

    /// Stops queueing a device's input reports and closes it.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn close(&self, device: u32) -> Promise {
        self.hid.close(device_id(device))
    }

    /// The input reports of an open device received since the last call,
    /// oldest first.
    #[wasm_bindgen(unchecked_return_type = "HidInputReport[]")]
    pub fn reports(&mut self, device: u32) -> Result<JsValue, JsError> {
        self.reports.clear();
        self.hid
            .take_reports(device_id(device), &mut self.reports)
            .map_err(error)?;
        let reports: Vec<HidInputReport<'_>> = self.reports.iter().map(report_view).collect();
        to_js(&reports)
    }

    /// Collects an open device's input reports received since the last call
    /// and, for the pads the library knows (PlayStation), decodes them.
    /// Replaces `reports()` for per-frame use; with `all`, the result also
    /// lists every report (to record them).
    #[wasm_bindgen(unchecked_return_type = "HidPoll")]
    pub fn poll(&mut self, device: u32, all: Option<bool>) -> Result<JsValue, JsError> {
        let id = device_id(device);
        self.reports.clear();
        self.hid
            .take_reports(id, &mut self.reports)
            .map_err(error)?;
        let model = *self.models.entry(id).or_insert_with(|| {
            let info = self.hid.info(id)?;
            Family::from_identity(&info.identity).and_then(Model::from_family)
        });
        let mut error = None;
        if let Some(model) = model {
            for report in &self.reports {
                match playstation::decode(model, report.report_id, &report.data) {
                    Ok(input) => {
                        self.inputs.insert(id, input);
                        error = None;
                    }
                    // Audio frames interleave with state; not a failure.
                    Err(playstation::DecodeError::NotState) => {}
                    Err(failure) => error = Some(failure.to_string()),
                }
            }
        }
        to_js(&HidPoll {
            received: u32::try_from(self.reports.len()).unwrap_or(u32::MAX),
            last: self.reports.last().map(report_view),
            reports: if all.unwrap_or(false) {
                self.reports.iter().map(report_view).collect()
            } else {
                Vec::new()
            },
            input: self.inputs.get(&id).map(|&input| input.into()),
            error,
        })
    }

    /// Switches a Bluetooth PlayStation pad to its full report (touchpad,
    /// motion, battery) by reading its calibration feature report. The pad
    /// stays switched for every application on the host until it is
    /// power-cycled, and some of them (DirectInput games, the Gamepad API
    /// in some browsers) then get no input from it: tell the user first.
    /// Reads nothing else.
    #[wasm_bindgen(
        js_name = switchToFullReports,
        unchecked_return_type = "Promise<DataView>"
    )]
    pub fn switch_to_full_reports(&self, device: u32) -> Promise {
        self.hid
            .receive_feature_report(device_id(device), playstation::CALIBRATION_REPORT)
    }

    /// Input reports of a device dropped so far because `reports()` was not
    /// called often enough; `undefined` for an unknown id.
    #[wasm_bindgen(js_name = droppedReports)]
    pub fn dropped_reports(&self, device: u32) -> Option<f64> {
        // Exact below 2^53, far beyond any count.
        self.hid
            .dropped_reports(device_id(device))
            .map(|dropped| dropped as f64)
    }

    /// Reads a feature report. Resolves with its data, starting with the
    /// report ID when the device uses report IDs.
    #[wasm_bindgen(
        js_name = receiveFeatureReport,
        unchecked_return_type = "Promise<DataView>"
    )]
    pub fn receive_feature_report(&self, device: u32, report_id: u8) -> Promise {
        self.hid
            .receive_feature_report(device_id(device), report_id)
    }

    /// Sends an output report.
    #[wasm_bindgen(js_name = sendReport, unchecked_return_type = "Promise<void>")]
    pub fn send_report(&self, device: u32, report_id: u8, data: &[u8]) -> Promise {
        self.hid.send_report(device_id(device), report_id, data)
    }

    /// Sends a feature report.
    #[wasm_bindgen(js_name = sendFeatureReport, unchecked_return_type = "Promise<void>")]
    pub fn send_feature_report(&self, device: u32, report_id: u8, data: &[u8]) -> Promise {
        self.hid
            .send_feature_report(device_id(device), report_id, data)
    }
}
