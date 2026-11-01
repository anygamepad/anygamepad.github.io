//! HID report descriptors: a builder for the emulated pads' descriptors, and
//! a reader that lists the reports a descriptor declares and their sizes.
//!
//! The reader only follows what sizes need (report IDs, sizes and counts,
//! `Push`/`Pop`); it is not a full parser. Exact HID emulation (UHID on
//! Linux, A5) uses it to know how long each report the host may ask for is.

use core::fmt;

/// The kind of a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReportKind {
    /// Device to host, sent on its own.
    Input,
    /// Host to device.
    Output,
    /// Read or written by the host on request.
    Feature,
}

/// A report a descriptor declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReportLayout {
    /// Input, output or feature.
    pub kind: ReportKind,
    /// Report ID; 0 when the descriptor uses none.
    pub report_id: u8,
    /// Size of the report's data in bits, without the report ID.
    pub bits: u32,
}

impl ReportLayout {
    /// Size of the report's data in bytes, without the report ID.
    pub const fn len(&self) -> usize {
        self.bits.div_ceil(8) as usize
    }

    /// Whether the report carries no data.
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

/// Why a descriptor could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    /// An item runs past the end of the descriptor.
    Truncated,
    /// A long item (HID 1.11, 6.2.2.3), which no gamepad uses.
    LongItem,
    /// A `Pop` without a matching `Push`.
    UnbalancedPop,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Truncated => "report descriptor truncated",
            Self::LongItem => "long items are not supported",
            Self::UnbalancedPop => "report descriptor pops more than it pushes",
        })
    }
}

impl std::error::Error for DescriptorError {}

/// The reports `descriptor` declares, in order of first appearance.
pub fn reports(descriptor: &[u8]) -> Result<Vec<ReportLayout>, DescriptorError> {
    #[derive(Clone, Copy, Default)]
    struct Globals {
        report_id: u8,
        report_size: u32,
        report_count: u32,
    }

    let mut globals = Globals::default();
    let mut stack = Vec::new();
    let mut layouts: Vec<ReportLayout> = Vec::new();
    let mut rest = descriptor;
    while let Some((&prefix, tail)) = rest.split_first() {
        if prefix == 0xfe {
            return Err(DescriptorError::LongItem);
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => usize::from(n),
        };
        if tail.len() < size {
            return Err(DescriptorError::Truncated);
        }
        let (data, tail) = tail.split_at(size);
        rest = tail;
        let value = data
            .iter()
            .rev()
            .fold(0u32, |value, &byte| (value << 8) | u32::from(byte));
        let kind = match prefix & 0xfc {
            0x74 => {
                globals.report_size = value;
                None
            }
            0x84 => {
                globals.report_id = value as u8;
                None
            }
            0x94 => {
                globals.report_count = value;
                None
            }
            0xa4 => {
                stack.push(globals);
                None
            }
            0xb4 => {
                globals = stack.pop().ok_or(DescriptorError::UnbalancedPop)?;
                None
            }
            0x80 => Some(ReportKind::Input),
            0x90 => Some(ReportKind::Output),
            0xb0 => Some(ReportKind::Feature),
            _ => None,
        };
        if let Some(kind) = kind {
            let bits = globals.report_size * globals.report_count;
            let id = globals.report_id;
            match layouts
                .iter_mut()
                .find(|layout| layout.kind == kind && layout.report_id == id)
            {
                Some(layout) => layout.bits += bits,
                None => layouts.push(ReportLayout {
                    kind,
                    report_id: id,
                    bits,
                }),
            }
        }
    }
    Ok(layouts)
}

/// Generic Desktop usage page.
pub(crate) const GENERIC_DESKTOP: u16 = 0x01;
/// Button usage page.
pub(crate) const BUTTON: u16 = 0x09;
/// First vendor-defined usage page.
pub(crate) const VENDOR: u16 = 0xff00;

/// Generic Desktop usages.
pub(crate) const GAME_PAD: u16 = 0x05;
pub(crate) const X: u16 = 0x30;
pub(crate) const Y: u16 = 0x31;
pub(crate) const Z: u16 = 0x32;
pub(crate) const RX: u16 = 0x33;
pub(crate) const RY: u16 = 0x34;
pub(crate) const RZ: u16 = 0x35;
pub(crate) const HAT_SWITCH: u16 = 0x39;

/// Main item flags: Data, Variable, Absolute.
pub(crate) const VARIABLE: u32 = 0x02;
/// Main item flags: Data, Variable, Absolute, Null state.
pub(crate) const VARIABLE_NULL: u32 = 0x42;

/// Writes a report descriptor item by item.
#[derive(Default)]
pub(crate) struct Builder(Vec<u8>);

