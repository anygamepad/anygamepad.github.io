//! The lab inventory (roadmap tasks H2 and F4, ADR 0008): the physical
//! controllers of a test lab and the machines they are tested on.
//!
//! A [`Unit`] is one physical pad, known by a [`Code`] its owner gives it
//! (`DS5-02`) and the catalog model it is. Codes are short and made of
//! letters, digits and hyphens, so they cannot hold a person's name; test
//! reports name units by their code only. Two pads of the same model are
//! told apart by their owner, never by serial numbers or Bluetooth
//! addresses, which anygamepad never reads (AGENTS.md).
//!
//! The inventory is a TOML file, `data/lab/inventory.toml`, described by a
//! JSON Schema ([`json_schema`], committed as `data/lab/schema.json`) and
//! embedded at build time when it exists ([`Inventory::embedded`]); apps
//! can also read one from a file.

use std::collections::BTreeSet;
use std::fmt;

use anygamepad_model::identity::{BrowserEngine, Os};
use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, MAX_NOTE_CHARS, check_text};

/// Version of the inventory format this library reads and writes.
pub const INVENTORY_SCHEMA_VERSION: u32 = 1;

/// Characters of a [`Code`].
pub const MAX_CODE_CHARS: usize = 16;

/// The code a person gives a unit or a machine (`DS5-02`, `LINUX-1`): 1 to
/// [`MAX_CODE_CHARS`] ASCII letters, digits and hyphens, starting with a
/// letter or a digit.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Code(String);

impl Code {
    /// The code, if `code` is one.
    pub fn new(code: &str) -> Result<Self, InvalidCode> {
        let valid = !code.is_empty()
            && code.len() <= MAX_CODE_CHARS
            && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && code.as_bytes()[0].is_ascii_alphanumeric();
        if valid {
            Ok(Self(code.to_owned()))
        } else {
            Err(InvalidCode)
        }
    }

    /// The code as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Code {
    type Error = InvalidCode;

    fn try_from(code: String) -> Result<Self, Self::Error> {
        Self::new(&code)
    }
}

impl From<Code> for String {
    fn from(code: Code) -> Self {
        code.0
    }
}

impl core::str::FromStr for Code {
    type Err = InvalidCode;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::new(code)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Code {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Code".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A unit's or a machine's code: ASCII letters, digits and hyphens, starting with a letter or a digit.",
            "type": "string",
            "pattern": "^[A-Za-z0-9][A-Za-z0-9-]{0,15}$",
        })
    }
}

/// A text that is not a [`Code`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCode;

impl fmt::Display for InvalidCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a code is 1 to {MAX_CODE_CHARS} ASCII letters, digits and hyphens, \
             starting with a letter or a digit"
        )
    }
}

impl std::error::Error for InvalidCode {}

/// The controllers of a test lab and its machines.
///
/// Fields left out of a TOML file take their defaults; the binary encoding
/// writes every field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    /// Format version: [`INVENTORY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The physical controllers, written as `[[unit]]` tables.
    #[serde(default, rename = "unit")]
    pub units: Vec<Unit>,
    /// The machines they are tested on, written as `[[machine]]` tables.
    #[serde(default, rename = "machine")]
    pub machines: Vec<Machine>,
}

/// One physical controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Unit {
    /// Its code, unique in the inventory.
    pub code: Code,
    /// The ID of its catalog model (`sony/dualsense`).
    pub model: String,
    /// Edition, colour, firmware as a vendor app shows it; never about a
    /// person.
    #[serde(default)]
    pub notes: Option<String>,
}

/// A machine controllers are tested on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Its code, unique in the inventory. Test reports never name it.
    pub code: Code,
    /// Its OS.
    pub os: Os,
    /// The OS release (`14.6`, `11 24H2`, `Fedora 41`).
    #[serde(default)]
    pub os_version: Option<String>,
    /// The browser engines installed.
    #[serde(default)]
    pub browsers: Vec<BrowserEngine>,
    /// Anything else worth knowing; never about a person.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Why an inventory is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidInventory {
    message: String,
}

impl InvalidInventory {
    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidInventory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid inventory: {}", self.message)
    }
}

impl std::error::Error for InvalidInventory {}

impl Inventory {
    /// Checks what the types cannot: the schema version, unique unit and
    /// machine codes, notes and OS versions, and, given a catalog, that
    /// every unit's model is in it.
    pub fn validate(&self, catalog: Option<&Catalog>) -> Result<(), InvalidInventory> {
        self.check(catalog)
            .map_err(|message| InvalidInventory { message })
    }

