//! The mapping engine: applies a device profile's [`Mapping`] to the raw
//! elements a provider reads, producing the canonical [`GamepadState`].
//!
//! A [`Mapper`] is built once per device, from the mapping its profile gives
//! for its identity ([`Profile::mapping_for`](anygamepad_db::Profile::mapping_for))
//! and the elements its provider reads ([`RawLayout`]). On every poll,
//! [`Mapper::apply`] turns the [`RawState`] into the canonical state without
//! allocating. The engine is pure and deterministic, and runs unchanged in
//! WebAssembly.
//!
//! ```
//! use anygamepad::db::{AxisMapping, Mapping};
//! use anygamepad::mapping::Mapper;
//! use anygamepad::model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
//! use anygamepad::model::{Axis, Button, GamepadState};
//!
//! // A provider that lists two buttons and one axis by index.
//! let layout = RawLayout {
//!     buttons: vec![RawSource::Index(0), RawSource::Index(1)],
//!     axes: vec![RawAxisInfo { source: RawSource::Index(0), min: 0.0, max: 255.0 }],
//!     ..RawLayout::default()
//! };
//! // Button 1 is the bottom face button; axis 0 is the left stick's Y,
//! // growing downwards.
//! let mut mapping = Mapping::default();
//! mapping.buttons.insert(Button::South, "b1".parse()?);
//! mapping.axes.insert(Axis::LeftStickY, AxisMapping::full("a0~".parse()?));
//! let mapper = Mapper::new(&mapping, &layout);
//!
//! let mut raw = RawState::for_layout(&layout);
//! raw.buttons[1] = 1.0;
//! raw.axes[0] = 0.0; // pushed all the way up
//! let mut state = GamepadState::new();
//! mapper.apply(&raw, &mut state);
//! assert!(state.pressed(Button::South));
//! assert_eq!(state.axis(Axis::LeftStickY), 1.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # How inputs are read
//!
//! The semantics are those of the profile format ([`Input`], [`Mapping`]):
//! every input reads as a position `t` in `[0, 1]`. The engine adds what the
//! format leaves to it:
//!
//! - An input names a raw element of its kind (button, axis or hat) by its
//!   [`RawSource`]: `b3` is the button the layout lists as
//!   `RawSource::Index(3)`, `b{evdev:0x0130}` the one it lists as
//!   `RawSource::Evdev(0x130)`. Inputs that name no element of the layout,
//!   or an axis whose range is empty, are left out
//!   ([`Mapper::unresolved`]).
//! - An axis reads over the range its [`RawAxisInfo`] declares, centred
//!   halfway between `min` and `max`. A whole stick axis read from an axis
//!   whose range is symmetric (`-1` to `1`) is that axis, exactly.
//! - A `b` input presses its button from [`PRESS_THRESHOLD`] on.
//! - Halves of a stick axis add up: both pressed at once read as rest.
//! - A deadzone `d` reads `|v| <= d` as rest and scales the rest of the
//!   travel to start from there: `(|v| - d) / (1 - d)`, keeping the sign.
//! - NaN raw values read as rest.
//!
//! And two defaults, which the mapping overrides by setting the target:
//!
//! - Elements whose source is already canonical ([`RawSource::Button`],
//!   [`RawSource::Axis`]) drive that control, so backends that normalize
//!   (XInput, gilrs) need no profile. Canonical axes read over their range
//!   like any other, so their Y must grow upwards.
//! - A trigger button follows its trigger axis, pressed from
//!   [`PRESS_THRESHOLD`] on. SDL GameControllerDB entries map triggers as
//!   axes only.
//!
//! Touches, power and the latest motion sample are copied from the raw
//! state: providers deliver them in canonical units.

use std::collections::BTreeMap;

use anygamepad_db::{AxisMapping, AxisRange, Input, Mapping};
use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad_model::{Axis, Button, Capabilities, GamepadState, OutputSet};

/// A button counts as pressed from this value on: Chromium's threshold for
/// analog buttons (30/255), which is also XInput's trigger threshold.
pub const PRESS_THRESHOLD: f32 = 30.0 / 255.0;

/// A control of the canonical model that a mapping drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A button.
    Button(Button),
    /// An axis.
    Axis(Axis),
}

/// An [`Input`] resolved against a layout: where its value is in a
/// [`RawState`], and how to read it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Button(usize),
    Axis {
        index: usize,
        min: f32,
        max: f32,
        range: AxisRange,
        inverted: bool,
    },
    Hat {
        index: usize,
        mask: u8,
    },
}

