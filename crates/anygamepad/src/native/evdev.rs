//! Linux input devices (evdev), sans-IO: which nodes are gamepads, the raw
//! layout of a gamepad node, and the motion and touchpad nodes that drivers
//! such as `hid-playstation` split a controller into (C4).
//!
//! The Linux provider ([`super::linux`]) reads the nodes and feeds what it
//! reads here, so this module is testable without devices and on any OS.
//!
//! Sources: the kernel's `input-event-codes.h`, `Documentation/input/`
//! (`gamepad.rst`, `event-codes.rst`, `multi-touch-protocol.rst`) and
//! research note 05.

use anygamepad_db::{AxisMapping, Input, Mapping};
use anygamepad_model::raw::{Hat, RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad_model::{
    Axis, Button, MAX_TOUCH_POINTS, MotionSample, STANDARD_GRAVITY, Sensor, SensorSet, TouchPoint,
    TouchpadInfo, Transport, Vec3,
};

/// Event types, codes and properties from `input-event-codes.h`.
pub mod codes {
    #![allow(missing_docs, reason = "kernel names, documented by the kernel")]

    pub const EV_SYN: u16 = 0x00;
    pub const EV_KEY: u16 = 0x01;
    pub const EV_ABS: u16 = 0x03;
    pub const EV_MSC: u16 = 0x04;

    pub const SYN_REPORT: u16 = 0;
    pub const SYN_DROPPED: u16 = 3;
    pub const MSC_TIMESTAMP: u16 = 0x05;

    pub const KEY_RECORD: u16 = 0xa7;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_JOYSTICK: u16 = 0x120;
    pub const BTN_SOUTH: u16 = 0x130;
    pub const BTN_EAST: u16 = 0x131;
    pub const BTN_C: u16 = 0x132;
    pub const BTN_NORTH: u16 = 0x133;
    pub const BTN_WEST: u16 = 0x134;
    pub const BTN_Z: u16 = 0x135;
    pub const BTN_TL: u16 = 0x136;
    pub const BTN_TR: u16 = 0x137;
    pub const BTN_TL2: u16 = 0x138;
    pub const BTN_TR2: u16 = 0x139;
    pub const BTN_SELECT: u16 = 0x13a;
    pub const BTN_START: u16 = 0x13b;
    pub const BTN_MODE: u16 = 0x13c;
    pub const BTN_THUMBL: u16 = 0x13d;
    pub const BTN_THUMBR: u16 = 0x13e;
    pub const BTN_DIGI: u16 = 0x140;
    pub const BTN_TOUCH: u16 = 0x14a;
    pub const BTN_DPAD_UP: u16 = 0x220;
    pub const BTN_DPAD_DOWN: u16 = 0x221;
    pub const BTN_DPAD_LEFT: u16 = 0x222;
    pub const BTN_DPAD_RIGHT: u16 = 0x223;
    pub const BTN_TRIGGER_HAPPY: u16 = 0x2c0;
    pub const BTN_TRIGGER_HAPPY40: u16 = 0x2e7;

    pub const ABS_X: u16 = 0x00;
    pub const ABS_Y: u16 = 0x01;
    pub const ABS_Z: u16 = 0x02;
    pub const ABS_RX: u16 = 0x03;
    pub const ABS_RY: u16 = 0x04;
    pub const ABS_RZ: u16 = 0x05;
    pub const ABS_GAS: u16 = 0x09;
    pub const ABS_BRAKE: u16 = 0x0a;
    pub const ABS_HAT0X: u16 = 0x10;
    pub const ABS_HAT0Y: u16 = 0x11;
    pub const ABS_HAT3Y: u16 = 0x17;
    pub const ABS_MT_SLOT: u16 = 0x2f;
    pub const ABS_MT_POSITION_X: u16 = 0x35;
    pub const ABS_MT_POSITION_Y: u16 = 0x36;
    pub const ABS_MT_TRACKING_ID: u16 = 0x39;
    pub const ABS_MAX: u16 = 0x3f;

    pub const INPUT_PROP_POINTER: u16 = 0x00;
    pub const INPUT_PROP_BUTTONPAD: u16 = 0x02;
    pub const INPUT_PROP_ACCELEROMETER: u16 = 0x06;

    pub const FF_RUMBLE: u16 = 0x50;

    pub const BUS_USB: u16 = 0x03;
    pub const BUS_BLUETOOTH: u16 = 0x05;
    pub const BUS_VIRTUAL: u16 = 0x06;
}

use codes::*;

/// The range of an absolute axis (`struct input_absinfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbsInfo {
    /// Current value.
    pub value: i32,
    /// Smallest value.
    pub minimum: i32,
    /// Largest value.
    pub maximum: i32,
    /// Noise the kernel filters out.
    pub fuzz: i32,
    /// Values within this distance from the centre read as the centre.
    pub flat: i32,
    /// Units per millimetre (positions), per g (accelerometers) or per
    /// degree per second (gyroscopes); 0 when unknown.
    pub resolution: i32,
}

