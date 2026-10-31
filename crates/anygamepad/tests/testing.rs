//! The test runner (H3) end to end: a simulated person follows the
//! prompts of the Quick and Full plans on built pads (a standard pad, a
//! decoded DualSense read over HID, a pad that lacks controls and a pad
//! whose mapping swaps two buttons), and the reports validate.

use std::f32::consts::TAU;

use anygamepad::db::catalog::Model;
use anygamepad::db::{Catalog, TestPlan};
use anygamepad::model::identity::{
    Backend, Browser, BrowserEngine, DeviceIdentity, Os, ProviderKind,
};
use anygamepad::model::raw::{RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad::model::{
    Button, ControllerMode, GamepadState, MotionSample, Output, OutputCommand, OutputSet, Power,
    PowerState, Sensor, SensorSet, TouchPoint, TouchpadInfo, Transport, Vec3,
};
use anygamepad::testing::{Prompt, Session, SessionSetup};
use anygamepad::web::standard;
use anygamepad_report::test_report::{
    Configuration, Selection, SessionStart, StepStatus, TestReport,
};
use anygamepad_report::{App, Date, Host, Report, Source, View};

const CHROME: Browser = Browser {
    engine: BrowserEngine::Blink,
    major_version: Some(141),
};

/// A built pad and what the simulated person does with it.
struct Pad {
    model: &'static str,
    mode: &'static str,
    /// Raw buttons: the 17 standard ones, then touchpad click and mute.
    buttons: usize,
    motion: bool,
    touchpad: bool,
    battery: bool,
    outputs: OutputSet,
    /// Buttons the pad does not have: the person presses south and east.
    lacks: Vec<Button>,
    /// The mapping swaps south and east.
    swapped: bool,
    /// The app reads HID input reports.
    hid: bool,
}

impl Pad {
    fn standard() -> Self {
        Self {
            model: "generic/xinput",
            mode: "xinput",
            buttons: 17,
            motion: false,
            touchpad: false,
            battery: false,
            outputs: OutputSet::from([Output::Rumble]),
            lacks: Vec::new(),
            swapped: false,
            hid: false,
        }
    }

    fn dualsense() -> Self {
        Self {
            model: "sony/dualsense",
            mode: "ps5",
            buttons: 19,
            motion: true,
            touchpad: true,
            battery: true,
            outputs: OutputSet::from([Output::Rumble, Output::Lightbar, Output::PlayerLeds]),
            hid: true,
            ..Self::standard()
        }
    }

    fn layout(&self) -> RawLayout {
        RawLayout {
            buttons: (0..self.buttons as u16).map(RawSource::Index).collect(),
            axes: (0..4)
                .map(|index| RawAxisInfo {
                    source: RawSource::Index(index),
                    min: -1.0,
                    max: 1.0,
                })
                .collect(),
            sensors: if self.motion {
                SensorSet::from([Sensor::Gyroscope, Sensor::Accelerometer])
            } else {
                SensorSet::default()
            },
            touchpads: if self.touchpad {
                vec![TouchpadInfo {
                    max_fingers: 2,
                    width: 1920,
                    height: 1080,
                }]
            } else {
                Vec::new()
            },
            battery: self.battery,
            ..RawLayout::default()
        }
    }

    fn identity(&self) -> DeviceIdentity {
        let (vendor, product) = match self.model {
            "sony/dualsense" => (0x054c, 0x0ce6),
            _ => (0x045e, 0x028e),
        };
        DeviceIdentity {
            vendor_id: Some(vendor),
            product_id: Some(product),
            transport: Transport::Usb,
            browser: Some(CHROME),
            ..DeviceIdentity::new(ProviderKind::Web, Backend::WebHid, Os::Windows)
        }
    }

    fn model(&self) -> Model {
        Catalog::embedded().get(self.model).unwrap().clone()
    }

    /// The raw index of a canonical button.
    fn index(&self, button: Button) -> usize {
        match button {
            Button::TouchpadClick => 17,
            Button::Mute => 18,
            _ => standard::BUTTONS.iter().position(|&b| b == button).unwrap(),
        }
    }

    /// The canonical state of `raw`, as the app's mapping gives it.
    fn canonical(&self, raw: &RawState, previous: &GamepadState) -> GamepadState {
        let mut state = GamepadState::new();
        standard::map(raw, &mut state);
        for (index, button) in [(17, Button::TouchpadClick), (18, Button::Mute)] {
            if raw.buttons.get(index).is_some_and(|&v| v >= 0.5) {
                state.set_pressed(button, true);
            }
            if index < raw.buttons.len() {
                state
                    .buttons
                    .remove(Button::extra((index - 17) as u8).unwrap());
            }
        }
        if self.swapped {
            let (south, east) = (state.pressed(Button::South), state.pressed(Button::East));
            state.set_pressed(Button::South, east);
            state.set_pressed(Button::East, south);
        }
        state.touches = raw.touches;
        state.motion = raw.motion.last().copied().or(previous.motion);
        state.power = raw.power;
        state
    }

    /// What the person does `k` ticks (10 ms) into `prompt`.
    fn act(&self, prompt: Prompt, k: u32, raw: &mut RawState) {
        let window = (5..15).contains(&k);
        match prompt {
            Prompt::Press(button) if window => {
                if self.lacks.contains(&button) {
                    raw.buttons[0] = 1.0;
                    raw.buttons[1] = 1.0;
                } else {
                    raw.buttons[self.index(button)] = 1.0;
                }
            }
            Prompt::Circle(side) if (5..105).contains(&k) => {
                let angle = (k - 5) as f32 / 100.0 * TAU;
                let first = if side == anygamepad::db::plan::Side::Left {
                    0
                } else {
                    2
                };
                raw.axes[first] = angle.cos();
                raw.axes[first + 1] = -angle.sin();
            }
            Prompt::Pull(side) if (5..45).contains(&k) => {
                let index = if side == anygamepad::db::plan::Side::Left {
                    6
                } else {
                    7
                };
                raw.buttons[index] = ((k - 5) as f32 / 30.0).min(1.0);
            }
            Prompt::MoveSticks => {
                let angle = k as f32 / 50.0 * TAU;
                raw.axes[0] = angle.cos();
                raw.axes[1] = angle.sin();
            }
            Prompt::PutDown => {
                raw.motion = vec![MotionSample {
                    timestamp_us: u64::from(k) * 10_000,
                    gyro: Some(Vec3 {
                        x: 0.01,
                        y: -0.005,
                        z: 0.0,
                    }),
                    accel: Some(Vec3 {
                        x: 0.1,
                        y: 9.8,
                        z: -0.2,
                    }),
                }];
            }
            Prompt::TurnLeft => {
                let turning = (5..25).contains(&k);
                raw.motion = vec![MotionSample {
                    timestamp_us: u64::from(k) * 10_000,
                    gyro: Some(Vec3 {
                        x: 0.05,
                        y: if turning { 2.0 } else { 0.0 },
                        z: 0.1,
                    }),
                    accel: Some(Vec3 {
                        x: 0.0,
                        y: 9.8,
                        z: 0.0,
                    }),
                }];
            }
            Prompt::Touch { .. } => {
                let point = |id, x, y| {
                    Some(TouchPoint {
                        touchpad: 0,
                        id,
                        x,
                        y,
                    })
                };
                match k / 10 {
                    1 => raw.touches[0] = point(0, 0.05, 0.05),
                    2 => raw.touches[0] = point(0, 0.95, 0.05),
                    3 => raw.touches[0] = point(0, 0.05, 0.95),
                    4 => raw.touches[0] = point(0, 0.95, 0.95),
                    5 => {
                        raw.touches[0] = point(0, 0.4, 0.5);
                        raw.touches[1] = point(1, 0.6, 0.5);
                    }
                    _ => {}
                }
            }
            Prompt::Ask(_) if window => raw.buttons[self.index(Button::South)] = 1.0,
            _ => {}
        }
    }
}

fn device(pad: &Pad) -> Report {
    let source = Source {
        app: App::Web,
        app_version: "0.0.4".parse().unwrap(),
        library_version: "0.0.4".parse().unwrap(),
    };
    let host = Host {
        os: Os::Windows,
        os_version: None,
        browser: Some(CHROME),
    };
    let mut report = Report::new(source, host, Date::new(2026, 10, 31).unwrap());
    report
        .views
        .push(View::new(pad.identity(), pad.layout(), pad.outputs));
    report
}

fn selection(pad: &Pad) -> Selection {
    let model = pad.model();
    Selection {
        unit: None,
        model: model.id.clone(),
        mode: pad.mode.into(),
        kind: model.mode(pad.mode).unwrap().kind,
        transport: Transport::Usb,
    }
}

/// Runs `plan` on `pad` with the simulated person; returns the report and
/// the output commands the session sent.
fn run(pad: &Pad, plan: &str) -> (TestReport, Vec<OutputCommand>) {
    let configuration = Configuration {
        start: SessionStart::Connecting,
        confirmed: selection(pad),
        detected: Some(selection(pad)),
    };
    let mut session = Session::new(SessionSetup {
        plan: TestPlan::get(plan).unwrap().clone(),
        model: pad.model(),
        configuration,
        device: device(pad),
        outputs: pad.outputs,
        hid: pad.hid,
    })
    .unwrap();
    let layout = pad.layout();
    let mut state = GamepadState::new();
    let mut commands = Vec::new();
    let (mut prompt, mut step, mut k) = (Prompt::Wait, usize::MAX, 0);
    for tick in 0..100_000u32 {
        let now = f64::from(tick) * 10.0;
        let status = session.status();
        if status.prompt == Prompt::Done {
            break;
        }
        if status.prompt != prompt || status.step != step {
            (prompt, step, k) = (status.prompt, status.step, 0);
        }
        // Touch and answer prompts change while the step runs.
        if matches!(prompt, Prompt::Touch { .. }) && k == 0 {
            k = 1;
        }
        let mut raw = RawState::for_layout(&layout);
        raw.power = Power {
            state: PowerState::OnBattery,
            percent: Some(80),
        };
        pad.act(prompt, k, &mut raw);
        match prompt {
            Prompt::Unplug if k == 5 => session.removed(now),
            Prompt::Reconnect if k == 5 => session.added(&pad.identity(), now),
            _ => {}
        }
        state = pad.canonical(&raw, &state);
        if pad.hid {
            let byte = (tick % 256) as u8;
            session.input_report(0x01, &[byte; 63], now);
        }
        session.update(&raw, &state, now);
        commands.extend(session.take_commands());
        k += 1;
    }
    assert!(session.is_done(), "stuck at {:?}", session.status());
    (session.finish(), commands)
}

fn statuses(report: &TestReport) -> Vec<(String, StepStatus, Option<String>)> {
    report
        .results
        .iter()
        .map(|r| (r.step.clone(), r.status, r.observed.clone()))
        .collect()
}

fn failures(report: &TestReport) -> Vec<(String, StepStatus, Option<String>)> {
    statuses(report)
        .into_iter()
        .filter(|(_, status, _)| *status != StepStatus::Pass)
        .collect()
}

#[test]
fn quick_plan_on_a_standard_pad() {
    let (report, _) = run(&Pad::standard(), "quick");
    report.validate().unwrap();
    assert_eq!(failures(&report), Vec::new());
    // Detection, then the 17 buttons of the generic XInput model.
    assert_eq!(report.results.len(), 18);
    assert_eq!(report.results[1].step, "controls.south");
    assert_eq!(report.plan.id, "quick");
    let frames = report.results[1].evidence.as_ref().unwrap().frames.len();
    assert!(frames >= 2, "{frames}");
}

#[test]
fn full_plan_on_a_standard_pad() {
    let (report, commands) = run(&Pad::standard(), "full");
    report.validate().unwrap();
    assert_eq!(failures(&report), Vec::new(), "{:#?}", statuses(&report));
    let steps: Vec<&str> = report.results.iter().map(|r| r.step.as_str()).collect();
    for step in [
        "sticks.left.circle",
        "triggers.right",
        "rate",
        "outputs.rumble",
        "hotplug",
    ] {
        assert!(steps.contains(&step), "{step}");
    }
    // No motion, touchpad or battery on this model: those steps are left out.
    assert!(!steps.contains(&"motion.rest") && !steps.contains(&"touchpad"));
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, OutputCommand::Rumble(r) if r.low_frequency == 1.0))
    );
    let circle = report
        .results
        .iter()
        .find(|r| r.step == "sticks.left.circle")
        .unwrap();
    assert!(circle.measurements["reach_up"] > 0.95);
    assert!(circle.measurements["circularity"] > 0.9);
    let rumble = report
        .results
        .iter()
        .find(|r| r.step == "outputs.rumble")
        .unwrap();
    assert_eq!(
        rumble.answer,
        Some(anygamepad_report::test_report::Answer::Yes)
    );
}