impl Source {
    /// The element `input` names in `layout`, if there is one that can be
    /// read.
    fn resolve(input: Input, layout: &RawLayout) -> Option<Self> {
        let find = |sources: &[RawSource], source| sources.iter().position(|&s| s == source);
        match input {
            Input::Button(source) => find(&layout.buttons, source).map(Self::Button),
            Input::Axis {
                source,
                range,
                inverted,
            } => {
                let index = layout.axes.iter().position(|axis| axis.source == source)?;
                let RawAxisInfo { min, max, .. } = layout.axes[index];
                // An empty or infinite range cannot be read.
                if !(max - min).is_finite() || min == max {
                    return None;
                }
                Some(Self::Axis {
                    index,
                    min,
                    max,
                    range,
                    inverted,
                })
            }
            Input::Hat { source, mask } => {
                find(&layout.hats, source).map(|index| Self::Hat { index, mask })
            }
        }
    }

    /// The input's position `t` in `[0, 1]`; `None` while it is inactive
    /// (half an axis while the axis is on the other half), and for missing
    /// or NaN values.
    fn position(self, raw: &RawState) -> Option<f32> {
        let (t, inverted) = match self {
            Self::Button(index) => (*raw.buttons.get(index)?, false),
            Self::Axis {
                index,
                min,
                max,
                range,
                inverted,
            } => {
                let value = *raw.axes.get(index)?;
                let t = match range {
                    AxisRange::Full => (value - min) / (max - min),
                    AxisRange::Positive | AxisRange::Negative => {
                        let end = if range == AxisRange::Positive {
                            max
                        } else {
                            min
                        };
                        let centre = (min + max) / 2.0;
                        let t = (value - centre) / (end - centre);
                        if t < 0.0 {
                            return None;
                        }
                        t
                    }
                };
                (t, inverted)
            }
            Self::Hat { index, mask } => {
                let pointing = raw.hats.get(index)?.has(mask);
                (if pointing { 1.0 } else { 0.0 }, false)
            }
        };
        if t.is_nan() {
            return None;
        }
        let t = t.clamp(0.0, 1.0);
        Some(if inverted { 1.0 - t } else { t })
    }

    /// The input as a whole stick axis, `2t - 1`. Computed directly for
    /// whole axes, so that an axis with a symmetric range reads as itself.
    fn signed(self, raw: &RawState) -> Option<f32> {
        match self {
            Self::Axis {
                index,
                min,
                max,
                range: AxisRange::Full,
                inverted,
            } => {
                let value = *raw.axes.get(index)?;
                let s = (2.0 * value - (min + max)) / (max - min);
                if s.is_nan() {
                    return None;
                }
                let s = s.clamp(-1.0, 1.0);
                Some(if inverted { -s } else { s })
            }
            _ => self.position(raw).map(|t| 2.0 * t - 1.0),
        }
    }

    /// Whether the input presses a button target.
    fn pressed(self, raw: &RawState) -> bool {
        match self {
            Self::Button(index) => raw
                .buttons
                .get(index)
                .is_some_and(|&value| value >= PRESS_THRESHOLD),
            _ => self.position(raw).is_some_and(|t| t >= 0.5),
        }
    }
}

/// What drives an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drive {
    /// One input over the whole axis.
    Whole(Source),
    /// Inputs for each half; at least one is set.
    Halves {
        negative: Option<Source>,
        positive: Option<Source>,
    },
}

/// An axis target and how it is driven.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AxisBinding {
    axis: Axis,
    drive: Drive,
    /// In `[0, 1)`; 0 for none.
    deadzone: f32,
}

impl AxisBinding {
    fn value(&self, raw: &RawState) -> f32 {
        let value = match self.drive {
            Drive::Whole(source) if self.axis.is_trigger() => source.position(raw),
            Drive::Whole(source) => source.signed(raw),
            Drive::Halves { negative, positive } => {
                let half = |source: Option<Source>| {
                    source
                        .and_then(|source| source.position(raw))
                        .unwrap_or(0.0)
                };
                Some(half(positive) - half(negative))
            }
        };
        // Inactive inputs leave the axis at rest.
        apply_deadzone(value.unwrap_or(0.0), self.deadzone)
    }
}

/// Reads `|value| <= deadzone` as rest and scales the rest of the travel to
/// start from there. A deadzone of 0 leaves the value as it is, except -0,
/// which becomes 0.
fn apply_deadzone(value: f32, deadzone: f32) -> f32 {
    let magnitude = value.abs();
    if magnitude <= deadzone {
        0.0
    } else {
        value.signum() * (magnitude - deadzone) / (1.0 - deadzone)
    }
}

