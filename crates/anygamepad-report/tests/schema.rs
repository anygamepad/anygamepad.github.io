//! The committed JSON Schema and example report are up to date, reports
//! survive a round trip through JSON, the schema rejects what the types
//! reject, and realistic reports fit the size limit.

mod common;

use std::path::{Path, PathBuf};

use anygamepad_model::identity::ProviderKind;
use anygamepad_report::{
    MAX_REPORT_BYTES, MAX_SAMPLE_BYTES, MAX_SAMPLES, MAX_VIEWS, Report, ReportError, json_schema,
};
use common::{desktop_report, full_confirmed_mapping, web_report};

fn reports_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/reports")
}

fn validator() -> jsonschema::Validator {
    let schema = serde_json::to_value(json_schema()).unwrap();
    jsonschema::validator_for(&schema).expect("the schema is a valid JSON Schema")
}

fn schema_errors(validator: &jsonschema::Validator, json: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    validator
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

/// Compares `expected` with a committed file, or writes it when
/// `UPDATE_SCHEMA` is set.
fn check_committed(name: &str, expected: &str) {
    let path = reports_dir().join(name);
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, expected).unwrap();
        return;
    }
    // Git may check the file out with CRLF line ends (Windows).
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == expected,
        "data/reports/{name} is stale; regenerate it with \
         `UPDATE_SCHEMA=1 cargo test -p anygamepad-report --all-features --test schema`"
    );
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
    let json = web_report().to_json_pretty().unwrap();
    check_committed("example.json", &json);
    assert_eq!(schema_errors(&validator(), &json), Vec::<String>::new());
    assert_eq!(Report::from_json(&json).unwrap(), web_report());
}

#[test]
fn reports_round_trip() {
    let validator = validator();
    for report in [web_report(), desktop_report()] {
        for json in [report.to_json().unwrap(), report.to_json_pretty().unwrap()] {
            assert_eq!(schema_errors(&validator, &json), Vec::<String>::new());
            assert_eq!(Report::from_json(&json).unwrap(), report);
        }
    }
}

/// The example in `data/reports/README.md` is a valid report.
#[test]
fn readme_example_is_valid() {
    let readme = std::fs::read_to_string(reports_dir().join("README.md")).unwrap();
    let readme = readme.replace("\r\n", "\n");
    let start = readme.find("```json\n").expect("a JSON example") + "```json\n".len();
    let example = &readme[start..start + readme[start..].find("```").unwrap()];
    let report = Report::from_json(example).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.views.len(), 1);
    assert_eq!(schema_errors(&validator(), example), Vec::<String>::new());
}

/// The schema rejects what the types reject, for the parts written by hand
/// and the strict field lists.
#[test]
fn schema_rejects_invalid_reports() {
    let validator = validator();
    let valid = desktop_report().to_json_pretty().unwrap();
    assert_eq!(schema_errors(&validator, &valid), Vec::<String>::new());

    for (from, to) in [
        (
            "\"format\": \"anygamepad-device-report\"",
            "\"format\": \"anygamepad-activity-log-v1\"",
        ),
        ("\"created\": \"2026-10-14\"", "\"created\": \"14/10/2026\""),
        ("\"app\": \"desktop\"", "\"app\": \"mobile\""),
        ("\"app_version\": \"0.1.0\"", "\"app_version\": \"0.1\""),
        ("\"mode\": \"ps5\"", "\"mode\": \"turbo\""),
        ("\"descriptor\": \"05", "\"descriptor\": \"0x05"),
        ("\"descriptor\": \"05", "\"descriptor\": \"5"),
        ("\"report_id\": 49", "\"report_id\": 256"),
        ("\"product\": \"DualSense\"", "\"serial\": \"DualSense\""),
        (
            "\"os_version\": \"6.8.0-45-generic\"",
            "\"os_version\": \"6.8\", \"hostname\": \"pc\"",
        ),
    ] {
        assert!(valid.contains(from), "{from}");
        let invalid = valid.replacen(from, to, 1);
        assert!(
            !schema_errors(&validator, &invalid).is_empty(),
            "the schema accepts `{to}`"
        );
        assert!(
            Report::from_json(&invalid).is_err(),
            "the types accept `{to}`"
        );
    }
}

#[test]
fn realistic_reports_fit_an_issue() {
    // A desktop report with the DualSense's descriptor and HID structure in
    // three views, and a mapping confirmed in each of them.
    let mut report = desktop_report();
    for provider in [ProviderKind::Sdl3, ProviderKind::Gilrs] {
        let mut view = report.views[0].clone();
        view.identity.provider = provider;
        view.fingerprint = view.identity.fingerprint();
        report.views.push(view);
    }
    for view in &mut report.views {
        view.confirmed = Some(full_confirmed_mapping());
    }
    let json = report.to_json().unwrap();
    assert!(json.len() < MAX_REPORT_BYTES / 2, "{} bytes", json.len());
    // Its indented form is longer, and reads back.
    let pretty = report.to_json_pretty().unwrap();
    assert!(pretty.len() > MAX_REPORT_BYTES, "{} bytes", pretty.len());
    assert_eq!(Report::from_json(&pretty).unwrap(), report);
}

#[test]
fn too_large_reports_are_rejected() {
    let text = " ".repeat(4 * MAX_REPORT_BYTES + 1);
    assert!(matches!(
        Report::from_json(&text),
        Err(ReportError::TooLarge(len)) if len == 4 * MAX_REPORT_BYTES + 1
    ));

    // Eight views with eight samples of 1,024 bytes: 131 KB of hexadecimal.
    let mut report = desktop_report();
    let mut view = report.views[0].clone();
    let hid = view.hid.as_mut().unwrap();
    hid.input_samples.clear();
    for id in 0..MAX_SAMPLES as u8 {
        hid.sample(id, &[0xaa; MAX_SAMPLE_BYTES]);
    }
    report.views = vec![view; MAX_VIEWS];
    report.validate().unwrap();
    assert!(matches!(report.to_json(), Err(ReportError::TooLarge(_))));
    assert!(matches!(
        report.to_json_pretty(),
        Err(ReportError::TooLarge(_))
    ));
    let compact = serde_json::to_string(&report).unwrap();
    assert!(matches!(
        Report::from_json(&compact),
        Err(ReportError::TooLarge(_))
    ));
}

#[test]
fn invalid_reports_are_not_written() {
    let mut report = web_report();
    report.views.clear();
    assert!(matches!(report.to_json(), Err(ReportError::Invalid(_))));
}
