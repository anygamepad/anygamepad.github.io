//! Building, scrubbing and validating reports.

mod common;

use anygamepad_db::{AxisMapping, MatchRule};
use anygamepad_model::identity::{Backend, BrowserEngine, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource};
use anygamepad_model::{Axis, Family, OutputSet};
use anygamepad_report::privacy::{Finding, REDACTED, Redaction};
use anygamepad_report::{
    App, HexBytes, HidData, InputSample, MAX_SAMPLES, MAX_VIEWS, Report, View,
};
use common::{CHROME, desktop_report, web_report};

fn error(report: &Report) -> String {
    report.validate().unwrap_err().message().to_owned()
}

#[test]
fn fixtures_are_valid() {
    for mut report in [web_report(), desktop_report()] {
        report.validate().unwrap();
        assert_eq!(report.scrub(), []);
    }
}

#[test]
fn views_know_their_fingerprint_and_family() {
    let report = web_report();
    for view in &report.views {
        assert_eq!(view.fingerprint, view.identity.fingerprint());
        assert_eq!(view.family, Some(Family::DualSense));
    }
}

#[test]
fn format_and_version_are_checked() {
    let mut report = web_report();
    report.format = "anygamepad-hid-recording-v1".into();
    assert_eq!(
        error(&report),
        "`format` is `anygamepad-hid-recording-v1`, not `anygamepad-device-report`"
    );
    let mut report = web_report();
    report.schema_version = 2;
    assert_eq!(
        error(&report),
        "`schema_version` is 2; this library reads 1"
    );
}

#[test]
fn apps_and_hosts_agree() {
    let mut report = web_report();
    report.host.browser = None;
    assert_eq!(error(&report), "web reports need `host.browser`");

    let mut report = desktop_report();
    report.host.browser = Some(CHROME);
    assert_eq!(error(&report), "desktop reports have no `host.browser`");

    let mut report = desktop_report();
    report.source.app = App::Web;
    report.host.browser = Some(CHROME);
    assert_eq!(error(&report), "views[0]: a `native` view in a web report");

    let mut report = web_report();
    report.host.os = Os::Linux;
    assert_eq!(
        error(&report),
        "views[0]: OS `windows` differs from the host's `linux`"
    );

    let mut report = web_report();
    report.host.browser = Some(anygamepad_model::identity::Browser {
        engine: BrowserEngine::Blink,
        major_version: Some(142),
    });
    assert_eq!(
        error(&report),
        "views[0]: its browser differs from the host's"
    );
}

#[test]
fn views_are_counted() {
    let mut report = web_report();
    report.views.clear();
    assert_eq!(error(&report), "no views");

    let mut report = web_report();
    let view = report.views[0].clone();
    report.views = vec![view; MAX_VIEWS + 1];
    assert_eq!(error(&report), "9 views; at most 8");
}

#[test]
fn fingerprints_and_descriptor_hashes_are_checked() {
    let mut report = web_report();
    report.views[1].identity.product_name = Some("Other".into());
    assert!(error(&report).starts_with("views[1]: `fingerprint` is "));

    let mut report = desktop_report();
    let hid = report.views[0].hid.as_mut().unwrap();
    hid.descriptor.as_mut().unwrap().0.push(0);
    assert!(
        error(&report)
            .starts_with("views[0]: the identity's `descriptor_hash` is not the descriptor's")
    );
}

#[test]
fn personal_data_is_rejected() {
    let mut report = web_report();
    report.views[1].identity.product_name = Some("Pad A1:B2:C3:D4:E5:F6".into());
    report.views[1].fingerprint = report.views[1].identity.fingerprint();
    assert_eq!(
        error(&report),
        "views[1].identity.product_name has a hexadecimal identifier (address or serial number)"
    );

    let mut report = web_report();
    report.user.product = Some("ana@example.org's pad".into());
    assert_eq!(error(&report), "user.product has an email address");

    let mut report = web_report();
    report.user.product = Some("x".repeat(101));
    assert_eq!(error(&report), "user.product has a text that is too long");

    let mut report = web_report();
    report.user.product = Some(" ".into());
    assert_eq!(error(&report), "`user.product` is empty");

    let mut report = desktop_report();
    report.host.os_version = Some("6.8\n".into());
    assert_eq!(error(&report), "host.os_version has control characters");
}

/// A Switch Pro Controller seen over hidraw, with samples of its full
/// input report and of a subcommand reply.
fn switch_pro_report() -> Report {
    let mut report = desktop_report();
    report.views.truncate(1);
    let view = &mut report.views[0];
    view.identity.vendor_id = Some(0x057e);
    view.identity.product_id = Some(0x2009);
    view.fingerprint = view.identity.fingerprint();
    let hid = view.hid.as_mut().unwrap();
    hid.input_samples = vec![
        InputSample {
            report_id: 0x30,
            data: HexBytes(vec![0; 48]),
        },
        InputSample {
            report_id: 0x21,
            data: HexBytes(vec![0x02; 48]),
        },
    ];
    report
}

#[test]
fn identifying_input_reports_are_rejected() {
    assert_eq!(
        error(&switch_pro_report()),
        "views[0].hid.input_samples[1]: report 0x21 of this vendor can carry identifiers"
    );
}

