//! The test runner (roadmap task H3, ADR 0008): the test modes of the web
//! and desktop apps run a [`Session`] over a [`TestPlan`] and get a
//! [`TestReport`].
//!
//! A session is a state machine without I/O, like the guided wizard
//! ([`wizard`](crate::wizard)) whose thresholds it shares. The app feeds it
//! once a frame the device's raw state and canonical state
//! ([`Session::update`]), the input reports when its API reads HID
//! ([`Session::input_report`]), the device going away and coming back
//! ([`Session::removed`], [`Session::added`]), and what the person says
//! ([`Session::answer`], [`Session::skip`], [`Session::not_on_this_pad`]).
//! It shows the current step's [`Prompt`] ([`Session::status`]), sends the
//! output commands the step needs ([`Session::take_commands`]), records the
//! input of each step as evidence, and builds the report
//! ([`Session::finish`]).
//!
//! The plan's steps are expanded for the model under test: one step per
//! button the model has in the confirmed mode, and no step for what it
//! lacks (no touchpad step for a pad without one). A step the API cannot
//! read (motion through the Gamepad API) or an output the provider cannot
//! drive ends as not applicable.
//!
//! The person can answer yes/no questions and say "not on this pad" from
//! the pad: south is yes, east is no, north is not sure; south and east
//! together say the pad lacks the button asked for, as in the wizard.
//!
//! Requires the `testing` feature.

use anygamepad_db::Input;
use anygamepad_db::catalog::{Controls, Model};
use anygamepad_db::detect::transports_fit;
use anygamepad_db::plan::{CheckKind, Side, TestPlan};
use anygamepad_model::identity::DeviceIdentity;
use anygamepad_model::raw::{Hat, RawLayout, RawState};
use anygamepad_model::{
    Axis, Button, ButtonLabel, ButtonSet, FaceLayout, GamepadState, Output, OutputCommand,
    OutputSet, PowerState, Rgb, Rumble, Sensor, Vec3,
};
use anygamepad_report::Report;
use anygamepad_report::privacy::is_identifying_input_report;
use anygamepad_report::test_report::{
    Answer, Configuration, Evidence, Frame, MAX_FRAMES, MAX_INPUT_REPORTS, Millis, PlanRef,
    StepResult, StepStatus, TestReport, TimedReport,
};

use crate::wizard::{MOVE, PRESS, RELEASED};

/// How long everything must stay released to end a press, in milliseconds.
const RELEASE_MS: f64 = 80.0;
/// How long to let a pad settle before measuring it at rest.
const SETTLE_MS: f64 = 400.0;
/// A stick counts as pushed from this far from its centre.
const PUSHED: f32 = 0.3;
/// A stick counts as back at its centre within this.
const CENTRED: f32 = 0.2;
/// A circle sector counts as reached from this far.
const SECTOR_REACHED: f32 = 0.7;
/// A trigger counts as released below this.
const TRIGGER_RELEASED: f32 = 0.1;
/// A trigger at rest reads at most this.
const TRIGGER_REST: f32 = 0.05;
/// Gyroscope readings of a pad at rest stay below this, in rad/s.
const GYRO_REST: f32 = 0.05;
/// A turn counts from this rate, in rad/s.
const GYRO_TURN: f32 = 1.0;
/// Acceleration at rest is within this of standard gravity, in m/s².
const GRAVITY_TOLERANCE: f32 = 1.0;
/// Output scripts: how long each command lasts, in milliseconds.
const OUTPUT_STEP_MS: f64 = 700.0;

/// The buttons in the order the steps ask for them; extra buttons follow.
const BUTTON_ORDER: [Button; 25] = [
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
    Button::Share,
    Button::Capture,
    Button::Mute,
    Button::TouchpadClick,
    Button::LeftPaddle1,
    Button::LeftPaddle2,
    Button::RightPaddle1,
    Button::RightPaddle2,
];

/// What the person is asked to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prompt {
    /// Nothing yet: the next update starts the step.
    Wait,
    /// Let go of the pad: something is still pressed from the last step.
    Release,
    /// Press and release this button.
    Press(Button),
    /// Leave the sticks alone.
    LeaveSticks,
    /// Turn this stick around its edge.
    Circle(Side),
    /// Pull this trigger all the way, slowly, and let go.
    Pull(Side),
    /// Move both sticks around until told to stop.
    MoveSticks,
    /// Lay the pad flat on a table, face up, and let go.
    PutDown,
    /// Keep the pad flat and turn it to the left.
    TurnLeft,
    /// Touch the touchpad's four corners, then use two fingers at once.
    Touch {
        /// Corners touched: top left, top right, bottom left, bottom right.
        corners: [bool; 4],
        /// Whether two fingers were seen at once.
        two_fingers: bool,
    },
    /// Watch or feel the pad: the app is sending this output.
    Watch(Output),
    /// Say whether this output worked: yes, no or not sure.
    Ask(Output),
    /// Unplug the pad or turn it off.
    Unplug,
    /// Connect the pad again.
    Reconnect,
    /// Every step ran.
    Done,
}

impl Prompt {
    /// The prompt in English, with the face buttons named as printed.
    pub fn text(&self, face: FaceLayout) -> String {
        match *self {
            Self::Wait => "Get ready.".into(),
            Self::Release => "Let go of every button and stick.".into(),
            Self::Press(button) => format!("Press {}, then let go.", button_name(button, face)),
            Self::LeaveSticks => "Leave the sticks alone.".into(),
            Self::Circle(side) => format!(
                "Turn the {} stick around its edge, slowly, then let it go.",
                side.as_str()
            ),
            Self::Pull(side) => format!(
                "Pull the {} trigger all the way, slowly, then let it go.",
                side.as_str()
            ),
            Self::MoveSticks => "Move both sticks around.".into(),
            Self::PutDown => "Lay the pad flat on a table, face up, and let go.".into(),
            Self::TurnLeft => "Keep the pad flat and turn it to the left.".into(),
            Self::Touch { .. } => {
                "Touch each corner of the touchpad, then touch it with two fingers.".into()
            }
            Self::Watch(output) => format!("Watch the pad: testing {}.", output_name(output)),
            Self::Ask(output) => format!(
                "Did {} work? South: yes, east: no, north: not sure.",
                output_name(output)
            ),
            Self::Unplug => "Unplug the pad or turn it off.".into(),
            Self::Reconnect => "Connect the pad again.".into(),
            Self::Done => "Done.".into(),
        }
    }
}

