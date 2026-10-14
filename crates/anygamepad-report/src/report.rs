//! The device report and its parts.

use core::fmt;
use core::str::FromStr;
use std::collections::BTreeSet;

use anygamepad_db::Mapping;
use anygamepad_model::hid::HidCollection;
use anygamepad_model::identity::{Browser, DeviceIdentity, Fingerprint, Os};
use anygamepad_model::raw::RawLayout;
use anygamepad_model::{Capabilities, Family, OutputSet};
use serde::{Deserialize, Serialize};

use crate::{FORMAT, MAX_SAMPLES, SCHEMA_VERSION};

/// A device report: what one app saw of one physical controller.
///
/// Build it with [`Report::new`] and [`View::new`], then
/// [`scrub`](Report::scrub) and [`validate`](Report::validate) it before it
/// is stored or shared. See `data/reports/README.md` for the format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// Always [`FORMAT`], so a report file tells what it is.
    #[cfg_attr(feature = "schemars", schemars(schema_with = "format_schema"))]
    pub format: String,
    /// Format version: [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Day the report was made (UTC).
    pub created: Date,
    /// The app and library that made it.
    pub source: Source,
    /// The computer the device was connected to.
    pub host: Host,
    /// What the person says about the device.
    #[serde(default, skip_serializing_if = "UserStatement::is_empty")]
    pub user: UserStatement,
    /// How each provider, or each browser API, sees the device.
    pub views: Vec<View>,
}

#[cfg(feature = "schemars")]
fn format_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "const": FORMAT })
}

impl Report {
    /// A report without views or user statements.
    pub fn new(source: Source, host: Host, created: Date) -> Self {
        Self {
            format: FORMAT.to_owned(),
            schema_version: SCHEMA_VERSION,
            created,
            source,
            host,
            user: UserStatement::default(),
            views: Vec::new(),
        }
    }
}

/// Which app made a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum App {
    /// The web tester (<https://anygamepad.github.io>).
    Web,
    /// The desktop app's collection mode.
    Desktop,
}

/// The app and library that made a report.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Which app.
    pub app: App,
    /// Its version (`0.1.0`).
    pub app_version: Version,
    /// Version of the anygamepad library that read the device.
    pub library_version: Version,
}

/// The computer a device was connected to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// Operating system.
    pub os: Os,
    /// OS release, from the desktop app (`11 23H2`, `14.6.1`, a Linux
    /// kernel release such as `6.8.0-45-generic`). Browsers do not tell it
    /// reliably, so web reports leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// Browser engine and major version, for web reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<Browser>,
}

/// What the person who made the report says about the device: the facts
/// no API tells.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UserStatement {
    /// What the controller is, in their words (`8BitDo Ultimate 2C`), at
    /// most [`MAX_USER_TEXT_CHARS`](crate::MAX_USER_TEXT_CHARS) characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// The mode they say it is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ControllerMode>,
}

impl UserStatement {
    /// Whether nothing is stated.
    pub fn is_empty(&self) -> bool {
        self.product.is_none() && self.mode.is_none()
    }
}

/// A mode a controller can be switched to, as printed on mode switches or
/// told in manuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ControllerMode {
    /// XInput ("X"): an Xbox 360 controller to Windows.
    Xinput,
    /// DirectInput ("D"): a generic HID game controller.
    Dinput,
    /// Nintendo Switch ("S"): a Switch Pro Controller.
    Switch,
    /// PlayStation 3: a DualShock 3.
    Ps3,
    /// PlayStation 4: a DualShock 4.
    Ps4,
    /// PlayStation 5: a DualSense.
    Ps5,
    /// Xbox One or Series controller (GIP), not XInput emulation.
    Xbox,
    /// Android ("A").
    Android,
    /// iOS, iPadOS or macOS (MFi).
    Apple,
    /// Keyboard and mouse emulation.
    KeyboardMouse,
    /// Another mode.
    Other,
}

/// How one provider, or one browser API, sees a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct View {
    /// Identity as this provider sees it.
    pub identity: DeviceIdentity,
    /// [`DeviceIdentity::fingerprint`] of `identity`, to group reports.
    pub fingerprint: Fingerprint,
    /// Family the library recognized from the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    /// Name of the device profile the library picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The raw elements the provider reads: buttons, axes and hats with
    /// their sources and ranges, sensors, touchpads, battery.
    pub layout: RawLayout,
    /// Outputs the provider can drive.
    pub outputs: OutputSet,
    /// Canonical controls the library derives for this view, when it maps
    /// it (a profile, or a browser's standard mapping).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    /// HID data, for views that read HID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hid: Option<HidData>,
    /// The mapping the person confirmed with the guided wizard, in the
    /// profile format, with inputs naming elements of `layout`; `when` is
    /// empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<Mapping>,
}

