//! [`GameControllerProvider`]: the pads macOS's GameController drives.

use std::collections::BTreeMap;
use std::time::Instant;

use anygamepad_model::identity::ProviderKind;
use anygamepad_model::raw::RawState;
use anygamepad_model::{OutputCommand, Rgb};
use objc2::rc::{Retained, autoreleasepool};
use objc2::{AllocAnyThread, available};
use objc2_game_controller::{
    GCColor, GCController, GCControllerButtonInput, GCControllerDirectionPad,
    GCControllerPlayerIndex, GCDevice, GCDeviceBattery, GCDeviceLight, GCDualSenseGamepad,
    GCDualShockGamepad, GCExtendedGamepad, GCMotion, GCSystemGestureState, GCXboxGamepad,
};

use super::iohid::RESCAN_INTERVAL;
use crate::native::game_controller::{
    self, Category, GcBatteryState, GcButton, GcElements, GcMotion, GcPad, GcSnapshot,
};
use crate::provider::{
    Access, DeviceId, DeviceInfo, DeviceLocation, Provider, ProviderError, ProviderEvent,
};

/// The touchpad elements of a PlayStation pad.
struct Touchpad {
    button: Retained<GCControllerButtonInput>,
    primary: Retained<GCControllerDirectionPad>,
    secondary: Retained<GCControllerDirectionPad>,
}

/// The objects of one controller the provider reads, looked up once.
struct Elements {
    gamepad: Retained<GCExtendedGamepad>,
    xbox: Option<Retained<GCXboxGamepad>>,
    touchpad: Option<Touchpad>,
    motion: Option<Retained<GCMotion>>,
    battery: Option<Retained<GCDeviceBattery>>,
    light: Option<Retained<GCDeviceLight>>,
}

struct Entry {
    controller: Retained<GCController>,
    elements: Elements,
    info: DeviceInfo,
    pad: GcPad,
    access: Option<Access>,
}

/// The macOS GameController provider (see the [module documentation](super)).
///
/// Requires macOS 11.3 (the DualSense profile).
pub struct GameControllerProvider {
    entries: BTreeMap<DeviceId, Entry>,
    next_id: u64,
    last_scan: Option<Instant>,
    epoch: Instant,
}

impl GameControllerProvider {
    /// The provider, on macOS 11.3 or later; [`ProviderError::Unsupported`]
    /// before.
    pub fn new() -> Result<Self, ProviderError> {
        if !available!(macos = 11.3) {
            return Err(ProviderError::Unsupported);
        }
        Ok(Self {
            entries: BTreeMap::new(),
            next_id: 1,
            last_scan: None,
            epoch: Instant::now(),
        })
    }

    /// Whether GameController keeps delivering input while the app is not
    /// the active one (`GCController.shouldMonitorBackgroundEvents`): off
    /// by default, which suits games; tools such as the desktop app's
    /// widget and virtual pads turn it on. It applies to the whole process.
    pub fn set_background_events(&mut self, monitor: bool) {
        // SAFETY: a class property, available from macOS 11.3, which `new`
        // checked.
        unsafe { GCController::setShouldMonitorBackgroundEvents(monitor) };
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// Compares GameController's controllers with the entries.
    fn scan(&mut self, events: &mut Vec<ProviderEvent>) {
        // SAFETY: a class method with no requirements.
        let controllers = unsafe { GCController::controllers() };
        let controllers: Vec<Retained<GCController>> = controllers.to_vec();
        let gone: Vec<DeviceId> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                !controllers
                    .iter()
                    .any(|c| Retained::as_ptr(c) == Retained::as_ptr(&entry.controller))
            })
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(entry) = self.entries.remove(&id) {
                deactivate(&entry);
            }
            events.push(ProviderEvent::Removed(id));
        }
        for controller in controllers {
            let known = self
                .entries
                .values()
                .any(|entry| Retained::as_ptr(&entry.controller) == Retained::as_ptr(&controller));
            if known {
                continue;
            }
            // Controllers without the extended gamepad profile (the Siri
            // Remote's micro gamepad) are not game pads.
            if let Some(entry) = probe(controller) {
                let id = DeviceId(self.next_id);
                self.next_id += 1;
                self.entries.insert(id, entry);
                events.push(ProviderEvent::Added(id));
            }
        }
    }

    fn entry_mut(&mut self, device: DeviceId) -> Result<&mut Entry, ProviderError> {
        self.entries
            .get_mut(&device)
            .ok_or(ProviderError::UnknownDevice)
    }
}