fn output_name(output: Output) -> &'static str {
    match output {
        Output::Rumble => "the vibration (strong motor, then weak motor)",
        Output::Lightbar => "the light (red, green, then blue)",
        Output::PlayerLeds => "the player lights (one to four)",
        _ => "the output",
    }
}

/// How a prompt names a button, with the face buttons' printed labels.
pub fn button_name(button: Button, face: FaceLayout) -> String {
    let label = face.label(button);
    let printed = match label {
        ButtonLabel::A => Some("A"),
        ButtonLabel::B => Some("B"),
        ButtonLabel::X => Some("X"),
        ButtonLabel::Y => Some("Y"),
        ButtonLabel::Cross => Some("Cross"),
        ButtonLabel::Circle => Some("Circle"),
        ButtonLabel::Square => Some("Square"),
        ButtonLabel::Triangle => Some("Triangle"),
        _ => None,
    };
    let position = match button {
        Button::South => "the bottom face button",
        Button::East => "the right face button",
        Button::West => "the left face button",
        Button::North => "the top face button",
        Button::DPadUp => "up on the d-pad",
        Button::DPadDown => "down on the d-pad",
        Button::DPadLeft => "left on the d-pad",
        Button::DPadRight => "right on the d-pad",
        Button::LeftShoulder => "the left shoulder button",
        Button::RightShoulder => "the right shoulder button",
        Button::LeftTrigger => "the left trigger",
        Button::RightTrigger => "the right trigger",
        Button::LeftStick => "the left stick (click it)",
        Button::RightStick => "the right stick (click it)",
        Button::Select => "the left menu button",
        Button::Start => "the right menu button",
        Button::Home => "the system button",
        Button::Share => "the share button",
        Button::Capture => "the capture button",
        Button::Mute => "the microphone button",
        Button::TouchpadClick => "the touchpad (click it)",
        Button::LeftPaddle1 => "the upper left back button",
        Button::LeftPaddle2 => "the lower left back button",
        Button::RightPaddle1 => "the upper right back button",
        Button::RightPaddle2 => "the lower right back button",
        Button::Extra(extra) => return format!("extra button {}", extra.index()),
        _ => return button.as_str().replace('_', " "),
    };
    match printed {
        Some(printed) => format!("{position} ({printed})"),
        None => position.into(),
    }
}

/// Where the session is.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The current step, from 0; equal to `total` when done.
    pub step: usize,
    /// Steps in the session.
    pub total: usize,
    /// The current step's ID.
    pub step_id: Option<String>,
    /// What the person is asked to do.
    pub prompt: Prompt,
}

/// Why a session cannot start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidSetup(String);

impl core::fmt::Display for InvalidSetup {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "cannot start the test session: {}", self.0)
    }
}

impl std::error::Error for InvalidSetup {}

/// What a session needs.
#[derive(Clone, Debug)]
pub struct SessionSetup {
    /// The plan to run.
    pub plan: TestPlan,
    /// The catalog model of the confirmed configuration.
    pub model: Model,
    /// What the person confirmed, and what detection proposed.
    pub configuration: Configuration,
    /// The device report of the device under test; its first view is the
    /// API the session reads.
    pub device: Report,
    /// The outputs the provider can drive on this device.
    pub outputs: OutputSet,
    /// Whether the app feeds input reports ([`Session::input_report`]):
    /// the evidence then holds them instead of raw states.
    pub hid: bool,
}

/// A step, expanded for the model.
#[derive(Clone, Debug, PartialEq)]
struct Step {
    id: String,
    check: &'static str,
    kind: Kind,
    /// Why the step does not apply, decided when it was expanded.
    not_applicable: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Detection,
    Button(Button),
    StickRest {
        side: Side,
        seconds: f32,
        tolerance: f32,
    },
    StickCircle {
        side: Side,
        tolerance: f32,
    },
    TriggerPull {
        side: Side,
        tolerance: f32,
    },
    Rate {
        seconds: f32,
    },
    MotionRest {
        seconds: f32,
    },
    MotionTurn,
    Touchpad,
    Battery,
    Output(Output),
    Hotplug,
}

/// The state of the current step.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// The step has not started.
    Pending,
    /// Waiting for the pad to be released before listening.
    Release {
        since: Option<f64>,
    },
    Button {
        seen: ButtonSet,
        raw: Option<Input>,
        chord: bool,
        held: bool,
        released_since: Option<f64>,
    },
    StickRest {
        samples: Vec<(f32, f32)>,
    },
    StickCircle {
        sectors: [f32; 8],
        /// Up, down, left, right.
        reach: [f32; 4],
        centred_since: Option<f64>,
    },
    TriggerPull {
        rest: f32,
        max: f32,
        between: bool,
        released_since: Option<f64>,
    },
    Rate {
        since: Option<f64>,
        updates: u32,
        reports: u32,
    },
    MotionRest {
        gyro: Vec<f32>,
        accel: Vec<Vec3>,
    },
    MotionTurn {
        peak: Option<(usize, f32)>,
    },
    Touchpad {
        corners: [bool; 4],
        two: bool,
    },
    Output {
        asking: bool,
        quiet: bool,
    },
    Hotplug {
        removed: bool,
    },
}

/// A test session. See the [module documentation](self).
#[derive(Clone, Debug)]
pub struct Session {
    report: TestReport,
    model: Model,
    mode: String,
    steps: Vec<Step>,
    index: usize,
    phase: Phase,
    started: f64,
    layout: RawLayout,
    face: FaceLayout,
    hid: bool,
    vendor_id: Option<u16>,
    /// Where each raw axis rests, once seen with nothing pressed.
    rest: Option<Vec<f32>>,
    evidence: Evidence,
    last_frame: Option<RawState>,
    commands: Vec<OutputCommand>,
    /// When the current output script started.
    script_started: f64,
    /// Commands of the output script already sent.
    script_sent: usize,
    now: f64,
}

