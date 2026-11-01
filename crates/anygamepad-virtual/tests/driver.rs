//! The emulated HID pads driven by the `anygamepad` crate's `HidDriver`
//! (C7), as the hidraw and WebHID backends drive real ones: start, state,
//! motion in SI units and outputs.

use anygamepad::protocol::driver::{HidDriver, Request};
use anygamepad::protocol::switch_pro::encode_rumble;
use anygamepad_model::{
    Axis, Button, GamepadState, MotionSample, OutputCommand, Rgb, Rumble, STANDARD_GRAVITY,
    Transport, Vec3,
};
use anygamepad_virtual::{Target, VirtualPad};

/// Moves everything the driver asks for to the pad and back, at `now_us`;
/// returns the commands the pad saw.
fn pump(driver: &mut HidDriver, pad: &mut dyn VirtualPad, now_us: u64) -> Vec<OutputCommand> {
    let mut commands = Vec::new();
    while let Some(request) = driver.transmit(now_us) {
        match request {
            Request::Output(report) => commands.extend(pad.output_report(&report)),
            Request::ReadFeature(id) => match pad.feature_report(id) {
                Some(reply) => driver.feature(&reply),
                None => driver.feature_failed(),
            },
        }
        while let Some(reply) = pad.pending_input() {
            driver.receive(&reply).unwrap();
        }
    }
    commands
}

/// A driver for `pad`, started and ready.
fn started(pad: &mut dyn VirtualPad, transport: Transport) -> HidDriver {
    let identity = pad.identity();
    let mut driver = HidDriver::new(identity.vendor_id, identity.product_id, transport).unwrap();
    driver.start();
    let mut now = 0;
    while !driver.is_ready() {
        assert!(now < 100_000_000, "{} never gets ready", pad.target());
        pump(&mut driver, pad, now);
        now += 1000;
    }
    driver
}

fn close(a: Vec3, b: Vec3, tolerance: f32) -> bool {
    (a.x - b.x).abs() < tolerance && (a.y - b.y).abs() < tolerance && (a.z - b.z).abs() < tolerance
}

#[test]
fn playstation_pads_through_the_driver() {
    for target in [Target::DualShock4, Target::DualSense] {
        let mut pad = target.create(0);
        let mut driver = started(pad.as_mut(), Transport::Usb);

        let gyro = Vec3::new(1.0, -2.0, 0.5);
        let accel = Vec3::new(0.0, STANDARD_GRAVITY, -STANDARD_GRAVITY);
        let mut state = GamepadState::new();
        state.set_pressed(Button::North, true);
        state.set_pressed(Button::TouchpadClick, true);
        state.set_axis(Axis::RightTrigger, 1.0);
        state.motion = Some(MotionSample {
            timestamp_us: 0,
            gyro: Some(gyro),
            accel: Some(accel),
        });
        let report = pad.input_report(&state).to_vec();
        assert_eq!(driver.receive(&report), Ok(true));
        assert_eq!(driver.state().buttons, state.buttons, "{target}");
        assert_eq!(driver.state().axis(Axis::RightTrigger), 1.0);
        // The calibration the pad gave, not the nominal one.
        let mut samples = Vec::new();
        driver.take_motion(&mut samples);
        let [sample] = samples[..] else {
            panic!("{target}: {samples:?}");
        };
        assert!(close(sample.gyro.unwrap(), gyro, 0.002), "{target}");
        assert!(close(sample.accel.unwrap(), accel, 0.002), "{target}");

        let rumble = Rumble {
            low_frequency: 1.0,
            high_frequency: 0.0,
        };
        let light = Rgb {
            r: 255,
            g: 0,
            b: 64,
        };
        driver.send(OutputCommand::Rumble(rumble));
        driver.send(OutputCommand::Lightbar(light));
        let mut expected = vec![
            OutputCommand::Rumble(rumble),
            OutputCommand::Lightbar(light),
        ];
        if target == Target::DualSense {
            assert!(driver.send(OutputCommand::PlayerNumber(3)));
            expected.push(OutputCommand::PlayerNumber(3));
        } else {
            assert!(!driver.send(OutputCommand::PlayerNumber(3)));
        }
        // One report carries every command.
        assert_eq!(pump(&mut driver, pad.as_mut(), 0), expected, "{target}");
    }
}

#[test]
fn switch_pro_through_the_driver() {
    for transport in [Transport::Usb, Transport::Bluetooth] {
        let mut pad = Target::SwitchPro.create(0);
        let mut driver = started(pad.as_mut(), transport);

        let mut state = GamepadState::new();
        state.set_pressed(Button::Capture, true);
        state.set_pressed(Button::RightTrigger, true);
        state.set_axis(Axis::LeftStickY, 1.0);
        let report = pad.input_report(&state).to_vec();
        assert_eq!(driver.receive(&report), Ok(true));
        assert_eq!(driver.state().buttons, state.buttons, "{transport}");
        assert!((driver.state().axis(Axis::LeftStickY) - 1.0).abs() < 0.001);
        let mut samples = Vec::new();
        driver.take_motion(&mut samples);
        assert_eq!(samples.len(), 3, "three IMU samples per report");

        driver.send(OutputCommand::PlayerNumber(2));
        let commands = pump(&mut driver, pad.as_mut(), 0);
        assert_eq!(commands, [OutputCommand::PlayerNumber(2)], "{transport}");
        let rumble = Rumble {
            low_frequency: 0.0,
            high_frequency: 1.0,
        };
        // The emulated pad does not decode rumble: check the report.
        driver.send(OutputCommand::Rumble(rumble));
        let Some(Request::Output(report)) = driver.transmit(0) else {
            panic!("{transport}: no rumble report");
        };
        assert_eq!(report[0], 0x10);
        assert_eq!(report[2..6], encode_rumble(rumble));
        assert_eq!(driver.transmit(0), None);
    }
}

#[test]
fn a_driver_that_is_not_started_only_decodes() {
    let mut pad = Target::DualSense.create(0);
    let identity = pad.identity();
    let mut driver =
        HidDriver::new(identity.vendor_id, identity.product_id, Transport::Usb).unwrap();
    assert!(pump(&mut driver, pad.as_mut(), 0).is_empty());
    let report = pad.input_report(&GamepadState::new()).to_vec();
    assert_eq!(driver.receive(&report), Ok(true));
    assert!(!driver.is_ready());
}
