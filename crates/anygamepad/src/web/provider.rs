//! [`WebProvider`]: the Gamepad API behind the [`Provider`] trait.

use anygamepad_model::identity::{Browser, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad_model::{Output, OutputCommand, OutputSet, Power, Rumble, TriggerRumble};

use crate::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};

/// One button as the Gamepad API reports it (`GamepadButton`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonSnapshot {
    /// `pressed`, as the browser decides it.
    pub pressed: bool,
    /// `touched` (capacitive buttons; equal to `pressed` elsewhere).
    pub touched: bool,
    /// `value`, in `[0.0, 1.0]`.
    pub value: f32,
}

/// One gamepad as the Gamepad API reports it, verbatim
/// (`navigator.getGamepads()[slot]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadSnapshot {
    /// Changes each time a gamepad leaves this slot, so that a reconnection
    /// between two reads is a new device even when nothing else changes.
    pub connection: u32,
    /// `Gamepad.id`.
    pub id: String,
    /// `Gamepad.mapping`: `"standard"` when the browser maps the device to
    /// the [standard layout](super::standard), `""` otherwise.
    pub mapping: String,
    /// `Gamepad.timestamp`: when the browser last received data from the
    /// device, in milliseconds on the `performance.now()` clock.
    pub timestamp: f64,
    /// `Gamepad.buttons`.
    pub buttons: Vec<ButtonSnapshot>,
    /// `Gamepad.axes`, in `[-1.0, 1.0]`.
    pub axes: Vec<f32>,
    /// Effects `Gamepad.vibrationActuator` supports: [`Output::Rumble`] for
    /// `"dual-rumble"`, [`Output::TriggerRumble`] for `"trigger-rumble"`.
    pub haptics: OutputSet,
}

/// Type of a haptic effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HapticKind {
    /// `"dual-rumble"`: the two body motors.
    DualRumble,
    /// `"trigger-rumble"`: the trigger motors, and the body motors with them.
    TriggerRumble,
}

impl HapticKind {
    /// Name of the effect type in the Gamepad API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DualRumble => "dual-rumble",
            Self::TriggerRumble => "trigger-rumble",
        }
    }
}

/// A haptic effect for `vibrationActuator.playEffect()`. Magnitudes are in
/// `[0.0, 1.0]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HapticEffect {
    /// Type of the effect.
    pub kind: HapticKind,
    /// Duration in milliseconds.
    pub duration_ms: f64,
    /// Low-frequency (strong) body motor.
    pub strong_magnitude: f32,
    /// High-frequency (weak) body motor.
    pub weak_magnitude: f32,
    /// Left trigger motor ([`HapticKind::TriggerRumble`] only).
    pub left_trigger: f32,
    /// Right trigger motor ([`HapticKind::TriggerRumble`] only).
    pub right_trigger: f32,
}

/// Where a [`WebProvider`] reads gamepads from: the browser's Gamepad API,
/// or a test double.
pub trait GamepadSource {
    /// Reads `navigator.getGamepads()` into `slots`: one entry per slot,
    /// `None` for empty slots. Entries are updated in place, so that reading
    /// does not allocate in the frame loop.
    fn read(&mut self, slots: &mut Vec<Option<GamepadSnapshot>>) -> Result<(), ProviderError>;

    /// Current time in milliseconds, on the clock of
    /// [`GamepadSnapshot::timestamp`].
    fn now(&self) -> f64;

    /// Plays `effect` on the gamepad in `slot`, replacing the one playing.
    /// Failures are ignored: haptics are best effort.
    fn play_effect(&mut self, slot: usize, effect: HapticEffect);

    /// Stops the effects of the gamepad in `slot`.
    fn stop_effects(&mut self, slot: usize);
}

/// Browsers cap effects at 5 seconds (Chromium's maximum), so continuous
/// rumble is played in 5 second effects...
const EFFECT_DURATION_MS: f64 = 5000.0;

/// ... replayed before they end. If the frame loop stops (a hidden tab), the
/// rumble stops within 5 seconds instead of running on.
const EFFECT_REFRESH_MS: f64 = 4000.0;

