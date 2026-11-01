//! Report descriptor parsing (HID 1.11, 6.2.2) and report fields.

use core::fmt;
use std::collections::BTreeMap;

use anygamepad_model::hid::{
    HidCollection, HidItemFlag, HidItemFlags, HidReportInfo, HidReportItem, HidUnitSystem,
    HidUsageRange,
};

/// Bytes of a report descriptor (Linux's `HID_MAX_DESCRIPTOR_SIZE`).
pub const MAX_DESCRIPTOR_BYTES: usize = 4096;

/// Nesting levels of collections, and of the `Push` global item stack.
pub const MAX_COLLECTION_DEPTH: usize = 32;

/// Bits of one report: its length must fit the 16-bit lengths of USB
/// transfers.
const MAX_REPORT_BITS: u32 = 8 * 0xffff;

/// The type of a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReportKind {
    /// Device to host, on the interrupt pipe.
    Input,
    /// Host to device: LEDs, rumble.
    Output,
    /// Either way, on request: configuration, calibration.
    Feature,
}

/// Why a report descriptor cannot be parsed.
///
/// The parser is otherwise lenient, as hosts are, so that a descriptor
/// that works with an operating system parses: an `End Collection` without
/// a collection and a `Pop` without a `Push` are ignored, collections still
/// open at the end are closed, and reserved items are skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The descriptor is longer than [`MAX_DESCRIPTOR_BYTES`].
    TooLong(usize),
    /// The item at this byte offset runs past the end of the descriptor.
    Truncated(usize),
    /// The collection or `Push` at this byte offset nests deeper than
    /// [`MAX_COLLECTION_DEPTH`].
    TooDeep(usize),
    /// The main item at this byte offset makes its report longer than a
    /// USB transfer.
    ReportTooLong(usize),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong(len) => write!(
                f,
                "report descriptor of {len} bytes, more than {MAX_DESCRIPTOR_BYTES}"
            ),
            Self::Truncated(offset) => {
                write!(f, "report descriptor item at byte {offset} is truncated")
            }
            Self::TooDeep(offset) => write!(
                f,
                "report descriptor item at byte {offset} nests deeper than {MAX_COLLECTION_DEPTH} levels"
            ),
            Self::ReportTooLong(offset) => write!(
                f,
                "report descriptor item at byte {offset} makes its report too long"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// A main item of a report, where it sits in the report.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HidField {
    /// The report the field belongs to.
    pub kind: ReportKind,
    /// Report ID; 0 when the device does not use report IDs.
    pub report_id: u8,
    /// Position of the first value, in bits from the start of the report
    /// data (after the report ID byte, when there is one).
    pub bit_offset: u32,
    /// Extended usage of the top-level collection the field is in
    /// (`0x0001_0005` for a Game Pad); 0 outside any collection.
    pub application: u32,
    /// The item, as it appears in the collections.
    pub item: HidReportItem,
}

impl HidField {
    /// Bits the field takes in its report.
    pub fn bits(&self) -> u32 {
        u32::from(self.item.report_size) * u32::from(self.item.report_count)
    }

    /// Whether the field is constant (padding or fixed data).
    pub fn is_constant(&self) -> bool {
        self.item.flags.contains(HidItemFlag::Constant)
    }

    /// Whether each value is the state of one usage (variable), rather than
    /// the index of a usage that is on (array).
    pub fn is_variable(&self) -> bool {
        self.item.flags.contains(HidItemFlag::Variable)
    }

    /// Value `index` of the field in `data` (the report without its ID
    /// byte), sign-extended when the logical minimum is negative. `None`
    /// when `data` is too short, `index` is out of the field or values are
    /// wider than 32 bits.
    pub fn read(&self, data: &[u8], index: u16) -> Option<i32> {
        let size = self.item.report_size;
        if index >= self.item.report_count || size == 0 || size > 32 {
            return None;
        }
        let start = self.bit_offset + u32::from(index) * u32::from(size);
        let value = read_bits(data, start, size)?;
        Some(if self.item.logical_minimum < 0 && size < 32 {
            let shift = 32 - u32::from(size);
            ((value << shift) as i32) >> shift
        } else {
            value as i32
        })
    }

