//! Reports shared by the tests: a web report of a DualSense over USB in
//! Chrome on Windows (Gamepad API and WebHID views, as
//! `data/reports/example.json`), and a desktop report on Linux with three
//! providers.
//!
//! The HID structure follows the DualSense's USB report descriptor
//! (`docs/research/09-dualsense-hid-protocol.md`); the values are
//! illustrative, not a recording.

#![allow(dead_code)]

use std::collections::BTreeMap;

use anygamepad_db::{AxisMapping, Input, Mapping};
use anygamepad_model::hid::{
    HidCollection, HidItemFlag, HidItemFlags, HidReportInfo, HidReportItem, HidUnitSystem,
    HidUsageRange,
};
use anygamepad_model::identity::{
    Backend, Browser, BrowserEngine, DescriptorHash, DeviceIdentity, Os, ProviderKind,
};
use anygamepad_model::raw::{RawAxisInfo, RawLayout, RawSource};
use anygamepad_model::{
    Axis, Button, Capabilities, Output, OutputSet, Sensor, SensorSet, TouchpadInfo, Transport,
};
use anygamepad_report::{
    App, ControllerMode, Date, HidData, Host, Report, Source, UserStatement, View,
};

pub const CHROME: Browser = Browser {
    engine: BrowserEngine::Blink,
    major_version: Some(141),
};

pub fn day() -> Date {
    Date::new(2026, 10, 14).unwrap()
}

fn source(app: App) -> Source {
    Source {
        app,
        app_version: "0.1.0".parse().unwrap(),
        library_version: "0.1.0".parse().unwrap(),
    }
}

fn item(
    usages: &[u32],
    size: u16,
    count: u16,
    min: i32,
    max: i32,
    flags: &[HidItemFlag],
) -> HidReportItem {
    HidReportItem {
        usages: usages.to_vec(),
        usage_range: None,
        report_size: size,
        report_count: count,
        logical_minimum: min,
        logical_maximum: max,
        physical_minimum: 0,
        physical_maximum: 0,
        unit_system: HidUnitSystem::None,
        unit_exponent: 0,
        flags: HidItemFlags::from_iter(flags.iter().copied()),
    }
}

const fn usage(page: u32, id: u32) -> u32 {
    page << 16 | id
}

/// A report of `count` vendor-defined bytes.
fn vendor_report(report_id: u8, vendor_usage: u32, count: u16, output: bool) -> HidReportInfo {
    let mut flags = vec![HidItemFlag::Variable];
    if output {
        flags.push(HidItemFlag::NonLinear);
    }
    HidReportInfo {
        report_id,
        items: vec![item(
            &[usage(0xff00, vendor_usage)],
            8,
            count,
            0,
            255,
            &flags,
        )],
    }
}

/// The DualSense's USB collections: input report 0x01, output report 0x02
/// and its feature reports.
pub fn dualsense_collections() -> Vec<HidCollection> {
    use HidItemFlag::{NullState, Variable};
    let data = [Variable];
    let mut hat = item(&[usage(0x01, 0x39)], 4, 1, 0, 7, &[Variable, NullState]);
    hat.physical_maximum = 315;
    hat.unit_system = HidUnitSystem::EnglishRotation;
    let mut buttons = item(&[], 1, 15, 0, 1, &data);
    buttons.usage_range = Some(HidUsageRange {
        minimum: usage(0x09, 0x01),
        maximum: usage(0x09, 0x0f),
    });
    let input = HidReportInfo {
        report_id: 0x01,
        items: vec![
            item(
                &[
                    usage(0x01, 0x30),
                    usage(0x01, 0x31),
                    usage(0x01, 0x32),
                    usage(0x01, 0x35),
                    usage(0x01, 0x33),
                    usage(0x01, 0x34),
                ],
                8,
                6,
                0,
                255,
                &data,
            ),
            item(&[usage(0xff00, 0x20)], 8, 1, 0, 255, &data),
            hat,
            buttons,
            item(&[usage(0xff00, 0x21)], 1, 13, 0, 1, &data),
            item(&[usage(0xff00, 0x22)], 8, 52, 0, 255, &data),
        ],
    };
    let features = [
        (0x05, 0x33, 40),
        (0x08, 0x34, 47),
        (0x09, 0x24, 19),
        (0x0a, 0x25, 26),
        (0x20, 0x26, 63),
        (0x21, 0x27, 4),
        (0x22, 0x40, 63),
        (0x80, 0x28, 63),
        (0x81, 0x29, 63),
        (0x82, 0x2a, 9),
        (0x83, 0x2b, 63),
        (0x84, 0x2c, 63),
        (0x85, 0x2d, 2),
        (0xa0, 0x2e, 1),
        (0xe0, 0x2f, 63),
        (0xf0, 0x30, 63),
        (0xf1, 0x31, 63),
        (0xf2, 0x32, 15),
        (0xf4, 0x35, 63),
        (0xf5, 0x36, 3),
    ];
    vec![HidCollection {
        usage_page: 0x01,
        usage: 0x05,
        kind: 1,
        children: Vec::new(),
        input_reports: vec![input],
        output_reports: vec![vendor_report(0x02, 0x23, 47, true)],
        feature_reports: features
            .iter()
            .map(|&(id, vendor_usage, count)| vendor_report(id, vendor_usage, count, false))
            .collect(),
    }]
}

/// An input report 0x01 of a DualSense over USB at rest (63 bytes).
pub fn dualsense_usb_input() -> Vec<u8> {
    let mut data = vec![0; 63];
    data[..4].copy_from_slice(&[0x80, 0x7f, 0x81, 0x80]);
    data[7] = 0x08; // hat centered
    data
}

