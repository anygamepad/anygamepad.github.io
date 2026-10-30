//! The emulated PlayStation pads, read back by the `anygamepad` crate's
//! decoder (C15): what a game reading the real pads would see.

use anygamepad::protocol::playstation::{Model, ReportForm, decode, transport_from_collections};
use anygamepad_model::hid::{HidCollection, HidReportInfo, HidReportItem};
use anygamepad_model::{
    Axis, Button, GamepadState, MotionSample, Output, OutputCommand, Power, PowerState, Rgb,
    Rumble, STANDARD_GRAVITY, TouchPoint, Transport, Vec3,
};
use anygamepad_virtual::Target;
use anygamepad_virtual::descriptor::{ReportKind, reports};

const TARGETS: [(Target, Model); 2] = [
    (Target::DualShock4, Model::DualShock4),
    (Target::DualSense, Model::DualSense),
];

/// A state with something in every field the pads report.
fn busy_state(target: Target) -> GamepadState {
    let mut state = GamepadState::new();
    for button in [
        Button::South,
        Button::North,
        Button::DPadDown,
        Button::DPadLeft,
        Button::RightShoulder,
        Button::LeftTrigger,
        Button::Start,
        Button::RightStick,
        Button::Home,
        Button::TouchpadClick,
    ] {
        state.set_pressed(button, true);
    }
    if target == Target::DualSense {
        state.set_pressed(Button::Mute, true);
    }
    state.set_axis(Axis::LeftStickX, -1.0);
    state.set_axis(Axis::LeftStickY, 0.5);
    state.set_axis(Axis::RightStickX, 0.25);
    state.set_axis(Axis::RightStickY, -1.0);
    state.set_axis(Axis::LeftTrigger, 1.0);
    state.set_axis(Axis::RightTrigger, 0.2);
    state.touches[1] = Some(TouchPoint {
        touchpad: 0,
        id: 9,
        x: 0.25,
        y: 1.0,
    });
    state.power = Power {
        state: PowerState::Charging,
        percent: Some(45),
    };
    state
}

#[test]
fn decoder_reads_the_emulated_state() {
    for (target, model) in TARGETS {
        let mut pad = target.create(0);
        let state = busy_state(target);
        let report = pad.input_report(&state).to_vec();
        let input = decode(model, report[0], &report[1..]).unwrap();
        assert_eq!(input.form, ReportForm::Usb);
        let decoded = input.state;

        assert_eq!(decoded.buttons, state.buttons, "{target}");
        for axis in Axis::ALL {
            let error = (decoded.axis(axis) - state.axis(axis)).abs();
            assert!(error <= 1.0 / 127.0, "{target} {axis}: {error}");
        }
        let touch = decoded.touches[0].expect("one finger");
        assert_eq!(touch.id, 9);
        assert!((touch.x - 0.25).abs() < 0.001 && touch.y == 1.0);
        assert_eq!(decoded.touches[1], None);
        assert_eq!(decoded.power.state, PowerState::Charging);
        assert_eq!(decoded.power.percent, Some(45));
    }
}

#[test]
fn neutral_state() {
    for (target, model) in TARGETS {
        let mut pad = target.create(0);
        let report = pad.input_report(&GamepadState::new()).to_vec();
        let input = decode(model, report[0], &report[1..]).unwrap();
        assert!(input.state.buttons.is_empty(), "{target}");
        assert_eq!(input.state.active_touches().count(), 0);
        // A full pad on its cable.
        assert_eq!(input.state.power.state, PowerState::Charged);
        // Lying flat: one g up.
        let imu = input.imu.unwrap();
        assert_eq!(imu.gyro, [0, 0, 0]);
        assert_eq!(imu.accel, [0, 8192, 0]);
    }
}

#[test]
fn motion_follows_the_nominal_calibration() {
    for (target, model) in TARGETS {
        let mut pad = target.create(0);
        let mut state = GamepadState::new();
        state.motion = Some(MotionSample {
            timestamp_us: 1_600,
            gyro: Some(Vec3::new(1.0, -2.0, 0.5)),
            accel: Some(Vec3::new(0.0, 0.0, -2.0 * STANDARD_GRAVITY)),
        });
        let report = pad.input_report(&state).to_vec();
        let imu = decode(model, report[0], &report[1..]).unwrap().imu.unwrap();
        // 1 rad/s = 57.3 °/s = 916.7 counts.
        assert_eq!(imu.gyro, [917, -1833, 458], "{target}");
        assert_eq!(imu.accel, [0, 0, -16384]);
        let clock = match target {
            Target::DualShock4 => 300,
            _ => 4_800,
        };
        assert_eq!(imu.timestamp, clock);
    }
}

