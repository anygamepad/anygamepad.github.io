//! [`IoHidProvider`]: HID game controllers through IOKit's `IOHIDManager`.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use anygamepad_model::raw::RawState;
use anygamepad_model::{OutputCommand, Rumble};
use objc2_core_foundation::{
    CFArray, CFData, CFDictionary, CFIndex, CFNumber, CFRetained, CFRunLoop, CFRunLoopRunResult,
    CFString,
};
use objc2_io_kit::{
    IOHIDDevice, IOHIDManager, IOHIDReportType, IOReturn, kIOHIDOptionsTypeNone,
    kIOHIDOptionsTypeSeizeDevice,
};

use crate::native::hid::HidPad;
use crate::native::iokit::{self, HidProperties, MATCHING, io_return, keys};
use crate::protocol::driver::Request;
use crate::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};
use anygamepad_model::identity::ProviderKind;

/// The run loop mode the provider's IOKit sources run in, so that pumping
/// them never runs the app's own sources.
const RUN_LOOP_MODE: &str = "org.anygamepad.iokit";

/// How often [`Provider::update`] compares IOKit's device set with the
/// devices it reports.
pub const RESCAN_INTERVAL: Duration = Duration::from_millis(250);

/// Run loop passes a pump makes at most, so a flood of reports cannot keep
/// `update` or `poll` from returning.
const MAX_PUMP_PASSES: usize = 256;

/// Input reports kept per device between two polls; older ones are dropped.
const MAX_QUEUED_REPORTS: usize = 256;

/// Input report buffer when the device does not tell its largest report.
const DEFAULT_REPORT_BUFFER: usize = 64;

/// Largest input report buffer the provider allocates.
const MAX_REPORT_BUFFER: usize = 4096;

/// Feature report buffer: large enough for the supported pads' feature
/// reports (78 bytes at most).
const FEATURE_BUFFER: usize = 128;

/// Where IOKit's callbacks for one open device write.
///
/// Allocated with `Box::into_raw` when the device opens and freed after
/// its callbacks are unregistered; the callbacks run only while the
/// provider pumps its run loop mode, on the provider's thread, when no
/// other reference to the sink exists.
struct Sink {
    /// IOKit writes each input report here before calling back.
    buffer: Box<[u8]>,
    reports: VecDeque<Vec<u8>>,
    removed: bool,
}

struct Open {
    access: Access,
    sink: NonNull<Sink>,
    /// The sink's buffer and its length, as registered with IOKit.
    buffer: NonNull<u8>,
    length: CFIndex,
}

struct Entry {
    device: CFRetained<IOHIDDevice>,
    info: DeviceInfo,
    pad: HidPad,
    report_buffer: usize,
    open: Option<Open>,
    disconnected: bool,
    /// macOS refused to open the device (Input Monitoring, usually).
    denied: bool,
}

/// The macOS IOKit HID provider (see the [module documentation](super)).
///
/// Its IOKit sources are scheduled on the run loop of the thread that
/// creates it, in a private mode that [`Provider::update`] and
/// [`Provider::poll`] run: create it on the thread that drives it. It is
/// not `Send`.
pub struct IoHidProvider {
    manager: CFRetained<IOHIDManager>,
    run_loop: CFRetained<CFRunLoop>,
    mode: CFRetained<CFString>,
    take_game_controller_pads: bool,
    entries: BTreeMap<DeviceId, Entry>,
    /// Devices IOKit matched that the provider does not take.
    ignored: Vec<CFRetained<IOHIDDevice>>,
    next_id: u64,
    last_scan: Option<Instant>,
    epoch: Instant,
}

