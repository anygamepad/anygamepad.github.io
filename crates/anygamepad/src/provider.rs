//! Input providers (ADR 0002): the sources devices are read from.
//!
//! Several providers can see the same physical device (native, SDL3, gilrs);
//! arbitration (C12) correlates their views with [`DeviceLocation`] and the
//! identity, and lets only one of them open it.

use core::fmt;

use anygamepad_model::identity::{DeviceIdentity, ProviderKind};
use anygamepad_model::raw::{RawLayout, RawState};
use anygamepad_model::{OutputCommand, OutputSet};

/// A device within one provider. Valid until the provider reports it
/// removed; never reused for another device by the same provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u64);

/// A change in the devices a provider sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderEvent {
    /// A device appeared; [`Provider::info`] describes it.
    Added(DeviceId),
    /// A device disappeared; its id is no longer valid.
    Removed(DeviceId),
}

/// How a device is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Access {
    /// Read input and drive outputs, including any handshake the device
    /// needs. Other applications keep seeing the device.
    Shared,
    /// Like [`Access::Shared`], and hide the device from other readers where
    /// the OS allows it (evdev grab, IOKit seize), for when a virtual
    /// controller replaces it.
    Exclusive,
    /// Read input only: no output reports, no handshakes, no mode switches.
    /// Used by collection mode to record every provider's view of a device.
    ObserveOnly,
}

/// Where the OS exposes a device, to correlate the views several providers
/// have of it.
///
/// Never serialized, sent or logged: OS paths and instance IDs can contain
/// serial numbers and Bluetooth addresses. Its `Debug` output is redacted.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct DeviceLocation {
    path: Option<String>,
    container: Option<String>,
}

impl DeviceLocation {
    /// A location from the device node or instance ID (`path`) and the key
    /// shared by every node of the same physical device (`container`).
    pub fn new(path: Option<String>, container: Option<String>) -> Self {
        Self { path, container }
    }

    /// OS device node or instance ID (`/dev/input/event5`,
    /// `\\?\HID#VID_054C&PID_0CE6...`).
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Key shared by every node of one physical device: on Linux the sysfs
    /// path of the parent device, on Windows the container ID, on macOS the
    /// location ID.
    pub fn container(&self) -> Option<&str> {
        self.container.as_deref()
    }

    /// Whether both locations belong to the same physical device: same
    /// container, or same path. Unknown locations never match.
    pub fn same_device(&self, other: &Self) -> bool {
        let same = |a: &Option<String>, b: &Option<String>| a.is_some() && a == b;
        same(&self.container, &other.container) || same(&self.path, &other.path)
    }
}

impl fmt::Debug for DeviceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceLocation { .. }")
    }
}

/// What a provider knows about a device, before or after opening it.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    /// Identity as this provider sees it.
    pub identity: DeviceIdentity,
    /// Where the OS exposes it.
    pub location: DeviceLocation,
    /// Raw elements [`Provider::poll`] fills.
    pub layout: RawLayout,
    /// Outputs [`Provider::send`] can drive.
    pub outputs: OutputSet,
}

