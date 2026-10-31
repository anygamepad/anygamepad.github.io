//! Test reports (roadmap task H1, ADR 0008): what a test mode session
//! found about one controller, in one configuration and one environment.
//!
//! A [`TestReport`] holds the device report of the device under test
//! ([`Report`]: the day, the app, the host and how the API sees the
//! device), the configuration the person confirmed and what automatic
//! detection proposed ([`Configuration`]), the plan that ran, and one
//! [`StepResult`] per step, with the input recorded during the step
//! ([`Evidence`]).
//!
//! # Anonymous by construction
//!
//! There is one form of test report, and it never identifies the person,
//! their machine or a pad beyond the unit code they chose:
//!
//! - everything the device report rules out ([`privacy`](crate::privacy)):
//!   serial numbers, Bluetooth addresses, device paths, personal data, and
//!   input reports that can carry identifiers, in the evidence too;
//! - the day, never the time ([`Date`](crate::Date)); times in the evidence
//!   count from the step's start;
//! - units by their [`Code`], never free text; `user.product` stays empty
//!   (the configuration says what the pad is);
//! - the OS version cut to its numbers ([`anonymous_os_version`]): a Linux
//!   kernel release can carry any text after them;
//! - notes are the person's choice, and checked like every text.
//!
//! [`TestReport::scrub`] makes a report so, [`TestReport::validate`] checks
//! it, and [`TestReport::review`] lists every text it holds for the person
//! to read before saving, flagging device names the catalog does not know
//! for the model (a person can rename a Bluetooth pad).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anygamepad_db::Catalog;
use anygamepad_db::lab::Code;
use anygamepad_model::identity::{GamepadApiId, Os};
use anygamepad_model::raw::RawState;
use anygamepad_model::{ControllerMode, Family, Transport};
use serde::{Deserialize, Serialize};

use crate::privacy::{Finding, Redaction, is_identifying_input_report, scrub_text, text_findings};
use crate::{HexBytes, Report};

/// The value of every test report's `format` field.
pub const TEST_REPORT_FORMAT: &str = "anygamepad-test-report";

/// The test report format version this library reads and writes.
pub const TEST_REPORT_SCHEMA_VERSION: u32 = 1;

/// Steps per report.
pub const MAX_STEPS: usize = 256;

/// Measurements per step.
pub const MAX_MEASUREMENTS: usize = 32;

/// Raw state frames per step.
pub const MAX_FRAMES: usize = 600;

/// Input reports per step.
pub const MAX_INPUT_REPORTS: usize = 600;

/// Bytes of one input report in the evidence.
pub const MAX_INPUT_REPORT_BYTES: usize = 1024;

/// Characters of an expected or observed value.
pub const MAX_VALUE_CHARS: usize = 64;

/// Characters of a note.
pub const MAX_NOTE_CHARS: usize = 1000;

/// Characters of an OS version.
pub const MAX_OS_VERSION_CHARS: usize = 32;

/// Characters of a model, mode, plan, step or check ID.
pub const MAX_ID_CHARS: usize = 64;

/// Bytes of a test report as compact JSON. Test reports are files, never
/// issue bodies; this keeps the ones committed to `data/test-reports/`
/// small.
pub const MAX_TEST_REPORT_BYTES: usize = 4 * 1024 * 1024;

/// What a test mode session found about one controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TestReport {
    /// Always [`TEST_REPORT_FORMAT`].
    pub format: String,
    /// Format version: [`TEST_REPORT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The device report of the device under test: the day, the app, the
    /// host, and how the API the session used sees the device (its first
    /// view, which the evidence follows).
    pub device: Report,
    /// What the person confirmed, and what detection proposed.
    pub configuration: Configuration,
    /// The test plan that ran.
    pub plan: PlanRef,
    /// One result per step, in the order they ran.
    pub results: Vec<StepResult>,
    /// The person's note on the session, when they keep one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// What the session tested, as the person confirmed it, and as automatic
/// detection proposed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// How the session started.
    pub start: SessionStart,
    /// What the person confirmed or picked: the ground truth.
    pub confirmed: Selection,
    /// What automatic detection proposed, when it ran (always when the
    /// session started by connecting; when it started from the list, what
    /// the connected device looked like).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected: Option<Selection>,
}

impl Configuration {
    /// Whether detection proposed something other than what was confirmed:
    /// a finding (a clone, a firmware change, a catalog to correct, or a
    /// detection to improve).
    pub fn detection_differs(&self) -> bool {
        self.detected
            .as_ref()
            .is_some_and(|detected| detected != &self.confirmed)
    }
}

