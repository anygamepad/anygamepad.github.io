//! [`LinuxProvider`]: evdev devices behind the [`Provider`] trait (C4).
//!
//! # Discovery
//!
//! Every `update` at most once per [`RESCAN_INTERVAL`], the provider lists
//! `/sys/class/input/event*` and classifies each node from the
//! capabilities sysfs shows, without opening it: keyboards and mice are
//! never opened. A controller's nodes (gamepad, motion sensors, touchpad)
//! share the sysfs parent device and become one [`DeviceId`]. A controller
//! is reported once it shows up in two scans in a row, so that every node
//! of it exists and udev has set its permissions; the first scan reports
//! what is already plugged in.
//!
//! # Permissions
//!
//! Desktop distributions let the logged-in user read and write gamepad
//! nodes (udev tags `ID_INPUT_JOYSTICK` devices with `uaccess`). Nodes that
//! cannot be opened are counted in [`LinuxProvider::inaccessible`] so an
//! app can explain it; see `docs/linux-permissions.md`.
//!
//! # Privacy
//!
//! The provider reads names, IDs, capabilities, the driver name and the
//! HID report descriptor, never `uniq` or `phys` (serial numbers and
//! Bluetooth addresses). Paths stay in [`DeviceLocation`], which is never
//! serialized or logged.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anygamepad_model::identity::{Backend, DescriptorHash, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::raw::{RawSource, RawState};
use anygamepad_model::{Button, Output, OutputCommand, OutputSet};
use evdev::{Device, FFEffect, FFEffectCode, FFEffectData, FFEffectKind, KeyCode};

use super::evdev::{
    AbsInfo, GamepadNode, MotionNode, NodeKind, TouchpadNode, classify, parse_bitmap, transport,
};
use crate::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};

/// How often `update` lists the input devices again.
pub const RESCAN_INTERVAL: Duration = Duration::from_millis(500);

/// Bits per word of sysfs capability bitmaps (the kernel's `long`; a
/// 32-bit program on a 64-bit kernel is not supported).
const WORD_BITS: u32 = usize::BITS;

/// One input node, as sysfs shows it.
#[derive(Clone, Debug)]
struct Node {
    /// `/dev/input/eventN`.
    dev: PathBuf,
    kind: NodeKind,
    /// The sysfs parent device that groups a controller's nodes.
    container: String,
}

/// A controller's nodes, as found by one scan.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Group {
    container: String,
    gamepad: PathBuf,
    motion: Option<PathBuf>,
    touchpad: Option<PathBuf>,
}

/// An open node and what it feeds.
struct OpenNodes {
    access: Access,
    gamepad: Device,
    motion: Option<(Device, MotionNode)>,
    touchpad: Option<(Device, TouchpadNode)>,
    rumble: Option<FFEffect>,
}

struct Entry {
    group: Group,
    info: DeviceInfo,
    node: GamepadNode,
    /// The motion and touchpad decoders as found when the device appeared.
    motion: Option<MotionNode>,
    touchpad: Option<TouchpadNode>,
    open: Option<OpenNodes>,
    disconnected: bool,
}

/// The Linux native provider (see the [module documentation](self)).
pub struct LinuxProvider {
    sysfs: PathBuf,
    entries: BTreeMap<DeviceId, Entry>,
    next_id: u64,
    last_scan: Option<Instant>,
    /// Controllers seen by the previous scan and not reported yet.
    pending: Vec<Group>,
    inaccessible: usize,
}

impl Default for LinuxProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxProvider {
    /// A provider reading `/sys/class/input`.
    pub fn new() -> Self {
        Self::with_sysfs("/sys/class/input")
    }

    /// A provider reading another directory laid out like
    /// `/sys/class/input` (tests, containers).
    pub fn with_sysfs(path: impl Into<PathBuf>) -> Self {
        Self {
            sysfs: path.into(),
            entries: BTreeMap::new(),
            next_id: 1,
            last_scan: None,
            pending: Vec::new(),
            inaccessible: 0,
        }
    }

