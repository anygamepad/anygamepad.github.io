//! WebHID access (Chromium only): the HID devices the user grants the page,
//! their report structure and their raw input reports.
//!
//! This is the raw data the desktop collector gets, without installing
//! anything (W3), and the transport for the protocol drivers (C7), which
//! consume and produce reports. WebHID has no counterpart in Firefox and
//! Safari: [`WebHid::new`] fails there with [`ProviderError::Unsupported`].
//!
//! WebHID is asynchronous: granting, opening and feature reports return
//! JavaScript promises, and input reports arrive as events, which are queued
//! until [`WebHid::take_reports`] collects them.
//!
//! Input reports are not decoded into raw elements: WebHID describes the
//! report items of nested collections separately, without their bit
//! positions, so decoding needs the device's protocol (C7) or profile.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use anygamepad_model::hid::{
    HidCollection, HidItemFlag, HidItemFlags, HidReportInfo, HidReportItem, HidUnitSystem,
    HidUsageRange,
};
use anygamepad_model::identity::{Backend, Browser, DeviceIdentity, Os, ProviderKind};
use js_sys::{Array, DataView, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise, spawn_local};

use super::detect;
use crate::provider::{DeviceId, ProviderError, ProviderEvent};

// WebHID is only in web-sys behind `web_sys_unstable_apis`.
#[wasm_bindgen]
extern "C" {
    /// `navigator.hid`.
    #[wasm_bindgen(extends = web_sys::EventTarget)]
    type Hid;

    #[wasm_bindgen(method, catch, js_name = requestDevice)]
    fn request_device(this: &Hid, options: &Object) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = getDevices)]
    fn get_devices(this: &Hid) -> Result<Promise, JsValue>;

    /// `HIDDevice`. Equality is identity (`===`): a reconnected device is a
    /// new object.
    #[wasm_bindgen(extends = web_sys::EventTarget)]
    #[derive(Clone, PartialEq)]
    type HidDevice;

    #[wasm_bindgen(method, getter)]
    fn opened(this: &HidDevice) -> bool;

    #[wasm_bindgen(method, getter, js_name = vendorId)]
    fn vendor_id(this: &HidDevice) -> u16;

    #[wasm_bindgen(method, getter, js_name = productId)]
    fn product_id(this: &HidDevice) -> u16;

    #[wasm_bindgen(method, getter, js_name = productName)]
    fn product_name(this: &HidDevice) -> String;

    #[wasm_bindgen(method, getter)]
    fn collections(this: &HidDevice) -> JsValue;

    #[wasm_bindgen(method, catch)]
    fn open(this: &HidDevice) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn close(this: &HidDevice) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = sendReport)]
    fn send_report(this: &HidDevice, report_id: u8, data: &Uint8Array) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = sendFeatureReport)]
    fn send_feature_report(
        this: &HidDevice,
        report_id: u8,
        data: &Uint8Array,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = receiveFeatureReport)]
    fn receive_feature_report(this: &HidDevice, report_id: u8) -> Result<Promise, JsValue>;

    /// `HIDInputReportEvent`.
    #[wasm_bindgen(extends = web_sys::Event)]
    type HidInputReportEvent;

    #[wasm_bindgen(method, getter, js_name = reportId)]
    fn report_id(this: &HidInputReportEvent) -> u8;

    #[wasm_bindgen(method, getter)]
    fn data(this: &HidInputReportEvent) -> DataView;

    /// `HIDConnectionEvent`.
    #[wasm_bindgen(extends = web_sys::Event)]
    type HidConnectionEvent;

    #[wasm_bindgen(method, getter)]
    fn device(this: &HidConnectionEvent) -> HidDevice;
}

/// Input reports kept per device until [`WebHid::take_reports`] collects
/// them; older reports are dropped beyond this (about a second of a 1 kHz
/// device).
pub const MAX_QUEUED_REPORTS: usize = 1024;

/// Which devices the permission chooser offers (`HIDDeviceFilter`). A device
/// matches when every field that is set matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HidFilter {
    /// Vendor ID.
    pub vendor_id: Option<u16>,
    /// Product ID (needs `vendor_id`).
    pub product_id: Option<u16>,
    /// Usage page of a top-level collection.
    pub usage_page: Option<u16>,
    /// Usage of a top-level collection (needs `usage_page`).
    pub usage: Option<u16>,
}

