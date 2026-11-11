//! What the macOS GameController provider reads, sans-IO (C6): the
//! elements of a `GCController`, their canonical names, and the
//! conversions of its touchpad, battery and motion readings. The provider
//! itself is `macos::GameControllerProvider`, on macOS.
//!
//! GameController drives the pads macOS supports (PlayStation from the
//! DualShock 4 on, Xbox, Switch Pro, MFi) and normalizes them into its
//! extended gamepad profile, whose elements are named by position. The
//! provider reports them as canonical sources
//! ([`RawSource::Button`], [`RawSource::Axis`]), so the mapping engine
//! needs no profile, and a device profile can still correct one.
//!
//! The macOS code reads the elements into a [`GcSnapshot`] (plain values);
//! [`GcPad::fill`] turns a snapshot into a [`RawState`].
//!
//! # Unverified
//!
//! Every name, sign and frame below comes from Apple's headers and SDL's
//! GameController driver as remembered, none from a pad: which face
//! button `buttonA` is on a Switch Pro Controller, the product category
//! strings other than the DualSense's, the touchpad's vertical direction
//! and the motion frame ([`motion_sample`]). The first test campaign (H6)
//! checks them.

use anygamepad_model::identity::{Backend, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad_model::{
    Axis, Button, Family, MotionSample, Output, OutputSet, Power, PowerState, STANDARD_GRAVITY,
    Sensor, SensorSet, TouchPoint, TouchpadInfo, Transport, Vec3,
};

/// A `GCController`'s product category (`GCDevice.productCategory`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// `"DualSense"` (`GCProductCategoryDualSense`).
    DualSense,
    /// `"DualShock 4"` (`GCProductCategoryDualShock4`).
    DualShock4,
    /// `"Xbox One"` (`GCProductCategoryXboxOne`), every Xbox Wireless
    /// Controller.
    Xbox,
    /// `"Switch Pro Controller"`.
    SwitchPro,
    /// `"Nintendo Switch Joy-Con (L)"`, `(R)` or `(L/R)`.
    JoyCon,
    /// `"MFi"` (`GCProductCategoryMFi`): a Made for iPhone controller.
    Mfi,
    /// `"HID"` (`GCProductCategoryHID`): a HID game controller GameController
    /// maps itself.
    Hid,
    /// Any other category.
    Other,
}

impl Category {
    /// The category of a `productCategory` string.
    pub fn parse(category: &str) -> Self {
        match category {
            "DualSense" => Self::DualSense,
            "DualShock 4" => Self::DualShock4,
            "Xbox One" => Self::Xbox,
            "Switch Pro Controller" => Self::SwitchPro,
            "MFi" => Self::Mfi,
            "HID" => Self::Hid,
            _ if category.starts_with("Nintendo Switch Joy-Con") => Self::JoyCon,
            _ => Self::Other,
        }
    }

    /// The pad family of this category.
    pub fn family(self) -> Option<Family> {
        match self {
            Self::DualSense => Some(Family::DualSense),
            Self::DualShock4 => Some(Family::DualShock4),
            Self::Xbox => Some(Family::Xbox),
            Self::SwitchPro => Some(Family::SwitchPro),
            Self::JoyCon | Self::Mfi | Self::Hid | Self::Other => None,
        }
    }

    /// The touchpad of the PlayStation pads, at their resolution.
    pub fn touchpad(self) -> Option<TouchpadInfo> {
        let height = match self {
            Self::DualShock4 => 942,
            Self::DualSense => 1080,
            _ => return None,
        };
        Some(TouchpadInfo {
            max_fingers: 2,
            width: 1920,
            height,
        })
    }

    /// Whether the pad has player lights that `GCController.playerIndex`
    /// sets.
    pub fn has_player_lights(self) -> bool {
        matches!(self, Self::DualSense | Self::SwitchPro | Self::JoyCon)
    }
}