    fn check(&self, catalog: Option<&Catalog>) -> Result<(), String> {
        if self.schema_version != INVENTORY_SCHEMA_VERSION {
            return Err(format!(
                "schema version {} is not {INVENTORY_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let mut codes = BTreeSet::new();
        for unit in &self.units {
            let what = format!("unit `{}`", unit.code);
            if !codes.insert(&unit.code) {
                return Err(format!("{what}: listed twice"));
            }
            if let Some(catalog) = catalog {
                if catalog.get(&unit.model).is_none() {
                    return Err(format!("{what}: no catalog model `{}`", unit.model));
                }
            }
            if let Some(notes) = &unit.notes {
                check_text(&format!("{what}: the notes"), notes, MAX_NOTE_CHARS, true)?;
            }
        }
        let mut codes = BTreeSet::new();
        for machine in &self.machines {
            let what = format!("machine `{}`", machine.code);
            if !codes.insert(&machine.code) {
                return Err(format!("{what}: listed twice"));
            }
            if let Some(version) = &machine.os_version {
                check_text(&format!("{what}: the OS version"), version, 100, false)?;
            }
            if let Some(notes) = &machine.notes {
                check_text(&format!("{what}: the notes"), notes, MAX_NOTE_CHARS, true)?;
            }
        }
        Ok(())
    }

    /// The unit with this code.
    pub fn unit(&self, code: &str) -> Option<&Unit> {
        self.units.iter().find(|unit| unit.code.as_str() == code)
    }

    /// The units of this catalog model, in inventory order.
    pub fn units_of<'a>(&'a self, model: &'a str) -> impl Iterator<Item = &'a Unit> + 'a {
        self.units.iter().filter(move |unit| unit.model == model)
    }
}

/// Encodes an inventory for embedding (postcard).
#[allow(dead_code, reason = "used by the build script")]
pub(crate) fn encode_inventory(inventory: &Inventory) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(inventory)
}

/// Decodes an inventory encoded by [`encode_inventory`].
pub(crate) fn decode_inventory(bytes: &[u8]) -> Result<Inventory, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// The JSON Schema of an inventory file, as committed in
/// `data/lab/schema.json`.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(Inventory);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/inventory.json".into(),
    );
    schema
}

/// An inventory file that could not be read.
#[cfg(feature = "toml")]
#[derive(Debug)]
pub enum InventoryError {
    /// Not TOML, or not the inventory format.
    Toml(toml::de::Error),
    /// Well-formed but invalid (see [`Inventory::validate`]).
    Invalid(InvalidInventory),
}

#[cfg(feature = "toml")]
impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "not an inventory: {error}"),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "toml")]
impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

#[cfg(feature = "toml")]
impl Inventory {
    /// Parses and [validates](Self::validate) a TOML inventory against
    /// `catalog`, when given.
    pub fn from_toml(text: &str, catalog: Option<&Catalog>) -> Result<Self, InventoryError> {
        let inventory: Self = toml::from_str(text).map_err(InventoryError::Toml)?;
        inventory
            .validate(catalog)
            .map_err(InventoryError::Invalid)?;
        Ok(inventory)
    }

    /// Writes the inventory as TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        for code in ["DS5-01", "a", "XSX-2", "0123456789ABCDEF", "8BD-U2C-01"] {
            assert_eq!(Code::new(code).unwrap().as_str(), code);
        }
        for code in [
            "",
            "-DS5",
            "DS5 01",
            "DS5_01",
            "Ana's pad",
            "0123456789ABCDEFG",
            "pad✓",
        ] {
            assert!(Code::new(code).is_err(), "{code}");
        }
    }

    fn inventory() -> Inventory {
        Inventory {
            schema_version: 1,
            units: vec![Unit {
                code: Code::new("DS5-01").unwrap(),
                model: "sony/dualsense".into(),
                notes: Some("White".into()),
            }],
            machines: vec![Machine {
                code: Code::new("MAC-1").unwrap(),
                os: Os::MacOs,
                os_version: Some("15.1".into()),
                browsers: vec![BrowserEngine::Blink, BrowserEngine::WebKit],
                notes: None,
            }],
        }
    }

    #[test]
    fn validate() {
        let catalog = Catalog::embedded();
        inventory().validate(Some(catalog)).unwrap();

        let mut twice = inventory();
        twice.units.push(twice.units[0].clone());
        assert!(
            twice
                .validate(None)
                .unwrap_err()
                .message()
                .contains("twice")
        );

        let mut unknown = inventory();
        unknown.units[0].model = "acme/nothing".into();
        unknown.validate(None).unwrap();
        assert!(unknown.validate(Some(catalog)).is_err());

        let mut notes = inventory();
        notes.machines[0].notes = Some("\u{7}".into());
        assert!(notes.validate(None).is_err());
    }

    #[test]
    fn units_of() {
        let inventory = inventory();
        assert_eq!(inventory.units_of("sony/dualsense").count(), 1);
        assert!(inventory.unit("DS5-01").is_some());
        assert!(inventory.unit("DS5-02").is_none());
    }

    #[test]
    fn postcard_round_trip() {
        let inventory = inventory();
        let bytes = encode_inventory(&inventory).unwrap();
        assert_eq!(decode_inventory(&bytes).unwrap(), inventory);
    }
}