/// The trigger buttons and the axes they follow when a mapping leaves them
/// unset.
const TRIGGERS: [(Button, Axis); 2] = [
    (Button::LeftTrigger, Axis::LeftTrigger),
    (Button::RightTrigger, Axis::RightTrigger),
];

/// A mapping resolved against a device's raw layout, ready to turn its raw
/// states into canonical ones. See the [module documentation](self).
#[derive(Clone, Debug, PartialEq)]
pub struct Mapper {
    buttons: Vec<(Button, Source)>,
    axes: Vec<AxisBinding>,
    /// Trigger buttons that follow their axis.
    derived: Vec<(Button, Axis)>,
    capabilities: Capabilities,
    unresolved: Vec<(Target, Input)>,
}

impl Mapper {
    /// Resolves `mapping` against the elements of `layout`.
    ///
    /// Canonical sources in the layout drive their own control unless
    /// `mapping` sets it. An axis mapping with an `input` ignores its
    /// halves, and a deadzone outside `[0, 1)` is ignored: profiles that
    /// pass [`Profile::validate`](anygamepad_db::Profile::validate) have
    /// neither.
    pub fn new(mapping: &Mapping, layout: &RawLayout) -> Self {
        let mut button_inputs: BTreeMap<Button, Input> = layout
            .buttons
            .iter()
            .filter_map(|&source| match source {
                RawSource::Button(button) => Some((button, Input::Button(source))),
                _ => None,
            })
            .collect();
        button_inputs.extend(&mapping.buttons);
        let unset_triggers: Vec<(Button, Axis)> = TRIGGERS
            .into_iter()
            .filter(|(button, _)| !button_inputs.contains_key(button))
            .collect();

        let mut axis_mappings: BTreeMap<Axis, AxisMapping> = layout
            .axes
            .iter()
            .filter_map(|info| match info.source {
                RawSource::Axis(axis) => Some((
                    axis,
                    AxisMapping::full(Input::Axis {
                        source: info.source,
                        range: AxisRange::Full,
                        inverted: false,
                    }),
                )),
                _ => None,
            })
            .collect();
        axis_mappings.extend(&mapping.axes);

        let mut unresolved = Vec::new();
        let mut resolve = |target: Target, input: Input| {
            let source = Source::resolve(input, layout);
            if source.is_none() {
                unresolved.push((target, input));
            }
            source
        };

        let mut buttons = Vec::with_capacity(button_inputs.len());
        for (button, input) in button_inputs {
            if let Some(source) = resolve(Target::Button(button), input) {
                buttons.push((button, source));
            }
        }

        let mut axes = Vec::with_capacity(axis_mappings.len());
        for (axis, axis_mapping) in axis_mappings {
            let target = Target::Axis(axis);
            let drive = match axis_mapping.input {
                Some(input) => resolve(target, input).map(Drive::Whole),
                None => {
                    let negative = axis_mapping.negative.and_then(|i| resolve(target, i));
                    let positive = axis_mapping.positive.and_then(|i| resolve(target, i));
                    (negative.is_some() || positive.is_some())
                        .then_some(Drive::Halves { negative, positive })
                }
            };
            if let Some(drive) = drive {
                let deadzone = axis_mapping
                    .deadzone
                    .filter(|deadzone| (0.0..1.0).contains(deadzone))
                    .unwrap_or(0.0);
                axes.push(AxisBinding {
                    axis,
                    drive,
                    deadzone,
                });
            }
        }

        let derived: Vec<(Button, Axis)> = unset_triggers
            .into_iter()
            .filter(|&(_, axis)| axes.iter().any(|binding| binding.axis == axis))
            .collect();

        let capabilities = Capabilities {
            buttons: buttons
                .iter()
                .map(|&(button, _)| button)
                .chain(derived.iter().map(|&(button, _)| button))
                .collect(),
            axes: axes.iter().map(|binding| binding.axis).collect(),
            sensors: layout.sensors,
            touchpads: layout.touchpads.clone(),
            outputs: OutputSet::EMPTY,
            battery: layout.battery,
        };

        Self {
            buttons,
            axes,
            derived,
            capabilities,
            unresolved,
        }
    }