impl HidFilter {
    /// Generic Desktop game pads, joysticks and multi-axis controllers.
    pub const GAME_CONTROLLERS: [Self; 3] = [
        Self::usage(0x01, 0x05),
        Self::usage(0x01, 0x04),
        Self::usage(0x01, 0x08),
    ];

    /// Devices with a top-level collection of this usage.
    pub const fn usage(usage_page: u16, usage: u16) -> Self {
        Self {
            vendor_id: None,
            product_id: None,
            usage_page: Some(usage_page),
            usage: Some(usage),
        }
    }

    fn to_js(self) -> Object {
        let filter = Object::new();
        let fields = [
            ("vendorId", self.vendor_id),
            ("productId", self.product_id),
            ("usagePage", self.usage_page),
            ("usage", self.usage),
        ];
        for (key, value) in fields {
            if let Some(value) = value {
                let _ = Reflect::set(&filter, &key.into(), &value.into());
            }
        }
        filter
    }
}

/// One input report, as the device sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct HidInputReport {
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Report data, without the report ID.
    pub data: Vec<u8>,
    /// When the browser received it, in milliseconds on the
    /// `performance.now()` clock.
    pub timestamp: f64,
}

/// What WebHID tells about a device.
#[derive(Clone, Debug, PartialEq)]
pub struct HidDeviceInfo {
    /// Identity: VID, PID and product name, browser and OS. WebHID does not
    /// tell the transport.
    pub identity: DeviceIdentity,
    /// Top-level collections.
    pub collections: Vec<HidCollection>,
}

struct Entry {
    id: DeviceId,
    device: HidDevice,
    info: HidDeviceInfo,
    reports: VecDeque<HidInputReport>,
    dropped: u64,
    /// `inputreport` listener, while open.
    listener: Option<Closure<dyn FnMut(HidInputReportEvent)>>,
}

impl Entry {
    fn stop_listening(&mut self) {
        if let Some(listener) = self.listener.take() {
            let _ = self.device.remove_event_listener_with_callback(
                "inputreport",
                listener.as_ref().unchecked_ref(),
            );
        }
    }
}

/// State shared with event handlers and promise callbacks.
struct Shared {
    browser: Browser,
    os: Os,
    next_id: u64,
    devices: Vec<Entry>,
    events: Vec<ProviderEvent>,
}

impl Shared {
    fn entry(&mut self, id: DeviceId) -> Option<&mut Entry> {
        self.devices.iter_mut().find(|entry| entry.id == id)
    }

    /// Adds a granted device, unless it is known already.
    fn add(&mut self, device: HidDevice) {
        if self.devices.iter().any(|entry| entry.device == device) {
            return;
        }
        let id = DeviceId(self.next_id);
        self.next_id += 1;
        let product_name = device.product_name();
        let identity = DeviceIdentity {
            vendor_id: Some(device.vendor_id()),
            product_id: Some(device.product_id()),
            product_name: (!product_name.is_empty()).then_some(product_name),
            browser: Some(self.browser),
            ..DeviceIdentity::new(ProviderKind::Web, Backend::WebHid, self.os)
        };
        let collections = parse_collections(&device.collections());
        self.devices.push(Entry {
            id,
            device,
            info: HidDeviceInfo {
                identity,
                collections,
            },
            reports: VecDeque::new(),
            dropped: 0,
            listener: None,
        });
        self.events.push(ProviderEvent::Added(id));
    }

    fn remove(&mut self, device: &HidDevice) {
        if let Some(index) = self
            .devices
            .iter()
            .position(|entry| entry.device == *device)
        {
            let mut entry = self.devices.remove(index);
            entry.stop_listening();
            self.events.push(ProviderEvent::Removed(entry.id));
        }
    }
}

/// The HID devices the user granted to the page.
///
/// Devices granted earlier are added at start; [`WebHid::request`] asks for
/// more. Call [`WebHid::update`] once per frame to learn which devices came
/// and went.
pub struct WebHid {
    hid: Hid,
    shared: Rc<RefCell<Shared>>,
    on_connect: Closure<dyn FnMut(HidConnectionEvent)>,
    on_disconnect: Closure<dyn FnMut(HidConnectionEvent)>,
}

impl WebHid {
    /// Whether the browser has WebHID.
    pub fn is_supported() -> bool {
        hid().is_some()
    }