/// How a test session started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SessionStart {
    /// The person picked the controller from the list, then connected it.
    List,
    /// The person connected a controller without picking it.
    Connecting,
}

/// A controller in one mode over one transport.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// The unit of the lab inventory, by its code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Code>,
    /// The catalog model ID (`sony/dualsense`).
    pub model: String,
    /// The ID of the model's mode (`ps5`).
    pub mode: String,
    /// What that mode imitates, so the report reads without the catalog.
    pub kind: ControllerMode,
    /// The transport.
    pub transport: Transport,
}

/// The test plan that ran.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlanRef {
    /// Its ID (`quick`, `full`).
    pub id: String,
    /// Its version.
    pub version: u32,
}

/// What one step found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StepResult {
    /// The step's ID in the plan: lowercase words joined by dots
    /// (`controls.south`, `sticks.left.circle`).
    pub step: String,
    /// The kind of check (`control`, `stick_circle`, `rumble`).
    pub check: String,
    /// The outcome.
    pub status: StepStatus,
    /// What was expected, in canonical names (`south`, `left_stick_y+`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// What was observed, in canonical names or the profile format's raw
    /// inputs (`b2`, `a1-`, `h0.up`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    /// The person's answer, for checks only a person can make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// Measurements by name (`noise`, `range_up`, `reports_per_second`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub measurements: BTreeMap<String, f64>,
    /// How long the step took, in milliseconds.
    pub duration_ms: u32,
    /// The person's note on the step, when they keep one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The input recorded during the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
}

/// The outcome of a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// The check passed.
    Pass,
    /// The check failed.
    Fail,
    /// The person skipped it.
    Skipped,
    /// It does not apply: the pad lacks the control ("Not on this pad"), or
    /// the API cannot read it.
    NotApplicable,
    /// The app could not run it (the device went away, an output failed).
    Error,
}

/// A person's answer to a question only they can answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// Yes.
    Yes,
    /// No.
    No,
    /// Not sure.
    NotSure,
}

/// The input recorded during a step, for the device report's first view.
///
/// When the API reads HID (WebHID, hidraw), the input reports alone are
/// recorded: the raw states follow from them by decoding, and replaying
/// them tests the decoder too. Otherwise the raw states are, as the API
/// gave them. A step records from its prompt to its end, at most
/// [`MAX_FRAMES`] frames or [`MAX_INPUT_REPORTS`] reports.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Raw states, sized for the view's layout, in time order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
    /// Input reports, in time order, when the API reads HID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reports: Vec<TimedReport>,
}

/// A raw state at a moment of a step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Frame {
    /// Milliseconds since the step started, rounded to microseconds.
    pub t: Millis,
    /// The raw state.
    pub state: RawState,
}

/// An input report at a moment of a step.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TimedReport {
    /// Milliseconds since the step started, rounded to microseconds.
    pub t: Millis,
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Report data, without the report ID.
    pub data: HexBytes,
}

/// Milliseconds, with three decimals: kept as microseconds so reports can
/// be compared and hashed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Millis(pub u32);

impl Millis {
    /// The time closest to `ms` milliseconds, clamped at 0.
    pub fn from_ms(ms: f64) -> Self {
        Self((ms.max(0.0) * 1000.0).round().min(f64::from(u32::MAX)) as u32)
    }

    /// The time in milliseconds.
    pub fn as_ms(self) -> f64 {
        f64::from(self.0) / 1000.0
    }
}

impl Serialize for Millis {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.as_ms())
    }
}

impl<'de> Deserialize<'de> for Millis {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let ms = f64::deserialize(deserializer)?;
        if !ms.is_finite() || ms < 0.0 {
            return Err(serde::de::Error::custom("a time is a non-negative number"));
        }
        Ok(Self::from_ms(ms))
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Millis {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Millis".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Milliseconds, with at most three decimals.",
            "type": "number",
            "minimum": 0,
        })
    }
}

/// How many steps ended each way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct Summary {
    /// Steps that passed.
    pub pass: usize,
    /// Steps that failed.
    pub fail: usize,
    /// Steps the person skipped.
    pub skipped: usize,
    /// Steps that did not apply.
    pub not_applicable: usize,
    /// Steps the app could not run.
    pub error: usize,
}

