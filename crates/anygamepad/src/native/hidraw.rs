//! [`HidrawProvider`]: the pads anygamepad drives itself through Linux raw
//! HID (`/dev/hidraw*`), with the protocol drivers (C4, C7).
//!
//! # Which devices
//!
//! The provider takes the pads the library has a [`HidDriver`] for
//! (DualShock 4, DualSense, DualSense Edge, Switch Pro Controller) when no
//! kernel protocol driver drives them: they are bound to `hid-generic`
//! (kernels without `hid-playstation` or `hid-nintendo`, or the driver
//! unbound). Pads a kernel driver drives are the evdev provider's
//! ([`LinuxProvider`](super::linux::LinuxProvider)), which reads them
//! through the driver's input nodes; [`HidrawProvider::set_kernel_driven`]
//! takes them as well, for apps that arbitrate between both (C12).
//!
//! # Discovery
//!
//! As the evdev provider: `/sys/class/hidraw` listed at most once per
//! [`RESCAN_INTERVAL`](super::linux::RESCAN_INTERVAL), a device reported
//! once two scans in a row see it (the first scan reports what is already
//! plugged in) and it can be opened. Nodes that cannot be opened are
//! counted in [`Provider::inaccessible`]: udev gives no access to hidraw
//! nodes by default (see `docs/linux-permissions.md`).
//!
//! # Access
//!
//! [`Access::Shared`] starts the driver ([`HidDriver::start`]): it reads a
//! PlayStation pad's calibration, which switches a Bluetooth pad to its
//! full report, and runs the Switch Pro Controller's set-up.
//! [`Access::ObserveOnly`] only reads reports. Raw HID cannot hide a
//! device from other readers, so [`Access::Exclusive`] is unsupported.
//!
//! # Privacy
//!
//! The provider reads `DRIVER`, `HID_ID` and `HID_NAME` from the HID
//! device's `uevent` and its report descriptor; never `HID_UNIQ` or
//! `HID_PHYS` (serial numbers and Bluetooth addresses), nor the
//! PlayStation pads' pairing and factory reports.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anygamepad_model::identity::{Backend, DescriptorHash, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::raw::RawState;
use anygamepad_model::{OutputCommand, Rumble};

use super::evdev::transport;
use super::linux::RESCAN_INTERVAL;
use crate::protocol::driver::{HidDriver, Request};
use crate::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};

/// Large enough for any report of the supported pads (78 bytes at most).
const REPORT_BUFFER: usize = 128;

/// Kernel drivers that take no pad over: the provider drives pads bound to
/// them by default.
const GENERIC_DRIVERS: [&str; 1] = ["hid-generic"];

nix::ioctl_readwrite_buf!(
    /// `HIDIOCGFEATURE`: reads a feature report into the buffer, whose
    /// first byte is the report ID.
    hidiocgfeature,
    b'H',
    0x07,
    u8
);

/// What a HID device's `uevent` tells.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Uevent {
    driver: Option<String>,
    bus: u16,
    vendor_id: u16,
    product_id: u16,
    name: Option<String>,
}

/// Parses the keys the provider uses from a `uevent` file; every other
/// line (`HID_UNIQ`, `HID_PHYS`, ...) is skipped unread.
fn parse_uevent(text: &str) -> Option<Uevent> {
    let mut uevent = Uevent::default();
    let mut ids = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "DRIVER" => uevent.driver = Some(value.to_owned()),
            "HID_NAME" => uevent.name = Some(value.to_owned()).filter(|name| !name.is_empty()),
            // `BBBB:VVVVVVVV:PPPPPPPP`, hexadecimal.
            "HID_ID" => {
                let mut parts = value.split(':').map(|part| u32::from_str_radix(part, 16));
                let (Some(Ok(bus)), Some(Ok(vendor)), Some(Ok(product))) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return None;
                };
                uevent.bus = u16::try_from(bus).ok()?;
                uevent.vendor_id = u16::try_from(vendor).ok()?;
                uevent.product_id = u16::try_from(product).ok()?;
                ids = true;
            }
            _ => {}
        }
    }
    ids.then_some(uevent)
}

/// A hidraw node of a pad the library drives, as sysfs shows it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Node {
    /// `/dev/hidrawN`.
    dev: PathBuf,
    /// The HID device's sysfs path, shared with its evdev nodes.
    container: String,
}

struct Open {
    access: Access,
    file: File,
}

struct Entry {
    node: Node,
    info: DeviceInfo,
    driver: HidDriver,
    open: Option<Open>,
    disconnected: bool,
}

