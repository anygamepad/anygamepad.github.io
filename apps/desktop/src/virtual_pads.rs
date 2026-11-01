//! The virtual output suite of the test mode (H5, check `virtual_output`):
//! which virtual pad types this machine can create, and the check of one.
//!
//! A check creates a virtual pad, shows a series of states on it (at rest,
//! each button alone, each stick axis at both ends, each trigger pulled),
//! and reads each one back through the app's own provider and mapping, as
//! a game on this machine would see it. [`Checker`] is the check without
//! I/O; the device worker owns the pad (Linux: uinput and UHID, A5) and
//! feeds the checker what its provider reads.
//!
//! After the check the pad copies the pad under test, as the app's virtual
//! pads do, and passes on to it the vibration a game sends: the session
//! asks the person whether it arrived.

use std::collections::BTreeSet;

use anygamepad::model::{Axis, Button, Capabilities, GamepadState, Output};
use anygamepad::provider::{DeviceId, DeviceInfo};
use anygamepad::testing::{VirtualCheck, VirtualTarget};
use anygamepad_virtual::Target;

/// How long a new virtual pad may take to show up in the app's provider,
/// in milliseconds. Drivers ask the pad for its feature reports first.
const APPEAR_MS: f64 = 5000.0;
/// How long a state may take to read back, in milliseconds.
const SETTLE_MS: f64 = 1000.0;
/// How far a read back axis may be from the value shown.
const AXIS_TOLERANCE: f32 = 0.15;

/// A target's name in step IDs.
pub fn name(target: Target) -> &'static str {
    match target {
        Target::Xbox360 => "xbox360",
        Target::DualShock4 => "dualshock4",
        Target::DualSense => "dualsense",
        Target::SwitchPro => "switch-pro",
        _ => "other",
    }
}

/// A target's name for people.
pub fn title(target: Target) -> &'static str {
    match target {
        Target::Xbox360 => "Xbox 360 Controller",
        Target::DualShock4 => "DualShock 4",
        Target::DualSense => "DualSense",
        Target::SwitchPro => "Switch Pro Controller",
        _ => "virtual pad",
    }
}

/// The target named `name` in step IDs.
pub fn target_named(name: &str) -> Option<Target> {
    Target::ALL.into_iter().find(|&t| self::name(t) == name)
}

/// The virtual pad types, each with why this machine cannot create it, if
/// it cannot.
pub fn targets() -> Vec<VirtualTarget> {
    Target::ALL
        .into_iter()
        .map(|target| VirtualTarget {
            name: name(target),
            title: title(target),
            rumble: target
                .create(0)
                .capabilities()
                .outputs
                .contains(Output::Rumble),
            unavailable: unavailable(target),
        })
        .collect()
}

/// Why this machine cannot create `target`: uinput for the Xbox 360
/// Controller, UHID for the others, writable by this user.
#[cfg(target_os = "linux")]
fn unavailable(target: Target) -> Option<&'static str> {
    use std::io::ErrorKind;
    let uinput = target.create(0).report_descriptor().is_none();
    let opened = std::fs::OpenOptions::new().write(true).open(if uinput {
        "/dev/uinput"
    } else {
        "/dev/uhid"
    });
    match (opened, uinput) {
        (Ok(_), _) => None,
        (Err(e), true) if e.kind() == ErrorKind::NotFound => Some("no /dev/uinput"),
        (Err(e), false) if e.kind() == ErrorKind::NotFound => Some("no /dev/uhid"),
        (Err(e), true) if e.kind() == ErrorKind::PermissionDenied => {
            Some("no permission to write /dev/uinput")
        }
        (Err(e), false) if e.kind() == ErrorKind::PermissionDenied => {
            Some("no permission to write /dev/uhid")
        }
        (Err(_), true) => Some("cannot open /dev/uinput"),
        (Err(_), false) => Some("cannot open /dev/uhid"),
    }
}

/// Windows (A7) and macOS (A8) have no virtual pads yet.
#[cfg(not(target_os = "linux"))]
fn unavailable(_target: Target) -> Option<&'static str> {
    Some("no virtual pads on this OS yet")
}