impl IoHidProvider {
    /// A provider matching the HID game controllers (see
    /// [`MATCHING`](crate::native::iokit::MATCHING)).
    pub fn new() -> Result<Self, ProviderError> {
        let run_loop = CFRunLoop::current()
            .ok_or_else(|| ProviderError::Backend("no run loop on this thread".into()))?;
        let mode = CFString::from_str(RUN_LOOP_MODE);
        let manager = IOHIDManager::new(None, kIOHIDOptionsTypeNone);
        let page_key = CFString::from_static_str(keys::DEVICE_USAGE_PAGE);
        let usage_key = CFString::from_static_str(keys::DEVICE_USAGE);
        let matching: Vec<CFRetained<CFDictionary<CFString, CFNumber>>> = MATCHING
            .iter()
            .map(|&(page, usage)| {
                let page = CFNumber::new_i32(page as i32);
                let usage = CFNumber::new_i32(usage as i32);
                CFDictionary::from_slices(&[&*page_key, &*usage_key], &[&*page, &*usage])
            })
            .collect();
        let matching = CFArray::from_retained_objects(&matching);
        let matching: &CFArray = (*matching).as_ref();
        // SAFETY: the array holds dictionaries of CFString keys and CFNumber
        // values, as `IOHIDManagerSetDeviceMatchingMultiple` expects.
        unsafe { manager.set_device_matching_multiple(Some(matching)) };
        // SAFETY: the run loop is this thread's and outlives the manager's
        // scheduling: `Drop` unschedules it before releasing either.
        unsafe { manager.schedule_with_run_loop(&run_loop, &mode) };
        Ok(Self {
            manager,
            run_loop,
            mode,
            take_game_controller_pads: false,
            entries: BTreeMap::new(),
            ignored: Vec::new(),
            next_id: 1,
            last_scan: None,
            epoch: Instant::now(),
        })
    }

    /// Also takes the pads GameController drives (PlayStation, Xbox,
    /// Switch Pro), read with their protocol drivers when the library has
    /// one; see [`iokit::takes`]. Takes effect at the next scan.
    pub fn set_game_controller_pads(&mut self, take: bool) {
        if take != self.take_game_controller_pads {
            self.take_game_controller_pads = take;
            self.ignored.clear();
        }
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// Runs the provider's run loop mode until no source is ready, which
    /// delivers hotplug notifications and input reports to the callbacks.
    fn pump(&self) {
        for _ in 0..MAX_PUMP_PASSES {
            let result = CFRunLoop::run_in_mode(Some(&self.mode), 0.0, true);
            if result != CFRunLoopRunResult::HandledSource {
                break;
            }
        }
    }

    /// Builds the entry of a newly matched device, or `None` when the
    /// provider does not take it.
    fn probe(&self, device: CFRetained<IOHIDDevice>) -> Option<Entry> {
        let properties = properties(&device);
        if !iokit::takes(
            properties.vendor_id,
            properties.product_id,
            self.take_game_controller_pads,
        ) {
            return None;
        }
        let pad = HidPad::new(
            properties.vendor_id,
            properties.product_id,
            properties.transport(),
            properties.descriptor.as_deref(),
        )?;
        let location = number(&device, keys::LOCATION_ID).map(|id| format!("{id:08x}"));
        let report_buffer = number(&device, keys::MAX_INPUT_REPORT_SIZE)
            .and_then(|size| usize::try_from(size).ok())
            .filter(|&size| size > 0)
            .unwrap_or(DEFAULT_REPORT_BUFFER)
            .min(MAX_REPORT_BUFFER);
        let info = DeviceInfo {
            identity: properties.identity(),
            location: DeviceLocation::new(None, location),
            layout: pad.layout(),
            outputs: pad.outputs(),
        };
        Some(Entry {
            device,
            info,
            pad,
            report_buffer,
            open: None,
            disconnected: false,
            denied: false,
        })
    }

    /// Compares IOKit's device set with the entries.
    fn scan(&mut self, events: &mut Vec<ProviderEvent>) {
        let devices = matched_devices(&self.manager);
        let present = |device: &IOHIDDevice| devices.iter().any(|d| same(d, device));

        let gone: Vec<DeviceId> = self
            .entries
            .iter()
            .filter(|(_, entry)| !present(&entry.device))
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(mut entry) = self.entries.remove(&id) {
                self.release(&mut entry);
            }
            events.push(ProviderEvent::Removed(id));
        }
        self.ignored.retain(|device| present(device));

        for device in devices {
            let known = self
                .entries
                .values()
                .any(|entry| same(&entry.device, &device))
                || self.ignored.iter().any(|ignored| same(ignored, &device));
            if known {
                continue;
            }
            match self.probe(device.clone()) {
                Some(entry) => {
                    let id = DeviceId(self.next_id);
                    self.next_id += 1;
                    self.entries.insert(id, entry);
                    events.push(ProviderEvent::Added(id));
                }
                None => self.ignored.push(device),
            }
        }
    }

