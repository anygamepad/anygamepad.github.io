//! Device profiles: which devices a profile is for ([`MatchRule`]) and how
//! their raw elements map to the canonical model ([`Mapping`]).

use std::collections::BTreeMap;
use std::fmt;

use anygamepad_model::identity::{Backend, BrowserEngine, DescriptorHash, DeviceIdentity, Os};
use anygamepad_model::sdl::SdlGuid;
use anygamepad_model::{Axis, Button, FaceLayout, Family, Transport};
use serde::{Deserialize, Serialize};

use crate::Input;

/// The profile format version this library reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Where a profile comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    /// Written by hand in `data/profiles/`.
    Curated,
    /// Imported from SDL GameControllerDB ([`crate::sdl`]).
    #[serde(rename = "sdl-gamecontrollerdb")]
    SdlGameControllerDb,
    /// Generated from device reports collected by the testers (P4).
    Report,
}

impl ProfileSource {
    /// Precedence between profiles that match a device equally well:
    /// curated, then reports, then SDL GameControllerDB.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Curated => 2,
            Self::Report => 1,
            Self::SdlGameControllerDb => 0,
        }
    }
}

/// What the indices of a profile's inputs (`b3`, `a1`, `h0`) count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Indices {
    /// Positions in the list of the backend the profile matches (Gamepad API
    /// `buttons` and `axes`, DirectInput objects, ...).
    #[default]
    Backend,
    /// SDL's joystick indices on the SDL backend named by the match rule's
    /// `sdl_guid` (its driver signature) for the rule's `os`, as in SDL
    /// GameControllerDB. Only providers that reproduce SDL's order can apply
    /// such a profile.
    Sdl,
}

impl Indices {
    fn is_backend(&self) -> bool {
        *self == Self::Backend
    }
}

/// Conditions on a [`DeviceIdentity`]; every field that is set must hold.
///
/// Used both to select profiles (`[[match]]`) and to restrict mapping
/// entries (`when`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MatchRule {
    /// USB/Bluetooth vendor ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<u16>,
    /// Product ID; needs `vendor_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<u16>,
    /// Version (`bcdDevice`, HID or evdev version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u16>,
    /// Product name, exactly as the backend reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// OS driver that owns the device (`xpad`, `hid-playstation`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// Host OS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    /// API the device is read through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
    /// How the device is connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// Browser engine, for web backends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserEngine>,
    /// `Gamepad.mapping` (`"standard"`, `""`), for the Gamepad API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamepad_mapping: Option<String>,
    /// Hash of the HID report descriptor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor_hash: Option<DescriptorHash>,
    /// SDL joystick GUID. Compared without its name CRC, and without the
    /// version when the GUID's version is 0, as SDL does; the device's GUID
    /// is computed with this GUID's driver signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdl_guid: Option<SdlGuid>,
}

impl MatchRule {
    /// Whether no condition is set (the rule holds for every device).
    pub fn is_empty(&self) -> bool {
        self.specificity() == 0
    }

    /// Number of conditions set.
    pub fn specificity(&self) -> u32 {
        let Self {
            vendor_id,
            product_id,
            version,
            name,
            driver,
            os,
            backend,
            transport,
            browser,
            gamepad_mapping,
            descriptor_hash,
            sdl_guid,
        } = self;
        [
            vendor_id.is_some(),
            product_id.is_some(),
            version.is_some(),
            name.is_some(),
            driver.is_some(),
            os.is_some(),
            backend.is_some(),
            transport.is_some(),
            browser.is_some(),
            gamepad_mapping.is_some(),
            descriptor_hash.is_some(),
            sdl_guid.is_some(),
        ]
        .into_iter()
        .filter(|&set| set)
        .count() as u32
    }

    /// Whether the rule names a device (product ID, name, descriptor hash or
    /// SDL GUID) rather than a class of devices (an OS, a backend, a
    /// browser mapping, a vendor).
    pub fn identifies_device(&self) -> bool {
        self.product_id.is_some()
            || self.name.is_some()
            || self.descriptor_hash.is_some()
            || self.sdl_guid.is_some()
    }