/// The Linux raw HID provider (see the [module documentation](self)).
pub struct HidrawProvider {
    sysfs: PathBuf,
    kernel_driven: bool,
    entries: BTreeMap<DeviceId, Entry>,
    next_id: u64,
    last_scan: Option<Instant>,
    pending: Vec<Node>,
    inaccessible: usize,
    epoch: Instant,
}

impl Default for HidrawProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl HidrawProvider {
    /// A provider reading `/sys/class/hidraw`.
    pub fn new() -> Self {
        Self::with_sysfs("/sys/class/hidraw")
    }

    /// A provider reading another directory laid out like
    /// `/sys/class/hidraw` (tests, containers).
    pub fn with_sysfs(path: impl Into<PathBuf>) -> Self {
        Self {
            sysfs: path.into(),
            kernel_driven: false,
            entries: BTreeMap::new(),
            next_id: 1,
            last_scan: None,
            pending: Vec::new(),
            inaccessible: 0,
            epoch: Instant::now(),
        }
    }

    /// Also takes pads a kernel protocol driver drives (see the
    /// [module documentation](self)). Takes effect at the next scan.
    pub fn set_kernel_driven(&mut self, take: bool) {
        self.kernel_driven = take;
    }

    /// Lists the hidraw nodes of pads the provider takes.
    fn scan(&self) -> Vec<Node> {
        let Ok(dir) = fs::read_dir(&self.sysfs) else {
            return Vec::new();
        };
        let mut nodes: Vec<Node> = dir
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                if !name.starts_with("hidraw") {
                    return None;
                }
                let device = entry.path().join("device");
                let uevent = parse_uevent(&fs::read_to_string(device.join("uevent")).ok()?)?;
                if !self.takes(&uevent) {
                    return None;
                }
                let container = fs::canonicalize(&device).ok()?.to_str()?.to_owned();
                Some(Node {
                    dev: Path::new("/dev").join(name),
                    container,
                })
            })
            .collect();
        nodes.sort();
        nodes
    }

    /// Whether the provider takes a device: one with a driver, not driven
    /// by a kernel protocol driver unless asked.
    fn takes(&self, uevent: &Uevent) -> bool {
        let generic = uevent
            .driver
            .as_deref()
            .is_none_or(|driver| GENERIC_DRIVERS.contains(&driver));
        (generic || self.kernel_driven)
            && HidDriver::new(uevent.vendor_id, uevent.product_id, transport(uevent.bus)).is_some()
    }

    /// Checks that the node opens and builds its entry.
    fn probe(node: Node) -> io::Result<Entry> {
        drop(open_node(&node.dev)?);
        let container = Path::new(&node.container);
        let uevent = fs::read_to_string(container.join("uevent"))
            .ok()
            .and_then(|text| parse_uevent(&text))
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let transport = transport(uevent.bus);
        let driver = HidDriver::new(uevent.vendor_id, uevent.product_id, transport)
            .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))?;
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Hidraw, Os::Linux);
        identity.transport = transport;
        identity.vendor_id = Some(uevent.vendor_id);
        identity.product_id = Some(uevent.product_id);
        identity.product_name = uevent.name;
        identity.driver = uevent.driver;
        identity.descriptor_hash = fs::read(container.join("report_descriptor"))
            .ok()
            .filter(|descriptor| !descriptor.is_empty())
            .map(|descriptor| DescriptorHash::of(&descriptor));
        let info = DeviceInfo {
            identity,
            location: DeviceLocation::new(
                node.dev.to_str().map(str::to_owned),
                Some(node.container.clone()),
            ),
            layout: driver.layout(),
            outputs: driver.outputs(),
        };
        Ok(Entry {
            node,
            info,
            driver,
            open: None,
            disconnected: false,
        })
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX)
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

/// Opens a hidraw node for non-blocking reads and writes.
fn open_node(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(nix::fcntl::OFlag::O_NONBLOCK.bits())
        .open(path)
}

/// A provider error for an I/O error.
fn error(error: &io::Error) -> ProviderError {
    match error.raw_os_error() {
        // ENODEV, EIO: the device is gone (unplugged, or out of range).
        Some(19 | 5) => ProviderError::Disconnected,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied,
            io::ErrorKind::NotFound => ProviderError::Disconnected,
            io::ErrorKind::ResourceBusy => ProviderError::Busy,
            kind => ProviderError::Backend(kind.to_string()),
        },
    }
}

