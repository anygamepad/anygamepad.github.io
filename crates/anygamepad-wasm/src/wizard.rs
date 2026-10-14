//! The `MappingWizard` class: the library's guided mapping wizard
//! (`anygamepad::wizard`) for one pad, fed by `Gamepads.feedWizard()` or
//! `HidDevices.feedWizard()` once a frame.

use anygamepad::mapping::{Mapper, Target};
use anygamepad::model::raw::{RawLayout, RawState};
use anygamepad::model::{Axis, Button, GamepadState};
use anygamepad::wizard::{Outcome, Phase, Status, Step, Wizard};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::gamepads::to_js;

/// A control the wizard asks for.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WizardStep {
    /// Press a button.
    Button {
        /// Which.
        button: Button,
    },
    /// Push a stick: right for an X axis, up for a Y axis.
    Stick {
        /// The stick axis.
        axis: Axis,
    },
    /// Pull a trigger all the way.
    Trigger {
        /// The trigger axis.
        axis: Axis,
    },
}

impl From<Step> for WizardStep {
    fn from(step: Step) -> Self {
        match step {
            Step::Button(button) => Self::Button { button },
            Step::Stick(axis) => Self::Stick { axis },
            Step::Trigger(axis) => Self::Trigger { axis },
        }
    }
}

/// A canonical control.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WizardTarget {
    /// A button.
    Button {
        /// Which.
        button: Button,
    },
    /// An axis.
    Axis {
        /// Which.
        axis: Axis,
    },
}

impl From<Target> for WizardTarget {
    fn from(target: Target) -> Self {
        match target {
            Target::Button(button) => Self::Button { button },
            Target::Axis(axis) => Self::Axis { axis },
        }
    }
}

/// Where the wizard is.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum WizardPhase {
    /// Waiting until every control is released and still.
    Release,
    /// Waiting for the person to use the control of the current step.
    Listen,
    /// A control answered, or the skip combination is held; the step
    /// completes when everything is released.
    Held,
    /// Every step is complete.
    Done,
}

impl From<Phase> for WizardPhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Release => Self::Release,
            Phase::Listen => Self::Listen,
            Phase::Held => Self::Held,
            Phase::Done => Self::Done,
        }
    }
}

/// What the page shows.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct WizardStatus {
    /// Index of the current step, or `steps` when done.
    pub step: usize,
    /// Number of steps.
    pub steps: usize,
    /// The current step, until done.
    pub current: Option<WizardStep>,
    /// Where the wizard is.
    pub phase: WizardPhase,
    /// The input the current step records when released, in the profile
    /// format (`b3`, `-a1`, `h0.1`).
    pub detected: Option<String>,
    /// Whether the current step will be recorded as not on this pad.
    pub skipping: bool,
    /// The control an element in use already drives.
    pub conflict: Option<WizardTarget>,
    /// The two buttons that, pressed together, say the pad does not have
    /// the current control, once both are known.
    pub chord: Option<[Button; 2]>,
}

impl From<Status> for WizardStatus {
    fn from(status: Status) -> Self {
        Self {
            step: status.step,
            steps: status.steps,
            current: status.current.map(Into::into),
            phase: status.phase.into(),
            detected: status.detected.map(|input| input.to_string()),
            skipping: status.skipping,
            conflict: status.conflict.map(Into::into),
            chord: status.chord,
        }
    }
}

/// An input a completed step recorded.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct WizardInput {
    /// The control it drives.
    pub target: WizardTarget,
    /// The raw element, in the profile format (`b3`, `a1~`).
    pub input: String,
}

/// What a completed step recorded.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct WizardResult {
    /// The step.
    pub step: WizardStep,
    /// The inputs that answered; empty when `missing`.
    pub inputs: Vec<WizardInput>,
    /// Whether the person said the pad does not have the control.
    pub missing: bool,
}

/// The controls the person said the pad does not have.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct WizardMissing {
    /// Buttons.
    pub buttons: Vec<Button>,
    /// Axes.
    pub axes: Vec<Axis>,
}

/// The guided mapping wizard for one pad: asks for each control of the
/// usual layout in turn and records which raw element answers.
///
/// Create it with `Gamepads.wizard()` or `HidDevices.wizard()`, and feed it
/// once a frame with `feedWizard()`; `status()` says what to show. Pressing
/// the bottom and right face buttons together says the pad does not have
/// the current control, as `skip()` does.
#[wasm_bindgen]
pub struct MappingWizard {
    wizard: Wizard,
    /// The mapping recorded so far, resolved against the layout.
    mapper: Mapper,
    /// How many outcomes `mapper` was built from, and whether the outcomes
    /// changed since.
    mapped: Option<usize>,
    state: GamepadState,
}

