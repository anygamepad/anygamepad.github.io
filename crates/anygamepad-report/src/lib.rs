//! Device reports: what the web tester and the desktop app's collection
//! mode saw of a controller, in one format for every place a report goes
//! (ADR 0007): a file, the browser's IndexedDB, a GitHub issue, and later
//! the sync server.
//!
//! A [`Report`] holds the app and host it comes from, what the person says
//! about the device ([`UserStatement`]), and one [`View`] per provider or
//! browser API that sees the device: its identity and fingerprint, the raw
//! elements it reads, HID structure and sample input reports, and the
//! mapping the person confirmed with the guided wizard.
//!
//! Reports never contain serial numbers, Bluetooth MAC addresses or other
//! personal data. Identities exclude them by construction, clients
//! [`scrub`](Report::scrub) what still looks like them, and every reader
//! [`validate`](Report::validate)s (see [`privacy`]).
//!
//! ```
//! use anygamepad_model::identity::{Browser, BrowserEngine, DeviceIdentity, Os};
//! use anygamepad_model::raw::RawLayout;
//! use anygamepad_model::OutputSet;
//! use anygamepad_report::{App, Date, Host, Report, Source, View};
//!
//! let browser = Browser { engine: BrowserEngine::Blink, major_version: Some(141) };
//! let source = Source {
//!     app: App::Web,
//!     app_version: "0.1.0".parse().unwrap(),
//!     library_version: "0.1.0".parse().unwrap(),
//! };
//! let host = Host { os: Os::Windows, os_version: None, browser: Some(browser) };
//! let mut report = Report::new(source, host, Date::new(2026, 10, 14).unwrap());
//!
//! let identity = DeviceIdentity::from_gamepad_api(
//!     "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
//!     "standard",
//!     browser,
//!     Os::Windows,
//! );
//! report.views.push(View::new(identity, RawLayout::default(), OutputSet::EMPTY));
//!
//! assert!(report.scrub().is_empty());
//! report.validate().unwrap();
//! ```
//!
//! # Features
//!
//! - `json`: [`Report::from_json`] and [`Report::to_json`], with the size
//!   limit of [`MAX_REPORT_BYTES`].
//! - `schemars`: [`json_schema`].
#![forbid(unsafe_code)]

pub mod privacy;
mod report;
mod validate;

#[cfg(feature = "json")]
mod json;

pub use anygamepad_model::ControllerMode;
#[cfg(feature = "json")]
pub use json::ReportError;
pub use report::{
    App, Date, HexBytes, HidData, Host, InputSample, ParseError, Report, Source, UserStatement,
    Version, View,
};
pub use validate::InvalidReport;

/// The value of every report's `format` field.
pub const FORMAT: &str = "anygamepad-device-report";

/// The report format version this library reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Views per report.
pub const MAX_VIEWS: usize = 8;

/// Characters of a text the device or the OS reports (names, ids, OS
/// release).
pub const MAX_TEXT_CHARS: usize = 256;

/// Characters of `user.product`.
pub const MAX_USER_TEXT_CHARS: usize = 100;

/// Buttons, axes, hats or touchpads in a view's layout, each.
pub const MAX_ELEMENTS: usize = 256;

/// Input samples per view.
pub const MAX_SAMPLES: usize = 8;

/// Bytes of one input sample.
pub const MAX_SAMPLE_BYTES: usize = 1024;

/// Bytes of a HID report descriptor (Linux's `HID_MAX_DESCRIPTOR_SIZE`).
pub const MAX_DESCRIPTOR_BYTES: usize = 4096;

/// Nesting levels of HID collections.
pub const MAX_HID_NESTING: usize = 8;

/// Collections, reports, items and usages in a view's HID structure,
/// together.
pub const MAX_HID_ENTRIES: usize = 2048;

/// Bytes of a report as JSON: it must fit in the body of a GitHub issue
/// (65,536 characters) next to the form's other fields (ADR 0007).
pub const MAX_REPORT_BYTES: usize = 60_000;

/// The JSON Schema of the report format, as committed in
/// `data/reports/schema.json`.
///
/// The schema checks the structure; [`Report::validate`] checks the rest.
#[cfg(feature = "schemars")]
pub fn json_schema() -> schemars::Schema {
    let mut schema = schemars::schema_for!(Report);
    schema.insert(
        "$id".into(),
        "https://anygamepad.github.io/schema/report.json".into(),
    );
    schema
}