    /// Whether `identity` meets every condition.
    pub fn matches(&self, identity: &DeviceIdentity) -> bool {
        fn holds<T: PartialEq>(want: &Option<T>, have: Option<T>) -> bool {
            want.as_ref().is_none_or(|want| have.as_ref() == Some(want))
        }

        holds(&self.vendor_id, identity.vendor_id)
            && holds(&self.product_id, identity.product_id)
            && holds(&self.version, identity.version)
            && holds(&self.name.as_deref(), identity.product_name.as_deref())
            && holds(&self.driver.as_deref(), identity.driver.as_deref())
            && holds(&self.os, Some(identity.os))
            && holds(&self.backend, Some(identity.backend))
            && holds(&self.transport, Some(identity.transport))
            && holds(
                &self.browser,
                identity.browser.map(|browser| browser.engine),
            )
            && holds(
                &self.gamepad_mapping.as_deref(),
                identity.web.as_ref().map(|web| web.mapping.as_str()),
            )
            && holds(&self.descriptor_hash, identity.descriptor_hash)
            && self
                .sdl_guid
                .is_none_or(|guid| sdl_guid_matches(guid, identity))
    }

    fn check(&self, what: &str) -> Result<(), String> {
        if self.product_id.is_some() && self.vendor_id.is_none() {
            return Err(format!("{what}: `product_id` needs `vendor_id`"));
        }
        Ok(())
    }
}

/// Whether the GUID SDL would give `identity` equals `rule`, ignoring the
/// name CRC, and the version when `rule` has none (SDL's own lookup).
fn sdl_guid_matches(rule: SdlGuid, identity: &DeviceIdentity) -> bool {
    let mut device = identity
        .sdl_guid(rule.driver_signature(), rule.driver_data())
        .without_crc();
    if rule.ids().is_some_and(|ids| ids.version == 0) {
        device.0[12] = 0;
        device.0[13] = 0;
    }
    device == rule.without_crc()
}

/// How a canonical axis is driven.
///
/// Written in profiles as an [`Input`] string when it is the whole axis
/// (`left_stick_x = "a0"`), or as a table:
///
/// ```toml
/// [mapping.axes]
/// left_stick_x = { negative = "b14", positive = "b15" }
/// right_trigger = { input = "a5", deadzone = 0.05 }
/// ```
///
/// Exactly one of `input` or the halves (`negative`, `positive`, or both)
/// is set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisMapping {
    /// Drives the whole axis: a stick from -1 to 1, a trigger from 0 to 1.
    pub input: Option<Input>,
    /// Drives the axis from 0 to -1 (sticks only).
    pub negative: Option<Input>,
    /// Drives the axis from 0 to 1.
    pub positive: Option<Input>,
    /// Calibration: readings within this fraction of the travel from rest
    /// read as rest, in `[0, 1)`.
    pub deadzone: Option<f32>,
}

impl AxisMapping {
    /// The axis driven by `input` over its whole range.
    pub const fn full(input: Input) -> Self {
        Self {
            input: Some(input),
            negative: None,
            positive: None,
            deadzone: None,
        }
    }

    fn check(&self, axis: Axis) -> Result<(), String> {
        let halves = self.negative.is_some() || self.positive.is_some();
        if self.input.is_some() == halves {
            return Err(format!(
                "axis `{axis}`: set either `input` or `negative`/`positive`"
            ));
        }
        if self.negative.is_some() && matches!(axis, Axis::LeftTrigger | Axis::RightTrigger) {
            return Err(format!("axis `{axis}`: triggers have no negative half"));
        }
        if let Some(deadzone) = self.deadzone {
            if !(0.0..1.0).contains(&deadzone) {
                return Err(format!(
                    "axis `{axis}`: `deadzone` must be in [0, 1), not {deadzone}"
                ));
            }
        }
        Ok(())
    }
}

/// The table form of [`AxisMapping`].
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    negative: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    positive: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deadzone: Option<f32>,
}