/// Reads feature report `report_id` (the reply starts with the ID).
#[allow(unsafe_code)]
fn read_feature(file: &File, report_id: u8) -> io::Result<Vec<u8>> {
    let mut buffer = [0u8; REPORT_BUFFER];
    buffer[0] = report_id;
    // SAFETY: the descriptor is an open hidraw node, and HIDIOCGFEATURE
    // writes at most `buffer.len()` bytes, the length the macro encodes
    // in the request from the slice.
    let len = unsafe { hidiocgfeature(file.as_raw_fd(), &mut buffer) }.map_err(io::Error::from)?;
    let len = usize::try_from(len).unwrap_or(0).min(buffer.len());
    Ok(buffer[..len].to_vec())
}

/// Sends what the driver asks for until it has nothing more.
fn pump(driver: &mut HidDriver, file: &mut File, now_us: u64) -> Result<(), ProviderError> {
    while let Some(request) = driver.transmit(now_us) {
        match request {
            Request::Output(report) => {
                file.write_all(&report).map_err(|e| error(&e))?;
            }
            Request::ReadFeature(id) => match read_feature(file, id) {
                Ok(reply) => driver.feature(&reply),
                Err(e) => {
                    let failure = error(&e);
                    if failure == ProviderError::Disconnected {
                        return Err(failure);
                    }
                    driver.feature_failed();
                }
            },
        }
    }
    Ok(())
}

