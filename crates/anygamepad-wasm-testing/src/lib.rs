//! WebAssembly bindings of the web app's test mode (roadmap tasks H4 and
//! W9, ADR 0008), a module of its own that only the test mode's page
//! (`/test/`) loads: the controller catalog, the lab inventory and the
//! test plans for the page's lists, automatic detection ([`identify`],
//! [`check_choice`]), and [`TestSession`], the library's test runner
//! (`anygamepad::testing`).
//!
//! It reads no device. The tester's module (`anygamepad-wasm`) describes
//! each device as a [`TestDevice`] (`Gamepads.testDevice()`,
//! `HidDevices.testDevice()`) and, once a frame, gives what it read as a
//! [`TestInput`] (`testInput()`); the page sends the commands of
//! [`TestSession::take_commands`] back through it. The two modules share no
//! memory, only these values.
//!
//! ```js
//! const testing = await import("./wasm-testing/anygamepad_wasm_testing.js");
//! await testing.default();
//! const device = gamepads.testDevice(id, Date.now());
//! const best = testing.identify(device).candidates[0];
//! const session = testing.startTest(device, {
//!   plan: "quick",
//!   start: "connecting",
//!   confirmed: { model: best.model, mode: best.mode, transport: best.transport ?? "usb" },
//! });
//! function frame(now) {
//!   session.feed(gamepads.testInput(id), now);
//!   for (const command of session.takeCommands()) gamepads.send(id, command);
//!   show(session.status());
//! }
//! ```

use std::collections::BTreeMap;

use anygamepad::db::detect::{ChoiceCheck, Identification};
use anygamepad::db::lab::Code;
use anygamepad::db::plan::Side;
use anygamepad::db::{Catalog, Inventory, TestPlan};
use anygamepad::model::identity::DeviceIdentity;
use anygamepad::model::raw::RawState;
use anygamepad::model::{
    Button, ButtonSet, ControllerMode, FaceLayout, Family, GamepadState, Output, OutputCommand,
    OutputSet, Transport,
};
use anygamepad::testing::{Prompt, Session, SessionSetup};
use anygamepad_report::Report;
use anygamepad_report::test_report::{
    Answer, Concern, Configuration, Selection, SessionStart, StepStatus,
};
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// A device as the test mode reads it, from the tester's module
/// (`Gamepads.testDevice()`, `HidDevices.testDevice()`).
#[derive(Clone, Debug, Deserialize, Tsify)]
pub struct TestDevice {
    /// The device report of the device; its first view is the API the page
    /// reads it through.
    #[tsify(type = "Record<string, unknown>")]
    pub report: Report,
    /// The outputs the page can drive on it.
    pub outputs: OutputSet,
    /// Whether its input comes with the HID input reports
    /// (`TestInput.reports`), kept as the evidence.
    pub hid: bool,
    /// Buttons of the pad the API is known not to read.
    pub unreadable: ButtonSet,
}

impl TestDevice {
    fn from_js(device: JsValue) -> Result<Self, JsError> {
        serde_wasm_bindgen::from_value(device).map_err(|error| JsError::new(&error.to_string()))
    }

    fn identity(&self) -> Result<&DeviceIdentity, JsError> {
        self.report
            .views
            .first()
            .map(|view| &view.identity)
            .ok_or_else(|| JsError::new("the device report has no view"))
    }
}

/// What a device read at one frame, from the tester's module
/// (`testInput()`).
#[derive(Clone, Debug, Deserialize, Tsify)]
pub struct TestInput {
    /// Its raw values, in the order of its layout.
    pub raw: RawState,
    /// Its canonical state.
    pub state: GamepadState,
    /// The HID input reports received since the last frame, oldest first.
    #[serde(default)]
    pub reports: Vec<TestInputReport>,
}

/// One HID input report.
#[derive(Clone, Debug, Deserialize, Tsify)]
pub struct TestInputReport {
    /// Its report ID (0 without IDs).
    pub report_id: u8,
    /// Its data, without the report ID.
    pub data: Vec<u8>,
    /// When it arrived (`performance.now()`).
    pub timestamp: f64,
}

fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value).map_err(|error| JsError::new(&error.to_string()))
}

