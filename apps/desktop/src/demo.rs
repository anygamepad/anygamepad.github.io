//! A provider with one made-up pad that moves by itself, to try the app
//! without a controller (`ANYGAMEPAD_DEMO=1`).
//!
//! In a test session (H5) the pad follows the session's prompts as a
//! person would ([`Autopilot`]): it presses the button asked for, turns
//! the sticks, pulls the triggers, answers yes from the pad, and goes away
//! and comes back when asked to.

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anygamepad::model::identity::{Backend, DeviceIdentity, ProviderKind};
use anygamepad::model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad::model::{
    Axis, Button, Output, OutputCommand, OutputSet, Power, PowerState, Transport,
};
use anygamepad::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};
use anygamepad::testing::Prompt;

const PAD: DeviceId = DeviceId(1);

/// Buttons of the demo pad, all canonical.
const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftStick,
    Button::RightStick,
    Button::Select,
    Button::Start,
    Button::Home,
];

/// Makes the demo pad follow a test session's prompts; shared by the
/// provider and the device worker.
#[derive(Clone, Debug, Default)]
pub struct Autopilot(Arc<Mutex<Option<(Prompt, Instant)>>>);

impl Autopilot {
    /// Follows `prompt`, from now if it is a new one.
    pub fn follow(&self, prompt: Prompt) {
        if let Ok(mut current) = self.0.lock() {
            if current.as_ref().is_none_or(|(p, _)| *p != prompt) {
                *current = Some((prompt, Instant::now()));
            }
        }
    }

    /// Lets the pad move by itself again.
    pub fn release(&self) {
        if let Ok(mut current) = self.0.lock() {
            *current = None;
        }
    }

    /// The prompt followed, and for how long, in milliseconds.
    fn current(&self) -> Option<(Prompt, u32)> {
        let current = *self.0.lock().ok()?;
        current.map(|(prompt, since)| {
            let elapsed = since.elapsed().as_millis();
            (prompt, u32::try_from(elapsed).unwrap_or(u32::MAX))
        })
    }
}

/// The demo provider.
pub struct DemoProvider {
    info: DeviceInfo,
    start: Instant,
    reported: bool,
    open: bool,
    /// Whether the pad is connected (the autopilot unplugs it).
    present: bool,
    autopilot: Autopilot,
}

impl DemoProvider {
    /// A demo provider whose pad follows `autopilot` in test sessions.
    pub fn with_autopilot(autopilot: Autopilot) -> Self {
        Self {
            autopilot,
            ..Self::default()
        }
    }
}

impl Default for DemoProvider {
    fn default() -> Self {
        let mut identity = DeviceIdentity::new(
            ProviderKind::Native,
            Backend::Unknown,
            crate::collection::host_os(),
        );
        identity.product_name = Some("Demo pad".into());
        identity.transport = Transport::Usb;
        let layout = RawLayout {
            buttons: BUTTONS.iter().map(|&b| RawSource::Button(b)).collect(),
            axes: Axis::ALL
                .iter()
                .map(|&axis| RawAxisInfo {
                    source: RawSource::Axis(axis),
                    min: axis.min(),
                    max: 1.0,
                })
                .collect(),
            battery: true,
            ..RawLayout::default()
        };
        Self {
            info: DeviceInfo {
                identity,
                location: DeviceLocation::default(),
                layout,
                outputs: OutputSet::from([Output::Rumble]),
            },
            start: Instant::now(),
            reported: false,
            open: false,
            present: true,
            autopilot: Autopilot::default(),
        }
    }
}

