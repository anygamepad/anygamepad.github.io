//! The device worker (A2): a thread that owns the input provider, maps
//! each device to the canonical model, and publishes a snapshot the UI
//! draws. It wakes the UI only when something changed, so the window keeps
//! repainting on demand.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anygamepad::db::Database;
use anygamepad::mapping::Mapper;
use anygamepad::model::raw::{RawLayout, RawSource, RawState};
use anygamepad::model::{GamepadState, OutputCommand, OutputSet};
use anygamepad::provider::{Access, DeviceId, DeviceInfo, Provider, ProviderEvent};
use eframe::egui;

/// How the canonical state of a device is obtained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingSource {
    /// A profile from the device database, by name.
    Profile(String),
    /// The Linux gamepad specification, for drivers that follow it.
    LinuxSpec,
    /// The backend already gives canonical buttons and axes.
    Canonical,
    /// None: only raw values.
    None,
}

/// One device, as the UI shows it.
#[derive(Clone, Debug)]
pub struct DeviceView {
    /// The provider's id.
    pub id: DeviceId,
    /// Identity and layout.
    pub info: DeviceInfo,
    /// Where the mapping comes from.
    pub mapping: MappingSource,
    /// Latest raw values.
    pub raw: RawState,
    /// Latest canonical state.
    pub state: GamepadState,
    /// Why the device cannot be read, if it cannot.
    pub error: Option<String>,
}

/// What the worker publishes.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Which provider runs, for the UI; `None` when this OS has none yet.
    pub backend: Option<&'static str>,
    /// Devices, in the order they appeared.
    pub devices: Vec<DeviceView>,
    /// Gamepads the provider could not open.
    pub inaccessible: usize,
}

/// Requests from the UI.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    /// Drives an output of a device.
    Send(DeviceId, OutputCommand),
}

/// The handle the UI keeps.
pub struct Devices {
    snapshot: Arc<Mutex<Snapshot>>,
    commands: Sender<Command>,
}

impl Devices {
    /// Starts the worker with `provider` (`None` when this OS has no
    /// provider yet). `ctx` is woken on changes.
    pub fn start(
        provider: Option<(&'static str, Box<dyn Provider + Send>)>,
        ctx: egui::Context,
    ) -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let (commands, receiver) = channel();
        if let Some((name, provider)) = provider {
            snapshot.lock().expect("fresh mutex").backend = Some(name);
            let shared = Arc::clone(&snapshot);
            let spawned = thread::Builder::new()
                .name("devices".into())
                .spawn(move || Worker::new(provider, shared, receiver, ctx).run());
            if let Err(e) = spawned {
                log::error!("device thread not started: {e}");
            }
        }
        Self { snapshot, commands }
    }