    /// Starts tracking the granted HID devices. Fails with
    /// [`ProviderError::Unsupported`] where the browser has no WebHID.
    pub fn new() -> Result<Self, ProviderError> {
        let hid = hid().ok_or(ProviderError::Unsupported)?;
        let navigator = web_sys::window()
            .ok_or(ProviderError::Unsupported)?
            .navigator();
        let user_agent = navigator.user_agent().unwrap_or_default();
        let touch_points = u32::try_from(navigator.max_touch_points()).unwrap_or(0);
        let (browser, os) = detect(&user_agent, touch_points);
        let shared = Rc::new(RefCell::new(Shared {
            browser,
            os,
            next_id: 1,
            devices: Vec::new(),
            events: Vec::new(),
        }));

        let weak = Rc::downgrade(&shared);
        let on_connect =
            Closure::<dyn FnMut(HidConnectionEvent)>::new(move |event: HidConnectionEvent| {
                if let Some(shared) = weak.upgrade() {
                    shared.borrow_mut().add(event.device());
                }
            });
        let weak = Rc::downgrade(&shared);
        let on_disconnect =
            Closure::<dyn FnMut(HidConnectionEvent)>::new(move |event: HidConnectionEvent| {
                if let Some(shared) = weak.upgrade() {
                    shared.borrow_mut().remove(&event.device());
                }
            });
        let listen = |kind: &str, handler: &Closure<dyn FnMut(HidConnectionEvent)>| {
            hid.add_event_listener_with_callback(kind, handler.as_ref().unchecked_ref())
                .map_err(|_| ProviderError::Backend("cannot listen to WebHID events".into()))
        };
        listen("connect", &on_connect)?;
        listen("disconnect", &on_disconnect)?;

        if let Ok(granted) = hid.get_devices() {
            let weak = Rc::downgrade(&shared);
            spawn_local(async move {
                if let (Ok(devices), Some(shared)) = (JsFuture::from(granted).await, weak.upgrade())
                {
                    let mut shared = shared.borrow_mut();
                    for device in Array::from(&devices).iter() {
                        shared.add(device.unchecked_into());
                    }
                }
            });
        }

        Ok(Self {
            hid,
            shared,
            on_connect,
            on_disconnect,
        })
    }

    /// Opens the browser's chooser for devices matching any of `filters`
    /// (all devices when empty). Must be called while handling a user
    /// gesture (a click). The promise resolves with the number of devices
    /// the user picked, which [`WebHid::update`] then reports as added.
    pub fn request(&self, filters: &[HidFilter]) -> Promise {
        let options = Object::new();
        let filters: Array = filters.iter().map(|filter| filter.to_js()).collect();
        let _ = Reflect::set(&options, &"filters".into(), &filters);
        let picked = match self.hid.request_device(&options) {
            Ok(picked) => picked,
            Err(error) => return Promise::reject(&error),
        };
        let weak = Rc::downgrade(&self.shared);
        future_to_promise(async move {
            let devices = Array::from(&JsFuture::from(picked).await?);
            if let Some(shared) = weak.upgrade() {
                let mut shared = shared.borrow_mut();
                for device in devices.iter() {
                    shared.add(device.unchecked_into());
                }
            }
            Ok(devices.length().into())
        })
    }

    /// Appends the devices that were added and removed since the last call
    /// to `events`.
    pub fn update(&mut self, events: &mut Vec<ProviderEvent>) {
        events.append(&mut self.shared.borrow_mut().events);
    }

    /// The granted devices that are connected.
    pub fn devices(&self) -> Vec<DeviceId> {
        let shared = self.shared.borrow();
        shared.devices.iter().map(|entry| entry.id).collect()
    }

    /// What WebHID tells about a device.
    pub fn info(&self, id: DeviceId) -> Option<HidDeviceInfo> {
        let mut shared = self.shared.borrow_mut();
        shared.entry(id).map(|entry| entry.info.clone())
    }