/// One text of a report, for the review before saving.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewItem {
    /// The field, as a path into the report
    /// (`device.views[0].identity.product_name`).
    pub field: String,
    /// The text.
    pub text: String,
    /// Why the person should look at it, if anything stands out.
    pub concern: Option<Concern>,
}

/// Why a text of a report deserves a look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Concern {
    /// A device name the catalog does not know for the model: it may be a
    /// clone's own name (worth keeping), or a name the person gave the pad
    /// (to replace with the catalog's).
    UnknownDeviceName,
}

/// Why a test report is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidTestReport {
    message: String,
}

impl InvalidTestReport {
    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidTestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid test report: {}", self.message)
    }
}

impl std::error::Error for InvalidTestReport {}

/// The OS version a test report keeps: the version numbers of `version`.
///
/// On Linux, the kernel release up to its numbers (`6.8.0-45-generic` →
/// `6.8.0`), since what follows can be any text. Elsewhere, the version as
/// long as it starts with a digit and holds only letters, digits, dots and
/// spaces (`14.6.1`, `11 23H2`), cut at the first other character and at
/// [`MAX_OS_VERSION_CHARS`]. `None` when nothing is left.
pub fn anonymous_os_version(os: Os, version: &str) -> Option<String> {
    let version = version.trim();
    let kept: String = if os == Os::Linux {
        version
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect()
    } else {
        version
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == ' ')
            .take(MAX_OS_VERSION_CHARS)
            .collect()
    };
    let kept = kept.trim_end_matches(['.', ' ']).to_owned();
    kept.starts_with(|c: char| c.is_ascii_digit())
        .then_some(kept)
}

/// Whether `id` is 1 to [`MAX_ID_CHARS`] characters of lowercase ASCII
/// letters, digits and `extra` characters, starting with a letter or a
/// digit.
fn is_id(id: &str, extra: &[char]) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra.contains(&c))
}

fn check_text(path: &str, text: &str, max_chars: usize, lines: bool) -> Result<(), String> {
    let text = if lines {
        text.replace('\n', " ")
    } else {
        text.to_owned()
    };
    match text_findings(&text, max_chars).first() {
        Some(finding) => Err(format!("{path} has {finding}")),
        None => Ok(()),
    }
}

fn check_selection(path: &str, selection: &Selection) -> Result<(), String> {
    let mut parts = selection.model.split('/');
    let model_ok = matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(vendor), Some(model), None) if is_id(vendor, &['-']) && is_id(model, &['-'])
    );
    if !model_ok {
        return Err(format!("{path}.model is not a catalog model ID"));
    }
    if !is_id(&selection.mode, &['-']) {
        return Err(format!("{path}.mode is not a mode ID"));
    }
    if selection.transport == Transport::Unknown {
        return Err(format!("{path}.transport is unknown"));
    }
    Ok(())
}

impl TestReport {
    /// A report of a session on `device`, with no results yet.
    pub fn new(device: Report, configuration: Configuration, plan: PlanRef) -> Self {
        Self {
            format: TEST_REPORT_FORMAT.into(),
            schema_version: TEST_REPORT_SCHEMA_VERSION,
            device,
            configuration,
            plan,
            results: Vec::new(),
            notes: None,
        }
    }

