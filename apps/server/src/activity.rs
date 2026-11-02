//! Activity logs: what the web app detects while it runs (`apps/web/src/
//! activity.ts`), sent in parts once the person turns sending on
//! (`apps/web/src/sync.ts`).
//!
//! A part is one JSON document: the page, the environment, a random ID of
//! the page load joining its parts, the part's number, the day, and the
//! events since the previous part. Events are objects with a time (`t`,
//! milliseconds since the page loaded) and a `type`; their other fields
//! depend on the type, so they are checked by shape:
//!
//! - names (`type` and every key) are lowercase words joined by `_`;
//! - nesting, keys, items and texts are bounded;
//! - every text is scrubbed like a report's
//!   ([`scrub_text`](anygamepad_report::privacy::scrub_text)): identifiers
//!   (12 or more hexadecimal digits, email addresses) become
//!   `[redacted]`, control characters spaces, long texts are cut. Two keys
//!   hold hexadecimal on purpose and keep it when it has their shape:
//!   `data` (a HID report's bytes) and `fingerprint` (16 digits).
//!
//! The server stores the events re-serialized, after the scrub.

use anygamepad_model::identity::{Browser, Os};
use anygamepad_report::privacy::scrub_text;
use anygamepad_report::{Date, Version};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The value of every part's `format`.
pub const LOG_FORMAT: &str = "anygamepad-activity-log-v1";

/// Bytes of a part as sent.
pub const MAX_PART_BYTES: usize = 1024 * 1024;

/// Events in a part (the web app sends 2,000 at most).
pub const MAX_PART_EVENTS: usize = 5_000;

/// Nesting levels inside an event (a HID device's collections nest).
const MAX_DEPTH: usize = 16;

/// Keys of an object, items of an array.
const MAX_ENTRIES: usize = 1024;

/// Characters of a text (names, ids, messages).
const MAX_TEXT_CHARS: usize = 1000;

/// Hexadecimal digits of a `data` field: a 1,024-byte HID report.
const MAX_DATA_DIGITS: usize = 2048;

/// Characters of a name (`type`, keys).
const MAX_NAME_CHARS: usize = 40;

/// The page that sent the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    /// The tester, `/`.
    Tester,
    /// The test mode, `/test/`.
    Test,
}

impl Page {
    /// Its name, as sent.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tester => "tester",
            Self::Test => "test",
        }
    }
}

/// The browser and OS the page runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// Browser engine and major version.
    pub browser: Browser,
    /// Operating system.
    pub os: Os,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPart {
    format: String,
    library: Version,
    page: Page,
    environment: Option<Environment>,
    session: String,
    part: u32,
    day: Date,
    dropped_input_events: u64,
    events: Vec<Value>,
}

/// One part of an activity log, checked and scrubbed.
#[derive(Clone, Debug, PartialEq)]
pub struct LogPart {
    /// The library version of the page.
    pub library: Version,
    /// The page.
    pub page: Page,
    /// The browser and OS, when the page knows them.
    pub environment: Option<Environment>,
    /// The page load's ID: 32 lowercase hexadecimal digits.
    pub session: String,
    /// The part's number, from 0.
    pub part: u32,
    /// The day the page sent it, as the page says.
    pub day: Date,
    /// Input events the page dropped before sending them.
    pub dropped_input_events: u64,
    /// The events, as a compact JSON array.
    pub events: String,
    /// How many events there are.
    pub event_count: usize,
    /// Texts the scrub changed.
    pub redactions: usize,
}

/// Why a part was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidPart(pub String);

impl core::fmt::Display for InvalidPart {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

fn invalid(message: impl Into<String>) -> InvalidPart {
    InvalidPart(message.into())
}

/// Whether `text` is a name: a lowercase letter, then lowercase letters,
/// digits and `_`.
fn is_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && text.len() <= MAX_NAME_CHARS
}