    /// Unregisters an open device's callbacks, closes it and frees its
    /// sink.
    fn release(&self, entry: &mut Entry) {
        let Some(open) = entry.open.take() else {
            return;
        };
        // SAFETY: the device was scheduled on this run loop and mode when
        // it was opened.
        unsafe {
            entry
                .device
                .unschedule_from_run_loop(&self.run_loop, &self.mode)
        };
        // SAFETY: unregisters the callbacks (a null callback) with the
        // buffer they were registered with, which is still allocated.
        unsafe {
            entry.device.register_input_report_callback(
                open.buffer,
                open.length,
                None,
                std::ptr::null_mut(),
            );
            entry
                .device
                .register_removal_callback(None, std::ptr::null_mut());
        }
        let _ = entry.device.close(open_options(open.access));
        // SAFETY: allocated with `Box::into_raw` in `open`; the callbacks
        // that could use it are unregistered and the device is closed.
        drop(unsafe { Box::from_raw(open.sink.as_ptr()) });
    }

    fn entry_mut(&mut self, device: DeviceId) -> Result<&mut Entry, ProviderError> {
        let entry = self
            .entries
            .get_mut(&device)
            .ok_or(ProviderError::UnknownDevice)?;
        if entry.disconnected {
            return Err(ProviderError::Disconnected);
        }
        Ok(entry)
    }
}

impl Drop for IoHidProvider {
    fn drop(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        for entry in entries.values_mut() {
            self.release(entry);
        }
        // SAFETY: the manager was scheduled on this run loop and mode in
        // `new`.
        unsafe {
            self.manager
                .unschedule_from_run_loop(&self.run_loop, &self.mode)
        };
    }
}

/// The IOKit options of an access mode.
fn open_options(access: Access) -> u32 {
    if access == Access::Exclusive {
        kIOHIDOptionsTypeSeizeDevice
    } else {
        kIOHIDOptionsTypeNone
    }
}

/// Whether two references are the same IOKit device object.
fn same(a: &IOHIDDevice, b: &IOHIDDevice) -> bool {
    std::ptr::eq(a, b)
}

/// The devices the manager currently matches.
fn matched_devices(manager: &IOHIDManager) -> Vec<CFRetained<IOHIDDevice>> {
    let Some(set) = manager.devices() else {
        return Vec::new();
    };
    let count = usize::try_from(set.count()).unwrap_or(0);
    let mut values: Vec<*const c_void> = vec![std::ptr::null(); count];
    // SAFETY: `values` has room for the set's `count` values.
    unsafe { set.values(values.as_mut_ptr()) };
    values
        .into_iter()
        .filter_map(|value| NonNull::new(value.cast_mut().cast::<IOHIDDevice>()))
        // SAFETY: `IOHIDManagerCopyDevices` returns a set of IOHIDDevice
        // objects, alive while the set is; retaining them keeps them alive
        // after.
        .map(|device| unsafe { CFRetained::retain(device) })
        .collect()
}

/// A number property of a device.
fn number(device: &IOHIDDevice, key: &'static str) -> Option<i64> {
    let value = device.property(&CFString::from_static_str(key))?;
    value.downcast_ref::<CFNumber>()?.as_i64()
}

/// A string property of a device.
fn string(device: &IOHIDDevice, key: &'static str) -> Option<String> {
    let value = device.property(&CFString::from_static_str(key))?;
    value.downcast_ref::<CFString>().map(ToString::to_string)
}

/// The properties the provider reads (and only those: see
/// [`iokit`](crate::native::iokit#privacy)).
fn properties(device: &IOHIDDevice) -> HidProperties {
    let id = |key| {
        number(device, key)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(0)
    };
    HidProperties {
        vendor_id: id(keys::VENDOR_ID),
        product_id: id(keys::PRODUCT_ID),
        version: number(device, keys::VERSION).and_then(|value| u16::try_from(value).ok()),
        product: string(device, keys::PRODUCT),
        manufacturer: string(device, keys::MANUFACTURER),
        transport: string(device, keys::TRANSPORT),
        descriptor: device
            .property(&CFString::from_static_str(keys::REPORT_DESCRIPTOR))
            .and_then(|value| value.downcast_ref::<CFData>().map(CFData::to_vec)),
    }
}

/// IOKit's input report callback: queues the report in the device's sink.
unsafe extern "C-unwind" fn input_report(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _kind: IOHIDReportType,
    _report_id: u32,
    report: NonNull<u8>,
    length: CFIndex,
) {
    if context.is_null() || result != io_return::SUCCESS {
        return;
    }
    // SAFETY: `context` is the sink registered with this callback, alive
    // until the callback is unregistered; callbacks run on the provider's
    // thread while it pumps, when nothing else borrows the sink.
    let sink = unsafe { &mut *context.cast::<Sink>() };
    let length = usize::try_from(length).unwrap_or(0).min(sink.buffer.len());
    // SAFETY: IOKit wrote `length` bytes of the report at `report`, inside
    // the sink's buffer (`length` is capped to its size).
    let report = unsafe { std::slice::from_raw_parts(report.as_ptr(), length) };
    if sink.reports.len() == MAX_QUEUED_REPORTS {
        sink.reports.pop_front();
    }
    sink.reports.push_back(report.to_vec());
}