/// What an input node is, from its capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A gamepad or joystick: buttons in the joystick ranges.
    Gamepad,
    /// The motion sensors of a controller (`INPUT_PROP_ACCELEROMETER`).
    Motion,
    /// The touchpad of a controller: multitouch positions, no joystick
    /// buttons.
    Touchpad,
    /// Anything else (keyboards, mice, ...): never opened.
    Other,
}

/// Whether `code` is a joystick or gamepad button (`BTN_JOYSTICK` to
/// `BTN_DIGI`, `BTN_TRIGGER_HAPPY*`), the buttons that make a node a
/// gamepad for udev's `input_id` too.
const fn is_joystick_button(code: u16) -> bool {
    (code >= BTN_JOYSTICK && code < BTN_DIGI)
        || (code >= BTN_TRIGGER_HAPPY && code <= BTN_TRIGGER_HAPPY40)
}

/// Classifies a node from its key codes, absolute axis codes and input
/// properties (as sysfs lists them, before opening it).
pub fn classify(keys: &[u16], abs: &[u16], properties: &[u16]) -> NodeKind {
    if properties.contains(&INPUT_PROP_ACCELEROMETER) {
        return NodeKind::Motion;
    }
    let joystick_buttons = keys.iter().any(|&code| is_joystick_button(code));
    let multitouch = abs.contains(&ABS_MT_POSITION_X) && abs.contains(&ABS_MT_POSITION_Y);
    if multitouch && !joystick_buttons {
        return NodeKind::Touchpad;
    }
    if joystick_buttons {
        NodeKind::Gamepad
    } else {
        NodeKind::Other
    }
}

/// The codes set in a sysfs capability bitmap (`capabilities/key`,
/// `properties`, ...): hexadecimal words separated by spaces, most
/// significant first, each `word_bits` wide (the kernel's `long`).
pub fn parse_bitmap(text: &str, word_bits: u32) -> Option<Vec<u16>> {
    let mut codes = Vec::new();
    for (index, word) in text.split_whitespace().rev().enumerate() {
        let word = u64::from_str_radix(word, 16).ok()?;
        for bit in 0..word_bits.min(64) {
            if word & (1 << bit) != 0 {
                codes.push(u16::try_from(index as u32 * word_bits + bit).ok()?);
            }
        }
    }
    Some(codes)
}

/// The transport of a bus type (`input_id.bustype`).
pub const fn transport(bustype: u16) -> Transport {
    match bustype {
        BUS_USB => Transport::Usb,
        BUS_BLUETOOTH => Transport::Bluetooth,
        _ => Transport::Unknown,
    }
}

/// Drivers that follow the Linux gamepad specification (`gamepad.rst`):
/// face buttons by position, sticks on `ABS_X/Y` and `ABS_RX/RY`. Their
/// pads read correctly with [`spec_mapping`] when no profile says better.
pub const SPEC_DRIVERS: [&str; 4] = ["xpad", "playstation", "nintendo", "sony"];

/// Whether the driver bound to a pad follows the Linux gamepad
/// specification.
pub fn follows_spec(driver: Option<&str>) -> bool {
    driver.is_some_and(|driver| SPEC_DRIVERS.contains(&driver))
}