    /// Writes the canonical state of `raw` into `state`.
    ///
    /// Controls the mapping does not drive are at rest. `state.motion`
    /// keeps its sample when `raw` brings none, so pass the device's state
    /// from the previous poll. A `raw` that does not fit the layout reads
    /// its missing elements as rest.
    pub fn apply(&self, raw: &RawState, state: &mut GamepadState) {
        let mut next = GamepadState::new();
        for &(button, source) in &self.buttons {
            if source.pressed(raw) {
                next.set_pressed(button, true);
            }
        }
        for binding in &self.axes {
            next.set_axis(binding.axis, binding.value(raw));
        }
        for &(button, axis) in &self.derived {
            if next.axis(axis) >= PRESS_THRESHOLD {
                next.set_pressed(button, true);
            }
        }
        next.touches = raw.touches;
        next.motion = raw.motion.last().copied().or(state.motion);
        next.power = raw.power;
        *state = next;
    }

    /// The controls the mapping drives, and the sensors, touchpads and
    /// battery of the layout.
    ///
    /// Outputs are not part of a mapping: `outputs` is empty, and the
    /// provider's [`DeviceInfo::outputs`](crate::provider::DeviceInfo::outputs)
    /// says which it can drive.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Inputs the layout cannot satisfy, with the control they were for: the
    /// raw element they name is missing, or is an axis with an empty range.
    /// A profile that fits the device has none.
    pub fn unresolved(&self) -> &[(Target, Input)] {
        &self.unresolved
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::raw::Hat;
    use anygamepad_model::{
        MotionSample, Power, PowerState, Sensor, SensorSet, TouchPoint, TouchpadInfo, Vec3,
    };

    use super::*;

    fn input(text: &str) -> Input {
        text.parse().unwrap()
    }

    fn index(index: u16) -> RawSource {
        RawSource::Index(index)
    }

    fn axis_info(source: RawSource, min: f32, max: f32) -> RawAxisInfo {
        RawAxisInfo { source, min, max }
    }

    /// `count` buttons, axes over `min..max` and hats, all by index.
    fn layout(buttons: u16, axes: &[(f32, f32)], hats: u16) -> RawLayout {
        RawLayout {
            buttons: (0..buttons).map(index).collect(),
            axes: (0..)
                .zip(axes)
                .map(|(i, &(min, max))| axis_info(index(i), min, max))
                .collect(),
            hats: (0..hats).map(index).collect(),
            ..RawLayout::default()
        }
    }

    fn mapping(buttons: &[(Button, &str)], axes: &[(Axis, AxisMapping)]) -> Mapping {
        Mapping {
            buttons: buttons.iter().map(|&(b, text)| (b, input(text))).collect(),
            axes: axes.iter().copied().collect(),
            ..Mapping::default()
        }
    }

    fn whole(text: &str) -> AxisMapping {
        AxisMapping::full(input(text))
    }

    fn halves(negative: &str, positive: &str) -> AxisMapping {
        AxisMapping {
            negative: Some(input(negative)),
            positive: Some(input(positive)),
            ..AxisMapping::default()
        }
    }

    fn map(mapper: &Mapper, raw: &RawState) -> GamepadState {
        let mut state = GamepadState::new();
        mapper.apply(raw, &mut state);
        state
    }

    /// The value of `axis` mapped from `axes` raw values.
    fn axis_value(mapper: &Mapper, layout: &RawLayout, axes: &[f32], axis: Axis) -> f32 {
        let mut raw = RawState::for_layout(layout);
        raw.axes.copy_from_slice(axes);
        map(mapper, &raw).axis(axis)
    }

    #[test]
    fn whole_axes_over_their_range() {
        for (min, max, rest, up, down) in [
            (0.0, 255.0, 127.5, 0.0, 255.0),
            (-32768.0, 32767.0, -0.5, -32768.0, 32767.0),
            (-1.0, 1.0, 0.0, -1.0, 1.0),
            // A provider that reports its range reversed.
            (255.0, 0.0, 127.5, 255.0, 0.0),
        ] {
            let layout = layout(0, &[(min, max)], 0);
            let mapper = Mapper::new(
                &mapping(
                    &[],
                    &[
                        (Axis::LeftStickY, whole("a0~")),
                        (Axis::LeftTrigger, whole("a0")),
                    ],
                ),
                &layout,
            );
            let value = |raw: f32, axis| axis_value(&mapper, &layout, &[raw], axis);
            assert_eq!(value(rest, Axis::LeftStickY), 0.0, "{min}..{max}");
            assert_eq!(value(up, Axis::LeftStickY), 1.0, "{min}..{max}");
            assert_eq!(value(down, Axis::LeftStickY), -1.0, "{min}..{max}");
            assert_eq!(value(up, Axis::LeftTrigger), 0.0, "{min}..{max}");
            assert_eq!(value(down, Axis::LeftTrigger), 1.0, "{min}..{max}");
            assert_eq!(value(rest, Axis::LeftTrigger), 0.5, "{min}..{max}");
        }
    }

    #[test]
    fn symmetric_axes_read_as_themselves() {
        let layout = layout(0, &[(-1.0, 1.0)], 0);
        let mapper = Mapper::new(
            &mapping(
                &[],
                &[
                    (Axis::LeftStickX, whole("a0")),
                    (Axis::LeftStickY, whole("a0~")),
                ],
            ),
            &layout,
        );
        for value in [0.0, 0.3, -0.7, 1.0 / 3.0, 1e-30, -0.999_999_9] {
            let mut raw = RawState::for_layout(&layout);
            raw.axes[0] = value;
            let state = map(&mapper, &raw);
            assert_eq!(state.axis(Axis::LeftStickX), value);
            assert_eq!(state.axis(Axis::LeftStickY), 0.0 - value);
        }
        // Rest is +0, also inverted.
        let rest = map(&mapper, &RawState::for_layout(&layout));
        assert!(rest.axis(Axis::LeftStickY).is_sign_positive());
        let mut raw = RawState::for_layout(&layout);
        raw.axes[0] = -0.0;
        assert!(map(&mapper, &raw).axis(Axis::LeftStickX).is_sign_positive());
    }

    #[test]
    fn combined_trigger_axis() {
        // Xbox pads in DirectInput mode: both triggers on one axis, the left
        // one towards the maximum, the right one towards the minimum.
        let layout = layout(0, &[(0.0, 65535.0)], 0);
        let mapper = Mapper::new(
            &mapping(
                &[],
                &[
                    (Axis::LeftTrigger, whole("+a0")),
                    (Axis::RightTrigger, whole("-a0")),
                ],
            ),
            &layout,
        );
        let triggers = |value: f32| {
            let mut raw = RawState::for_layout(&layout);
            raw.axes[0] = value;
            let state = map(&mapper, &raw);
            (
                state.axis(Axis::LeftTrigger),
                state.axis(Axis::RightTrigger),
                state.pressed(Button::LeftTrigger),
                state.pressed(Button::RightTrigger),
            )
        };
        assert_eq!(triggers(32767.5), (0.0, 0.0, false, false));
        assert_eq!(triggers(65535.0), (1.0, 0.0, true, false));
        assert_eq!(triggers(0.0), (0.0, 1.0, false, true));
        let (left, right, _, _) = triggers(32767.5 + 32767.5 / 2.0);
        assert_eq!((left, right), (0.5, 0.0));
        assert!(mapper.capabilities().has_button(Button::LeftTrigger));
    }

    #[test]
    fn inverted_halves() {
        let layout = layout(0, &[(-1.0, 1.0)], 0);
        let mapper = Mapper::new(
            &mapping(&[], &[(Axis::RightTrigger, whole("+a0~"))]),
            &layout,
        );
        let value = |raw: f32| axis_value(&mapper, &layout, &[raw], Axis::RightTrigger);
        assert_eq!(value(0.0), 1.0);
        assert_eq!(value(1.0), 0.0);
        assert_eq!(value(0.25), 0.75);
        // Inactive on the other half: at rest, not reversed.
        assert_eq!(value(-0.5), 0.0);
    }

    #[test]
    fn stick_halves_add_up() {
        let layout = layout(0, &[(-1.0, 1.0), (-1.0, 1.0)], 0);
        let mapper = Mapper::new(
            &mapping(&[], &[(Axis::RightStickX, halves("-a0", "+a1"))]),
            &layout,
        );
        let value = |axes: [f32; 2]| axis_value(&mapper, &layout, &axes, Axis::RightStickX);
        assert_eq!(value([0.0, 0.0]), 0.0);
        assert_eq!(value([-0.5, 0.0]), -0.5);
        assert_eq!(value([0.0, 0.25]), 0.25);
        assert_eq!(value([-1.0, 1.0]), 0.0);
        // Each half reads only its side.
        assert_eq!(value([0.5, -0.5]), 0.0);
    }

    #[test]
    fn hats_drive_the_dpad_and_sticks() {
        let layout = layout(0, &[], 1);
        let mapper = Mapper::new(
            &mapping(
                &[
                    (Button::DPadUp, "h0.1"),
                    (Button::DPadRight, "h0.2"),
                    (Button::DPadDown, "h0.4"),
                    (Button::DPadLeft, "h0.8"),
                ],
                &[
                    (Axis::LeftStickX, halves("h0.8", "h0.2")),
                    (Axis::LeftStickY, halves("h0.4", "h0.1")),
                ],
            ),
            &layout,
        );
        let mut raw = RawState::for_layout(&layout);
        raw.hats[0] = Hat(Hat::UP | Hat::LEFT);
        let state = map(&mapper, &raw);
        assert!(state.pressed(Button::DPadUp));
        assert!(state.pressed(Button::DPadLeft));
        assert!(!state.pressed(Button::DPadDown));
        assert!(!state.pressed(Button::DPadRight));
        assert_eq!(state.axis(Axis::LeftStickX), -1.0);
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);

        raw.hats[0] = Hat::CENTERED;
        let state = map(&mapper, &raw);
        assert!(state.buttons.is_empty());
        assert_eq!(state.left_stick().magnitude(), 0.0);
    }

