//! The guided mapping wizard (W4): asks the person to use each control of
//! the usual layout in turn, and records which raw element answers, as a
//! [`Mapping`] in the profile format.
//!
//! The wizard is a state machine fed with the device's [`RawState`] once a
//! frame ([`Wizard::update`]); the app only shows its [`Status`]. It is pure
//! and deterministic, so the web tester and the desktop app's collection
//! mode run the same logic.
//!
//! - **Steps** ([`Step`]): buttons by position (the bottom face button
//!   first), sticks (push right, push up) and triggers (pull fully).
//!   [`Step::USUAL`] is the usual layout.
//! - **Detection**: a button pressed past half its travel, a hat switch off
//!   centre, or an axis moved half its range away from where it rests. A
//!   button step takes a button first, then a hat direction, then half an
//!   axis (d-pads and face buttons on axes); a stick step takes an axis; a
//!   trigger step takes an axis, a button, or both (an analog trigger
//!   reported twice). The step completes when everything is released.
//! - **Rest**: the wizard starts by waiting until nothing is pressed and
//!   nothing moves, and learns where every axis rests (triggers that rest
//!   at one end, drift). It keeps learning while nothing is pressed.
//! - **Already used**: an element that answered an earlier step is ignored,
//!   and the status says which control it is ([`Status::conflict`]).
//! - **Not on this pad**: pressing the controls of the first two steps
//!   together (the bottom and right face buttons, [`Status::chord`])
//!   records the current control as missing, so the person need not let go
//!   of the pad; [`Wizard::skip`] does the same from the app.
//!   [`Wizard::back`] redoes the previous step.
//!
//! ```
//! use anygamepad::model::raw::{RawLayout, RawSource, RawState};
//! use anygamepad::model::Button;
//! use anygamepad::wizard::{Phase, Step, Wizard};
//!
//! let layout = RawLayout {
//!     buttons: (0..4).map(RawSource::Index).collect(),
//!     ..RawLayout::default()
//! };
//! let mut wizard = Wizard::with_steps(layout.clone(), vec![Step::Button(Button::South)]);
//! let mut raw = RawState::for_layout(&layout);
//!
//! // At rest for a moment, then button 2 pressed and released.
//! for (now, value) in [(0.0, 0.0), (400.0, 0.0), (420.0, 1.0), (500.0, 0.0), (600.0, 0.0)] {
//!     raw.buttons[2] = value;
//!     wizard.update(&raw, now);
//! }
//! assert_eq!(wizard.status().phase, Phase::Done);
//! assert_eq!(wizard.mapping().buttons[&Button::South].to_string(), "b2");
//! ```

use anygamepad_db::{AxisMapping, AxisRange, Input, Mapping};
use anygamepad_model::raw::{Hat, RawLayout, RawState};
use anygamepad_model::{Axis, AxisSet, Button, ButtonSet};

use crate::mapping::Target;

/// A button counts as pressed for detection from this value on.
pub(crate) const PRESS: f32 = 0.5;
/// Every button is below this value when the pad is released.
pub(crate) const RELEASED: f32 = 0.3;
/// An axis counts as moved this far from its rest, in half-ranges.
pub(crate) const MOVE: f32 = 0.5;
/// Changes below this (in half-ranges for axes) are noise.
const NOISE: f32 = 0.02;
/// How long the pad must be still to learn where its axes rest, in
/// milliseconds.
const SETTLE_MS: f64 = 300.0;
/// How long the pad must be released to complete a step, in milliseconds.
const RELEASE_MS: f64 = 80.0;
/// A button whose value was seen within this far from both ends is analog.
const ANALOG_MARGIN: f32 = 0.15;
/// An axis whose low end is this close to its minimum (in half-ranges) is
/// read whole, as a trigger that rests there.
const END: f32 = 0.25;

/// What the wizard asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// Press a button.
    Button(Button),
    /// Push a stick: right for an X axis, up for a Y axis.
    Stick(Axis),
    /// Pull a trigger all the way: [`Axis::LeftTrigger`] or
    /// [`Axis::RightTrigger`].
    Trigger(Axis),
}

impl Step {
    /// The usual layout: face buttons (bottom and right first), d-pad,
    /// shoulders, triggers, menu buttons, the system button, then each
    /// stick and its click.
    pub const USUAL: [Self; 21] = [
        Self::Button(Button::South),
        Self::Button(Button::East),
        Self::Button(Button::West),
        Self::Button(Button::North),
        Self::Button(Button::DPadUp),
        Self::Button(Button::DPadDown),
        Self::Button(Button::DPadLeft),
        Self::Button(Button::DPadRight),
        Self::Button(Button::LeftShoulder),
        Self::Button(Button::RightShoulder),
        Self::Trigger(Axis::LeftTrigger),
        Self::Trigger(Axis::RightTrigger),
        Self::Button(Button::Select),
        Self::Button(Button::Start),
        Self::Button(Button::Home),
        Self::Stick(Axis::LeftStickX),
        Self::Stick(Axis::LeftStickY),
        Self::Button(Button::LeftStick),
        Self::Stick(Axis::RightStickX),
        Self::Stick(Axis::RightStickY),
        Self::Button(Button::RightStick),
    ];

    /// The controls this step maps: a trigger step maps its axis and its
    /// button.
    pub fn targets(self) -> Vec<Target> {
        match self {
            Self::Button(button) => vec![Target::Button(button)],
            Self::Stick(axis) => vec![Target::Axis(axis)],
            Self::Trigger(axis) => {
                let mut targets = vec![Target::Axis(axis)];
                targets.extend(trigger_button(axis).map(Target::Button));
                targets
            }
        }
    }
}

