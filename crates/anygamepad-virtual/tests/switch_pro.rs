//! The emulated Switch Pro Controller, set up and read by the `anygamepad`
//! crate's Switch Pro protocol (C7), as a host driver would.

use anygamepad::protocol::switch_pro::{Setup, StickAxis, Writer, decode};
use anygamepad_model::{Axis, Button, GamepadState, OutputCommand, Transport};
use anygamepad_virtual::{STICK_CENTER, STICK_RANGE, Target, VirtualPad};

/// Runs the set-up exchange between C7's host side and the emulated pad.
fn set_up(pad: &mut dyn VirtualPad, transport: Transport) -> Setup {
    let mut setup = Setup::new(transport, 1);
    let mut rounds = 0;
    while let Some(message) = setup.message() {
        rounds += 1;
        assert!(rounds < 50, "the exchange does not end");
        pad.output_report(&message);
        while let Some(reply) = pad.pending_input() {
            setup.receive(&reply);
        }
    }
    assert!(setup.is_done());
    setup
}

#[test]
fn host_sets_up_the_emulated_pad() {
    for transport in [Transport::Usb, Transport::Bluetooth] {
        let mut pad = Target::SwitchPro.create(0);
        let setup = set_up(pad.as_mut(), transport);
        let nominal = StickAxis {
            center: STICK_CENTER,
            below: STICK_RANGE,
            above: STICK_RANGE,
        };
        let calibration = *setup.calibration();
        assert_eq!(calibration.left, [nominal; 2], "{transport}");
        assert_eq!(calibration.right, [nominal; 2]);

        let mut state = GamepadState::new();
        state.set_pressed(Button::East, true);
        state.set_pressed(Button::Capture, true);
        state.set_axis(Axis::LeftStickX, -1.0);
        state.set_axis(Axis::RightStickY, 0.5);
        let report = pad.input_report(&state).to_vec();
        let input = decode(&report, &calibration).unwrap();
        assert_eq!(input.state.buttons, state.buttons);
        for axis in [
            Axis::LeftStickX,
            Axis::LeftStickY,
            Axis::RightStickX,
            Axis::RightStickY,
        ] {
            assert!((input.state.axis(axis) - state.axis(axis)).abs() < 0.001, "{axis}");
        }
    }
}

#[test]
fn player_lights_reach_the_emulated_pad() {
    let mut pad = Target::SwitchPro.create(0);
    let mut writer = Writer::new();
    for player in 0..=4 {
        assert_eq!(
            pad.output_report(&writer.player_lights(player)),
            [OutputCommand::PlayerNumber(player)]
        );
        // The subcommand is acknowledged.
        let reply = pad.pending_input().unwrap();
        assert_eq!((reply[0], reply[14]), (0x21, 0x30));
        assert_eq!(pad.pending_input(), None);
    }
}
