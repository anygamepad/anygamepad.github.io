//! Raw elements of a HID game controller, read from its input reports.

use anygamepad_model::raw::{Hat, RawAxisInfo, RawLayout, RawSource, RawState};

use crate::descriptor::{Descriptor, HidField, ReportKind, split_report};

/// Buttons, axes or hat switches a [`RawReader`] lists, each; elements
/// beyond it are left out. Matches the device report format's limit.
pub const MAX_RAW_ELEMENTS: usize = 256;

const GENERIC_DESKTOP: u16 = 0x01;
const HAT_SWITCH: u16 = 0x39;
const BUTTON: u16 = 0x09;
const FIRST_VENDOR_PAGE: u16 = 0xff00;

/// Top-level collections of game controllers: Generic Desktop Joystick,
/// Game Pad and Multi-axis Controller.
const GAME_CONTROLLERS: [u32; 3] = [0x0001_0004, 0x0001_0005, 0x0001_0008];

/// What one value of a variable field feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A 1-bit (or 0 to 1) value: pressed or not.
    Button(usize),
    /// A wider value on the Button page: pressure, scaled to `[0, 1]`.
    AnalogButton(usize),
    Axis(usize),
    /// `four_way`: the hat reports 4 directions (0 to 3) rather than 8.
    Hat {
        index: usize,
        four_way: bool,
    },
}

#[derive(Clone, Debug)]
struct Binding {
    field: usize,
    value: u16,
    target: Target,
}

/// An array field: each value is the index of a usage that is on.
#[derive(Clone, Debug)]
struct ArrayBinding {
    field: usize,
    /// (extended usage, button), sorted by usage.
    buttons: Vec<(u32, usize)>,
}

/// Reads the buttons, axes and hat switches of a HID game controller from
/// its input reports.
///
/// Elements are named by usage ([`RawSource::Hid`]), so a device profile
/// can refer to them as `b{hid:0x0009:0x0001}`. They come from the input
/// fields that are data (not constant) on a usage page that is not
/// vendor-defined, with a usage other than 0:
///
/// - the Hat Switch usage (Generic Desktop `0x39`) is a hat; values outside
///   its logical range (its null state) read as centered;
/// - 1-bit values (or values from 0 to 1) are buttons;
/// - wider values on the Button page are analog buttons, scaled to
///   `[0, 1]` over their logical range;
/// - every other wider value is an axis, over its logical range;
/// - the usages an array field can report are buttons, pressed while the
///   array holds them.
///
/// A usage that several fields report (the same stick in two reports, as
/// Bluetooth pads send) is one element, fed by each; its axis range is the
/// first field's. Building a reader allocates; reading does not.
#[derive(Clone, Debug)]
pub struct RawReader {
    layout: RawLayout,
    fields: Vec<HidField>,
    bindings: Vec<Binding>,
    arrays: Vec<ArrayBinding>,
    report_ids: bool,
}

impl RawReader {
    /// The reader of a device's game controller collections (Joystick, Game
    /// Pad, Multi-axis Controller), or of all its input fields when it has
    /// none.
    pub fn new(descriptor: &Descriptor) -> Self {
        let game = |field: &HidField| GAME_CONTROLLERS.contains(&field.application);
        if descriptor.fields.iter().any(game) {
            Self::with_filter(descriptor, game)
        } else {
            Self::with_filter(descriptor, |_| true)
        }
    }

    /// The reader of the input fields `keep` accepts.
    pub fn with_filter(descriptor: &Descriptor, mut keep: impl FnMut(&HidField) -> bool) -> Self {
        let mut reader = Self {
            layout: RawLayout::default(),
            fields: Vec::new(),
            bindings: Vec::new(),
            arrays: Vec::new(),
            report_ids: descriptor.uses_report_ids(),
        };
        let inputs = descriptor
            .fields
            .iter()
            .filter(|field| field.kind == ReportKind::Input && !field.is_constant());
        for field in inputs.filter(|field| keep(field)) {
            let index = reader.fields.len();
            let bound = if field.is_variable() {
                reader.bind_variable(index, field)
            } else {
                reader.bind_array(index, field)
            };
            if bound {
                reader.fields.push(field.clone());
            }
        }
        reader
    }