impl Session {
    /// A session over `setup`: the plan's steps expanded for the model in
    /// the confirmed mode.
    pub fn new(setup: SessionSetup) -> Result<Self, InvalidSetup> {
        let SessionSetup {
            plan,
            model,
            configuration,
            device,
            outputs,
            hid,
        } = setup;
        let confirmed = &configuration.confirmed;
        if confirmed.model != model.id {
            return Err(InvalidSetup(format!(
                "the confirmed model is `{}`, not `{}`",
                confirmed.model, model.id
            )));
        }
        let Some(mode) = model.mode(&confirmed.mode) else {
            return Err(InvalidSetup(format!(
                "`{}` has no mode `{}`",
                model.id, confirmed.mode
            )));
        };
        let Some(view) = device.views.first() else {
            return Err(InvalidSetup("the device report has no view".into()));
        };
        let layout = view.layout.clone();
        let vendor_id = view.identity.vendor_id;
        let controls = model.controls_in(mode).clone();
        let steps = expand(&plan, &controls, &layout, outputs);
        let mode = mode.id.clone();
        let plan = PlanRef {
            id: plan.id.clone(),
            version: plan.version,
        };
        let face = model.face_layout();
        Ok(Self {
            report: TestReport::new(device, configuration, plan),
            model,
            mode,
            steps,
            index: 0,
            phase: Phase::Pending,
            started: 0.0,
            layout,
            face,
            hid,
            vendor_id,
            rest: None,
            evidence: Evidence::default(),
            last_frame: None,
            commands: Vec::new(),
            script_started: 0.0,
            script_sent: 0,
            now: 0.0,
        })
    }

    /// Where the session is and what the person is asked to do.
    pub fn status(&self) -> Status {
        let step = self.steps.get(self.index);
        Status {
            step: self.index,
            total: self.steps.len(),
            step_id: step.map(|step| step.id.clone()),
            prompt: self.prompt(),
        }
    }

    /// What is printed on the face buttons of the model under test.
    pub fn face_layout(&self) -> FaceLayout {
        self.face
    }

    /// Whether every step ran.
    pub fn is_done(&self) -> bool {
        self.index >= self.steps.len()
    }

    /// The report so far: the results of the steps that ended.
    pub fn report(&self) -> &TestReport {
        &self.report
    }

    /// Sets the person's note on the session.
    pub fn set_notes(&mut self, notes: Option<String>) {
        self.report.notes = notes;
    }

    /// The output commands to send to the device now, in order.
    pub fn take_commands(&mut self) -> Vec<OutputCommand> {
        std::mem::take(&mut self.commands)
    }

    /// The report, [scrubbed](TestReport::scrub): anonymous, with the
    /// results of the steps that ended. Outputs the session drove are
    /// stopped first ([`take_commands`](Self::take_commands) before
    /// dropping the session).
    pub fn finish(mut self) -> TestReport {
        self.report.scrub();
        self.report
    }

    fn prompt(&self) -> Prompt {
        let Some(step) = self.steps.get(self.index) else {
            return Prompt::Done;
        };
        match (&self.phase, step.kind) {
            (Phase::Pending, _) => Prompt::Wait,
            (Phase::Release { .. }, _) => Prompt::Release,
            (_, Kind::Button(button)) => Prompt::Press(button),
            (_, Kind::StickRest { .. }) => Prompt::LeaveSticks,
            (_, Kind::StickCircle { side, .. }) => Prompt::Circle(side),
            (_, Kind::TriggerPull { side, .. }) => Prompt::Pull(side),
            (_, Kind::Rate { .. }) => Prompt::MoveSticks,
            (_, Kind::MotionRest { .. }) => Prompt::PutDown,
            (_, Kind::MotionTurn) => Prompt::TurnLeft,
            (Phase::Touchpad { corners, two }, _) => Prompt::Touch {
                corners: *corners,
                two_fingers: *two,
            },
            (Phase::Output { asking: true, .. }, Kind::Output(output)) => Prompt::Ask(output),
            (_, Kind::Output(output)) => Prompt::Watch(output),
            (Phase::Hotplug { removed: false }, _) => Prompt::Unplug,
            (Phase::Hotplug { removed: true }, _) => Prompt::Reconnect,
            _ => Prompt::Wait,
        }
    }

    /// Feeds the device's raw state and its canonical state at `now`
    /// (milliseconds, any origin, never going back).
    pub fn update(&mut self, raw: &RawState, state: &GamepadState, now: f64) {
        self.now = now;
        let fits = raw.buttons.len() == self.layout.buttons.len()
            && raw.axes.len() == self.layout.axes.len()
            && raw.hats.len() == self.layout.hats.len();
        if !fits || self.is_done() {
            return;
        }
        if self.phase == Phase::Pending {
            self.start(raw, state, now);
            if self.is_done() || self.phase == Phase::Pending {
                return;
            }
        }
        self.learn_rest(raw);
        self.record_frame(raw, now);
        self.step(raw, state, now);
    }

    /// Feeds an input report the device sent at `now`, when the app reads
    /// HID; it goes into the current step's evidence.
    pub fn input_report(&mut self, report_id: u8, data: &[u8], now: f64) {
        if let Phase::Rate {
            since: Some(_),
            reports,
            ..
        } = &mut self.phase
        {
            *reports += 1;
        }
        if !self.hid
            || matches!(self.phase, Phase::Pending)
            || self.evidence.reports.len() >= MAX_INPUT_REPORTS
            || is_identifying_input_report(self.vendor_id, report_id)
        {
            return;
        }
        self.evidence.reports.push(TimedReport {
            t: Millis::from_ms(now - self.started),
            report_id,
            data: anygamepad_report::HexBytes(data.to_vec()),
        });
    }

    /// The device went away at `now`.
    pub fn removed(&mut self, now: f64) {
        self.now = now;
        if let Phase::Hotplug { removed } = &mut self.phase {
            *removed = true;
        }
    }

    /// A device appeared at `now`: the one under test coming back, for the
    /// hotplug step.
    pub fn added(&mut self, identity: &DeviceIdentity, now: f64) {
        self.now = now;
        if self.phase != (Phase::Hotplug { removed: true }) {
            return;
        }
        let expected = self.report.device.views[0].fingerprint;
        let same = identity.fingerprint() == expected;
        let (status, observed) = if same {
            (StepStatus::Pass, "the same identity")
        } else {
            (StepStatus::Fail, "another identity")
        };
        self.end(
            status,
            Some("the same identity".into()),
            Some(observed.into()),
            None,
            Vec::new(),
        );
    }

