//! The raw element a mapping reads, written as a short string in profiles.

use core::fmt;
use core::str::FromStr;

use anygamepad_model::raw::RawSource;

/// Which part of a raw axis an [`Input`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AxisRange {
    /// The whole axis, from its minimum to its maximum.
    Full,
    /// From the centre to the maximum.
    Positive,
    /// From the centre to the minimum.
    Negative,
}

/// A raw element read by a mapping: a button, an axis or half of one, or
/// directions of a hat switch.
///
/// Every input reads as a position `t` in `[0, 1]` along its range:
///
/// | Text | Reads | `t` |
/// |------|-------|-----|
/// | `b<ref>` | a button | its value: 0 released, 1 pressed (analog buttons in between) |
/// | `a<ref>` | an axis | 0 at its minimum, 1 at its maximum |
/// | `+a<ref>`, `-a<ref>` | half an axis | 0 at the centre, 1 at the maximum (`+`) or minimum (`-`); the input is inactive while the axis is on the other half |
/// | `a<ref>~`, `+a<ref>~`, ... | the same range reversed | `1 - t` |
/// | `h<ref>.<mask>` | a hat switch | 1 while it points towards any direction in `mask` (1 up, 2 right, 4 down, 8 left), else 0 |
///
/// `<ref>` is a decimal index ([`RawSource::Index`]) or another
/// [`RawSource`] in braces: `b{evdev:0x0130}`, `a{hid:0x0001:0x0030}~`,
/// `h{hid:0x0001:0x0039}.1`. The syntax follows SDL GameControllerDB, whose
/// entries use indices only.
///
/// How a target uses `t` is described in [`Mapping`](crate::Mapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    /// A button.
    Button(RawSource),
    /// An axis, or part of one.
    Axis {
        /// The axis.
        source: RawSource,
        /// Which part of the axis is read.
        range: AxisRange,
        /// Whether the range is reversed (`~`).
        inverted: bool,
    },
    /// Directions of a hat switch.
    Hat {
        /// The hat.
        source: RawSource,
        /// Directions (bits of [`Hat`](anygamepad_model::raw::Hat)), 1 to
        /// 15.
        mask: u8,
    },
}

impl Input {
    /// The raw element this input reads.
    pub const fn source(&self) -> RawSource {
        match *self {
            Self::Button(source) | Self::Axis { source, .. } | Self::Hat { source, .. } => source,
        }
    }

    /// This input with its range reversed: `~` toggled for axes. Buttons and
    /// hats cannot be reversed and come back unchanged.
    pub const fn inverted(self) -> Self {
        match self {
            Self::Axis {
                source,
                range,
                inverted,
            } => Self::Axis {
                source,
                range,
                inverted: !inverted,
            },
            other => other,
        }
    }
}

/// Error returned when an [`Input`] string is malformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseInputError(String);

impl fmt::Display for ParseInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid input `{}`: expected `b<ref>`, `a<ref>`, `+a<ref>`, `-a<ref>` \
             (optionally ending in `~`) or `h<ref>.<mask 1-15>`, where <ref> is a \
             decimal index or a raw source in braces",
            self.0
        )
    }
}

impl std::error::Error for ParseInputError {}

/// Splits a leading `<ref>` from `s`.
fn split_ref(s: &str) -> Option<(RawSource, &str)> {
    if let Some(rest) = s.strip_prefix('{') {
        let (inner, rest) = rest.split_once('}')?;
        // Indices are written bare.
        match inner.parse().ok()? {
            RawSource::Index(_) => None,
            source => Some((source, rest)),
        }
    } else {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        Some((s[..end].parse().ok()?, &s[end..]))
    }
}

impl FromStr for Input {
    type Err = ParseInputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseInputError(s.to_owned());
        let (range, rest) = match s.as_bytes().first() {
            Some(b'+') => (Some(AxisRange::Positive), &s[1..]),
            Some(b'-') => (Some(AxisRange::Negative), &s[1..]),
            _ => (None, s),
        };
        let kind = rest.as_bytes().first().copied().ok_or_else(error)?;
        let (source, rest) = split_ref(&rest[1..]).ok_or_else(error)?;
        match (kind, range) {
            (b'b', None) if rest.is_empty() => Ok(Self::Button(source)),
            (b'a', range) => {
                let (inverted, rest) = match rest.strip_suffix('~') {
                    Some(rest) => (true, rest),
                    None => (false, rest),
                };
                if !rest.is_empty() {
                    return Err(error());
                }
                Ok(Self::Axis {
                    source,
                    range: range.unwrap_or(AxisRange::Full),
                    inverted,
                })
            }
            (b'h', None) => {
                let mask = rest.strip_prefix('.').ok_or_else(error)?;
                // Decimal without leading zeros.
                if mask.starts_with('0') {
                    return Err(error());
                }
                match mask.parse() {
                    Ok(mask @ 1..=15) => Ok(Self::Hat { source, mask }),
                    _ => Err(error()),
                }
            }
            _ => Err(error()),
        }
    }
}

