//! The controller catalog (roadmap task H2, ADR 0008): controller models as
//! sold, with their controls and the modes they can be switched to.
//!
//! A [`Model`] is what a person can buy (`8BitDo Ultimate 2C Wireless`): the
//! controls it has, its [`Mode`]s (as printed on its switch or told in its
//! manual, with how to switch to each), and for each mode the ways to
//! connect it ([`Connection`]: a transport, how to connect over it, and the
//! identities a device connected that way shows). Generic models
//! (`generic/xinput`, ...) stand for pads the catalog does not list.
//!
//! A model is not a device profile ([`Profile`](crate::Profile)): a profile
//! says how to map one identity to the canonical model, a catalog model says
//! what a product is and which identities it takes. The test modes of the
//! apps list the catalog, show the ways to connect a model, and detect which
//! model and mode a connected device is.
//!
//! Models are TOML files in `data/controllers/<vendor>/<model>.toml`,
//! described by a JSON Schema ([`json_schema`], committed as
//! `data/controllers/schema.json`), and embedded at build time
//! ([`Catalog::embedded`]).

use std::collections::BTreeSet;
use std::fmt;

use anygamepad_model::identity::{Backend, DescriptorHash, DeviceIdentity, Os};
use anygamepad_model::{
    AxisSet, ButtonSet, Capabilities, ControllerMode, FaceLayout, Family, OutputSet, SensorSet,
    TouchpadInfo, Transport,
};
use serde::{Deserialize, Serialize};

/// Version of the catalog format this library reads and writes.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// Characters of a model's vendor and name, and of a mode's name.
pub const MAX_NAME_CHARS: usize = 100;

/// Characters of an instruction or a note.
pub const MAX_NOTE_CHARS: usize = 1000;

/// A controller model as sold, with its controls and modes.
///
/// Fields left out of a TOML file take their defaults; the binary encoding
/// of the embedded catalog writes every field, so no field skips
/// serialization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Format version: [`CATALOG_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `<vendor>/<model>`, lowercase words of letters and digits joined by
    /// hyphens: the file's path under `data/controllers/` without `.toml`
    /// (`sony/dualsense`). Generic models are `generic/<type>`.
    pub id: String,
    /// The maker, as printed on the box (`Sony`, `8BitDo`).
    pub vendor: String,
    /// The product name, as printed on the box.
    pub name: String,
    /// Whether this stands for every pad of a kind the catalog does not list
    /// (`generic/xinput`), rather than a product. Generic models are
    /// detected by their family, not by identities.
    #[serde(default)]
    pub generic: bool,
    /// The family it is drawn as, if any.
    #[serde(default)]
    pub family: Option<Family>,
    /// What is printed on its face buttons, when its family does not tell.
    #[serde(default)]
    pub labels: Option<Labels>,
    /// The controls and outputs it has, in every mode unless a mode says
    /// otherwise ([`Mode::controls`]).
    pub controls: Controls,
    /// The modes it can be switched to; a pad without a mode switch has one.
    pub modes: Vec<Mode>,
    /// Anything else worth knowing (editions, firmware), never about a
    /// person or a particular unit.
    #[serde(default)]
    pub notes: Option<String>,
}

/// What is printed on the four face buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Labels {
    /// `A` south, `B` east, `X` west, `Y` north.
    Xbox,
    /// Cross south, circle east, square west, triangle north.
    Playstation,
    /// `B` south, `A` east, `Y` west, `X` north.
    Nintendo,
}

impl Labels {
    /// The labels as the canonical model names them.
    pub const fn face_layout(self) -> FaceLayout {
        match self {
            Self::Xbox => FaceLayout::XBOX,
            Self::Playstation => FaceLayout::PLAYSTATION,
            Self::Nintendo => FaceLayout::NINTENDO,
        }
    }
}