/// The states a check shows, in order: at rest, each button alone, each
/// stick axis at both ends, each trigger pulled with its button, at rest.
pub fn script(capabilities: &Capabilities) -> Vec<GamepadState> {
    let rest = GamepadState::new();
    let mut states = vec![rest];
    for button in capabilities.buttons.iter() {
        let trigger_axis = match button {
            Button::LeftTrigger => Some(Axis::LeftTrigger),
            Button::RightTrigger => Some(Axis::RightTrigger),
            _ => None,
        };
        // Analog triggers are pulled with their axis below.
        if trigger_axis.is_some_and(|axis| capabilities.axes.contains(axis)) {
            continue;
        }
        let mut state = rest;
        state.set_pressed(button, true);
        states.push(state);
    }
    for axis in capabilities.axes.iter() {
        let ends: &[f32] = if axis.is_trigger() {
            &[1.0]
        } else {
            &[1.0, -1.0]
        };
        for &value in ends {
            let mut state = rest;
            state.set_axis(axis, value);
            if axis == Axis::LeftTrigger && capabilities.buttons.contains(Button::LeftTrigger) {
                state.set_pressed(Button::LeftTrigger, true);
            }
            if axis == Axis::RightTrigger && capabilities.buttons.contains(Button::RightTrigger) {
                state.set_pressed(Button::RightTrigger, true);
            }
            states.push(state);
        }
    }
    states.push(rest);
    states
}

/// Whether `observed` shows `expected` on the controls in `capabilities`.
pub fn matches(
    expected: &GamepadState,
    observed: &GamepadState,
    capabilities: &Capabilities,
) -> bool {
    let buttons = capabilities.buttons;
    (expected.buttons & buttons) == (observed.buttons & buttons)
        && capabilities
            .axes
            .iter()
            .all(|axis| (expected.axis(axis) - observed.axis(axis)).abs() <= AXIS_TOLERANCE)
}

/// A state in canonical names: the buttons pressed and the axes pushed
/// (`south`, `left_stick_x+`, `left_trigger`), or `nothing`.
pub fn describe(state: &GamepadState, capabilities: &Capabilities) -> String {
    let mut parts: Vec<String> = (state.buttons & capabilities.buttons)
        .iter()
        .map(|button| button.as_str().to_owned())
        .collect();
    for axis in capabilities.axes.iter() {
        let value = state.axis(axis);
        if axis.is_trigger() {
            if value >= 0.5 {
                parts.push(format!("{}_axis", axis.as_str()));
            }
        } else if value.abs() >= 0.5 {
            let sign = if value > 0.0 { '+' } else { '-' };
            parts.push(format!("{}{sign}", axis.as_str()));
        }
    }
    if parts.is_empty() {
        "nothing".into()
    } else {
        parts.join("+")
    }
}

/// How the app reads a device, for [`Checker::tick`].
pub struct Seen<'a> {
    /// The provider's id.
    pub id: DeviceId,
    /// Its identity and layout.
    pub info: &'a DeviceInfo,
    /// Its canonical state, or why it cannot be read.
    pub state: Result<&'a GamepadState, &'a str>,
}

/// The check of one virtual pad, without I/O (see the [module
/// documentation](self)).
#[derive(Clone, Debug)]
pub struct Checker {
    vendor_id: u16,
    product_id: u16,
    capabilities: Capabilities,
    /// Devices that were there before the pad was created.
    known: BTreeSet<DeviceId>,
    device: Option<DeviceId>,
    created: f64,
    script: Vec<GamepadState>,
    index: usize,
    shown_at: f64,
    latencies: Vec<f64>,
    done: bool,
}

impl Checker {
    /// The check of a pad of `target` created at `now`, with `known` the
    /// devices the app saw before.
    pub fn new(target: Target, known: BTreeSet<DeviceId>, now: f64) -> Self {
        let identity = target.identity();
        let capabilities = target.create(0).capabilities();
        Self {
            vendor_id: identity.vendor_id,
            product_id: identity.product_id,
            script: script(&capabilities),
            capabilities,
            known,
            device: None,
            created: now,
            index: 0,
            shown_at: now,
            latencies: Vec::new(),
            done: false,
        }
    }

    /// Whether `info` is a device of this check's pad (a new device with
    /// its vendor and product IDs).
    pub fn is_ours(&self, id: DeviceId, info: &DeviceInfo) -> bool {
        !self.known.contains(&id)
            && info.identity.vendor_id == Some(self.vendor_id)
            && info.identity.product_id == Some(self.product_id)
    }

