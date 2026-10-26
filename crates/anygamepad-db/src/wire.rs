//! Compact binary encoding of [`Profile`]s using [`postcard`].
//!
//! Non-human-readable serializers like `postcard` serialize struct fields
//! positionally without field names and do not support `skip_serializing_if`.
//! The wire types in this module mirror the profile structures without
//! skipping fields, enabling fast and compact serialization and deserialization
//! for the compile-time embedded database (ADR 0003).

use std::collections::BTreeMap;

use anygamepad_model::identity::{Backend, BrowserEngine, DescriptorHash, Os};
use anygamepad_model::sdl::SdlGuid;
use anygamepad_model::{Axis, Button, FaceLayout, Family, Transport};
use serde::{Deserialize, Serialize};

use crate::{AxisMapping, Indices, Input, Mapping, MatchRule, Profile, ProfileSource};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CompactWireRule {
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    version: Option<u16>,
    name: Option<String>,
    driver: Option<String>,
    os: Option<Os>,
    backend: Option<Backend>,
    transport: Option<Transport>,
    browser: Option<BrowserEngine>,
    gamepad_mapping: Option<String>,
    descriptor_hash: Option<DescriptorHash>,
    sdl_guid: Option<SdlGuid>,
}

impl From<&MatchRule> for CompactWireRule {
    fn from(rule: &MatchRule) -> Self {
        Self {
            vendor_id: rule.vendor_id,
            product_id: rule.product_id,
            version: rule.version,
            name: rule.name.clone(),
            driver: rule.driver.clone(),
            os: rule.os,
            backend: rule.backend,
            transport: rule.transport,
            browser: rule.browser,
            gamepad_mapping: rule.gamepad_mapping.clone(),
            descriptor_hash: rule.descriptor_hash,
            sdl_guid: rule.sdl_guid,
        }
    }
}

impl From<CompactWireRule> for MatchRule {
    fn from(rule: CompactWireRule) -> Self {
        Self {
            vendor_id: rule.vendor_id,
            product_id: rule.product_id,
            version: rule.version,
            name: rule.name,
            driver: rule.driver,
            os: rule.os,
            backend: rule.backend,
            transport: rule.transport,
            browser: rule.browser,
            gamepad_mapping: rule.gamepad_mapping,
            descriptor_hash: rule.descriptor_hash,
            sdl_guid: rule.sdl_guid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct CompactWireAxis {
    input: Option<Input>,
    negative: Option<Input>,
    positive: Option<Input>,
    deadzone: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CompactWireMapping {
    when: CompactWireRule,
    buttons: BTreeMap<Button, Input>,
    axes: BTreeMap<Axis, CompactWireAxis>,
}

impl From<&Mapping> for CompactWireMapping {
    fn from(mapping: &Mapping) -> Self {
        Self {
            when: (&mapping.when).into(),
            buttons: mapping.buttons.clone(),
            axes: mapping
                .axes
                .iter()
                .map(|(&axis, m)| {
                    (
                        axis,
                        CompactWireAxis {
                            input: m.input,
                            negative: m.negative,
                            positive: m.positive,
                            deadzone: m.deadzone,
                        },
                    )
                })
                .collect(),
        }
    }
}

impl From<CompactWireMapping> for Mapping {
    fn from(mapping: CompactWireMapping) -> Self {
        Self {
            when: mapping.when.into(),
            buttons: mapping.buttons,
            axes: mapping
                .axes
                .into_iter()
                .map(|(axis, m)| {
                    (
                        axis,
                        AxisMapping {
                            input: m.input,
                            negative: m.negative,
                            positive: m.positive,
                            deadzone: m.deadzone,
                        },
                    )
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CompactWireProfile {
    schema_version: u32,
    name: String,
    source: ProfileSource,
    verified: bool,
    family: Option<Family>,
    face: Option<FaceLayout>,
    indices: Indices,
    attribution: Option<String>,
    matches: Vec<CompactWireRule>,
    mappings: Vec<CompactWireMapping>,
}

impl From<&Profile> for CompactWireProfile {
    fn from(profile: &Profile) -> Self {
        Self {
            schema_version: profile.schema_version,
            name: profile.name.clone(),
            source: profile.source,
            verified: profile.verified,
            family: profile.family,
            face: profile.face,
            indices: profile.indices,
            attribution: profile.attribution.clone(),
            matches: profile.matches.iter().map(Into::into).collect(),
            mappings: profile.mappings.iter().map(Into::into).collect(),
        }
    }
}

impl From<CompactWireProfile> for Profile {
    fn from(profile: CompactWireProfile) -> Self {
        Self {
            schema_version: profile.schema_version,
            name: profile.name,
            source: profile.source,
            verified: profile.verified,
            family: profile.family,
            face: profile.face,
            indices: profile.indices,
            attribution: profile.attribution,
            matches: profile.matches.into_iter().map(Into::into).collect(),
            mappings: profile.mappings.into_iter().map(Into::into).collect(),
        }
    }
}

/// Serializes profiles into a compact binary byte slice using [`postcard`].
pub fn encode_profiles(profiles: &[Profile]) -> Result<Vec<u8>, postcard::Error> {
    let wire: Vec<CompactWireProfile> = profiles.iter().map(Into::into).collect();
    postcard::to_allocvec(&wire)
}

/// Deserializes profiles from a compact binary byte slice using [`postcard`].
pub fn decode_profiles(bytes: &[u8]) -> Result<Vec<Profile>, postcard::Error> {
    let wire: Vec<CompactWireProfile> = postcard::from_bytes(bytes)?;
    Ok(wire.into_iter().map(Into::into).collect())
}