/// The browser's gamepads as a [`Provider`].
///
/// Call [`Provider::update`] once per frame, from `requestAnimationFrame`:
/// the Gamepad API is poll-based. Browsers only expose a gamepad once the
/// user presses one of its buttons.
///
/// Raw elements are the Gamepad API's own lists ([`RawSource::Index`]): each
/// button's `value` and each axis, verbatim. For devices in the standard
/// layout, [`super::standard::map`] gives the canonical state.
///
/// Rumble ([`OutputCommand::Rumble`], [`OutputCommand::TriggerRumble`]) goes
/// through `vibrationActuator` where the browser has it (Chromium, Safari).
/// Browsers cannot hide a gamepad from other pages, so
/// [`Access::Exclusive`] is not supported.
pub struct WebProvider {
    source: Box<dyn GamepadSource>,
    browser: Browser,
    os: Os,
    /// The latest read, by slot.
    slots: Vec<Option<GamepadSnapshot>>,
    /// The device in each slot, in step with `slots` after each update.
    devices: Vec<Option<Device>>,
    next_id: u64,
}

struct Device {
    id: DeviceId,
    connection: u32,
    info: DeviceInfo,
    access: Option<Access>,
    rumble: Rumble,
    trigger_rumble: TriggerRumble,
    /// When the effect playing was started, if any.
    effect_started: Option<f64>,
}

impl Device {
    /// Whether `snapshot` is still this device, with the same layout.
    fn matches(&self, snapshot: &GamepadSnapshot) -> bool {
        let web = self.info.identity.web.as_ref();
        snapshot.connection == self.connection
            && web.is_some_and(|web| web.id == snapshot.id && web.mapping == snapshot.mapping)
            && snapshot.buttons.len() == self.info.layout.buttons.len()
            && snapshot.axes.len() == self.info.layout.axes.len()
            && snapshot.haptics == self.info.outputs
    }

    /// The effect that plays the current motor strengths, if any is not 0.
    fn effect(&self) -> Option<HapticEffect> {
        let strong_magnitude = magnitude(self.rumble.low_frequency);
        let weak_magnitude = magnitude(self.rumble.high_frequency);
        let left_trigger = magnitude(self.trigger_rumble.left);
        let right_trigger = magnitude(self.trigger_rumble.right);
        let kind = if left_trigger > 0.0 || right_trigger > 0.0 {
            HapticKind::TriggerRumble
        } else if strong_magnitude > 0.0 || weak_magnitude > 0.0 {
            HapticKind::DualRumble
        } else {
            return None;
        };
        Some(HapticEffect {
            kind,
            duration_ms: EFFECT_DURATION_MS,
            strong_magnitude,
            weak_magnitude,
            left_trigger,
            right_trigger,
        })
    }
}

/// A motor strength the Gamepad API accepts: in `[0.0, 1.0]`, NaN as 0.
fn magnitude(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

impl WebProvider {
    /// A provider reading `source`, in `browser` on `os` (see
    /// [`super::detect`]).
    pub fn with_source(source: Box<dyn GamepadSource>, browser: Browser, os: Os) -> Self {
        Self {
            source,
            browser,
            os,
            slots: Vec::new(),
            devices: Vec::new(),
            next_id: 1,
        }
    }

    /// The browser, as detected.
    pub fn browser(&self) -> Browser {
        self.browser
    }

    /// The OS, as the browser tells it.
    pub fn os(&self) -> Os {
        self.os
    }

    /// The devices connected at the last [`Provider::update`].
    pub fn devices(&self) -> impl Iterator<Item = DeviceId> + '_ {
        self.devices.iter().flatten().map(|device| device.id)
    }

    /// What the Gamepad API reported for a device at the last
    /// [`Provider::update`], verbatim (including `pressed`, `touched` and
    /// the timestamp, which [`RawState`] has no room for). The device does
    /// not need to be open.
    pub fn snapshot(&self, device: DeviceId) -> Option<&GamepadSnapshot> {
        let slot = self.slot(device)?;
        self.slots.get(slot)?.as_ref()
    }

    fn slot(&self, id: DeviceId) -> Option<usize> {
        self.devices
            .iter()
            .position(|device| device.as_ref().is_some_and(|device| device.id == id))
    }

    fn device_info(&self, snapshot: &GamepadSnapshot) -> DeviceInfo {
        let index = |i: usize| RawSource::Index(u16::try_from(i).unwrap_or(u16::MAX));
        DeviceInfo {
            identity: DeviceIdentity::from_gamepad_api(
                &snapshot.id,
                &snapshot.mapping,
                self.browser,
                self.os,
            ),
            location: DeviceLocation::default(),
            layout: RawLayout {
                buttons: (0..snapshot.buttons.len()).map(index).collect(),
                axes: (0..snapshot.axes.len())
                    .map(|i| RawAxisInfo {
                        source: index(i),
                        min: -1.0,
                        max: 1.0,
                    })
                    .collect(),
                ..RawLayout::default()
            },
            outputs: snapshot.haptics,
        }
    }

    /// Plays, or stops, the effect for the current motor strengths.
    fn apply_effect(source: &mut dyn GamepadSource, slot: usize, device: &mut Device) {
        match device.effect() {
            Some(effect) => {
                source.play_effect(slot, effect);
                device.effect_started = Some(source.now());
            }
            None => {
                if device.effect_started.take().is_some() {
                    source.stop_effects(slot);
                }
            }
        }
    }

    /// Stops a device's effects and forgets its motor strengths.
    fn stop(source: &mut dyn GamepadSource, slot: usize, device: &mut Device) {
        device.rumble = Rumble::default();
        device.trigger_rumble = TriggerRumble::default();
        Self::apply_effect(source, slot, device);
    }

    /// The slot of device `id`, and its access mode if it is open.
    fn open_slot(&self, id: DeviceId) -> Result<(usize, Access), ProviderError> {
        let slot = self.slot(id).ok_or(ProviderError::UnknownDevice)?;
        let access = self.devices[slot]
            .as_ref()
            .and_then(|device| device.access)
            .ok_or(ProviderError::NotOpen)?;
        Ok((slot, access))
    }
}