    /// The device the app reads the pad through, once found.
    pub fn device(&self) -> Option<DeviceId> {
        self.device
    }

    /// The state to show on the pad now; `None` once the check ended.
    pub fn showing(&self) -> Option<&GamepadState> {
        (!self.done).then(|| &self.script[self.index.min(self.script.len() - 1)])
    }

    /// Reads back at `now` what the app sees of `devices`; returns the
    /// outcome once the check ends.
    pub fn tick<'a>(
        &mut self,
        devices: impl IntoIterator<Item = Seen<'a>>,
        now: f64,
    ) -> Option<VirtualCheck> {
        if self.done {
            return None;
        }
        let outcome = self.step(devices, now);
        self.done = outcome.is_some();
        outcome
    }

    fn step<'a>(
        &mut self,
        devices: impl IntoIterator<Item = Seen<'a>>,
        now: f64,
    ) -> Option<VirtualCheck> {
        let devices: Vec<Seen<'a>> = devices.into_iter().collect();
        if self.device.is_none() {
            // The pad's main device: the one with the most buttons (PlayStation
            // pads show their motion sensors and touchpad apart).
            self.device = devices
                .iter()
                .filter(|seen| self.is_ours(seen.id, seen.info))
                .max_by_key(|seen| seen.info.layout.buttons.len())
                .map(|seen| seen.id);
            if self.device.is_none() {
                return (now - self.created > APPEAR_MS)
                    .then(|| VirtualCheck::Failed("the virtual pad did not appear".into()));
            }
            self.shown_at = now;
        }
        let Some(seen) = devices.iter().find(|seen| Some(seen.id) == self.device) else {
            return Some(VirtualCheck::Failed("the virtual pad went away".into()));
        };
        let observed = match seen.state {
            Ok(state) => state,
            Err(error) => {
                return Some(VirtualCheck::Failed(format!(
                    "the app cannot read the virtual pad: {error}"
                )));
            }
        };
        let expected = &self.script[self.index];
        if matches(expected, observed, &self.capabilities) {
            self.latencies.push(now - self.shown_at);
            self.index += 1;
            self.shown_at = now;
            if self.index == self.script.len() {
                let states = self.script.len() as u32;
                let latency_ms = self.latencies.iter().sum::<f64>() / self.latencies.len() as f64;
                return Some(VirtualCheck::Matched {
                    states,
                    latency_ms: Some(latency_ms),
                });
            }
        } else if now - self.shown_at > SETTLE_MS {
            return Some(VirtualCheck::Differs {
                states: self.index as u32,
                expected: describe(expected, &self.capabilities),
                observed: describe(observed, &self.capabilities),
            });
        }
        None
    }
}

/// A virtual pad of the test mode on Linux, with its check.
#[cfg(target_os = "linux")]
pub struct VirtualRun {
    /// The target's name in step IDs.
    pub name: &'static str,
    pad: anygamepad_virtual::linux::LinuxVirtualPad,
    /// The check, until it ended.
    pub checker: Checker,
}

#[cfg(target_os = "linux")]
impl VirtualRun {
    /// Creates the virtual pad named `name` at `now`; `known` are the
    /// devices the app saw before.
    pub fn create(name: &'static str, known: BTreeSet<DeviceId>, now: f64) -> Result<Self, String> {
        let target = target_named(name).ok_or_else(|| format!("no virtual pad `{name}`"))?;
        let pad = anygamepad_virtual::linux::LinuxVirtualPad::create(target, 0)
            .map_err(|e| format!("cannot create the virtual pad: {e}"))?;
        log::info!("virtual {name} created for a test");
        Ok(Self {
            name,
            pad,
            checker: Checker::new(target, known, now),
        })
    }

    /// Shows the check's state, else `mirror` (the pad under test), and
    /// answers the host; returns the output commands a game sent.
    pub fn update(
        &mut self,
        mirror: &GamepadState,
    ) -> std::io::Result<Vec<anygamepad::model::OutputCommand>> {
        let state = *self.checker.showing().unwrap_or(mirror);
        self.pad.update(&state)
    }
}