    /// How many steps ended each way.
    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for result in &self.results {
            *match result.status {
                StepStatus::Pass => &mut summary.pass,
                StepStatus::Fail => &mut summary.fail,
                StepStatus::Skipped => &mut summary.skipped,
                StepStatus::NotApplicable => &mut summary.not_applicable,
                StepStatus::Error => &mut summary.error,
            } += 1;
        }
        summary
    }

    /// Makes the report anonymous (see the [module documentation](self)):
    /// scrubs the device report, cuts the OS version, empties
    /// `user.product`, scrubs notes and values, and drops input reports
    /// that can carry identifiers from the evidence. Returns what it did,
    /// with paths into the test report.
    ///
    /// Run it on every report a client builds, before
    /// [`validate`](Self::validate).
    pub fn scrub(&mut self) -> Vec<Redaction> {
        let mut redactions: Vec<Redaction> = self
            .device
            .scrub()
            .into_iter()
            .map(|redaction| Redaction {
                field: format!("device.{}", redaction.field),
                finding: redaction.finding,
            })
            .collect();
        let mut record = |field: String, findings: Vec<Finding>| {
            redactions.extend(findings.into_iter().map(|finding| Redaction {
                field: field.clone(),
                finding,
            }));
        };
        let host = &mut self.device.host;
        if let Some(version) = host.os_version.take() {
            host.os_version = anonymous_os_version(host.os, &version);
            if host.os_version.as_deref() != Some(version.as_str()) {
                record(
                    "device.host.os_version".into(),
                    vec![Finding::OsVersionDetail],
                );
            }
        }
        if self.device.user.product.take().is_some() {
            record("device.user.product".into(), vec![Finding::FreeText]);
        }
        scrub_note(&mut self.notes, "notes", &mut record);
        let vendor_id = self
            .device
            .views
            .first()
            .and_then(|view| view.identity.vendor_id);
        for (index, result) in self.results.iter_mut().enumerate() {
            let path = format!("results[{index}]");
            scrub_note(&mut result.note, &format!("{path}.note"), &mut record);
            for (name, value) in [
                ("expected", &mut result.expected),
                ("observed", &mut result.observed),
            ] {
                if let Some(text) = value {
                    record(format!("{path}.{name}"), scrub_text(text, MAX_VALUE_CHARS));
                    if text.is_empty() {
                        *value = None;
                    }
                }
            }
            if let Some(evidence) = &mut result.evidence {
                let mut position = 0;
                evidence.reports.retain(|report| {
                    let identifying = is_identifying_input_report(vendor_id, report.report_id);
                    if identifying {
                        record(
                            format!("{path}.evidence.reports[{position}]"),
                            vec![Finding::IdentifyingReport],
                        );
                    }
                    position += 1;
                    !identifying
                });
            }
        }
        redactions
    }

    /// Checks what the types cannot: the format and its version, the device
    /// report (P1's rules) with at least one view, an anonymous OS version
    /// and no `user.product`, the configuration's IDs and transports, the
    /// plan's ID, unique step IDs, values, measurements, notes, and the
    /// evidence (sized for the first view's layout, in time order, within
    /// the limits, without input reports that can carry identifiers).
    pub fn validate(&self) -> Result<(), InvalidTestReport> {
        self.check()
            .map_err(|message| InvalidTestReport { message })
    }

    fn check(&self) -> Result<(), String> {
        if self.format != TEST_REPORT_FORMAT {
            return Err(format!("format is not `{TEST_REPORT_FORMAT}`"));
        }
        if self.schema_version != TEST_REPORT_SCHEMA_VERSION {
            return Err(format!(
                "schema version {} is not {TEST_REPORT_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        self.device
            .validate()
            .map_err(|error| format!("device: {}", error.message()))?;
        let Some(view) = self.device.views.first() else {
            return Err("device: no views".into());
        };
        let host = &self.device.host;
        if let Some(version) = &host.os_version {
            if anonymous_os_version(host.os, version).as_deref() != Some(version.as_str()) {
                return Err("device.host.os_version is more than version numbers".into());
            }
        }
        if self.device.user.product.is_some() {
            return Err(
                "device.user.product is set; the configuration says what the pad is".into(),
            );
        }
        check_selection("configuration.confirmed", &self.configuration.confirmed)?;
        if let Some(detected) = &self.configuration.detected {
            check_selection("configuration.detected", detected)?;
        }
        if !is_id(&self.plan.id, &['-', '_']) {
            return Err("plan.id is not an ID".into());
        }
        if self.plan.version == 0 {
            return Err("plan.version is 0".into());
        }
        if let Some(notes) = &self.notes {
            check_text("notes", notes, MAX_NOTE_CHARS, true)?;
        }
        if self.results.len() > MAX_STEPS {
            return Err(format!("{} steps; at most {MAX_STEPS}", self.results.len()));
        }
        let layout = &view.layout;
        let mut steps = BTreeSet::new();
        for (index, result) in self.results.iter().enumerate() {
            let path = format!("results[{index}]");
            if !is_id(&result.step, &['-', '_', '.']) {
                return Err(format!("{path}.step is not a step ID"));
            }
            if !steps.insert(result.step.as_str()) {
                return Err(format!("{path}.step `{}` is listed twice", result.step));
            }
            if !is_id(&result.check, &['_']) {
                return Err(format!("{path}.check is not a check name"));
            }
            for (name, value) in [
                ("expected", &result.expected),
                ("observed", &result.observed),
            ] {
                if let Some(value) = value {
                    check_text(&format!("{path}.{name}"), value, MAX_VALUE_CHARS, false)?;
                    if !value.is_ascii() {
                        return Err(format!("{path}.{name} is not ASCII"));
                    }
                }
            }
            if result.measurements.len() > MAX_MEASUREMENTS {
                return Err(format!("{path}: more than {MAX_MEASUREMENTS} measurements"));
            }
            for (name, value) in &result.measurements {
                if !is_id(name, &['_']) {
                    return Err(format!("{path}.measurements: `{name}` is not a name"));
                }
                if !value.is_finite() {
                    return Err(format!("{path}.measurements.{name} is not a number"));
                }
            }
            if let Some(note) = &result.note {
                check_text(&format!("{path}.note"), note, MAX_NOTE_CHARS, true)?;
            }
            if let Some(evidence) = &result.evidence {
                check_evidence(
                    &format!("{path}.evidence"),
                    evidence,
                    layout,
                    view.identity.vendor_id,
                )?;
            }
        }
        Ok(())
    }

    /// Every text of the report, for the person to read before saving,
    /// with what deserves a look: device names that `catalog` does not
    /// know for the confirmed model, nor the library's family rules.
    pub fn review(&self, catalog: &Catalog) -> Vec<ReviewItem> {
        let model = catalog.get(&self.configuration.confirmed.model);
        let known_name = |name: &str| {
            model.is_some_and(|model| {
                let named = model
                    .modes
                    .iter()
                    .flat_map(|mode| &mode.connections)
                    .flat_map(|connection| &connection.identities)
                    .any(|identity| identity.name.as_deref() == Some(name));
                named || (model.family.is_some() && Family::from_name(name) == model.family)
            })
        };
        let mut items = Vec::new();
        let mut push = |field: String, text: &str, concern: Option<Concern>| {
            items.push(ReviewItem {
                field,
                text: text.to_owned(),
                concern,
            });
        };
        if let Some(version) = &self.device.host.os_version {
            push("device.host.os_version".into(), version, None);
        }
        for (index, view) in self.device.views.iter().enumerate() {
            let path = format!("device.views[{index}]");
            let identity = &view.identity;
            if let Some(name) = &identity.vendor_name {
                push(format!("{path}.identity.vendor_name"), name, None);
            }
            if let Some(name) = &identity.product_name {
                let concern = (!known_name(name)).then_some(Concern::UnknownDeviceName);
                push(format!("{path}.identity.product_name"), name, concern);
            }
            if let Some(driver) = &identity.driver {
                push(format!("{path}.identity.driver"), driver, None);
            }
            if let Some(web) = &identity.web {
                let name = GamepadApiId::parse(&web.id).name;
                let concern = name
                    .filter(|name| !known_name(name))
                    .map(|_| Concern::UnknownDeviceName);
                push(format!("{path}.identity.web.id"), &web.id, concern);
            }
            if let Some(profile) = &view.profile {
                push(format!("{path}.profile"), profile, None);
            }
        }
        if let Some(notes) = &self.notes {
            push("notes".into(), notes, None);
        }
        for (index, result) in self.results.iter().enumerate() {
            if let Some(note) = &result.note {
                push(format!("results[{index}].note"), note, None);
            }
        }
        items
    }

    /// The report's file name:
    /// `<unit or model>-<mode>-<transport>-<os>-<app>-<api>-<day>-<n>.json`,
    /// lowercase.
    pub fn file_name(&self, n: u32) -> String {
        let confirmed = &self.configuration.confirmed;
        let subject = match &confirmed.unit {
            Some(unit) => unit.as_str().to_ascii_lowercase(),
            None => confirmed.model.replace('/', "-"),
        };
        let api = self
            .device
            .views
            .first()
            .map_or("unknown", |view| view.identity.backend.as_str());
        let app = match self.device.source.app {
            crate::App::Web => "web",
            crate::App::Desktop => "desktop",
        };
        format!(
            "{subject}-{}-{}-{}-{app}-{api}-{}-{n}.json",
            confirmed.mode, confirmed.transport, self.device.host.os, self.device.created
        )
    }
}

fn scrub_note(
    note: &mut Option<String>,
    field: &str,
    record: &mut impl FnMut(String, Vec<Finding>),
) {
    if let Some(text) = note {
        let lines: Vec<String> = text
            .split('\n')
            .map(|line| {
                let mut line = line.to_owned();
                record(field.to_owned(), scrub_text(&mut line, MAX_NOTE_CHARS));
                line
            })
            .collect();
        let mut joined = lines.join("\n").trim().to_owned();
        if joined.chars().count() > MAX_NOTE_CHARS {
            joined = joined.chars().take(MAX_NOTE_CHARS).collect();
            record(field.to_owned(), vec![Finding::TooLong]);
        }
        *note = (!joined.is_empty()).then_some(joined);
    }
}

fn check_evidence(
    path: &str,
    evidence: &Evidence,
    layout: &anygamepad_model::raw::RawLayout,
    vendor_id: Option<u16>,
) -> Result<(), String> {
    if evidence.frames.len() > MAX_FRAMES {
        return Err(format!("{path}: more than {MAX_FRAMES} frames"));
    }
    let mut last = Millis(0);
    for (index, frame) in evidence.frames.iter().enumerate() {
        let at = format!("{path}.frames[{index}]");
        if frame.t < last {
            return Err(format!("{at}: times go back"));
        }
        last = frame.t;
        let state = &frame.state;
        if state.buttons.len() != layout.buttons.len()
            || state.axes.len() != layout.axes.len()
            || state.hats.len() != layout.hats.len()
        {
            return Err(format!("{at}: not sized for the first view's layout"));
        }
        let finite = state
            .buttons
            .iter()
            .chain(&state.axes)
            .all(|value| value.is_finite());
        if !finite {
            return Err(format!("{at}: a value is not a number"));
        }
    }
    if evidence.reports.len() > MAX_INPUT_REPORTS {
        return Err(format!(
            "{path}: more than {MAX_INPUT_REPORTS} input reports"
        ));
    }
    let mut last = Millis(0);
    for (index, report) in evidence.reports.iter().enumerate() {
        let at = format!("{path}.reports[{index}]");
        if report.t < last {
            return Err(format!("{at}: times go back"));
        }
        last = report.t;
        if report.data.0.len() > MAX_INPUT_REPORT_BYTES {
            return Err(format!("{at}: more than {MAX_INPUT_REPORT_BYTES} bytes"));
        }
        if is_identifying_input_report(vendor_id, report.report_id) {
            return Err(format!("{at} has {}", Finding::IdentifyingReport));
        }
    }
    Ok(())
}

/// The JSON Schema of the test report format, as committed in
/// `data/test-reports/schema.json`.
///
/// The schema checks the structure; [`TestReport::validate`] checks the
/// rest.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(TestReport);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/test-report.json".into(),
    );
    schema
}