/// Whether a button value is between its ends: a button that takes it is
/// analog.
fn is_between(value: f32) -> bool {
    value > ANALOG_MARGIN && value < 1.0 - ANALOG_MARGIN
}

/// The digital button of a trigger axis.
fn trigger_button(axis: Axis) -> Option<Button> {
    match axis {
        Axis::LeftTrigger => Some(Button::LeftTrigger),
        Axis::RightTrigger => Some(Button::RightTrigger),
        _ => None,
    }
}

/// What a completed step recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The inputs that answered, with the control each one drives.
    Mapped(Vec<(Target, Input)>),
    /// The person said the device does not have the control.
    Missing,
}

/// Where the wizard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Waiting until every control is released and still.
    Release,
    /// Waiting for the person to use the control of the current step.
    Listen,
    /// A control answered (or the skip combination is held); the step
    /// completes when everything is released.
    Held,
    /// Every step is complete.
    Done,
}

/// What the app shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// Index of the current step, or the number of steps when done.
    pub step: usize,
    /// Number of steps.
    pub steps: usize,
    /// The current step, until done.
    pub current: Option<Step>,
    /// Where the wizard is.
    pub phase: Phase,
    /// The input the current step will record when released.
    pub detected: Option<Input>,
    /// Whether the current step will be recorded as missing when released.
    pub skipping: bool,
    /// The control an element in use already drives, when it answered an
    /// earlier step.
    pub conflict: Option<Target>,
    /// The two buttons that, pressed together, say the device does not have
    /// the current control; once both are known.
    pub chord: Option<[Button; 2]>,
}

/// An element in use.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Activation {
    Button(usize),
    Hat(usize, u8),
    /// An axis and its deviation from rest, in half-ranges.
    Axis(usize, f32),
}

/// What answered the current step, refined while it is held.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Detection {
    button: Option<usize>,
    /// Whether the button took values between its ends: analog.
    analog: bool,
    hat: Option<(usize, u8)>,
    /// The axis that moved furthest, and the lowest and highest values it
    /// took.
    axis: Option<(usize, f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Release,
    Listen,
    Held { detection: Detection, skip: bool },
    Done,
}

/// The guided mapping wizard. See the [module documentation](self).
#[derive(Clone, Debug)]
pub struct Wizard {
    layout: RawLayout,
    steps: Vec<Step>,
    outcomes: Vec<Outcome>,
    state: State,
    /// Where each axis rests, once learned.
    rest: Option<Vec<f32>>,
    previous: Option<RawState>,
    /// When the pad last moved.
    still_since: f64,
    /// When the pad was last in use.
    released_since: Option<f64>,
    conflict: Option<Target>,
}

impl Wizard {
    /// A wizard over the usual layout ([`Step::USUAL`]) for a device whose
    /// provider reads `layout`.
    pub fn new(layout: RawLayout) -> Self {
        Self::with_steps(layout, Step::USUAL.to_vec())
    }

    /// A wizard over `steps`. The skip combination needs steps for
    /// [`Button::South`] and [`Button::East`].
    pub fn with_steps(layout: RawLayout, steps: Vec<Step>) -> Self {
        Self {
            layout,
            steps,
            outcomes: Vec::new(),
            state: State::Release,
            rest: None,
            previous: None,
            still_since: 0.0,
            released_since: None,
            conflict: None,
        }
    }

    /// The raw elements the wizard reads.
    pub fn layout(&self) -> &RawLayout {
        &self.layout
    }

    /// The steps, in order.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The outcome of each completed step, in order.
    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// Feeds the device's latest raw state, read at `now` (milliseconds on
    /// any steady clock). A state that does not fit the layout is ignored.
    pub fn update(&mut self, raw: &RawState, now: f64) {
        if !raw.fits(&self.layout) {
            return;
        }
        let moved = self
            .previous
            .as_ref()
            .is_none_or(|previous| self.changed(previous, raw));
        if moved {
            self.still_since = now;
        }
        let quiet = self.quiet(raw);
        if !quiet {
            self.released_since = None;
        } else if self.released_since.is_none() {
            self.released_since = Some(now);
        }
        let still = now - self.still_since >= SETTLE_MS;

        match self.state {
            State::Done => {}
            State::Release => {
                if quiet && still {
                    self.learn_rest(raw);
                    self.state = State::Listen;
                }
            }
            State::Listen => {
                if quiet && still {
                    self.learn_rest(raw);
                }
                self.listen(raw);
            }
            State::Held {
                mut detection,
                mut skip,
            } => {
                skip |= self.chord_held(raw);
                self.refine(&mut detection, raw);
                let released = quiet
                    && self.axis_released(&detection, raw)
                    && self
                        .released_since
                        .is_some_and(|since| now - since >= RELEASE_MS)
                    && now - self.still_since >= RELEASE_MS;
                if released {
                    let outcome = if skip {
                        Some(Outcome::Missing)
                    } else {
                        self.outcome(&detection, raw)
                    };
                    self.learn_rest(raw);
                    match outcome {
                        Some(outcome) => self.complete(outcome),
                        // Nothing usable answered (an axis that came back
                        // where it was): ask again.
                        None => self.state = State::Listen,
                    }
                } else {
                    self.state = State::Held { detection, skip };
                }
            }
        }
        self.previous = Some(raw.clone());
    }

    /// Records the current step as missing once everything is released, as
    /// the skip combination does.
    pub fn skip(&mut self) {
        match self.state {
            State::Done => {}
            State::Held { detection, .. } => {
                self.state = State::Held {
                    detection,
                    skip: true,
                }
            }
            State::Release | State::Listen => {
                self.state = State::Held {
                    detection: Detection::default(),
                    skip: true,
                }
            }
        }
    }

