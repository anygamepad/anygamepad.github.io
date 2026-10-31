//! Test plans (roadmap task H3, ADR 0008): the steps a test mode session
//! runs, as data.
//!
//! A [`TestPlan`] is a list of [`PlanStep`]s; each names a [`CheckKind`]
//! and the parameters it takes. The runner (`anygamepad::testing`) expands
//! the steps for the model under test (one step per button the model has,
//! for [`CheckKind::Buttons`]) and leaves out what the model lacks.
//!
//! Plans are TOML files in `data/test-plans/`, described by a JSON Schema
//! ([`json_schema`], committed as `data/test-plans/schema.json`), and
//! embedded at build time ([`TestPlan::embedded`]). Steps are flat
//! structures so the same types read TOML, write the embedded binary copy
//! and generate the schema.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::catalog::{MAX_NAME_CHARS, MAX_NOTE_CHARS, check_text, is_slug};

/// Version of the plan format this library reads and writes.
pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// A test plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TestPlan {
    /// Format version: [`PLAN_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Lowercase words joined by hyphens; the file's name (`quick`).
    pub id: String,
    /// Raised whenever the steps change, so reports say which ran.
    pub version: u32,
    /// Its name in the apps (`Quick`).
    pub name: String,
    /// What it covers and how long it takes.
    pub description: String,
    /// The steps, in the order they run.
    pub steps: Vec<PlanStep>,
}

/// One step of a plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    /// Unique in the plan: lowercase words joined by dots (`sticks.left`).
    /// A [`CheckKind::Buttons`] step's ID prefixes the steps it expands
    /// into (`controls.south`).
    pub id: String,
    /// What the step checks.
    pub check: CheckKind,
    /// The stick or trigger, for [`CheckKind::StickRest`],
    /// [`CheckKind::StickCircle`] and [`CheckKind::TriggerPull`].
    #[serde(default)]
    pub side: Option<Side>,
    /// How long to measure, in seconds, for [`CheckKind::StickRest`],
    /// [`CheckKind::Rate`] and [`CheckKind::MotionRest`].
    #[serde(default)]
    pub seconds: Option<f32>,
    /// The threshold of the check, in `(0, 1]`: the largest offset at rest
    /// ([`CheckKind::StickRest`]), the smallest reach in every direction
    /// ([`CheckKind::StickCircle`]), the smallest full pull
    /// ([`CheckKind::TriggerPull`]).
    #[serde(default)]
    pub tolerance: Option<f32>,
}

/// What a step checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// The device is the model, mode and transport confirmed: what
    /// detection proposed, and the identities the catalog expects.
    Detection,
    /// Every button of the model, one step each: pressing it gives exactly
    /// that canonical button.
    Buttons,
    /// A stick left alone rests at its centre, within the tolerance.
    StickRest,
    /// A stick turned around its edge reaches it in every direction.
    StickCircle,
    /// An analog trigger pulled slowly goes from rest to its end, through
    /// values in between.
    TriggerPull,
    /// Input updates per second while the sticks move.
    Rate,
    /// A pad lying still: gyroscope near zero, acceleration near gravity.
    MotionRest,
    /// A pad turned to the left on a table: the gyroscope's up axis turns
    /// positive.
    MotionTurn,
    /// The touchpad sees its four corners and two fingers.
    Touchpad,
    /// The battery reports its state.
    Battery,
    /// Each motor vibrates (the person answers).
    Rumble,
    /// The light bar turns red, green and blue (the person answers).
    Lightbar,
    /// The player lights count from one to four (the person answers).
    PlayerLeds,
    /// The pad unplugged or turned off goes away, and comes back with the
    /// same identity.
    Hotplug,
}

impl CheckKind {
    /// Its name in test reports (`stick_circle`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detection => "detection",
            Self::Buttons => "button",
            Self::StickRest => "stick_rest",
            Self::StickCircle => "stick_circle",
            Self::TriggerPull => "trigger_pull",
            Self::Rate => "rate",
            Self::MotionRest => "motion_rest",
            Self::MotionTurn => "motion_turn",
            Self::Touchpad => "touchpad",
            Self::Battery => "battery",
            Self::Rumble => "rumble",
            Self::Lightbar => "lightbar",
            Self::PlayerLeds => "player_leds",
            Self::Hotplug => "hotplug",
        }
    }

    const fn takes_side(self) -> bool {
        matches!(
            self,
            Self::StickRest | Self::StickCircle | Self::TriggerPull
        )
    }

    const fn takes_seconds(self) -> bool {
        matches!(self, Self::StickRest | Self::Rate | Self::MotionRest)
    }

    const fn takes_tolerance(self) -> bool {
        matches!(
            self,
            Self::StickRest | Self::StickCircle | Self::TriggerPull
        )
    }
}

/// Left or right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The left stick or trigger.
    Left,
    /// The right stick or trigger.
    Right,
}

impl Side {
    /// `left` or `right`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Why a plan is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidPlan {
    plan: String,
    message: String,
}

impl InvalidPlan {
    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid test plan `{}`: {}", self.plan, self.message)
    }
}

impl std::error::Error for InvalidPlan {}

impl TestPlan {
    /// Checks what the types cannot: the schema version, IDs, a version from
    /// 1, names, at least one step, unique step IDs, and parameters that
    /// fit their check (a side where one is needed and none elsewhere,
    /// seconds in `(0, 30]`, tolerances in `(0, 1]`).
    pub fn validate(&self) -> Result<(), InvalidPlan> {
        self.check().map_err(|message| InvalidPlan {
            plan: self.id.clone(),
            message,
        })
    }