/// Builds the entry of a new controller with an extended gamepad.
fn probe(controller: Retained<GCController>) -> Option<Entry> {
    // SAFETY: GameController's documented properties of a connected
    // controller, available from macOS 11.3 (checked by the provider's
    // constructor); the profile objects live as long as the controller,
    // which the entry retains.
    unsafe {
        let gamepad = controller.extendedGamepad()?;
        let category = controller.productCategory().to_string();
        let vendor_name = controller.vendorName().map(|name| name.to_string());
        let xbox = gamepad.clone().downcast::<GCXboxGamepad>().ok();
        let touchpad = if let Some(dualsense) = gamepad.downcast_ref::<GCDualSenseGamepad>() {
            Some(Touchpad {
                button: dualsense.touchpadButton(),
                primary: dualsense.touchpadPrimary(),
                secondary: dualsense.touchpadSecondary(),
            })
        } else if let Some(dualshock) = gamepad.downcast_ref::<GCDualShockGamepad>() {
            match (
                dualshock.touchpadButton(),
                dualshock.touchpadPrimary(),
                dualshock.touchpadSecondary(),
            ) {
                (Some(button), Some(primary), Some(secondary)) => Some(Touchpad {
                    button,
                    primary,
                    secondary,
                }),
                _ => None,
            }
        } else {
            None
        };
        let motion = controller.motion();
        let (gyroscope, accelerometer) = motion.as_ref().map_or((false, false), |motion| {
            (
                motion.hasRotationRate(),
                motion.hasGravityAndUserAcceleration() || motion.sensorsRequireManualActivation(),
            )
        });
        let elements = Elements {
            xbox,
            touchpad,
            motion,
            battery: controller.battery(),
            light: controller.light(),
            gamepad,
        };
        let present = GcElements {
            options: elements.gamepad.buttonOptions().is_some(),
            home: elements.gamepad.buttonHome().is_some(),
            thumbstick_buttons: elements.gamepad.leftThumbstickButton().is_some()
                && elements.gamepad.rightThumbstickButton().is_some(),
            touchpad: elements.touchpad.is_some(),
            share: elements
                .xbox
                .as_ref()
                .is_some_and(|xbox| xbox.buttonShare().is_some()),
            paddles: elements.xbox.as_ref().map_or([false; 4], |xbox| {
                [
                    xbox.paddleButton1().is_some(),
                    xbox.paddleButton2().is_some(),
                    xbox.paddleButton3().is_some(),
                    xbox.paddleButton4().is_some(),
                ]
            }),
            gyroscope,
            accelerometer,
            battery: elements.battery.is_some(),
            light: elements.light.is_some(),
        };
        let pad = GcPad::new(Category::parse(&category), present);
        let info = DeviceInfo {
            identity: game_controller::identity(vendor_name.as_deref(), &category),
            location: DeviceLocation::default(),
            layout: pad.layout(),
            outputs: pad.outputs(),
        };
        Some(Entry {
            controller,
            elements,
            info,
            pad,
            access: None,
        })
    }
}

/// Turns off the sensors the provider turned on, and gives the system
/// buttons back to macOS.
fn deactivate(entry: &Entry) {
    if let Some(motion) = &entry.elements.motion {
        // SAFETY: documented GCMotion properties (macOS 11.0).
        unsafe {
            if motion.sensorsRequireManualActivation() {
                motion.setSensorsActive(false);
            }
        }
    }
    system_gestures(&entry.elements, GCSystemGestureState::Enabled);
}

/// Sets whether macOS acts on the buttons it uses itself: Home opens the
/// Games app (or Launchpad), the share buttons take screenshots. Disabled
/// while an app reads the controller, so that the app gets those presses;
/// macOS applies it while the app is the active one.
fn system_gestures(elements: &Elements, state: GCSystemGestureState) {
    // SAFETY: documented properties of the extended gamepad profile and of
    // GCControllerElement (`preferredSystemGestureState`, macOS 11.0),
    // available from macOS 11.3 (checked by the provider's constructor).
    unsafe {
        let gamepad = &elements.gamepad;
        let share = elements.xbox.as_ref().and_then(|xbox| xbox.buttonShare());
        for button in [gamepad.buttonHome(), gamepad.buttonOptions(), share]
            .into_iter()
            .flatten()
        {
            button.setPreferredSystemGestureState(state);
        }
    }
}

