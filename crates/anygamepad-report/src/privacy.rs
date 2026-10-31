//! Personal data in reports: what [`Report::scrub`] removes and
//! [`Report::validate`] rejects.
//!
//! The model keeps serial numbers, Bluetooth addresses and device paths out
//! of identities by construction (`anygamepad_model::identity`), but the
//! texts devices and OSes report can still carry them: some Bluetooth pads
//! put their address in their name, and an OS may show the name a person
//! gave the device. Checks here are heuristics on top of that:
//!
//! - **Hexadecimal identifiers**: 12 or more hexadecimal digits in a row,
//!   possibly split by `:` or `-` (`A1:B2:C3:D4:E5:F6`, `a1b2c3d4e5f6`,
//!   UUIDs, most serial numbers). Vendor and product IDs have four digits.
//! - **Email addresses**.
//! - **Control characters**, and texts longer than
//!   [`MAX_TEXT_CHARS`].
//! - **Input reports that answer identity requests**: on Nintendo pads
//!   (vendor `057e`), subcommand replies (`0x21`) and USB command replies
//!   (`0x81`) can hold the Bluetooth address.
//!
//! A name a person gave their device ("Ana's controller") cannot be told
//! from a product name; the preview before sharing (W6, A3) shows every
//! text of the report.

use core::fmt;
use core::ops::Range;

use serde::Serialize;

use crate::{MAX_TEXT_CHARS, MAX_USER_TEXT_CHARS, Report};

/// What replaces an identifier in a text.
pub const REDACTED: &str = "[redacted]";

/// Hexadecimal digits from which a run counts as an identifier.
const HEX_IDENTIFIER_DIGITS: usize = 12;

/// Input reports that can carry identifiers: vendor ID, report ID.
const IDENTIFYING_INPUT_REPORTS: [(u16, u8); 2] = [
    // Nintendo subcommand replies: the reply to "request device info"
    // (0x02) holds the Bluetooth address.
    (0x057e, 0x21),
    // Nintendo USB command replies: the reply to 0x80 0x01 is the
    // Bluetooth address.
    (0x057e, 0x81),
];

/// Whether an input report of a device with this vendor ID can carry
/// identifiers, so it must not be sampled.
pub fn is_identifying_input_report(vendor_id: Option<u16>, report_id: u8) -> bool {
    vendor_id.is_some_and(|vendor| IDENTIFYING_INPUT_REPORTS.contains(&(vendor, report_id)))
}

/// Something that looked like personal data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Finding {
    /// 12 or more hexadecimal digits in a row: a Bluetooth address, a
    /// serial number, a UUID.
    HexIdentifier,
    /// An email address.
    EmailAddress,
    /// Control characters.
    ControlCharacters,
    /// A text longer than the format allows.
    TooLong,
    /// An input report that can carry identifiers.
    IdentifyingReport,
    /// Text after an OS version's numbers, which test reports do not keep
    /// (a custom Linux kernel can put any text there).
    OsVersionDetail,
    /// Free text a test report does not keep (`user.product`: the
    /// configuration says what the pad is).
    FreeText,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::HexIdentifier => "a hexadecimal identifier (address or serial number)",
            Self::EmailAddress => "an email address",
            Self::ControlCharacters => "control characters",
            Self::TooLong => "a text that is too long",
            Self::IdentifyingReport => "an input report that can carry identifiers",
            Self::OsVersionDetail => "text after the OS version's numbers",
            Self::FreeText => "free text the format does not keep",
        })
    }
}

/// A change [`Report::scrub`] made.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Redaction {
    /// The field, as a path into the report
    /// (`views[0].identity.product_name`).
    pub field: String,
    /// What was found there.
    pub finding: Finding,
}

/// The identifiers in `text`: byte ranges, in order, not overlapping.
pub(crate) fn identifiers(text: &str) -> Vec<(Range<usize>, Finding)> {
    let mut found: Vec<(Range<usize>, Finding)> = hex_identifiers(text)
        .into_iter()
        .map(|range| (range, Finding::HexIdentifier))
        .chain(
            email_addresses(text)
                .into_iter()
                .map(|range| (range, Finding::EmailAddress)),
        )
        .collect();
    found.sort_by_key(|(range, _)| range.start);
    // Merge overlaps (a hexadecimal run inside an email address).
    let mut merged: Vec<(Range<usize>, Finding)> = Vec::with_capacity(found.len());
    for (range, finding) in found {
        match merged.last_mut() {
            Some((last, _)) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push((range, finding)),
        }
    }
    merged
}