/// The controls and outputs of a model, in canonical names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Controls {
    /// Buttons, by position (back paddles and extra buttons included).
    pub buttons: ButtonSet,
    /// Analog axes: sticks, and triggers that are analog.
    #[serde(default)]
    pub axes: AxisSet,
    /// Motion sensors.
    #[serde(default)]
    pub sensors: SensorSet,
    /// Touchpads.
    #[serde(default)]
    pub touchpads: Vec<TouchpadInfo>,
    /// Outputs the hardware has.
    #[serde(default)]
    pub outputs: OutputSet,
    /// Whether it has a battery whose level it reports.
    #[serde(default)]
    pub battery: bool,
}

impl Controls {
    /// The same, as the capability set games query.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            buttons: self.buttons,
            axes: self.axes,
            sensors: self.sensors,
            touchpads: self.touchpads.clone(),
            outputs: self.outputs,
            battery: self.battery,
        }
    }
}

/// A mode a model can be switched to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Mode {
    /// Unique within the model, lowercase words of letters and digits joined
    /// by hyphens (`ps5`, `x`, `switch`).
    pub id: String,
    /// Its name as printed on the switch or told in the manual (`X`,
    /// `Switch`, `PS5`).
    pub name: String,
    /// What the mode imitates.
    pub kind: ControllerMode,
    /// How to put the pad in this mode (switch position, buttons held while
    /// turning it on).
    #[serde(default)]
    pub how_to: Option<String>,
    /// The controls in this mode, when they differ from the model's (a mode
    /// that hides the motion sensors or makes the triggers digital).
    #[serde(default)]
    pub controls: Option<Controls>,
    /// The ways to connect the pad in this mode, one per transport.
    pub connections: Vec<Connection>,
}

/// One way to connect a pad in one mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Connection {
    /// The transport.
    pub transport: Transport,
    /// How to connect over it (cable, pairing steps, dongle).
    #[serde(default)]
    pub how_to: Option<String>,
    /// The identities a device connected this way shows, any of them;
    /// empty while no one has seen it (a test report fills it in).
    #[serde(default)]
    pub identities: Vec<ExpectedIdentity>,
}

/// What a device connected in one mode over one transport shows: every
/// condition set must hold, the others are free.
///
/// The transport is the [`Connection`]'s, not a condition here, since some
/// APIs cannot tell it (the Gamepad API).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExpectedIdentity {
    /// USB/Bluetooth vendor ID.
    #[serde(default)]
    pub vendor_id: Option<u16>,
    /// Product ID; needs `vendor_id`.
    #[serde(default)]
    pub product_id: Option<u16>,
    /// Product name, exactly as the backend reports it.
    #[serde(default)]
    pub name: Option<String>,
    /// Host OS the other conditions are for.
    #[serde(default)]
    pub os: Option<Os>,
    /// API the other conditions are for.
    #[serde(default)]
    pub backend: Option<Backend>,
    /// OS driver that owns the device (`xpad`, `hid-playstation`, ...).
    #[serde(default)]
    pub driver: Option<String>,
    /// Hash of the HID report descriptor.
    #[serde(default)]
    pub descriptor_hash: Option<DescriptorHash>,
}

impl ExpectedIdentity {
    /// Whether `identity` meets every condition set.
    pub fn matches(&self, identity: &DeviceIdentity) -> bool {
        fn holds<T: PartialEq>(want: &Option<T>, have: Option<T>) -> bool {
            want.as_ref().is_none_or(|want| have.as_ref() == Some(want))
        }

        holds(&self.vendor_id, identity.vendor_id)
            && holds(&self.product_id, identity.product_id)
            && holds(&self.name.as_deref(), identity.product_name.as_deref())
            && holds(&self.os, Some(identity.os))
            && holds(&self.backend, Some(identity.backend))
            && holds(&self.driver.as_deref(), identity.driver.as_deref())
            && holds(&self.descriptor_hash, identity.descriptor_hash)
    }

