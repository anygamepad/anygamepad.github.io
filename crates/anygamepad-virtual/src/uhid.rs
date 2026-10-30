//! The Linux UHID protocol, sans-IO: the `struct uhid_event` messages a
//! program writes to and reads from `/dev/uhid` to be a HID device (A5).
//!
//! Every message is one `struct uhid_event` (packed, native byte order):
//! a 32-bit type and a union of requests. The kernel answers short writes
//! with errors, so messages are always written whole ([`EVENT_SIZE`]).
//!
//! Source: the kernel's `include/uapi/linux/uhid.h` and
//! `Documentation/hid/uhid.rst`.

use crate::VirtualIdentity;

/// Size of `struct uhid_event`: the type and the largest request
/// (`uhid_create2_req`, 4372 bytes).
pub const EVENT_SIZE: usize = 4 + 4372;
/// Largest report or descriptor (`UHID_DATA_MAX`, `HID_MAX_DESCRIPTOR_SIZE`).
pub const DATA_MAX: usize = 4096;

const DESTROY: u32 = 1;
const START: u32 = 2;
const STOP: u32 = 3;
const OPEN: u32 = 4;
const CLOSE: u32 = 5;
const OUTPUT: u32 = 6;
const GET_REPORT: u32 = 9;
const GET_REPORT_REPLY: u32 = 10;
const CREATE2: u32 = 11;
const INPUT2: u32 = 12;
const SET_REPORT: u32 = 13;
const SET_REPORT_REPLY: u32 = 14;

/// `BUS_USB`, the bus every virtual pad shows.
pub const BUS_USB: u16 = 0x03;
/// `EIO`, the error a report request is answered with when the pad has no
/// such report.
pub const EIO: u16 = 5;

/// The type of a report in a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportType {
    /// `UHID_FEATURE_REPORT`.
    Feature,
    /// `UHID_OUTPUT_REPORT`.
    Output,
    /// `UHID_INPUT_REPORT`.
    Input,
}

impl ReportType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Feature),
            1 => Some(Self::Output),
            2 => Some(Self::Input),
            _ => None,
        }
    }
}

/// A message from the kernel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// A driver bound the device.
    Start,
    /// The driver released it.
    Stop,
    /// A program opened it: input reports are read from now on.
    Open,
    /// The last program closed it.
    Close,
    /// An output report, report ID first when the device uses IDs.
    Output {
        /// The report.
        data: Vec<u8>,
        /// Usually [`ReportType::Output`].
        report_type: ReportType,
    },
    /// The host reads a report; answer with [`get_report_reply`].
    GetReport {
        /// Request ID, to repeat in the reply.
        id: u32,
        /// Report number (ID).
        report: u8,
        /// Usually [`ReportType::Feature`].
        report_type: ReportType,
    },
    /// The host writes a report; answer with [`set_report_reply`].
    SetReport {
        /// Request ID, to repeat in the reply.
        id: u32,
        /// Report number (ID).
        report: u8,
        /// Usually [`ReportType::Feature`].
        report_type: ReportType,
        /// The report, report ID first when the device uses IDs.
        data: Vec<u8>,
    },
}

fn u16_at(event: &[u8], at: usize) -> u16 {
    u16::from_ne_bytes([event[at], event[at + 1]])
}

fn u32_at(event: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]])
}

/// Reads one message from the kernel; `None` for messages of other types
/// or too short to be one.
pub fn parse(event: &[u8]) -> Option<Request> {
    if event.len() < 4 {
        return None;
    }
    let data = |len_at: usize, data_at: usize| {
        let len = usize::from(u16_at(event, len_at)).min(DATA_MAX);
        event.get(data_at..data_at + len).map(<[u8]>::to_vec)
    };
    match u32_at(event, 0) {
        START => Some(Request::Start),
        STOP => Some(Request::Stop),
        OPEN => Some(Request::Open),
        CLOSE => Some(Request::Close),
        OUTPUT if event.len() >= 4 + DATA_MAX + 3 => Some(Request::Output {
            data: data(4 + DATA_MAX, 4)?,
            report_type: ReportType::from_u8(event[4 + DATA_MAX + 2])?,
        }),
        GET_REPORT if event.len() >= 10 => Some(Request::GetReport {
            id: u32_at(event, 4),
            report: event[8],
            report_type: ReportType::from_u8(event[9])?,
        }),
        SET_REPORT if event.len() >= 12 => Some(Request::SetReport {
            id: u32_at(event, 4),
            report: event[8],
            report_type: ReportType::from_u8(event[9])?,
            data: data(10, 12)?,
        }),
        _ => None,
    }
}

fn event(kind: u32) -> Vec<u8> {
    let mut event = vec![0; EVENT_SIZE];
    event[..4].copy_from_slice(&kind.to_ne_bytes());
    event
}

/// Creates the device (`UHID_CREATE2`). `name` is cut to 127 bytes; the
/// descriptor must fit [`DATA_MAX`]. No physical path and no unique ID:
/// nothing that looks like a serial number or an address.
pub fn create(identity: &VirtualIdentity, descriptor: &[u8]) -> Vec<u8> {
    assert!(descriptor.len() <= DATA_MAX, "report descriptor too long");
    let mut e = event(CREATE2);
    let name = identity.name.as_bytes();
    let name = &name[..name.len().min(127)];
    e[4..4 + name.len()].copy_from_slice(name);
    e[260..262].copy_from_slice(&(descriptor.len() as u16).to_ne_bytes());
    e[262..264].copy_from_slice(&BUS_USB.to_ne_bytes());
    e[264..268].copy_from_slice(&u32::from(identity.vendor_id).to_ne_bytes());
    e[268..272].copy_from_slice(&u32::from(identity.product_id).to_ne_bytes());
    e[272..276].copy_from_slice(&u32::from(identity.version).to_ne_bytes());
    // Country code 0 (not localized).
    e[280..280 + descriptor.len()].copy_from_slice(descriptor);
    e
}