    #[test]
    fn button_targets() {
        let layout = layout(2, &[(-1.0, 1.0), (0.0, 1.0)], 0);
        let mapper = Mapper::new(
            &mapping(
                &[
                    (Button::South, "b0"),
                    (Button::DPadUp, "-a0"),
                    (Button::DPadDown, "+a0"),
                    (Button::LeftShoulder, "a1"),
                ],
                &[],
            ),
            &layout,
        );
        let pressed = |buttons: [f32; 2], axes: [f32; 2], button| {
            let mut raw = RawState::for_layout(&layout);
            raw.buttons.copy_from_slice(&buttons);
            raw.axes.copy_from_slice(&axes);
            map(&mapper, &raw).pressed(button)
        };
        // Button inputs press at the analog threshold.
        assert!(pressed([PRESS_THRESHOLD, 0.0], [0.0, 0.0], Button::South));
        assert!(!pressed([0.1, 0.0], [0.0, 0.0], Button::South));
        // Axis inputs press halfway through their range.
        assert!(pressed([0.0; 2], [-0.5, 0.0], Button::DPadUp));
        assert!(!pressed([0.0; 2], [-0.49, 0.0], Button::DPadUp));
        assert!(!pressed([0.0; 2], [-0.5, 0.0], Button::DPadDown));
        assert!(pressed([0.0; 2], [0.0, 0.5], Button::LeftShoulder));
        assert!(!pressed([0.0; 2], [0.0, 0.4], Button::LeftShoulder));
    }