#[test]
fn full_plan_on_a_decoded_dualsense() {
    let pad = Pad::dualsense();
    let (report, commands) = run(&pad, "full");
    report.validate().unwrap();
    assert_eq!(failures(&report), Vec::new(), "{:#?}", statuses(&report));
    let find = |step: &str| report.results.iter().find(|r| r.step == step).unwrap();
    assert_eq!(find("controls.mute").status, StepStatus::Pass);
    assert_eq!(find("motion.rest").observed.as_deref(), Some("accel_y+"));
    assert_eq!(find("motion.turn").observed.as_deref(), Some("gyro_y+"));
    assert_eq!(find("battery").measurements["percent"], 80.0);
    assert!(find("rate").measurements["reports_per_second"] > 50.0);
    // HID: the evidence holds input reports, not raw states.
    let evidence = find("controls.south").evidence.as_ref().unwrap();
    assert!(evidence.frames.is_empty() && !evidence.reports.is_empty());
    assert!(commands.contains(&OutputCommand::PlayerNumber(4)));
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, OutputCommand::Lightbar(rgb) if rgb.g == 255))
    );
}

#[test]
fn a_pad_that_lacks_controls() {
    let pad = Pad {
        model: "generic/hid",
        mode: "dinput",
        lacks: vec![Button::Home, Button::RightStick],
        ..Pad::standard()
    };
    let (report, _) = run(&pad, "quick");
    report.validate().unwrap();
    let failures = failures(&report);
    assert_eq!(
        failures,
        vec![
            (
                "controls.right_stick".into(),
                StepStatus::NotApplicable,
                Some("not on this pad".into())
            ),
            (
                "controls.home".into(),
                StepStatus::NotApplicable,
                Some("not on this pad".into())
            ),
        ]
    );
    let missing = report.device.user.missing_buttons;
    assert!(missing.contains(Button::Home) && missing.contains(Button::RightStick));
    // Generic models have no identities: detection has nothing to compare.
    assert_eq!(report.results[0].status, StepStatus::Pass);
}