/// A button of GameController's extended gamepad profile and its
/// subclasses (`GCDualSenseGamepad`, `GCDualShockGamepad`,
/// `GCXboxGamepad`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GcButton {
    /// `buttonA`: the bottom face button.
    A,
    /// `buttonB`: the right face button.
    B,
    /// `buttonX`: the left face button.
    X,
    /// `buttonY`: the top face button.
    Y,
    /// `dpad.up`.
    DpadUp,
    /// `dpad.down`.
    DpadDown,
    /// `dpad.left`.
    DpadLeft,
    /// `dpad.right`.
    DpadRight,
    /// `leftShoulder`.
    LeftShoulder,
    /// `rightShoulder`.
    RightShoulder,
    /// `leftTrigger`, pressed or not.
    LeftTrigger,
    /// `rightTrigger`, pressed or not.
    RightTrigger,
    /// `leftThumbstickButton` (optional).
    LeftThumbstickButton,
    /// `rightThumbstickButton` (optional).
    RightThumbstickButton,
    /// `buttonMenu`: the right menu button.
    Menu,
    /// `buttonOptions` (optional): the left menu button.
    Options,
    /// `buttonHome` (optional).
    Home,
    /// `touchpadButton` (DualSense, DualShock 4).
    TouchpadButton,
    /// `buttonShare` (Xbox Series).
    Share,
    /// `paddleButton1` (Xbox Elite, `P1`).
    Paddle1,
    /// `paddleButton2` (Xbox Elite, `P2`).
    Paddle2,
    /// `paddleButton3` (Xbox Elite, `P3`).
    Paddle3,
    /// `paddleButton4` (Xbox Elite, `P4`).
    Paddle4,
}

impl GcButton {
    /// The buttons every extended gamepad has, in layout order.
    pub const REQUIRED: [Self; 13] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Menu,
    ];

    /// The canonical button it is.
    pub const fn canonical(self) -> Button {
        match self {
            Self::A => Button::South,
            Self::B => Button::East,
            Self::X => Button::West,
            Self::Y => Button::North,
            Self::DpadUp => Button::DPadUp,
            Self::DpadDown => Button::DPadDown,
            Self::DpadLeft => Button::DPadLeft,
            Self::DpadRight => Button::DPadRight,
            Self::LeftShoulder => Button::LeftShoulder,
            Self::RightShoulder => Button::RightShoulder,
            Self::LeftTrigger => Button::LeftTrigger,
            Self::RightTrigger => Button::RightTrigger,
            Self::LeftThumbstickButton => Button::LeftStick,
            Self::RightThumbstickButton => Button::RightStick,
            Self::Menu => Button::Start,
            Self::Options => Button::Select,
            Self::Home => Button::Home,
            Self::TouchpadButton => Button::TouchpadClick,
            Self::Share => Button::Share,
            Self::Paddle1 => Button::RightPaddle1,
            Self::Paddle2 => Button::RightPaddle2,
            Self::Paddle3 => Button::LeftPaddle1,
            Self::Paddle4 => Button::LeftPaddle2,
        }
    }
}

/// The axes of the extended gamepad profile, in layout order: the sticks
/// in `[-1, 1]` with Y up, then the triggers in `[0, 1]`.
pub const AXES: [Axis; 6] = [
    Axis::LeftStickX,
    Axis::LeftStickY,
    Axis::RightStickX,
    Axis::RightStickY,
    Axis::LeftTrigger,
    Axis::RightTrigger,
];

/// The optional elements a controller has, as the macOS code finds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcElements {
    /// `buttonOptions`.
    pub options: bool,
    /// `buttonHome`.
    pub home: bool,
    /// `leftThumbstickButton` and `rightThumbstickButton`.
    pub thumbstick_buttons: bool,
    /// A `GCDualSenseGamepad`, or a `GCDualShockGamepad` with its touchpad.
    pub touchpad: bool,
    /// `buttonShare` of a `GCXboxGamepad`.
    pub share: bool,
    /// `paddleButton1` to `paddleButton4` of a `GCXboxGamepad`.
    pub paddles: [bool; 4],
    /// `GCController.motion` with a rotation rate.
    pub gyroscope: bool,
    /// `GCController.motion` with acceleration.
    pub accelerometer: bool,
    /// `GCController.battery`.
    pub battery: bool,
    /// `GCController.light`.
    pub light: bool,
}

/// One controller as the provider reports it: its category, elements and
/// the layout they make.
#[derive(Clone, Debug, PartialEq)]
pub struct GcPad {
    category: Category,
    elements: GcElements,
    buttons: Vec<GcButton>,
    touchpad: Option<TouchpadInfo>,
    last_motion: Option<GcMotion>,
}

