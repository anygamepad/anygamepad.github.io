//! HID report descriptors and reports, sans-IO.
//!
//! WebHID hands the browser backend a device's report structure already
//! parsed (`HIDDevice.collections`). Backends that read HID devices directly
//! (native HID APIs, USB host firmware) get the raw report descriptor
//! instead. This crate parses it into the same structure, and reads the
//! device's reports with it:
//!
//! - [`Descriptor::parse`] turns the descriptor bytes into the
//!   [`HidCollection`](anygamepad_model::hid::HidCollection)s WebHID would
//!   give, plus every report field with its bit position ([`HidField`]).
//! - [`HidField::read`] reads one value of a field from a report.
//! - [`RawReader`] lists a game controller's buttons, axes and hat switches
//!   as a [`RawLayout`](anygamepad_model::raw::RawLayout), named by HID usage
//!   ([`RawSource::Hid`](anygamepad_model::raw::RawSource::Hid)), and fills a
//!   [`RawState`](anygamepad_model::raw::RawState) from each input report
//!   without allocating. The mapping engine then applies the device's profile.
//!
//! ```
//! use anygamepad_hid::{Descriptor, RawReader};
//! use anygamepad_model::raw::{RawSource, RawState};
//!
//! // A game pad with two buttons, padded to a byte.
//! let descriptor = Descriptor::parse(&[
//!     0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, // Generic Desktop, Game Pad, Application
//!     0x05, 0x09, 0x19, 0x01, 0x29, 0x02, // Button 1 to 2
//!     0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02, // 2 x 1 bit, Data Var Abs
//!     0x95, 0x06, 0x81, 0x01, // 6 bits of padding
//!     0xc0,
//! ])?;
//! let reader = RawReader::new(&descriptor);
//! assert_eq!(reader.layout().buttons[1], RawSource::Hid { page: 0x09, usage: 2 });
//!
//! let mut raw = RawState::for_layout(reader.layout());
//! assert!(reader.read(&[0b10], &mut raw)); // button 2 pressed
//! assert_eq!(raw.buttons, [0.0, 1.0]);
//! # Ok::<(), anygamepad_hid::ParseError>(())
//! ```
#![forbid(unsafe_code)]

mod descriptor;
mod raw;

pub use descriptor::{
    Descriptor, HidField, MAX_COLLECTION_DEPTH, MAX_DESCRIPTOR_BYTES, ParseError, ReportKind,
};
pub use raw::{MAX_RAW_ELEMENTS, RawReader};