/// Error of a provider operation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// The device was removed.
    Disconnected,
    /// The provider does not know this device id.
    UnknownDevice,
    /// The device is not open.
    NotOpen,
    /// The OS or the browser denied access (missing udev rule, permission
    /// prompt declined, ...).
    PermissionDenied,
    /// Another process holds the device exclusively.
    Busy,
    /// The device, the provider or the access mode does not allow this
    /// operation (an output in [`Access::ObserveOnly`], a missing output,
    /// exclusive access where the OS has none, ...).
    Unsupported,
    /// Any other backend failure. The message must not contain device paths
    /// or other data that can identify a person.
    Backend(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => f.write_str("device disconnected"),
            Self::UnknownDevice => f.write_str("unknown device"),
            Self::NotOpen => f.write_str("device not open"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::Busy => f.write_str("device busy"),
            Self::Unsupported => f.write_str("operation not supported"),
            Self::Backend(message) => write!(f, "backend error: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// An input source: anygamepad's native and web backends, SDL3 or gilrs.
///
/// The caller drives providers from its frame loop: [`Provider::update`]
/// once per frame, then [`Provider::poll`] for every open device. No method
/// blocks; a provider that needs it runs its own thread for hotplug or
/// reads.
pub trait Provider {
    /// Which provider this is.
    fn kind(&self) -> ProviderKind;

    /// Processes hotplug and refreshes backend data, appending what changed
    /// to `events`. The first call reports every connected device as
    /// [`ProviderEvent::Added`].
    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError>;

    /// Describes a device this provider sees, open or not.
    fn info(&self, device: DeviceId) -> Option<&DeviceInfo>;

    /// Opens a device. Opening an open device again changes its access mode.
    fn open(&mut self, device: DeviceId, access: Access) -> Result<(), ProviderError>;

    /// Closes a device and stops its outputs; does nothing if it is not
    /// open.
    fn close(&mut self, device: DeviceId);

    /// Writes the latest raw values of an open device into `state`, which
    /// must fit its layout ([`RawState::for_layout`]). Motion samples
    /// received since the previous poll replace `state.motion`.
    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError>;

    /// Drives an output of an open device.
    fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError>;

    /// Devices the provider sees but cannot open (missing permissions,
    /// usually), so an app can explain why they do not appear.
    fn inaccessible(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anygamepad_model::identity::{Backend, Os};
    use anygamepad_model::raw::RawSource;
    use anygamepad_model::{Button, Output, Rumble};

    use super::*;

    /// A provider with one scripted device, to check that the trait is
    /// usable as `dyn Provider` and that its contract fits together.
    struct ScriptedProvider {
        info: DeviceInfo,
        connected: bool,
        reported_added: bool,
        reported_removed: bool,
        open: BTreeMap<DeviceId, Access>,
        pressed: bool,
        sent: Vec<OutputCommand>,
    }

    const PAD: DeviceId = DeviceId(1);

    impl ScriptedProvider {
        fn new() -> Self {
            let info = DeviceInfo {
                identity: DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux),
                location: DeviceLocation::new(
                    Some("/dev/input/event5".into()),
                    Some("/sys/devices/usb1/1-2".into()),
                ),
                layout: RawLayout {
                    buttons: vec![RawSource::Button(Button::South)],
                    ..RawLayout::default()
                },
                outputs: OutputSet::from([Output::Rumble]),
            };
            Self {
                info,
                connected: true,
                reported_added: false,
                reported_removed: false,
                open: BTreeMap::new(),
                pressed: true,
                sent: Vec::new(),
            }
        }

        fn check_open(&self, device: DeviceId) -> Result<Access, ProviderError> {
            if device != PAD {
                return Err(ProviderError::UnknownDevice);
            }
            if !self.connected {
                return Err(ProviderError::Disconnected);
            }
            self.open
                .get(&device)
                .copied()
                .ok_or(ProviderError::NotOpen)
        }
    }

    impl Provider for ScriptedProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Native
        }

        fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
            if !self.reported_added {
                self.reported_added = true;
                events.push(ProviderEvent::Added(PAD));
            } else if !self.connected && !self.reported_removed {
                self.reported_removed = true;
                self.open.remove(&PAD);
                events.push(ProviderEvent::Removed(PAD));
            }
            Ok(())
        }

        fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
            (device == PAD && self.connected).then_some(&self.info)
        }

        fn open(&mut self, device: DeviceId, access: Access) -> Result<(), ProviderError> {
            if self.info(device).is_none() {
                return Err(ProviderError::UnknownDevice);
            }
            self.open.insert(device, access);
            Ok(())
        }

        fn close(&mut self, device: DeviceId) {
            self.open.remove(&device);
        }

        fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
            self.check_open(device)?;
            state.buttons[0] = if self.pressed { 1.0 } else { 0.0 };
            Ok(())
        }

        fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
            if self.check_open(device)? == Access::ObserveOnly
                || !self.info.outputs.contains(command.output())
            {
                return Err(ProviderError::Unsupported);
            }
            self.sent.push(command);
            Ok(())
        }
    }

    #[test]
    fn provider_lifecycle_through_dyn() {
        let mut provider: Box<dyn Provider> = Box::new(ScriptedProvider::new());
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        assert_eq!(events, [ProviderEvent::Added(PAD)]);

        let info = provider.info(PAD).unwrap();
        let mut state = RawState::for_layout(&info.layout);
        assert_eq!(provider.poll(PAD, &mut state), Err(ProviderError::NotOpen));

        provider.open(PAD, Access::Shared).unwrap();
        provider.poll(PAD, &mut state).unwrap();
        assert_eq!(state.buttons, [1.0]);

        let rumble = OutputCommand::Rumble(Rumble {
            low_frequency: 1.0,
            high_frequency: 0.5,
        });
        provider.send(PAD, rumble).unwrap();
        let lightbar = OutputCommand::Lightbar(Default::default());
        assert_eq!(
            provider.send(PAD, lightbar),
            Err(ProviderError::Unsupported)
        );

        provider.open(PAD, Access::ObserveOnly).unwrap();
        assert_eq!(provider.send(PAD, rumble), Err(ProviderError::Unsupported));

        provider.close(PAD);
        assert_eq!(provider.poll(PAD, &mut state), Err(ProviderError::NotOpen));
        assert_eq!(
            provider.poll(DeviceId(9), &mut state),
            Err(ProviderError::UnknownDevice)
        );
    }

    #[test]
    fn removal_is_reported() {
        let mut provider = ScriptedProvider::new();
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        provider.open(PAD, Access::Shared).unwrap();
        provider.connected = false;
        let mut state = RawState::for_layout(&provider.info.layout);
        assert_eq!(
            provider.poll(PAD, &mut state),
            Err(ProviderError::Disconnected)
        );
        events.clear();
        provider.update(&mut events).unwrap();
        provider.update(&mut events).unwrap();
        assert_eq!(events, [ProviderEvent::Removed(PAD)]);
        assert!(provider.info(PAD).is_none());
    }

    #[test]
    fn location_debug_is_redacted() {
        let location = DeviceLocation::new(
            Some("/dev/hidraw0".into()),
            Some("A4:AE:12:34:56:78".into()),
        );
        let debug = format!("{location:?}");
        assert!(!debug.contains("hidraw"));
        assert!(!debug.contains("A4:AE"));
        assert_eq!(location.path(), Some("/dev/hidraw0"));
    }

    #[test]
    fn locations_match_by_container_or_path() {
        let node = |path: &str, container: Option<&str>| {
            DeviceLocation::new(Some(path.into()), container.map(Into::into))
        };
        let gamepad = node("/dev/input/event5", Some("/sys/devices/usb1/1-2"));
        let motion = node("/dev/input/event6", Some("/sys/devices/usb1/1-2"));
        let other = node("/dev/input/event7", Some("/sys/devices/usb1/1-3"));
        assert!(gamepad.same_device(&motion));
        assert!(!gamepad.same_device(&other));
        assert!(node("/dev/hidraw1", None).same_device(&node("/dev/hidraw1", None)));
        assert!(!DeviceLocation::default().same_device(&DeviceLocation::default()));
    }

    #[test]
    fn errors_display() {
        assert_eq!(ProviderError::Busy.to_string(), "device busy");
        assert_eq!(
            ProviderError::Backend("timeout".into()).to_string(),
            "backend error: timeout"
        );
    }
}