impl Serialize for AxisMapping {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            Self {
                input: Some(input),
                negative: None,
                positive: None,
                deadzone: None,
            } if serializer.is_human_readable() => input.serialize(serializer),
            Self {
                input,
                negative,
                positive,
                deadzone,
            } => AxisTable {
                input,
                negative,
                positive,
                deadzone,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AxisMapping {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = AxisMapping;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an input string or an axis mapping table")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<AxisMapping, E> {
                v.parse().map(AxisMapping::full).map_err(E::custom)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<AxisMapping, A::Error> {
                AxisTable::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(from_table)
            }
        }

        fn from_table(table: AxisTable) -> AxisMapping {
            AxisMapping {
                input: table.input,
                negative: table.negative,
                positive: table.positive,
                deadzone: table.deadzone,
            }
        }

        // Binary formats (the embedded database) always hold the table and
        // cannot be asked for "a string or a map".
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(Visitor)
        } else {
            AxisTable::deserialize(deserializer).map(from_table)
        }
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for AxisMapping {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AxisMapping".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let input = generator.subschema_for::<Input>();
        let halves = schemars::json_schema!({
            "anyOf": [{ "required": ["negative"] }, { "required": ["positive"] }],
        });
        schemars::json_schema!({
            "description": "How a canonical axis is driven: an input over the whole \
                axis, or a table with either `input` or the halves (`negative`, \
                `positive`), and an optional `deadzone`.",
            "anyOf": [
                input,
                {
                    "type": "object",
                    "properties": {
                        "input": input,
                        "negative": input,
                        "positive": input,
                        "deadzone": { "type": "number", "minimum": 0, "exclusiveMaximum": 1 },
                    },
                    "additionalProperties": false,
                    "anyOf": [
                        { "required": ["input"], "not": halves },
                        { "allOf": [halves, { "not": { "required": ["input"] } }] },
                    ],
                },
            ],
        })
    }
}

/// One entry of a profile's mapping: canonical targets and the raw inputs
/// that drive them, for the devices that meet `when`.
///
/// A target reads its input's position `t` in `[0, 1]` (see [`Input`]):
///
/// - a **button** is pressed while `t` is at least 0.5; a `b` input presses
///   it from 30/255 on, the threshold of analog buttons (Chromium's, and
///   XInput's for triggers);
/// - a **stick axis** is `2t - 1` (from -1 to 1), a **trigger axis** is `t`;
/// - a `negative` half is `-t`, a `positive` half is `t`; both halves add
///   up.
///
/// Inactive inputs (the other half of a half axis) leave their target at
/// rest. Stick Y axes are positive **up** (ADR 0001): most backends report
/// them positive down, so their inputs end in `~`.
///
/// Where no entry sets a target, raw elements that are already canonical
/// (`b{button:south}`, `a{axis:left_trigger}`) drive their own control, and
/// a trigger button follows its trigger axis from 30/255 on. The mapping
/// engine (`anygamepad::mapping`) applies a mapping.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    /// Devices this entry applies to; empty for all the profile matches.
    #[serde(default, skip_serializing_if = "MatchRule::is_empty")]
    pub when: MatchRule,
    /// Canonical buttons and their inputs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "map_schema::<Button, Input>")
    )]
    pub buttons: BTreeMap<Button, Input>,
    /// Canonical axes and their inputs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "map_schema::<Axis, AxisMapping>")
    )]
    pub axes: BTreeMap<Axis, AxisMapping>,
}

/// A table whose keys are `K` names: schemars leaves map keys unchecked.
#[cfg(feature = "schemars")]
fn map_schema<K: schemars::JsonSchema, V: schemars::JsonSchema>(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "propertyNames": generator.subschema_for::<K>(),
        "additionalProperties": generator.subschema_for::<V>(),
    })
}

/// A device profile.
///
/// See `data/profiles/README.md` for the file format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Format version: [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Human-readable name of the device or class of devices.
    pub name: String,
    /// Where the profile comes from.
    pub source: ProfileSource,
    /// Whether the mapping was checked on real hardware.
    pub verified: bool,
    /// Device family, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    /// Labels printed on the face buttons; the family's when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceLayout>,
    /// What input indices count.
    #[serde(default, skip_serializing_if = "Indices::is_backend")]
    pub indices: Indices,
    /// Origin and license notice of imported data (required for SDL
    /// GameControllerDB entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// The devices this profile is for: any rule may hold.
    #[serde(rename = "match")]
    pub matches: Vec<MatchRule>,
    /// Mapping entries; those whose `when` holds apply in order, and later
    /// entries replace the targets they set.
    #[serde(rename = "mapping", default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<Mapping>,
}