    fn check(&self) -> Result<(), String> {
        if self.schema_version != PLAN_SCHEMA_VERSION {
            return Err(format!(
                "schema version {} is not {PLAN_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if !is_slug(&self.id) {
            return Err("the ID is not lowercase words joined by hyphens".into());
        }
        if self.version == 0 {
            return Err("the version is 0".into());
        }
        check_text("the name", &self.name, MAX_NAME_CHARS, false)?;
        check_text("the description", &self.description, MAX_NOTE_CHARS, true)?;
        if self.steps.is_empty() {
            return Err("no steps".into());
        }
        let mut ids = BTreeSet::new();
        for step in &self.steps {
            let what = format!("step `{}`", step.id);
            if step.id.split('.').any(|word| !is_slug(word)) {
                return Err(format!("{what}: the ID is not words joined by dots"));
            }
            if !ids.insert(step.id.as_str()) {
                return Err(format!("{what}: listed twice"));
            }
            let kind = step.check;
            if kind.takes_side() != step.side.is_some() {
                return Err(format!(
                    "{what}: `side` is {} for `{}`",
                    if kind.takes_side() {
                        "needed"
                    } else {
                        "not taken"
                    },
                    kind.as_str()
                ));
            }
            if step.seconds.is_some() && !kind.takes_seconds() {
                return Err(format!("{what}: `seconds` is not taken"));
            }
            if step.seconds.is_some_and(|s| !(s > 0.0 && s <= 30.0)) {
                return Err(format!("{what}: `seconds` is not in (0, 30]"));
            }
            if step.tolerance.is_some() && !kind.takes_tolerance() {
                return Err(format!("{what}: `tolerance` is not taken"));
            }
            if step.tolerance.is_some_and(|t| !(t > 0.0 && t <= 1.0)) {
                return Err(format!("{what}: `tolerance` is not in (0, 1]"));
            }
        }
        Ok(())
    }
}

/// Encodes plans for embedding (postcard).
#[allow(dead_code, reason = "used by the build script")]
pub(crate) fn encode_plans(plans: &[TestPlan]) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(plans)
}

/// Decodes plans encoded by [`encode_plans`].
pub(crate) fn decode_plans(bytes: &[u8]) -> Result<Vec<TestPlan>, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// The JSON Schema of a test plan file, as committed in
/// `data/test-plans/schema.json`.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(TestPlan);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/test-plan.json".into(),
    );
    schema
}

/// A plan file that could not be read.
#[cfg(feature = "toml")]
#[derive(Debug)]
pub enum PlanError {
    /// Not TOML, or not the plan format.
    Toml(toml::de::Error),
    /// Well-formed but invalid (see [`TestPlan::validate`]).
    Invalid(InvalidPlan),
}

#[cfg(feature = "toml")]
impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "not a test plan: {error}"),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "toml")]
impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

#[cfg(feature = "toml")]
impl TestPlan {
    /// Parses and [validates](Self::validate) a TOML plan.
    pub fn from_toml(text: &str) -> Result<Self, PlanError> {
        let plan: Self = toml::from_str(text).map_err(PlanError::Toml)?;
        plan.validate().map_err(PlanError::Invalid)?;
        Ok(plan)
    }

    /// Writes the plan as TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> TestPlan {
        TestPlan {
            schema_version: 1,
            id: "mini".into(),
            version: 1,
            name: "Mini".into(),
            description: "Two steps.".into(),
            steps: vec![
                PlanStep {
                    id: "controls".into(),
                    check: CheckKind::Buttons,
                    side: None,
                    seconds: None,
                    tolerance: None,
                },
                PlanStep {
                    id: "sticks.left.circle".into(),
                    check: CheckKind::StickCircle,
                    side: Some(Side::Left),
                    seconds: None,
                    tolerance: Some(0.9),
                },
            ],
        }
    }

    fn message(plan: &TestPlan) -> String {
        plan.validate().unwrap_err().message().to_owned()
    }

    #[test]
    fn valid() {
        plan().validate().unwrap();
    }

    #[test]
    fn parameters_fit_their_check() {
        let mut p = plan();
        p.steps[1].side = None;
        assert!(message(&p).contains("needed"));
        let mut p = plan();
        p.steps[0].side = Some(Side::Left);
        assert!(message(&p).contains("not taken"));
        let mut p = plan();
        p.steps[1].seconds = Some(1.0);
        assert!(message(&p).contains("seconds"));
        let mut p = plan();
        p.steps[1].tolerance = Some(1.5);
        assert!(message(&p).contains("tolerance"));
    }

    #[test]
    fn ids() {
        let mut p = plan();
        p.steps[1].id = "controls".into();
        assert!(message(&p).contains("twice"));
        let mut p = plan();
        p.steps[1].id = "Sticks left".into();
        assert!(message(&p).contains("dots"));
        let mut p = plan();
        p.version = 0;
        assert!(message(&p).contains("version"));
    }

    #[test]
    fn embedded_plans() {
        let quick = TestPlan::get("quick").unwrap();
        assert!(
            quick
                .steps
                .iter()
                .any(|step| step.check == CheckKind::Buttons)
        );
        assert!(TestPlan::get("full").is_some());
        for plan in TestPlan::embedded() {
            plan.validate().unwrap();
        }
    }
}
