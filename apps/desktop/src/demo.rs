//! A provider with one made-up pad that moves by itself, to try the app
//! without a controller (`ANYGAMEPAD_DEMO=1`).

use std::time::Instant;

use anygamepad::model::identity::{Backend, DeviceIdentity, Os, ProviderKind};
use anygamepad::model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad::model::{
    Axis, Button, Output, OutputCommand, OutputSet, Power, PowerState, Transport,
};
use anygamepad::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};

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

/// The demo provider.
pub struct DemoProvider {
    info: DeviceInfo,
    start: Instant,
    reported: bool,
    open: bool,
}

impl Default for DemoProvider {
    fn default() -> Self {
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Unknown, Os::Unknown);
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
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        (device == PAD).then_some(&self.info)
    }

    fn open(&mut self, device: DeviceId, _access: Access) -> Result<(), ProviderError> {
        if device != PAD {
            return Err(ProviderError::UnknownDevice);
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
        if !self.open {
            return Err(ProviderError::NotOpen);
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
        state.power = Power {
            state: PowerState::OnBattery,
            percent: Some(80),
        };
        Ok(())
    }

    fn send(&mut self, _device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        log::info!("demo pad output: {command:?}");
        Ok(())
    }
}