    fn bind_variable(&mut self, index: usize, field: &HidField) -> bool {
        let item = &field.item;
        let digital =
            item.report_size == 1 || (item.logical_minimum == 0 && item.logical_maximum == 1);
        let mut bound = false;
        for value in 0..item.report_count {
            let Some(source) = field.usage(value).and_then(source) else {
                continue;
            };
            let RawSource::Hid { page, usage } = source else {
                unreachable!("`source` returns HID sources");
            };
            let target = if page == GENERIC_DESKTOP && usage == HAT_SWITCH {
                element(&mut self.layout.hats, source, |&hat| hat).map(|index| Target::Hat {
                    index,
                    four_way: item.logical_maximum - item.logical_minimum == 3,
                })
            } else if digital {
                element(&mut self.layout.buttons, source, |&button| button).map(Target::Button)
            } else if page == BUTTON {
                element(&mut self.layout.buttons, source, |&button| button)
                    .map(Target::AnalogButton)
            } else {
                let axis = RawAxisInfo {
                    source,
                    min: item.logical_minimum as f32,
                    max: item.logical_maximum as f32,
                };
                element(&mut self.layout.axes, axis, |axis| axis.source).map(Target::Axis)
            };
            if let Some(target) = target {
                self.bindings.push(Binding {
                    field: index,
                    value,
                    target,
                });
                bound = true;
            }
        }
        bound
    }

    fn bind_array(&mut self, index: usize, field: &HidField) -> bool {
        let item = &field.item;
        let ranged = item.usage_range.into_iter().flat_map(|range| {
            // Bounded, so a range over a whole page stays cheap.
            (range.minimum..=range.maximum).take(MAX_RAW_ELEMENTS)
        });
        let mut buttons: Vec<(u32, usize)> = Vec::new();
        for usage in item.usages.iter().copied().chain(ranged) {
            let Some(source) = source(usage) else {
                continue;
            };
            if let Some(button) = element(&mut self.layout.buttons, source, |&button| button) {
                buttons.push((usage, button));
            }
        }
        buttons.sort_unstable();
        buttons.dedup_by_key(|&mut (usage, _)| usage);
        if buttons.is_empty() {
            return false;
        }
        self.arrays.push(ArrayBinding {
            field: index,
            buttons,
        });
        true
    }

    /// The elements the reader fills.
    pub fn layout(&self) -> &RawLayout {
        &self.layout
    }

    /// Reads an input report as a USB host or a HID API delivers it: with
    /// the report ID as its first byte when the device uses report IDs.
    /// See [`read_report`](Self::read_report).
    pub fn read(&self, report: &[u8], raw: &mut RawState) -> bool {
        split_report(self.report_ids, report)
            .is_some_and(|(report_id, data)| self.read_report(report_id, data, raw))
    }