    /// Gamepad nodes found by the last scan that could not be opened
    /// (missing permissions, usually).
    pub fn inaccessible(&self) -> usize {
        self.inaccessible
    }

    /// Lists the input nodes sysfs shows and groups them by controller.
    fn scan(&self) -> Vec<Group> {
        let Ok(dir) = fs::read_dir(&self.sysfs) else {
            return Vec::new();
        };
        let mut nodes: Vec<Node> = dir
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                name.starts_with("event")
                    .then(|| read_node(&entry.path(), &name))
                    .flatten()
            })
            .collect();
        nodes.sort_by(|a, b| a.dev.cmp(&b.dev));
        let mut groups: Vec<Group> = Vec::new();
        for node in nodes.iter().filter(|n| n.kind == NodeKind::Gamepad) {
            // Two gamepad nodes on one parent stay two devices.
            let sibling = |kind| {
                nodes
                    .iter()
                    .find(|n| n.kind == kind && n.container == node.container)
                    .map(|n| n.dev.clone())
            };
            groups.push(Group {
                container: node.container.clone(),
                gamepad: node.dev.clone(),
                motion: sibling(NodeKind::Motion),
                touchpad: sibling(NodeKind::Touchpad),
            });
        }
        groups.sort();
        groups
    }

    /// Opens a controller's nodes once to read their ranges, and builds its
    /// entry.
    fn probe(group: Group) -> io::Result<Entry> {
        let gamepad = Device::open(&group.gamepad)?;
        let keys: Vec<u16> = gamepad
            .supported_keys()
            .map(|keys| keys.iter().map(|key| key.0).collect())
            .unwrap_or_default();
        let abs = abs_infos(&gamepad)?;
        let node = GamepadNode::new(&keys, &abs);

        let motion = group
            .motion
            .as_ref()
            .and_then(|path| Device::open(path).ok())
            .and_then(|device| abs_infos(&device).ok())
            .and_then(|abs| MotionNode::new(&abs));
        let touchpad = group
            .touchpad
            .as_ref()
            .and_then(|path| Device::open(path).ok())
            .and_then(|device| abs_infos(&device).ok())
            .and_then(|abs| TouchpadNode::new(&abs));

        // Sibling nodes add to the gamepad's layout; the touchpad click goes
        // last.
        let mut layout = node.layout().clone();
        if let Some(motion) = &motion {
            layout.sensors = motion.sensors();
        }
        if let Some(touchpad) = &touchpad {
            layout.touchpads.push(touchpad.info());
            layout
                .buttons
                .push(RawSource::Button(Button::TouchpadClick));
        }

        let id = gamepad.input_id();
        let container = Path::new(&group.container);
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux);
        identity.transport = transport(id.bus_type().0);
        identity.vendor_id = Some(id.vendor());
        identity.product_id = Some(id.product());
        identity.version = Some(id.version());
        identity.product_name = gamepad.name().map(str::to_owned);
        identity.driver = fs::read_link(container.join("driver"))
            .ok()
            .and_then(|link| link.file_name()?.to_str().map(str::to_owned));
        identity.descriptor_hash = fs::read(container.join("report_descriptor"))
            .ok()
            .filter(|descriptor| !descriptor.is_empty())
            .map(|descriptor| DescriptorHash::of(&descriptor));

        let rumble = gamepad
            .supported_ff()
            .is_some_and(|ff| ff.contains(FFEffectCode::FF_RUMBLE));
        let outputs = if rumble {
            OutputSet::from([Output::Rumble])
        } else {
            OutputSet::EMPTY
        };
        let info = DeviceInfo {
            identity,
            location: DeviceLocation::new(
                group.gamepad.to_str().map(str::to_owned),
                Some(group.container.clone()),
            ),
            layout,
            outputs,
        };
        Ok(Entry {
            group,
            info,
            node,
            motion,
            touchpad,
            open: None,
            disconnected: false,
        })
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