/// A catalog model, for the page's list.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct CatalogModel {
    /// `<vendor>/<model>` (`sony/dualsense`).
    pub id: String,
    /// The maker, as printed on the box.
    pub vendor: String,
    /// The product name, as printed on the box.
    pub name: String,
    /// Whether it stands for every pad of a kind the catalog does not list.
    pub generic: bool,
    /// The family it is drawn as.
    pub family: Option<Family>,
    /// What is printed on its face buttons.
    pub face_layout: FaceLayout,
    /// Anything else worth knowing.
    pub notes: Option<String>,
    /// Its modes.
    pub modes: Vec<CatalogMode>,
}

/// A mode of a catalog model.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct CatalogMode {
    /// Unique in the model (`ps5`).
    pub id: String,
    /// As printed or told in the manual (`PS5`).
    pub name: String,
    /// What it imitates.
    pub kind: ControllerMode,
    /// How to put the pad in this mode.
    pub how_to: Option<String>,
    /// The ways to connect it in this mode.
    pub connections: Vec<CatalogConnection>,
}

/// One way to connect a pad in one mode.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct CatalogConnection {
    /// The transport.
    pub transport: Transport,
    /// How to connect over it.
    pub how_to: Option<String>,
    /// Whether the catalog knows the identity a pad connected this way
    /// shows, so the app can check it.
    pub known: bool,
}

/// A unit of the lab inventory.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct LabUnit {
    /// Its code (`DS5-02`).
    pub code: String,
    /// The ID of its catalog model.
    pub model: String,
    /// Edition, colour, firmware.
    pub notes: Option<String>,
}

/// A test plan, for the page's choice.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct PlanInfo {
    /// Its ID (`quick`).
    pub id: String,
    /// Its name (`Quick`).
    pub name: String,
    /// What it covers and how long it takes.
    pub description: String,
    /// Its version.
    pub version: u32,
}