/// How well a profile matches a device; higher is better.
///
/// Ordered by, in turn: whether the rule names the device (product, name,
/// descriptor or SDL GUID) rather than a class of devices, whether the
/// profile is verified, its [source](ProfileSource::rank), and the number
/// of conditions of the rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchScore {
    /// The rule names the device.
    pub identifies_device: bool,
    /// The profile is verified.
    pub verified: bool,
    /// [`ProfileSource::rank`].
    pub source_rank: u8,
    /// [`MatchRule::specificity`].
    pub specificity: u32,
}

/// Error returned by [`Profile::validate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidProfile {
    profile: String,
    message: String,
}

impl InvalidProfile {
    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid profile `{}`: {}", self.profile, self.message)
    }
}

impl std::error::Error for InvalidProfile {}

impl Profile {
    /// Checks what the types cannot: the schema version, a non-empty name,
    /// at least one non-empty match rule, product IDs with vendor IDs, axis
    /// mappings with either an input or halves, deadzones in `[0, 1)`, and
    /// attribution on imported profiles.
    pub fn validate(&self) -> Result<(), InvalidProfile> {
        self.check().map_err(|message| InvalidProfile {
            profile: self.name.clone(),
            message,
        })
    }

    fn check(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "`schema_version` is {}; this library reads {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.name.trim().is_empty() {
            return Err("`name` is empty".into());
        }
        if self.source == ProfileSource::SdlGameControllerDb && self.attribution.is_none() {
            return Err("imported profiles need `attribution`".into());
        }
        if self.matches.is_empty() {
            return Err("no `[[match]]` rule".into());
        }
        for (index, rule) in self.matches.iter().enumerate() {
            let what = format!("match rule {}", index + 1);
            if rule.is_empty() {
                return Err(format!("{what} is empty and would match every device"));
            }
            rule.check(&what)?;
        }
        for (index, mapping) in self.mappings.iter().enumerate() {
            mapping
                .when
                .check(&format!("`when` of mapping entry {}", index + 1))?;
            for (&axis, axis_mapping) in &mapping.axes {
                axis_mapping
                    .check(axis)
                    .map_err(|message| format!("mapping entry {}: {message}", index + 1))?;
            }
        }
        Ok(())
    }

    /// How well the profile matches `identity`: the best score among its
    /// rules that hold, `None` when none does.
    pub fn match_score(&self, identity: &DeviceIdentity) -> Option<MatchScore> {
        self.matches
            .iter()
            .filter(|rule| rule.matches(identity))
            .map(|rule| MatchScore {
                identifies_device: rule.identifies_device(),
                verified: self.verified,
                source_rank: self.source.rank(),
                specificity: rule.specificity(),
            })
            .max()
    }

    /// The mapping for `identity`: the entries whose `when` holds, merged in
    /// order (a later entry replaces the targets it sets).
    pub fn mapping_for(&self, identity: &DeviceIdentity) -> Mapping {
        let mut merged = Mapping::default();
        for entry in self.mappings.iter().filter(|m| m.when.matches(identity)) {
            merged.buttons.extend(&entry.buttons);
            merged.axes.extend(&entry.axes);
        }
        merged
    }

    /// The labels of the face buttons: `face`, else the family's, else
    /// unknown.
    pub fn face_layout(&self) -> FaceLayout {
        self.face
            .or(self.family.map(Family::face_layout))
            .unwrap_or(FaceLayout::UNKNOWN)
    }
}

/// Error returned by [`Profile::from_toml`].
#[cfg(feature = "toml")]
#[derive(Debug)]
pub enum ProfileError {
    /// Not TOML, or not the profile format.
    Toml(toml::de::Error),
    /// Well-formed but invalid (see [`Profile::validate`]).
    Invalid(InvalidProfile),
}

#[cfg(feature = "toml")]
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "toml")]
impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

