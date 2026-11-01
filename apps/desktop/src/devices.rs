//! The device worker (A2): a thread that owns the input provider, maps
//! each device to the canonical model, and publishes a snapshot the UI
//! draws. It wakes the UI only when something changed, so the window keeps
//! repainting on demand. It also runs the test mode's session (H5,
//! [`crate::test_mode`]).

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anygamepad::db::Database;
use anygamepad::mapping::Mapper;
use anygamepad::model::raw::{RawLayout, RawSource, RawState};
use anygamepad::model::{Capabilities, GamepadState, OutputCommand, OutputSet};
use anygamepad::provider::{Access, DeviceId, DeviceInfo, Provider, ProviderEvent};
use eframe::egui;

use crate::demo::Autopilot;
use crate::test_mode::{TestCommand, TestRun, TestState};

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
    /// Canonical controls the mapping gives.
    pub capabilities: Capabilities,
    /// Why the device cannot be read, if it cannot.
    pub error: Option<String>,
    /// Whether the app made it: a virtual pad of a test session. Collection
    /// mode leaves it out.
    pub made_here: bool,
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
    /// The test session, while there is one.
    pub test: Option<TestState>,
}

/// Requests from the UI.
#[derive(Debug)]
pub enum Command {
    /// Drives an output of a device.
    Send(DeviceId, OutputCommand),
    /// Drives the test session.
    Test(TestCommand),
}

/// An input provider for the worker.
pub struct Backend {
    /// Its name, for the UI.
    pub name: &'static str,
    /// The provider.
    pub provider: Box<dyn Provider + Send>,
    /// Makes the demo pad follow test sessions (demo provider only).
    pub autopilot: Option<Autopilot>,
}

/// The handle the UI keeps.
pub struct Devices {
    snapshot: Arc<Mutex<Snapshot>>,
    commands: Sender<Command>,
}

impl Devices {
    /// Starts the worker with `backend` (`None` when this OS has no
    /// provider yet). `ctx` is woken on changes.
    pub fn start(backend: Option<Backend>, ctx: egui::Context) -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let (commands, receiver) = channel();
        if let Some(backend) = backend {
            snapshot.lock().expect("fresh mutex").backend = Some(backend.name);
            let shared = Arc::clone(&snapshot);
            let spawned = thread::Builder::new()
                .name("devices".into())
                .spawn(move || Worker::new(backend, shared, receiver, ctx).run());
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
pub fn native_provider() -> Option<Backend> {
    if std::env::var_os("ANYGAMEPAD_DEMO").is_some() {
        return Some(demo_backend());
    }
    #[cfg(target_os = "linux")]
    {
        Some(Backend {
            name: "Linux evdev",
            provider: Box::new(anygamepad::native::linux::LinuxProvider::new()),
            autopilot: None,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The demo provider, its pad following test sessions.
pub fn demo_backend() -> Backend {
    let autopilot = Autopilot::default();
    Backend {
        name: "demo",
        provider: Box::new(crate::demo::DemoProvider::with_autopilot(autopilot.clone())),
        autopilot: Some(autopilot),
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

/// A device the worker reads.
pub struct Open {
    mapper: Mapper,
    /// Latest raw values.
    pub raw: RawState,
    /// Latest canonical state.
    pub state: GamepadState,
}

struct Worker {
    provider: Box<dyn Provider + Send>,
    autopilot: Option<Autopilot>,
    database: Database,
    shared: Arc<Mutex<Snapshot>>,
    commands: Receiver<Command>,
    ctx: egui::Context,
    devices: BTreeMap<DeviceId, (DeviceView, Option<Open>)>,
    test: Option<TestRun>,
    /// The origin of the test session's clock.
    origin: Instant,
}

impl Worker {
    fn new(
        backend: Backend,
        shared: Arc<Mutex<Snapshot>>,
        commands: Receiver<Command>,
        ctx: egui::Context,
    ) -> Self {
        Self {
            provider: backend.provider,
            autopilot: backend.autopilot,
            database: Database::embedded(),
            shared,
            commands,
            ctx,
            devices: BTreeMap::new(),
            test: None,
            origin: Instant::now(),
        }
    }

    /// Milliseconds since the worker started.
    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
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
                let now = self.now();
                match event {
                    ProviderEvent::Added(id) => {
                        self.add(id);
                        if let (Some(test), Some((view, _))) =
                            (&mut self.test, self.devices.get(&id))
                        {
                            test.added(id, &view.info, now);
                        }
                    }
                    ProviderEvent::Removed(id) => {
                        log::info!("device {} removed", id.0);
                        self.devices.remove(&id);
                        if let Some(test) = &mut self.test {
                            test.removed(id, now);
                        }
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
                    Ok(Command::Test(command)) => {
                        self.test_command(command);
                        changed = true;
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
            let now = self.now();
            if let Some(test) = &mut self.test {
                changed |= test.tick(self.provider.as_mut(), &self.devices, now);
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
                    shared.test = self.test.as_ref().map(|test| test.state().clone());
                }
                self.ctx.request_repaint();
            }
            // Poll fast while something is open; otherwise only hotplug.
            let open = self.devices.values().any(|(_, open)| open.is_some());
            thread::sleep(Duration::from_millis(if open { 4 } else { 100 }));
        }
    }

    fn test_command(&mut self, command: TestCommand) {
        let now = self.now();
        match command {
            TestCommand::Start(id, session) => {
                if let Some(mut old) = self.test.take() {
                    old.command(TestCommand::Close, self.provider.as_mut(), now);
                }
                self.test = Some(TestRun::new(id, *session, self.autopilot.clone()));
            }
            TestCommand::Close => {
                if let Some(mut old) = self.test.take() {
                    old.command(TestCommand::Close, self.provider.as_mut(), now);
                }
            }
            command => {
                if let Some(test) = &mut self.test {
                    test.command(command, self.provider.as_mut(), now);
                }
            }
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
        let capabilities = mapper.capabilities().clone();
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
        let made_here = self
            .test
            .as_ref()
            .is_some_and(|test| test.made_here(id, &info));
        let view = DeviceView {
            id,
            info,
            mapping,
            raw,
            state: GamepadState::new(),
            capabilities,
            error,
            made_here,
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
        let devices = Devices::start(Some(demo_backend()), egui::Context::default());
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