/// The buttons, axes and hats of a gamepad node, and how its state fills a
/// [`RawState`].
///
/// Every element is listed by its evdev code ([`RawSource::Evdev`]); hats
/// by the code of their X axis. Axes keep the device's range and
/// direction (Y grows downwards on Linux); [`spec_mapping`] or a profile
/// turns them into canonical axes.
#[derive(Clone, Debug, PartialEq)]
pub struct GamepadNode {
    buttons: Vec<u16>,
    axes: Vec<u16>,
    /// X and Y codes of each hat.
    hats: Vec<(u16, u16)>,
    layout: RawLayout,
}

impl GamepadNode {
    /// A node with these key codes and absolute axes (sorted or not).
    pub fn new(keys: &[u16], abs: &[(u16, AbsInfo)]) -> Self {
        let mut buttons: Vec<u16> = keys.to_vec();
        buttons.sort_unstable();
        buttons.dedup();
        let mut abs: Vec<(u16, AbsInfo)> = abs
            .iter()
            .copied()
            .filter(|(code, _)| *code < ABS_MT_SLOT)
            .collect();
        abs.sort_unstable_by_key(|(code, _)| *code);
        abs.dedup_by_key(|(code, _)| *code);

        let is_hat = |code: u16| (ABS_HAT0X..=ABS_HAT3Y).contains(&code);
        let axes: Vec<(u16, AbsInfo)> = abs
            .iter()
            .copied()
            .filter(|(code, _)| !is_hat(*code))
            .collect();
        let hats: Vec<(u16, u16)> = (0..4)
            .map(|i| (ABS_HAT0X + 2 * i, ABS_HAT0Y + 2 * i))
            .filter(|(x, y)| abs.iter().any(|(code, _)| code == x || code == y))
            .collect();

        let layout = RawLayout {
            buttons: buttons.iter().map(|&code| RawSource::Evdev(code)).collect(),
            axes: axes
                .iter()
                .map(|(code, info)| RawAxisInfo {
                    source: RawSource::Evdev(*code),
                    min: info.minimum as f32,
                    max: info.maximum as f32,
                })
                .collect(),
            hats: hats.iter().map(|(x, _)| RawSource::Evdev(*x)).collect(),
            ..RawLayout::default()
        };
        Self {
            buttons,
            axes: axes.into_iter().map(|(code, _)| code).collect(),
            hats,
            layout,
        }
    }

    /// The raw elements, for [`DeviceInfo`](crate::provider::DeviceInfo).
    pub fn layout(&self) -> &RawLayout {
        &self.layout
    }

    /// Writes the node's current state into the first elements of `state`:
    /// `pressed` tells whether a key is down, `value` gives an absolute
    /// axis's value.
    pub fn fill(
        &self,
        pressed: impl Fn(u16) -> bool,
        value: impl Fn(u16) -> i32,
        state: &mut RawState,
    ) {
        for (slot, &code) in state.buttons.iter_mut().zip(&self.buttons) {
            *slot = if pressed(code) { 1.0 } else { 0.0 };
        }
        for (slot, &code) in state.axes.iter_mut().zip(&self.axes) {
            *slot = value(code) as f32;
        }
        for (slot, &(x, y)) in state.hats.iter_mut().zip(&self.hats) {
            *slot = Hat::from_axes(value(x), value(y));
        }
    }
}