    #[test]
    fn buttons_drive_axes() {
        let layout = layout(3, &[], 0);
        let mapper = Mapper::new(
            &mapping(
                &[],
                &[
                    (Axis::LeftTrigger, whole("b0")),
                    (Axis::LeftStickX, halves("b1", "b2")),
                ],
            ),
            &layout,
        );
        let mut raw = RawState::for_layout(&layout);
        raw.buttons = vec![0.6, 0.0, 1.0];
        let state = map(&mapper, &raw);
        assert_eq!(state.axis(Axis::LeftTrigger), 0.6);
        assert_eq!(state.axis(Axis::LeftStickX), 1.0);
        raw.buttons = vec![0.0, 1.0, 1.0];
        assert_eq!(map(&mapper, &raw).axis(Axis::LeftStickX), 0.0);
    }

    #[test]
    fn deadzones_rescale_the_travel() {
        let layout = layout(0, &[(-1.0, 1.0), (0.0, 1.0)], 0);
        let with_deadzone = |text: &str| AxisMapping {
            deadzone: Some(0.2),
            ..whole(text)
        };
        let mapper = Mapper::new(
            &mapping(
                &[],
                &[
                    (Axis::LeftStickX, with_deadzone("a0")),
                    (Axis::RightTrigger, with_deadzone("a1")),
                ],
            ),
            &layout,
        );
        let values = |stick: f32, trigger: f32| {
            let mut raw = RawState::for_layout(&layout);
            raw.axes = vec![stick, trigger];
            let state = map(&mapper, &raw);
            (state.axis(Axis::LeftStickX), state.axis(Axis::RightTrigger))
        };
        assert_eq!(values(0.2, 0.2), (0.0, 0.0));
        assert_eq!(values(-0.1, 0.1), (0.0, 0.0));
        assert_eq!(values(1.0, 1.0), (1.0, 1.0));
        assert_eq!(values(-1.0, 0.0), (-1.0, 0.0));
        let (stick, trigger) = values(-0.6, 0.6);
        assert!((stick + 0.5).abs() < 1e-6, "{stick}");
        assert!((trigger - 0.5).abs() < 1e-6, "{trigger}");
        // The trigger button follows the axis after its deadzone.
        let mut raw = RawState::for_layout(&layout);
        raw.axes = vec![0.0, 0.25];
        assert!(!map(&mapper, &raw).pressed(Button::RightTrigger));
        raw.axes = vec![0.0, 0.4];
        assert!(map(&mapper, &raw).pressed(Button::RightTrigger));

        // Out-of-range deadzones are ignored.
        let ignored = Mapper::new(
            &mapping(
                &[],
                &[(
                    Axis::LeftStickX,
                    AxisMapping {
                        deadzone: Some(1.0),
                        ..whole("a0")
                    },
                )],
            ),
            &layout,
        );
        assert_eq!(
            axis_value(&ignored, &layout, &[0.5, 0.0], Axis::LeftStickX),
            0.5
        );
    }