    /// Extended usage of value `index` of a variable field: the usages in
    /// declaration order, then the usage range; the last one applies to
    /// the values beyond them (HID 1.11, 6.2.2.8).
    pub fn usage(&self, index: u16) -> Option<u32> {
        let usages = &self.item.usages;
        let index = usize::from(index);
        if let Some(&usage) = usages.get(index) {
            return Some(usage);
        }
        match self.item.usage_range {
            Some(range) if range.minimum <= range.maximum => {
                let step = u32::try_from(index - usages.len()).unwrap_or(u32::MAX);
                Some(range.minimum.saturating_add(step).min(range.maximum))
            }
            _ => usages.last().copied(),
        }
    }

    /// Extended usage that `value` of an array field stands for: the
    /// usages in declaration order, then the usage range, counted from the
    /// logical minimum. `None` for values outside them (no usage on).
    pub fn array_usage(&self, value: i32) -> Option<u32> {
        let index =
            usize::try_from(i64::from(value) - i64::from(self.item.logical_minimum)).ok()?;
        let usages = &self.item.usages;
        if let Some(&usage) = usages.get(index) {
            return Some(usage);
        }
        let range = self.item.usage_range?;
        let usage = u64::from(range.minimum) + (index - usages.len()) as u64;
        u32::try_from(usage)
            .ok()
            .filter(|&usage| usage <= range.maximum)
    }
}

/// Reads `size` (at most 32) bits from `start`, little-endian, as reports
/// lay them out.
fn read_bits(data: &[u8], start: u32, size: u16) -> Option<u32> {
    let start = start as usize;
    let end = start + usize::from(size);
    if end > data.len() * 8 {
        return None;
    }
    // At most 5 bytes: 32 bits plus 7 of offset into the first byte.
    let bytes = &data[start / 8..end.div_ceil(8)];
    let value = bytes
        .iter()
        .rev()
        .fold(0u64, |value, &byte| (value << 8) | u64::from(byte));
    Some(((value >> (start % 8)) & ((1u64 << size) - 1)) as u32)
}

/// A parsed report descriptor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Descriptor {
    /// Top-level collections, as WebHID gives them (`HIDDevice.collections`):
    /// each lists, per report ID in ascending order, the main items declared
    /// directly in it.
    pub collections: Vec<HidCollection>,
    /// Every main item with its position in its report, in descriptor
    /// order, including items outside any collection.
    pub fields: Vec<HidField>,
}