#[test]
fn reports_match_the_descriptor() {
    for (target, _) in TARGETS {
        let mut pad = target.create(1);
        let layouts = reports(pad.report_descriptor().unwrap()).unwrap();
        let input = layouts
            .iter()
            .find(|layout| layout.kind == ReportKind::Input)
            .unwrap();
        // The size that tells USB pads apart (C15).
        assert_eq!(input.bits, 504, "{target}");
        let report = pad.input_report(&GamepadState::new());
        assert_eq!(report.len(), 1 + input.len());
        assert_eq!(report[0], input.report_id);

        let mut features = 0;
        for layout in &layouts {
            match layout.kind {
                ReportKind::Feature => {
                    let reply = pad.feature_report(layout.report_id).unwrap();
                    assert_eq!(reply.len(), 1 + layout.len(), "{target} {layout:?}");
                    features += 1;
                }
                ReportKind::Output => {
                    let mut output = vec![0; 1 + layout.len()];
                    output[0] = layout.report_id;
                    assert!(pad.output_report(&output).is_empty());
                }
                ReportKind::Input => {}
            }
        }
        assert_eq!(features, 3);
    }
}

#[test]
fn decoder_sees_a_usb_pad() {
    for (target, model) in TARGETS {
        let pad = target.create(0);
        let layouts = reports(pad.report_descriptor().unwrap()).unwrap();
        // The structure WebHID would give, reduced to report sizes.
        let input_reports = layouts
            .iter()
            .filter(|layout| layout.kind == ReportKind::Input)
            .map(|layout| HidReportInfo {
                report_id: layout.report_id,
                items: vec![HidReportItem {
                    report_size: 8,
                    report_count: layout.len() as u16,
                    ..HidReportItem::default()
                }],
            })
            .collect();
        let collection = HidCollection {
            usage_page: 0x01,
            usage: 0x05,
            kind: 1,
            input_reports,
            ..HidCollection::default()
        };
        assert_eq!(
            transport_from_collections(model, &[collection]),
            Transport::Usb,
            "{target}"
        );
    }
}

#[test]
fn dualshock4_output() {
    let mut pad = Target::DualShock4.create(0);
    let mut report = [0u8; 32];
    report[0] = 0x05;
    report[1] = 0x03;
    report[4] = 0x80; // weak motor
    report[5] = 0xff; // strong motor
    report[6..9].copy_from_slice(&[1, 2, 3]);
    assert_eq!(
        pad.output_report(&report),
        [
            OutputCommand::Rumble(Rumble {
                low_frequency: 1.0,
                high_frequency: 128.0 / 255.0
            }),
            OutputCommand::Lightbar(Rgb { r: 1, g: 2, b: 3 }),
        ]
    );
    // Too short, or another report.
    assert!(pad.output_report(&report[..20]).is_empty());
    report[0] = 0x11;
    assert!(pad.output_report(&report).is_empty());
}

#[test]
fn dualsense_output() {
    let mut pad = Target::DualSense.create(0);
    let mut report = [0u8; 48];
    report[0] = 0x02;
    report[1] = 0x02; // haptics select
    report[2] = 0x14; // light bar, player LEDs
    report[3] = 0x40; // right (weak) motor
    report[4] = 0x00; // left (strong) motor
    report[44] = 0x20 | 0x15; // instant, player 3
    report[45..48].copy_from_slice(&[0, 0, 255]);
    assert_eq!(
        pad.output_report(&report),
        [
            OutputCommand::Rumble(Rumble {
                low_frequency: 0.0,
                high_frequency: 64.0 / 255.0
            }),
            OutputCommand::Lightbar(Rgb { r: 0, g: 0, b: 255 }),
            OutputCommand::PlayerNumber(3),
        ]
    );
    // Only the player LEDs, with a pattern no player has.
    report[1] = 0;
    report[2] = 0x10;
    report[44] = 0x11;
    assert!(pad.output_report(&report).is_empty());
}

#[test]
fn capabilities() {
    let ds4 = Target::DualShock4.create(0).capabilities();
    let ds = Target::DualSense.create(0).capabilities();
    assert!(!ds4.has_button(Button::Mute) && ds.has_button(Button::Mute));
    assert!(ds4.has_button(Button::TouchpadClick));
    assert!(!ds4.has_output(Output::PlayerLeds) && ds.has_output(Output::PlayerLeds));
    assert_eq!(ds4.touchpads[0].height, 942);
    assert_eq!(ds.touchpads[0].height, 1080);
    assert!(ds4.battery && ds.battery);
}