    /// The person's answer to the current question.
    pub fn answer(&mut self, answer: Answer, now: f64) {
        self.now = now;
        if let Phase::Output { asking: true, .. } = self.phase {
            let status = match answer {
                Answer::Yes => StepStatus::Pass,
                Answer::No => StepStatus::Fail,
                Answer::NotSure => StepStatus::Skipped,
            };
            self.end(status, None, None, Some(answer), Vec::new());
        }
    }

    /// The person skips the current step.
    pub fn skip(&mut self, now: f64) {
        self.now = now;
        if !self.is_done() && self.phase != Phase::Pending {
            self.stop_outputs();
            self.end(StepStatus::Skipped, None, None, None, Vec::new());
        }
    }

    /// The person says the pad does not have what the current step asks
    /// for: the step does not apply, and a missing button goes into the
    /// device report.
    pub fn not_on_this_pad(&mut self, now: f64) {
        self.now = now;
        if self.is_done() || self.phase == Phase::Pending {
            return;
        }
        if let Kind::Button(button) = self.steps[self.index].kind {
            self.report.device.user.missing_buttons.insert(button);
        }
        self.stop_outputs();
        self.end(
            StepStatus::NotApplicable,
            None,
            Some("not on this pad".into()),
            None,
            Vec::new(),
        );
    }

    /// Repeats the current output test.
    pub fn repeat(&mut self, now: f64) {
        if let Phase::Output { asking, quiet } = &mut self.phase {
            *asking = false;
            *quiet = false;
            self.script_started = now;
            self.script_sent = 0;
        }
    }

    fn start(&mut self, raw: &RawState, state: &GamepadState, now: f64) {
        while let Some(step) = self.steps.get(self.index) {
            self.started = now;
            self.evidence = Evidence::default();
            self.last_frame = None;
            if let Some(reason) = step.not_applicable {
                self.phase = Phase::Release { since: None };
                self.end(
                    StepStatus::NotApplicable,
                    None,
                    Some(reason.into()),
                    None,
                    Vec::new(),
                );
                continue;
            }
            match step.kind {
                Kind::Detection => {
                    self.phase = Phase::Release { since: None };
                    self.detection();
                    continue;
                }
                Kind::Battery => {
                    self.phase = Phase::Release { since: None };
                    let power = state.power;
                    let (status, observed) = if power.state == PowerState::Unknown {
                        (StepStatus::Fail, "unknown")
                    } else {
                        (StepStatus::Pass, power.state.as_str())
                    };
                    let measurements = power
                        .percent
                        .map(|percent| ("percent", f64::from(percent)))
                        .into_iter()
                        .collect();
                    self.end(status, None, Some(observed.into()), None, measurements);
                    continue;
                }
                Kind::Output(_) => {
                    self.phase = Phase::Output {
                        asking: false,
                        quiet: false,
                    };
                    self.script_started = now;
                    self.script_sent = 0;
                }
                Kind::Hotplug => self.phase = Phase::Hotplug { removed: false },
                Kind::MotionRest { .. } | Kind::StickRest { .. } => {
                    self.phase = self.listening(step.kind, raw, state);
                }
                _ => self.phase = Phase::Release { since: None },
            }
            return;
        }
    }

    /// The phase that listens for `kind`.
    fn listening(&self, kind: Kind, _raw: &RawState, state: &GamepadState) -> Phase {
        match kind {
            Kind::Button(_) => Phase::Button {
                seen: ButtonSet::default(),
                raw: None,
                chord: false,
                held: false,
                released_since: None,
            },
            Kind::StickRest { .. } => Phase::StickRest {
                samples: Vec::new(),
            },
            Kind::StickCircle { .. } => Phase::StickCircle {
                sectors: [0.0; 8],
                reach: [0.0; 4],
                centred_since: None,
            },
            Kind::TriggerPull { side, .. } => Phase::TriggerPull {
                rest: state.axis(trigger_axis(side)),
                max: 0.0,
                between: false,
                released_since: None,
            },
            Kind::Rate { .. } => Phase::Rate {
                since: None,
                updates: 0,
                reports: 0,
            },
            Kind::MotionRest { .. } => Phase::MotionRest {
                gyro: Vec::new(),
                accel: Vec::new(),
            },
            Kind::MotionTurn => Phase::MotionTurn { peak: None },
            Kind::Touchpad => Phase::Touchpad {
                corners: [false; 4],
                two: false,
            },
            _ => Phase::Release { since: None },
        }
    }