    /// A copy of the latest snapshot.
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Sends a command to the worker.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// The provider for this OS, if there is one yet.
pub fn native_provider() -> Option<(&'static str, Box<dyn Provider + Send>)> {
    if std::env::var_os("ANYGAMEPAD_DEMO").is_some() {
        return Some(("demo", Box::new(crate::demo::DemoProvider::default())));
    }
    #[cfg(target_os = "linux")]
    {
        Some((
            "Linux evdev",
            Box::new(anygamepad::native::linux::LinuxProvider::new()),
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The mapping for a device: its profile, else the Linux gamepad
/// specification for drivers that follow it, else nothing.
pub fn mapper_for(database: &Database, info: &DeviceInfo) -> (Mapper, MappingSource) {
    if let Some((mapper, profile)) = Mapper::from_database(database, &info.identity, &info.layout) {
        return (mapper, MappingSource::Profile(profile.name.clone()));
    }
    let driver = info.identity.driver.as_deref();
    if anygamepad::native::evdev::follows_spec(driver) {
        let mapping = anygamepad::native::evdev::spec_mapping(&info.layout, driver);
        return (
            Mapper::new(&mapping, &info.layout),
            MappingSource::LinuxSpec,
        );
    }
    let canonical = info
        .layout
        .buttons
        .iter()
        .chain(info.layout.axes.iter().map(|axis| &axis.source))
        .any(|source| matches!(source, RawSource::Button(_) | RawSource::Axis(_)));
    let source = if canonical {
        MappingSource::Canonical
    } else {
        MappingSource::None
    };
    (Mapper::new(&Default::default(), &info.layout), source)
}

struct Open {
    mapper: Mapper,
    raw: RawState,
    state: GamepadState,
}

struct Worker {
    provider: Box<dyn Provider + Send>,
    database: Database,
    shared: Arc<Mutex<Snapshot>>,
    commands: Receiver<Command>,
    ctx: egui::Context,
    devices: BTreeMap<DeviceId, (DeviceView, Option<Open>)>,
}

impl Worker {
    fn new(
        provider: Box<dyn Provider + Send>,
        shared: Arc<Mutex<Snapshot>>,
        commands: Receiver<Command>,
        ctx: egui::Context,
    ) -> Self {
        Self {
            provider,
            database: Database::embedded(),
            shared,
            commands,
            ctx,
            devices: BTreeMap::new(),
        }
    }

    fn run(mut self) {
        let mut events = Vec::new();
        loop {
            let mut changed = false;
            events.clear();
            if let Err(e) = self.provider.update(&mut events) {
                log::warn!("provider update: {e}");
            }
            for event in events.drain(..) {
                changed = true;
                match event {
                    ProviderEvent::Added(id) => self.add(id),
                    ProviderEvent::Removed(id) => {
                        log::info!("device {} removed", id.0);
                        self.devices.remove(&id);
                    }
                }
            }
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Send(id, command)) => {
                        if let Err(e) = self.provider.send(id, command) {
                            log::info!("output to device {}: {e}", id.0);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    // The UI is gone.
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            for (id, (view, open)) in &mut self.devices {
                let Some(open) = open else {
                    continue;
                };
                match self.provider.poll(*id, &mut open.raw) {
                    Ok(()) => {
                        open.mapper.apply(&open.raw, &mut open.state);
                        if open.state != view.state || open.raw != view.raw {
                            view.state = open.state;
                            view.raw.clone_from(&open.raw);
                            changed = true;
                        }
                    }
                    Err(e) => {
                        let message = e.to_string();
                        if view.error.as_ref() != Some(&message) {
                            view.error = Some(message);
                            changed = true;
                        }
                    }
                }
            }
            let inaccessible = self.provider.inaccessible();
            if changed || self.published_inaccessible() != inaccessible {
                if let Ok(mut shared) = self.shared.lock() {
                    shared.devices = self
                        .devices
                        .values()
                        .map(|(view, _)| view.clone())
                        .collect();
                    shared.inaccessible = inaccessible;
                }
                self.ctx.request_repaint();
            }
            // Poll fast while something is open; otherwise only hotplug.
            let open = self.devices.values().any(|(_, open)| open.is_some());
            thread::sleep(Duration::from_millis(if open { 4 } else { 100 }));
        }
    }

    fn published_inaccessible(&self) -> usize {
        self.shared.lock().map(|s| s.inaccessible).unwrap_or(0)
    }

    fn add(&mut self, id: DeviceId) {
        let Some(info) = self.provider.info(id).cloned() else {
            return;
        };
        let (mapper, mapping) = mapper_for(&self.database, &info);
        log::info!(
            "device {} added: {} ({:04x}:{:04x}), mapping {mapping:?}",
            id.0,
            info.identity.product_name.as_deref().unwrap_or("unnamed"),
            info.identity.vendor_id.unwrap_or(0),
            info.identity.product_id.unwrap_or(0),
        );
        let raw = RawState::for_layout(&info.layout);
        let (open, error) = match self.provider.open(id, Access::Shared) {
            Ok(()) => (
                Some(Open {
                    mapper,
                    raw: raw.clone(),
                    state: GamepadState::new(),
                }),
                None,
            ),
            Err(e) => (None, Some(e.to_string())),
        };
        let view = DeviceView {
            id,
            info,
            mapping,
            raw,
            state: GamepadState::new(),
            error,
        };
        self.devices.insert(id, (view, open));
    }
}

/// Whether a device can rumble, for the UI.
pub fn can_rumble(outputs: OutputSet) -> bool {
    outputs.contains(anygamepad::model::Output::Rumble)
}

/// A layout's element counts, for the UI.
pub fn summary(layout: &RawLayout) -> String {
    format!(
        "{} buttons, {} axes, {} hats",
        layout.buttons.len(),
        layout.axes.len(),
        layout.hats.len()
    )
}

#[cfg(test)]
mod tests {
    use anygamepad::model::identity::{Backend, DeviceIdentity, Os, ProviderKind};
    use anygamepad::model::raw::RawSource;
    use anygamepad::model::{Button, OutputSet};
    use anygamepad::provider::{DeviceInfo, DeviceLocation};

    use super::*;

    fn info(driver: Option<&str>, buttons: Vec<RawSource>) -> DeviceInfo {
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux);
        identity.driver = driver.map(str::to_owned);
        identity.vendor_id = Some(0x1234);
        identity.product_id = Some(0x5678);
        DeviceInfo {
            identity,
            location: DeviceLocation::default(),
            layout: RawLayout {
                buttons,
                ..RawLayout::default()
            },
            outputs: OutputSet::EMPTY,
        }
    }

    #[test]
    fn mapping_sources() {
        let database = Database::embedded();
        let south = vec![RawSource::Evdev(0x130)];
        let (mapper, source) = mapper_for(&database, &info(Some("playstation"), south.clone()));
        assert_eq!(source, MappingSource::LinuxSpec);
        assert!(mapper.capabilities().has_button(Button::South));
        let (_, source) = mapper_for(&database, &info(Some("hid-generic"), south));
        assert_eq!(source, MappingSource::None);
        let canonical = vec![RawSource::Button(Button::South)];
        let (_, source) = mapper_for(&database, &info(None, canonical));
        assert_eq!(source, MappingSource::Canonical);
    }

    #[test]
    fn demo_worker_publishes_the_pad() {
        let devices = Devices::start(
            Some(("demo", Box::new(crate::demo::DemoProvider::default()))),
            egui::Context::default(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while devices.snapshot().devices.is_empty() {
            assert!(std::time::Instant::now() < deadline, "no device published");
            thread::sleep(Duration::from_millis(10));
        }
        let snapshot = devices.snapshot();
        assert_eq!(snapshot.backend, Some("demo"));
        assert_eq!(snapshot.devices[0].mapping, MappingSource::Canonical);
        assert!(snapshot.devices[0].error.is_none());
    }
}