/// Removes the device (`UHID_DESTROY`); closing `/dev/uhid` does too.
pub fn destroy() -> Vec<u8> {
    event(DESTROY)
}

/// Sends an input report (`UHID_INPUT2`), report ID first when the device
/// uses IDs.
pub fn input(report: &[u8]) -> Vec<u8> {
    let len = report.len().min(DATA_MAX);
    let mut e = event(INPUT2);
    e[4..6].copy_from_slice(&(len as u16).to_ne_bytes());
    e[6..6 + len].copy_from_slice(&report[..len]);
    e
}

/// Answers a [`Request::GetReport`]: the report, or an errno (`error`
/// non-zero, `report` ignored).
pub fn get_report_reply(id: u32, error: u16, report: &[u8]) -> Vec<u8> {
    let len = if error == 0 {
        report.len().min(DATA_MAX)
    } else {
        0
    };
    let mut e = event(GET_REPORT_REPLY);
    e[4..8].copy_from_slice(&id.to_ne_bytes());
    e[8..10].copy_from_slice(&error.to_ne_bytes());
    e[10..12].copy_from_slice(&(len as u16).to_ne_bytes());
    e[12..12 + len].copy_from_slice(&report[..len]);
    e
}

/// Answers a [`Request::SetReport`]: 0 or an errno.
pub fn set_report_reply(id: u32, error: u16) -> Vec<u8> {
    let mut e = event(SET_REPORT_REPLY);
    e[4..8].copy_from_slice(&id.to_ne_bytes());
    e[8..10].copy_from_slice(&error.to_ne_bytes());
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

    fn kernel_event(kind: u32, body: &[(usize, &[u8])]) -> Vec<u8> {
        let mut e = event(kind);
        for (at, bytes) in body {
            e[*at..*at + bytes.len()].copy_from_slice(bytes);
        }
        e
    }

    #[test]
    fn create_layout() {
        let identity = Target::DualSense.identity();
        let e = create(identity, &[0x05, 0x01, 0xc0]);
        assert_eq!(e.len(), EVENT_SIZE);
        assert_eq!(u32_at(&e, 0), CREATE2);
        assert!(e[4..].starts_with(identity.name.as_bytes()));
        assert_eq!(e[4 + identity.name.len()], 0);
        // No phys, no uniq.
        assert!(e[132..260].iter().all(|&b| b == 0));
        assert_eq!(u16_at(&e, 260), 3);
        assert_eq!(u16_at(&e, 262), BUS_USB);
        assert_eq!(u32_at(&e, 264), 0x054c);
        assert_eq!(u32_at(&e, 268), 0x0ce6);
        assert_eq!(e[280..283], [0x05, 0x01, 0xc0]);
    }

    #[test]
    fn replies() {
        let e = input(&[0x01, 0x80]);
        assert_eq!((u32_at(&e, 0), u16_at(&e, 4)), (INPUT2, 2));
        assert_eq!(e[6..8], [0x01, 0x80]);

        let e = get_report_reply(7, 0, &[0x05, 1, 2]);
        assert_eq!((u32_at(&e, 4), u16_at(&e, 8), u16_at(&e, 10)), (7, 0, 3));
        assert_eq!(e[12..15], [0x05, 1, 2]);
        let e = get_report_reply(8, EIO, &[1, 2]);
        assert_eq!((u16_at(&e, 8), u16_at(&e, 10)), (EIO, 0));

        let e = set_report_reply(9, 0);
        assert_eq!((u32_at(&e, 0), u32_at(&e, 4)), (SET_REPORT_REPLY, 9));
        assert_eq!(u32_at(&destroy(), 0), DESTROY);
    }

    #[test]
    fn parses_kernel_events() {
        assert_eq!(parse(&kernel_event(START, &[])), Some(Request::Start));
        assert_eq!(parse(&kernel_event(OPEN, &[])), Some(Request::Open));
        let output = kernel_event(
            OUTPUT,
            &[
                (4, &[0x02, 0xff]),
                (4100, &2u16.to_ne_bytes()),
                (4102, &[1]),
            ],
        );
        assert_eq!(
            parse(&output),
            Some(Request::Output {
                data: vec![0x02, 0xff],
                report_type: ReportType::Output
            })
        );
        let get = kernel_event(GET_REPORT, &[(4, &42u32.to_ne_bytes()), (8, &[0x05, 0])]);
        assert_eq!(
            parse(&get),
            Some(Request::GetReport {
                id: 42,
                report: 0x05,
                report_type: ReportType::Feature
            })
        );
        let set = kernel_event(
            SET_REPORT,
            &[
                (4, &3u32.to_ne_bytes()),
                (8, &[0x08, 0]),
                (10, &1u16.to_ne_bytes()),
                (12, &[9]),
            ],
        );
        assert_eq!(
            parse(&set),
            Some(Request::SetReport {
                id: 3,
                report: 0x08,
                report_type: ReportType::Feature,
                data: vec![9]
            })
        );
        assert_eq!(parse(&kernel_event(99, &[])), None);
        assert_eq!(parse(&[1, 2]), None);
        // A bad report type.
        assert_eq!(parse(&kernel_event(GET_REPORT, &[(9, &[7])])), None);
    }
}