    /// Forgets the last completed step and asks for it again, once
    /// everything is released.
    pub fn back(&mut self) {
        if self.outcomes.pop().is_some() {
            self.state = State::Release;
            self.conflict = None;
        }
    }

    /// Starts over: forgets every step and where the axes rest.
    pub fn restart(&mut self) {
        *self = Self::with_steps(self.layout.clone(), self.steps.clone());
    }

    /// What the app shows.
    pub fn status(&self) -> Status {
        let (phase, detected, skipping) = match self.state {
            State::Release => (Phase::Release, None, false),
            State::Listen => (Phase::Listen, None, false),
            State::Held { detection, skip } => (
                Phase::Held,
                if skip {
                    None
                } else {
                    self.current()
                        .and_then(|step| self.primary(step, &detection))
                },
                skip,
            ),
            State::Done => (Phase::Done, None, false),
        };
        Status {
            step: self.outcomes.len(),
            steps: self.steps.len(),
            current: self.current(),
            phase,
            detected,
            skipping,
            conflict: self.conflict,
            chord: self.chord().map(|_| [Button::South, Button::East]),
        }
    }

    /// The mapping recorded so far: the inputs of every completed step, in
    /// the profile format, with no `when`.
    pub fn mapping(&self) -> Mapping {
        let mut mapping = Mapping::default();
        for outcome in &self.outcomes {
            if let Outcome::Mapped(inputs) = outcome {
                for &(target, input) in inputs {
                    match target {
                        Target::Button(button) => {
                            mapping.buttons.insert(button, input);
                        }
                        Target::Axis(axis) => {
                            mapping.axes.insert(axis, AxisMapping::full(input));
                        }
                    }
                }
            }
        }
        mapping
    }

    /// The controls the person said the device does not have.
    pub fn missing(&self) -> (ButtonSet, AxisSet) {
        let mut buttons = ButtonSet::EMPTY;
        let mut axes = AxisSet::EMPTY;
        for (step, outcome) in self.steps.iter().zip(&self.outcomes) {
            if *outcome != Outcome::Missing {
                continue;
            }
            for target in step.targets() {
                match target {
                    Target::Button(button) => buttons = buttons.with(button),
                    Target::Axis(axis) => axes = axes.with(axis),
                }
            }
        }
        (buttons, axes)
    }

    fn current(&self) -> Option<Step> {
        self.steps.get(self.outcomes.len()).copied()
    }

    fn complete(&mut self, outcome: Outcome) {
        self.outcomes.push(outcome);
        self.conflict = None;
        self.state = if self.outcomes.len() >= self.steps.len() {
            State::Done
        } else {
            State::Listen
        };
    }

    /// Half the range of axis `index`, at least a tiny positive number.
    fn half(&self, index: usize) -> f32 {
        let info = &self.layout.axes[index];
        ((info.max - info.min) / 2.0).abs().max(f32::EPSILON)
    }

    fn centre(&self, index: usize) -> f32 {
        let info = &self.layout.axes[index];
        (info.min + info.max) / 2.0
    }

    fn rest_of(&self, index: usize) -> f32 {
        self.rest
            .as_ref()
            .and_then(|rest| rest.get(index).copied())
            .unwrap_or_else(|| self.centre(index))
    }

    /// Deviation of axis `index` from its rest, in half-ranges; NaN reads
    /// as rest.
    fn deviation(&self, raw: &RawState, index: usize) -> f32 {
        let value = raw.axes[index];
        if value.is_nan() {
            0.0
        } else {
            (value - self.rest_of(index)) / self.half(index)
        }
    }

    fn changed(&self, previous: &RawState, raw: &RawState) -> bool {
        let buttons = previous
            .buttons
            .iter()
            .zip(&raw.buttons)
            .any(|(a, b)| (a - b).abs() > NOISE);
        let axes = previous
            .axes
            .iter()
            .zip(&raw.axes)
            .enumerate()
            .any(|(index, (a, b))| (a - b).abs() / self.half(index) > NOISE);
        buttons || axes || previous.hats != raw.hats
    }

    /// No button pressed and every hat centred.
    fn quiet(&self, raw: &RawState) -> bool {
        raw.buttons
            .iter()
            .all(|&value| value.is_nan() || value < RELEASED)
            && raw.hats.iter().all(|&hat| hat == Hat::CENTERED)
    }

    fn learn_rest(&mut self, raw: &RawState) {
        self.rest = Some(
            raw.axes
                .iter()
                .enumerate()
                .map(|(index, &value)| {
                    if value.is_finite() {
                        value
                    } else {
                        self.centre(index)
                    }
                })
                .collect(),
        );
    }

    /// The elements in use: buttons, then hats, then axes, in layout
    /// order.
    fn activations(&self, raw: &RawState) -> Vec<Activation> {
        let buttons = (raw.buttons.iter().enumerate())
            .filter(|&(_, &value)| value >= PRESS)
            .map(|(index, _)| Activation::Button(index));
        let hats = (raw.hats.iter().enumerate())
            .filter(|&(_, &hat)| hat != Hat::CENTERED)
            .map(|(index, hat)| Activation::Hat(index, hat.0));
        let axes = (0..raw.axes.len())
            .map(|index| (index, self.deviation(raw, index)))
            .filter(|&(_, deviation)| deviation.abs() >= MOVE)
            .map(|(index, deviation)| Activation::Axis(index, deviation));
        buttons.chain(hats).chain(axes).collect()
    }