impl Provider for DemoProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        if !self.reported {
            self.reported = true;
            events.push(ProviderEvent::Added(PAD));
        }
        match self.autopilot.current() {
            Some((Prompt::Unplug, t)) if t >= 300 && self.present => {
                self.present = false;
                self.open = false;
                events.push(ProviderEvent::Removed(PAD));
            }
            Some((Prompt::Reconnect, t)) if t >= 300 && !self.present => {
                self.present = true;
                events.push(ProviderEvent::Added(PAD));
            }
            _ => {}
        }
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        (device == PAD).then_some(&self.info)
    }

    fn open(&mut self, device: DeviceId, _access: Access) -> Result<(), ProviderError> {
        if device != PAD {
            return Err(ProviderError::UnknownDevice);
        }
        if !self.present {
            return Err(ProviderError::Disconnected);
        }
        self.open = true;
        Ok(())
    }

    fn close(&mut self, _device: DeviceId) {
        self.open = false;
    }

    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
        if device != PAD {
            return Err(ProviderError::UnknownDevice);
        }
        if !self.present {
            return Err(ProviderError::Disconnected);
        }
        if !self.open {
            return Err(ProviderError::NotOpen);
        }
        state.power = Power {
            state: PowerState::OnBattery,
            percent: Some(80),
        };
        if let Some((prompt, t)) = self.autopilot.current() {
            follow(prompt, t, state);
            return Ok(());
        }
        // Sticks circle, triggers breathe, one button at a time, at a
        // pace a person can follow (20 updates a second).
        let t = (self.start.elapsed().as_millis() / 50) as f32 / 20.0;
        let (sin, cos) = (t * 1.5).sin_cos();
        state.axes.copy_from_slice(&[
            cos,
            sin,
            -sin * 0.6,
            cos * 0.6,
            (t.sin() + 1.0) / 2.0,
            (t.cos() + 1.0) / 2.0,
        ]);
        let pressed = (t as usize) % BUTTONS.len();
        for (i, value) in state.buttons.iter_mut().enumerate() {
            *value = if i == pressed { 1.0 } else { 0.0 };
        }
        Ok(())
    }

    fn send(&mut self, _device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        log::info!("demo pad output: {command:?}");
        Ok(())
    }
}

/// The demo pad `t` milliseconds into `prompt`, as a quick and patient
/// person would hold it: each action repeats until the session moves on.
fn follow(prompt: Prompt, t: u32, state: &mut RawState) {
    state.buttons.fill(0.0);
    state.axes.fill(0.0);
    let axis = |axis: Axis| {
        Axis::ALL
            .iter()
            .position(|&a| a == axis)
            .expect("every axis")
    };
    let mut press = |button: Button| {
        if let Some(index) = BUTTONS.iter().position(|&b| b == button) {
            state.buttons[index] = 1.0;
        }
    };
    match prompt {
        Prompt::Press(button) if (30..130).contains(&(t % 600)) => {
            if BUTTONS.contains(&button) {
                press(button);
            } else {
                // South and east together: "not on this pad".
                press(Button::South);
                press(Button::East);
            }
        }
        Prompt::Ask(_) if (300..450).contains(&(t % 1500)) => press(Button::South),
        Prompt::Circle(side) => {
            let t = t % 2000;
            if (100..1300).contains(&t) {
                let angle = (t - 100) as f32 / 1000.0 * TAU;
                let (x, y) = match side {
                    anygamepad::db::plan::Side::Left => (Axis::LeftStickX, Axis::LeftStickY),
                    _ => (Axis::RightStickX, Axis::RightStickY),
                };
                state.axes[axis(x)] = angle.cos();
                state.axes[axis(y)] = angle.sin();
            }
        }
        Prompt::Pull(side) => {
            let t = t % 1500;
            let value = match t {
                100..500 => (t - 100) as f32 / 400.0,
                500..700 => 1.0,
                _ => 0.0,
            };
            let trigger = match side {
                anygamepad::db::plan::Side::Left => Axis::LeftTrigger,
                _ => Axis::RightTrigger,
            };
            state.axes[axis(trigger)] = value;
        }
        Prompt::MoveSticks => {
            let angle = t as f32 / 1000.0 * TAU;
            state.axes[axis(Axis::LeftStickX)] = angle.cos();
            state.axes[axis(Axis::LeftStickY)] = angle.sin();
            state.axes[axis(Axis::RightStickX)] = -angle.sin();
            state.axes[axis(Axis::RightStickY)] = angle.cos();
        }
        _ => {}
    }
}