impl Builder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.0
    }

    /// An item whose data is unsigned, in as few bytes as it fits.
    fn unsigned(&mut self, prefix: u8, value: u32) -> &mut Self {
        let bytes = value.to_le_bytes();
        let size = match value {
            0 => 0,
            1..=0xff => 1,
            0x100..=0xffff => 2,
            _ => 4,
        };
        self.push(prefix, &bytes[..size])
    }

    /// An item whose data is signed, in as few bytes as it fits.
    fn signed(&mut self, prefix: u8, value: i32) -> &mut Self {
        let bytes = value.to_le_bytes();
        let size = if value == 0 {
            0
        } else if i8::try_from(value).is_ok() {
            1
        } else if i16::try_from(value).is_ok() {
            2
        } else {
            4
        };
        self.push(prefix, &bytes[..size])
    }

    fn push(&mut self, prefix: u8, data: &[u8]) -> &mut Self {
        let size = match data.len() {
            4 => 3,
            n => n as u8,
        };
        self.0.push(prefix | size);
        self.0.extend_from_slice(data);
        self
    }

    pub(crate) fn usage_page(&mut self, page: u16) -> &mut Self {
        self.unsigned(0x04, page.into())
    }

    pub(crate) fn usage(&mut self, usage: u16) -> &mut Self {
        self.unsigned(0x08, usage.into())
    }

    pub(crate) fn usage_range(&mut self, min: u16, max: u16) -> &mut Self {
        self.unsigned(0x18, min.into()).unsigned(0x28, max.into())
    }

    pub(crate) fn logical(&mut self, min: i32, max: i32) -> &mut Self {
        self.signed(0x14, min).signed(0x24, max)
    }

    pub(crate) fn physical(&mut self, min: i32, max: i32) -> &mut Self {
        self.signed(0x34, min).signed(0x44, max)
    }

    /// Unit (HID 1.11, 6.2.2.7); 0 for none.
    pub(crate) fn unit(&mut self, unit: u32) -> &mut Self {
        self.unsigned(0x64, unit)
    }

    pub(crate) fn report_id(&mut self, id: u8) -> &mut Self {
        self.unsigned(0x84, id.into())
    }

    /// `count` values of `size` bits.
    pub(crate) fn report(&mut self, size: u32, count: u32) -> &mut Self {
        self.unsigned(0x74, size).unsigned(0x94, count)
    }

    pub(crate) fn input(&mut self, flags: u32) -> &mut Self {
        self.unsigned(0x80, flags)
    }

    pub(crate) fn output(&mut self, flags: u32) -> &mut Self {
        self.unsigned(0x90, flags)
    }

    pub(crate) fn feature(&mut self, flags: u32) -> &mut Self {
        self.unsigned(0xb0, flags)
    }

    /// An application collection.
    pub(crate) fn application(&mut self) -> &mut Self {
        self.unsigned(0xa0, 0x01)
    }

    pub(crate) fn end_collection(&mut self) -> &mut Self {
        self.push(0xc0, &[])
    }

    /// `len` bytes of vendor-defined data in a report of `kind`, under
    /// vendor `usage`.
    pub(crate) fn vendor_bytes(
        &mut self,
        kind: ReportKind,
        report_id: u8,
        usage: u16,
        len: u32,
    ) -> &mut Self {
        self.report_id(report_id)
            .usage_page(VENDOR)
            .usage(usage)
            .logical(0, 255)
            .report(8, len);
        match kind {
            ReportKind::Input => self.input(VARIABLE),
            ReportKind::Output => self.output(VARIABLE),
            ReportKind::Feature => self.feature(VARIABLE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_sizes() {
        let mut b = Builder::new();
        b.usage_page(VENDOR)
            .logical(0, 255)
            .report(8, 0)
            .end_collection();
        // 255 does not fit a signed byte; a count of 0 takes no data.
        assert_eq!(
            b.finish(),
            [
                0x06, 0x00, 0xff, 0x14, 0x26, 0xff, 0x00, 0x75, 0x08, 0x94, 0xc0
            ]
        );
        let mut b = Builder::new();
        b.logical(-32768, 32767).physical(0, 315);
        assert_eq!(
            b.finish(),
            [0x16, 0x00, 0x80, 0x26, 0xff, 0x7f, 0x34, 0x46, 0x3b, 0x01]
        );
    }

    #[test]
    fn reads_report_sizes() {
        let mut b = Builder::new();
        b.usage_page(GENERIC_DESKTOP)
            .usage(GAME_PAD)
            .application()
            .report_id(1)
            .report(8, 4)
            .input(VARIABLE)
            .report(4, 1)
            .input(VARIABLE_NULL);
        let mut descriptor = b.finish();
        // Push, a different report size, Pop: the size comes back.
        descriptor.extend_from_slice(&[0xa4, 0x75, 0x01, 0xb4]);
        let mut b = Builder::new();
        b.report(4, 1)
            .input(VARIABLE)
            .vendor_bytes(ReportKind::Feature, 2, 0x24, 36)
            .end_collection();
        descriptor.extend(b.finish());

        assert_eq!(
            reports(&descriptor),
            Ok(vec![
                ReportLayout {
                    kind: ReportKind::Input,
                    report_id: 1,
                    bits: 40
                },
                ReportLayout {
                    kind: ReportKind::Feature,
                    report_id: 2,
                    bits: 288
                },
            ])
        );
        assert_eq!(reports(&[0x75]), Err(DescriptorError::Truncated));
        assert_eq!(reports(&[0xfe, 0x00]), Err(DescriptorError::LongItem));
        assert_eq!(reports(&[0xb4]), Err(DescriptorError::UnbalancedPop));
        assert_eq!(reports(&[]), Ok(vec![]));
    }
}