impl Descriptor {
    /// Parses a report descriptor.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() > MAX_DESCRIPTOR_BYTES {
            return Err(ParseError::TooLong(bytes.len()));
        }
        let mut parser = Parser::default();
        let mut offset = 0;
        while offset < bytes.len() {
            let prefix = bytes[offset];
            if prefix == 0xfe {
                // Long item: data size, tag, data. HID 1.11 defines no long
                // item tags.
                let size = *bytes.get(offset + 1).ok_or(ParseError::Truncated(offset))?;
                let end = offset + 3 + usize::from(size);
                if end > bytes.len() {
                    return Err(ParseError::Truncated(offset));
                }
                offset = end;
                continue;
            }
            let size = [0, 1, 2, 4][usize::from(prefix & 0x03)];
            let end = offset + 1 + size;
            let data = bytes
                .get(offset + 1..end)
                .ok_or(ParseError::Truncated(offset))?;
            let item = Data {
                value: data
                    .iter()
                    .rev()
                    .fold(0, |value, &byte| (value << 8) | u32::from(byte)),
                size: size as u8,
            };
            let tag = prefix >> 4;
            match (prefix >> 2) & 0x03 {
                0 => parser.main(tag, item, offset)?,
                1 => parser.global(tag, item, offset)?,
                2 => parser.local(tag, item),
                _ => {}
            }
            offset = end;
        }
        Ok(parser.finish())
    }

    /// Whether the device puts a report ID before each report.
    pub fn uses_report_ids(&self) -> bool {
        self.fields.iter().any(|field| field.report_id != 0)
    }

    /// The fields of one report, in order.
    pub fn report_fields(
        &self,
        kind: ReportKind,
        report_id: u8,
    ) -> impl Iterator<Item = &HidField> + '_ {
        self.fields
            .iter()
            .filter(move |field| field.kind == kind && field.report_id == report_id)
    }

    /// Bytes of one report, without its report ID byte; 0 for reports the
    /// descriptor does not declare.
    pub fn report_bytes(&self, kind: ReportKind, report_id: u8) -> usize {
        let bits = self
            .report_fields(kind, report_id)
            .map(|field| field.bit_offset + field.bits())
            .max()
            .unwrap_or(0);
        bits.div_ceil(8) as usize
    }

    /// Splits a report as a USB host or a HID API delivers it into its
    /// report ID and data: the ID is the first byte when the device uses
    /// report IDs, 0 otherwise. `None` for an empty report from a device
    /// that uses report IDs.
    pub fn split_report<'a>(&self, report: &'a [u8]) -> Option<(u8, &'a [u8])> {
        split_report(self.uses_report_ids(), report)
    }
}

pub(crate) fn split_report(report_ids: bool, report: &[u8]) -> Option<(u8, &[u8])> {
    if report_ids {
        report.split_first().map(|(&id, data)| (id, data))
    } else {
        Some((0, report))
    }
}

/// The data of a short item, as read: little-endian, `size` bytes.
#[derive(Clone, Copy, Debug, Default)]
struct Data {
    value: u32,
    size: u8,
}

impl Data {
    /// The data as a two's complement number of its size.
    fn signed(self) -> i32 {
        match self.size {
            0 => 0,
            1 => i32::from(self.value as u8 as i8),
            2 => i32::from(self.value as u16 as i16),
            _ => self.value as i32,
        }
    }

    /// The maximum of a range whose minimum is `minimum`: signed when the
    /// minimum is negative, unsigned otherwise. Descriptors often encode
    /// `0..255` as `15 00 25 ff`, whose maximum reads as -1 if signed; hosts
    /// (Linux among them) read it as 255.
    fn maximum(self, minimum: i32) -> i32 {
        if minimum < 0 {
            self.signed()
        } else {
            self.value as i32
        }
    }
}

/// Global items (HID 1.11, 6.2.2.7).
#[derive(Clone, Copy, Debug, Default)]
struct Globals {
    usage_page: u16,
    logical_minimum: Data,
    logical_maximum: Data,
    physical_minimum: Data,
    physical_maximum: Data,
    unit_exponent: i8,
    unit: u32,
    report_size: u16,
    report_id: u8,
    report_count: u16,
}

/// A local usage item. A 4-byte usage is an extended usage (usage page in
/// the high 16 bits); shorter ones take the usage page in effect at the
/// main item.
#[derive(Clone, Copy, Debug)]
struct Usage(Data);

impl Usage {
    fn resolve(self, usage_page: u16) -> u32 {
        if self.0.size == 4 {
            self.0.value
        } else {
            (u32::from(usage_page) << 16) | (self.0.value & 0xffff)
        }
    }
}

/// Local items (HID 1.11, 6.2.2.8), cleared after each main item.
#[derive(Debug, Default)]
struct Locals {
    usages: Vec<Usage>,
    minimum: Option<Usage>,
    maximum: Option<Usage>,
}