impl MappingWizard {
    pub(crate) fn new(layout: RawLayout) -> Self {
        let wizard = Wizard::new(layout);
        Self {
            mapper: Mapper::new(&wizard.mapping(), wizard.layout()),
            wizard,
            mapped: Some(0),
            state: GamepadState::new(),
        }
    }

    /// The raw elements the wizard reads.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn layout(&self) -> &RawLayout {
        self.wizard.layout()
    }

    /// Feeds a raw state read at `now`, and maps it with what is recorded.
    pub(crate) fn feed(&mut self, raw: &RawState, now: f64) {
        self.wizard.update(raw, now);
        let outcomes = self.wizard.outcomes().len();
        if self.mapped != Some(outcomes) {
            self.mapper = Mapper::new(&self.wizard.mapping(), self.wizard.layout());
            self.mapped = Some(outcomes);
        }
        if raw.fits(self.wizard.layout()) {
            self.mapper.apply(raw, &mut self.state);
        }
    }
}

#[wasm_bindgen]
impl MappingWizard {
    /// What to show.
    #[wasm_bindgen(unchecked_return_type = "WizardStatus")]
    pub fn status(&self) -> Result<JsValue, JsError> {
        to_js(&WizardStatus::from(self.wizard.status()))
    }

    /// What each completed step recorded, in order.
    #[wasm_bindgen(unchecked_return_type = "WizardResult[]")]
    pub fn results(&self) -> Result<JsValue, JsError> {
        let results: Vec<WizardResult> = self
            .wizard
            .steps()
            .iter()
            .zip(self.wizard.outcomes())
            .map(|(&step, outcome)| WizardResult {
                step: step.into(),
                inputs: match outcome {
                    Outcome::Mapped(inputs) => inputs
                        .iter()
                        .map(|&(target, input)| WizardInput {
                            target: target.into(),
                            input: input.to_string(),
                        })
                        .collect(),
                    Outcome::Missing => Vec::new(),
                },
                missing: *outcome == Outcome::Missing,
            })
            .collect();
        to_js(&results)
    }

    /// The controls the person said the pad does not have.
    #[wasm_bindgen(unchecked_return_type = "WizardMissing")]
    pub fn missing(&self) -> Result<JsValue, JsError> {
        let (buttons, axes) = self.wizard.missing();
        to_js(&WizardMissing {
            buttons: buttons.iter().collect(),
            axes: axes.iter().collect(),
        })
    }

    /// The pad's canonical state through the mapping recorded so far, as of
    /// the last `feedWizard()`.
    #[wasm_bindgen(js_name = mappedState, unchecked_return_type = "GamepadState")]
    pub fn mapped_state(&self) -> Result<JsValue, JsError> {
        to_js(&self.state)
    }

    /// Records the current control as not on this pad, once everything is
    /// released.
    pub fn skip(&mut self) {
        self.wizard.skip();
    }

    /// Asks for the previous control again.
    pub fn back(&mut self) {
        self.wizard.back();
        self.mapped = None;
    }

    /// Starts over.
    pub fn restart(&mut self) {
        self.wizard.restart();
        self.mapped = None;
    }
}

/// The raw layout of a decoded pad: its canonical controls, as a provider
/// that normalizes them would list them.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn canonical_layout() -> RawLayout {
    use anygamepad::model::raw::{RawAxisInfo, RawSource};
    RawLayout {
        buttons: Button::STANDARD
            .into_iter()
            .map(RawSource::Button)
            .collect(),
        axes: Axis::ALL
            .into_iter()
            .map(|axis| RawAxisInfo {
                source: RawSource::Axis(axis),
                min: if matches!(axis, Axis::LeftTrigger | Axis::RightTrigger) {
                    0.0
                } else {
                    -1.0
                },
                max: 1.0,
            })
            .collect(),
        ..RawLayout::default()
    }
}

/// A decoded state as the raw state of [`canonical_layout`].
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn canonical_raw(state: &GamepadState, raw: &mut RawState) {
    for (value, source) in raw.buttons.iter_mut().zip(Button::STANDARD) {
        *value = if state.pressed(source) { 1.0 } else { 0.0 };
    }
    for (value, axis) in raw.axes.iter_mut().zip(Axis::ALL) {
        *value = state.axis(axis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_states_map_through_the_canonical_layout() {
        let layout = canonical_layout();
        let mut raw = RawState::for_layout(&layout);
        let mut state = GamepadState::new();
        state.set_pressed(Button::East, true);
        state.set_axis(Axis::LeftTrigger, 0.5);
        canonical_raw(&state, &mut raw);

        let mut wizard = MappingWizard::new(layout);
        wizard.feed(&raw, 0.0);
        // Before any step, canonical elements drive their own control.
        assert!(wizard.state.pressed(Button::East));
        assert_eq!(wizard.state.axis(Axis::LeftTrigger), 0.5);
        assert!(!wizard.state.pressed(Button::South));
    }
}