/// The models of the embedded catalog, generic ones last.
#[wasm_bindgen(js_name = catalogModels, unchecked_return_type = "CatalogModel[]")]
pub fn catalog_models() -> Result<JsValue, JsError> {
    let mut models: Vec<CatalogModel> = Catalog::embedded()
        .models()
        .iter()
        .map(|model| CatalogModel {
            id: model.id.clone(),
            vendor: model.vendor.clone(),
            name: model.name.clone(),
            generic: model.generic,
            family: model.family,
            face_layout: model.face_layout(),
            notes: model.notes.clone(),
            modes: model
                .modes
                .iter()
                .map(|mode| CatalogMode {
                    id: mode.id.clone(),
                    name: mode.name.clone(),
                    kind: mode.kind,
                    how_to: mode.how_to.clone(),
                    connections: mode
                        .connections
                        .iter()
                        .map(|connection| CatalogConnection {
                            transport: connection.transport,
                            how_to: connection.how_to.clone(),
                            known: !connection.identities.is_empty(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    models.sort_by_key(|model| model.generic);
    to_js(&models)
}

/// The units of the embedded lab inventory, in inventory order.
#[wasm_bindgen(js_name = labUnits, unchecked_return_type = "LabUnit[]")]
pub fn lab_units() -> Result<JsValue, JsError> {
    let units: Vec<LabUnit> = Inventory::embedded()
        .units
        .iter()
        .map(|unit| LabUnit {
            code: unit.code.to_string(),
            model: unit.model.clone(),
            notes: unit.notes.clone(),
        })
        .collect();
    to_js(&units)
}

/// A code for a new unit of a catalog model (F4): its short name and the
/// next number free among `used`, the codes this browser knows (`DS5-03`).
#[wasm_bindgen(js_name = proposeUnitCode)]
pub fn propose_unit_code(model: &str, used: Vec<String>) -> Result<String, JsError> {
    let model = Catalog::embedded()
        .get(model)
        .ok_or_else(|| JsError::new(&format!("no catalog model `{model}`")))?;
    let code = anygamepad::db::lab::propose_code(model, used.iter().map(String::as_str));
    Ok(code.to_string())
}

/// Whether `code` can name a unit: 1 to 16 ASCII letters, digits and
/// hyphens, starting with a letter or a digit.
#[wasm_bindgen(js_name = isUnitCode)]
pub fn is_unit_code(code: &str) -> bool {
    Code::new(code).is_ok()
}

/// The embedded test plans the web app runs: all but the desktop app's
/// virtual pad plans.
#[wasm_bindgen(js_name = testPlans, unchecked_return_type = "PlanInfo[]")]
pub fn test_plans() -> Result<JsValue, JsError> {
    let plans: Vec<PlanInfo> = TestPlan::embedded()
        .iter()
        .filter(|plan| !plan.needs_virtual_pads())
        .map(|plan| PlanInfo {
            id: plan.id.clone(),
            name: plan.name.clone(),
            description: plan.description.clone(),
            version: plan.version,
        })
        .collect();
    to_js(&plans)
}

/// One thing a connected device can be.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct DetectedCandidate {
    /// The catalog model ID.
    pub model: String,
    /// The model's mode ID.
    pub mode: String,
    /// What the mode imitates.
    pub kind: ControllerMode,
    /// The transport; `undefined` when the person must say.
    pub transport: Option<Transport>,
    /// Conditions of the catalog identity that matched; 0 for a generic
    /// model.
    pub specificity: u32,
    /// The inventory's units of this model, in inventory order.
    pub units: Vec<String>,
}

/// What a connected device can be, most likely first.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct Detection {
    /// The candidates.
    pub candidates: Vec<DetectedCandidate>,
    /// Whether no model matched and the candidate is a generic model.
    pub generic: bool,
}

impl From<Identification<'_>> for Detection {
    fn from(identification: Identification<'_>) -> Self {
        let inventory = Inventory::embedded();
        Self {
            candidates: identification
                .candidates
                .iter()
                .map(|candidate| DetectedCandidate {
                    model: candidate.model.id.clone(),
                    mode: candidate.mode.id.clone(),
                    kind: candidate.mode.kind,
                    transport: candidate.transport,
                    specificity: candidate.specificity,
                    units: inventory
                        .units_of(&candidate.model.id)
                        .map(|unit| unit.code.to_string())
                        .collect(),
                })
                .collect(),
            generic: identification.generic,
        }
    }
}

/// Whether a picked model, mode and transport fits a connected device.
#[derive(Clone, Debug, Serialize, Tsify)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ChoiceResult {
    /// The device shows an identity the catalog expects for the choice.
    Matches,
    /// The catalog has no identity for the choice yet: nothing to compare.
    NotKnown,
    /// The device shows something else.
    Differs {
        /// What it looks like instead.
        seen: Detection,
    },
    /// The catalog has no such model or mode.
    NoSuchChoice,
}

/// What catalog model, mode and transport a device can be, most likely
/// first (H10).
#[wasm_bindgen(unchecked_return_type = "Detection")]
pub fn identify(
    #[wasm_bindgen(unchecked_param_type = "TestDevice")] device: JsValue,
) -> Result<JsValue, JsError> {
    let device = TestDevice::from_js(device)?;
    let detection: Detection = Catalog::embedded()
        .identify(device.identity()?, Some(Inventory::embedded()))
        .into();
    to_js(&detection)
}

/// Whether a device is the model, mode and transport picked from the
/// list.
#[wasm_bindgen(js_name = checkChoice, unchecked_return_type = "ChoiceResult")]
pub fn check_choice(
    #[wasm_bindgen(unchecked_param_type = "TestDevice")] device: JsValue,
    model: &str,
    mode: &str,
    #[wasm_bindgen(unchecked_param_type = "Transport")] transport: JsValue,
) -> Result<JsValue, JsError> {
    let device = TestDevice::from_js(device)?;
    let transport: Transport =
        serde_wasm_bindgen::from_value(transport).map_err(|e| JsError::new(&e.to_string()))?;
    let result = match Catalog::embedded().check_choice(
        device.identity()?,
        model,
        mode,
        transport,
        Some(Inventory::embedded()),
    ) {
        ChoiceCheck::Matches => ChoiceResult::Matches,
        ChoiceCheck::NotKnown => ChoiceResult::NotKnown,
        ChoiceCheck::Differs(seen) => ChoiceResult::Differs { seen: seen.into() },
        ChoiceCheck::NoSuchChoice => ChoiceResult::NoSuchChoice,
    };
    to_js(&result)
}

/// A controller in one mode over one transport, as the page gives it.
#[derive(Clone, Debug, Deserialize, Tsify)]
pub struct TestSelection {
    /// The catalog model ID.
    pub model: String,
    /// The model's mode ID.
    pub mode: String,
    /// The transport.
    pub transport: Transport,
    /// The unit's code, if the pad is in the inventory.
    #[serde(default)]
    pub unit: Option<String>,
}

/// How a session starts, as the page gives it.
#[derive(Clone, Debug, Deserialize, Tsify)]
pub struct TestSetup {
    /// The plan's ID (`quick`, `full`).
    pub plan: String,
    /// `list` when the person picked the controller first, `connecting`
    /// when they connected it without picking.
    pub start: String,
    /// What the person confirmed or picked.
    pub confirmed: TestSelection,
    /// What detection proposed.
    #[serde(default)]
    pub detected: Option<TestSelection>,
}

fn selection(selection: TestSelection) -> Result<Selection, JsError> {
    let model = Catalog::embedded()
        .get(&selection.model)
        .ok_or_else(|| JsError::new(&format!("no catalog model `{}`", selection.model)))?;
    let mode = model
        .mode(&selection.mode)
        .ok_or_else(|| JsError::new(&format!("`{}` has no mode `{}`", model.id, selection.mode)))?;
    let unit = selection
        .unit
        .map(|code| Code::new(&code).map_err(|error| JsError::new(&error.to_string())))
        .transpose()?;
    Ok(Selection {
        unit,
        model: model.id.clone(),
        mode: mode.id.clone(),
        kind: mode.kind,
        transport: selection.transport,
    })
}

/// Starts a test session on a device.
#[wasm_bindgen(js_name = startTest)]
pub fn start_test(
    #[wasm_bindgen(unchecked_param_type = "TestDevice")] device: JsValue,
    #[wasm_bindgen(unchecked_param_type = "TestSetup")] setup: JsValue,
) -> Result<TestSession, JsError> {
    let TestDevice {
        report: device,
        outputs,
        hid,
        unreadable,
    } = TestDevice::from_js(device)?;
    let setup: TestSetup =
        serde_wasm_bindgen::from_value(setup).map_err(|error| JsError::new(&error.to_string()))?;
    let plan = TestPlan::get(&setup.plan)
        .ok_or_else(|| JsError::new(&format!("no test plan `{}`", setup.plan)))?
        .clone();
    let start = match setup.start.as_str() {
        "list" => SessionStart::List,
        "connecting" => SessionStart::Connecting,
        other => return Err(JsError::new(&format!("unknown start `{other}`"))),
    };
    let confirmed = selection(setup.confirmed)?;
    let detected = setup.detected.map(selection).transpose()?;
    let model = Catalog::embedded()
        .get(&confirmed.model)
        .expect("checked by `selection`")
        .clone();
    let session = Session::new(SessionSetup {
        plan,
        model,
        configuration: Configuration {
            start,
            confirmed,
            detected,
        },
        device,
        outputs,
        hid,
        unreadable,
        virtual_targets: Vec::new(),
    })
    .map_err(|error| JsError::new(&error.to_string()))?;
    Ok(TestSession { session })
}

/// What the person is asked to do.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromptView {
    /// Nothing yet.
    Wait,
    /// Let go of the pad.
    Release,
    /// Press and release a button.
    Press {
        /// Which.
        button: Button,
    },
    /// Leave the sticks alone.
    LeaveSticks,
    /// Turn a stick around its edge.
    Circle {
        /// `left` or `right`.
        side: &'static str,
    },
    /// Pull a trigger all the way.
    Pull {
        /// `left` or `right`.
        side: &'static str,
    },
    /// Move both sticks around.
    MoveSticks,
    /// Lay the pad flat, face up.
    PutDown,
    /// Turn the pad to the left.
    TurnLeft,
    /// Touch the touchpad's corners, then two fingers.
    Touch {
        /// Top left, top right, bottom left, bottom right.
        corners: [bool; 4],
        /// Whether two fingers were seen.
        two_fingers: bool,
    },
    /// The app is driving an output.
    Watch {
        /// Which.
        output: Output,
    },
    /// Did the output work?
    Ask {
        /// Which.
        output: Output,
    },
    /// Unplug the pad or turn it off.
    Unplug,
    /// Connect it again.
    Reconnect,
    /// Every step ran.
    Done,
}

fn side(side: Side) -> &'static str {
    side.as_str()
}

impl From<Prompt> for PromptView {
    fn from(prompt: Prompt) -> Self {
        match prompt {
            Prompt::Release => Self::Release,
            Prompt::Press(button) => Self::Press { button },
            Prompt::LeaveSticks => Self::LeaveSticks,
            Prompt::Circle(s) => Self::Circle { side: side(s) },
            Prompt::Pull(s) => Self::Pull { side: side(s) },
            Prompt::MoveSticks => Self::MoveSticks,
            Prompt::PutDown => Self::PutDown,
            Prompt::TurnLeft => Self::TurnLeft,
            Prompt::Touch {
                corners,
                two_fingers,
            } => Self::Touch {
                corners,
                two_fingers,
            },
            Prompt::Watch(output) => Self::Watch { output },
            Prompt::Ask(output) => Self::Ask { output },
            Prompt::Unplug => Self::Unplug,
            Prompt::Reconnect => Self::Reconnect,
            Prompt::Done => Self::Done,
            _ => Self::Wait,
        }
    }
}

/// Where a session is.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestStatus {
    /// The current step, from 0; `total` when done.
    pub step: usize,
    /// Steps in the session.
    pub total: usize,
    /// The current step's ID.
    pub step_id: Option<String>,
    /// What the person is asked to do.
    pub prompt: PromptView,
    /// The prompt in English, face buttons named as printed.
    pub text: String,
}