#[test]
fn sizes_are_limited() {
    let mut report = desktop_report();
    let hid = report.views[0].hid.as_mut().unwrap();
    hid.input_samples = (0..=MAX_SAMPLES as u8)
        .map(|id| InputSample {
            report_id: id,
            data: HexBytes(vec![0]),
        })
        .collect();
    assert_eq!(error(&report), "views[0].hid: 9 input samples; at most 8");

    let mut report = desktop_report();
    report.views[0].hid.as_mut().unwrap().input_samples[0].data = HexBytes(vec![0; 1025]);
    assert_eq!(
        error(&report),
        "views[0].hid.input_samples[0]: 1025 bytes; at most 1024"
    );

    let mut report = desktop_report();
    report.views[2].layout.buttons = vec![RawSource::Index(0); 257];
    assert_eq!(error(&report), "views[2].layout: 257 buttons; at most 256");

    let mut report = desktop_report();
    report.views[2].layout.axes = vec![RawAxisInfo {
        source: RawSource::Index(0),
        min: 0.0,
        max: f32::INFINITY,
    }];
    assert_eq!(
        error(&report),
        "views[2].layout.axes[0]: range is not finite"
    );

    let mut report = web_report();
    let hid = report.views[1].hid.as_mut().unwrap();
    let mut nested = hid.collections[0].clone();
    for _ in 0..8 {
        let mut parent = nested.clone();
        parent.children = vec![nested];
        nested = parent;
    }
    hid.collections = vec![nested];
    assert_eq!(
        error(&report),
        "views[1].hid.collections: nested deeper than 8 levels"
    );

    let mut report = web_report();
    let hid = report.views[1].hid.as_mut().unwrap();
    let collection = hid.collections[0].clone();
    hid.collections = vec![collection; 40];
    assert_eq!(
        error(&report),
        "views[1].hid.collections: more than 2048 collections, reports, items and usages"
    );
}

#[test]
fn confirmed_mappings_are_checked() {
    let mut report = web_report();
    let confirmed = report.views[0].confirmed.as_mut().unwrap();
    confirmed.when = MatchRule {
        os: Some(Os::Windows),
        ..MatchRule::default()
    };
    assert_eq!(error(&report), "views[0].confirmed: `when` must be empty");

    let mut report = web_report();
    let confirmed = report.views[0].confirmed.as_mut().unwrap();
    confirmed
        .axes
        .insert(Axis::LeftTrigger, AxisMapping::default());
    assert_eq!(
        error(&report),
        "views[0].confirmed: axis `left_trigger`: set either `input` or `negative`/`positive`"
    );
}

#[test]
fn scrub_redacts_and_recomputes_fingerprints() {
    let mut report = web_report();
    let id = "Pad\u{7}a1b2c3d4e5f6 (Vendor: 054c Product: 0ce6)";
    let identity = DeviceIdentity::from_gamepad_api(id, "", CHROME, Os::Windows);
    let before = identity.fingerprint();
    report.views[0] = View::new(identity, RawLayout::default(), OutputSet::EMPTY);
    report.user.product = Some("  Ana's pad, ana@example.org ".into());
    report.host.os_version = None;

    let redactions = report.scrub();
    let field = |field: &str, finding| Redaction {
        field: field.into(),
        finding,
    };
    assert_eq!(
        redactions,
        [
            field("user.product", Finding::EmailAddress),
            field("views[0].identity.product_name", Finding::ControlCharacters),
            field("views[0].identity.product_name", Finding::HexIdentifier),
            field("views[0].identity.web.id", Finding::ControlCharacters),
            field("views[0].identity.web.id", Finding::HexIdentifier),
        ]
    );
    assert_eq!(
        report.user.product.as_deref(),
        Some("Ana's pad, [redacted]")
    );
    let view = &report.views[0];
    assert_eq!(
        view.identity.web.as_ref().unwrap().id,
        format!("Pad {REDACTED} (Vendor: 054c Product: 0ce6)")
    );
    assert_eq!(
        view.identity.product_name.as_deref(),
        Some("Pad [redacted]")
    );
    assert_ne!(view.fingerprint, before);
    assert_eq!(view.fingerprint, view.identity.fingerprint());
    report.validate().unwrap();
    // Scrubbing again finds nothing.
    assert_eq!(report.scrub(), []);
}

#[test]
fn scrub_removes_identifying_samples() {
    let mut report = switch_pro_report();
    assert_eq!(
        report.scrub(),
        [Redaction {
            field: "views[0].hid.input_samples[1]".into(),
            finding: Finding::IdentifyingReport,
        }]
    );
    let samples = &report.views[0].hid.as_ref().unwrap().input_samples;
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].report_id, 0x30);
    report.validate().unwrap();
}

#[test]
fn scrub_drops_an_empty_user_product() {
    let mut report = web_report();
    report.user.product = Some("\u{0}".into());
    report.scrub();
    assert_eq!(report.user.product, None);
    report.validate().unwrap();
}

#[test]
fn desktop_views_need_no_browser() {
    let mut report = desktop_report();
    let identity = DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux);
    let mut view = View::new(identity, RawLayout::default(), OutputSet::EMPTY);
    view.hid = Some(HidData::default());
    report.views.push(view);
    report.validate().unwrap();
}