/// Error returned by [`TestReport::from_json`] and the writers.
#[cfg(feature = "json")]
#[derive(Debug)]
pub enum TestReportError {
    /// Larger than [`MAX_TEST_REPORT_BYTES`] as compact JSON, or a text
    /// four times as large.
    TooLarge(usize),
    /// Not JSON, or not the test report format.
    Json(serde_json::Error),
    /// Well-formed but invalid (see [`TestReport::validate`]).
    Invalid(InvalidTestReport),
}

#[cfg(feature = "json")]
impl fmt::Display for TestReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(len) => write!(
                f,
                "test report of {len} bytes; at most {MAX_TEST_REPORT_BYTES} as compact JSON"
            ),
            Self::Json(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "json")]
impl std::error::Error for TestReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TooLarge(_) => None,
            Self::Json(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

#[cfg(feature = "json")]
impl TestReport {
    /// Parses and [validates](Self::validate) a JSON test report, compact
    /// or indented. Unknown fields are errors.
    pub fn from_json(text: &str) -> Result<Self, TestReportError> {
        if text.len() > 4 * MAX_TEST_REPORT_BYTES {
            return Err(TestReportError::TooLarge(text.len()));
        }
        let report: Self = serde_json::from_str(text).map_err(TestReportError::Json)?;
        report.validate().map_err(TestReportError::Invalid)?;
        report.compact()?;
        Ok(report)
    }

    /// Validates the report and writes it as compact JSON; fails above
    /// [`MAX_TEST_REPORT_BYTES`].
    pub fn to_json(&self) -> Result<String, TestReportError> {
        self.validate().map_err(TestReportError::Invalid)?;
        self.compact()
    }

    /// Like [`to_json`](Self::to_json), indented, for files; ends with a
    /// newline.
    pub fn to_json_pretty(&self) -> Result<String, TestReportError> {
        self.to_json()?;
        Ok(serde_json::to_string_pretty(self).map_err(TestReportError::Json)? + "\n")
    }

    fn compact(&self) -> Result<String, TestReportError> {
        let text = serde_json::to_string(self).map_err(TestReportError::Json)?;
        if text.len() > MAX_TEST_REPORT_BYTES {
            return Err(TestReportError::TooLarge(text.len()));
        }
        Ok(text)
    }
}