    fn step(&mut self, raw: &RawState, state: &GamepadState, now: f64) {
        let kind = self.steps[self.index].kind;
        let elapsed = now - self.started;
        match &mut self.phase {
            Phase::Pending => {}
            Phase::Release { since } => {
                if quiet(raw, state) {
                    let since = *since.get_or_insert(now);
                    if now - since >= RELEASE_MS {
                        self.phase = self.listening(kind, raw, state);
                    }
                } else {
                    *since = None;
                }
            }
            Phase::Button { .. } => self.button(kind, raw, state, now),
            Phase::StickRest { samples } => {
                let Kind::StickRest {
                    side,
                    seconds,
                    tolerance,
                } = kind
                else {
                    return;
                };
                if elapsed < SETTLE_MS {
                    return;
                }
                let stick = stick_of(state, side);
                samples.push((stick.0, stick.1));
                if elapsed >= SETTLE_MS + f64::from(seconds) * 1000.0 {
                    let samples = std::mem::take(samples);
                    self.stick_rest(&samples, tolerance);
                }
            }
            Phase::StickCircle {
                sectors,
                reach,
                centred_since,
            } => {
                let Kind::StickCircle { side, tolerance } = kind else {
                    return;
                };
                let (x, y) = stick_of(state, side);
                let radius = (x * x + y * y).sqrt();
                if radius >= PUSHED {
                    let angle = y.atan2(x).rem_euclid(core::f32::consts::TAU);
                    let sector = ((angle / core::f32::consts::TAU * 8.0 + 0.5) as usize) % 8;
                    sectors[sector] = sectors[sector].max(radius);
                    reach[0] = reach[0].max(y);
                    reach[1] = reach[1].max(-y);
                    reach[2] = reach[2].max(-x);
                    reach[3] = reach[3].max(x);
                }
                let around = sectors.iter().all(|&r| r >= SECTOR_REACHED);
                if around && radius < CENTRED {
                    let since = *centred_since.get_or_insert(now);
                    if now - since >= RELEASE_MS {
                        let (sectors, reach) = (*sectors, *reach);
                        self.stick_circle(sectors, reach, tolerance);
                    }
                } else {
                    *centred_since = None;
                }
            }
            Phase::TriggerPull {
                rest,
                max,
                between,
                released_since,
            } => {
                let Kind::TriggerPull { side, tolerance } = kind else {
                    return;
                };
                let value = state.axis(trigger_axis(side));
                *max = max.max(value);
                if (0.2..=0.8).contains(&value) {
                    *between = true;
                }
                if *max >= 0.5 && value <= TRIGGER_RELEASED {
                    let since = *released_since.get_or_insert(now);
                    if now - since >= RELEASE_MS {
                        let (rest, max, between) = (*rest, *max, *between);
                        let status = if max >= tolerance && rest <= TRIGGER_REST && between {
                            StepStatus::Pass
                        } else {
                            StepStatus::Fail
                        };
                        let observed = if between { "analog" } else { "digital" };
                        self.end(
                            status,
                            Some("analog".into()),
                            Some(observed.into()),
                            None,
                            vec![("max", f64::from(max)), ("rest", f64::from(rest))],
                        );
                    }
                } else {
                    *released_since = None;
                }
            }
            Phase::Rate { since, updates, .. } => {
                let Kind::Rate { seconds } = kind else { return };
                let moving = [Side::Left, Side::Right].iter().any(|&side| {
                    let (x, y) = stick_of(state, side);
                    (x * x + y * y).sqrt() >= PUSHED
                });
                match since {
                    None if moving => *since = Some(now),
                    None => {}
                    Some(start) => {
                        *updates += 1;
                        let window = now - *start;
                        if window >= f64::from(seconds) * 1000.0 {
                            self.rate(window);
                        }
                    }
                }
            }
            Phase::MotionRest { gyro, accel } => {
                let Kind::MotionRest { seconds } = kind else {
                    return;
                };
                if elapsed < SETTLE_MS {
                    return;
                }
                let samples: Vec<_> = if raw.motion.is_empty() {
                    state.motion.into_iter().collect()
                } else {
                    raw.motion.clone()
                };
                for sample in samples {
                    if let Some(g) = sample.gyro {
                        gyro.push(length(g));
                    }
                    if let Some(a) = sample.accel {
                        accel.push(a);
                    }
                }
                if elapsed >= SETTLE_MS + f64::from(seconds) * 1000.0 {
                    let (gyro, accel) = (std::mem::take(gyro), std::mem::take(accel));
                    self.motion_rest(&gyro, &accel);
                }
            }
            Phase::MotionTurn { peak } => {
                let samples: Vec<_> = if raw.motion.is_empty() {
                    state.motion.into_iter().collect()
                } else {
                    raw.motion.clone()
                };
                let mut still = false;
                for gyro in samples.iter().filter_map(|sample| sample.gyro) {
                    let components = [gyro.x, gyro.y, gyro.z];
                    let (axis, value) = components
                        .iter()
                        .copied()
                        .enumerate()
                        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                        .unwrap_or((0, 0.0));
                    if value.abs() >= GYRO_TURN && peak.is_none_or(|(_, p)| value.abs() > p.abs()) {
                        *peak = Some((axis, value));
                    }
                    still = length(gyro) < GYRO_TURN * 0.3;
                }
                if let (Some((axis, value)), true) = (*peak, still) {
                    let observed = format!(
                        "gyro_{}{}",
                        ["x", "y", "z"][axis],
                        if value > 0.0 { "+" } else { "-" }
                    );
                    let status = if observed == "gyro_y+" {
                        StepStatus::Pass
                    } else {
                        StepStatus::Fail
                    };
                    self.end(
                        status,
                        Some("gyro_y+".into()),
                        Some(observed),
                        None,
                        vec![("peak", f64::from(value))],
                    );
                }
            }
            Phase::Touchpad { corners, two } => {
                let touches: Vec<_> = state.active_touches().collect();
                if touches.len() >= 2 {
                    *two = true;
                }
                for touch in &touches {
                    let (left, top) = (touch.x < 0.25, touch.y < 0.25);
                    let (right, bottom) = (touch.x > 0.75, touch.y > 0.75);
                    for (index, hit) in [left && top, right && top, left && bottom, right && bottom]
                        .into_iter()
                        .enumerate()
                    {
                        corners[index] |= hit;
                    }
                }
                if corners.iter().all(|&c| c) && *two && touches.is_empty() {
                    self.end(
                        StepStatus::Pass,
                        None,
                        Some("four corners and two fingers".into()),
                        None,
                        Vec::new(),
                    );
                }
            }
            Phase::Output {
                asking,
                quiet: was_quiet,
            } => {
                let Kind::Output(output) = kind else { return };
                if !*asking {
                    let script = script(output);
                    while let Some(&(at, command)) = script.get(self.script_sent) {
                        if now - self.script_started < at {
                            break;
                        }
                        self.commands.push(command);
                        self.script_sent += 1;
                    }
                    if self.script_sent >= script.len() {
                        *asking = true;
                        *was_quiet = false;
                    }
                    return;
                }
                // Answers from the pad: a press after the pad was quiet.
                let pressed = |button| state.pressed(button);
                if quiet(raw, state) {
                    *was_quiet = true;
                    return;
                }
                if !*was_quiet {
                    return;
                }
                let answer = if pressed(Button::South) && !pressed(Button::East) {
                    Some(Answer::Yes)
                } else if pressed(Button::East) && !pressed(Button::South) {
                    Some(Answer::No)
                } else if pressed(Button::North) {
                    Some(Answer::NotSure)
                } else {
                    None
                };
                if let Some(answer) = answer {
                    self.answer(answer, now);
                    // The next step waits for this press to end.
                }
            }
            Phase::Hotplug { .. } => {}
        }
    }