/// Writes a `<ref>`.
struct Ref(RawSource);

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            RawSource::Index(index) => write!(f, "{index}"),
            source => write!(f, "{{{source}}}"),
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Button(source) => write!(f, "b{}", Ref(source)),
            Self::Axis {
                source,
                range,
                inverted,
            } => {
                let sign = match range {
                    AxisRange::Full => "",
                    AxisRange::Positive => "+",
                    AxisRange::Negative => "-",
                };
                let tilde = if inverted { "~" } else { "" };
                write!(f, "{sign}a{}{tilde}", Ref(source))
            }
            Self::Hat { source, mask } => write!(f, "h{}.{mask}", Ref(source)),
        }
    }
}

impl serde::Serialize for Input {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Input {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// `<ref>` in the JSON Schema: an index or a raw source in braces.
#[cfg(feature = "schemars")]
const REF_PATTERN: &str = concat!(
    "(0|[1-9][0-9]*",
    "|\\{evdev:0x[0-9a-fA-F]{1,4}\\}",
    "|\\{hid:0x[0-9a-fA-F]{1,4}:0x[0-9a-fA-F]{1,4}\\}",
    "|\\{button:[a-z0-9_]+\\}",
    "|\\{axis:[a-z_]+\\})",
);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Input {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Input".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let pattern =
            format!("^(b{REF_PATTERN}|[+-]?a{REF_PATTERN}~?|h{REF_PATTERN}\\.([1-9]|1[0-5]))$");
        schemars::json_schema!({
            "description": "A raw element: `b<ref>` (button), `a<ref>` (axis), \
                `+a<ref>`/`-a<ref>` (half axis), optionally ending in `~` (reversed), \
                or `h<ref>.<mask>` (hat directions: 1 up, 2 right, 4 down, 8 left). \
                <ref> is a decimal index or a raw source in braces: \
                `{evdev:0x0130}`, `{hid:0x0001:0x0030}`, `{button:south}`, \
                `{axis:left_stick_x}`.",
            "type": "string",
            "pattern": pattern,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anygamepad_model::Button;

    #[test]
    fn parse_and_display() {
        let index = RawSource::Index;
        for (text, input) in [
            ("b0", Input::Button(index(0))),
            ("b12", Input::Button(index(12))),
            (
                "a1",
                Input::Axis {
                    source: index(1),
                    range: AxisRange::Full,
                    inverted: false,
                },
            ),
            (
                "-a1~",
                Input::Axis {
                    source: index(1),
                    range: AxisRange::Negative,
                    inverted: true,
                },
            ),
            (
                "+a{evdev:0x0010}",
                Input::Axis {
                    source: RawSource::Evdev(0x10),
                    range: AxisRange::Positive,
                    inverted: false,
                },
            ),
            (
                "h0.4",
                Input::Hat {
                    source: index(0),
                    mask: 4,
                },
            ),
            (
                "h{hid:0x0001:0x0039}.12",
                Input::Hat {
                    source: RawSource::Hid {
                        page: 1,
                        usage: 0x39,
                    },
                    mask: 12,
                },
            ),
            (
                "b{button:south}",
                Input::Button(RawSource::Button(Button::South)),
            ),
        ] {
            assert_eq!(text.parse::<Input>(), Ok(input), "{text}");
            assert_eq!(input.to_string(), text);
        }
        assert_eq!(
            "b{evdev:0x130}".parse::<Input>().unwrap().to_string(),
            "b{evdev:0x0130}"
        );
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "b",
            "a",
            "x0",
            "b-1",
            "b01",
            "+b0",
            "b0~",
            "a0~~",
            "~a0",
            "a0.1",
            "h0",
            "h0.",
            "h0.0",
            "h0.16",
            "h0.04",
            "+h0.1",
            "b{0}",
            "b{evdev:0x130",
            "b{evdev:130}",
            "b0 ",
            " b0",
            "B0",
        ] {
            assert!(bad.parse::<Input>().is_err(), "{bad}");
        }
    }

    #[test]
    fn inverted_toggles_axes_only() {
        let axis: Input = "+a2".parse().unwrap();
        assert_eq!(axis.inverted().to_string(), "+a2~");
        assert_eq!(axis.inverted().inverted(), axis);
        let button: Input = "b3".parse().unwrap();
        assert_eq!(button.inverted(), button);
    }
}