/// The mapping of a pad whose driver follows the Linux gamepad
/// specification ([`SPEC_DRIVERS`]), for the elements `layout` has.
///
/// Face buttons, shoulders, menus and stick clicks by their positional
/// codes; sticks on `ABS_X/Y` and `ABS_RX/RY` with Y reversed (evdev Y
/// grows downwards); analog triggers on `ABS_Z/RZ`, else `ABS_BRAKE/GAS`,
/// else the digital `BTN_TL2/TR2`; the d-pad from the first hat, else
/// `BTN_DPAD_*`. `KEY_RECORD` is the Xbox Series share button (`xpad`).
///
/// `xpad` is the exception for the top and left face buttons: it reports
/// the Xbox `X` button (west) as `BTN_X` and `Y` (north) as `BTN_Y`, which
/// are the codes of `BTN_NORTH` and `BTN_WEST` (research note 05). `driver`
/// is the driver bound to the pad.
pub fn spec_mapping(layout: &RawLayout, driver: Option<&str>) -> Mapping {
    let (north, west) = if driver == Some("xpad") {
        (BTN_WEST, BTN_NORTH)
    } else {
        (BTN_NORTH, BTN_WEST)
    };
    let has_button = |code: u16| layout.buttons.contains(&RawSource::Evdev(code));
    let has_axis = |code: u16| {
        layout
            .axes
            .iter()
            .any(|axis| axis.source == RawSource::Evdev(code))
    };
    let mut mapping = Mapping::default();
    let buttons = [
        (Button::South, BTN_SOUTH),
        (Button::East, BTN_EAST),
        (Button::North, north),
        (Button::West, west),
        (Button::LeftShoulder, BTN_TL),
        (Button::RightShoulder, BTN_TR),
        (Button::Select, BTN_SELECT),
        (Button::Start, BTN_START),
        (Button::Home, BTN_MODE),
        (Button::LeftStick, BTN_THUMBL),
        (Button::RightStick, BTN_THUMBR),
        (Button::Share, KEY_RECORD),
    ];
    for (button, code) in buttons {
        if has_button(code) {
            mapping
                .buttons
                .insert(button, Input::Button(RawSource::Evdev(code)));
        }
    }
    let axis = |code: u16, inverted: bool| Input::Axis {
        source: RawSource::Evdev(code),
        range: anygamepad_db::AxisRange::Full,
        inverted,
    };
    for (target, code, inverted) in [
        (Axis::LeftStickX, ABS_X, false),
        (Axis::LeftStickY, ABS_Y, true),
        (Axis::RightStickX, ABS_RX, false),
        (Axis::RightStickY, ABS_RY, true),
    ] {
        if has_axis(code) {
            mapping
                .axes
                .insert(target, AxisMapping::full(axis(code, inverted)));
        }
    }
    let triggers = [
        (
            Axis::LeftTrigger,
            Button::LeftTrigger,
            [ABS_Z, ABS_BRAKE],
            BTN_TL2,
        ),
        (
            Axis::RightTrigger,
            Button::RightTrigger,
            [ABS_RZ, ABS_GAS],
            BTN_TR2,
        ),
    ];
    for (target, button, codes, digital) in triggers {
        if let Some(&code) = codes.iter().find(|&&code| has_axis(code)) {
            mapping
                .axes
                .insert(target, AxisMapping::full(axis(code, false)));
        } else if has_button(digital) {
            mapping
                .buttons
                .insert(button, Input::Button(RawSource::Evdev(digital)));
        }
    }
    let dpad = [
        (Button::DPadUp, Hat::UP, BTN_DPAD_UP),
        (Button::DPadDown, Hat::DOWN, BTN_DPAD_DOWN),
        (Button::DPadLeft, Hat::LEFT, BTN_DPAD_LEFT),
        (Button::DPadRight, Hat::RIGHT, BTN_DPAD_RIGHT),
    ];
    let hat = layout.hats.first().copied();
    for (button, mask, code) in dpad {
        let input = match hat {
            Some(source) => Some(Input::Hat { source, mask }),
            None => has_button(code).then_some(Input::Button(RawSource::Evdev(code))),
        };
        if let Some(input) = input {
            mapping.buttons.insert(button, input);
        }
    }
    mapping
}

/// The motion node of a controller: accelerometer on `ABS_X/Y/Z` in units
/// per g, gyroscope on `ABS_RX/RY/RZ` in units per degree per second, and
/// the sensor clock in `MSC_TIMESTAMP` (microseconds, wrapping at 32 bits).
///
/// The axes are taken as the canonical device frame, which is SDL's, as
/// `hid-playstation` and `hid-nintendo` report them; not checked on a
/// device yet.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionNode {
    /// Counts per g (accelerometer) or per °/s (gyroscope) of each axis, in
    /// code order; 0 where the axis is missing.
    resolution: [f32; 6],
    values: [i32; 6],
    sensors: SensorSet,
    clock: Option<u32>,
    timestamp_us: u64,
}

