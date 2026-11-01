//! Test reports (H1): the committed JSON Schema and example are up to
//! date, reports round-trip through JSON, scrubbing makes them anonymous,
//! validation rejects what is not, the review flags unknown device names,
//! and a long session fits the size limit.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anygamepad_db::Catalog;
use anygamepad_db::lab::Code;
use anygamepad_model::identity::Os;
use anygamepad_model::raw::RawState;
use anygamepad_model::{ControllerMode, Transport};
use anygamepad_report::privacy::Finding;
use anygamepad_report::test_report::{
    Answer, Concern, Configuration, Evidence, Frame, MAX_TEST_REPORT_BYTES, Millis, PlanRef,
    Selection, SessionStart, StepResult, StepStatus, TestReport, TestReportError, TimedReport,
    anonymous_os_version, json_schema,
};
use anygamepad_report::{HexBytes, Report};
use common::{desktop_report, web_report};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/test-reports")
}

fn validator() -> jsonschema::Validator {
    let schema = serde_json::to_value(json_schema()).unwrap();
    jsonschema::validator_for(&schema).expect("the schema is a valid JSON Schema")
}

fn schema_errors(json: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    validator()
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

fn check_committed(name: &str, expected: &str) {
    let path = dir().join(name);
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, expected).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == expected,
        "data/test-reports/{name} is stale; regenerate it with \
         `UPDATE_SCHEMA=1 cargo test -p anygamepad-report --all-features --test test_report`"
    );
}

fn selection() -> Selection {
    Selection {
        unit: Some(Code::new("DS5-01").unwrap()),
        model: "sony/dualsense".into(),
        mode: "ps5".into(),
        kind: ControllerMode::Ps5,
        transport: Transport::Usb,
    }
}

/// A raw state of the first view of `device`, with `button` pressed.
fn frame(device: &Report, t: f64, button: Option<usize>) -> Frame {
    let mut state = RawState::for_layout(&device.views[0].layout);
    if let Some(button) = button {
        state.buttons[button] = 1.0;
    }
    Frame {
        t: Millis::from_ms(t),
        state,
    }
}

/// The example: a Quick session on a DualSense over USB, through the
/// Gamepad API in Chrome on Windows, started by connecting it.
fn example() -> TestReport {
    let mut device = web_report();
    device.user.product = None;
    let configuration = Configuration {
        start: SessionStart::Connecting,
        confirmed: selection(),
        detected: Some(selection()),
    };
    let plan = PlanRef {
        id: "quick".into(),
        version: 1,
    };
    let mut report = TestReport::new(device, configuration, plan);
    let device = &report.device;
    let south = StepResult {
        step: "controls.south".into(),
        check: "control".into(),
        status: StepStatus::Pass,
        expected: Some("south".into()),
        observed: Some("south".into()),
        answer: None,
        measurements: BTreeMap::new(),
        duration_ms: 812,
        note: None,
        evidence: Some(Evidence {
            frames: vec![
                frame(device, 0.0, None),
                frame(device, 401.25, Some(0)),
                frame(device, 520.5, None),
            ],
            reports: Vec::new(),
        }),
    };
    let circle = StepResult {
        step: "sticks.left.circle".into(),
        check: "stick_circle".into(),
        status: StepStatus::Pass,
        expected: Some("left_stick".into()),
        observed: Some("left_stick".into()),
        answer: None,
        measurements: BTreeMap::from([
            ("circularity".into(), 0.97),
            ("range_up".into(), 1.0),
            ("range_down".into(), 0.99),
        ]),
        duration_ms: 2310,
        note: None,
        evidence: None,
    };
    let rumble = StepResult {
        step: "outputs.rumble".into(),
        check: "rumble".into(),
        status: StepStatus::Pass,
        expected: None,
        observed: None,
        answer: Some(Answer::Yes),
        measurements: BTreeMap::new(),
        duration_ms: 1900,
        note: Some("Both motors.".into()),
        evidence: None,
    };
    let motion = StepResult {
        step: "motion.rest".into(),
        check: "motion_rest".into(),
        status: StepStatus::NotApplicable,
        expected: None,
        observed: None,
        answer: None,
        measurements: BTreeMap::new(),
        duration_ms: 0,
        note: None,
        evidence: None,
    };
    report.results = vec![south, circle, rumble, motion];
    report
}

#[test]
fn schema_is_up_to_date() {
    check_committed(
        "schema.json",
        &(serde_json::to_string_pretty(&json_schema()).unwrap() + "\n"),
    );
}

#[test]
fn example_is_up_to_date_and_valid() {
    let report = example();
    assert!(
        report.clone().scrub().is_empty(),
        "the example is anonymous"
    );
    let json = report.to_json_pretty().unwrap();
    check_committed("example.json", &json);
    assert_eq!(schema_errors(&json), Vec::<String>::new());
    assert_eq!(TestReport::from_json(&json).unwrap(), report);
    let summary = report.summary();
    assert_eq!((summary.pass, summary.not_applicable), (3, 1));
    assert!(!report.configuration.detection_differs());
}