    /// Opens a device and starts queueing its input reports. Opening does not
    /// send anything to the device.
    pub fn open(&self, id: DeviceId) -> Promise {
        let Some(device) = self.device(id) else {
            return reject(ProviderError::UnknownDevice);
        };
        let weak = Rc::downgrade(&self.shared);
        future_to_promise(async move {
            if !device.opened() {
                JsFuture::from(device.open()?).await?;
            }
            if let Some(shared) = weak.upgrade() {
                listen_to_reports(&shared, id);
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Stops queueing a device's input reports and closes it.
    pub fn close(&self, id: DeviceId) -> Promise {
        let mut shared = self.shared.borrow_mut();
        let Some(entry) = shared.entry(id) else {
            return reject(ProviderError::UnknownDevice);
        };
        entry.stop_listening();
        entry.reports.clear();
        entry
            .device
            .close()
            .unwrap_or_else(|error| Promise::reject(&error))
    }

    /// Moves the queued input reports of a device, oldest first, to the end
    /// of `reports`.
    pub fn take_reports(
        &self,
        id: DeviceId,
        reports: &mut Vec<HidInputReport>,
    ) -> Result<(), ProviderError> {
        let mut shared = self.shared.borrow_mut();
        let entry = shared.entry(id).ok_or(ProviderError::UnknownDevice)?;
        reports.extend(entry.reports.drain(..));
        Ok(())
    }

    /// Input reports of a device dropped so far because nobody collected
    /// them (see [`MAX_QUEUED_REPORTS`]).
    pub fn dropped_reports(&self, id: DeviceId) -> Option<u64> {
        self.shared
            .borrow_mut()
            .entry(id)
            .map(|entry| entry.dropped)
    }

    /// Reads a feature report. The promise resolves with a `DataView` of its
    /// data, starting with the report ID when the device uses report IDs.
    pub fn receive_feature_report(&self, id: DeviceId, report_id: u8) -> Promise {
        match self.device(id) {
            Some(device) => settle(device.receive_feature_report(report_id)),
            None => reject(ProviderError::UnknownDevice),
        }
    }

    /// Sends an output report (rumble, LEDs, protocol commands).
    pub fn send_report(&self, id: DeviceId, report_id: u8, data: &[u8]) -> Promise {
        match self.device(id) {
            Some(device) => settle(device.send_report(report_id, &Uint8Array::from(data))),
            None => reject(ProviderError::UnknownDevice),
        }
    }

    /// Sends a feature report.
    pub fn send_feature_report(&self, id: DeviceId, report_id: u8, data: &[u8]) -> Promise {
        match self.device(id) {
            Some(device) => settle(device.send_feature_report(report_id, &Uint8Array::from(data))),
            None => reject(ProviderError::UnknownDevice),
        }
    }

    fn device(&self, id: DeviceId) -> Option<HidDevice> {
        let mut shared = self.shared.borrow_mut();
        shared.entry(id).map(|entry| entry.device.clone())
    }
}

impl Drop for WebHid {
    fn drop(&mut self) {
        for (kind, handler) in [
            ("connect", &self.on_connect),
            ("disconnect", &self.on_disconnect),
        ] {
            let _ = self
                .hid
                .remove_event_listener_with_callback(kind, handler.as_ref().unchecked_ref());
        }
        for entry in &mut self.shared.borrow_mut().devices {
            if entry.listener.is_some() {
                entry.stop_listening();
                let _ = entry.device.close();
            }
        }
    }
}

/// `navigator.hid`, if the browser has WebHID.
fn hid() -> Option<Hid> {
    let navigator = web_sys::window()?.navigator();
    let hid = Reflect::get(&navigator, &"hid".into()).ok()?;
    (!hid.is_undefined() && !hid.is_null()).then(|| hid.unchecked_into())
}

/// Queues the input reports of an open device.
fn listen_to_reports(shared: &Rc<RefCell<Shared>>, id: DeviceId) {
    let weak: Weak<RefCell<Shared>> = Rc::downgrade(shared);
    let mut state = shared.borrow_mut();
    let Some(entry) = state.entry(id) else { return };
    if entry.listener.is_some() {
        return;
    }
    let listener =
        Closure::<dyn FnMut(HidInputReportEvent)>::new(move |event: HidInputReportEvent| {
            let Some(shared) = weak.upgrade() else { return };
            let Ok(mut shared) = shared.try_borrow_mut() else {
                return;
            };
            let Some(entry) = shared.entry(id) else {
                return;
            };
            let data = event.data();
            let bytes = Uint8Array::new_with_byte_offset_and_length(
                &data.buffer(),
                data.byte_offset() as u32,
                data.byte_length() as u32,
            );
            if entry.reports.len() >= MAX_QUEUED_REPORTS {
                entry.reports.pop_front();
                entry.dropped += 1;
            }
            entry.reports.push_back(HidInputReport {
                report_id: event.report_id(),
                data: bytes.to_vec(),
                timestamp: event.time_stamp(),
            });
        });
    if entry
        .device
        .add_event_listener_with_callback("inputreport", listener.as_ref().unchecked_ref())
        .is_ok()
    {
        entry.listener = Some(listener);
    }
}

/// The promise of a WebHID call, or a rejected one if the call threw.
fn settle(promise: Result<Promise, JsValue>) -> Promise {
    promise.unwrap_or_else(|error| Promise::reject(&error))
}

fn reject(error: ProviderError) -> Promise {
    Promise::reject(&js_sys::Error::new(&error.to_string()))
}

/// Collections nest; deeper ones are ignored (real descriptors are shallow).
const MAX_COLLECTION_DEPTH: usize = 32;

/// Converts WebHID's `HIDDevice.collections` (an array of
/// `HIDCollectionInfo`) into [`HidCollection`]s. Missing members read as 0,
/// `false` or empty.
pub fn parse_collections(collections: &JsValue) -> Vec<HidCollection> {
    parse_collection_list(collections, 0)
}

fn parse_collection_list(list: &JsValue, depth: usize) -> Vec<HidCollection> {
    if depth >= MAX_COLLECTION_DEPTH {
        return Vec::new();
    }
    array(list)
        .iter()
        .map(|collection| HidCollection {
            usage_page: number(&collection, "usagePage") as u16,
            usage: number(&collection, "usage") as u16,
            kind: number(&collection, "type") as u8,
            children: parse_collection_list(&member(&collection, "children"), depth + 1),
            input_reports: parse_reports(&member(&collection, "inputReports")),
            output_reports: parse_reports(&member(&collection, "outputReports")),
            feature_reports: parse_reports(&member(&collection, "featureReports")),
        })
        .collect()
}

fn parse_reports(list: &JsValue) -> Vec<HidReportInfo> {
    array(list)
        .iter()
        .map(|report| HidReportInfo {
            report_id: number(&report, "reportId") as u8,
            items: array(&member(&report, "items"))
                .iter()
                .map(|item| parse_item(&item))
                .collect(),
        })
        .collect()
}

fn parse_item(item: &JsValue) -> HidReportItem {
    let flags = [
        (HidItemFlag::Constant, flag(item, "isConstant")),
        (HidItemFlag::Variable, !flag(item, "isArray")),
        (HidItemFlag::Relative, !flag(item, "isAbsolute")),
        (HidItemFlag::Wrap, flag(item, "wrap")),
        (HidItemFlag::NonLinear, !flag(item, "isLinear")),
        (HidItemFlag::NoPreferred, !flag(item, "hasPreferredState")),
        (HidItemFlag::NullState, flag(item, "hasNull")),
        (HidItemFlag::Volatile, flag(item, "isVolatile")),
        (HidItemFlag::BufferedBytes, flag(item, "isBufferedBytes")),
    ];
    let unit_system = member(item, "unitSystem")
        .as_string()
        .and_then(|name| name.replace('-', "_").parse().ok())
        .unwrap_or(HidUnitSystem::None);
    HidReportItem {
        usages: array(&member(item, "usages"))
            .iter()
            .map(|usage| usage.as_f64().unwrap_or(0.0) as u32)
            .collect(),
        usage_range: flag(item, "isRange").then(|| HidUsageRange {
            minimum: number(item, "usageMinimum") as u32,
            maximum: number(item, "usageMaximum") as u32,
        }),
        report_size: number(item, "reportSize") as u16,
        report_count: number(item, "reportCount") as u16,
        logical_minimum: number(item, "logicalMinimum") as i32,
        logical_maximum: number(item, "logicalMaximum") as i32,
        physical_minimum: number(item, "physicalMinimum") as i32,
        physical_maximum: number(item, "physicalMaximum") as i32,
        unit_system,
        unit_exponent: number(item, "unitExponent") as i8,
        flags: flags
            .into_iter()
            .filter(|&(_, set)| set)
            .map(|(flag, _)| flag)
            .collect::<HidItemFlags>(),
    }
}

fn member(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

/// A numeric member; float to integer casts saturate, and NaN becomes 0.
fn number(object: &JsValue, key: &str) -> f64 {
    member(object, key).as_f64().unwrap_or(0.0)
}

fn flag(object: &JsValue, key: &str) -> bool {
    member(object, key).as_bool().unwrap_or(false)
}

fn array(value: &JsValue) -> Array {
    if Array::is_array(value) {
        value.clone().unchecked_into()
    } else {
        Array::new()
    }
}