fn is_hex(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `text` is a page load's ID.
pub fn is_session_id(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl LogPart {
    /// Reads, checks and scrubs a part sent as JSON.
    pub fn from_json(text: &str) -> Result<Self, InvalidPart> {
        if text.len() > MAX_PART_BYTES {
            return Err(invalid(format!(
                "a part holds {MAX_PART_BYTES} bytes at most"
            )));
        }
        let raw: RawPart = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
        if raw.format != LOG_FORMAT {
            return Err(invalid(format!("format must be {LOG_FORMAT}")));
        }
        if !is_session_id(&raw.session) {
            return Err(invalid("session must be 32 lowercase hexadecimal digits"));
        }
        if raw.events.is_empty() || raw.events.len() > MAX_PART_EVENTS {
            return Err(invalid(format!(
                "a part holds 1 to {MAX_PART_EVENTS} events"
            )));
        }
        let mut redactions = 0;
        let mut events = raw.events;
        for (index, event) in events.iter_mut().enumerate() {
            check_event(event, &mut redactions)
                .map_err(|e| invalid(format!("events[{index}]: {e}")))?;
        }
        let event_count = events.len();
        Ok(Self {
            library: raw.library,
            page: raw.page,
            environment: raw.environment,
            session: raw.session,
            part: raw.part,
            day: raw.day,
            dropped_input_events: raw.dropped_input_events,
            events: Value::Array(events).to_string(),
            event_count,
            redactions,
        })
    }
}

fn check_event(event: &mut Value, redactions: &mut usize) -> Result<(), String> {
    let Value::Object(fields) = event else {
        return Err("an event is an object".into());
    };
    match fields.get("t") {
        Some(Value::Number(t)) if t.as_f64().is_some_and(|t| (0.0..1e10).contains(&t)) => {}
        _ => return Err("t must be the milliseconds since the page loaded".into()),
    }
    match fields.get("type") {
        Some(Value::String(kind)) if is_name(kind) => {}
        _ => return Err("type must be a name".into()),
    }
    check_object(fields, 1, redactions)
}

fn check_object(
    fields: &mut Map<String, Value>,
    depth: usize,
    redactions: &mut usize,
) -> Result<(), String> {
    if fields.len() > MAX_ENTRIES {
        return Err(format!("an object holds {MAX_ENTRIES} keys at most"));
    }
    if let Some(key) = fields.keys().find(|key| !is_name(key)) {
        return Err(format!("{key:?} is not a name"));
    }
    for (key, value) in fields.iter_mut() {
        check_value(value, Some(key), depth, redactions).map_err(|e| format!("{key}: {e}"))?;
    }
    Ok(())
}

fn check_value(
    value: &mut Value,
    key: Option<&str>,
    depth: usize,
    redactions: &mut usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{MAX_DEPTH} nesting levels at most"));
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(text) => {
            let kept = match key {
                Some("data") => {
                    is_hex(text) && text.len() % 2 == 0 && text.len() <= MAX_DATA_DIGITS
                }
                Some("fingerprint") => text.len() == 16 && is_hex(text),
                _ => false,
            };
            if !kept && !scrub_text(text, MAX_TEXT_CHARS).is_empty() {
                *redactions += 1;
            }
            Ok(())
        }
        Value::Array(items) => {
            if items.len() > MAX_ENTRIES {
                return Err(format!("an array holds {MAX_ENTRIES} items at most"));
            }
            for item in items.iter_mut() {
                // Items of an array keep their key's meaning (`data` lists).
                check_value(item, key, depth + 1, redactions)?;
            }
            Ok(())
        }
        Value::Object(fields) => check_object(fields, depth + 1, redactions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn part(events: Value) -> Value {
        json!({
            "format": LOG_FORMAT,
            "library": "0.0.5",
            "page": "test",
            "environment": { "browser": { "engine": "blink", "major_version": 141 }, "os": "linux" },
            "session": "0123456789abcdef0123456789abcdef",
            "part": 0,
            "day": "2026-11-02",
            "dropped_input_events": 0,
            "events": events,
        })
    }

    #[test]
    fn reads_a_part() {
        let sent = part(json!([
            { "t": 12.5, "type": "session", "library": "0.0.5", "page": "test", "webhid": true, "hint": null },
            { "t": 40, "type": "gamepad_added", "pad": "gamepad:0", "fingerprint": "0123456789abcdef",
              "identity": { "web": { "id": "Xbox 360 Controller (XInput STANDARD GAMEPAD)" } } },
            { "t": 41, "type": "hid_report_sample", "pad": "webhid:1", "report_id": 49, "data": "a1b2c3d4e5f60718293a" },
            { "t": 50, "type": "canonical_buttons", "pad": "gamepad:0", "pressed": ["south", "start"] },
        ]));
        let read = LogPart::from_json(&sent.to_string()).unwrap();
        assert_eq!(read.page, Page::Test);
        assert_eq!(read.session, "0123456789abcdef0123456789abcdef");
        assert_eq!(read.event_count, 4);
        assert_eq!(read.redactions, 0);
        // Nothing to scrub: the events are stored as sent.
        let events: Value = serde_json::from_str(&read.events).unwrap();
        assert_eq!(events, sent["events"]);
        assert_eq!(read.environment.unwrap().os, Os::Linux);
    }

    #[test]
    fn scrubs_identifiers() {
        let text = part(json!([
            { "t": 1, "type": "hid_added", "name": "Pro Controller A1:B2:C3:D4:E5:F6",
              "nested": [{ "who": "ana@example.org" }], "fingerprint": "not a fingerprint 0123456789ab" },
            { "t": 2, "type": "error", "message": "line\nbreak" },
            // `data` keeps hexadecimal only where it is a report's bytes.
            { "t": 3, "type": "hid_message", "data": "serial 0123456789abcdef" },
        ]))
        .to_string();
        let read = LogPart::from_json(&text).unwrap();
        assert_eq!(read.redactions, 5);
        let events: Value = serde_json::from_str(&read.events).unwrap();
        assert_eq!(events[0]["name"], "Pro Controller [redacted]");
        assert_eq!(events[0]["nested"][0]["who"], "[redacted]");
        assert_eq!(events[0]["fingerprint"], "not a fingerprint [redacted]");
        assert_eq!(events[1]["message"], "line break");
        assert_eq!(events[2]["data"], "serial [redacted]");
    }

    #[test]
    fn refuses_what_is_not_a_part() {
        let refused = |value: Value| LogPart::from_json(&value.to_string()).unwrap_err().0;
        let event = json!({ "t": 1, "type": "session" });
        let mut unknown = part(json!([event]));
        unknown["started_at"] = json!("2026-11-02T10:00:00Z");
        assert!(refused(unknown).contains("started_at"));
        let mut format = part(json!([event]));
        format["format"] = json!("other");
        assert!(refused(format).contains("format"));
        let mut session = part(json!([event]));
        session["session"] = json!("ABCDEF0123456789ABCDEF0123456789");
        assert!(refused(session).contains("session"));
        assert!(refused(part(json!([]))).contains("1 to"));
        assert!(refused(part(json!([{ "type": "session" }]))).contains("events[0]: t"));
        assert!(refused(part(json!([{ "t": -1, "type": "session" }]))).contains("t must"));
        assert!(refused(part(json!([{ "t": 1, "type": "Session" }]))).contains("type must"));
        assert!(
            refused(part(json!([{ "t": 1, "type": "a", "Key": 1 }])))
                .contains("\"Key\" is not a name")
        );
        assert!(refused(part(json!([1]))).contains("an event is an object"));
        let mut deep = json!(1);
        for _ in 0..20 {
            deep = json!([deep]);
        }
        assert!(refused(part(json!([{ "t": 1, "type": "a", "deep": deep }]))).contains("nesting"));
        let big = "x".repeat(MAX_PART_BYTES);
        assert!(
            refused(part(json!([{ "t": 1, "type": "a", "big": big }]))).contains("bytes at most")
        );
    }
}
