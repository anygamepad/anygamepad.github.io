//! SDL GameControllerDB importer (D2): turns the entries of
//! [SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB)
//! (`gamecontrollerdb.txt`) into [`Profile`]s.
//!
//! An entry is one line: `<GUID>,<name>,<target>:<input>,...,platform:<OS>,`.
//! Each one becomes a profile with `source = "sdl-gamecontrollerdb"`,
//! `verified = false`, the given attribution, and `indices = "sdl"`: its
//! inputs count SDL's joystick indices on the SDL backend the GUID names,
//! not HID usages or evdev codes (`docs/research/02-sdl-gamecontrollerdb.md`).
//!
//! Targets are positional, like the canonical model (`a` is south on every
//! pad). SDL's stick Y axes grow downwards, so Y inputs are reversed, and
//! `misc1` becomes the button it is on the vendor's pads (Sony mute,
//! Nintendo capture, Microsoft share).

use std::collections::btree_map::Entry;
use std::fmt;

use anygamepad_model::identity::{Backend, Os};
use anygamepad_model::sdl::SdlGuid;
use anygamepad_model::{Axis, Button, Family};

use crate::{
    AxisRange, Indices, Input, Mapping, MatchRule, Profile, ProfileSource, SCHEMA_VERSION,
};

/// Attribution of the copy in `data/sdl-gamecontrollerdb/`, for its
/// profiles.
pub const VENDORED_ATTRIBUTION: &str = "SDL_GameControllerDB \
    (https://github.com/mdqinc/SDL_GameControllerDB) at commit c1d5289, \
    zlib license, Copyright (C) 1997-2025 Sam Lantinga; converted by anygamepad";

const SONY: u16 = 0x054c;
const NINTENDO: u16 = 0x057e;
const MICROSOFT: u16 = 0x045e;

/// Error returned for an entry that cannot be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError(String);

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

/// Imports every entry of a `gamecontrollerdb.txt` file, skipping comments
/// and blank lines. Yields the 1-based line number with each result.
pub fn import<'a>(
    text: &'a str,
    attribution: &'a str,
) -> impl Iterator<Item = (usize, Result<Profile, ImportError>)> + 'a {
    text.lines().enumerate().filter_map(move |(index, line)| {
        import_line(line, attribution)
            .transpose()
            .map(|result| (index + 1, result))
    })
}

/// Imports one line: `Ok(None)` for comments and blank lines.
pub fn import_line(line: &str, attribution: &str) -> Result<Option<Profile>, ImportError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let error = |message: String| ImportError(message);
    let mut fields = line.split(',');
    let guid = fields.next().unwrap_or_default();
    let name = fields
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| error("no name".into()))?;

    let mut os = None;
    let mut pairs = Vec::new();
    for field in fields.filter(|field| !field.is_empty()) {
        let (key, value) = field
            .split_once(':')
            .ok_or_else(|| error(format!("`{field}` is not `<target>:<input>`")))?;
        if key == "platform" {
            os = Some(platform(value).ok_or_else(|| error(format!("unknown platform `{value}`")))?);
        } else {
            pairs.push((key, value));
        }
    }

    let (rule, vendor_id, family) = if guid == "xinput" {
        // SDL's mapping for XInput devices, which have no GUID of their own.
        let rule = MatchRule {
            os,
            backend: Some(Backend::XInput),
            ..MatchRule::default()
        };
        (rule, Some(MICROSOFT), Some(Family::Xbox))
    } else {
        let guid: SdlGuid = guid
            .parse()
            .map_err(|_| error(format!("invalid GUID `{guid}`")))?;
        let ids = guid.ids().filter(|ids| ids.vendor_id != 0);
        let rule = MatchRule {
            vendor_id: ids.map(|ids| ids.vendor_id),
            product_id: ids.map(|ids| ids.product_id),
            version: ids.map(|ids| ids.version).filter(|&version| version != 0),
            os,
            sdl_guid: Some(guid),
            ..MatchRule::default()
        };
        let family = ids.and_then(|ids| Family::from_ids(ids.vendor_id, ids.product_id));
        (rule, ids.map(|ids| ids.vendor_id), family)
    };

    let mut mapping = Mapping::default();
    for (key, value) in pairs {
        bind(&mut mapping, key, value, vendor_id)
            .map_err(|message| error(format!("`{key}:{value}`: {message}")))?;
    }

    let profile = Profile {
        schema_version: SCHEMA_VERSION,
        name: name.to_owned(),
        source: ProfileSource::SdlGameControllerDb,
        verified: false,
        family,
        face: None,
        indices: Indices::Sdl,
        attribution: Some(attribution.to_owned()),
        matches: vec![rule],
        mappings: vec![mapping],
    };
    profile
        .validate()
        .map_err(|invalid| error(invalid.message().to_owned()))?;
    Ok(Some(profile))
}