#[test]
fn file_name() {
    assert_eq!(
        example().file_name(1),
        "ds5-01-ps5-usb-windows-web-gamepad_api-2026-10-14-1.json"
    );
    let mut report = example();
    report.configuration.confirmed.unit = None;
    assert_eq!(
        report.file_name(2),
        "sony-dualsense-ps5-usb-windows-web-gamepad_api-2026-10-14-2.json"
    );
}

#[test]
fn anonymous_os_versions() {
    for (os, version, kept) in [
        (Os::Linux, "6.8.0-45-generic", Some("6.8.0")),
        (Os::Linux, "6.11.4-arch1-1", Some("6.11.4")),
        (Os::Linux, "6.8.0-ana", Some("6.8.0")),
        (Os::Linux, "ana", None),
        (Os::MacOs, "14.6.1", Some("14.6.1")),
        (Os::Windows, "11 23H2", Some("11 23H2")),
        (Os::Windows, "10.0.22631 (Ana's PC)", Some("10.0.22631")),
        (Os::MacOs, "Sonoma", None),
    ] {
        assert_eq!(
            anonymous_os_version(os, version).as_deref(),
            kept,
            "{version}"
        );
    }
}

#[test]
fn scrub_makes_reports_anonymous() {
    let mut report = example();
    report.device.host.os_version = Some("10.0.22631 (Ana's PC)".into());
    report.device.user.product = Some("Ana's DualSense".into());
    report.notes = Some("Mine.\nMAC a1:b2:c3:d4:e5:f6".into());
    report.results[0].note = Some("Contact ana@example.com".into());
    report.results[0].evidence.as_mut().unwrap().reports = vec![TimedReport {
        t: Millis::from_ms(1.0),
        report_id: 0x01,
        data: HexBytes(vec![0; 63]),
    }];
    let found = report.scrub();
    let fields: Vec<(&str, Finding)> = found
        .iter()
        .map(|r| (r.field.as_str(), r.finding))
        .collect();
    assert!(fields.contains(&("device.host.os_version", Finding::OsVersionDetail)));
    assert!(fields.contains(&("device.user.product", Finding::FreeText)));
    assert!(fields.contains(&("notes", Finding::HexIdentifier)));
    assert!(fields.contains(&("results[0].note", Finding::EmailAddress)));
    assert_eq!(report.device.host.os_version.as_deref(), Some("10.0.22631"));
    assert_eq!(report.notes.as_deref(), Some("Mine.\nMAC [redacted]"));
    report.validate().unwrap();

    // Nintendo replies that can hold the Bluetooth address leave the
    // evidence.
    let mut report = example();
    report.device.views[0].identity.vendor_id = Some(0x057e);
    report.device.views[0].fingerprint = report.device.views[0].identity.fingerprint();
    report.results[0].evidence.as_mut().unwrap().reports = vec![
        TimedReport {
            t: Millis::from_ms(1.0),
            report_id: 0x30,
            data: HexBytes(vec![0; 48]),
        },
        TimedReport {
            t: Millis::from_ms(2.0),
            report_id: 0x21,
            data: HexBytes(vec![0; 48]),
        },
    ];
    assert!(report.validate().is_err());
    let found = report.scrub();
    assert_eq!(found[0].field, "results[0].evidence.reports[1]");
    assert_eq!(found[0].finding, Finding::IdentifyingReport);
    assert_eq!(
        report.results[0].evidence.as_ref().unwrap().reports.len(),
        1
    );
    report.validate().unwrap();
}

#[test]
fn validation_rejects_what_is_not_anonymous_or_well_formed() {
    type Change = Box<dyn Fn(&mut TestReport)>;
    let cases: Vec<(&str, Change)> = vec![
        ("format", Box::new(|r| r.format = "x".into())),
        (
            "os_version",
            Box::new(|r| {
                r.device.host.os_version = Some("10.0.22631 (Ana's PC)".into());
            }),
        ),
        (
            "user.product",
            Box::new(|r| r.device.user.product = Some("DualSense".into())),
        ),
        ("no views", Box::new(|r| r.device.views.clear())),
        (
            "model",
            Box::new(|r| r.configuration.confirmed.model = "DualSense".into()),
        ),
        (
            "mode",
            Box::new(|r| r.configuration.confirmed.mode = "PS 5".into()),
        ),
        (
            "transport",
            Box::new(|r| {
                r.configuration.detected.as_mut().unwrap().transport = Transport::Unknown;
            }),
        ),
        ("plan", Box::new(|r| r.plan.id = "Quick plan".into())),
        ("plan version", Box::new(|r| r.plan.version = 0)),
        (
            "step twice",
            Box::new(|r| r.results[1].step = "controls.south".into()),
        ),
        (
            "step id",
            Box::new(|r| r.results[0].step = "Controls south".into()),
        ),
        (
            "check",
            Box::new(|r| r.results[0].check = "control.press".into()),
        ),
        (
            "value",
            Box::new(|r| r.results[0].observed = Some("a1b2c3d4e5f6a7".into())),
        ),
        (
            "measurement",
            Box::new(|r| {
                r.results[1].measurements.insert("noise".into(), f64::NAN);
            }),
        ),
        (
            "measurement name",
            Box::new(|r| {
                r.results[1].measurements.insert("Noise level".into(), 1.0);
            }),
        ),
        (
            "note",
            Box::new(|r| r.notes = Some("a1:b2:c3:d4:e5:f6".into())),
        ),
        (
            "frame size",
            Box::new(|r| {
                r.results[0].evidence.as_mut().unwrap().frames[0]
                    .state
                    .buttons
                    .push(0.0);
            }),
        ),
        (
            "frame time",
            Box::new(|r| {
                r.results[0].evidence.as_mut().unwrap().frames[2].t = Millis(0);
            }),
        ),
    ];
    for (name, change) in cases {
        let mut report = example();
        change(&mut report);
        assert!(report.validate().is_err(), "{name}");
        assert!(
            matches!(report.to_json(), Err(TestReportError::Invalid(_))),
            "{name}"
        );
    }
}