    /// The strongest activation that `accepts` and that answered no earlier
    /// step: buttons by value, axes by how far they moved, first of each
    /// kind on ties; a button before a hat before an axis.
    fn strongest(
        &self,
        activations: &[Activation],
        raw: &RawState,
        accepts: impl Fn(Activation) -> bool,
    ) -> Option<Activation> {
        let strength = |activation: Activation| match activation {
            Activation::Button(index) => (0, raw.buttons[index]),
            Activation::Hat(..) => (1, 0.0),
            Activation::Axis(_, deviation) => (2, deviation.abs()),
        };
        let mut best: Option<(Activation, (u8, f32))> = None;
        for &activation in activations {
            if !accepts(activation) || self.assigned(activation, raw).is_some() {
                continue;
            }
            let (kind, value) = strength(activation);
            let better = best.is_none_or(|(_, (best_kind, best_value))| {
                kind < best_kind || (kind == best_kind && value > best_value)
            });
            if better {
                best = Some((activation, (kind, value)));
            }
        }
        best.map(|(activation, _)| activation)
    }

    /// The control an activation already drives, if it answered an earlier
    /// step.
    fn assigned(&self, activation: Activation, raw: &RawState) -> Option<Target> {
        let layout = &self.layout;
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Mapped(inputs) => Some(inputs),
                Outcome::Missing => None,
            })
            .flatten()
            .find(|(_, input)| match (activation, *input) {
                (Activation::Button(index), Input::Button(source)) => {
                    layout.buttons[index] == source
                }
                (Activation::Hat(index, bits), Input::Hat { source, mask }) => {
                    layout.hats[index] == source && bits & mask != 0
                }
                (Activation::Axis(index, _), Input::Axis { source, range, .. }) => {
                    let above = raw.axes[index] >= self.centre(index);
                    layout.axes[index].source == source
                        && match range {
                            AxisRange::Full => true,
                            AxisRange::Positive => above,
                            AxisRange::Negative => !above,
                        }
                }
                _ => false,
            })
            .map(|&(target, _)| target)
    }

    /// The inputs of the bottom and right face buttons, once both are
    /// recorded.
    fn chord(&self) -> Option<[Input; 2]> {
        let input = |button: Button| {
            self.steps
                .iter()
                .zip(&self.outcomes)
                .find(|(step, _)| **step == Step::Button(button))
                .and_then(|(_, outcome)| match outcome {
                    Outcome::Mapped(inputs) => inputs.first().map(|&(_, input)| input),
                    Outcome::Missing => None,
                })
        };
        Some([input(Button::South)?, input(Button::East)?])
    }

    fn chord_held(&self, raw: &RawState) -> bool {
        self.chord()
            .is_some_and(|inputs| inputs.iter().all(|&input| self.input_active(input, raw)))
    }

    /// Whether a recorded input is in use.
    fn input_active(&self, input: Input, raw: &RawState) -> bool {
        let layout = &self.layout;
        match input {
            Input::Button(source) => layout
                .buttons
                .iter()
                .position(|&s| s == source)
                .is_some_and(|index| raw.buttons[index] >= PRESS),
            Input::Hat { source, mask } => layout
                .hats
                .iter()
                .position(|&s| s == source)
                .is_some_and(|index| raw.hats[index].0 & mask != 0),
            Input::Axis {
                source,
                range,
                inverted,
            } => layout
                .axes
                .iter()
                .position(|axis| axis.source == source)
                .is_some_and(|index| {
                    let deviation = self.deviation(raw, index);
                    match (range, inverted) {
                        (AxisRange::Positive, _) | (AxisRange::Full, false) => deviation >= MOVE,
                        (AxisRange::Negative, _) | (AxisRange::Full, true) => deviation <= -MOVE,
                    }
                }),
        }
    }

    /// Whether `activation` can answer `step`.
    fn accepts(step: Step, activation: Activation) -> bool {
        match step {
            Step::Button(_) => true,
            Step::Stick(_) => matches!(activation, Activation::Axis(..)),
            Step::Trigger(_) => !matches!(activation, Activation::Hat(..)),
        }
    }

    fn listen(&mut self, raw: &RawState) {
        let Some(step) = self.current() else {
            self.state = State::Done;
            return;
        };
        if self.chord_held(raw) {
            self.conflict = None;
            self.state = State::Held {
                detection: Detection::default(),
                skip: true,
            };
            return;
        }
        let activations = self.activations(raw);
        self.conflict = activations
            .iter()
            .find_map(|&activation| self.assigned(activation, raw));
        // A trigger step prefers an axis; a button step a button, then a
        // hat, then an axis.
        let answer = match step {
            Step::Trigger(_) => self
                .strongest(&activations, raw, |a| matches!(a, Activation::Axis(..)))
                .or_else(|| self.strongest(&activations, raw, |a| Self::accepts(step, a))),
            _ => self.strongest(&activations, raw, |a| Self::accepts(step, a)),
        };
        let Some(activation) = answer else {
            return;
        };
        let mut detection = Detection::default();
        match activation {
            Activation::Button(index) => {
                detection.button = Some(index);
                // A trigger pressed fast shows its travel in the frame
                // before.
                detection.analog = self
                    .previous
                    .as_ref()
                    .is_some_and(|previous| is_between(previous.buttons[index]));
            }
            Activation::Hat(index, bits) => detection.hat = Some((index, bits)),
            Activation::Axis(index, _) => {
                detection.axis = Some((index, raw.axes[index], raw.axes[index]));
            }
        }
        self.refine(&mut detection, raw);
        self.state = State::Held {
            detection,
            skip: false,
        };
    }

    /// Follows what answered while it is held: the button's travel, the
    /// axis's extremes, a trigger's second element.
    fn refine(&self, detection: &mut Detection, raw: &RawState) {
        let Some(step) = self.current() else {
            return;
        };
        if let Some(index) = detection.button {
            detection.analog |= is_between(raw.buttons[index]);
        }
        if let Some((index, low, high)) = &mut detection.axis {
            let value = raw.axes[*index];
            if value.is_finite() {
                *low = low.min(value);
                *high = high.max(value);
            }
        }
        let activations = self.activations(raw);
        let is_axis = |a: Activation| matches!(a, Activation::Axis(..));
        let is_button = |a: Activation| matches!(a, Activation::Button(..));
        // A stick moved diagonally: the axis that moved most answers.
        if let (Step::Stick(_), Some((index, ..))) = (step, detection.axis) {
            if let Some(Activation::Axis(other, deviation)) =
                self.strongest(&activations, raw, is_axis)
            {
                if other != index && deviation.abs() > self.deviation(raw, index).abs() {
                    detection.axis = Some((other, raw.axes[other], raw.axes[other]));
                }
            }
        }
        // An analog trigger reported as an axis and a button: keep both.
        if let Step::Trigger(_) = step {
            if detection.button.is_none() {
                if let Some(Activation::Button(index)) =
                    self.strongest(&activations, raw, is_button)
                {
                    detection.button = Some(index);
                }
            }
            if detection.axis.is_none() {
                if let Some(Activation::Axis(index, _)) = self.strongest(&activations, raw, is_axis)
                {
                    detection.axis = Some((index, raw.axes[index], raw.axes[index]));
                }
            }
        }
    }

    /// Whether the axis that answered has come back from its extreme.
    fn axis_released(&self, detection: &Detection, raw: &RawState) -> bool {
        let Some((index, low, high)) = detection.axis else {
            return true;
        };
        let value = raw.axes[index];
        let half = self.half(index);
        // Back half a range from the extreme it reached, or more.
        let peak = if (high - value).abs() >= (value - low).abs() {
            high
        } else {
            low
        };
        (peak - value).abs() / half >= MOVE
    }

    /// The input `detection` gives `step`, as the status shows it.
    fn primary(&self, step: Step, detection: &Detection) -> Option<Input> {
        let layout = &self.layout;
        let axis = detection
            .axis
            .map(|(index, low, high)| self.axis_input(step, index, low, high, None));
        let button = detection
            .button
            .map(|index| Input::Button(layout.buttons[index]));
        let hat = detection.hat.map(|(index, mask)| Input::Hat {
            source: layout.hats[index],
            mask,
        });
        match step {
            Step::Button(_) => button.or(hat).or(axis),
            Step::Stick(_) => axis,
            Step::Trigger(_) => axis.or(button),
        }
    }

    /// The input of an axis that went from `low` to `high` and settled at
    /// `settled` (its value when released; `None` while held).
    fn axis_input(
        &self,
        step: Step,
        index: usize,
        low: f32,
        high: f32,
        settled: Option<f32>,
    ) -> Input {
        let info = &self.layout.axes[index];
        let half = self.half(index);
        let rest = settled.unwrap_or_else(|| self.rest_of(index));
        // The end that was pushed: the extreme furthest from where it
        // rests (signed, for an axis that only went one way).
        let positive = high - rest >= rest - low;
        let source = info.source;
        match step {
            Step::Stick(_) => Input::Axis {
                source,
                range: AxisRange::Full,
                inverted: !positive,
            },
            Step::Trigger(_) => {
                let from_min = (low.min(rest) - info.min) / half <= END;
                let from_max = (info.max - high.max(rest)) / half <= END;
                match (positive, from_min, from_max) {
                    (true, true, _) => Input::Axis {
                        source,
                        range: AxisRange::Full,
                        inverted: false,
                    },
                    (false, _, true) => Input::Axis {
                        source,
                        range: AxisRange::Full,
                        inverted: true,
                    },
                    (true, false, _) => Input::Axis {
                        source,
                        range: AxisRange::Positive,
                        inverted: false,
                    },
                    (false, _, false) => Input::Axis {
                        source,
                        range: AxisRange::Negative,
                        inverted: false,
                    },
                }
            }
            Step::Button(_) => Input::Axis {
                source,
                range: if (if positive { high } else { low }) >= self.centre(index) {
                    AxisRange::Positive
                } else {
                    AxisRange::Negative
                },
                inverted: false,
            },
        }
    }

    /// What the step records, from what answered and where the axis
    /// settled; `None` when nothing usable answered.
    fn outcome(&self, detection: &Detection, raw: &RawState) -> Option<Outcome> {
        let step = self.current()?;
        let layout = &self.layout;
        let button = detection
            .button
            .map(|index| Input::Button(layout.buttons[index]));
        let axis = detection.axis.map(|(index, low, high)| {
            self.axis_input(step, index, low, high, Some(raw.axes[index]))
        });
        let mut inputs = Vec::new();
        match step {
            Step::Button(target) => {
                let hat = detection.hat.map(|(index, mask)| Input::Hat {
                    source: layout.hats[index],
                    mask,
                });
                inputs.extend(
                    button
                        .or(hat)
                        .or(axis)
                        .map(|input| (Target::Button(target), input)),
                );
            }
            Step::Stick(target) => {
                inputs.extend(axis.map(|input| (Target::Axis(target), input)));
            }
            Step::Trigger(target) => {
                let digital = trigger_button(target).map(Target::Button);
                match (axis, button) {
                    (Some(axis), button) => {
                        inputs.push((Target::Axis(target), axis));
                        if let (Some(digital), Some(button)) = (digital, button) {
                            inputs.push((digital, button));
                        }
                    }
                    (None, Some(button)) => {
                        // An analog button is the trigger's travel too; a
                        // digital one is only the trigger button.
                        if detection.analog {
                            inputs.push((Target::Axis(target), button));
                        }
                        if let Some(digital) = digital {
                            inputs.push((digital, button));
                        }
                    }
                    (None, None) => {}
                }
            }
        }
        (!inputs.is_empty()).then_some(Outcome::Mapped(inputs))
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::raw::{RawAxisInfo, RawSource};

    use super::*;

    /// Drives a wizard with a simulated pad, 10 ms per frame.
    struct Pad {
        wizard: Wizard,
        raw: RawState,
        now: f64,
    }

    impl Pad {
        fn new(layout: RawLayout, steps: Option<Vec<Step>>) -> Self {
            Self::starting(layout, steps, |_| {})
        }

        /// A pad whose controls rest as `init` sets them.
        fn starting(
            layout: RawLayout,
            steps: Option<Vec<Step>>,
            init: impl Fn(&mut RawState),
        ) -> Self {
            let mut raw = RawState::for_layout(&layout);
            init(&mut raw);
            let wizard = match steps {
                Some(steps) => Wizard::with_steps(layout, steps),
                None => Wizard::new(layout),
            };
            let mut pad = Self {
                wizard,
                raw,
                now: 0.0,
            };
            pad.wait(400.0);
            pad
        }

        fn wait(&mut self, ms: f64) {
            let end = self.now + ms;
            while self.now < end {
                self.wizard.update(&self.raw, self.now);
                self.now += 10.0;
            }
        }

        /// Sets the pad, holds it for 100 ms, then restores it and waits.
        fn hold(&mut self, change: impl Fn(&mut RawState)) {
            let rest = self.raw.clone();
            change(&mut self.raw);
            self.wait(100.0);
            self.raw = rest;
            self.wait(150.0);
        }

        fn button(&mut self, index: usize) {
            self.hold(|raw| raw.buttons[index] = 1.0);
        }

        fn axis(&mut self, index: usize, value: f32) {
            self.hold(|raw| raw.axes[index] = value);
        }

        fn status(&self) -> Status {
            self.wizard.status()
        }
    }

    fn input(text: &str) -> Input {
        text.parse().unwrap()
    }

    fn indexed(buttons: u16, axes: u16, hats: u16, min: f32, max: f32) -> RawLayout {
        RawLayout {
            buttons: (0..buttons).map(RawSource::Index).collect(),
            axes: (0..axes)
                .map(|index| RawAxisInfo {
                    source: RawSource::Index(index),
                    min,
                    max,
                })
                .collect(),
            hats: (0..hats).map(RawSource::Index).collect(),
            ..RawLayout::default()
        }
    }

    #[test]
    fn waits_for_a_still_pad_first() {
        let layout = indexed(4, 0, 0, -1.0, 1.0);
        let mut wizard = Wizard::new(layout.clone());
        let mut raw = RawState::for_layout(&layout);
        raw.buttons[0] = 1.0;
        for frame in 0..50 {
            wizard.update(&raw, f64::from(frame) * 10.0);
        }
        assert_eq!(wizard.status().phase, Phase::Release);
        raw.buttons[0] = 0.0;
        for frame in 50..100 {
            wizard.update(&raw, f64::from(frame) * 10.0);
        }
        let status = wizard.status();
        assert_eq!(status.phase, Phase::Listen);
        assert_eq!(status.current, Some(Step::Button(Button::South)));
        assert_eq!((status.step, status.steps), (0, 21));
    }

    /// Every step of the usual layout on a Gamepad API pad in the standard
    /// mapping gives the curated standard profile.
    #[test]
    fn standard_pad_gives_the_standard_profile() {
        let mut pad = Pad::new(indexed(17, 4, 0, -1.0, 1.0), None);
        // Face buttons, d-pad, shoulders.
        for index in [0, 1, 2, 3, 12, 13, 14, 15, 4, 5] {
            pad.button(index);
        }
        // Analog triggers: buttons 6 and 7 move through their travel.
        for index in [6, 7] {
            for value in [0.3, 0.7, 1.0] {
                pad.raw.buttons[index] = value;
                pad.wait(30.0);
            }
            pad.raw.buttons[index] = 0.0;
            pad.wait(150.0);
        }
        assert_eq!(pad.status().step, 12);
        pad.button(8);
        pad.button(9);
        pad.button(16);
        pad.axis(0, 1.0);
        pad.axis(1, -1.0);
        pad.button(10);
        pad.axis(2, 1.0);
        pad.axis(3, -1.0);
        pad.button(11);
        assert_eq!(pad.status().phase, Phase::Done);

        let profile = anygamepad_db::Profile::from_toml(include_str!(
            "../../../data/profiles/gamepad-api-standard.toml"
        ))
        .unwrap();
        let expected = profile.mappings[0].clone();
        let mapping = pad.wizard.mapping();
        assert_eq!(mapping, expected);
        assert_eq!(pad.wizard.missing(), (ButtonSet::EMPTY, AxisSet::EMPTY));
    }

    /// A DirectInput-style pad: a hat d-pad, sticks on axes 0, 1 and 3, 4,
    /// triggers on one combined axis (2), and a trigger axis that rests at
    /// its minimum (5) on the right.
    #[test]
    fn hats_combined_axes_and_resting_ends() {
        let layout = indexed(12, 6, 1, 0.0, 255.0);
        let mut pad = Pad::starting(layout, None, |raw| {
            raw.axes = vec![128.0, 128.0, 128.0, 128.0, 128.0, 0.0];
        });

        for index in [1, 2, 0, 3] {
            pad.button(index);
        }
        for bits in [Hat::UP, Hat::DOWN, Hat::LEFT, Hat::RIGHT] {
            pad.hold(|raw| raw.hats[0] = Hat(bits));
        }
        pad.button(4);
        pad.button(5);
        pad.axis(2, 255.0); // left trigger: up from the centre
        pad.axis(5, 255.0); // right trigger: from its minimum
        pad.button(8);
        pad.button(9);
        pad.button(11);
        pad.axis(0, 255.0);
        pad.axis(1, 0.0);
        pad.button(10);
        pad.axis(3, 255.0);
        pad.axis(4, 0.0);
        // The right stick click is not there: South and East together.
        pad.hold(|raw| {
            raw.buttons[1] = 1.0;
            raw.buttons[2] = 1.0;
        });
        assert_eq!(pad.status().phase, Phase::Done);

        let mapping = pad.wizard.mapping();
        let button = |b: Button| mapping.buttons[&b].to_string();
        let axis = |a: Axis| mapping.axes[&a].input.unwrap().to_string();
        assert_eq!(button(Button::South), "b1");
        assert_eq!(button(Button::East), "b2");
        assert_eq!(button(Button::DPadUp), "h0.1");
        assert_eq!(button(Button::DPadDown), "h0.4");
        assert_eq!(button(Button::DPadLeft), "h0.8");
        assert_eq!(button(Button::DPadRight), "h0.2");
        assert_eq!(button(Button::Home), "b11");
        assert_eq!(axis(Axis::LeftTrigger), "+a2");
        assert_eq!(axis(Axis::RightTrigger), "a5");
        assert_eq!(axis(Axis::LeftStickX), "a0");
        assert_eq!(axis(Axis::LeftStickY), "a1~");
        assert_eq!(axis(Axis::RightStickY), "a4~");
        // Digital trigger buttons follow their axes (mapping engine).
        assert!(!mapping.buttons.contains_key(&Button::LeftTrigger));
        assert_eq!(
            pad.wizard.missing(),
            (ButtonSet::from([Button::RightStick]), AxisSet::EMPTY)
        );
        assert_eq!(pad.wizard.outcomes().last(), Some(&Outcome::Missing));
    }

    #[test]
    fn combined_trigger_axis_halves() {
        let layout = indexed(4, 1, 0, -1.0, 1.0);
        let steps = vec![
            Step::Trigger(Axis::LeftTrigger),
            Step::Trigger(Axis::RightTrigger),
        ];
        let mut pad = Pad::new(layout, Some(steps));
        pad.axis(0, 1.0);
        // The same half again is already the left trigger.
        pad.raw.axes[0] = 1.0;
        pad.wait(50.0);
        assert_eq!(pad.status().conflict, Some(Target::Axis(Axis::LeftTrigger)));
        assert_eq!(pad.status().phase, Phase::Listen);
        pad.raw.axes[0] = 0.0;
        pad.wait(150.0);
        pad.axis(0, -1.0);
        let mapping = pad.wizard.mapping();
        assert_eq!(
            mapping.axes[&Axis::LeftTrigger],
            AxisMapping::full(input("+a0"))
        );
        assert_eq!(
            mapping.axes[&Axis::RightTrigger],
            AxisMapping::full(input("-a0"))
        );
    }

    /// Browsers can report a trigger axis at 0 until it is first used, and
    /// at -1 after: the end it settles at is its rest.
    #[test]
    fn trigger_whose_rest_jumps() {
        let layout = indexed(0, 1, 0, -1.0, 1.0);
        let mut pad = Pad::new(layout, Some(vec![Step::Trigger(Axis::LeftTrigger)]));
        for value in [-1.0, 0.0, 1.0] {
            pad.raw.axes[0] = value;
            pad.wait(30.0);
        }
        pad.raw.axes[0] = -1.0;
        pad.wait(150.0);
        assert_eq!(
            pad.wizard.mapping().axes[&Axis::LeftTrigger],
            AxisMapping::full(input("a0"))
        );
    }

    #[test]
    fn digital_and_analog_trigger_buttons() {
        let layout = indexed(8, 0, 0, -1.0, 1.0);
        let steps = vec![
            Step::Trigger(Axis::LeftTrigger),
            Step::Trigger(Axis::RightTrigger),
        ];
        let mut pad = Pad::new(layout, Some(steps));
        pad.button(6); // digital: 0 then 1
        for value in [0.4, 1.0] {
            pad.raw.buttons[7] = value;
            pad.wait(40.0);
        }
        pad.raw.buttons[7] = 0.0;
        pad.wait(150.0);
        let mapping = pad.wizard.mapping();
        assert_eq!(mapping.buttons[&Button::LeftTrigger], input("b6"));
        assert!(!mapping.axes.contains_key(&Axis::LeftTrigger));
        assert_eq!(mapping.buttons[&Button::RightTrigger], input("b7"));
        assert_eq!(
            mapping.axes[&Axis::RightTrigger],
            AxisMapping::full(input("b7"))
        );
    }

    #[test]
    fn used_elements_are_ignored_and_named() {
        let mut pad = Pad::new(indexed(6, 0, 0, -1.0, 1.0), None);
        pad.button(0);
        pad.raw.buttons[0] = 1.0;
        pad.wait(50.0);
        let status = pad.status();
        assert_eq!(status.conflict, Some(Target::Button(Button::South)));
        assert_eq!(status.phase, Phase::Listen);
        assert_eq!(status.current, Some(Step::Button(Button::East)));
        // The chord is not known yet: still the East step.
        pad.raw.buttons[1] = 1.0;
        pad.wait(50.0);
        assert_eq!(pad.status().phase, Phase::Held);
        assert_eq!(pad.status().detected, Some(input("b1")));
        pad.raw.buttons = vec![0.0; 6];
        pad.wait(150.0);
        assert_eq!(pad.status().step, 2);
        assert_eq!(pad.status().conflict, None);
        assert_eq!(pad.status().chord, Some([Button::South, Button::East]));
    }

    #[test]
    fn chord_says_missing_and_skip_and_back_work() {
        let mut pad = Pad::new(indexed(6, 0, 0, -1.0, 1.0), None);
        pad.button(0);
        pad.button(1);
        // West is missing: South first (already used), then East too.
        pad.raw.buttons[0] = 1.0;
        pad.wait(50.0);
        pad.raw.buttons[1] = 1.0;
        pad.wait(50.0);
        let status = pad.status();
        assert!(status.skipping);
        assert_eq!(status.phase, Phase::Held);
        pad.raw.buttons = vec![0.0; 6];
        pad.wait(150.0);
        assert_eq!(pad.status().current, Some(Step::Button(Button::North)));

        // North from the app.
        pad.wizard.skip();
        pad.wait(150.0);
        assert_eq!(pad.status().current, Some(Step::Button(Button::DPadUp)));
        assert_eq!(
            pad.wizard.missing().0,
            ButtonSet::from([Button::West, Button::North])
        );

        // Back: North again, answered by button 3.
        pad.wizard.back();
        pad.wait(400.0);
        assert_eq!(pad.status().current, Some(Step::Button(Button::North)));
        pad.button(3);
        assert_eq!(pad.wizard.mapping().buttons[&Button::North], input("b3"));
        assert_eq!(pad.wizard.missing().0, ButtonSet::from([Button::West]));

        pad.wizard.restart();
        assert_eq!(pad.status().step, 0);
        assert_eq!(pad.status().phase, Phase::Release);
        assert_eq!(pad.wizard.mapping(), Mapping::default());
    }

    #[test]
    fn a_held_control_completes_on_release_only() {
        let mut pad = Pad::new(
            indexed(4, 2, 0, -1.0, 1.0),
            Some(vec![
                Step::Stick(Axis::LeftStickX),
                Step::Stick(Axis::LeftStickY),
            ]),
        );
        pad.raw.axes[0] = 1.0;
        pad.wait(1000.0);
        let status = pad.status();
        assert_eq!(status.phase, Phase::Held);
        assert_eq!(status.detected, Some(input("a0")));
        pad.raw.axes[0] = 0.0;
        pad.wait(150.0);
        pad.axis(1, -1.0);
        let mapping = pad.wizard.mapping();
        assert_eq!(
            mapping.axes[&Axis::LeftStickX],
            AxisMapping::full(input("a0"))
        );
        assert_eq!(
            mapping.axes[&Axis::LeftStickY],
            AxisMapping::full(input("a1~"))
        );
    }

    #[test]
    fn held_axes_show_their_direction() {
        let layout = indexed(0, 1, 0, -1.0, 1.0);
        let mut pad = Pad::new(layout, Some(vec![Step::Stick(Axis::LeftStickY)]));
        pad.raw.axes[0] = -1.0;
        pad.wait(50.0);
        assert_eq!(pad.status().detected, Some(input("a0~")));
    }

    #[test]
    fn sticks_ignore_buttons_and_take_the_strongest_axis() {
        let layout = indexed(2, 2, 0, -1.0, 1.0);
        let mut pad = Pad::new(layout, Some(vec![Step::Stick(Axis::LeftStickX)]));
        pad.button(0);
        assert_eq!(pad.status().phase, Phase::Listen);
        // Diagonally, then mostly along axis 1.
        pad.raw.axes[0] = 0.6;
        pad.raw.axes[1] = 0.55;
        pad.wait(30.0);
        pad.raw.axes[0] = 0.2;
        pad.raw.axes[1] = 0.9;
        pad.wait(30.0);
        pad.raw.axes = vec![0.0, 0.0];
        pad.wait(150.0);
        assert_eq!(
            pad.wizard.mapping().axes[&Axis::LeftStickX],
            AxisMapping::full(input("a1"))
        );
    }

    #[test]
    fn canonical_sources_answer_too() {
        // A decoded pad: its elements are already canonical.
        let layout = RawLayout {
            buttons: vec![
                RawSource::Button(Button::South),
                RawSource::Button(Button::East),
            ],
            ..RawLayout::default()
        };
        let mut pad = Pad::new(
            layout,
            Some(vec![
                Step::Button(Button::South),
                Step::Button(Button::East),
            ]),
        );
        pad.button(0);
        pad.button(1);
        assert_eq!(
            pad.wizard.mapping().buttons[&Button::South].to_string(),
            "b{button:south}"
        );
    }

    #[test]
    fn misfit_states_and_nan_are_ignored() {
        let layout = indexed(2, 1, 0, -1.0, 1.0);
        let mut pad = Pad::new(layout, Some(vec![Step::Stick(Axis::LeftStickX)]));
        pad.wizard.update(&RawState::default(), pad.now);
        pad.raw.axes[0] = f32::NAN;
        pad.wait(100.0);
        assert_eq!(pad.status().phase, Phase::Listen);
    }
}