    #[test]
    fn canonical_sources_map_to_themselves() {
        let layout = RawLayout {
            buttons: vec![
                RawSource::Button(Button::South),
                RawSource::Button(Button::East),
                RawSource::Button(Button::Start),
            ],
            axes: vec![
                axis_info(RawSource::Axis(Axis::LeftStickY), -1.0, 1.0),
                axis_info(RawSource::Axis(Axis::RightTrigger), 0.0, 255.0),
            ],
            ..RawLayout::default()
        };
        let mut raw = RawState::for_layout(&layout);
        raw.buttons = vec![1.0, 0.0, 1.0];
        raw.axes = vec![0.5, 255.0];

        let state = map(&Mapper::new(&Mapping::default(), &layout), &raw);
        assert!(state.pressed(Button::South));
        assert!(state.pressed(Button::Start));
        assert!(!state.pressed(Button::East));
        assert_eq!(state.axis(Axis::LeftStickY), 0.5);
        assert_eq!(state.axis(Axis::RightTrigger), 1.0);
        assert!(state.pressed(Button::RightTrigger));

        // A mapping replaces the targets it sets, and only those: a pad
        // whose south and east are swapped.
        let swapped = mapping(
            &[
                (Button::South, "b{button:east}"),
                (Button::East, "b{button:south}"),
            ],
            &[(Axis::LeftStickY, whole("a{axis:left_stick_y}~"))],
        );
        let state = map(&Mapper::new(&swapped, &layout), &raw);
        assert!(!state.pressed(Button::South));
        assert!(state.pressed(Button::East));
        assert!(state.pressed(Button::Start));
        assert_eq!(state.axis(Axis::LeftStickY), -0.5);
    }

    #[test]
    fn trigger_buttons_follow_their_axis_unless_mapped() {
        let layout = layout(1, &[(0.0, 1.0), (0.0, 1.0)], 0);
        let mapper = Mapper::new(
            &mapping(
                &[(Button::LeftTrigger, "b0")],
                &[
                    (Axis::LeftTrigger, whole("a0")),
                    (Axis::RightTrigger, whole("a1")),
                ],
            ),
            &layout,
        );
        let mut raw = RawState::for_layout(&layout);
        raw.axes = vec![1.0, PRESS_THRESHOLD];
        let state = map(&mapper, &raw);
        assert!(!state.pressed(Button::LeftTrigger), "mapped to b0");
        assert!(state.pressed(Button::RightTrigger));
        raw.axes = vec![1.0, 0.1];
        assert!(!map(&mapper, &raw).pressed(Button::RightTrigger));

        let caps = mapper.capabilities();
        assert!(caps.has_button(Button::LeftTrigger));
        assert!(caps.has_button(Button::RightTrigger));

        // A trigger button the mapping sets to a missing input stays
        // unresolved rather than following the axis.
        let mapper = Mapper::new(
            &mapping(
                &[(Button::LeftTrigger, "b9")],
                &[(Axis::LeftTrigger, whole("a0"))],
            ),
            &layout,
        );
        assert!(!mapper.capabilities().has_button(Button::LeftTrigger));
        raw.axes = vec![1.0, 0.0];
        assert!(!map(&mapper, &raw).pressed(Button::LeftTrigger));
    }