    /// Number of conditions set.
    pub fn specificity(&self) -> u32 {
        let Self {
            vendor_id,
            product_id,
            name,
            os,
            backend,
            driver,
            descriptor_hash,
        } = self;
        [
            vendor_id.is_some(),
            product_id.is_some(),
            name.is_some(),
            os.is_some(),
            backend.is_some(),
            driver.is_some(),
            descriptor_hash.is_some(),
        ]
        .into_iter()
        .filter(|&set| set)
        .count() as u32
    }
}

/// Why a model, or a catalog, is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidModel {
    model: String,
    message: String,
}

impl InvalidModel {
    /// The ID of the model at fault.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid catalog model `{}`: {}",
            self.model, self.message
        )
    }
}

impl std::error::Error for InvalidModel {}

/// Whether `id` is lowercase words of ASCII letters and digits joined by
/// single hyphens.
pub(crate) fn is_slug(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Checks a text field: not blank when required, at most `max` characters,
/// no control characters but line breaks in notes.
pub(crate) fn check_text(what: &str, text: &str, max: usize, lines: bool) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{what} is empty"));
    }
    if text.chars().count() > max {
        return Err(format!("{what} is longer than {max} characters"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !(lines && c == '\n'))
    {
        return Err(format!("{what} has control characters"));
    }
    Ok(())
}

impl Model {
    /// Checks what the types cannot: the schema version, the ID's form (and
    /// `generic/` for generic models), names and instructions, at least one
    /// button and one mode, unique mode IDs, at least one connection per
    /// mode, one connection per transport (never `unknown`), product IDs
    /// with vendor IDs, and no identities on generic models.
    pub fn validate(&self) -> Result<(), InvalidModel> {
        self.check().map_err(|message| InvalidModel {
            model: self.id.clone(),
            message,
        })
    }

    fn check(&self) -> Result<(), String> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(format!(
                "schema version {} is not {CATALOG_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let mut parts = self.id.split('/');
        let (Some(vendor), Some(model), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err("the ID is not `<vendor>/<model>`".into());
        };
        if !is_slug(vendor) || !is_slug(model) {
            return Err("the ID's parts are not lowercase words joined by hyphens".into());
        }
        if self.generic != (vendor == "generic") {
            return Err("generic models, and only they, have IDs under `generic/`".into());
        }
        check_text("the vendor", &self.vendor, MAX_NAME_CHARS, false)?;
        check_text("the name", &self.name, MAX_NAME_CHARS, false)?;
        if let Some(notes) = &self.notes {
            check_text("the notes", notes, MAX_NOTE_CHARS, true)?;
        }
        check_controls("the controls", &self.controls)?;
        if self.modes.is_empty() {
            return Err("no modes".into());
        }
        let mut ids = BTreeSet::new();
        for mode in &self.modes {
            let what = format!("mode `{}`", mode.id);
            if !is_slug(&mode.id) {
                return Err(format!(
                    "{what}: the ID is not lowercase words joined by hyphens"
                ));
            }
            if !ids.insert(mode.id.as_str()) {
                return Err(format!("{what}: listed twice"));
            }
            check_text(
                &format!("{what}: the name"),
                &mode.name,
                MAX_NAME_CHARS,
                false,
            )?;
            if let Some(how_to) = &mode.how_to {
                check_text(&format!("{what}: `how_to`"), how_to, MAX_NOTE_CHARS, true)?;
            }
            if let Some(controls) = &mode.controls {
                check_controls(&format!("{what}: the controls"), controls)?;
            }
            if mode.connections.is_empty() {
                return Err(format!("{what}: no connections"));
            }
            let mut transports = BTreeSet::new();
            for connection in &mode.connections {
                let what = format!("{what}, {}", connection.transport);
                if connection.transport == Transport::Unknown {
                    return Err(format!("{what}: the transport is unknown"));
                }
                if !transports.insert(connection.transport) {
                    return Err(format!("{what}: listed twice"));
                }
                if let Some(how_to) = &connection.how_to {
                    check_text(&format!("{what}: `how_to`"), how_to, MAX_NOTE_CHARS, true)?;
                }
                if self.generic && !connection.identities.is_empty() {
                    return Err(format!("{what}: generic models have no identities"));
                }
                for identity in &connection.identities {
                    if identity.specificity() == 0 {
                        return Err(format!("{what}: an identity without conditions"));
                    }
                    if identity.product_id.is_some() && identity.vendor_id.is_none() {
                        return Err(format!("{what}: `product_id` needs `vendor_id`"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The mode with this ID.
    pub fn mode(&self, id: &str) -> Option<&Mode> {
        self.modes.iter().find(|mode| mode.id == id)
    }

    /// The controls in `mode`: the mode's own, or the model's.
    pub fn controls_in<'a>(&'a self, mode: &'a Mode) -> &'a Controls {
        mode.controls.as_ref().unwrap_or(&self.controls)
    }

    /// What is printed on the face buttons: the model's labels, else its
    /// family's, else unknown.
    pub fn face_layout(&self) -> FaceLayout {
        self.labels
            .map(Labels::face_layout)
            .or_else(|| self.family.map(Family::face_layout))
            .unwrap_or(FaceLayout::UNKNOWN)
    }
}

fn check_controls(what: &str, controls: &Controls) -> Result<(), String> {
    if controls.buttons.is_empty() {
        return Err(format!("{what}: no buttons"));
    }
    if controls
        .touchpads
        .iter()
        .any(|touchpad| touchpad.max_fingers == 0)
    {
        return Err(format!("{what}: a touchpad that tracks no finger"));
    }
    Ok(())
}

/// Every catalog model, with unique IDs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub(crate) models: Vec<Model>,
}

impl Catalog {
    /// A catalog of these models, in this order, once each is valid and no
    /// two share an ID.
    pub fn new(models: Vec<Model>) -> Result<Self, InvalidModel> {
        let mut ids = BTreeSet::new();
        for model in &models {
            model.validate()?;
            if !ids.insert(model.id.as_str()) {
                return Err(InvalidModel {
                    model: model.id.clone(),
                    message: "two models share this ID".into(),
                });
            }
        }
        Ok(Self { models })
    }

    /// Every model, in ID order for the embedded catalog.
    pub fn models(&self) -> &[Model] {
        &self.models
    }

    /// The model with this ID.
    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }
}

/// Encodes models for embedding (postcard).
#[allow(dead_code, reason = "used by the build script")]
pub(crate) fn encode_models(models: &[Model]) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(models)
}

/// Decodes models encoded by [`encode_models`].
pub(crate) fn decode_models(bytes: &[u8]) -> Result<Vec<Model>, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// The JSON Schema of a catalog model file, as committed in
/// `data/controllers/schema.json`.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(Model);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/controller.json".into(),
    );
    schema
}

/// A catalog model file that could not be read.
#[cfg(feature = "toml")]
#[derive(Debug)]
pub enum ModelError {
    /// Not TOML, or not the catalog format.
    Toml(toml::de::Error),
    /// Well-formed but invalid (see [`Model::validate`]).
    Invalid(InvalidModel),
}

#[cfg(feature = "toml")]
impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "not a catalog model: {error}"),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "toml")]
impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

#[cfg(feature = "toml")]
impl Model {
    /// Parses and [validates](Self::validate) a TOML catalog model.
    pub fn from_toml(text: &str) -> Result<Self, ModelError> {
        let model: Self = toml::from_str(text).map_err(ModelError::Toml)?;
        model.validate().map_err(ModelError::Invalid)?;
        Ok(model)
    }

    /// Writes the model as TOML, with vendor and product IDs in hexadecimal
    /// (`0x054c`) as in hand-written files.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self).map(|text| crate::profile::hex_ids(&text))
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::{Axis, Button};

    use super::*;

    fn pad() -> Model {
        Model {
            schema_version: 1,
            id: "acme/pad-2".into(),
            vendor: "Acme".into(),
            name: "Pad 2".into(),
            generic: false,
            family: None,
            labels: Some(Labels::Nintendo),
            controls: Controls {
                buttons: ButtonSet::from([Button::South, Button::East]),
                axes: AxisSet::from([Axis::LeftStickX, Axis::LeftStickY]),
                ..Controls::default()
            },
            modes: vec![Mode {
                id: "x".into(),
                name: "X".into(),
                kind: ControllerMode::Xinput,
                how_to: Some("Hold X and Start to turn it on.".into()),
                controls: None,
                connections: vec![Connection {
                    transport: Transport::Usb,
                    how_to: None,
                    identities: vec![ExpectedIdentity {
                        vendor_id: Some(0x045e),
                        product_id: Some(0x028e),
                        ..ExpectedIdentity::default()
                    }],
                }],
            }],
            notes: None,
        }
    }

    fn message(model: &Model) -> String {
        model.validate().unwrap_err().message().to_owned()
    }

    #[test]
    fn valid() {
        pad().validate().unwrap();
        assert_eq!(pad().face_layout(), FaceLayout::NINTENDO);
    }

    #[test]
    fn ids() {
        for id in [
            "acme",
            "acme/pad/2",
            "Acme/pad",
            "acme/pad_2",
            "acme/-pad",
            "acme/pad--2",
            "/pad",
        ] {
            let mut model = pad();
            model.id = id.into();
            assert!(model.validate().is_err(), "{id}");
        }
        let mut model = pad();
        model.id = "generic/pad".into();
        assert!(message(&model).contains("generic"));
        model.generic = true;
        assert!(message(&model).contains("no identities"));
    }

    #[test]
    fn modes_and_connections() {
        let mut model = pad();
        let mode = model.modes[0].clone();
        model.modes.push(mode);
        assert!(message(&model).contains("listed twice"));

        let mut model = pad();
        let connection = model.modes[0].connections[0].clone();
        model.modes[0].connections.push(connection);
        assert!(message(&model).contains("listed twice"));

        let mut model = pad();
        model.modes[0].connections[0].transport = Transport::Unknown;
        assert!(message(&model).contains("unknown"));

        let mut model = pad();
        model.modes[0].connections.clear();
        assert!(message(&model).contains("no connections"));

        let mut model = pad();
        model.modes[0].connections[0].identities[0].vendor_id = None;
        assert!(message(&model).contains("needs `vendor_id`"));

        let mut model = pad();
        model.modes[0].connections[0].identities[0] = ExpectedIdentity::default();
        assert!(message(&model).contains("without conditions"));
    }

    #[test]
    fn texts() {
        let mut model = pad();
        model.name = " ".into();
        assert!(message(&model).contains("empty"));
        let mut model = pad();
        model.vendor = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(message(&model).contains("longer"));
        let mut model = pad();
        model.notes = Some("one\ntwo".into());
        model.validate().unwrap();
        model.name = "a\tb".into();
        assert!(message(&model).contains("control"));
    }

    #[test]
    fn catalog_ids_are_unique() {
        assert!(Catalog::new(vec![pad(), pad()]).is_err());
        let catalog = Catalog::new(vec![pad()]).unwrap();
        assert!(catalog.get("acme/pad-2").is_some());
    }

    #[test]
    fn expected_identity() {
        let browser = anygamepad_model::identity::Browser {
            engine: anygamepad_model::identity::BrowserEngine::Blink,
            major_version: None,
        };
        let identity = DeviceIdentity::from_gamepad_api(
            "Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)",
            "standard",
            browser,
            Os::Windows,
        );
        let expected = &pad().modes[0].connections[0].identities[0];
        assert!(expected.matches(&identity));
        let linux = ExpectedIdentity {
            os: Some(Os::Linux),
            ..expected.clone()
        };
        assert!(!linux.matches(&identity));
    }

    #[test]
    fn embedded_catalog() {
        let catalog = Catalog::embedded();
        assert!(catalog.get("sony/dualsense").is_some());
        assert!(catalog.models().iter().any(|model| model.generic));
        Catalog::new(catalog.models().to_vec()).unwrap();
    }
}