/// The Gamepad API's 17 standard buttons and 4 axes.
fn gamepad_api_layout() -> RawLayout {
    RawLayout {
        buttons: (0..17).map(RawSource::Index).collect(),
        axes: (0..4)
            .map(|index| RawAxisInfo {
                source: RawSource::Index(index),
                min: -1.0,
                max: 1.0,
            })
            .collect(),
        ..RawLayout::default()
    }
}

/// The web report of `data/reports/example.json`.
pub fn web_report() -> Report {
    let host = Host {
        os: Os::Windows,
        os_version: None,
        browser: Some(CHROME),
    };
    let mut report = Report::new(source(App::Web), host, day());
    report.user = UserStatement {
        product: Some("DualSense (PS5 controller)".into()),
        mode: None,
    };

    let gamepad = DeviceIdentity::from_gamepad_api(
        "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
        "standard",
        CHROME,
        Os::Windows,
    );
    let mut view = View::new(
        gamepad,
        gamepad_api_layout(),
        OutputSet::from([Output::Rumble]),
    );
    view.capabilities = Some(Capabilities::standard());
    let mut confirmed = Mapping::default();
    confirmed
        .buttons
        .insert(Button::South, Input::Button(RawSource::Index(0)));
    confirmed
        .buttons
        .insert(Button::Home, Input::Button(RawSource::Index(16)));
    confirmed
        .axes
        .insert(Axis::LeftStickY, AxisMapping::full("a1~".parse().unwrap()));
    view.confirmed = Some(confirmed);
    report.views.push(view);

    let hid = DeviceIdentity {
        transport: Transport::Usb,
        vendor_id: Some(0x054c),
        product_id: Some(0x0ce6),
        product_name: Some("DualSense Wireless Controller".into()),
        browser: Some(CHROME),
        ..DeviceIdentity::new(ProviderKind::Web, Backend::WebHid, Os::Windows)
    };
    let mut view = View::new(hid, RawLayout::default(), OutputSet::EMPTY);
    let mut data = HidData {
        collections: dualsense_collections(),
        ..HidData::default()
    };
    data.sample(0x01, &dualsense_usb_input());
    view.hid = Some(data);
    report.views.push(view);
    report
}

/// A short HID report descriptor (Generic Desktop, Game Pad, one 8-bit
/// axis).
pub const DESCRIPTOR: [u8; 16] = [
    0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, 0x09, 0x30, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0xc0,
];

/// A desktop report of a DualSense on Linux, read by the native provider,
/// SDL3 and gilrs.
pub fn desktop_report() -> Report {
    let host = Host {
        os: Os::Linux,
        os_version: Some("6.8.0-45-generic".into()),
        browser: None,
    };
    let mut report = Report::new(source(App::Desktop), host, day());
    report.user = UserStatement {
        product: Some("DualSense".into()),
        mode: Some(ControllerMode::Ps5),
    };

    let base = DeviceIdentity {
        transport: Transport::Bluetooth,
        vendor_id: Some(0x054c),
        product_id: Some(0x0ce6),
        version: Some(0x8111),
        product_name: Some("DualSense Wireless Controller".into()),
        ..DeviceIdentity::new(ProviderKind::Native, Backend::Hidraw, Os::Linux)
    };

    let native = DeviceIdentity {
        driver: Some("hid-playstation".into()),
        descriptor_hash: Some(DescriptorHash::of(&DESCRIPTOR)),
        ..base.clone()
    };
    let layout = RawLayout {
        buttons: vec![
            RawSource::Button(Button::South),
            RawSource::Button(Button::East),
        ],
        axes: vec![RawAxisInfo {
            source: RawSource::Axis(Axis::LeftStickX),
            min: -1.0,
            max: 1.0,
        }],
        sensors: SensorSet::from([Sensor::Gyroscope, Sensor::Accelerometer]),
        touchpads: vec![TouchpadInfo {
            width: 1920,
            height: 1080,
            max_fingers: 2,
        }],
        battery: true,
        ..RawLayout::default()
    };
    let mut view = View::new(
        native,
        layout,
        OutputSet::from([Output::Rumble, Output::Lightbar, Output::PlayerLeds]),
    );
    let mut data = HidData {
        descriptor: Some(anygamepad_report::HexBytes(DESCRIPTOR.to_vec())),
        collections: dualsense_collections(),
        ..HidData::default()
    };
    data.sample(0x31, &[0x11; 77]);
    view.hid = Some(data);
    report.views.push(view);

    let sdl = DeviceIdentity {
        provider: ProviderKind::Sdl3,
        ..base.clone()
    };
    report
        .views
        .push(View::new(sdl, RawLayout::default(), OutputSet::EMPTY));

    let gilrs = DeviceIdentity {
        provider: ProviderKind::Gilrs,
        backend: Backend::Evdev,
        driver: Some("hid-playstation".into()),
        ..base
    };
    report.views.push(View::new(
        gilrs,
        RawLayout {
            buttons: vec![RawSource::Evdev(0x130), RawSource::Evdev(0x131)],
            ..RawLayout::default()
        },
        OutputSet::from([Output::Rumble]),
    ));
    report
}

/// Every button of the standard layout as confirmed, for size tests.
pub fn full_confirmed_mapping() -> Mapping {
    let buttons: BTreeMap<Button, Input> = Capabilities::standard()
        .buttons
        .iter()
        .enumerate()
        .map(|(index, button)| (button, Input::Button(RawSource::Index(index as u16))))
        .collect();
    Mapping {
        buttons,
        ..Mapping::default()
    }
}