impl Provider for WebProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Web
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        self.source.read(&mut self.slots)?;
        let slots = self.slots.len().max(self.devices.len());
        self.devices.resize_with(slots, || None);
        for slot in 0..slots {
            let snapshot = self.slots.get(slot).and_then(Option::as_ref);
            if let Some(device) = &mut self.devices[slot] {
                if snapshot.is_some_and(|snapshot| device.matches(snapshot)) {
                    continue;
                }
                // Gone, or replaced: a reconnection or a new layout.
                if snapshot.is_some() {
                    Self::stop(self.source.as_mut(), slot, device);
                }
                events.push(ProviderEvent::Removed(device.id));
                self.devices[slot] = None;
            }
            if let Some(snapshot) = snapshot {
                let id = DeviceId(self.next_id);
                self.next_id += 1;
                self.devices[slot] = Some(Device {
                    id,
                    connection: snapshot.connection,
                    info: self.device_info(snapshot),
                    access: None,
                    rumble: Rumble::default(),
                    trigger_rumble: TriggerRumble::default(),
                    effect_started: None,
                });
                events.push(ProviderEvent::Added(id));
            }
        }
        self.devices.truncate(self.slots.len());

        let now = self.source.now();
        for (slot, device) in self.devices.iter_mut().enumerate() {
            let Some(device) = device else { continue };
            if device
                .effect_started
                .is_some_and(|started| now - started >= EFFECT_REFRESH_MS)
            {
                Self::apply_effect(self.source.as_mut(), slot, device);
            }
        }
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        let slot = self.slot(device)?;
        self.devices[slot].as_ref().map(|device| &device.info)
    }

    fn open(&mut self, id: DeviceId, access: Access) -> Result<(), ProviderError> {
        let slot = self.slot(id).ok_or(ProviderError::UnknownDevice)?;
        let Some(device) = self.devices[slot].as_mut() else {
            return Err(ProviderError::UnknownDevice);
        };
        match access {
            Access::Shared => {}
            Access::Exclusive => return Err(ProviderError::Unsupported),
            Access::ObserveOnly => Self::stop(self.source.as_mut(), slot, device),
        }
        device.access = Some(access);
        Ok(())
    }

    fn close(&mut self, id: DeviceId) {
        let Some(slot) = self.slot(id) else { return };
        if let Some(device) = self.devices[slot].as_mut() {
            Self::stop(self.source.as_mut(), slot, device);
            device.access = None;
        }
    }

    fn poll(&mut self, id: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
        let (slot, _) = self.open_slot(id)?;
        let layout = &self.devices[slot]
            .as_ref()
            .expect("open device")
            .info
            .layout;
        if !state.fits(layout) {
            return Err(ProviderError::Backend(
                "raw state does not fit the device layout".into(),
            ));
        }
        let snapshot = self
            .slots
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(ProviderError::Disconnected)?;
        for (value, button) in state.buttons.iter_mut().zip(&snapshot.buttons) {
            *value = button.value;
        }
        for (value, axis) in state.axes.iter_mut().zip(&snapshot.axes) {
            *value = *axis;
        }
        state.motion.clear();
        state.touches = Default::default();
        state.power = Power::default();
        Ok(())
    }

    fn send(&mut self, id: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        let (slot, access) = self.open_slot(id)?;
        if access == Access::ObserveOnly {
            return Err(ProviderError::Unsupported);
        }
        let device = self.devices[slot].as_mut().expect("open device");
        let outputs = device.info.outputs;
        match command {
            OutputCommand::Rumble(rumble) if outputs.contains(Output::Rumble) => {
                device.rumble = rumble;
            }
            OutputCommand::TriggerRumble(rumble) if outputs.contains(Output::TriggerRumble) => {
                device.trigger_rumble = rumble;
            }
            _ => return Err(ProviderError::Unsupported),
        }
        Self::apply_effect(self.source.as_mut(), slot, device);
        Ok(())
    }
}