#[test]
fn a_pad_that_answers_with_the_wrong_control() {
    let pad = Pad {
        swapped: true,
        ..Pad::standard()
    };
    let (report, _) = run(&pad, "quick");
    report.validate().unwrap();
    assert_eq!(
        failures(&report),
        vec![
            (
                "controls.south".into(),
                StepStatus::Fail,
                Some("east".into())
            ),
            (
                "controls.east".into(),
                StepStatus::Fail,
                Some("south".into())
            ),
        ]
    );
}

#[test]
fn detection_mismatch_and_skips() {
    let pad = Pad::standard();
    let mut detected = selection(&pad);
    detected.model = "microsoft/xbox-360".into();
    let configuration = Configuration {
        start: SessionStart::Connecting,
        confirmed: selection(&pad),
        detected: Some(detected),
    };
    let mut session = Session::new(SessionSetup {
        plan: TestPlan::get("quick").unwrap().clone(),
        model: pad.model(),
        configuration,
        device: device(&pad),
        outputs: pad.outputs,
        hid: false,
    })
    .unwrap();
    let layout = pad.layout();
    let raw = RawState::for_layout(&layout);
    let state = GamepadState::new();
    session.update(&raw, &state, 0.0);
    assert_eq!(session.report().results[0].status, StepStatus::Fail);
    assert_eq!(
        session.report().results[0].observed.as_deref(),
        Some("microsoft/xbox-360 xinput usb")
    );
    assert_eq!(session.status().prompt, Prompt::Release);
    for t in 1..20 {
        session.update(&raw, &state, f64::from(t) * 10.0);
    }
    assert_eq!(session.status().prompt, Prompt::Press(Button::South));
    assert_eq!(
        session.status().prompt.text(session.face_layout()),
        "Press the bottom face button (A), then let go."
    );
    session.skip(200.0);
    session.not_on_this_pad(210.0);
    let report = session.finish();
    report.validate().unwrap();
    assert_eq!(report.results[1].status, StepStatus::Skipped);
    // Not on this pad only applies once the next step started.
    assert_eq!(report.results.len(), 2);
}

#[test]
fn setup_errors() {
    let pad = Pad::standard();
    let mut configuration = Configuration {
        start: SessionStart::List,
        confirmed: selection(&pad),
        detected: None,
    };
    configuration.confirmed.mode = "turbo".into();
    let setup = SessionSetup {
        plan: TestPlan::get("quick").unwrap().clone(),
        model: pad.model(),
        configuration,
        device: device(&pad),
        outputs: pad.outputs,
        hid: false,
    };
    assert!(Session::new(setup).is_err());
    let _ = ControllerMode::Xinput;
}