/// A step's result, for the page's summary.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestResultView {
    /// The step's ID.
    pub step: String,
    /// The kind of check.
    pub check: String,
    /// `pass`, `fail`, `skipped`, `not_applicable` or `error`.
    pub status: &'static str,
    /// What was expected.
    pub expected: Option<String>,
    /// What was observed.
    pub observed: Option<String>,
    /// Measurements by name.
    pub measurements: BTreeMap<String, f64>,
}

/// How many steps ended each way.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
pub struct TestSummary {
    /// Passed.
    pub pass: usize,
    /// Failed.
    pub fail: usize,
    /// Skipped.
    pub skipped: usize,
    /// Did not apply.
    pub not_applicable: usize,
    /// Could not run.
    pub error: usize,
}

/// A text of the report, for the review before saving.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct ReviewText {
    /// The field, as a path into the report.
    pub field: String,
    /// The text.
    pub text: String,
    /// Why it deserves a look: `unknown_device_name` when the catalog does
    /// not know it for the model (a pad renamed by its owner, or a clone's
    /// own name).
    pub concern: Option<&'static str>,
}

/// The finished report and what the page shows of it.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestOutcome {
    /// The anonymous test report (`anygamepad-test-report`), as a
    /// JSON-compatible object: `JSON.stringify()` gives the file.
    #[tsify(type = "Record<string, unknown>")]
    pub report: anygamepad_report::test_report::TestReport,
    /// Its file name.
    pub file_name: String,
    /// How many steps ended each way.
    pub summary: TestSummary,
    /// Every text the report holds.
    pub review: Vec<ReviewText>,
}