impl Drop for WebProvider {
    fn drop(&mut self) {
        for (slot, device) in self.devices.iter_mut().enumerate() {
            if let Some(device) = device {
                Self::stop(self.source.as_mut(), slot, device);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use anygamepad_model::identity::{Backend, BrowserEngine};

    use super::*;

    /// What the fake browser shows, shared with the test.
    #[derive(Default)]
    struct FakeBrowser {
        slots: Vec<Option<GamepadSnapshot>>,
        now: f64,
        played: Vec<(usize, HapticEffect)>,
        stopped: Vec<usize>,
        fail: bool,
    }

    #[derive(Clone, Default)]
    struct FakeSource(Rc<RefCell<FakeBrowser>>);

    impl GamepadSource for FakeSource {
        fn read(&mut self, slots: &mut Vec<Option<GamepadSnapshot>>) -> Result<(), ProviderError> {
            let browser = self.0.borrow();
            if browser.fail {
                return Err(ProviderError::PermissionDenied);
            }
            slots.clone_from(&browser.slots);
            Ok(())
        }

        fn now(&self) -> f64 {
            self.0.borrow().now
        }

        fn play_effect(&mut self, slot: usize, effect: HapticEffect) {
            self.0.borrow_mut().played.push((slot, effect));
        }

        fn stop_effects(&mut self, slot: usize) {
            self.0.borrow_mut().stopped.push(slot);
        }
    }

    const DUALSHOCK_ID: &str = "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)";

    fn dualshock(connection: u32) -> GamepadSnapshot {
        GamepadSnapshot {
            connection,
            id: DUALSHOCK_ID.into(),
            mapping: "standard".into(),
            timestamp: 10.0,
            buttons: vec![ButtonSnapshot::default(); 18],
            axes: vec![0.0; 4],
            haptics: OutputSet::from([Output::Rumble]),
        }
    }

    fn provider() -> (WebProvider, FakeSource) {
        let source = FakeSource::default();
        let browser = Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(141),
        };
        let provider = WebProvider::with_source(Box::new(source.clone()), browser, Os::Windows);
        (provider, source)
    }

    fn update(provider: &mut WebProvider) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        events
    }

    fn rumble(low_frequency: f32, high_frequency: f32) -> OutputCommand {
        OutputCommand::Rumble(Rumble {
            low_frequency,
            high_frequency,
        })
    }

    #[test]
    fn devices_come_and_go() {
        let (mut provider, source) = provider();
        assert_eq!(provider.kind(), ProviderKind::Web);
        assert!(update(&mut provider).is_empty());

        source.0.borrow_mut().slots = vec![None, Some(dualshock(0)), None, None];
        let events = update(&mut provider);
        assert_eq!(events, [ProviderEvent::Added(DeviceId(1))]);
        assert!(update(&mut provider).is_empty());
        assert_eq!(provider.devices().collect::<Vec<_>>(), [DeviceId(1)]);

        let info = provider.info(DeviceId(1)).unwrap();
        assert_eq!(info.identity.provider, ProviderKind::Web);
        assert_eq!(info.identity.backend, Backend::GamepadApi);
        assert_eq!(info.identity.os, Os::Windows);
        assert_eq!(info.identity.vendor_id, Some(0x054c));
        assert_eq!(info.identity.web.as_ref().unwrap().mapping, "standard");
        assert_eq!(info.layout.buttons.len(), 18);
        assert_eq!(info.layout.buttons[17], RawSource::Index(17));
        assert_eq!(info.layout.axes[3].source, RawSource::Index(3));
        assert_eq!(
            (info.layout.axes[0].min, info.layout.axes[0].max),
            (-1.0, 1.0)
        );
        assert_eq!(info.outputs, OutputSet::from([Output::Rumble]));
        assert_eq!(provider.snapshot(DeviceId(1)).unwrap().timestamp, 10.0);

        source.0.borrow_mut().slots = vec![None, None, None, None];
        assert_eq!(update(&mut provider), [ProviderEvent::Removed(DeviceId(1))]);
        assert!(provider.info(DeviceId(1)).is_none());
        assert!(provider.snapshot(DeviceId(1)).is_none());
        assert_eq!(provider.devices().count(), 0);
    }

    #[test]
    fn a_reconnection_is_a_new_device() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().slots = vec![Some(dualshock(0))];
        update(&mut provider);

        // Same pad, unplugged and plugged in between two frames.
        source.0.borrow_mut().slots = vec![Some(dualshock(1))];
        assert_eq!(
            update(&mut provider),
            [
                ProviderEvent::Removed(DeviceId(1)),
                ProviderEvent::Added(DeviceId(2))
            ]
        );

        // Another pad in the same slot.
        let mut other = dualshock(1);
        other.id = "xinput".into();
        source.0.borrow_mut().slots = vec![Some(other)];
        assert_eq!(
            update(&mut provider),
            [
                ProviderEvent::Removed(DeviceId(2)),
                ProviderEvent::Added(DeviceId(3))
            ]
        );

        // A layout change is a new device too, so that states fit layouts.
        let mut fewer_buttons = dualshock(1);
        fewer_buttons.id = "xinput".into();
        fewer_buttons.buttons.truncate(17);
        source.0.borrow_mut().slots = vec![Some(fewer_buttons)];
        assert_eq!(
            update(&mut provider),
            [
                ProviderEvent::Removed(DeviceId(3)),
                ProviderEvent::Added(DeviceId(4))
            ]
        );
    }