/// The OS of a `platform:` field.
fn platform(name: &str) -> Option<Os> {
    Some(match name {
        "Windows" => Os::Windows,
        "Mac OS X" => Os::MacOs,
        "Linux" => Os::Linux,
        "Android" => Os::Android,
        "iOS" => Os::Ios,
        _ => return None,
    })
}

/// The canonical button of an SDL button target.
fn button(target: &str, vendor_id: Option<u16>) -> Option<Button> {
    Some(match target {
        "a" => Button::South,
        "b" => Button::East,
        "x" => Button::West,
        "y" => Button::North,
        "back" => Button::Select,
        "start" => Button::Start,
        "guide" => Button::Home,
        "leftshoulder" => Button::LeftShoulder,
        "rightshoulder" => Button::RightShoulder,
        "leftstick" => Button::LeftStick,
        "rightstick" => Button::RightStick,
        "dpup" => Button::DPadUp,
        "dpdown" => Button::DPadDown,
        "dpleft" => Button::DPadLeft,
        "dpright" => Button::DPadRight,
        "touchpad" => Button::TouchpadClick,
        // SDL3: paddle1 and paddle3 are the upper and lower right paddles
        // (Xbox Elite P1, P2), paddle2 and paddle4 the left ones (P3, P4).
        "paddle1" => Button::RightPaddle1,
        "paddle2" => Button::LeftPaddle1,
        "paddle3" => Button::RightPaddle2,
        "paddle4" => Button::LeftPaddle2,
        "misc1" => match vendor_id {
            Some(SONY) => Button::Mute,
            Some(NINTENDO) => Button::Capture,
            Some(MICROSOFT) => Button::Share,
            _ => Button::extra(0)?,
        },
        _ => {
            let n: u8 = target.strip_prefix("misc")?.parse().ok()?;
            if !(2..=6).contains(&n) {
                return None;
            }
            Button::extra(n - 1)?
        }
    })
}

/// The canonical axis of an SDL axis target.
fn axis(target: &str) -> Option<Axis> {
    Some(match target {
        "leftx" => Axis::LeftStickX,
        "lefty" => Axis::LeftStickY,
        "rightx" => Axis::RightStickX,
        "righty" => Axis::RightStickY,
        "lefttrigger" => Axis::LeftTrigger,
        "righttrigger" => Axis::RightTrigger,
        _ => return None,
    })
}