    #[test]
    fn unresolved_inputs_and_capabilities() {
        let layout = RawLayout {
            sensors: SensorSet::from([Sensor::Gyroscope]),
            touchpads: vec![TouchpadInfo {
                max_fingers: 2,
                width: 1920,
                height: 1080,
            }],
            battery: true,
            ..layout(2, &[(-1.0, 1.0), (0.0, 0.0)], 0)
        };
        let mapper = Mapper::new(
            &mapping(
                &[
                    (Button::South, "b0"),
                    (Button::East, "b5"),
                    (Button::West, "b{evdev:0x0130}"),
                    (Button::DPadUp, "h0.1"),
                ],
                &[
                    (Axis::LeftStickX, whole("a0")),
                    // An axis with an empty range.
                    (Axis::LeftStickY, whole("a1")),
                    (Axis::RightStickX, halves("a9", "+a0")),
                    (Axis::RightStickY, halves("b7", "b8")),
                ],
            ),
            &layout,
        );
        assert_eq!(
            mapper.unresolved(),
            [
                (Target::Button(Button::East), input("b5")),
                (Target::Button(Button::West), input("b{evdev:0x0130}")),
                (Target::Button(Button::DPadUp), input("h0.1")),
                (Target::Axis(Axis::LeftStickY), input("a1")),
                (Target::Axis(Axis::RightStickX), input("a9")),
                (Target::Axis(Axis::RightStickY), input("b7")),
                (Target::Axis(Axis::RightStickY), input("b8")),
            ]
        );
        let caps = mapper.capabilities();
        assert_eq!(caps.buttons.iter().collect::<Vec<_>>(), [Button::South]);
        assert_eq!(
            caps.axes.iter().collect::<Vec<_>>(),
            [Axis::LeftStickX, Axis::RightStickX]
        );
        assert_eq!(caps.sensors, layout.sensors);
        assert_eq!(caps.touchpads, layout.touchpads);
        assert!(caps.battery);
        assert!(caps.outputs.is_empty());

        // A resolved half still drives the axis.
        let mut raw = RawState::for_layout(&layout);
        raw.axes[0] = 0.5;
        assert_eq!(map(&mapper, &raw).axis(Axis::RightStickX), 0.5);
    }

    #[test]
    fn odd_raw_states_read_as_rest() {
        let layout = layout(1, &[(-1.0, 1.0), (-1.0, 1.0)], 1);
        let mapper = Mapper::new(
            &mapping(
                &[(Button::South, "b0"), (Button::DPadUp, "h0.1")],
                &[
                    (Axis::LeftStickX, whole("a0")),
                    (Axis::LeftStickY, halves("-a1", "+a1")),
                    (Axis::LeftTrigger, whole("b0")),
                ],
            ),
            &layout,
        );
        let mut raw = RawState::for_layout(&layout);
        raw.buttons[0] = f32::NAN;
        raw.axes = vec![f32::NAN, f32::NAN];
        assert_eq!(map(&mapper, &raw), GamepadState::new());

        // Out of range values are clamped.
        raw.buttons[0] = 2.0;
        raw.axes = vec![-3.0, f32::INFINITY];
        let state = map(&mapper, &raw);
        assert!(state.pressed(Button::South));
        assert_eq!(state.axis(Axis::LeftTrigger), 1.0);
        assert_eq!(state.axis(Axis::LeftStickX), -1.0);
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);

        // A state that does not fit the layout: missing elements at rest.
        let mut state = GamepadState::new();
        mapper.apply(&RawState::default(), &mut state);
        assert_eq!(state, GamepadState::new());
    }

    #[test]
    fn touches_power_and_latest_motion() {
        let layout = layout(0, &[], 0);
        let mapper = Mapper::new(&Mapping::default(), &layout);
        let sample = |timestamp_us| MotionSample {
            timestamp_us,
            gyro: Some(Vec3::default()),
            accel: None,
        };
        let finger = TouchPoint {
            touchpad: 0,
            id: 3,
            x: 0.5,
            y: 0.25,
        };
        let mut raw = RawState::for_layout(&layout);
        raw.motion = vec![sample(1), sample(2)];
        raw.touches[0] = Some(finger);
        raw.power = Power {
            state: PowerState::OnBattery,
            percent: Some(70),
        };
        let mut state = GamepadState::new();
        mapper.apply(&raw, &mut state);
        assert_eq!(state.motion, Some(sample(2)));
        assert_eq!(state.active_touches().collect::<Vec<_>>(), [finger]);
        assert_eq!(state.power, raw.power);

        // No new sample: the latest one stays; touches follow the raw state.
        raw.motion.clear();
        raw.touches[0] = None;
        mapper.apply(&raw, &mut state);
        assert_eq!(state.motion, Some(sample(2)));
        assert_eq!(state.active_touches().count(), 0);
    }
}