    #[test]
    fn poll_copies_the_raw_values() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().slots = vec![Some(dualshock(0))];
        update(&mut provider);
        let pad = DeviceId(1);
        let layout = provider.info(pad).unwrap().layout.clone();
        let mut state = RawState::for_layout(&layout);
        assert_eq!(provider.poll(pad, &mut state), Err(ProviderError::NotOpen));
        assert_eq!(
            provider.poll(DeviceId(7), &mut state),
            Err(ProviderError::UnknownDevice)
        );

        provider.open(pad, Access::Shared).unwrap();
        {
            let mut browser = source.0.borrow_mut();
            let snapshot = browser.slots[0].as_mut().unwrap();
            snapshot.buttons[7] = ButtonSnapshot {
                pressed: true,
                touched: true,
                value: 0.5,
            };
            snapshot.axes[1] = -1.0;
        }
        update(&mut provider);
        provider.poll(pad, &mut state).unwrap();
        assert_eq!(state.buttons[7], 0.5);
        assert_eq!(state.axes, [0.0, -1.0, 0.0, 0.0]);
        assert!(provider.snapshot(pad).unwrap().buttons[7].pressed);

        let mut wrong = RawState::default();
        assert!(matches!(
            provider.poll(pad, &mut wrong),
            Err(ProviderError::Backend(_))
        ));