    fn button(&mut self, kind: Kind, raw: &RawState, state: &GamepadState, now: f64) {
        let Kind::Button(expected) = kind else { return };
        let raw_active = self.raw_activation(raw);
        let Phase::Button {
            seen,
            raw: raw_seen,
            chord,
            held,
            released_since,
        } = &mut self.phase
        else {
            return;
        };
        let canonical = state.buttons;
        let in_use = !canonical.is_empty() || raw_active.is_some();
        if in_use {
            *held = true;
            *released_since = None;
            *seen |= canonical;
            if raw_seen.is_none() {
                *raw_seen = raw_active;
            }
            if !matches!(expected, Button::South | Button::East)
                && canonical.contains(Button::South)
                && canonical.contains(Button::East)
            {
                *chord = true;
            }
            return;
        }
        if !*held {
            return;
        }
        let since = *released_since.get_or_insert(now);
        if now - since < RELEASE_MS {
            return;
        }
        let (seen, raw_seen, chord) = (*seen, *raw_seen, *chord);
        if chord {
            self.report.device.user.missing_buttons.insert(expected);
            self.end(
                StepStatus::NotApplicable,
                Some(expected.as_str().into()),
                Some("not on this pad".into()),
                None,
                Vec::new(),
            );
            return;
        }
        let names: Vec<&str> = BUTTON_ORDER
            .iter()
            .copied()
            .chain(seen.iter().filter(|b| matches!(b, Button::Extra(_))))
            .filter(|&button| seen.contains(button))
            .map(Button::as_str)
            .collect();
        let (status, observed) = if seen.contains(expected) && seen.iter().count() == 1 {
            (StepStatus::Pass, expected.as_str().to_owned())
        } else if !names.is_empty() {
            (StepStatus::Fail, truncate(&names.join("+")))
        } else if let Some(input) = raw_seen {
            (StepStatus::Fail, truncate(&input.to_string()))
        } else {
            (StepStatus::Fail, "nothing".to_owned())
        };
        self.end(
            status,
            Some(expected.as_str().into()),
            Some(observed),
            None,
            Vec::new(),
        );
    }

    /// The first raw element in use: a button past half its travel, a hat
    /// off centre, an axis half its range from rest.
    fn raw_activation(&self, raw: &RawState) -> Option<Input> {
        if let Some(index) = raw.buttons.iter().position(|&value| value >= PRESS) {
            return Some(Input::Button(self.layout.buttons[index]));
        }
        if let Some(index) = raw.hats.iter().position(|&hat| hat != Hat::CENTERED) {
            return Some(Input::Hat {
                source: self.layout.hats[index],
                mask: raw.hats[index].0,
            });
        }
        let rest = self.rest.as_ref()?;
        raw.axes.iter().enumerate().find_map(|(index, &value)| {
            let info = &self.layout.axes[index];
            let half = ((info.max - info.min) / 2.0).abs().max(f32::EPSILON);
            let deviation = (value - rest[index]) / half;
            (value.is_finite() && deviation.abs() >= MOVE).then_some(Input::Axis {
                source: info.source,
                range: if deviation > 0.0 {
                    anygamepad_db::AxisRange::Positive
                } else {
                    anygamepad_db::AxisRange::Negative
                },
                inverted: false,
            })
        })
    }

    fn learn_rest(&mut self, raw: &RawState) {
        let released = raw.buttons.iter().all(|&v| v.is_nan() || v < RELEASED)
            && raw.hats.iter().all(|&hat| hat == Hat::CENTERED);
        if !released {
            return;
        }
        let moved = self.rest.as_ref().is_some_and(|rest| {
            raw.axes
                .iter()
                .zip(rest)
                .enumerate()
                .any(|(index, (&value, &at))| {
                    let info = &self.layout.axes[index];
                    let half = ((info.max - info.min) / 2.0).abs().max(f32::EPSILON);
                    value.is_finite() && ((value - at) / half).abs() >= MOVE
                })
        });
        if moved {
            return;
        }
        let centre = |index: usize| {
            let info = &self.layout.axes[index];
            (info.min + info.max) / 2.0
        };
        self.rest = Some(
            raw.axes
                .iter()
                .enumerate()
                .map(|(index, &value)| {
                    if value.is_finite() {
                        value
                    } else {
                        centre(index)
                    }
                })
                .collect(),
        );
    }

    fn record_frame(&mut self, raw: &RawState, now: f64) {
        let recording = matches!(
            self.phase,
            Phase::Button { .. }
                | Phase::StickRest { .. }
                | Phase::StickCircle { .. }
                | Phase::TriggerPull { .. }
                | Phase::Rate { .. }
                | Phase::MotionRest { .. }
                | Phase::MotionTurn { .. }
                | Phase::Touchpad { .. }
        );
        if self.hid || !recording || self.evidence.frames.len() >= MAX_FRAMES {
            return;
        }
        if self.last_frame.as_ref() == Some(raw) {
            return;
        }
        self.evidence.frames.push(Frame {
            t: Millis::from_ms(now - self.started),
            state: raw.clone(),
        });
        self.last_frame = Some(raw.clone());
    }

    fn detection(&mut self) {
        let configuration = self.report.configuration.clone();
        let confirmed = &configuration.confirmed;
        let identity = self.report.device.views[0].identity.clone();
        let mode = self.model.mode(&self.mode).expect("checked in new");
        let expected: Vec<_> = mode
            .connections
            .iter()
            .filter(|connection| transports_fit(confirmed.transport, connection.transport))
            .flat_map(|connection| &connection.identities)
            .collect();
        let identity_matches = expected.iter().any(|expected| expected.matches(&identity));
        let ids = match (identity.vendor_id, identity.product_id) {
            (Some(vendor), Some(product)) => format!("{vendor:04x}:{product:04x}"),
            _ => "no vendor and product IDs".into(),
        };
        let selection = |s: &anygamepad_report::test_report::Selection| {
            truncate(&format!("{} {} {}", s.model, s.mode, s.transport))
        };
        let (status, observed) = if configuration.detection_differs() {
            (
                StepStatus::Fail,
                selection(configuration.detected.as_ref().expect("differs")),
            )
        } else if !expected.is_empty() && !identity_matches {
            (StepStatus::Fail, ids)
        } else if expected.is_empty() && configuration.detected.is_none() {
            (StepStatus::NotApplicable, ids)
        } else {
            (StepStatus::Pass, selection(confirmed))
        };
        self.end(
            status,
            Some(selection(confirmed)),
            Some(observed),
            None,
            Vec::new(),
        );
    }