/// Reads one `/sys/class/input/eventN` directory.
fn read_node(path: &Path, name: &str) -> Option<Node> {
    let input = path.join("device");
    let bitmap = |file: &str| {
        fs::read_to_string(input.join(file))
            .ok()
            .and_then(|text| parse_bitmap(&text, WORD_BITS))
            .unwrap_or_default()
    };
    let kind = classify(
        &bitmap("capabilities/key"),
        &bitmap("capabilities/abs"),
        &bitmap("properties"),
    );
    if kind == NodeKind::Other {
        return None;
    }
    // The input device's parent: the HID device, or the USB interface for
    // `xpad`. Virtual devices without one group by themselves.
    let container = fs::canonicalize(input.join("device"))
        .or_else(|_| fs::canonicalize(&input))
        .ok()?
        .to_str()?
        .to_owned();
    Some(Node {
        dev: Path::new("/dev/input").join(name),
        kind,
        container,
    })
}

/// The absolute axes of an open node and their ranges.
fn abs_infos(device: &Device) -> io::Result<Vec<(u16, AbsInfo)>> {
    let Some(axes) = device.supported_absolute_axes() else {
        return Ok(Vec::new());
    };
    let state = device.get_abs_state()?;
    Ok(axes
        .iter()
        .filter_map(|axis| {
            let code = axis.0;
            let info = state.get(usize::from(code))?;
            Some((
                code,
                AbsInfo {
                    value: info.value,
                    minimum: info.minimum,
                    maximum: info.maximum,
                    fuzz: info.fuzz,
                    flat: info.flat,
                    resolution: info.resolution,
                },
            ))
        })
        .collect())
}

/// A provider error for an I/O error.
fn error(error: &io::Error) -> ProviderError {
    match error.raw_os_error() {
        // ENODEV: the device is gone.
        Some(19) => ProviderError::Disconnected,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied,
            io::ErrorKind::NotFound => ProviderError::Disconnected,
            io::ErrorKind::ResourceBusy => ProviderError::Busy,
            kind => ProviderError::Backend(kind.to_string()),
        },
    }
}

/// Opens a node for polling, grabbing it for exclusive access.
fn open_node(path: &Path, access: Access) -> Result<Device, ProviderError> {
    let mut device = Device::open(path).map_err(|e| error(&e))?;
    device.set_nonblocking(true).map_err(|e| error(&e))?;
    if access == Access::Exclusive {
        device.grab().map_err(|e| error(&e))?;
    }
    Ok(device)
}