#[cfg(feature = "toml")]
impl Profile {
    /// Parses and [validates](Self::validate) a TOML profile.
    pub fn from_toml(text: &str) -> Result<Self, ProfileError> {
        let profile: Self = toml::from_str(text).map_err(ProfileError::Toml)?;
        profile.validate().map_err(ProfileError::Invalid)?;
        Ok(profile)
    }

    /// Writes the profile as TOML, with vendor IDs, product IDs and
    /// versions in hexadecimal (`0x054c`) as in hand-written profiles.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        let text = toml::to_string(self)?;
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            let hex = ["vendor_id", "product_id", "version"]
                .iter()
                .find_map(|key| {
                    let value = line.strip_prefix(key)?.strip_prefix(" = ")?;
                    value.parse::<u16>().ok().map(|value| (key, value))
                });
            match hex {
                Some((key, value)) => out.push_str(&format!("{key} = {value:#06x}")),
                None => out.push_str(line),
            }
            out.push('\n');
        }
        Ok(out)
    }
}

/// The profile that matches `identity` best (see [`MatchScore`]); the first
/// one on ties.
///
/// Filter the profiles first when the caller cannot apply some of them
/// (profiles with [`Indices::Sdl`] on a provider that does not use SDL's
/// order).
pub fn best_match<'a>(
    profiles: impl IntoIterator<Item = &'a Profile>,
    identity: &DeviceIdentity,
) -> Option<&'a Profile> {
    let mut best: Option<(MatchScore, &Profile)> = None;
    for profile in profiles {
        if let Some(score) = profile.match_score(identity) {
            if best.is_none_or(|(best, _)| score > best) {
                best = Some((score, profile));
            }
        }
    }
    best.map(|(_, profile)| profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anygamepad_model::identity::{Browser, ProviderKind};

    fn input(s: &str) -> Input {
        s.parse().unwrap()
    }

    fn profile(name: &str, source: ProfileSource, verified: bool, rule: MatchRule) -> Profile {
        Profile {
            schema_version: SCHEMA_VERSION,
            name: name.into(),
            source,
            verified,
            family: None,
            face: None,
            indices: Indices::Backend,
            attribution: None,
            matches: vec![rule],
            mappings: vec![],
        }
    }

    fn chrome_dualshock4() -> DeviceIdentity {
        DeviceIdentity::from_gamepad_api(
            "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)",
            "standard",
            Browser {
                engine: BrowserEngine::Blink,
                major_version: Some(141),
            },
            Os::MacOs,
        )
    }

    fn linux_dualshock4() -> DeviceIdentity {
        DeviceIdentity {
            transport: Transport::Usb,
            vendor_id: Some(0x054c),
            product_id: Some(0x09cc),
            version: Some(0x8111),
            product_name: Some("Sony Interactive Entertainment Wireless Controller".into()),
            driver: Some("hid-playstation".into()),
            ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
        }
    }

    #[test]
    fn rules_match_set_fields_only() {
        let identity = chrome_dualshock4();
        assert!(MatchRule::default().matches(&identity));
        let rule = MatchRule {
            vendor_id: Some(0x054c),
            product_id: Some(0x09cc),
            browser: Some(BrowserEngine::Blink),
            gamepad_mapping: Some("standard".into()),
            ..MatchRule::default()
        };
        assert!(rule.matches(&identity));
        assert_eq!(rule.specificity(), 4);
        assert!(rule.identifies_device());
        for miss in [
            MatchRule {
                product_id: Some(0x05c4),
                ..rule.clone()
            },
            MatchRule {
                os: Some(Os::Windows),
                ..rule.clone()
            },
            MatchRule {
                gamepad_mapping: Some(String::new()),
                ..rule.clone()
            },
            // The Gamepad API exposes no version, driver or descriptor.
            MatchRule {
                version: Some(0x0100),
                ..rule.clone()
            },
            MatchRule {
                driver: Some("hid-playstation".into()),
                ..rule.clone()
            },
        ] {
            assert!(!miss.matches(&identity), "{miss:?}");
        }
    }

    #[test]
    fn sdl_guids_match_without_crc_and_zero_version() {
        let identity = linux_dualshock4();
        // As SDL's Linux backend builds it: no driver signature.
        let exact = identity.sdl_guid(0, 0);
        let rule = |guid: SdlGuid| MatchRule {
            sdl_guid: Some(guid),
            ..MatchRule::default()
        };
        assert!(rule(exact).matches(&identity));
        assert!(rule(exact.without_crc()).matches(&identity));

        let mut any_version = exact.without_crc();
        any_version.0[12] = 0;
        any_version.0[13] = 0;
        assert!(rule(any_version).matches(&identity));

        let mut other_version = exact;
        other_version.0[12] = 0x01;
        assert!(!rule(other_version).matches(&identity));

        // The identity does not say which SDL backend would read it: the
        // GUID is computed with the rule's driver signature (HIDAPI here).
        let mut hidapi = exact;
        hidapi.0[14] = b'h';
        assert!(
            rule(hidapi).matches(&identity),
            "computed with the rule's signature"
        );
        let mut bluetooth = exact;
        bluetooth.0[0] = 0x05;
        assert!(!rule(bluetooth).matches(&identity));
    }

    #[test]
    fn best_match_prefers_device_rules_then_verified_then_source() {
        let identity = linux_dualshock4();
        let class = profile(
            "Linux evdev pads",
            ProfileSource::Curated,
            true,
            MatchRule {
                os: Some(Os::Linux),
                backend: Some(Backend::Evdev),
                driver: Some("hid-playstation".into()),
                ..MatchRule::default()
            },
        );
        let sdl = profile(
            "PS4 Controller",
            ProfileSource::SdlGameControllerDb,
            false,
            MatchRule {
                vendor_id: Some(0x054c),
                product_id: Some(0x09cc),
                os: Some(Os::Linux),
                sdl_guid: Some(identity.sdl_guid(0, 0).without_crc()),
                ..MatchRule::default()
            },
        );
        let curated = profile(
            "DualShock 4",
            ProfileSource::Curated,
            true,
            MatchRule {
                vendor_id: Some(0x054c),
                product_id: Some(0x09cc),
                ..MatchRule::default()
            },
        );
        let unverified_curated = Profile {
            verified: false,
            ..curated.clone()
        };
        let report = Profile {
            source: ProfileSource::Report,
            ..unverified_curated.clone()
        };
        let other = profile(
            "DualSense",
            ProfileSource::Curated,
            true,
            MatchRule {
                vendor_id: Some(0x054c),
                product_id: Some(0x0ce6),
                ..MatchRule::default()
            },
        );

        fn best<'a>(list: &[&'a Profile]) -> Option<&'a Profile> {
            best_match(list.iter().copied(), &linux_dualshock4())
        }
        assert_eq!(best(&[&other]), None);
        // A device rule beats a more specific class rule.
        assert_eq!(best(&[&class, &sdl]), Some(&sdl));
        // Verified beats a more specific unverified rule.
        assert_eq!(best(&[&sdl, &curated]), Some(&curated));
        // Then curated beats reports, which beat SDL.
        assert_eq!(best(&[&sdl, &report]), Some(&report));
        assert_eq!(
            best(&[&report, &unverified_curated]),
            Some(&unverified_curated)
        );
        // Ties keep the first.
        let twin = Profile {
            name: "Twin".into(),
            ..curated.clone()
        };
        assert_eq!(best(&[&curated, &twin]).unwrap().name, "DualShock 4");
    }

    #[test]
    fn mapping_entries_merge_in_order() {
        let mut profile = profile(
            "Pad",
            ProfileSource::Curated,
            true,
            MatchRule {
                vendor_id: Some(0x054c),
                ..MatchRule::default()
            },
        );
        profile.mappings = vec![
            Mapping {
                buttons: BTreeMap::from([
                    (Button::South, input("b0")),
                    (Button::East, input("b1")),
                ]),
                axes: BTreeMap::from([(Axis::LeftStickY, AxisMapping::full(input("a1~")))]),
                ..Mapping::default()
            },
            Mapping {
                when: MatchRule {
                    os: Some(Os::MacOs),
                    ..MatchRule::default()
                },
                buttons: BTreeMap::from([(Button::South, input("b2"))]),
                ..Mapping::default()
            },
            Mapping {
                when: MatchRule {
                    os: Some(Os::Windows),
                    ..MatchRule::default()
                },
                buttons: BTreeMap::from([(Button::East, input("b9"))]),
                ..Mapping::default()
            },
        ];
        let mapping = profile.mapping_for(&chrome_dualshock4());
        assert_eq!(mapping.buttons[&Button::South], input("b2"));
        assert_eq!(mapping.buttons[&Button::East], input("b1"));
        assert_eq!(mapping.axes[&Axis::LeftStickY].input, Some(input("a1~")));
        assert!(mapping.when.is_empty());
    }

    #[test]
    fn validation() {
        let rule = MatchRule {
            vendor_id: Some(0x054c),
            ..MatchRule::default()
        };
        let valid = profile("Pad", ProfileSource::Curated, true, rule.clone());
        assert_eq!(valid.validate(), Ok(()));

        let with_axis = |axis: Axis, mapping: AxisMapping| Profile {
            mappings: vec![Mapping {
                axes: BTreeMap::from([(axis, mapping)]),
                ..Mapping::default()
            }],
            ..valid.clone()
        };
        let halves = AxisMapping {
            negative: Some(input("b14")),
            positive: Some(input("b15")),
            ..AxisMapping::default()
        };
        assert_eq!(with_axis(Axis::LeftStickX, halves).validate(), Ok(()));

        for (invalid, message) in [
            (
                Profile {
                    schema_version: 2,
                    ..valid.clone()
                },
                "`schema_version` is 2; this library reads 1",
            ),
            (
                Profile {
                    name: " ".into(),
                    ..valid.clone()
                },
                "`name` is empty",
            ),
            (
                Profile {
                    source: ProfileSource::SdlGameControllerDb,
                    ..valid.clone()
                },
                "imported profiles need `attribution`",
            ),
            (
                Profile {
                    matches: vec![],
                    ..valid.clone()
                },
                "no `[[match]]` rule",
            ),
            (
                Profile {
                    matches: vec![rule.clone(), MatchRule::default()],
                    ..valid.clone()
                },
                "match rule 2 is empty and would match every device",
            ),
            (
                Profile {
                    matches: vec![MatchRule {
                        product_id: Some(0x09cc),
                        ..MatchRule::default()
                    }],
                    ..valid.clone()
                },
                "match rule 1: `product_id` needs `vendor_id`",
            ),
            (
                with_axis(Axis::LeftStickX, AxisMapping::default()),
                "mapping entry 1: axis `left_stick_x`: set either `input` or `negative`/`positive`",
            ),
            (
                with_axis(
                    Axis::LeftStickX,
                    AxisMapping {
                        input: Some(input("a0")),
                        ..halves
                    },
                ),
                "mapping entry 1: axis `left_stick_x`: set either `input` or `negative`/`positive`",
            ),
            (
                with_axis(Axis::RightTrigger, halves),
                "mapping entry 1: axis `right_trigger`: triggers have no negative half",
            ),
            (
                with_axis(
                    Axis::RightTrigger,
                    AxisMapping {
                        deadzone: Some(1.0),
                        ..AxisMapping::full(input("a5"))
                    },
                ),
                "mapping entry 1: axis `right_trigger`: `deadzone` must be in [0, 1), not 1",
            ),
        ] {
            assert_eq!(invalid.validate().unwrap_err().message(), message);
        }
    }

    #[test]
    fn face_layout_falls_back_to_family() {
        let mut pad = profile(
            "Pad",
            ProfileSource::Curated,
            true,
            MatchRule {
                vendor_id: Some(0x057e),
                ..MatchRule::default()
            },
        );
        assert_eq!(pad.face_layout(), FaceLayout::UNKNOWN);
        pad.family = Some(Family::SwitchPro);
        assert_eq!(pad.face_layout(), FaceLayout::NINTENDO);
        pad.face = Some(FaceLayout::XBOX);
        assert_eq!(pad.face_layout(), FaceLayout::XBOX);
    }
}