/// IOKit's removal callback: marks the device's sink removed.
unsafe extern "C-unwind" fn removed(context: *mut c_void, _result: IOReturn, _sender: *mut c_void) {
    if context.is_null() {
        return;
    }
    // SAFETY: as in `input_report`.
    let sink = unsafe { &mut *context.cast::<Sink>() };
    sink.removed = true;
}

/// Reads feature report `report_id`; the reply starts with the ID.
fn read_feature(device: &IOHIDDevice, report_id: u8) -> Result<Vec<u8>, ProviderError> {
    let mut buffer = [0u8; FEATURE_BUFFER];
    buffer[0] = report_id;
    // IOKit writes the report ID first for numbered reports; an unnumbered
    // report goes after the 0 the reply starts with.
    let offset = usize::from(report_id == 0);
    let mut length = (FEATURE_BUFFER - offset) as CFIndex;
    // SAFETY: the buffer has `length` bytes from `offset` on, and IOKit
    // writes at most `length` bytes, then the length it wrote.
    let result = unsafe {
        device.report(
            IOHIDReportType::Feature,
            CFIndex::from(report_id),
            NonNull::new_unchecked(buffer.as_mut_ptr().add(offset)),
            NonNull::from(&mut length),
        )
    };
    if result != io_return::SUCCESS {
        return Err(iokit::error(result));
    }
    let length = usize::try_from(length)
        .unwrap_or(0)
        .min(FEATURE_BUFFER - offset)
        + offset;
    Ok(buffer[..length].to_vec())
}

/// Writes an output report (report ID first).
fn write_output(device: &IOHIDDevice, report: &[u8]) -> Result<(), ProviderError> {
    let Some((&report_id, _)) = report.split_first() else {
        return Ok(());
    };
    // An unnumbered report is sent without its leading 0.
    let data = if report_id == 0 { &report[1..] } else { report };
    if data.is_empty() {
        return Ok(());
    }
    // SAFETY: IOKit reads `data.len()` bytes from `data`, which outlives
    // the synchronous call.
    let result = unsafe {
        device.set_report(
            IOHIDReportType::Output,
            CFIndex::from(report_id),
            NonNull::new_unchecked(data.as_ptr().cast_mut()),
            data.len() as CFIndex,
        )
    };
    if result == io_return::SUCCESS {
        Ok(())
    } else {
        Err(iokit::error(result))
    }
}

/// Opens a device, registers its callbacks with a new sink and schedules
/// it on the provider's run loop mode.
fn open_device(
    entry: &mut Entry,
    access: Access,
    run_loop: &CFRunLoop,
    mode: &CFString,
) -> Result<(), ProviderError> {
    let result = entry.device.open(open_options(access));
    if result != io_return::SUCCESS {
        return Err(iokit::error(result));
    }
    let mut buffer = vec![0u8; entry.report_buffer].into_boxed_slice();
    // The slice's heap memory stays where it is when the box moves into
    // the sink.
    let length = buffer.len() as CFIndex;
    let buffer_ptr = NonNull::from(&mut buffer[..]).cast::<u8>();
    let sink = Box::into_raw(Box::new(Sink {
        buffer,
        reports: VecDeque::new(),
        removed: false,
    }));
    // SAFETY: `sink` is a fresh allocation that stays valid until
    // `release` unregisters these callbacks and frees it; IOKit writes
    // reports into its buffer, `length` bytes at most. The run loop is the
    // provider's, which unschedules the device before dropping it.
    unsafe {
        let buffer = buffer_ptr;
        entry.device.register_input_report_callback(
            buffer,
            length,
            Some(input_report),
            sink.cast(),
        );
        entry
            .device
            .register_removal_callback(Some(removed), sink.cast());
        entry.device.schedule_with_run_loop(run_loop, mode);
    }
    entry.open = Some(Open {
        access,
        // SAFETY: `Box::into_raw` never returns null.
        sink: unsafe { NonNull::new_unchecked(sink) },
        buffer: buffer_ptr,
        length,
    });
    Ok(())
}