const fn status_name(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pass => "pass",
        StepStatus::Fail => "fail",
        StepStatus::Skipped => "skipped",
        StepStatus::NotApplicable => "not_applicable",
        StepStatus::Error => "error",
    }
}

/// A test mode session: the library's test runner on one pad. Start it
/// with [`start_test`], feed it once a frame with what the tester's module
/// read ([`feed`](Self::feed)), send the commands it asks for, show
/// `status()`, and pass on what the person says.
#[wasm_bindgen]
pub struct TestSession {
    session: Session,
}

#[wasm_bindgen]
impl TestSession {
    /// Feeds what the device read at `now` (`performance.now()`): its input
    /// reports, then its raw and canonical state.
    pub fn feed(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "TestInput")] input: JsValue,
        now: f64,
    ) -> Result<(), JsError> {
        let input: TestInput =
            serde_wasm_bindgen::from_value(input).map_err(|e| JsError::new(&e.to_string()))?;
        for report in &input.reports {
            self.session
                .input_report(report.report_id, &report.data, report.timestamp);
        }
        self.session.update(&input.raw, &input.state, now);
        Ok(())
    }

    /// The output commands to send to the device now, in order (through
    /// the tester's `send()`).
    #[wasm_bindgen(js_name = takeCommands, unchecked_return_type = "OutputCommand[]")]
    pub fn take_commands(&mut self) -> Result<JsValue, JsError> {
        let commands: Vec<OutputCommand> = self.session.take_commands();
        to_js(&commands)
    }

    /// A device connected while the session waits for the pad under test
    /// to come back (the hotplug step), at `now`.
    pub fn added(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "TestDevice")] device: JsValue,
        now: f64,
    ) -> Result<(), JsError> {
        let device = TestDevice::from_js(device)?;
        self.session.added(device.identity()?, now);
        Ok(())
    }

    /// Where the session is and what the person is asked to do.
    #[wasm_bindgen(unchecked_return_type = "TestStatus")]
    pub fn status(&self) -> Result<JsValue, JsError> {
        let status = self.session.status();
        to_js(&TestStatus {
            step: status.step,
            total: status.total,
            step_id: status.step_id,
            prompt: status.prompt.into(),
            text: status.prompt.text(self.session.face_layout()),
        })
    }

    /// Whether every step ran.
    #[wasm_bindgen(js_name = isDone)]
    pub fn is_done(&self) -> bool {
        self.session.is_done()
    }

    /// The person's answer to the current question: `yes`, `no` or
    /// `not_sure`.
    pub fn answer(&mut self, answer: &str, now: f64) -> Result<(), JsError> {
        let answer = match answer {
            "yes" => Answer::Yes,
            "no" => Answer::No,
            "not_sure" => Answer::NotSure,
            other => return Err(JsError::new(&format!("unknown answer `{other}`"))),
        };
        self.session.answer(answer, now);
        Ok(())
    }

    /// Skips the current step.
    pub fn skip(&mut self, now: f64) {
        self.session.skip(now);
    }

    /// The pad does not have what the current step asks for.
    #[wasm_bindgen(js_name = notOnThisPad)]
    pub fn not_on_this_pad(&mut self, now: f64) {
        self.session.not_on_this_pad(now);
    }

    /// Nothing happens when the person uses the control asked for: the step
    /// fails, observing nothing.
    #[wasm_bindgen(js_name = nothingHappens)]
    pub fn nothing_happens(&mut self, now: f64) {
        self.session.nothing_happens(now);
    }

    /// Repeats the current output test.
    pub fn repeat(&mut self, now: f64) {
        self.session.repeat(now);
    }

    /// The pad went away (unplugged, turned off).
    pub fn removed(&mut self, now: f64) {
        self.session.removed(now);
    }

    /// The person's note on the session; `undefined` or empty clears it.
    #[wasm_bindgen(js_name = setNotes)]
    pub fn set_notes(&mut self, notes: Option<String>) {
        self.session
            .set_notes(notes.filter(|notes| !notes.trim().is_empty()));
    }

    /// The results of the steps that ended, in order.
    #[wasm_bindgen(unchecked_return_type = "TestResultView[]")]
    pub fn results(&self) -> Result<JsValue, JsError> {
        let results: Vec<TestResultView> = self
            .session
            .report()
            .results
            .iter()
            .map(|result| TestResultView {
                step: result.step.clone(),
                check: result.check.clone(),
                status: status_name(result.status),
                expected: result.expected.clone(),
                observed: result.observed.clone(),
                measurements: result.measurements.clone(),
            })
            .collect();
        to_js_objects(&results)
    }

    /// The report so far, anonymous: the report, its file name (`n` tells it from
    /// others of the same day), summary and texts to review. The session
    /// goes on; call it again after `setNotes()`.
    #[wasm_bindgen(unchecked_return_type = "TestOutcome")]
    pub fn outcome(&self, n: u32) -> Result<JsValue, JsError> {
        let mut report = self.session.report().clone();
        report.scrub();
        report
            .validate()
            .map_err(|error| JsError::new(&error.to_string()))?;
        let summary = report.summary();
        let review = report
            .review(Catalog::embedded())
            .into_iter()
            .map(|item| ReviewText {
                field: item.field,
                text: item.text,
                concern: item.concern.map(|concern| match concern {
                    Concern::UnknownDeviceName => "unknown_device_name",
                }),
            })
            .collect();
        let file_name = report.file_name(n);
        // Maps as objects, so that `JSON.stringify()` writes the
        // measurements.
        to_js_objects(&TestOutcome {
            report,
            file_name,
            summary: TestSummary {
                pass: summary.pass,
                fail: summary.fail,
                skipped: summary.skipped,
                not_applicable: summary.not_applicable,
                error: summary.error,
            },
            review,
        })
    }
}

/// Like `to_js`, with maps as plain objects (the measurements).
fn to_js_objects<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Returns the version of the anygamepad library compiled into this module.
#[wasm_bindgen]
pub fn version() -> String {
    anygamepad::VERSION.to_owned()
}