impl View {
    /// A view of `identity`, with its fingerprint and family.
    pub fn new(identity: DeviceIdentity, layout: RawLayout, outputs: OutputSet) -> Self {
        Self {
            fingerprint: identity.fingerprint(),
            family: Family::from_identity(&identity),
            identity,
            profile: None,
            layout,
            outputs,
            capabilities: None,
            hid: None,
            confirmed: None,
        }
    }
}

/// What a view read over HID.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HidData {
    /// The raw report descriptor, when the backend can read it (native
    /// backends; WebHID gives `collections` only). Its hash is the
    /// identity's `descriptor_hash`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<HexBytes>,
    /// Top-level collections: the structure of the device's reports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<HidCollection>,
    /// The first input report of each report ID and length, in the order
    /// they arrived (see [`HidData::sample`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_samples: Vec<InputSample>,
}

impl HidData {
    /// Keeps `data` as a sample when it is the first input report of its
    /// ID and length and fewer than [`MAX_SAMPLES`] are kept; returns
    /// whether it was kept.
    pub fn sample(&mut self, report_id: u8, data: &[u8]) -> bool {
        let seen: BTreeSet<(u8, usize)> = self
            .input_samples
            .iter()
            .map(|sample| (sample.report_id, sample.data.0.len()))
            .collect();
        if self.input_samples.len() >= MAX_SAMPLES || seen.contains(&(report_id, data.len())) {
            return false;
        }
        self.input_samples.push(InputSample {
            report_id,
            data: HexBytes(data.to_vec()),
        });
        true
    }
}

/// One input report, as the device sent it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputSample {
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Report data, without the report ID.
    pub data: HexBytes,
}

/// Bytes, written in human-readable formats as lowercase hexadecimal
/// digits, two per byte (`"05010905"`).
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct HexBytes(pub Vec<u8>);

impl fmt::Display for HexBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for HexBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HexBytes({self})")
    }
}

impl FromStr for HexBytes {
    type Err = ParseError;

    /// Parses pairs of hexadecimal digits, in either case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseError::new("hexadecimal bytes", s);
        if s.len() % 2 != 0 {
            return Err(error());
        }
        s.as_bytes()
            .chunks(2)
            .map(|pair| {
                let digits = core::str::from_utf8(pair).map_err(|_| error())?;
                if digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                    u8::from_str_radix(digits, 16).map_err(|_| error())
                } else {
                    Err(error())
                }
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

impl Serialize for HexBytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for HexBytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
            s.parse().map_err(serde::de::Error::custom)
        } else {
            <Vec<u8>>::deserialize(deserializer).map(Self)
        }
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for HexBytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "HexBytes".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^([0-9a-fA-F]{2})*$",
        })
    }
}

/// A calendar day, written `YYYY-MM-DD`.
///
/// Reports carry the day they were made, not the time: enough to tell
/// which browser and library versions were current, and less to correlate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// The day, if it exists (years 1970 to 9999).
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year < 1970 || year > 9999 || month < 1 || month > 12 {
            return None;
        }
        if day < 1 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The UTC day of a Unix time in seconds; `None` before 1970 or after
    /// 9999.
    pub fn from_unix_seconds(seconds: i64) -> Option<Self> {
        let days = seconds.div_euclid(86_400);
        // Howard Hinnant's `civil_from_days`, for the proleptic Gregorian
        // calendar.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self::new(
            u16::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }

    /// The year.
    pub const fn year(self) -> u16 {
        self.year
    }

    /// The month, 1 to 12.
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, from 1.
    pub const fn day(self) -> u8 {
        self.day
    }
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = ParseError;

    /// Parses `YYYY-MM-DD`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseError::new("date", s);
        let digits = |part: &str, len: usize| {
            (part.len() == len && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse::<u16>().ok())
                .flatten()
        };
        let mut parts = s.split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(error());
        };
        let year = digits(year, 4).ok_or_else(error)?;
        let month = digits(month, 2).ok_or_else(error)?;
        let day = digits(day, 2).ok_or_else(error)?;
        Self::new(year, month as u8, day as u8).ok_or_else(error)
    }
}

/// A version number: `MAJOR.MINOR.PATCH`, optionally followed by
/// `-<pre-release>` and `+<build>` of letters, digits, dots and hyphens, at
/// most 64 characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(String);