impl Provider for HidrawProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    /// Nodes of pads the provider takes that could not be opened.
    fn inaccessible(&self) -> usize {
        self.inaccessible
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        for (&id, entry) in &self.entries {
            if entry.disconnected {
                events.push(ProviderEvent::Removed(id));
            }
        }
        self.entries.retain(|_, entry| !entry.disconnected);

        let now = Instant::now();
        let first = self.last_scan.is_none();
        if self
            .last_scan
            .is_some_and(|last| now.duration_since(last) < RESCAN_INTERVAL)
        {
            return Ok(());
        }
        self.last_scan = Some(now);
        let nodes = self.scan();

        let gone: Vec<DeviceId> = self
            .entries
            .iter()
            .filter(|(_, entry)| !nodes.contains(&entry.node))
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            self.entries.remove(&id);
            events.push(ProviderEvent::Removed(id));
        }

        let known: Vec<&Node> = self.entries.values().map(|entry| &entry.node).collect();
        let new: Vec<Node> = nodes
            .iter()
            .filter(|node| !known.contains(node))
            .cloned()
            .collect();
        let mut inaccessible = 0;
        let mut waiting = Vec::new();
        for node in new {
            if !first && !self.pending.contains(&node) {
                waiting.push(node);
                continue;
            }
            match Self::probe(node.clone()) {
                Ok(entry) => {
                    let id = DeviceId(self.next_id);
                    self.next_id += 1;
                    self.entries.insert(id, entry);
                    events.push(ProviderEvent::Added(id));
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::PermissionDenied {
                        inaccessible += 1;
                    }
                    waiting.push(node);
                }
            }
        }
        self.pending = waiting;
        self.inaccessible = inaccessible;
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        self.entries
            .get(&device)
            .filter(|entry| !entry.disconnected)
            .map(|entry| &entry.info)
    }

    fn open(&mut self, device: DeviceId, access: Access) -> Result<(), ProviderError> {
        if access == Access::Exclusive {
            return Err(ProviderError::Unsupported);
        }
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        if entry.open.is_none() {
            let file = open_node(&entry.node.dev).map_err(|e| error(&e))?;
            entry.open = Some(Open { access, file });
        }
        let open = entry.open.as_mut().expect("opened above");
        open.access = access;
        if access == Access::Shared {
            entry.driver.start();
            let result = pump(&mut entry.driver, &mut open.file, now_us);
            if result == Err(ProviderError::Disconnected) {
                entry.disconnected = true;
                entry.open = None;
            }
            result?;
        }
        Ok(())
    }

    fn close(&mut self, device: DeviceId) {
        let now_us = self.now_us();
        if let Some(entry) = self.entries.get_mut(&device) {
            if let Some(mut open) = entry.open.take() {
                if open.access != Access::ObserveOnly
                    && entry.driver.send(OutputCommand::Rumble(Rumble::default()))
                {
                    let _ = pump(&mut entry.driver, &mut open.file, now_us);
                }
            }
        }
    }

    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
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
        let result = (|| {
            let mut buffer = [0u8; REPORT_BUFFER];
            loop {
                match open.file.read(&mut buffer) {
                    Ok(0) => return Err(ProviderError::Disconnected),
                    // Reports that do not decode are skipped: a corrupted
                    // Bluetooth report is followed by the next one.
                    Ok(len) => {
                        let _ = entry.driver.receive(&buffer[..len]);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(error(&e)),
                }
            }
            if open.access != Access::ObserveOnly {
                pump(&mut entry.driver, &mut open.file, now_us)?;
            }
            entry.driver.fill(state);
            Ok(())
        })();
        if result == Err(ProviderError::Disconnected) {
            entry.disconnected = true;
            entry.open = None;
        }
        result
    }

    fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        let open = entry.open.as_mut().ok_or(ProviderError::NotOpen)?;
        if open.access == Access::ObserveOnly || !entry.driver.send(command) {
            return Err(ProviderError::Unsupported);
        }
        pump(&mut entry.driver, &mut open.file, now_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUALSENSE: &str = "DRIVER=hid-generic\n\
        HID_ID=0005:0000054C:00000CE6\n\
        HID_NAME=Wireless Controller\n\
        HID_PHYS=00:00:00:00:00:00\n\
        HID_UNIQ=00:00:00:00:00:00\n\
        MODALIAS=hid:b0005g0000v0000054Cp00000CE6\n";

    #[test]
    fn uevent_keys() {
        let uevent = parse_uevent(DUALSENSE).unwrap();
        assert_eq!(
            uevent,
            Uevent {
                driver: Some("hid-generic".into()),
                bus: 0x05,
                vendor_id: 0x054c,
                product_id: 0x0ce6,
                name: Some("Wireless Controller".into()),
            }
        );
        assert_eq!(
            transport(uevent.bus),
            anygamepad_model::Transport::Bluetooth
        );
        assert_eq!(parse_uevent("DRIVER=hid-generic\n"), None);
        assert_eq!(parse_uevent("HID_ID=0003:zz:0\n"), None);
    }

    /// A fake `/sys/class/hidraw` with a DualSense on `hid-generic`, a
    /// Switch Pro Controller on `nintendo` and a keyboard.
    fn fake_sysfs(root: &Path) {
        let devices = root.join("devices");
        let class = root.join("class");
        let node = |n: u32, name: &str, uevent: &str| {
            let device = devices.join(name);
            fs::create_dir_all(&device).unwrap();
            fs::write(device.join("uevent"), uevent).unwrap();
            let hidraw = class.join(format!("hidraw{n}"));
            fs::create_dir_all(&hidraw).unwrap();
            std::os::unix::fs::symlink(&device, hidraw.join("device")).unwrap();
        };
        node(0, "0005:054C:0CE6.0001", DUALSENSE);
        node(
            1,
            "0003:057E:2009.0002",
            "DRIVER=nintendo\nHID_ID=0003:0000057E:00002009\nHID_NAME=Pro Controller\n",
        );
        node(
            2,
            "0003:046D:C31C.0003",
            "DRIVER=hid-generic\nHID_ID=0003:0000046D:0000C31C\nHID_NAME=Keyboard\n",
        );
    }

    #[test]
    fn scan_takes_generic_pads() {
        let dir = std::env::temp_dir().join(format!("anygamepad-hidraw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fake_sysfs(&dir);
        let mut provider = HidrawProvider::with_sysfs(dir.join("class"));
        let nodes = provider.scan();
        assert_eq!(nodes.len(), 1, "{nodes:?}");
        assert_eq!(nodes[0].dev, Path::new("/dev/hidraw0"));
        assert!(nodes[0].container.ends_with("0005:054C:0CE6.0001"));

        provider.set_kernel_driven(true);
        let nodes = provider.scan();
        assert_eq!(nodes.len(), 2, "{nodes:?}");

        // The nodes do not exist: nothing is added.
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        assert!(events.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn exclusive_access_is_unsupported() {
        let mut provider = HidrawProvider::with_sysfs("/nonexistent/anygamepad");
        assert_eq!(
            provider.open(DeviceId(1), Access::Exclusive),
            Err(ProviderError::Unsupported)
        );
        assert_eq!(
            provider.open(DeviceId(1), Access::Shared),
            Err(ProviderError::UnknownDevice)
        );
    }

    #[test]
    fn io_errors() {
        assert_eq!(
            error(&io::Error::from_raw_os_error(5)),
            ProviderError::Disconnected
        );
        assert_eq!(
            error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            ProviderError::PermissionDenied
        );
    }
}