#[test]
fn notes_keep_their_lines() {
    let mut report = example();
    report.notes = Some("Left stick drifts.\nRight one is fine.".into());
    assert!(report.scrub().is_empty());
    report.validate().unwrap();
}

#[test]
fn review_flags_unknown_device_names() {
    let catalog = Catalog::embedded();
    let review = example().review(catalog);
    assert!(
        review.iter().all(|item| item.concern.is_none()),
        "{review:#?}"
    );
    assert!(
        review
            .iter()
            .any(|item| item.field == "device.views[0].identity.web.id")
    );

    let mut renamed = example();
    for view in &mut renamed.device.views {
        view.identity.product_name = Some("Ana's controller".into());
    }
    let flagged: Vec<_> = renamed
        .review(catalog)
        .into_iter()
        .filter(|item| item.concern == Some(Concern::UnknownDeviceName))
        .map(|item| item.field)
        .collect();
    assert!(flagged.contains(&"device.views[1].identity.product_name".to_owned()));
}

#[test]
fn reports_round_trip() {
    let validator = validator();
    let mut desktop = desktop_report();
    desktop.user.product = None;
    desktop.host.os_version = Some("6.8.0".into());
    let mut configuration = example().configuration;
    configuration.start = SessionStart::List;
    configuration.detected = None;
    let report = TestReport::new(
        desktop,
        configuration,
        PlanRef {
            id: "full".into(),
            version: 1,
        },
    );
    for report in [example(), report] {
        for json in [report.to_json().unwrap(), report.to_json_pretty().unwrap()] {
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(validator.iter_errors(&value).count(), 0);
            assert_eq!(TestReport::from_json(&json).unwrap(), report);
        }
    }
}

#[test]
fn schema_rejects_what_the_types_reject() {
    let valid = example().to_json_pretty().unwrap();
    for (from, to) in [
        ("\"status\": \"pass\"", "\"status\": \"passed\""),
        ("\"start\": \"connecting\"", "\"start\": \"auto\""),
        ("\"unit\": \"DS5-01\"", "\"unit\": \"Ana's DualSense\""),
        ("\"answer\": \"yes\"", "\"answer\": \"maybe\""),
        ("\"duration_ms\": 812", "\"duration_ms\": -1"),
        (
            "\"duration_ms\": 812",
            "\"duration_ms\": 812, \"time\": \"10:42\"",
        ),
    ] {
        assert!(valid.contains(from), "{from}");
        let invalid = valid.replacen(from, to, 1);
        assert!(!schema_errors(&invalid).is_empty(), "{to}");
        assert!(TestReport::from_json(&invalid).is_err(), "{to}");
    }
}

/// A Full session fits the size limit: 45 steps of 1.2 seconds, either a
/// decoded DualSense over USB at 250 input reports per second (HID APIs
/// record reports only), or a Gamepad API pad sampled at 60 frames per
/// second with moving axes.
#[test]
fn a_full_session_fits() {
    for hid in [true, false] {
        let mut report = example();
        let template = report.results[0].clone();
        report.results = (0..45)
            .map(|index| {
                let mut result = template.clone();
                result.step = format!("step.{index}");
                let device = &report.device;
                let evidence = result.evidence.as_mut().unwrap();
                if hid {
                    evidence.frames.clear();
                    evidence.reports = (0..300)
                        .map(|n| TimedReport {
                            t: Millis::from_ms(f64::from(n) * 4.0),
                            report_id: 0x01,
                            data: HexBytes((0..63).map(|b| (b * n + index) as u8).collect()),
                        })
                        .collect();
                } else {
                    evidence.frames = (0..72)
                        .map(|n| {
                            let mut frame =
                                frame(device, f64::from(n) * 16.667, Some(n as usize % 17));
                            for (axis, value) in frame.state.axes.iter_mut().enumerate() {
                                *value = ((n as usize * 7 + axis * 13) % 200) as f32 / 100.0 - 1.0;
                            }
                            frame
                        })
                        .collect();
                }
                result
            })
            .collect();
        let size = report
            .to_json()
            .unwrap_or_else(|error| panic!("{error}"))
            .len();
        println!("full session, hid: {hid}: {size} bytes");
        assert!(size <= MAX_TEST_REPORT_BYTES, "{size} bytes (hid: {hid})");
    }
}