#[derive(Debug, Default)]
struct Parser {
    globals: Globals,
    stack: Vec<Globals>,
    locals: Locals,
    /// Open collections, outermost first.
    open: Vec<HidCollection>,
    /// Extended usage of the open top-level collection.
    application: u32,
    collections: Vec<HidCollection>,
    fields: Vec<HidField>,
    /// Bits declared so far in each report.
    report_bits: BTreeMap<(ReportKind, u8), u32>,
}

impl Parser {
    fn main(&mut self, tag: u8, data: Data, offset: usize) -> Result<(), ParseError> {
        match tag {
            0x8 => self.report_item(ReportKind::Input, data, offset)?,
            0x9 => self.report_item(ReportKind::Output, data, offset)?,
            0xb => self.report_item(ReportKind::Feature, data, offset)?,
            0xa => {
                if self.open.len() >= MAX_COLLECTION_DEPTH {
                    return Err(ParseError::TooDeep(offset));
                }
                let usage = self
                    .locals
                    .usages
                    .first()
                    .map_or(0, |usage| usage.resolve(self.globals.usage_page));
                if self.open.is_empty() {
                    self.application = usage;
                }
                self.open.push(HidCollection {
                    usage_page: (usage >> 16) as u16,
                    usage: usage as u16,
                    kind: data.value as u8,
                    ..HidCollection::default()
                });
            }
            0xc => self.close_collection(),
            _ => {}
        }
        self.locals = Locals::default();
        Ok(())
    }

    fn report_item(
        &mut self,
        kind: ReportKind,
        data: Data,
        offset: usize,
    ) -> Result<(), ParseError> {
        let globals = self.globals;
        let page = globals.usage_page;
        let logical_minimum = globals.logical_minimum.signed();
        let physical_minimum = globals.physical_minimum.signed();
        let usage_range = match (self.locals.minimum, self.locals.maximum) {
            (Some(minimum), Some(maximum)) => Some(HidUsageRange {
                minimum: minimum.resolve(page),
                maximum: maximum.resolve(page),
            }),
            _ => None,
        };
        let item = HidReportItem {
            usages: self
                .locals
                .usages
                .iter()
                .map(|usage| usage.resolve(page))
                .collect(),
            usage_range,
            report_size: globals.report_size,
            report_count: globals.report_count,
            logical_minimum,
            logical_maximum: globals.logical_maximum.maximum(logical_minimum),
            physical_minimum,
            physical_maximum: globals.physical_maximum.maximum(physical_minimum),
            unit_system: unit_system(globals.unit),
            unit_exponent: globals.unit_exponent,
            flags: HidItemFlags::from_bits(data.value as u16),
        };

        let bits = self
            .report_bits
            .entry((kind, globals.report_id))
            .or_default();
        let bit_offset = *bits;
        let field_bits = u32::from(globals.report_size) * u32::from(globals.report_count);
        *bits = bit_offset
            .checked_add(field_bits)
            .filter(|&bits| bits <= MAX_REPORT_BITS)
            .ok_or(ParseError::ReportTooLong(offset))?;

        if let Some(collection) = self.open.last_mut() {
            let reports = match kind {
                ReportKind::Input => &mut collection.input_reports,
                ReportKind::Output => &mut collection.output_reports,
                ReportKind::Feature => &mut collection.feature_reports,
            };
            let index = match reports.binary_search_by_key(&globals.report_id, |r| r.report_id) {
                Ok(index) => index,
                Err(index) => {
                    reports.insert(
                        index,
                        HidReportInfo {
                            report_id: globals.report_id,
                            items: Vec::new(),
                        },
                    );
                    index
                }
            };
            reports[index].items.push(item.clone());
        }
        self.fields.push(HidField {
            kind,
            report_id: globals.report_id,
            bit_offset,
            application: if self.open.is_empty() {
                0
            } else {
                self.application
            },
            item,
        });
        Ok(())
    }

    fn close_collection(&mut self) {
        if let Some(collection) = self.open.pop() {
            match self.open.last_mut() {
                Some(parent) => parent.children.push(collection),
                None => self.collections.push(collection),
            }
        }
    }