/// Reads every pending event of a node; `each` sees them in order.
fn drain(device: &mut Device, mut each: impl FnMut(u16, u16, i32)) -> Result<(), ProviderError> {
    loop {
        match device.fetch_events() {
            Ok(events) => {
                let mut any = false;
                for event in events {
                    any = true;
                    each(event.event_type().0, event.code(), event.value());
                }
                if !any {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(error(&e)),
        }
    }
}

impl Provider for LinuxProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        for (&id, entry) in &mut self.entries {
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
        let groups = self.scan();

        // Removed: the gamepad node is gone, or its group changed.
        let gone: Vec<DeviceId> = self
            .entries
            .iter()
            .filter(|(_, entry)| !groups.contains(&entry.group))
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            self.entries.remove(&id);
            events.push(ProviderEvent::Removed(id));
        }

        // Added: seen in two scans in a row (or in the first one).
        let known: Vec<&Group> = self.entries.values().map(|entry| &entry.group).collect();
        let new: Vec<Group> = groups
            .iter()
            .filter(|group| !known.contains(group))
            .cloned()
            .collect();
        let mut inaccessible = 0;
        let mut waiting = Vec::new();
        for group in new {
            if !first && !self.pending.contains(&group) {
                waiting.push(group);
                continue;
            }
            match Self::probe(group.clone()) {
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
                    // Tried again on the next scan.
                    waiting.push(group);
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
        let entry = self.entry_mut(device)?;
        if let Some(open) = &mut entry.open {
            if open.access == access {
                return Ok(());
            }
            let grab = access == Access::Exclusive;
            let nodes = core::iter::once(&mut open.gamepad)
                .chain(open.motion.as_mut().map(|(device, _)| device))
                .chain(open.touchpad.as_mut().map(|(device, _)| device));
            for node in nodes {
                let result = if grab { node.grab() } else { node.ungrab() };
                // Ungrabbing a node that is not grabbed fails harmlessly.
                if grab {
                    result.map_err(|e| error(&e))?;
                }
            }
            if access == Access::ObserveOnly {
                if let Some(mut effect) = open.rumble.take() {
                    let _ = effect.stop();
                }
            }
            open.access = access;
            return Ok(());
        }
        let gamepad = open_node(&entry.group.gamepad, access)?;
        let motion = match (&entry.group.motion, &entry.motion) {
            (Some(path), Some(decoder)) => Some((open_node(path, access)?, decoder.clone())),
            _ => None,
        };
        let touchpad = match (&entry.group.touchpad, &entry.touchpad) {
            (Some(path), Some(decoder)) => Some((open_node(path, access)?, decoder.clone())),
            _ => None,
        };
        entry.open = Some(OpenNodes {
            access,
            gamepad,
            motion,
            touchpad,
            rumble: None,
        });
        Ok(())
    }

    fn close(&mut self, device: DeviceId) {
        if let Some(entry) = self.entries.get_mut(&device) {
            if let Some(mut open) = entry.open.take() {
                if let Some(mut effect) = open.rumble.take() {
                    let _ = effect.stop();
                }
            }
        }
    }

    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
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
            drain(&mut open.gamepad, |_, _, _| {})?;
            let cached = open.gamepad.cached_state();
            let keys = cached.key_vals();
            let abs = cached.abs_vals();
            entry.node.fill(
                |code| keys.is_some_and(|keys| keys.contains(KeyCode(code))),
                |code| {
                    abs.and_then(|abs| abs.get(usize::from(code)))
                        .map_or(0, |info| info.value)
                },
                state,
            );
            state.motion.clear();
            if let Some((device, decoder)) = &mut open.motion {
                drain(device, |kind, code, value| {
                    if let Some(sample) = decoder.event(kind, code, value) {
                        state.motion.push(sample);
                    }
                })?;
            }
            if let Some((device, decoder)) = &mut open.touchpad {
                drain(device, |kind, code, value| decoder.event(kind, code, value))?;
                decoder.fill(&mut state.touches);
                if let Some(click) = state.buttons.last_mut() {
                    *click = if decoder.clicked() { 1.0 } else { 0.0 };
                }
            }
            Ok(())
        })();
        if result == Err(ProviderError::Disconnected) {
            entry.disconnected = true;
            entry.open = None;
        }
        result
    }

    fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        let entry = self.entry_mut(device)?;
        if !entry.info.outputs.contains(command.output()) {
            return Err(ProviderError::Unsupported);
        }
        let open = entry.open.as_mut().ok_or(ProviderError::NotOpen)?;
        if open.access == Access::ObserveOnly {
            return Err(ProviderError::Unsupported);
        }
        let OutputCommand::Rumble(rumble) = command else {
            return Err(ProviderError::Unsupported);
        };
        let magnitude = |value: f32| (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        let data = FFEffectData {
            direction: 0,
            trigger: Default::default(),
            // Until replaced or stopped (the longest replay the kernel
            // takes, about 33 seconds).
            replay: evdev::FFReplay {
                length: 0x7fff,
                delay: 0,
            },
            kind: FFEffectKind::Rumble {
                strong_magnitude: magnitude(rumble.low_frequency),
                weak_magnitude: magnitude(rumble.high_frequency),
            },
        };
        let result = match &mut open.rumble {
            Some(effect) => effect.update(data),
            None => open.gamepad.upload_ff_effect(data).map(|effect| {
                open.rumble = Some(effect);
            }),
        };
        result.map_err(|e| error(&e))?;
        if let Some(effect) = &mut open.rumble {
            let play = if rumble.low_frequency <= 0.0 && rumble.high_frequency <= 0.0 {
                effect.stop()
            } else {
                effect.play(1)
            };
            play.map_err(|e| error(&e))?;
        }
        Ok(())
    }
}

