//! What the test mode's module (`anygamepad-wasm-testing`, W9) needs from
//! this one: a device as a session reads it ([`TestDevice`], from
//! `testDevice()`) and what it read at each frame ([`TestInput`], from
//! `testInput()`). The two modules share no memory, so these cross as plain
//! JavaScript values; the field names are the other module's.

use anygamepad::model::raw::RawState;
use anygamepad::model::{ButtonSet, GamepadState, OutputSet};
use anygamepad_report::Report;
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// A device as a test session reads it.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestDevice {
    /// The device report of the device; its first view is the API the page
    /// reads it through.
    #[tsify(type = "Record<string, unknown>")]
    pub report: Report,
    /// The outputs the page can drive on it.
    pub outputs: OutputSet,
    /// Whether its input comes with the HID input reports
    /// (`TestInput.reports`).
    pub hid: bool,
    /// Buttons of the pad the API is known not to read.
    pub unreadable: ButtonSet,
}

/// What a device read at one frame.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestInput {
    /// Its raw values, in the order of its layout.
    pub raw: RawState,
    /// Its canonical state.
    pub state: GamepadState,
    /// The HID input reports received since the last frame, oldest first.
    pub reports: Vec<TestInputReport>,
}

/// One HID input report.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct TestInputReport {
    /// Its report ID (0 without IDs).
    pub report_id: u8,
    /// Its data, without the report ID.
    pub data: Vec<u8>,
    /// When it arrived (`performance.now()`).
    pub timestamp: f64,
}

/// A value as plain JavaScript objects (maps too, `None` as `null`), as the
/// other module reads it back.
pub(crate) fn to_plain_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|error| JsError::new(&error.to_string()))
}