/// Whether a button is pressed.
fn pressed(input: Option<Retained<GCControllerButtonInput>>) -> bool {
    // SAFETY: a documented property of a button input.
    input.is_some_and(|input| unsafe { input.isPressed() })
}

/// Reads one button of a controller.
fn button(elements: &Elements, button: GcButton) -> bool {
    let gamepad = &elements.gamepad;
    // SAFETY: documented properties of the extended gamepad profile and
    // its subclasses, available from macOS 11.3 (checked by the
    // provider's constructor).
    unsafe {
        let xbox = elements.xbox.as_deref();
        match button {
            GcButton::A => pressed(Some(gamepad.buttonA())),
            GcButton::B => pressed(Some(gamepad.buttonB())),
            GcButton::X => pressed(Some(gamepad.buttonX())),
            GcButton::Y => pressed(Some(gamepad.buttonY())),
            GcButton::DpadUp => pressed(Some(gamepad.dpad().up())),
            GcButton::DpadDown => pressed(Some(gamepad.dpad().down())),
            GcButton::DpadLeft => pressed(Some(gamepad.dpad().left())),
            GcButton::DpadRight => pressed(Some(gamepad.dpad().right())),
            GcButton::LeftShoulder => pressed(Some(gamepad.leftShoulder())),
            GcButton::RightShoulder => pressed(Some(gamepad.rightShoulder())),
            GcButton::LeftTrigger => pressed(Some(gamepad.leftTrigger())),
            GcButton::RightTrigger => pressed(Some(gamepad.rightTrigger())),
            GcButton::LeftThumbstickButton => pressed(gamepad.leftThumbstickButton()),
            GcButton::RightThumbstickButton => pressed(gamepad.rightThumbstickButton()),
            GcButton::Menu => pressed(Some(gamepad.buttonMenu())),
            GcButton::Options => pressed(gamepad.buttonOptions()),
            GcButton::Home => pressed(gamepad.buttonHome()),
            GcButton::TouchpadButton => pressed(
                elements
                    .touchpad
                    .as_ref()
                    .map(|touchpad| touchpad.button.clone()),
            ),
            GcButton::Share => pressed(xbox.and_then(|xbox| xbox.buttonShare())),
            GcButton::Paddle1 => pressed(xbox.and_then(|xbox| xbox.paddleButton1())),
            GcButton::Paddle2 => pressed(xbox.and_then(|xbox| xbox.paddleButton2())),
            GcButton::Paddle3 => pressed(xbox.and_then(|xbox| xbox.paddleButton3())),
            GcButton::Paddle4 => pressed(xbox.and_then(|xbox| xbox.paddleButton4())),
        }
    }
}

/// Reads a controller's current state.
fn snapshot(pad: &GcPad, elements: &Elements) -> GcSnapshot {
    let gamepad = &elements.gamepad;
    // SAFETY: documented properties of the extended gamepad profile, the
    // PlayStation touchpads, GCMotion and GCDeviceBattery, available from
    // macOS 11.3 (checked by the provider's constructor).
    unsafe {
        let stick =
            |pad: Retained<GCControllerDirectionPad>| (pad.xAxis().value(), pad.yAxis().value());
        let (left_x, left_y) = stick(gamepad.leftThumbstick());
        let (right_x, right_y) = stick(gamepad.rightThumbstick());
        let touches = elements.touchpad.as_ref().map_or([None, None], |touchpad| {
            [
                Some(stick(touchpad.primary.clone())),
                Some(stick(touchpad.secondary.clone())),
            ]
        });
        let motion = elements
            .motion
            .as_ref()
            .filter(|motion| motion.sensorsActive())
            .map(|motion| {
                let rate = motion.rotationRate();
                let acceleration = if motion.hasGravityAndUserAcceleration() {
                    let (g, u) = (motion.gravity(), motion.userAcceleration());
                    [g.x + u.x, g.y + u.y, g.z + u.z]
                } else {
                    let a = motion.acceleration();
                    [a.x, a.y, a.z]
                };
                GcMotion {
                    rotation_rate: [rate.x, rate.y, rate.z],
                    acceleration,
                }
            });
        GcSnapshot {
            buttons: pad
                .buttons()
                .iter()
                .map(|&gc_button| button(elements, gc_button))
                .collect(),
            axes: [
                left_x,
                left_y,
                right_x,
                right_y,
                gamepad.leftTrigger().value(),
                gamepad.rightTrigger().value(),
            ],
            touches,
            battery: elements.battery.as_ref().map(|battery| {
                (
                    battery.batteryLevel(),
                    GcBatteryState::from_raw(battery.batteryState().0),
                )
            }),
            motion,
        }
    }
}