/// The absolute axis codes `evdev` knows, to check the kernel's constants
/// against.
#[cfg(test)]
const _: () = {
    use super::evdev::codes;
    use evdev::AbsoluteAxisCode;

    assert!(AbsoluteAxisCode::ABS_MT_SLOT.0 == codes::ABS_MT_SLOT);
    assert!(AbsoluteAxisCode::ABS_HAT0X.0 == codes::ABS_HAT0X);
    assert!(KeyCode::BTN_SOUTH.0 == codes::BTN_SOUTH);
    assert!(KeyCode::BTN_TRIGGER_HAPPY1.0 == codes::BTN_TRIGGER_HAPPY);
    assert!(FFEffectCode::FF_RUMBLE.0 == codes::FF_RUMBLE);
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake `/sys/class/input` with a DualSense's three nodes, a
    /// keyboard and a lone joystick.
    fn fake_sysfs(root: &Path) {
        let devices = root.join("devices");
        let class = root.join("class");
        let hid = devices.join("usb1/1-1/1-1:1.3/0003:054C:0CE6.0001");
        let node = |n: u32, parent: &Path, key: &str, abs: &str, properties: &str| {
            let input = parent.join(format!("input/input{n}"));
            fs::create_dir_all(input.join("capabilities")).unwrap();
            fs::write(input.join("capabilities/key"), key).unwrap();
            fs::write(input.join("capabilities/abs"), abs).unwrap();
            fs::write(input.join("properties"), properties).unwrap();
            let event = class.join(format!("event{n}"));
            fs::create_dir_all(&event).unwrap();
            std::os::unix::fs::symlink(&input, event.join("device")).unwrap();
            std::os::unix::fs::symlink(parent, input.join("device")).unwrap();
        };
        // Gamepad: BTN_SOUTH; motion: accelerometer property; touchpad:
        // BTN_LEFT, MT positions.
        node(3, &hid, "1000000000000 0 0 0 0", "3003f", "0");
        node(4, &hid, "0", "3f", "40");
        node(5, &hid, "10000 0 0 0 0", "260800000000000", "4");
        let keyboard = devices.join("platform/i8042/serio0");
        node(1, &keyboard, "fffffffffffffffe", "0", "0");
        let other = devices.join("usb1/1-2/1-2:1.0");
        node(7, &other, "1000000000000 0 0 0 0", "3", "0");
    }

    #[test]
    fn scan_groups_nodes_and_skips_keyboards() {
        let dir = std::env::temp_dir().join(format!("anygamepad-sysfs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fake_sysfs(&dir);
        let provider = LinuxProvider::with_sysfs(dir.join("class"));
        let groups = provider.scan();
        assert_eq!(groups.len(), 2, "{groups:?}");
        let dualsense = groups
            .iter()
            .find(|g| g.gamepad == Path::new("/dev/input/event3"))
            .unwrap();
        assert_eq!(
            dualsense.motion.as_deref(),
            Some(Path::new("/dev/input/event4"))
        );
        assert_eq!(
            dualsense.touchpad.as_deref(),
            Some(Path::new("/dev/input/event5"))
        );
        assert!(dualsense.container.ends_with("0003:054C:0CE6.0001"));
        let joystick = groups
            .iter()
            .find(|g| g.gamepad == Path::new("/dev/input/event7"))
            .unwrap();
        assert_eq!(
            (joystick.motion.clone(), joystick.touchpad.clone()),
            (None, None)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_devices_without_sysfs() {
        let mut provider = LinuxProvider::with_sysfs("/nonexistent/anygamepad");
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        assert!(events.is_empty());
        assert_eq!(provider.inaccessible(), 0);
        let mut state = RawState::default();
        assert_eq!(
            provider.poll(DeviceId(1), &mut state),
            Err(ProviderError::UnknownDevice)
        );
    }

    #[test]
    fn io_errors() {
        assert_eq!(
            error(&io::Error::from_raw_os_error(19)),
            ProviderError::Disconnected
        );
        assert_eq!(
            error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            ProviderError::PermissionDenied
        );
    }
}