/// Runs of hexadecimal digits, joined by single `:` or `-`, with at least
/// [`HEX_IDENTIFIER_DIGITS`] digits.
fn hex_identifiers(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_hexdigit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        let mut digits = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_hexdigit() {
                digits += 1;
                i += 1;
                end = i;
            } else if matches!(bytes[i], b':' | b'-')
                && bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
            {
                i += 1;
            } else {
                break;
            }
        }
        if digits >= HEX_IDENTIFIER_DIGITS {
            found.push(start..end);
        }
    }
    found
}

/// `local@domain.tld` addresses.
fn email_addresses(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let local = |b: u8| b.is_ascii_alphanumeric() || b"._%+-".contains(&b);
    let domain = |b: u8| b.is_ascii_alphanumeric() || b".-".contains(&b);
    let mut found = Vec::new();
    for (at, _) in text.match_indices('@') {
        let start = bytes[..at]
            .iter()
            .rposition(|&b| !local(b))
            .map_or(0, |i| i + 1);
        let end = bytes[at + 1..]
            .iter()
            .position(|&b| !domain(b))
            .map_or(bytes.len(), |i| at + 1 + i);
        let host = text[at + 1..end].trim_end_matches('.');
        let dotted = host
            .rsplit_once('.')
            .is_some_and(|(name, tld)| !name.is_empty() && tld.len() >= 2);
        if start < at && dotted {
            found.push(start..at + 1 + host.len());
        }
    }
    found
}

/// Whether `text` has control characters.
fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// Problems of a text field: what [`Report::validate`] rejects.
pub(crate) fn text_findings(text: &str, max_chars: usize) -> Vec<Finding> {
    let mut findings: Vec<Finding> = identifiers(text).into_iter().map(|(_, f)| f).collect();
    if has_control(text) {
        findings.push(Finding::ControlCharacters);
    }
    if text.chars().count() > max_chars {
        findings.push(Finding::TooLong);
    }
    findings.dedup();
    findings
}

/// Cleans a text field in place: control characters become spaces,
/// identifiers become [`REDACTED`], surrounding spaces go, and the text is
/// cut at `max_chars` characters. Returns what was found.
pub(crate) fn scrub_text(text: &mut String, max_chars: usize) -> Vec<Finding> {
    let mut findings = Vec::new();
    if has_control(text) {
        *text = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        findings.push(Finding::ControlCharacters);
    }
    let found = identifiers(text);
    if !found.is_empty() {
        let mut clean = String::with_capacity(text.len());
        let mut last = 0;
        for (range, finding) in found {
            clean.push_str(&text[last..range.start]);
            clean.push_str(REDACTED);
            last = range.end;
            if !findings.contains(&finding) {
                findings.push(finding);
            }
        }
        clean.push_str(&text[last..]);
        *text = clean;
    }
    let trimmed = text.trim();
    if trimmed.len() != text.len() {
        *text = trimmed.to_owned();
    }
    if text.chars().count() > max_chars {
        *text = text
            .chars()
            .take(max_chars)
            .collect::<String>()
            .trim_end()
            .to_owned();
        findings.push(Finding::TooLong);
    }
    findings
}