impl GcPad {
    /// A controller of this category with these elements.
    pub fn new(category: Category, elements: GcElements) -> Self {
        let mut buttons = GcButton::REQUIRED.to_vec();
        let optional = [
            (elements.thumbstick_buttons, GcButton::LeftThumbstickButton),
            (elements.thumbstick_buttons, GcButton::RightThumbstickButton),
            (elements.options, GcButton::Options),
            (elements.home, GcButton::Home),
            (elements.touchpad, GcButton::TouchpadButton),
            (elements.share, GcButton::Share),
            (elements.paddles[0], GcButton::Paddle1),
            (elements.paddles[1], GcButton::Paddle2),
            (elements.paddles[2], GcButton::Paddle3),
            (elements.paddles[3], GcButton::Paddle4),
        ];
        buttons.extend(
            optional
                .into_iter()
                .filter(|(present, _)| *present)
                .map(|(_, button)| button),
        );
        let touchpad = elements
            .touchpad
            .then(|| category.touchpad())
            .flatten()
            .or_else(|| {
                elements.touchpad.then_some(TouchpadInfo {
                    max_fingers: 2,
                    width: 0,
                    height: 0,
                })
            });
        Self {
            category,
            elements,
            buttons,
            touchpad,
            last_motion: None,
        }
    }

    /// The controller's category.
    pub fn category(&self) -> Category {
        self.category
    }

    /// The buttons, in the order of the layout and of
    /// [`GcSnapshot::buttons`].
    pub fn buttons(&self) -> &[GcButton] {
        &self.buttons
    }

    /// The elements [`GcPad::fill`] writes.
    pub fn layout(&self) -> RawLayout {
        let mut sensors = SensorSet::default();
        if self.elements.gyroscope {
            sensors.insert(Sensor::Gyroscope);
        }
        if self.elements.accelerometer {
            sensors.insert(Sensor::Accelerometer);
        }
        RawLayout {
            buttons: self
                .buttons
                .iter()
                .map(|button| RawSource::Button(button.canonical()))
                .collect(),
            axes: AXES
                .iter()
                .map(|&axis| RawAxisInfo {
                    source: RawSource::Axis(axis),
                    min: if axis.is_trigger() { 0.0 } else { -1.0 },
                    max: 1.0,
                })
                .collect(),
            hats: Vec::new(),
            sensors,
            touchpads: self.touchpad.into_iter().collect(),
            battery: self.elements.battery,
        }
    }

    /// The outputs the provider drives: the light bar and the player
    /// lights where the controller has them.
    pub fn outputs(&self) -> OutputSet {
        let mut outputs = OutputSet::default();
        if self.elements.light {
            outputs.insert(Output::Lightbar);
        }
        if self.category.has_player_lights() {
            outputs.insert(Output::PlayerLeds);
        }
        outputs
    }

    /// Writes a snapshot into `state`, which must fit the layout. A motion
    /// reading becomes a sample only when it changed since the previous
    /// fill, stamped `now_us` (GameController gives no sensor clock).
    pub fn fill(&mut self, snapshot: &GcSnapshot, now_us: u64, state: &mut RawState) {
        for (value, pressed) in state.buttons.iter_mut().zip(&snapshot.buttons) {
            *value = if *pressed { 1.0 } else { 0.0 };
        }
        for (value, axis) in state.axes.iter_mut().zip(snapshot.axes) {
            *value = axis;
        }
        state.touches = Default::default();
        if self.touchpad.is_some() {
            let fingers = snapshot.touches.iter().enumerate();
            for (slot, (id, touch)) in state.touches.iter_mut().zip(fingers) {
                *slot = touch.and_then(|(x, y)| touch_point(id as u8, x, y));
            }
        }
        state.power = snapshot.battery.map(power).unwrap_or_default();
        state.motion.clear();
        if let Some(motion) = snapshot.motion {
            if self.last_motion != Some(motion) {
                self.last_motion = Some(motion);
                state.motion.push(motion_sample(
                    motion,
                    now_us,
                    self.elements.gyroscope,
                    self.elements.accelerometer,
                ));
            }
        }
    }
}

/// What the macOS code reads of a controller at one instant.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GcSnapshot {
    /// Whether each button of [`GcPad::buttons`] is pressed, in its order.
    pub buttons: Vec<bool>,
    /// The values of [`AXES`], in its order.
    pub axes: [f32; 6],
    /// `touchpadPrimary` and `touchpadSecondary`: `(x, y)` in `[-1, 1]`,
    /// or `None`.
    pub touches: [Option<(f32, f32)>; 2],
    /// `batteryLevel` in `[0, 1]` and `batteryState`.
    pub battery: Option<(f32, GcBatteryState)>,
    /// The motion reading, when the sensors are active.
    pub motion: Option<GcMotion>,
}

/// `GCDeviceBatteryState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcBatteryState {
    /// `GCDeviceBatteryStateUnknown`.
    Unknown,
    /// `GCDeviceBatteryStateDischarging`.
    Discharging,
    /// `GCDeviceBatteryStateCharging`.
    Charging,
    /// `GCDeviceBatteryStateFull`.
    Full,
}