impl MotionNode {
    /// A motion node with these absolute axes; `None` without any sensor.
    pub fn new(abs: &[(u16, AbsInfo)]) -> Option<Self> {
        let mut resolution = [0.0; 6];
        let mut values = [0; 6];
        for &(code, info) in abs {
            if code <= ABS_RZ {
                // A missing resolution reads as one count per unit.
                resolution[usize::from(code)] = info.resolution.max(1) as f32;
                values[usize::from(code)] = info.value;
            }
        }
        let mut sensors = SensorSet::EMPTY;
        if resolution[..3].iter().all(|&r| r > 0.0) {
            sensors.insert(Sensor::Accelerometer);
        }
        if resolution[3..].iter().all(|&r| r > 0.0) {
            sensors.insert(Sensor::Gyroscope);
        }
        (!sensors.is_empty()).then_some(Self {
            resolution,
            values,
            sensors,
            clock: None,
            timestamp_us: 0,
        })
    }

    /// The sensors the node has.
    pub fn sensors(&self) -> SensorSet {
        self.sensors
    }

    /// Takes one event; returns a sample at the end of each report.
    pub fn event(&mut self, kind: u16, code: u16, value: i32) -> Option<MotionSample> {
        match (kind, code) {
            (EV_ABS, ABS_X..=ABS_RZ) => {
                self.values[usize::from(code)] = value;
                None
            }
            (EV_MSC, MSC_TIMESTAMP) => {
                let clock = value as u32;
                let elapsed = self.clock.map_or(0, |last| clock.wrapping_sub(last));
                self.timestamp_us += u64::from(elapsed);
                self.clock = Some(clock);
                None
            }
            (EV_SYN, SYN_REPORT) => Some(self.sample()),
            _ => None,
        }
    }

    fn sample(&self) -> MotionSample {
        let read = |i: usize| self.values[i] as f32 / self.resolution[i];
        let vector = |first: usize, scale: f32| {
            Vec3::new(
                read(first) * scale,
                read(first + 1) * scale,
                read(first + 2) * scale,
            )
        };
        MotionSample {
            timestamp_us: self.timestamp_us,
            accel: self
                .sensors
                .contains(Sensor::Accelerometer)
                .then(|| vector(0, STANDARD_GRAVITY)),
            gyro: self
                .sensors
                .contains(Sensor::Gyroscope)
                .then(|| vector(3, core::f32::consts::PI / 180.0)),
        }
    }
}

/// The touchpad node of a controller: the multitouch protocol B (slots
/// and tracking IDs) and the click on `BTN_LEFT`.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchpadNode {
    x: AbsInfo,
    y: AbsInfo,
    slot: usize,
    /// Tracking ID, X and Y of the finger in each slot.
    slots: [Option<(i32, i32, i32)>; MAX_TOUCH_POINTS],
    click: bool,
    info: TouchpadInfo,
}

impl TouchpadNode {
    /// A touchpad with these absolute axes; `None` without multitouch
    /// positions.
    pub fn new(abs: &[(u16, AbsInfo)]) -> Option<Self> {
        let find = |code: u16| abs.iter().find(|(c, _)| *c == code).map(|(_, info)| *info);
        let x = find(ABS_MT_POSITION_X)?;
        let y = find(ABS_MT_POSITION_Y)?;
        let slots = find(ABS_MT_SLOT).map_or(1, |slot| slot.maximum + 1);
        let size = |info: AbsInfo| u16::try_from(info.maximum - info.minimum + 1).unwrap_or(0);
        Some(Self {
            x,
            y,
            slot: 0,
            slots: [None; MAX_TOUCH_POINTS],
            click: false,
            info: TouchpadInfo {
                max_fingers: slots.clamp(1, MAX_TOUCH_POINTS as i32) as u8,
                width: size(x),
                height: size(y),
            },
        })
    }

    /// The touchpad, for the raw layout.
    pub fn info(&self) -> TouchpadInfo {
        self.info
    }