impl Report {
    /// Removes what looks like personal data (see the
    /// [module documentation](self)), recomputes the fingerprints of the
    /// views it changed, and returns what it did.
    ///
    /// Run it on every report a client builds, before
    /// [`validate`](Self::validate); servers and importers only validate.
    pub fn scrub(&mut self) -> Vec<Redaction> {
        let mut redactions = Vec::new();
        let mut record = |field: String, findings: Vec<Finding>| {
            redactions.extend(findings.into_iter().map(|finding| Redaction {
                field: field.clone(),
                finding,
            }));
        };
        if let Some(version) = &mut self.host.os_version {
            record(
                "host.os_version".into(),
                scrub_text(version, MAX_TEXT_CHARS),
            );
        }
        if let Some(product) = &mut self.user.product {
            record(
                "user.product".into(),
                scrub_text(product, MAX_USER_TEXT_CHARS),
            );
        }
        if self.user.product.as_deref() == Some("") {
            self.user.product = None;
        }
        for (index, view) in self.views.iter_mut().enumerate() {
            let before = view.identity.clone();
            for (name, text) in identity_texts(&mut view.identity) {
                record(
                    format!("views[{index}].identity.{name}"),
                    scrub_text(text, MAX_TEXT_CHARS),
                );
            }
            if view.identity != before {
                view.fingerprint = view.identity.fingerprint();
            }
            if let Some(profile) = &mut view.profile {
                record(
                    format!("views[{index}].profile"),
                    scrub_text(profile, MAX_TEXT_CHARS),
                );
            }
            let vendor_id = view.identity.vendor_id;
            if let Some(hid) = &mut view.hid {
                // Paths name the samples as they were before removal.
                let mut position = 0;
                hid.input_samples.retain(|sample| {
                    let identifying = is_identifying_input_report(vendor_id, sample.report_id);
                    if identifying {
                        record(
                            format!("views[{index}].hid.input_samples[{position}]"),
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
}

/// The text fields of an identity, by name.
pub(crate) fn identity_texts(
    identity: &mut anygamepad_model::identity::DeviceIdentity,
) -> Vec<(&'static str, &mut String)> {
    let mut texts = Vec::new();
    if let Some(text) = &mut identity.vendor_name {
        texts.push(("vendor_name", text));
    }
    if let Some(text) = &mut identity.product_name {
        texts.push(("product_name", text));
    }
    if let Some(text) = &mut identity.driver {
        texts.push(("driver", text));
    }
    if let Some(web) = &mut identity.web {
        texts.push(("web.id", &mut web.id));
        texts.push(("web.mapping", &mut web.mapping));
    }
    texts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(&str, Finding)> {
        identifiers(text)
            .into_iter()
            .map(|(range, finding)| (&text[range], finding))
            .collect()
    }

    #[test]
    fn finds_hexadecimal_identifiers() {
        use Finding::HexIdentifier as Hex;
        assert_eq!(found("Pad A1:B2:C3:D4:E5:F6"), [("A1:B2:C3:D4:E5:F6", Hex)]);
        assert_eq!(found("Pad a1-b2-c3-d4-e5-f6"), [("a1-b2-c3-d4-e5-f6", Hex)]);
        assert_eq!(found("BT_PAD_a1b2c3d4e5f6!"), [("a1b2c3d4e5f6", Hex)]);
        assert_eq!(
            found("id 1b4e28ba-2fa1-11d2-883f-0016d3cca427"),
            [("1b4e28ba-2fa1-11d2-883f-0016d3cca427", Hex)]
        );
        // Separators join digits only.
        assert_eq!(
            found("a1b2c3:-d4e5f6 a1b2c3::d4e5f6"),
            Vec::<(&str, Finding)>::new()
        );
    }

    #[test]
    fn leaves_ordinary_ids_and_names() {
        for text in [
            "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
            "054c-0ce6-DualSense Wireless Controller",
            "54c-9cc-Wireless Controller",
            "2dc8-6012-8BitDo Ultimate 2C Wireless Controller",
            "Controller (XBOX 360 For Windows)",
            "xinput",
            "standard",
            "hid-playstation",
            "6.8.0-45-generic",
            "10.0.22631",
            "14.6.1",
            "Sony Interactive Entertainment",
            "Pro Controller",
        ] {
            assert_eq!(found(text), [], "{text}");
        }
    }

    #[test]
    fn finds_email_addresses() {
        use Finding::EmailAddress as Email;
        assert_eq!(
            found("pad of ana.b@example.org"),
            [("ana.b@example.org", Email)]
        );
        assert_eq!(found("ana@example.org."), [("ana@example.org", Email)]);
        assert_eq!(found("pad @ 2.4 GHz"), []);
        assert_eq!(found("a@b"), []);
        assert_eq!(found("@example.org"), []);
    }

    #[test]
    fn scrubs_texts() {
        let mut text = "Pad\u{0}A1:B2:C3:D4:E5:F6 ".to_owned();
        let findings = scrub_text(&mut text, MAX_TEXT_CHARS);
        assert_eq!(text, "Pad [redacted]");
        assert_eq!(
            findings,
            [Finding::ControlCharacters, Finding::HexIdentifier]
        );

        let mut text = "é".repeat(10);
        assert_eq!(scrub_text(&mut text, 4), [Finding::TooLong]);
        assert_eq!(text, "éééé");

        let mut text = "DualSense Wireless Controller".to_owned();
        assert_eq!(scrub_text(&mut text, MAX_TEXT_CHARS), []);
        assert_eq!(text, "DualSense Wireless Controller");
    }

    #[test]
    fn text_findings_match_scrubbing() {
        assert_eq!(
            text_findings("x\ty a1b2c3d4e5f6", MAX_TEXT_CHARS),
            [Finding::HexIdentifier, Finding::ControlCharacters]
        );
        assert_eq!(text_findings("abcde", 4), [Finding::TooLong]);
        assert_eq!(text_findings("Pro Controller", MAX_TEXT_CHARS), []);
    }

    #[test]
    fn identifying_reports() {
        assert!(is_identifying_input_report(Some(0x057e), 0x21));
        assert!(is_identifying_input_report(Some(0x057e), 0x81));
        assert!(!is_identifying_input_report(Some(0x057e), 0x30));
        assert!(!is_identifying_input_report(Some(0x054c), 0x21));
        assert!(!is_identifying_input_report(None, 0x21));
    }
}
