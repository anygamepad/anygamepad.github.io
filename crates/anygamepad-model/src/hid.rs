//! The report structure of a HID device: its collections and the items of
//! its reports.
//!
//! WebHID gives this structure (`HIDDevice.collections`) instead of the raw
//! report descriptor; native backends can parse the same structure from the
//! descriptor, so device reports (P1) compare web and native views of a
//! device.

use crate::macros::{enum_set, named_enum};

/// A HID collection: report items grouped under a usage (HID 1.11, 6.2.2.6).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct HidCollection {
    /// Usage page (`0x01` Generic Desktop, `0xff00` and up vendor-defined).
    pub usage_page: u16,
    /// Usage within the page (Generic Desktop `0x05` Game Pad, `0x04`
    /// Joystick, ...).
    pub usage: u16,
    /// Collection type: 0 physical, 1 application, 2 logical, 3 report,
    /// 4 named array, 5 usage switch, 6 usage modifier.
    pub kind: u8,
    /// Nested collections, in descriptor order.
    pub children: Vec<HidCollection>,
    /// Input reports described in this collection.
    pub input_reports: Vec<HidReportInfo>,
    /// Output reports described in this collection.
    pub output_reports: Vec<HidReportInfo>,
    /// Feature reports described in this collection.
    pub feature_reports: Vec<HidReportInfo>,
}

/// The items of one report within a collection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct HidReportInfo {
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Main items, in descriptor order.
    pub items: Vec<HidReportItem>,
}

/// A main item of a report: `report_count` values of `report_size` bits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct HidReportItem {
    /// Extended usages (usage page in the high 16 bits, usage in the low 16
    /// bits), in order.
    pub usages: Vec<u32>,
    /// Extended usage range, when the item declares one.
    pub usage_range: Option<HidUsageRange>,
    /// Bits per value.
    pub report_size: u16,
    /// Number of values.
    pub report_count: u16,
    /// Smallest value the device reports.
    pub logical_minimum: i32,
    /// Largest value the device reports.
    pub logical_maximum: i32,
    /// Physical value of `logical_minimum`, in `unit`.
    pub physical_minimum: i32,
    /// Physical value of `logical_maximum`, in `unit`.
    pub physical_maximum: i32,
    /// System of the physical unit.
    pub unit_system: HidUnitSystem,
    /// Base 10 exponent of the physical unit.
    pub unit_exponent: i8,
    /// Main item flags.
    pub flags: HidItemFlags,
}

/// A range of extended usages, both ends included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct HidUsageRange {
    /// First usage.
    pub minimum: u32,
    /// Last usage.
    pub maximum: u32,
}

named_enum! {
    /// System of a physical unit (HID 1.11, 6.2.2.7).
    #[derive(Default)]
    pub enum HidUnitSystem: "HID unit system" {
        /// No unit.
        #[default]
        None = "none",
        /// SI, linear (centimeters, grams, seconds, ...).
        SiLinear = "si_linear",
        /// SI, rotation (radians).
        SiRotation = "si_rotation",
        /// English, linear (inches, slugs, ...).
        EnglishLinear = "english_linear",
        /// English, rotation (degrees).
        EnglishRotation = "english_rotation",
        /// Vendor-defined.
        VendorDefined = "vendor_defined",
        /// Reserved values.
        Reserved = "reserved",
    }
}

named_enum! {
    /// A main item flag (HID 1.11, 6.2.2.5), named after the meaning of a set
    /// bit. In declaration order, so a flag's index is its bit in the main
    /// item data.
    pub enum HidItemFlag: "HID item flag" {
        /// Constant (padding or fixed data) rather than data.
        Constant = "constant",
        /// Variable (one value per usage) rather than array (indices of the
        /// usages that are on).
        Variable = "variable",
        /// Relative rather than absolute.
        Relative = "relative",
        /// Wraps around its range.
        Wrap = "wrap",
        /// Non-linear.
        NonLinear = "non_linear",
        /// No preferred state (does not return to rest by itself).
        NoPreferred = "no_preferred",
        /// Has a null state outside its logical range (a centered hat
        /// switch).
        NullState = "null_state",
        /// Volatile (output and feature items).
        Volatile = "volatile",
        /// Buffered bytes rather than a bit field.
        BufferedBytes = "buffered_bytes",
    }
}

enum_set! {
    /// The flags of a main item: the set bits of its data.
    pub struct HidItemFlags(u16): HidItemFlag;
}

impl HidItemFlags {
    /// The flags of main item data bits.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// The main item data bits.
    pub const fn bits(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_main_item_bits() {
        // Data, Variable, Absolute, Null State: a hat switch.
        let hat = HidItemFlags::from_bits(0b0100_0010);
        assert!(hat.contains(HidItemFlag::Variable));
        assert!(hat.contains(HidItemFlag::NullState));
        assert!(!hat.contains(HidItemFlag::Constant));
        assert_eq!(hat.bits(), 0x42);
        assert_eq!(HidItemFlag::BufferedBytes.index(), 8);
        // Bits beyond the defined flags are dropped.
        assert_eq!(HidItemFlags::from_bits(0xff00).bits(), 0x0100);
        assert_eq!(HidItemFlags::ALL.len(), HidItemFlag::COUNT);
    }

    #[test]
    fn unit_system_names() {
        assert_eq!(HidUnitSystem::default(), HidUnitSystem::None);
        assert_eq!("si_rotation".parse(), Ok(HidUnitSystem::SiRotation));
    }
}