/// Adds the binding `key:value` (`dpup:h0.1`, `-leftx:b4`, ...).
fn bind(
    mapping: &mut Mapping,
    key: &str,
    value: &str,
    vendor_id: Option<u16>,
) -> Result<(), String> {
    let (half, target) = match key.as_bytes().first() {
        Some(b'+') => (Some(AxisRange::Positive), &key[1..]),
        Some(b'-') => (Some(AxisRange::Negative), &key[1..]),
        _ => (None, key),
    };
    let (button, axis) = match (button(target, vendor_id), axis(target)) {
        (None, None) => return Err("unknown target".into()),
        targets => targets,
    };
    let input: Input = value.parse().map_err(|_| "invalid input".to_owned())?;

    if let Some(button) = button {
        if half.is_some() {
            return Err("a button has no halves".into());
        }
        return match mapping.buttons.entry(button) {
            Entry::Vacant(entry) => {
                entry.insert(input);
                Ok(())
            }
            Entry::Occupied(_) => Err(format!("`{button}` is bound twice")),
        };
    }

    let Some(axis) = axis else {
        return Err("unknown target".into());
    };
    let stick = !matches!(axis, Axis::LeftTrigger | Axis::RightTrigger);
    let flip_y = matches!(axis, Axis::LeftStickY | Axis::RightStickY);
    // SDL drives a whole stick axis from a button or hat to its maximum
    // while pressed and back to 0 on release: the positive half.
    let half = match half {
        None if stick && !matches!(input, Input::Axis { .. }) => Some(AxisRange::Positive),
        half => half,
    };
    let entry = mapping.axes.entry(axis).or_default();
    let slot = match half {
        None => {
            if flip_y {
                // Reversing the range negates the stick value.
                entry.input.replace(input.inverted())
            } else {
                entry.input.replace(input)
            }
        }
        // Down is SDL's positive half and the canonical negative one.
        Some(AxisRange::Positive) if flip_y => entry.negative.replace(input),
        Some(AxisRange::Negative) if flip_y => entry.positive.replace(input),
        Some(AxisRange::Positive) => entry.positive.replace(input),
        Some(AxisRange::Negative) | Some(AxisRange::Full) => entry.negative.replace(input),
    };
    match slot {
        None => Ok(()),
        Some(_) => Err(format!("`{axis}` is bound twice")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisMapping;

    fn import_one(line: &str) -> Profile {
        import_line(line, "test").unwrap().unwrap()
    }

    fn input(s: &str) -> Input {
        s.parse().unwrap()
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        assert_eq!(import_line("# Windows", "test"), Ok(None));
        assert_eq!(import_line("  ", "test"), Ok(None));
    }

    #[test]
    fn converts_an_entry() {
        let profile = import_one(
            "030000004c050000c405000000010000,PS4 Controller,a:b1,b:b2,back:b8,\
             dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b12,leftshoulder:b4,\
             leftstick:b10,lefttrigger:a3,leftx:a0,lefty:a1,rightshoulder:b5,\
             rightstick:b11,righttrigger:a4,rightx:a2,righty:a5,start:b9,touchpad:b13,\
             x:b0,y:b3,platform:Mac OS X,",
        );
        assert_eq!(profile.name, "PS4 Controller");
        assert_eq!(profile.source, ProfileSource::SdlGameControllerDb);
        assert!(!profile.verified);
        assert_eq!(profile.indices, Indices::Sdl);
        assert_eq!(profile.family, Some(Family::DualShock4));
        assert_eq!(profile.attribution.as_deref(), Some("test"));
        let rule = &profile.matches[0];
        assert_eq!(rule.vendor_id, Some(0x054c));
        assert_eq!(rule.product_id, Some(0x05c4));
        assert_eq!(rule.version, Some(0x0100));
        assert_eq!(rule.os, Some(Os::MacOs));
        assert!(rule.sdl_guid.is_some());

        let mapping = &profile.mappings[0];
        assert_eq!(mapping.buttons[&Button::South], input("b1"));
        assert_eq!(mapping.buttons[&Button::West], input("b0"));
        assert_eq!(mapping.buttons[&Button::Home], input("b12"));
        assert_eq!(mapping.buttons[&Button::DPadDown], input("h0.4"));
        assert_eq!(mapping.buttons[&Button::TouchpadClick], input("b13"));
        assert_eq!(
            mapping.axes[&Axis::LeftStickX],
            AxisMapping::full(input("a0"))
        );
        assert_eq!(
            mapping.axes[&Axis::LeftStickY],
            AxisMapping::full(input("a1~"))
        );
        assert_eq!(
            mapping.axes[&Axis::RightStickY],
            AxisMapping::full(input("a5~"))
        );
        assert_eq!(
            mapping.axes[&Axis::LeftTrigger],
            AxisMapping::full(input("a3"))
        );
    }

    #[test]
    fn half_targets_and_inverted_inputs() {
        let profile = import_one(
            "03000000000000000000000000000000,Pad,a:b0,-leftx:h0.8,+leftx:h0.2,\
             -lefty:h0.1,+lefty:h0.4,righty:a3~,dpup:-a1,righttrigger:+a5,\
             rightx:b7,platform:Linux,",
        );
        let axes = &profile.mappings[0].axes;
        let halves = |negative: &str, positive: &str| AxisMapping {
            negative: Some(input(negative)),
            positive: Some(input(positive)),
            ..AxisMapping::default()
        };
        assert_eq!(axes[&Axis::LeftStickX], halves("h0.8", "h0.2"));
        // SDL's -lefty is up, the canonical positive half.
        assert_eq!(axes[&Axis::LeftStickY], halves("h0.4", "h0.1"));
        assert_eq!(axes[&Axis::RightStickY], AxisMapping::full(input("a3")));
        assert_eq!(axes[&Axis::RightTrigger], AxisMapping::full(input("+a5")));
        // A button on a whole stick axis pushes it to the maximum.
        assert_eq!(
            axes[&Axis::RightStickX],
            AxisMapping {
                positive: Some(input("b7")),
                ..AxisMapping::default()
            }
        );
        assert_eq!(profile.mappings[0].buttons[&Button::DPadUp], input("-a1"));
        // No vendor ID: the GUID alone.
        let rule = &profile.matches[0];
        assert_eq!((rule.vendor_id, rule.product_id), (None, None));
        assert_eq!(rule.os, Some(Os::Linux));
    }

    #[test]
    fn misc1_depends_on_the_vendor() {
        let misc = |guid: &str| {
            let profile = import_one(&format!("{guid},Pad,a:b0,misc1:b15,misc2:b16,"));
            let buttons = &profile.mappings[0].buttons;
            assert_eq!(buttons[&Button::extra(1).unwrap()], input("b16"));
            buttons
                .iter()
                .find(|(_, input)| **input == self::input("b15"))
                .map(|(button, _)| *button)
                .unwrap()
        };
        assert_eq!(misc("030000004c050000e60c000000000000"), Button::Mute);
        assert_eq!(misc("030000007e0500000920000000000000"), Button::Capture);
        assert_eq!(misc("030000005e040000120b000000000000"), Button::Share);
        assert_eq!(
            misc("03000000c82d00000090000000000000"),
            Button::extra(0).unwrap()
        );
    }

    #[test]
    fn xinput_entry() {
        let profile =
            import_one("xinput,XInput Controller,a:b0,b:b1,leftx:a0,lefty:a1,platform:Windows,");
        let rule = &profile.matches[0];
        assert_eq!(rule.backend, Some(Backend::XInput));
        assert_eq!(rule.os, Some(Os::Windows));
        assert_eq!(rule.sdl_guid, None);
        assert_eq!(profile.family, Some(Family::Xbox));
    }

    #[test]
    fn rejects_malformed_entries() {
        for (line, message) in [
            ("0300,Pad,a:b0,", "invalid GUID `0300`"),
            ("03000000000000000000000000000000,,a:b0,", "no name"),
            (
                "03000000000000000000000000000000,Pad,a,",
                "`a` is not `<target>:<input>`",
            ),
            (
                "03000000000000000000000000000000,Pad,a:b0,platform:BeOS,",
                "unknown platform `BeOS`",
            ),
            (
                "03000000000000000000000000000000,Pad,crc:1234,",
                "`crc:1234`: unknown target",
            ),
            (
                "03000000000000000000000000000000,Pad,a:x0,",
                "`a:x0`: invalid input",
            ),
            (
                "03000000000000000000000000000000,Pad,+a:b0,",
                "`+a:b0`: a button has no halves",
            ),
            (
                "03000000000000000000000000000000,Pad,a:b0,a:b1,",
                "`a:b1`: `south` is bound twice",
            ),
            (
                "03000000000000000000000000000000,Pad,leftx:a0,leftx:a1,",
                "`leftx:a1`: `left_stick_x` is bound twice",
            ),
            (
                "03000000000000000000000000000000,Pad,leftx:a0,+leftx:b1,",
                "mapping entry 1: axis `left_stick_x`: set either `input` or `negative`/`positive`",
            ),
            (
                "03000000000000000000000000000000,Pad,-lefttrigger:a2,",
                "mapping entry 1: axis `left_trigger`: triggers have no negative half",
            ),
        ] {
            assert_eq!(
                import_line(line, "test").unwrap_err().to_string(),
                message,
                "{line}"
            );
        }
    }
}
