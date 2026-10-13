//! The `HidDevices` class: WebHID access for JavaScript (Chromium only).

use anygamepad::model::hid::HidCollection;
use anygamepad::model::identity::{DeviceIdentity, Fingerprint};
use anygamepad::provider::ProviderEvent;
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

/// Serializes bytes as a `Uint8Array` (serde-wasm-bindgen).
fn bytes<S: Serializer>(data: &&[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(data)
}

/// The HID devices the user granted to the page, through WebHID
/// (Chromium-based browsers only; see `HidDevices.isSupported()`).
///
/// `request()` opens the browser's chooser and must be called from a user
/// gesture (a click). `open()` a device to queue its input reports, and
/// collect them with `reports()`, for example once per frame.
#[wasm_bindgen]
pub struct HidDevices {
    hid: WebHid,
    events: Vec<ProviderEvent>,
    reports: Vec<web_hid::HidInputReport>,
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
        to_js(&HidDeviceInfo {
            fingerprint: info.identity.fingerprint(),
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
        let reports: Vec<HidInputReport<'_>> = self
            .reports
            .iter()
            .map(|report| HidInputReport {
                report_id: report.report_id,
                data: &report.data,
                timestamp: report.timestamp,
            })
            .collect();
        to_js(&reports)
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