    fn stick_rest(&mut self, samples: &[(f32, f32)], tolerance: f32) {
        if samples.is_empty() {
            self.end(
                StepStatus::Error,
                None,
                Some("no input".into()),
                None,
                Vec::new(),
            );
            return;
        }
        let n = samples.len() as f32;
        let mean_x = samples.iter().map(|s| s.0).sum::<f32>() / n;
        let mean_y = samples.iter().map(|s| s.1).sum::<f32>() / n;
        let noise = samples
            .iter()
            .map(|s| ((s.0 - mean_x).powi(2) + (s.1 - mean_y).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        let offset = (mean_x * mean_x + mean_y * mean_y).sqrt();
        let status = if offset <= tolerance {
            StepStatus::Pass
        } else {
            StepStatus::Fail
        };
        self.end(
            status,
            None,
            None,
            None,
            vec![
                ("offset_x", f64::from(mean_x)),
                ("offset_y", f64::from(mean_y)),
                ("noise", f64::from(noise)),
            ],
        );
    }

    fn stick_circle(&mut self, sectors: [f32; 8], reach: [f32; 4], tolerance: f32) {
        let outer = sectors
            .iter()
            .copied()
            .fold(0.0f32, f32::max)
            .max(f32::EPSILON);
        let circularity = sectors.iter().sum::<f32>() / 8.0 / outer;
        let status = if reach.iter().all(|&r| r >= tolerance) {
            StepStatus::Pass
        } else {
            StepStatus::Fail
        };
        self.end(
            status,
            None,
            None,
            None,
            vec![
                ("reach_up", f64::from(reach[0])),
                ("reach_down", f64::from(reach[1])),
                ("reach_left", f64::from(reach[2])),
                ("reach_right", f64::from(reach[3])),
                ("circularity", f64::from(circularity)),
            ],
        );
    }

    fn rate(&mut self, window: f64) {
        let Phase::Rate {
            updates, reports, ..
        } = self.phase
        else {
            return;
        };
        let seconds = window / 1000.0;
        let mut measurements = vec![("updates_per_second", f64::from(updates) / seconds)];
        if self.hid {
            measurements.push(("reports_per_second", f64::from(reports) / seconds));
        }
        let status = if updates > 0 {
            StepStatus::Pass
        } else {
            StepStatus::Fail
        };
        self.end(status, None, None, None, measurements);
    }

    fn motion_rest(&mut self, gyro: &[f32], accel: &[Vec3]) {
        if gyro.is_empty() && accel.is_empty() {
            self.end(
                StepStatus::Fail,
                None,
                Some("no motion samples".into()),
                None,
                Vec::new(),
            );
            return;
        }
        let mut measurements = Vec::new();
        let mut ok = true;
        if !gyro.is_empty() {
            let mean = gyro.iter().sum::<f32>() / gyro.len() as f32;
            measurements.push(("gyro_rest", f64::from(mean)));
            ok &= mean <= GYRO_REST;
        }
        let mut observed = None;
        if !accel.is_empty() {
            let n = accel.len() as f32;
            let mean = Vec3 {
                x: accel.iter().map(|a| a.x).sum::<f32>() / n,
                y: accel.iter().map(|a| a.y).sum::<f32>() / n,
                z: accel.iter().map(|a| a.z).sum::<f32>() / n,
            };
            let magnitude = length(mean);
            measurements.push(("accel_magnitude", f64::from(magnitude)));
            for (name, value) in [
                ("gravity_x", mean.x),
                ("gravity_y", mean.y),
                ("gravity_z", mean.z),
            ] {
                measurements.push((name, f64::from(value / magnitude.max(f32::EPSILON))));
            }
            ok &= (magnitude - anygamepad_model::STANDARD_GRAVITY).abs() <= GRAVITY_TOLERANCE;
            let components = [mean.x, mean.y, mean.z];
            let (axis, value) = components
                .iter()
                .copied()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .unwrap_or((1, 0.0));
            let up = format!(
                "accel_{}{}",
                ["x", "y", "z"][axis],
                if value > 0.0 { "+" } else { "-" }
            );
            ok &= up == "accel_y+";
            observed = Some(up);
        }
        let status = if ok {
            StepStatus::Pass
        } else {
            StepStatus::Fail
        };
        let expected = observed.as_ref().map(|_| "accel_y+".to_owned());
        self.end(status, expected, observed, None, measurements);
    }

    fn stop_outputs(&mut self) {
        if let Some(Kind::Output(output)) = self.steps.get(self.index).map(|step| step.kind) {
            if output == Output::Rumble {
                self.commands.push(OutputCommand::Rumble(Rumble {
                    low_frequency: 0.0,
                    high_frequency: 0.0,
                }));
            }
        }
    }

    /// Ends the current step with this result, keeps its evidence, and
    /// moves to the next step.
    fn end(
        &mut self,
        status: StepStatus,
        expected: Option<String>,
        observed: Option<String>,
        answer: Option<Answer>,
        measurements: Vec<(&str, f64)>,
    ) {
        let Some(step) = self.steps.get(self.index) else {
            return;
        };
        let evidence = std::mem::take(&mut self.evidence);
        let has_evidence = !evidence.frames.is_empty() || !evidence.reports.is_empty();
        self.report.results.push(StepResult {
            step: step.id.clone(),
            check: step.check.into(),
            status,
            expected,
            observed,
            answer,
            measurements: measurements
                .into_iter()
                .filter(|(_, value)| value.is_finite())
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
            duration_ms: (self.now - self.started).clamp(0.0, f64::from(u32::MAX)) as u32,
            note: None,
            evidence: has_evidence.then_some(evidence),
        });
        self.index += 1;
        self.phase = Phase::Pending;
        self.last_frame = None;
    }
}

/// Every button released and every hat centred, in raw and canonical form.
fn quiet(raw: &RawState, state: &GamepadState) -> bool {
    state.buttons.is_empty()
        && raw.buttons.iter().all(|&v| v.is_nan() || v < RELEASED)
        && raw.hats.iter().all(|&hat| hat == Hat::CENTERED)
}

fn stick_of(state: &GamepadState, side: Side) -> (f32, f32) {
    let stick = match side {
        Side::Left => state.left_stick(),
        Side::Right => state.right_stick(),
    };
    (stick.x, stick.y)
}

fn trigger_axis(side: Side) -> Axis {
    match side {
        Side::Left => Axis::LeftTrigger,
        Side::Right => Axis::RightTrigger,
    }
}

fn length(v: Vec3) -> f32 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

fn truncate(text: &str) -> String {
    text.chars().take(64).collect()
}

/// The commands of an output test, with when to send them (milliseconds
/// from its start). The last entry ends the script.
fn script(output: Output) -> Vec<(f64, OutputCommand)> {
    let rumble = |low, high| {
        OutputCommand::Rumble(Rumble {
            low_frequency: low,
            high_frequency: high,
        })
    };
    let rgb = |r, g, b| OutputCommand::Lightbar(Rgb { r, g, b });
    match output {
        Output::Rumble => vec![
            (0.0, rumble(1.0, 0.0)),
            (OUTPUT_STEP_MS, rumble(0.0, 1.0)),
            (2.0 * OUTPUT_STEP_MS, rumble(0.0, 0.0)),
        ],
        Output::Lightbar => vec![
            (0.0, rgb(255, 0, 0)),
            (OUTPUT_STEP_MS, rgb(0, 255, 0)),
            (2.0 * OUTPUT_STEP_MS, rgb(0, 0, 255)),
            (3.0 * OUTPUT_STEP_MS, rgb(0, 0, 255)),
        ],
        Output::PlayerLeds => (1..=4)
            .map(|n| {
                (
                    f64::from(n - 1) * OUTPUT_STEP_MS,
                    OutputCommand::PlayerNumber(n),
                )
            })
            .chain([(4.0 * OUTPUT_STEP_MS, OutputCommand::PlayerNumber(1))])
            .collect(),
        _ => Vec::new(),
    }
}

/// The plan's steps for a model with `controls`, read through `layout`,
/// with `outputs` drivable.
fn expand(
    plan: &TestPlan,
    controls: &Controls,
    layout: &RawLayout,
    outputs: OutputSet,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut push =
        |id: String, check: &'static str, kind: Kind, not_applicable: Option<&'static str>| {
            steps.push(Step {
                id,
                check,
                kind,
                not_applicable,
            });
        };
    for step in &plan.steps {
        let check = step.check.as_str();
        let side = step.side.unwrap_or(Side::Left);
        let tolerance = step.tolerance;
        match step.check {
            CheckKind::Detection => push(step.id.clone(), check, Kind::Detection, None),
            CheckKind::Buttons => {
                let extras = controls
                    .buttons
                    .iter()
                    .filter(|b| matches!(b, Button::Extra(_)));
                for button in BUTTON_ORDER.iter().copied().chain(extras) {
                    if controls.buttons.contains(button) {
                        let id = format!("{}.{}", step.id, button.as_str());
                        push(id, check, Kind::Button(button), None);
                    }
                }
            }
            CheckKind::StickRest | CheckKind::StickCircle => {
                let axes = match side {
                    Side::Left => [Axis::LeftStickX, Axis::LeftStickY],
                    Side::Right => [Axis::RightStickX, Axis::RightStickY],
                };
                if axes.iter().all(|&axis| controls.axes.contains(axis)) {
                    let kind = if step.check == CheckKind::StickRest {
                        Kind::StickRest {
                            side,
                            seconds: step.seconds.unwrap_or(1.5),
                            tolerance: tolerance.unwrap_or(0.15),
                        }
                    } else {
                        Kind::StickCircle {
                            side,
                            tolerance: tolerance.unwrap_or(0.9),
                        }
                    };
                    push(step.id.clone(), check, kind, None);
                }
            }
            CheckKind::TriggerPull => {
                if controls.axes.contains(trigger_axis(side)) {
                    let kind = Kind::TriggerPull {
                        side,
                        tolerance: tolerance.unwrap_or(0.95),
                    };
                    push(step.id.clone(), check, kind, None);
                }
            }
            CheckKind::Rate => {
                let kind = Kind::Rate {
                    seconds: step.seconds.unwrap_or(3.0),
                };
                push(step.id.clone(), check, kind, None);
            }
            CheckKind::MotionRest | CheckKind::MotionTurn => {
                let needed = if step.check == CheckKind::MotionTurn {
                    controls.sensors.contains(Sensor::Gyroscope)
                } else {
                    !controls.sensors.is_empty()
                };
                if needed {
                    let reads = if step.check == CheckKind::MotionTurn {
                        layout.has_sensor(Sensor::Gyroscope)
                    } else {
                        !layout.sensors.is_empty()
                    };
                    let kind = if step.check == CheckKind::MotionTurn {
                        Kind::MotionTurn
                    } else {
                        Kind::MotionRest {
                            seconds: step.seconds.unwrap_or(1.5),
                        }
                    };
                    let reason = (!reads).then_some("the API does not read motion");
                    push(step.id.clone(), check, kind, reason);
                }
            }
            CheckKind::Touchpad => {
                if !controls.touchpads.is_empty() {
                    let reason = layout
                        .touchpads
                        .is_empty()
                        .then_some("the API does not read the touchpad");
                    push(step.id.clone(), check, Kind::Touchpad, reason);
                }
            }
            CheckKind::Battery => {
                if controls.battery {
                    let reason = (!layout.battery).then_some("the API does not read the battery");
                    push(step.id.clone(), check, Kind::Battery, reason);
                }
            }
            CheckKind::Rumble | CheckKind::Lightbar | CheckKind::PlayerLeds => {
                let output = match step.check {
                    CheckKind::Rumble => Output::Rumble,
                    CheckKind::Lightbar => Output::Lightbar,
                    _ => Output::PlayerLeds,
                };
                if controls.outputs.contains(output) {
                    let reason = (!outputs.contains(output)).then_some("the API cannot drive it");
                    push(step.id.clone(), check, Kind::Output(output), reason);
                }
            }
            CheckKind::Hotplug => push(step.id.clone(), check, Kind::Hotplug, None),
        }
    }
    steps
}