impl Provider for GameControllerProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    fn update(&mut self, events: &mut Vec<ProviderEvent>) -> Result<(), ProviderError> {
        let now = Instant::now();
        if self
            .last_scan
            .is_some_and(|last| now.duration_since(last) < RESCAN_INTERVAL)
        {
            return Ok(());
        }
        self.last_scan = Some(now);
        autoreleasepool(|_| self.scan(events));
        Ok(())
    }

    fn info(&self, device: DeviceId) -> Option<&DeviceInfo> {
        self.entries.get(&device).map(|entry| &entry.info)
    }

    /// Opens a controller: GameController reads it already, so this turns
    /// on its motion sensors when they need it, and, unless observing,
    /// asks macOS to leave Home and the share buttons to the app.
    /// [`Access::Exclusive`] is unsupported: GameController cannot hide a
    /// controller.
    fn open(&mut self, device: DeviceId, access: Access) -> Result<(), ProviderError> {
        if access == Access::Exclusive {
            return Err(ProviderError::Unsupported);
        }
        let entry = self.entry_mut(device)?;
        if let Some(motion) = &entry.elements.motion {
            // SAFETY: documented GCMotion properties (macOS 11.0).
            unsafe {
                if motion.sensorsRequireManualActivation() {
                    motion.setSensorsActive(true);
                }
            }
        }
        // Observing changes nothing for the person: macOS keeps its buttons.
        let state = if access == Access::ObserveOnly {
            GCSystemGestureState::Enabled
        } else {
            GCSystemGestureState::Disabled
        };
        system_gestures(&entry.elements, state);
        entry.access = Some(access);
        Ok(())
    }

    fn close(&mut self, device: DeviceId) {
        if let Some(entry) = self.entries.get_mut(&device) {
            if entry.access.take().is_some() {
                deactivate(entry);
            }
        }
    }

    fn poll(&mut self, device: DeviceId, state: &mut RawState) -> Result<(), ProviderError> {
        let now_us = self.now_us();
        let entry = self.entry_mut(device)?;
        if entry.access.is_none() {
            return Err(ProviderError::NotOpen);
        }
        if !state.fits(&entry.info.layout) {
            return Err(ProviderError::Backend(
                "state does not fit the layout".into(),
            ));
        }
        let snapshot = autoreleasepool(|_| snapshot(&entry.pad, &entry.elements));
        entry.pad.fill(&snapshot, now_us, state);
        Ok(())
    }

    fn send(&mut self, device: DeviceId, command: OutputCommand) -> Result<(), ProviderError> {
        let entry = self.entry_mut(device)?;
        let access = entry.access.ok_or(ProviderError::NotOpen)?;
        if access == Access::ObserveOnly || !entry.info.outputs.contains(command.output()) {
            return Err(ProviderError::Unsupported);
        }
        match command {
            OutputCommand::Lightbar(Rgb { r, g, b }) => {
                let light = entry
                    .elements
                    .light
                    .as_ref()
                    .ok_or(ProviderError::Unsupported)?;
                let channel = |value: u8| f32::from(value) / 255.0;
                // SAFETY: documented GCColor and GCDeviceLight methods
                // (macOS 11.0).
                unsafe {
                    let color = GCColor::initWithRed_green_blue(
                        GCColor::alloc(),
                        channel(r),
                        channel(g),
                        channel(b),
                    );
                    light.setColor(&color);
                }
                Ok(())
            }
            OutputCommand::PlayerNumber(number) => {
                let index = match number {
                    1..=4 => GCControllerPlayerIndex(isize::from(number) - 1),
                    _ => GCControllerPlayerIndex::IndexUnset,
                };
                // SAFETY: a documented GCController property.
                unsafe { entry.controller.setPlayerIndex(index) };
                Ok(())
            }
            _ => Err(ProviderError::Unsupported),
        }
    }
}