impl GcBatteryState {
    /// The state of a `GCDeviceBatteryState` value.
    pub fn from_raw(value: isize) -> Self {
        match value {
            0 => Self::Discharging,
            1 => Self::Charging,
            2 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// A `GCMotion` reading as GameController gives it: rotation rate in rad/s
/// and acceleration in g, in its frame and sign convention (see
/// [`motion_sample`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GcMotion {
    /// `rotationRate`.
    pub rotation_rate: [f64; 3],
    /// `acceleration`, or `gravity` plus `userAcceleration`.
    pub acceleration: [f64; 3],
}

/// The power status of a battery reading.
pub fn power((level, state): (f32, GcBatteryState)) -> Power {
    let state = match state {
        GcBatteryState::Unknown => PowerState::Unknown,
        GcBatteryState::Discharging => PowerState::OnBattery,
        GcBatteryState::Charging => PowerState::Charging,
        GcBatteryState::Full => PowerState::Charged,
    };
    let percent =
        (level.is_finite() && (0.0..=1.0).contains(&level)).then(|| (level * 100.0).round() as u8);
    Power { state, percent }
}

/// A finger on the touchpad from a direction pad reading in `[-1, 1]`
/// (Y up). GameController reads `(0, 0)` with no finger, as SDL takes it.
pub fn touch_point(id: u8, x: f32, y: f32) -> Option<TouchPoint> {
    if x == 0.0 && y == 0.0 {
        return None;
    }
    Some(TouchPoint {
        touchpad: 0,
        id,
        x: ((x + 1.0) / 2.0).clamp(0.0, 1.0),
        y: ((1.0 - y) / 2.0).clamp(0.0, 1.0),
    })
}

/// A motion sample in the canonical [device frame](anygamepad_model::motion)
/// from a GameController reading.
///
/// Assumes GameController's frame and convention are Core Motion's: X to
/// the right, Y towards the far edge, Z out of the face, and gravity read
/// as -1 g on the axis pointing up. So the canonical vector is
/// `(x, z, -y)`, and the acceleration changes sign (the canonical
/// accelerometer reads +1 g upwards). SDL's GameController driver maps the
/// rotation rate the same way. To verify on a pad (H6).
pub fn motion_sample(
    motion: GcMotion,
    timestamp_us: u64,
    gyroscope: bool,
    accelerometer: bool,
) -> MotionSample {
    let frame = |[x, y, z]: [f64; 3]| Vec3::new(x as f32, z as f32, -y as f32);
    let [x, y, z] = motion.acceleration;
    let g = f64::from(STANDARD_GRAVITY);
    MotionSample {
        timestamp_us,
        gyro: gyroscope.then(|| frame(motion.rotation_rate)),
        accel: accelerometer.then(|| frame([-x * g, -y * g, -z * g])),
    }
}

/// A controller's identity as the GameController backend sees it.
///
/// GameController tells no USB IDs and no transport: the product name is
/// `vendorName` (`"DualSense Wireless Controller"`, the name Safari shows
/// too), and the product category stands as the driver.
pub fn identity(vendor_name: Option<&str>, product_category: &str) -> DeviceIdentity {
    let mut identity =
        DeviceIdentity::new(ProviderKind::Native, Backend::GameController, Os::MacOs);
    identity.transport = Transport::Unknown;
    identity.product_name = vendor_name
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    identity.driver = Some(product_category.to_owned()).filter(|category| !category.is_empty());
    identity
}

#[cfg(test)]
mod tests {
    use anygamepad_model::raw::RawSource;

    use super::*;

    fn dualsense() -> GcPad {
        GcPad::new(
            Category::DualSense,
            GcElements {
                options: true,
                home: true,
                thumbstick_buttons: true,
                touchpad: true,
                gyroscope: true,
                accelerometer: true,
                battery: true,
                light: true,
                ..GcElements::default()
            },
        )
    }

    #[test]
    fn categories() {
        assert_eq!(Category::parse("DualSense"), Category::DualSense);
        assert_eq!(Category::parse("DualShock 4"), Category::DualShock4);
        assert_eq!(Category::parse("Xbox One"), Category::Xbox);
        assert_eq!(
            Category::parse("Nintendo Switch Joy-Con (L/R)"),
            Category::JoyCon
        );
        assert_eq!(Category::parse("Racing Wheel"), Category::Other);
        assert_eq!(Category::DualSense.family(), Some(Family::DualSense));
        assert_eq!(Category::Mfi.family(), None);
    }

    #[test]
    fn layout_of_a_dualsense() {
        let pad = dualsense();
        let layout = pad.layout();
        assert_eq!(layout.buttons.len(), 18);
        assert_eq!(layout.buttons[0], RawSource::Button(Button::South));
        assert_eq!(layout.buttons[12], RawSource::Button(Button::Start));
        assert!(
            layout
                .buttons
                .contains(&RawSource::Button(Button::TouchpadClick))
        );
        assert_eq!(layout.axes.len(), 6);
        assert_eq!((layout.axes[1].min, layout.axes[1].max), (-1.0, 1.0));
        assert_eq!((layout.axes[4].min, layout.axes[4].max), (0.0, 1.0));
        assert_eq!(layout.touchpads[0].height, 1080);
        assert!(layout.sensors.contains(Sensor::Gyroscope));
        assert!(layout.battery);
        assert!(pad.outputs().contains(Output::Lightbar));
        assert!(pad.outputs().contains(Output::PlayerLeds));
    }

    #[test]
    fn layout_of_an_xbox_elite() {
        let pad = GcPad::new(
            Category::Xbox,
            GcElements {
                options: true,
                home: true,
                thumbstick_buttons: true,
                share: true,
                paddles: [true; 4],
                ..GcElements::default()
            },
        );
        let buttons: Vec<Button> = pad.buttons().iter().map(|b| b.canonical()).collect();
        assert!(buttons.ends_with(&[
            Button::Share,
            Button::RightPaddle1,
            Button::RightPaddle2,
            Button::LeftPaddle1,
            Button::LeftPaddle2,
        ]));
        assert!(pad.layout().touchpads.is_empty());
        assert!(pad.outputs().is_empty());
    }

    #[test]
    fn fill_from_a_snapshot() {
        let mut pad = dualsense();
        let layout = pad.layout();
        let mut state = RawState::for_layout(&layout);
        let mut snapshot = GcSnapshot {
            buttons: vec![false; pad.buttons().len()],
            axes: [0.5, -1.0, 0.0, 0.0, 0.25, 1.0],
            touches: [Some((1.0, 1.0)), None],
            battery: Some((0.42, GcBatteryState::Charging)),
            motion: Some(GcMotion {
                rotation_rate: [0.1, 0.2, 0.3],
                acceleration: [0.0, 0.0, -1.0],
            }),
        };
        snapshot.buttons[0] = true;
        pad.fill(&snapshot, 1_000, &mut state);
        assert_eq!(state.buttons[0], 1.0);
        assert_eq!(state.buttons[1], 0.0);
        assert_eq!(state.axes, [0.5, -1.0, 0.0, 0.0, 0.25, 1.0]);
        let touch = state.touches[0].unwrap();
        assert_eq!((touch.x, touch.y), (1.0, 0.0));
        assert_eq!(state.touches[1], None);
        assert_eq!(
            state.power,
            Power {
                state: PowerState::Charging,
                percent: Some(42)
            }
        );
        let sample = state.motion[0];
        assert_eq!(sample.timestamp_us, 1_000);
        assert_eq!(sample.gyro, Some(Vec3::new(0.1, 0.3, -0.2)));
        // Lying flat, face up: +1 g on the canonical Y axis.
        let accel = sample.accel.unwrap();
        assert!((accel.y - STANDARD_GRAVITY).abs() < 1e-5);
        assert_eq!((accel.x, accel.z), (0.0, 0.0));

        // The same reading again is no new sample.
        pad.fill(&snapshot, 2_000, &mut state);
        assert!(state.motion.is_empty());
    }

    #[test]
    fn touch_and_power_edges() {
        assert_eq!(touch_point(0, 0.0, 0.0), None);
        let center_top = touch_point(1, 0.0, 1.0).unwrap();
        assert_eq!((center_top.id, center_top.x, center_top.y), (1, 0.5, 0.0));
        assert_eq!(power((-1.0, GcBatteryState::Unknown)).percent, None);
        assert_eq!(
            power((1.0, GcBatteryState::Full)).state,
            PowerState::Charged
        );
        assert_eq!(GcBatteryState::from_raw(-1), GcBatteryState::Unknown);
        assert_eq!(GcBatteryState::from_raw(0), GcBatteryState::Discharging);
    }

    #[test]
    fn identity_has_no_ids() {
        let identity = super::identity(Some("DualSense Wireless Controller"), "DualSense");
        assert_eq!(identity.backend, Backend::GameController);
        assert_eq!((identity.vendor_id, identity.product_id), (None, None));
        assert_eq!(Family::from_identity(&identity), Some(Family::DualSense));
        assert_eq!(identity.driver.as_deref(), Some("DualSense"));
        assert_eq!(super::identity(None, "").driver, None);
    }
}