        provider.close(pad);
        assert_eq!(provider.poll(pad, &mut state), Err(ProviderError::NotOpen));
    }

    #[test]
    fn access_modes() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().slots = vec![Some(dualshock(0))];
        update(&mut provider);
        let pad = DeviceId(1);
        assert_eq!(
            provider.open(pad, Access::Exclusive),
            Err(ProviderError::Unsupported)
        );
        assert_eq!(
            provider.open(DeviceId(9), Access::Shared),
            Err(ProviderError::UnknownDevice)
        );
        provider.open(pad, Access::ObserveOnly).unwrap();
        assert_eq!(
            provider.send(pad, rumble(1.0, 0.0)),
            Err(ProviderError::Unsupported)
        );
        provider.open(pad, Access::Shared).unwrap();
        provider.send(pad, rumble(1.0, 0.0)).unwrap();

        // Switching to observe-only stops the rumble.
        provider.open(pad, Access::ObserveOnly).unwrap();
        assert_eq!(source.0.borrow().stopped, [0]);
    }

    #[test]
    fn rumble_is_played_refreshed_and_stopped() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().slots = vec![None, Some(dualshock(0))];
        update(&mut provider);
        let pad = DeviceId(1);
        assert_eq!(
            provider.send(pad, rumble(1.0, 0.5)),
            Err(ProviderError::NotOpen)
        );
        provider.open(pad, Access::Shared).unwrap();

        provider.send(pad, rumble(2.0, f32::NAN)).unwrap();
        let expected = HapticEffect {
            kind: HapticKind::DualRumble,
            duration_ms: EFFECT_DURATION_MS,
            strong_magnitude: 1.0,
            weak_magnitude: 0.0,
            left_trigger: 0.0,
            right_trigger: 0.0,
        };
        assert_eq!(source.0.borrow().played, [(1, expected)]);

        // Replayed before the browser ends it.
        source.0.borrow_mut().now = EFFECT_REFRESH_MS - 1.0;
        update(&mut provider);
        assert_eq!(source.0.borrow().played.len(), 1);
        source.0.borrow_mut().now = EFFECT_REFRESH_MS;
        update(&mut provider);
        assert_eq!(source.0.borrow().played.len(), 2);

        // Zero stops it, once.
        provider.send(pad, rumble(0.0, 0.0)).unwrap();
        provider.send(pad, rumble(0.0, 0.0)).unwrap();
        assert_eq!(source.0.borrow().stopped, [1]);
        source.0.borrow_mut().now = 3.0 * EFFECT_REFRESH_MS;
        update(&mut provider);
        assert_eq!(source.0.borrow().played.len(), 2);

        // Outputs the device lacks are refused.
        let trigger = OutputCommand::TriggerRumble(TriggerRumble {
            left: 1.0,
            right: 0.0,
        });
        assert_eq!(provider.send(pad, trigger), Err(ProviderError::Unsupported));
        let lightbar = OutputCommand::Lightbar(Default::default());
        assert_eq!(
            provider.send(pad, lightbar),
            Err(ProviderError::Unsupported)
        );
    }

    #[test]
    fn trigger_rumble_carries_the_body_motors() {
        let (mut provider, source) = provider();
        let mut pad = dualshock(0);
        pad.haptics = OutputSet::from([Output::Rumble, Output::TriggerRumble]);
        source.0.borrow_mut().slots = vec![Some(pad)];
        update(&mut provider);
        let pad = DeviceId(1);
        provider.open(pad, Access::Shared).unwrap();
        provider.send(pad, rumble(0.25, 0.5)).unwrap();
        let trigger = OutputCommand::TriggerRumble(TriggerRumble {
            left: 0.0,
            right: 0.75,
        });
        provider.send(pad, trigger).unwrap();
        let (_, effect) = *source.0.borrow().played.last().unwrap();
        assert_eq!(effect.kind, HapticKind::TriggerRumble);
        assert_eq!(effect.strong_magnitude, 0.25);
        assert_eq!(effect.weak_magnitude, 0.5);
        assert_eq!(effect.right_trigger, 0.75);
    }

    #[test]
    fn rumble_stops_on_close_replacement_and_drop() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().slots = vec![Some(dualshock(0)), Some(dualshock(0))];
        update(&mut provider);
        for pad in [DeviceId(1), DeviceId(2)] {
            provider.open(pad, Access::Shared).unwrap();
            provider.send(pad, rumble(1.0, 1.0)).unwrap();
        }
        provider.close(DeviceId(1));
        assert_eq!(source.0.borrow().stopped, [0]);

        // Reconnected: the old device's rumble is stopped.
        source.0.borrow_mut().slots[1] = Some(dualshock(1));
        update(&mut provider);
        assert_eq!(source.0.borrow().stopped, [0, 1]);

        provider.open(DeviceId(3), Access::Shared).unwrap();
        provider.send(DeviceId(3), rumble(1.0, 0.0)).unwrap();
        drop(provider);
        assert_eq!(source.0.borrow().stopped, [0, 1, 1]);
    }

    #[test]
    fn read_errors_are_reported() {
        let (mut provider, source) = provider();
        source.0.borrow_mut().fail = true;
        let mut events = Vec::new();
        assert_eq!(
            provider.update(&mut events),
            Err(ProviderError::PermissionDenied)
        );
        assert!(events.is_empty());
    }
}