/// Sends what the pad asks for until it has nothing more.
fn transmit(pad: &mut HidPad, device: &IOHIDDevice, now_us: u64) -> Result<(), ProviderError> {
    while let Some(request) = pad.transmit(now_us) {
        match request {
            Request::Output(report) => write_output(device, &report)?,
            Request::ReadFeature(id) => match read_feature(device, id) {
                Ok(reply) => pad.feature(&reply),
                Err(ProviderError::Disconnected) => return Err(ProviderError::Disconnected),
                Err(_) => pad.feature_failed(),
            },
        }
    }
    Ok(())
}

impl Provider for IoHidProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    /// Devices the provider takes that macOS refused to open.
    fn inaccessible(&self) -> usize {
        self.entries.values().filter(|entry| entry.denied).count()
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        self.pump();
        let mut removed = Vec::new();
        for (&id, entry) in &mut self.entries {
            if let Some(open) = &entry.open {
                // SAFETY: no callback runs outside `pump`, so nothing else
                // borrows the sink.
                if unsafe { open.sink.as_ref() }.removed {
                    entry.disconnected = true;
                }
            }
            if entry.disconnected {
                removed.push(id);
            }
        }
        for id in removed {
            if let Some(mut entry) = self.entries.remove(&id) {
                self.release(&mut entry);
                // IOKit can list a removed device until its own removal
                // notification runs: not a new device in the meantime.
                self.ignored.push(entry.device);
            }
            events.push(ProviderEvent::Removed(id));
        }

        let now = Instant::now();
        if self
            .last_scan
            .is_some_and(|last| now.duration_since(last) < RESCAN_INTERVAL)
        {
            return Ok(());
        }
        self.last_scan = Some(now);
        self.scan(events);
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        self.entries
            .get(&device)
            .filter(|entry| !entry.disconnected)
            .map(|entry| &entry.info)
    }

    fn open(&mut self, device: DeviceId, access: Access) -> Result<(), ProviderError> {
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        let reopen = entry
            .open
            .as_ref()
            .is_some_and(|open| open_options(open.access) != open_options(access));
        if reopen {
            let mut taken = self.entries.remove(&device).expect("checked above");
            self.release(&mut taken);
            self.entries.insert(device, taken);
        }
        let run_loop = self.run_loop.clone();
        let mode = self.mode.clone();
        let entry = self.entry_mut(device)?;
        if entry.open.is_none() {
            let result = open_device(entry, access, &run_loop, &mode);
            entry.denied = result == Err(ProviderError::PermissionDenied);
            result?;
        }
        let open = entry.open.as_mut().expect("opened above");
        open.access = access;
        if access != Access::ObserveOnly {
            entry.pad.start();
            let result = transmit(&mut entry.pad, &entry.device, now_us);
            if result == Err(ProviderError::Disconnected) {
                entry.disconnected = true;
            }
            result?;
        }
        Ok(())
    }

    fn close(&mut self, device: DeviceId) {
        let now_us = self.now_us();
        let Some(mut entry) = self.entries.remove(&device) else {
            return;
        };
        if let Some(open) = &entry.open {
            if open.access != Access::ObserveOnly
                && entry.pad.send(OutputCommand::Rumble(Rumble::default()))
            {
                let _ = transmit(&mut entry.pad, &entry.device, now_us);
            }
        }
        self.release(&mut entry);
        self.entries.insert(device, entry);
    }

    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
        self.pump();
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        if !state.fits(&entry.info.layout) {
            return Err(ProviderError::Backend(
                "state does not fit the layout".into(),
            ));
        }
        let Some(open) = entry.open.as_mut() else {
            return Err(ProviderError::NotOpen);
        };
        // SAFETY: no callback runs outside `pump`, so this is the only
        // reference to the sink.
        let sink = unsafe { open.sink.as_mut() };
        while let Some(report) = sink.reports.pop_front() {
            entry.pad.receive(&report);
        }
        if sink.removed {
            entry.disconnected = true;
            return Err(ProviderError::Disconnected);
        }
        if open.access != Access::ObserveOnly {
            let result = transmit(&mut entry.pad, &entry.device, now_us);
            if result == Err(ProviderError::Disconnected) {
                entry.disconnected = true;
            }
            result?;
        }
        entry.pad.fill(state);
        Ok(())
    }

    fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        let open = entry.open.as_ref().ok_or(ProviderError::NotOpen)?;
        if open.access == Access::ObserveOnly || !entry.pad.send(command) {
            return Err(ProviderError::Unsupported);
        }
        transmit(&mut entry.pad, &entry.device, now_us)
    }
}