    /// Reads input report `report_id`, given without its ID byte (as WebHID
    /// gives it), into `raw`. Only the elements this report carries change.
    /// Returns whether the report carried any; `false` also when `raw` was
    /// not made for this reader's layout ([`RawState::for_layout`]).
    pub fn read_report(&self, report_id: u8, data: &[u8], raw: &mut RawState) -> bool {
        if !raw.fits(&self.layout) {
            return false;
        }
        let mut read = false;
        for binding in &self.bindings {
            let field = &self.fields[binding.field];
            if field.report_id != report_id {
                continue;
            }
            let Some(value) = field.read(data, binding.value) else {
                continue;
            };
            read = true;
            let minimum = field.item.logical_minimum;
            let span = i64::from(field.item.logical_maximum) - i64::from(minimum);
            let offset = i64::from(value) - i64::from(minimum);
            match binding.target {
                Target::Button(index) => {
                    raw.buttons[index] = if offset > 0 { 1.0 } else { 0.0 };
                }
                Target::AnalogButton(index) => {
                    raw.buttons[index] = if span > 0 {
                        (offset as f32 / span as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                }
                Target::Axis(index) => raw.axes[index] = value as f32,
                Target::Hat { index, four_way } => {
                    raw.hats[index] = match (four_way, i32::try_from(offset)) {
                        (true, Ok(quarter @ 0..4)) => Hat::from_hid(quarter * 2),
                        (false, Ok(eighth)) => Hat::from_hid(eighth),
                        _ => Hat::CENTERED,
                    };
                }
            }
        }
        for array in &self.arrays {
            let field = &self.fields[array.field];
            if field.report_id != report_id
                || field
                    .read(data, field.item.report_count.saturating_sub(1))
                    .is_none()
            {
                continue;
            }
            read = true;
            for &(_, button) in &array.buttons {
                raw.buttons[button] = 0.0;
            }
            for value in 0..field.item.report_count {
                let found = field
                    .read(data, value)
                    .and_then(|value| field.array_usage(value))
                    .and_then(|usage| {
                        array
                            .buttons
                            .binary_search_by_key(&usage, |&(usage, _)| usage)
                            .ok()
                    });
                if let Some(found) = found {
                    raw.buttons[array.buttons[found].1] = 1.0;
                }
            }
        }
        read
    }
}

/// The raw source of an extended usage, unless it is vendor-defined or 0.
fn source(usage: u32) -> Option<RawSource> {
    let (page, usage) = ((usage >> 16) as u16, usage as u16);
    (page < FIRST_VENDOR_PAGE && usage != 0).then_some(RawSource::Hid { page, usage })
}

/// The index of the element of `list` with `key`'s source, added if new.
/// `None` when the list is full.
fn element<T: Copy>(list: &mut Vec<T>, new: T, source: impl Fn(&T) -> RawSource) -> Option<usize> {
    let wanted = source(&new);
    if let Some(index) = list.iter().position(|element| source(element) == wanted) {
        return Some(index);
    }
    if list.len() >= MAX_RAW_ELEMENTS {
        return None;
    }
    list.push(new);
    Some(list.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hid(page: u16, usage: u16) -> RawSource {
        RawSource::Hid { page, usage }
    }

    /// A game pad with report ID 1: 12 buttons, an 8-way hat with a null
    /// state, X, Y, Z and Rz from 0 to 255 (written `25 ff`), then a vendor
    /// byte.
    const GAME_PAD: &[u8] = &[
        0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, 0x85, 0x01, //
        0x05, 0x09, 0x19, 0x01, 0x29, 0x0c, //
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0c, 0x81, 0x02, //
        0x05, 0x01, 0x09, 0x39, 0x15, 0x00, 0x25, 0x07, //
        0x35, 0x00, 0x46, 0x3b, 0x01, 0x65, 0x14, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, //
        0x65, 0x00, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09, 0x35, //
        0x15, 0x00, 0x25, 0xff, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, //
        0x06, 0x00, 0xff, 0x09, 0x20, 0x95, 0x01, 0x81, 0x02, //
        0xc0,
    ];

    #[test]
    fn game_pad_layout_and_report() {
        let descriptor = Descriptor::parse(GAME_PAD).unwrap();
        let reader = RawReader::new(&descriptor);
        let layout = reader.layout();
        assert_eq!(layout.buttons.len(), 12);
        assert_eq!(layout.buttons[0], hid(0x09, 1));
        assert_eq!(layout.buttons[11], hid(0x09, 12));
        assert_eq!(layout.hats, [hid(0x01, 0x39)]);
        let axes: Vec<_> = layout.axes.iter().map(|axis| axis.source).collect();
        assert_eq!(axes, [0x30, 0x31, 0x32, 0x35].map(|usage| hid(0x01, usage)));
        assert_eq!((layout.axes[0].min, layout.axes[0].max), (0.0, 255.0));

        let mut raw = RawState::for_layout(layout);
        // Buttons 1 and 12, hat right (2), X 0, Y 128, Z 255, Rz 64.
        let report = [0x01, 0b0000_0001, 0b0010_1000, 0x00, 0x80, 0xff, 0x40, 0x99];
        assert!(reader.read(&report, &mut raw));
        assert_eq!(raw.buttons[0], 1.0);
        assert_eq!(raw.buttons[1], 0.0);
        assert_eq!(raw.buttons[11], 1.0);
        assert_eq!(raw.hats[0], Hat(Hat::RIGHT));
        assert_eq!(raw.axes, [0.0, 128.0, 255.0, 64.0]);

        // The null state centers the hat.
        let report = [0x01, 0, 0b1000_0000, 0, 0, 0, 0, 0];
        assert!(reader.read(&report, &mut raw));
        assert_eq!(raw.hats[0], Hat::CENTERED);
        assert_eq!(raw.buttons[11], 0.0);

        // Other reports, short reports and mismatched states read nothing.
        assert!(!reader.read(&[0x02, 0xff, 0xff], &mut raw));
        assert!(!reader.read(&[], &mut raw));
        assert!(!reader.read(&report, &mut RawState::default()));
        assert!(!reader.read_report(1, &[], &mut raw));
    }

    #[test]
    fn other_collections_are_left_out() {
        // A keyboard collection, then the game pad: only the game pad's
        // elements are listed.
        let mut bytes = vec![
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x85, 0x02, //
            0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, //
            0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0xc0,
        ];
        bytes.extend_from_slice(GAME_PAD);
        let descriptor = Descriptor::parse(&bytes).unwrap();
        assert_eq!(RawReader::new(&descriptor).layout().buttons.len(), 12);
        let all = RawReader::with_filter(&descriptor, |_| true);
        assert_eq!(all.layout().buttons.len(), 20);
    }

    #[test]
    fn array_buttons_and_analog_buttons() {
        // No report IDs. Two array slots for Button 1 to 4 (1 = none), and
        // one 8-bit pressure value for Button 5.
        let descriptor = Descriptor::parse(&[
            0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, //
            0x05, 0x09, 0x19, 0x01, 0x29, 0x04, //
            0x15, 0x01, 0x25, 0x04, 0x75, 0x04, 0x95, 0x02, 0x81, 0x00, //
            0x09, 0x05, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, //
            0xc0,
        ])
        .unwrap();
        let reader = RawReader::new(&descriptor);
        assert_eq!(
            reader.layout().buttons,
            (1..=5).map(|usage| hid(0x09, usage)).collect::<Vec<_>>()
        );
        let mut raw = RawState::for_layout(reader.layout());
        // Slots hold values 4 and 2, i.e. buttons 4 and 2; pressure 51.
        assert!(reader.read(&[0x24, 51], &mut raw));
        assert_eq!(raw.buttons, [0.0, 1.0, 0.0, 1.0, 0.2]);
        // Values outside the logical range press nothing; earlier presses
        // are released.
        assert!(reader.read(&[0x0f, 0], &mut raw));
        assert_eq!(raw.buttons, [0.0; 5]);
    }

    #[test]
    fn four_way_hats_and_shared_usages() {
        // Report 1 and report 2 both carry X and a 4-way hat.
        let descriptor = Descriptor::parse(&[
            0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, //
            0x85, 0x01, 0x09, 0x30, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
            0x09, 0x39, 0x15, 0x00, 0x25, 0x03, 0x81, 0x42, //
            0x85, 0x02, 0x09, 0x39, 0x81, 0x42, 0x09, 0x30, 0x15, 0x81, 0x25, 0x7f, 0x81, 0x02,
            0xc0,
        ])
        .unwrap();
        let reader = RawReader::new(&descriptor);
        assert_eq!(reader.layout().axes.len(), 1);
        assert_eq!(reader.layout().hats.len(), 1);
        assert_eq!(reader.layout().axes[0].min, -127.0);
        let mut raw = RawState::for_layout(reader.layout());
        assert!(reader.read(&[1, 0x81, 1], &mut raw));
        assert_eq!((raw.axes[0], raw.hats[0]), (-127.0, Hat(Hat::RIGHT)));
        assert!(reader.read(&[2, 2, 0x10], &mut raw));
        assert_eq!((raw.axes[0], raw.hats[0]), (16.0, Hat(Hat::DOWN)));
        assert!(reader.read(&[2, 4, 0], &mut raw));
        assert_eq!(raw.hats[0], Hat::CENTERED);
    }

    #[test]
    fn element_limit() {
        // An array over a whole page lists at most MAX_RAW_ELEMENTS buttons.
        let descriptor = Descriptor::parse(&[
            0x05, 0x09, 0x09, 0x05, 0xa1, 0x01, //
            0x19, 0x01, 0x2a, 0xff, 0xff, 0x15, 0x01, 0x27, 0xff, 0xff, 0x00, 0x00, //
            0x75, 0x10, 0x95, 0x01, 0x81, 0x00, 0xc0,
        ])
        .unwrap();
        let reader = RawReader::new(&descriptor);
        assert_eq!(reader.layout().buttons.len(), MAX_RAW_ELEMENTS);
    }
}