    fn global(&mut self, tag: u8, data: Data, offset: usize) -> Result<(), ParseError> {
        let globals = &mut self.globals;
        let saturated = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        match tag {
            0x0 => globals.usage_page = data.value as u16,
            0x1 => globals.logical_minimum = data,
            0x2 => globals.logical_maximum = data,
            0x3 => globals.physical_minimum = data,
            0x4 => globals.physical_maximum = data,
            0x5 => {
                // A 4-bit two's complement number (HID 1.11, 6.2.2.7), which
                // some devices write as a whole signed byte instead.
                globals.unit_exponent = if data.value & !0x0f == 0 {
                    ((data.value as i8) << 4) >> 4
                } else {
                    data.signed().clamp(i8::MIN.into(), i8::MAX.into()) as i8
                };
            }
            0x6 => globals.unit = data.value,
            0x7 => globals.report_size = saturated(data.value),
            0x8 => globals.report_id = data.value as u8,
            0x9 => globals.report_count = saturated(data.value),
            0xa => {
                if self.stack.len() >= MAX_COLLECTION_DEPTH {
                    return Err(ParseError::TooDeep(offset));
                }
                self.stack.push(*globals);
            }
            0xb => {
                if let Some(saved) = self.stack.pop() {
                    self.globals = saved;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn local(&mut self, tag: u8, data: Data) {
        match tag {
            0x0 => self.locals.usages.push(Usage(data)),
            0x1 => self.locals.minimum = Some(Usage(data)),
            0x2 => self.locals.maximum = Some(Usage(data)),
            // Designators, strings and delimiters do not change the
            // structure.
            _ => {}
        }
    }

    fn finish(mut self) -> Descriptor {
        while !self.open.is_empty() {
            self.close_collection();
        }
        Descriptor {
            collections: self.collections,
            fields: self.fields,
        }
    }
}

/// The system nibble of a unit item.
fn unit_system(unit: u32) -> HidUnitSystem {
    match unit & 0x0f {
        0x0 => HidUnitSystem::None,
        0x1 => HidUnitSystem::SiLinear,
        0x2 => HidUnitSystem::SiRotation,
        0x3 => HidUnitSystem::EnglishLinear,
        0x4 => HidUnitSystem::EnglishRotation,
        0xf => HidUnitSystem::VendorDefined,
        _ => HidUnitSystem::Reserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boot keyboard of HID 1.11, appendix E.6.
    const KEYBOARD: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, // Generic Desktop, Keyboard, Application
        0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, // Keyboard, Left Control to Right GUI
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, // modifiers
        0x95, 0x01, 0x75, 0x08, 0x81, 0x01, // reserved byte
        0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, // LEDs
        0x95, 0x01, 0x75, 0x03, 0x91, 0x01, // LED padding
        0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65, // 6 keys, 0 to 101
        0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, // Data Array Abs
        0xc0,
    ];

    /// The boot mouse of HID 1.11, appendix E.10.
    const MOUSE: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, // Generic Desktop, Mouse, Application
        0x09, 0x01, 0xa1, 0x00, // Pointer, Physical
        0x05, 0x09, 0x19, 0x01, 0x29, 0x03, // Button 1 to 3
        0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, // buttons
        0x95, 0x01, 0x75, 0x05, 0x81, 0x01, // padding
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, // X, Y
        0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06, // Data Var Rel
        0xc0, 0xc0,
    ];

    #[test]
    fn boot_keyboard() {
        let descriptor = Descriptor::parse(KEYBOARD).unwrap();
        assert_eq!(descriptor.collections.len(), 1);
        let keyboard = &descriptor.collections[0];
        assert_eq!(
            (keyboard.usage_page, keyboard.usage, keyboard.kind),
            (0x01, 0x06, 1)
        );
        assert!(keyboard.children.is_empty());

        assert_eq!(keyboard.input_reports.len(), 1);
        let input = &keyboard.input_reports[0];
        assert_eq!(input.report_id, 0);
        assert_eq!(input.items.len(), 3);
        let modifiers = &input.items[0];
        assert_eq!(
            modifiers.usage_range,
            Some(HidUsageRange {
                minimum: 0x0007_00e0,
                maximum: 0x0007_00e7,
            })
        );
        assert_eq!((modifiers.report_size, modifiers.report_count), (1, 8));
        assert!(modifiers.flags.contains(HidItemFlag::Variable));
        assert!(input.items[1].flags.contains(HidItemFlag::Constant));
        let keys = &input.items[2];
        assert!(!keys.flags.contains(HidItemFlag::Variable));
        assert_eq!((keys.logical_minimum, keys.logical_maximum), (0, 101));

        let leds = &keyboard.output_reports[0].items[0];
        assert_eq!(leds.usage_range.unwrap().minimum, 0x0008_0001);
        assert!(keyboard.feature_reports.is_empty());

        assert!(!descriptor.uses_report_ids());
        assert_eq!(descriptor.report_bytes(ReportKind::Input, 0), 8);
        assert_eq!(descriptor.report_bytes(ReportKind::Output, 0), 1);
        assert_eq!(descriptor.report_bytes(ReportKind::Feature, 0), 0);
        let offsets: Vec<_> = descriptor
            .report_fields(ReportKind::Input, 0)
            .map(|field| field.bit_offset)
            .collect();
        assert_eq!(offsets, [0, 8, 16]);
        assert!(
            descriptor
                .fields
                .iter()
                .all(|field| field.application == 0x0001_0006)
        );
    }

    #[test]
    fn keyboard_report_values() {
        let descriptor = Descriptor::parse(KEYBOARD).unwrap();
        let fields: Vec<_> = descriptor.report_fields(ReportKind::Input, 0).collect();
        // Left Shift, and A and B pressed.
        let report = [0x02, 0x00, 0x04, 0x05, 0, 0, 0, 0];
        assert_eq!(fields[0].read(&report, 0), Some(0));
        assert_eq!(fields[0].read(&report, 1), Some(1));
        assert_eq!(fields[0].usage(1), Some(0x0007_00e1)); // Left Shift
        assert_eq!(fields[2].read(&report, 0), Some(4));
        assert_eq!(fields[2].array_usage(4), Some(0x0007_0004)); // a and A
        assert_eq!(fields[2].array_usage(0x66), None);
        assert_eq!(fields[2].read(&report, 6), None);
        assert_eq!(fields[2].read(&report[..4], 2), None);
    }

    #[test]
    fn boot_mouse() {
        let descriptor = Descriptor::parse(MOUSE).unwrap();
        let mouse = &descriptor.collections[0];
        assert!(mouse.input_reports.is_empty());
        let pointer = &mouse.children[0];
        assert_eq!(
            (pointer.usage_page, pointer.usage, pointer.kind),
            (0x01, 0x01, 0)
        );
        let motion = &pointer.input_reports[0].items[2];
        assert_eq!(motion.usages, [0x0001_0030, 0x0001_0031]);
        assert_eq!(
            (motion.logical_minimum, motion.logical_maximum),
            (-127, 127)
        );
        assert!(motion.flags.contains(HidItemFlag::Relative));

        let fields: Vec<_> = descriptor.report_fields(ReportKind::Input, 0).collect();
        let report = [0b101, 0xff, 0x05];
        assert_eq!(fields[0].read(&report, 2), Some(1));
        assert_eq!(fields[2].read(&report, 0), Some(-1));
        assert_eq!(fields[2].read(&report, 1), Some(5));
        assert_eq!(fields[2].usage(1), Some(0x0001_0031));
        assert!(
            descriptor
                .fields
                .iter()
                .all(|field| field.application == 0x0001_0002)
        );
    }

    #[test]
    fn report_ids_and_unsigned_maximum() {
        let descriptor = Descriptor::parse(&[
            0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, // Game Pad
            0x85, 0x02, 0x09, 0x30, 0x15, 0x00, 0x25, 0xff, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
            0x85, 0x01, 0x09, 0x31, 0x81, 0x02, // same globals, report 1
            0x85, 0x02, 0x09, 0x32, 0x81, 0x02, // back to report 2
            0xc0,
        ])
        .unwrap();
        let collection = &descriptor.collections[0];
        let ids: Vec<_> = collection
            .input_reports
            .iter()
            .map(|r| r.report_id)
            .collect();
        assert_eq!(ids, [1, 2]);
        assert_eq!(collection.input_reports[1].items.len(), 2);
        // `25 ff` after a non-negative minimum is 255, not -1.
        assert_eq!(collection.input_reports[0].items[0].logical_maximum, 255);

        assert!(descriptor.uses_report_ids());
        let z = descriptor
            .report_fields(ReportKind::Input, 2)
            .nth(1)
            .unwrap();
        assert_eq!((z.bit_offset, z.item.usages[0]), (8, 0x0001_0032));
        assert_eq!(z.read(&[0x10, 0xc8], 0), Some(200));
        assert_eq!(descriptor.report_bytes(ReportKind::Input, 2), 2);
        assert_eq!(descriptor.split_report(&[2, 9, 8]), Some((2, &[9, 8][..])));
        assert_eq!(descriptor.split_report(&[]), None);
    }

    #[test]
    fn extended_usages_push_pop_and_units() {
        let descriptor = Descriptor::parse(&[
            0x05, 0x09, // Button page in effect
            0x0b, 0x05, 0x00, 0x01, 0x00, // extended usage Generic Desktop, Game Pad
            0xa1, 0x01, //
            0x15, 0x00, 0x25, 0x07, 0x75, 0x04, 0x95, 0x01, //
            0xa4, // Push
            0x05, 0x01, 0x09, 0x39, // Hat Switch
            0x35, 0x00, 0x46, 0x3b, 0x01, // 0 to 315
            0x65, 0x14, 0x55, 0x0e, // degrees, exponent -2
            0x81, 0x42, // Data Var Abs Null
            0xb4, // Pop: Button page, no unit
            0x09, 0x01, 0x81, 0x02, //
            0xfe, 0x02, 0x00, 0xaa, 0xbb, // long item, skipped
            0xb4, // Pop without Push, ignored
            0xc0, 0xc0, // one End Collection too many, ignored
        ])
        .unwrap();
        let game_pad = &descriptor.collections[0];
        assert_eq!((game_pad.usage_page, game_pad.usage), (0x01, 0x05));
        let items = &game_pad.input_reports[0].items;
        let hat = &items[0];
        assert_eq!(hat.usages, [0x0001_0039]);
        assert_eq!(hat.physical_maximum, 315);
        assert_eq!(hat.unit_system, HidUnitSystem::EnglishRotation);
        assert_eq!(hat.unit_exponent, -2);
        assert!(hat.flags.contains(HidItemFlag::NullState));
        let button = &items[1];
        assert_eq!(button.usages, [0x0009_0001]);
        assert_eq!(button.unit_system, HidUnitSystem::None);
        assert_eq!(button.unit_exponent, 0);
        assert_eq!(button.logical_maximum, 7); // not pushed, kept
    }

    #[test]
    fn unclosed_collections_are_closed() {
        let descriptor =
            Descriptor::parse(&[0x09, 0x05, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00]).unwrap();
        assert_eq!(descriptor.collections.len(), 1);
        assert_eq!(descriptor.collections[0].children.len(), 1);
    }

    #[test]
    fn items_outside_collections_are_fields_only() {
        let descriptor = Descriptor::parse(&[0x75, 0x08, 0x95, 0x01, 0x81, 0x01]).unwrap();
        assert!(descriptor.collections.is_empty());
        assert_eq!(descriptor.fields.len(), 1);
        assert_eq!(descriptor.fields[0].application, 0);
    }

    #[test]
    fn errors() {
        assert_eq!(Descriptor::parse(&[]), Ok(Descriptor::default()));
        assert_eq!(
            Descriptor::parse(&[0x05, 0x01, 0x26, 0xff]),
            Err(ParseError::Truncated(2))
        );
        assert_eq!(Descriptor::parse(&[0xfe]), Err(ParseError::Truncated(0)));
        assert_eq!(
            Descriptor::parse(&[0xfe, 0x04, 0x00, 0x01]),
            Err(ParseError::Truncated(0))
        );
        let deep: Vec<u8> = [0xa1, 0x00].repeat(MAX_COLLECTION_DEPTH + 1);
        assert_eq!(
            Descriptor::parse(&deep),
            Err(ParseError::TooDeep(2 * MAX_COLLECTION_DEPTH))
        );
        let pushes = vec![0xa4; MAX_COLLECTION_DEPTH + 1];
        assert_eq!(
            Descriptor::parse(&pushes),
            Err(ParseError::TooDeep(MAX_COLLECTION_DEPTH))
        );
        // 65535 values of 16 bits.
        let long = [
            0x77, 0x10, 0x00, 0x00, 0x00, 0x97, 0xff, 0xff, 0x00, 0x00, 0x81, 0x02,
        ];
        assert_eq!(Descriptor::parse(&long), Err(ParseError::ReportTooLong(10)));
        assert_eq!(
            Descriptor::parse(&vec![0; MAX_DESCRIPTOR_BYTES + 1]),
            Err(ParseError::TooLong(MAX_DESCRIPTOR_BYTES + 1))
        );
        assert!(ParseError::Truncated(3).to_string().contains("byte 3"));
    }

    #[test]
    fn wide_and_unaligned_values() {
        let field = |size: u16, logical_minimum: i32| HidField {
            kind: ReportKind::Input,
            report_id: 0,
            bit_offset: 4,
            application: 0,
            item: HidReportItem {
                report_size: size,
                report_count: 2,
                logical_minimum,
                ..HidReportItem::default()
            },
        };
        let data = [0xf0, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(field(32, 0).read(&data, 0), Some(-1)); // as i32, no sign extension
        assert_eq!(field(32, 0).read(&data, 1), Some(7));
        assert_eq!(field(12, -2048).read(&data, 0), Some(-1));
        assert_eq!(field(12, 0).read(&data, 0), Some(0xfff));
        assert_eq!(field(33, 0).read(&data, 0), None);
        assert_eq!(field(0, 0).read(&data, 0), None);
    }

    #[test]
    fn usages_beyond_the_declared_ones() {
        let mut item = HidReportItem {
            usages: vec![0x0001_0030],
            usage_range: Some(HidUsageRange {
                minimum: 0x0009_0001,
                maximum: 0x0009_0002,
            }),
            report_count: 5,
            logical_minimum: 1,
            ..HidReportItem::default()
        };
        let field = |item: &HidReportItem| HidField {
            kind: ReportKind::Input,
            report_id: 0,
            bit_offset: 0,
            application: 0,
            item: item.clone(),
        };
        let usages: Vec<_> = (0..5).map(|i| field(&item).usage(i)).collect();
        assert_eq!(
            usages,
            [
                0x0001_0030,
                0x0009_0001,
                0x0009_0002,
                0x0009_0002,
                0x0009_0002
            ]
            .map(Some)
        );
        assert_eq!(field(&item).array_usage(1), Some(0x0001_0030));
        assert_eq!(field(&item).array_usage(3), Some(0x0009_0002));
        assert_eq!(field(&item).array_usage(4), None);
        assert_eq!(field(&item).array_usage(0), None);
        item.usage_range = None;
        assert_eq!(field(&item).usage(3), Some(0x0001_0030));
        item.usages.clear();
        assert_eq!(field(&item).usage(0), None);
    }
}