#[cfg(test)]
mod tests {
    use anygamepad::model::identity::{Backend, DeviceIdentity, Os, ProviderKind};
    use anygamepad::model::raw::RawLayout;
    use anygamepad::model::{OutputSet, Transport};
    use anygamepad::provider::DeviceLocation;

    use super::*;

    fn info(vendor: u16, product: u16, buttons: usize) -> DeviceInfo {
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux);
        identity.vendor_id = Some(vendor);
        identity.product_id = Some(product);
        identity.transport = Transport::Usb;
        DeviceInfo {
            identity,
            location: DeviceLocation::default(),
            layout: RawLayout {
                buttons: vec![anygamepad::model::raw::RawSource::Index(0); buttons],
                ..RawLayout::default()
            },
            outputs: OutputSet::EMPTY,
        }
    }

    #[test]
    fn targets_and_names() {
        let targets = targets();
        assert_eq!(targets.len(), Target::ALL.len());
        for target in &targets {
            assert_eq!(name(target_named(target.name).unwrap()), target.name);
            // The Switch Pro Controller's HD rumble is not emulated yet.
            assert_eq!(
                target.rumble,
                target.name != "switch-pro",
                "{}",
                target.name
            );
            // Names are step ID words.
            assert!(
                target
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
        }
        #[cfg(not(target_os = "linux"))]
        assert!(targets.iter().all(|t| t.unavailable.is_some()));
    }

    #[test]
    fn scripts_cover_every_control() {
        let capabilities = Target::DualSense.create(0).capabilities();
        let states = script(&capabilities);
        assert_eq!(states.first(), Some(&GamepadState::new()));
        assert_eq!(states.last(), Some(&GamepadState::new()));
        let described: Vec<String> = states.iter().map(|s| describe(s, &capabilities)).collect();
        for expected in [
            "south",
            "touchpad_click",
            "left_stick_x+",
            "left_stick_y-",
            "left_trigger+left_trigger_axis",
        ] {
            assert!(
                described.iter().any(|d| d == expected),
                "{expected} in {described:?}"
            );
        }
        // Analog triggers come with their axis, not alone.
        assert!(!described.iter().any(|d| d == "left_trigger"));
    }

    /// A loopback that reads back what is shown after `delay` ticks,
    /// through `read`.
    fn run(read: impl Fn(&GamepadState) -> GamepadState, appear: bool) -> VirtualCheck {
        let target = Target::Xbox360;
        let identity = target.identity();
        let other = info(0x054c, 0x0ce6, 17);
        let ours = info(identity.vendor_id, identity.product_id, 15);
        let mut checker = Checker::new(target, BTreeSet::from([DeviceId(1)]), 0.0);
        let mut shown = GamepadState::new();
        for tick in 1..2000 {
            let now = f64::from(tick) * 4.0;
            let state = read(&shown);
            let mut devices = vec![Seen {
                id: DeviceId(1),
                info: &other,
                state: Ok(&state),
            }];
            if appear && tick > 50 {
                devices.push(Seen {
                    id: DeviceId(2),
                    info: &ours,
                    state: Ok(&state),
                });
            }
            if let Some(outcome) = checker.tick(devices, now) {
                assert!(checker.showing().is_none());
                return outcome;
            }
            shown = *checker.showing().unwrap();
        }
        panic!("the check never ended");
    }

    #[test]
    fn a_pad_that_reads_back() {
        let outcome = run(|shown| *shown, true);
        let VirtualCheck::Matched { states, latency_ms } = outcome else {
            panic!("{outcome:?}");
        };
        let capabilities = Target::Xbox360.create(0).capabilities();
        assert_eq!(states as usize, script(&capabilities).len());
        assert!(latency_ms.unwrap() >= 0.0);
    }

    #[test]
    fn a_pad_that_swaps_buttons() {
        let outcome = run(
            |shown| {
                let mut state = *shown;
                state.set_pressed(Button::South, shown.pressed(Button::East));
                state.set_pressed(Button::East, shown.pressed(Button::South));
                state
            },
            true,
        );
        assert_eq!(
            outcome,
            VirtualCheck::Differs {
                states: 1,
                expected: "south".into(),
                observed: "east".into(),
            }
        );
    }

    #[test]
    fn a_pad_that_does_not_appear() {
        assert_eq!(
            run(|shown| *shown, false),
            VirtualCheck::Failed("the virtual pad did not appear".into())
        );
    }
}