    /// Takes one event.
    pub fn event(&mut self, kind: u16, code: u16, value: i32) {
        match (kind, code) {
            (EV_KEY, BTN_LEFT) => self.click = value != 0,
            (EV_ABS, ABS_MT_SLOT) => self.slot = usize::try_from(value).unwrap_or(usize::MAX),
            (EV_ABS, ABS_MT_TRACKING_ID | ABS_MT_POSITION_X | ABS_MT_POSITION_Y) => {
                let Some(slot) = self.slots.get_mut(self.slot) else {
                    return;
                };
                match code {
                    ABS_MT_TRACKING_ID if value < 0 => *slot = None,
                    ABS_MT_TRACKING_ID => {
                        let (x, y) = slot.map_or((0, 0), |(_, x, y)| (x, y));
                        *slot = Some((value, x, y));
                    }
                    ABS_MT_POSITION_X => {
                        if let Some(finger) = slot {
                            finger.1 = value;
                        }
                    }
                    _ => {
                        if let Some(finger) = slot {
                            finger.2 = value;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Whether the touchpad is clicked.
    pub fn clicked(&self) -> bool {
        self.click
    }

    /// Writes the fingers down, normalized, into `touches`.
    pub fn fill(&self, touches: &mut [Option<TouchPoint>; MAX_TOUCH_POINTS]) {
        let scale = |value: i32, info: AbsInfo| {
            let span = (info.maximum - info.minimum) as f32;
            if span > 0.0 {
                ((value - info.minimum) as f32 / span).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        for (touch, slot) in touches.iter_mut().zip(&self.slots) {
            *touch = slot.map(|(id, x, y)| TouchPoint {
                touchpad: 0,
                id: (id & 0x7f) as u8,
                x: scale(x, self.x),
                y: scale(y, self.y),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::GamepadState;

    use super::*;
    use crate::mapping::Mapper;

    fn info(minimum: i32, maximum: i32) -> AbsInfo {
        AbsInfo {
            minimum,
            maximum,
            ..AbsInfo::default()
        }
    }

    /// The gamepad node of a DualSense under `hid-playstation`.
    fn dualsense() -> (Vec<u16>, Vec<(u16, AbsInfo)>) {
        let keys = vec![
            BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_TL2, BTN_TR2, BTN_SELECT,
            BTN_START, BTN_MODE, BTN_THUMBL, BTN_THUMBR,
        ];
        let mut abs = vec![(ABS_HAT0X, info(-1, 1)), (ABS_HAT0Y, info(-1, 1))];
        for code in [ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ] {
            abs.push((code, info(0, 255)));
        }
        (keys, abs)
    }

    #[test]
    fn classifies_nodes() {
        let (keys, abs) = dualsense();
        let codes: Vec<u16> = abs.iter().map(|(c, _)| *c).collect();
        assert_eq!(classify(&keys, &codes, &[]), NodeKind::Gamepad);
        assert_eq!(
            classify(&[], &[ABS_X, ABS_Y, ABS_Z], &[INPUT_PROP_ACCELEROMETER]),
            NodeKind::Motion
        );
        let touch = [
            ABS_X,
            ABS_Y,
            ABS_MT_SLOT,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
        ];
        assert_eq!(
            classify(&[BTN_LEFT, BTN_TOUCH], &touch, &[INPUT_PROP_BUTTONPAD]),
            NodeKind::Touchpad
        );
        // A keyboard, a mouse.
        assert_eq!(classify(&[1, 2, 30, 0x1c], &[], &[]), NodeKind::Other);
        assert_eq!(classify(&[BTN_LEFT, 0x111], &[], &[]), NodeKind::Other);
        // Extra buttons only.
        assert_eq!(classify(&[BTN_TRIGGER_HAPPY], &[], &[]), NodeKind::Gamepad);
    }

    #[test]
    fn parses_sysfs_bitmaps() {
        // BTN_SOUTH (0x130 = 304 = word 4, bit 48) and KEY_ESC (1).
        let key = "1000000000000 0 0 0 2";
        let mut expected = vec![1, 0x130];
        assert_eq!(parse_bitmap(key, 64), Some(expected.clone()));
        // The same on a 32-bit kernel: word 9, bit 16.
        let key32 = "10000 0 0 0 0 0 0 0 0 2";
        assert_eq!(parse_bitmap(key32, 32), Some(expected.clone()));
        expected.clear();
        assert_eq!(parse_bitmap("0", 64), Some(expected));
        assert_eq!(parse_bitmap("zz", 64), None);
        assert_eq!(parse_bitmap("40", 64), Some(vec![INPUT_PROP_ACCELEROMETER]));
    }

    #[test]
    fn gamepad_layout_and_state() {
        let (keys, abs) = dualsense();
        let node = GamepadNode::new(&keys, &abs);
        let layout = node.layout();
        assert_eq!(layout.buttons.len(), 13);
        assert_eq!(layout.buttons[0], RawSource::Evdev(BTN_SOUTH));
        assert_eq!(layout.axes.len(), 6);
        assert_eq!(layout.axes[1].source, RawSource::Evdev(ABS_Y));
        assert_eq!((layout.axes[1].min, layout.axes[1].max), (0.0, 255.0));
        assert_eq!(layout.hats, [RawSource::Evdev(ABS_HAT0X)]);

        let mut state = RawState::for_layout(layout);
        node.fill(
            |code| code == BTN_EAST,
            |code| match code {
                ABS_Y => 0,
                ABS_HAT0X => 1,
                ABS_HAT0Y => -1,
                _ => 128,
            },
            &mut state,
        );
        assert_eq!(state.buttons[1], 1.0);
        assert_eq!(state.buttons[0], 0.0);
        assert_eq!(state.axes[1], 0.0);
        assert_eq!(state.hats[0], Hat(Hat::UP | Hat::RIGHT));
    }

    #[test]
    fn spec_mapping_reads_a_dualsense() {
        let (keys, abs) = dualsense();
        let node = GamepadNode::new(&keys, &abs);
        let mapping = spec_mapping(node.layout(), Some("playstation"));
        let mapper = Mapper::new(&mapping, node.layout());
        assert!(mapper.unresolved().is_empty());

        let mut raw = RawState::for_layout(node.layout());
        node.fill(
            |code| code == BTN_SOUTH || code == BTN_MODE,
            |code| match code {
                ABS_Y => 0,     // up
                ABS_RX => 255,  // right
                ABS_Z => 255,   // left trigger in
                ABS_HAT0Y => 1, // d-pad down
                ABS_HAT0X => 0,
                _ => 128,
            },
            &mut raw,
        );
        let mut state = GamepadState::new();
        mapper.apply(&raw, &mut state);
        assert!(state.pressed(Button::South) && state.pressed(Button::Home));
        assert!(state.pressed(Button::DPadDown) && !state.pressed(Button::DPadUp));
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);
        assert_eq!(state.axis(Axis::RightStickX), 1.0);
        assert_eq!(state.axis(Axis::LeftTrigger), 1.0);
        assert!(state.pressed(Button::LeftTrigger));
        assert!(state.axis(Axis::LeftStickX).abs() < 0.01);
    }

    #[test]
    fn spec_mapping_without_analog_triggers_or_hat() {
        // A Switch Pro Controller under hid-nintendo: digital ZL/ZR, d-pad
        // buttons.
        let keys = [
            BTN_SOUTH,
            BTN_EAST,
            BTN_NORTH,
            BTN_WEST,
            BTN_TL,
            BTN_TR,
            BTN_TL2,
            BTN_TR2,
            BTN_DPAD_UP,
            BTN_DPAD_DOWN,
            BTN_DPAD_LEFT,
            BTN_DPAD_RIGHT,
        ];
        let abs: Vec<_> = [ABS_X, ABS_Y, ABS_RX, ABS_RY]
            .map(|code| (code, info(-32767, 32767)))
            .into();
        let node = GamepadNode::new(&keys, &abs);
        let mapping = spec_mapping(node.layout(), Some("nintendo"));
        assert_eq!(
            mapping.buttons.get(&Button::North),
            Some(&Input::Button(RawSource::Evdev(BTN_NORTH)))
        );
        let xpad = spec_mapping(node.layout(), Some("xpad"));
        assert_eq!(
            xpad.buttons.get(&Button::North),
            Some(&Input::Button(RawSource::Evdev(BTN_WEST)))
        );
        assert_eq!(
            mapping.buttons.get(&Button::LeftTrigger),
            Some(&Input::Button(RawSource::Evdev(BTN_TL2)))
        );
        assert_eq!(
            mapping.buttons.get(&Button::DPadLeft),
            Some(&Input::Button(RawSource::Evdev(BTN_DPAD_LEFT)))
        );
        assert!(!mapping.axes.contains_key(&Axis::LeftTrigger));
        assert!(follows_spec(Some("nintendo")));
        assert!(!follows_spec(Some("hid-generic")) && !follows_spec(None));
    }

    #[test]
    fn motion_samples() {
        let mut abs: Vec<(u16, AbsInfo)> = [ABS_X, ABS_Y, ABS_Z]
            .map(|code| {
                (
                    code,
                    AbsInfo {
                        resolution: 8192,
                        ..info(-32768, 32767)
                    },
                )
            })
            .into();
        abs.extend([ABS_RX, ABS_RY, ABS_RZ].map(|code| {
            (
                code,
                AbsInfo {
                    resolution: 1024,
                    ..info(-2_097_152, 2_097_152)
                },
            )
        }));
        let mut node = MotionNode::new(&abs).unwrap();
        assert_eq!(node.sensors(), SensorSet::ALL);
        for (kind, code, value) in [
            (EV_ABS, ABS_Y, 8192),
            (EV_ABS, ABS_RZ, -1024 * 90),
            (EV_MSC, MSC_TIMESTAMP, u32::MAX as i32 - 99),
        ] {
            assert_eq!(node.event(kind, code, value), None);
        }
        let first = node.event(EV_SYN, SYN_REPORT, 0).unwrap();
        assert_eq!(first.timestamp_us, 0);
        assert_eq!(first.accel, Some(Vec3::new(0.0, STANDARD_GRAVITY, 0.0)));
        let gyro = first.gyro.unwrap();
        assert!((gyro.z + core::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // The clock wraps.
        node.event(EV_MSC, MSC_TIMESTAMP, 900);
        let second = node.event(EV_SYN, SYN_REPORT, 0).unwrap();
        assert_eq!(second.timestamp_us, 1000);

        assert_eq!(MotionNode::new(&[]), None);
    }

    #[test]
    fn touchpad_slots() {
        let abs = [
            (ABS_MT_SLOT, info(0, 1)),
            (ABS_MT_POSITION_X, info(0, 1919)),
            (ABS_MT_POSITION_Y, info(0, 1079)),
            (ABS_MT_TRACKING_ID, info(0, 65535)),
        ];
        let mut node = TouchpadNode::new(&abs).unwrap();
        assert_eq!(
            node.info(),
            TouchpadInfo {
                max_fingers: 2,
                width: 1920,
                height: 1080
            }
        );
        for (kind, code, value) in [
            (EV_ABS, ABS_MT_SLOT, 1),
            (EV_ABS, ABS_MT_TRACKING_ID, 130),
            (EV_ABS, ABS_MT_POSITION_X, 1919),
            (EV_ABS, ABS_MT_POSITION_Y, 0),
            (EV_KEY, BTN_LEFT, 1),
            // Out-of-range slots are ignored.
            (EV_ABS, ABS_MT_SLOT, 9),
            (EV_ABS, ABS_MT_TRACKING_ID, 4),
        ] {
            node.event(kind, code, value);
        }
        let mut touches = [None; MAX_TOUCH_POINTS];
        node.fill(&mut touches);
        assert_eq!(touches[0], None);
        assert_eq!(
            touches[1],
            Some(TouchPoint {
                touchpad: 0,
                id: 2,
                x: 1.0,
                y: 0.0
            })
        );
        assert!(node.clicked());
        node.event(EV_ABS, ABS_MT_SLOT, 1);
        node.event(EV_ABS, ABS_MT_TRACKING_ID, -1);
        node.fill(&mut touches);
        assert_eq!(touches[1], None);
        assert_eq!(TouchpadNode::new(&abs[..1]), None);
    }

    #[test]
    fn transports() {
        assert_eq!(transport(BUS_USB), Transport::Usb);
        assert_eq!(transport(BUS_BLUETOOTH), Transport::Bluetooth);
        assert_eq!(transport(BUS_VIRTUAL), Transport::Unknown);
    }
}