impl Version {
    /// The version text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Version {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseError::new("version", s);
        let number = |part: &str| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        };
        let label = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        };
        if s.len() > 64 {
            return Err(error());
        }
        let (rest, build) = match s.split_once('+') {
            Some((rest, build)) => (rest, Some(build)),
            None => (s, None),
        };
        let (core, pre) = match rest.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (rest, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        let valid = parts.len() == 3
            && parts.iter().all(|part| number(part))
            && pre.is_none_or(label)
            && build.is_none_or(label);
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(error())
        }
    }
}

macro_rules! text_serde {
    ($name:ident, $pattern:literal, $description:literal) => {
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "description": $description,
                    "type": "string",
                    "pattern": $pattern,
                })
            }
        }
    };
}

text_serde!(
    Date,
    "^[0-9]{4}-[0-9]{2}-[0-9]{2}$",
    "A calendar day (UTC), `YYYY-MM-DD`."
);
text_serde!(
    Version,
    "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$",
    "A version number, `MAJOR.MINOR.PATCH` with optional `-pre-release` and `+build` labels."
);

/// Error returned when parsing a [`Date`], a [`Version`] or [`HexBytes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    kind: &'static str,
    text: String,
}

impl ParseError {
    fn new(kind: &'static str, text: &str) -> Self {
        Self {
            kind,
            text: text.chars().take(80).collect(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: `{}`", self.kind, self.text)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let date = Date::new(2026, 10, 14).unwrap();
        assert_eq!(date.to_string(), "2026-10-14");
        assert_eq!("2026-10-14".parse(), Ok(date));
        assert_eq!(Date::new(2028, 2, 29).map(|d| d.day()), Some(29));
        for bad in [
            "2026-10-1",
            "2026-13-01",
            "2026-02-29",
            "2100-02-29",
            "1969-12-31",
            "2026/10/14",
            "2026-10-14T00:00:00Z",
            "+026-10-14",
            "",
        ] {
            assert!(bad.parse::<Date>().is_err(), "{bad}");
        }
        assert!("2000-02-29".parse::<Date>().is_ok());
    }

    #[test]
    fn dates_from_unix_time() {
        assert_eq!(Date::from_unix_seconds(0), Date::new(1970, 1, 1));
        // 2026-10-14T23:59:59Z, and one second later.
        assert_eq!(
            Date::from_unix_seconds(1_792_022_399),
            Date::new(2026, 10, 14)
        );
        assert_eq!(
            Date::from_unix_seconds(1_792_022_400),
            Date::new(2026, 10, 15)
        );
        assert_eq!(Date::from_unix_seconds(951_782_400), Date::new(2000, 2, 29));
        assert_eq!(Date::from_unix_seconds(-1), None);
    }

    #[test]
    fn versions() {
        for good in [
            "0.0.1",
            "1.2.3",
            "10.0.0-beta.1",
            "0.1.0+abc.5",
            "1.0.0-rc-1+x",
        ] {
            assert_eq!(good.parse::<Version>().unwrap().as_str(), good);
        }
        for bad in [
            "1.2", "1.2.3.4", "01.2.3", "1.2.x", "1.2.3-", "1.2.3+", "v1.2.3", "",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
        assert!(
            format!("1.2.3-{}", "a".repeat(60))
                .parse::<Version>()
                .is_err()
        );
    }

    #[test]
    fn hex_bytes() {
        let bytes: HexBytes = "05010905ff".parse().unwrap();
        assert_eq!(bytes.0, [0x05, 0x01, 0x09, 0x05, 0xff]);
        assert_eq!(bytes.to_string(), "05010905ff");
        assert_eq!("0A0b".parse::<HexBytes>().unwrap().0, [0x0a, 0x0b]);
        assert_eq!("".parse::<HexBytes>().unwrap().0, Vec::<u8>::new());
        for bad in ["0", "0g", "+1", "é0"] {
            assert!(bad.parse::<HexBytes>().is_err(), "{bad}");
        }
    }

    #[test]
    fn samples_keep_the_first_of_each_id_and_length() {
        let mut hid = HidData::default();
        assert!(hid.sample(0x01, &[1, 2, 3]));
        assert!(!hid.sample(0x01, &[4, 5, 6]));
        assert!(hid.sample(0x01, &[1; 9]));
        assert!(hid.sample(0x31, &[1, 2, 3]));
        assert_eq!(hid.input_samples.len(), 3);
        assert_eq!(hid.input_samples[0].data.0, [1, 2, 3]);
        for id in 0..32 {
            hid.sample(0x40 + id, &[0]);
        }
        assert_eq!(hid.input_samples.len(), MAX_SAMPLES);
    }
}
