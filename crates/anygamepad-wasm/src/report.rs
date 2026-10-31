use anygamepad::model::identity::Os;
use anygamepad_report::privacy::Finding;
use anygamepad_report::{App, ControllerMode, Date, Host, Report, Source, UserStatement, Version};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::gamepads::to_js;

/// Something that was changed in the report before sharing.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct Redaction {
    /// The field path in the report (e.g. `views[0].identity.product_name`).
    pub field: String,
    /// The kind of finding.
    pub finding: String,
    /// Human-readable explanation.
    pub message: String,
}

impl From<anygamepad_report::privacy::Redaction> for Redaction {
    fn from(r: anygamepad_report::privacy::Redaction) -> Self {
        let finding = match r.finding {
            Finding::HexIdentifier => "hex_identifier",
            Finding::EmailAddress => "email_address",
            Finding::ControlCharacters => "control_characters",
            Finding::TooLong => "too_long",
            Finding::IdentifyingReport => "identifying_report",
            Finding::OsVersionDetail => "os_version_detail",
            Finding::FreeText => "free_text",
        }
        .to_string();
        Self {
            field: r.field,
            finding,
            message: r.finding.to_string(),
        }
    }
}

/// The result of scrubbing a report.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct ScrubResult {
    /// The scrubbed report.
    pub report: Report,
    /// Redactions applied.
    pub redactions: Vec<Redaction>,
}

/// Parses a controller mode from a string.
pub(crate) fn parse_controller_mode(mode: Option<&str>) -> Option<ControllerMode> {
    match mode?.trim() {
        "xinput" => Some(ControllerMode::Xinput),
        "dinput" => Some(ControllerMode::Dinput),
        "switch" => Some(ControllerMode::Switch),
        "ps3" => Some(ControllerMode::Ps3),
        "ps4" => Some(ControllerMode::Ps4),
        "ps5" => Some(ControllerMode::Ps5),
        "xbox" => Some(ControllerMode::Xbox),
        "android" => Some(ControllerMode::Android),
        "apple" => Some(ControllerMode::Apple),
        "keyboard_mouse" => Some(ControllerMode::KeyboardMouse),
        "other" => Some(ControllerMode::Other),
        _ => None,
    }
}

pub(crate) fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// Builds a base report for the web app.
pub(crate) fn base_web_report(
    os: Os,
    browser: Option<anygamepad::model::identity::Browser>,
    now_ms: Option<f64>,
    user_product: Option<String>,
    controller_mode: Option<String>,
) -> Result<Report, JsError> {
    let app_version: Version = "0.0.3".parse().map_err(js_err)?;
    let library_version: Version = anygamepad::VERSION.parse().map_err(js_err)?;
    let source = Source {
        app: App::Web,
        app_version,
        library_version,
    };
    let host = Host {
        os,
        os_version: None,
        browser,
    };
    let now_secs = ((now_ms.unwrap_or_else(js_sys::Date::now)) / 1000.0) as i64;
    let created =
        Date::from_unix_seconds(now_secs).unwrap_or_else(|| Date::new(2026, 10, 25).unwrap());

    let user = UserStatement {
        product: user_product.filter(|s| !s.trim().is_empty()),
        mode: parse_controller_mode(controller_mode.as_deref()),
        ..UserStatement::default()
    };

    let mut report = Report::new(source, host, created);
    report.user = user;
    Ok(report)
}

/// Builds a scrubbed and validated report and returns the Report and redactions.
pub(crate) fn finish_report(mut report: Report) -> Result<JsValue, JsError> {
    let redactions = report.scrub();
    report.validate().map_err(js_err)?;
    let result = ScrubResult {
        report,
        redactions: redactions.into_iter().map(Into::into).collect(),
    };
    to_js(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anygamepad::model::OutputSet;
    use anygamepad::model::identity::{Browser, BrowserEngine, DeviceIdentity, Os};
    use anygamepad::model::raw::RawLayout;
    use anygamepad_report::View;

    #[test]
    fn test_report_building_scrubbing_and_validation() {
        let browser = Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(141),
        };
        let mut report = base_web_report(
            Os::Windows,
            Some(browser),
            Some(1728950400000.0),                     // 2024-10-15
            Some("My Controller a1b2c3d4e5f6".into()), // Has identifier
            Some("xinput".into()),
        )
        .unwrap();

        let identity = DeviceIdentity::from_gamepad_api(
            "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
            "standard",
            browser,
            Os::Windows,
        );
        let view = View::new(identity, RawLayout::default(), OutputSet::EMPTY);
        report.views.push(view);

        // Validation fails before scrubbing due to hex identifier in user.product
        assert!(report.validate().is_err());

        // Scrubbing removes the hex identifier
        let redactions = report.scrub();
        assert_eq!(redactions.len(), 1);
        assert_eq!(redactions[0].field, "user.product");
        assert_eq!(
            report.user.product.as_deref(),
            Some("My Controller [redacted]")
        );

        // Now validation passes
        report.validate().unwrap();

        // Also test Report::from_json with scrubbed JSON
        let scrubbed_json = report.to_json().unwrap();
        let parsed = Report::from_json(&scrubbed_json).unwrap();
        parsed.validate().unwrap();
    }
}
