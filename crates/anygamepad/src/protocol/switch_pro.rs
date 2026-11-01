//! The Nintendo Switch Pro Controller (C7), sans-IO: the set-up exchange
//! that makes it send full reports, its calibration from SPI flash, the
//! full input report (`0x30`), player lights and rumble.
//!
//! Over USB the pad first takes four USB commands (report `0x80`), then,
//! as over Bluetooth, subcommands in report `0x01`, each answered by a
//! `0x21` report. [`Setup`] walks through the exchange; the backend sends
//! [`Setup::message`] and feeds every input report to [`Setup::receive`],
//! resending the message when no answer comes.
//!
//! Sources: `hid-nintendo` (note 05) and the community's reverse-engineered
//! protocol notes it builds on (dekuNukem's `Nintendo_Switch_Reverse_Engineering`,
//! for the rumble encoding and the calibration layouts), not re-read in
//! this session; the IMU axes follow SDL's HIDAPI driver. Nothing here has
//! been checked against a pad yet.

use anygamepad_model::{
    Axis, Button, GamepadState, MotionSample, Power, PowerState, Rumble, STANDARD_GRAVITY,
    Transport, Vec3,
};

/// Bits of the three button bytes: right side, shared, left side. Face
/// buttons by position: Nintendo `B` is south, `A` east.
const BUTTONS: [(usize, u8, Button); 18] = [
    (0, 0x01, Button::West),
    (0, 0x02, Button::North),
    (0, 0x04, Button::South),
    (0, 0x08, Button::East),
    (0, 0x40, Button::RightShoulder),
    (0, 0x80, Button::RightTrigger),
    (1, 0x01, Button::Select),
    (1, 0x02, Button::Start),
    (1, 0x04, Button::RightStick),
    (1, 0x08, Button::LeftStick),
    (1, 0x10, Button::Home),
    (1, 0x20, Button::Capture),
    (2, 0x01, Button::DPadDown),
    (2, 0x02, Button::DPadUp),
    (2, 0x04, Button::DPadRight),
    (2, 0x08, Button::DPadLeft),
    (2, 0x40, Button::LeftShoulder),
    (2, 0x80, Button::LeftTrigger),
];

/// Rumble data that keeps the motors still.
pub const NEUTRAL_RUMBLE: [u8; 4] = [0x00, 0x01, 0x40, 0x40];

/// SPI flash: factory stick calibration (left then right, 9 bytes each).
const SPI_FACTORY_STICKS: (u32, u8) = (0x603d, 0x12);
/// SPI flash: user stick calibration (magic and 9 bytes, per stick).
const SPI_USER_STICKS: (u32, u8) = (0x8010, 0x16);
/// SPI flash: factory IMU calibration.
const SPI_FACTORY_IMU: (u32, u8) = (0x6020, 0x18);
/// Marks user calibration as present.
const USER_MAGIC: [u8; 2] = [0xb2, 0xa1];

const SUB_SET_REPORT_MODE: u8 = 0x03;
const SUB_SPI_READ: u8 = 0x10;
const SUB_SET_PLAYER_LIGHTS: u8 = 0x30;
const SUB_ENABLE_IMU: u8 = 0x40;
const SUB_ENABLE_VIBRATION: u8 = 0x48;

/// One axis of a stick's calibration: centre and reach on each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickAxis {
    /// Raw value at rest.
    pub center: u16,
    /// Raw distance from the centre to full deflection below it.
    pub below: u16,
    /// Raw distance from the centre to full deflection above it.
    pub above: u16,
}

impl StickAxis {
    /// Used when the flash holds nothing valid: the range anygamepad's
    /// virtual Switch Pro Controller reports.
    pub const DEFAULT: Self = Self {
        center: 2048,
        below: 1600,
        above: 1600,
    };

    /// `raw` in `[-1, 1]`.
    pub fn normalize(self, raw: u16) -> f32 {
        let offset = f32::from(raw) - f32::from(self.center);
        let reach = if offset < 0.0 { self.below } else { self.above };
        if reach == 0 {
            0.0
        } else {
            (offset / f32::from(reach)).clamp(-1.0, 1.0)
        }
    }
}

/// A pad's calibration: sticks (X then Y, left then right) and IMU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    /// Left stick X and Y.
    pub left: [StickAxis; 2],
    /// Right stick X and Y.
    pub right: [StickAxis; 2],
    /// Accelerometer origin and sensitivity per axis (raw frame).
    accel: [(i16, i16); 3],
    /// Gyroscope origin and sensitivity per axis (raw frame).
    gyro: [(i16, i16); 3],
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            left: [StickAxis::DEFAULT; 2],
            right: [StickAxis::DEFAULT; 2],
            accel: [(0, 16384); 3],
            gyro: [(0, 13371); 3],
        }
    }
}

/// Two 12-bit values packed in three bytes.
fn unpack(raw: &[u8]) -> (u16, u16) {
    let a = u16::from(raw[0]) | (u16::from(raw[1] & 0x0f) << 8);
    let b = u16::from(raw[1] >> 4) | (u16::from(raw[2]) << 4);
    (a, b)
}

/// A stick's calibration from its 9 bytes; the left stick lists above,
/// centre, below, the right stick centre, below, above. `None` for erased
/// flash.
fn stick(raw: &[u8], left: bool) -> Option<[StickAxis; 2]> {
    let (a, b, c) = (unpack(&raw[0..3]), unpack(&raw[3..6]), unpack(&raw[6..9]));
    let (above, center, below) = if left { (a, b, c) } else { (c, a, b) };
    let axis = |i: usize| {
        let pick = |pair: (u16, u16)| if i == 0 { pair.0 } else { pair.1 };
        StickAxis {
            center: pick(center),
            below: pick(below),
            above: pick(above),
        }
    };
    let axes = [axis(0), axis(1)];
    let valid = axes
        .iter()
        .all(|axis| axis.center != 0xfff && axis.below != 0 && axis.above != 0);
    valid.then_some(axes)
}

impl Calibration {
    fn read_spi(&mut self, address: u32, data: &[u8]) {
        match address {
            0x603d if data.len() >= 18 => {
                if let Some(axes) = stick(&data[0..9], true) {
                    self.left = axes;
                }
                if let Some(axes) = stick(&data[9..18], false) {
                    self.right = axes;
                }
            }
            0x8010 if data.len() >= 22 => {
                if data[0..2] == USER_MAGIC {
                    if let Some(axes) = stick(&data[2..11], true) {
                        self.left = axes;
                    }
                }
                if data[11..13] == USER_MAGIC {
                    if let Some(axes) = stick(&data[13..22], false) {
                        self.right = axes;
                    }
                }
            }
            0x6020 if data.len() >= 24 => {
                let at = |i: usize| i16::from_le_bytes([data[2 * i], data[2 * i + 1]]);
                for axis in 0..3 {
                    let (accel, gyro) = ((at(axis), at(3 + axis)), (at(6 + axis), at(9 + axis)));
                    if accel.1 != accel.0 {
                        self.accel[axis] = accel;
                    }
                    if gyro.1 != gyro.0 {
                        self.gyro[axis] = gyro;
                    }
                }
            }
            _ => {}
        }
    }

    /// Gyroscope (rad/s) and accelerometer (m/s²) of one raw IMU sample
    /// (accelerometer X, Y, Z, then gyroscope X, Y, Z), in the canonical
    /// frame.
    pub fn motion(&self, raw: [i16; 6]) -> (Vec3, Vec3) {
        let accel = |i: usize| {
            let (origin, sensitivity) = self.accel[i];
            f32::from(raw[i]) * 4.0 / (f32::from(sensitivity) - f32::from(origin))
                * STANDARD_GRAVITY
        };
        let gyro = |i: usize| {
            let (origin, sensitivity) = self.gyro[i];
            (f32::from(raw[3 + i]) - f32::from(origin)) * 936.0
                / (f32::from(sensitivity) - f32::from(origin))
                * core::f32::consts::PI
                / 180.0
        };
        // The pad's X points away from the player, Y left, Z up.
        let frame = |x: f32, y: f32, z: f32| Vec3::new(-y, z, -x);
        (
            frame(gyro(0), gyro(1), gyro(2)),
            frame(accel(0), accel(1), accel(2)),
        )
    }
}

/// A message to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// A USB command (report `0x80`), and whether the pad answers it.
    Usb(u8, bool),
    /// A subcommand, its argument bytes.
    Sub(u8, [u8; 5], usize),
}

/// Writes output reports: subcommands and rumble, with the packet counter
/// the pad expects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Writer {
    counter: u8,
    rumble: [u8; 4],
}

impl Writer {
    /// A writer whose rumble data is neutral.
    pub fn new() -> Self {
        Self {
            counter: 0,
            rumble: NEUTRAL_RUMBLE,
        }
    }

    fn header(&mut self, report_id: u8) -> Vec<u8> {
        let mut report = vec![0; 49];
        report[0] = report_id;
        report[1] = self.counter;
        self.counter = (self.counter + 1) & 0x0f;
        // The same rumble on the left and right halves of the data.
        report[2..6].copy_from_slice(&self.rumble);
        report[6..10].copy_from_slice(&self.rumble);
        report
    }

    /// A subcommand report (`0x01`) with the current rumble.
    pub fn subcommand(&mut self, id: u8, args: &[u8]) -> Vec<u8> {
        let mut report = self.header(0x01);
        report[10] = id;
        report[11..11 + args.len()].copy_from_slice(args);
        report
    }

    /// Sets the rumble and returns the report (`0x10`) that plays it.
    pub fn rumble(&mut self, rumble: Rumble) -> Vec<u8> {
        self.rumble = encode_rumble(rumble);
        self.header(0x10)
    }

    /// The subcommand that shows player `player` (1-4 light one LED each,
    /// 0 turns them off).
    pub fn player_lights(&mut self, player: u8) -> Vec<u8> {
        let lights = match player {
            0 => 0,
            n => 1 << ((n - 1) % 4),
        };
        self.subcommand(SUB_SET_PLAYER_LIGHTS, &[lights])
    }
}

/// Encodes rumble: the low-frequency motor on the low band at 160 Hz, the
/// high-frequency motor on the high band at 320 Hz.
pub fn encode_rumble(rumble: Rumble) -> [u8; 4] {
    fn amplitude(value: f32) -> u16 {
        let value = value.clamp(0.0, 1.0);
        if value <= 0.0 {
            return 0;
        }
        let encoded = if value > 0.23 {
            (value * 8.7).log2() * 32.0
        } else if value > 0.12 {
            (value * 17.0).log2() * 16.0
        } else {
            (value.log2() * 32.0 - 96.0) / (4.0 - 2.0 * value)
        };
        encoded.round().max(0.0) as u16
    }
    let frequency = |hz: f32| ((hz / 10.0).log2() * 32.0).round() as u16;
    let high = (frequency(320.0) - 0x60) * 4;
    let low = frequency(160.0) - 0x40;
    let high_amp = amplitude(rumble.high_frequency) * 2;
    let low_amp = amplitude(rumble.low_frequency) / 2 + 64;
    [
        (high & 0xff) as u8,
        (high_amp + (high >> 8)) as u8,
        (low + (low_amp >> 8)) as u8,
        (low_amp & 0xff) as u8,
    ]
}

/// The set-up exchange (see the [module documentation](self)).
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    steps: Vec<Step>,
    next: usize,
    writer: Writer,
    calibration: Calibration,
}

impl Setup {
    /// The exchange for a pad on `transport`, ending with `player`'s
    /// lights.
    pub fn new(transport: Transport, player: u8) -> Self {
        let mut steps = Vec::new();
        if transport == Transport::Usb {
            // Handshake, 3 Mbit/s, handshake again, no Bluetooth timeout
            // (unanswered).
            steps.extend([
                Step::Usb(0x02, true),
                Step::Usb(0x03, true),
                Step::Usb(0x02, true),
                Step::Usb(0x04, false),
            ]);
        }
        let spi = |(address, len): (u32, u8)| {
            let a = address.to_le_bytes();
            Step::Sub(SUB_SPI_READ, [a[0], a[1], a[2], a[3], len], 5)
        };
        let lights = match player {
            0 => 0,
            n => 1 << ((n - 1) % 4),
        };
        steps.extend([
            spi(SPI_FACTORY_STICKS),
            spi(SPI_USER_STICKS),
            spi(SPI_FACTORY_IMU),
            Step::Sub(SUB_SET_REPORT_MODE, [0x30, 0, 0, 0, 0], 1),
            Step::Sub(SUB_ENABLE_IMU, [0x01, 0, 0, 0, 0], 1),
            Step::Sub(SUB_ENABLE_VIBRATION, [0x01, 0, 0, 0, 0], 1),
            Step::Sub(SUB_SET_PLAYER_LIGHTS, [lights, 0, 0, 0, 0], 1),
        ]);
        Self {
            steps,
            next: 0,
            writer: Writer::new(),
            calibration: Calibration::default(),
        }
    }

    /// The message to send now; `None` once the exchange is over. Messages
    /// the pad does not answer count as done once returned.
    pub fn message(&mut self) -> Option<Vec<u8>> {
        let step = *self.steps.get(self.next)?;
        Some(match step {
            Step::Usb(command, answered) => {
                if !answered {
                    self.next += 1;
                }
                let mut report = vec![0; 2];
                report[0] = 0x80;
                report[1] = command;
                report
            }
            Step::Sub(id, args, len) => self.writer.subcommand(id, &args[..len]),
        })
    }

    /// Takes an input report (report ID first); moves on when it answers
    /// the current message.
    pub fn receive(&mut self, report: &[u8]) {
        let Some(&step) = self.steps.get(self.next) else {
            return;
        };
        let answered = match (step, report) {
            (Step::Usb(command, _), [0x81, echo, ..]) => *echo == command,
            // ACK bit, then the subcommand it answers.
            (Step::Sub(id, _, _), [0x21, rest @ ..])
                if rest.len() > 14 && rest[12] & 0x80 != 0 && rest[13] == id =>
            {
                if id == SUB_SPI_READ && rest.len() >= 19 {
                    let address = u32::from_le_bytes([rest[14], rest[15], rest[16], rest[17]]);
                    let len = usize::from(rest[18]);
                    if let Some(data) = rest.get(19..19 + len) {
                        self.calibration.read_spi(address, data);
                    }
                }
                true
            }
            _ => false,
        };
        if answered {
            self.next += 1;
        }
    }

    /// How many messages were answered (or sent, for those the pad does
    /// not answer): it changes when the exchange moves on.
    pub fn progress(&self) -> usize {
        self.next
    }

    /// Whether every message was answered.
    pub fn is_done(&self) -> bool {
        self.next >= self.steps.len()
    }

    /// The calibration read so far (defaults for what was not).
    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }

    /// The writer, to keep its packet counter after the exchange.
    pub fn into_writer(self) -> Writer {
        self.writer
    }
}

/// One decoded full report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    /// Buttons, sticks and power. Triggers are digital (the trigger
    /// buttons).
    pub state: GamepadState,
    /// The three IMU samples of the report, oldest first, raw (accelerometer
    /// X, Y, Z, gyroscope X, Y, Z); see [`Calibration::motion`].
    pub imu: [[i16; 6]; 3],
}

/// Decodes a full input report (`0x30`, report ID first); `None` for other
/// reports.
pub fn decode(report: &[u8], calibration: &Calibration) -> Option<Input> {
    let [0x30, d @ ..] = report else {
        return None;
    };
    if d.len() < 48 {
        return None;
    }
    let mut state = GamepadState::new();
    for (byte, mask, button) in BUTTONS {
        state.set_pressed(button, d[2 + byte] & mask != 0);
    }
    let (lx, ly) = unpack(&d[5..8]);
    let (rx, ry) = unpack(&d[8..11]);
    state.set_axis(Axis::LeftStickX, calibration.left[0].normalize(lx));
    state.set_axis(Axis::LeftStickY, calibration.left[1].normalize(ly));
    state.set_axis(Axis::RightStickX, calibration.right[0].normalize(rx));
    state.set_axis(Axis::RightStickY, calibration.right[1].normalize(ry));
    // Level 0, 2, 4, 6, 8 (full) and a charging bit in the high nibble.
    let battery = d[1] >> 4;
    let percent = Some(((battery >> 1).min(4)) * 25);
    state.power = Power {
        state: if battery & 1 != 0 {
            PowerState::Charging
        } else {
            PowerState::OnBattery
        },
        percent,
    };
    let mut imu = [[0i16; 6]; 3];
    for (sample, raw) in imu.iter_mut().zip(d[12..48].chunks_exact(12)) {
        for (value, bytes) in sample.iter_mut().zip(raw.chunks_exact(2)) {
            *value = i16::from_le_bytes([bytes[0], bytes[1]]);
        }
    }
    Some(Input { state, imu })
}

/// Motion samples of the reports' IMU readings: three per report, 5 ms
/// apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionTracker {
    timestamp_us: u64,
}

impl MotionTracker {
    /// The samples of one report.
    pub fn samples(&mut self, calibration: &Calibration, input: &Input) -> [MotionSample; 3] {
        input.imu.map(|raw| {
            let (gyro, accel) = calibration.motion(raw);
            self.timestamp_us += 5000;
            MotionSample {
                timestamp_us: self.timestamp_us,
                gyro: Some(gyro),
                accel: Some(accel),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(a: u16, b: u16) -> [u8; 3] {
        [
            a as u8,
            ((a >> 8) as u8 & 0x0f) | ((b as u8 & 0x0f) << 4),
            (b >> 4) as u8,
        ]
    }

    /// The `0x21` answer to a subcommand.
    fn reply(id: u8, data: &[u8]) -> Vec<u8> {
        let mut report = vec![0; 64];
        report[0] = 0x21;
        report[13] = 0x80;
        report[14] = id;
        report[15..15 + data.len()].copy_from_slice(data);
        report
    }

    #[test]
    fn neutral_rumble_and_full_strength() {
        assert_eq!(encode_rumble(Rumble::default()), NEUTRAL_RUMBLE);
        let full = encode_rumble(Rumble {
            low_frequency: 1.0,
            high_frequency: 1.0,
        });
        // Amplitude 100: 200 on the high band, 114 on the low band.
        assert_eq!(full, [0x00, 0xc9, 0x40, 0x72]);
    }

    #[test]
    fn usb_setup_reads_the_calibration() {
        let mut setup = Setup::new(Transport::Usb, 1);
        for command in [0x02, 0x03, 0x02] {
            assert_eq!(setup.message(), Some(vec![0x80, command]));
            setup.receive(&[0x21]);
            assert_eq!(
                setup.message(),
                Some(vec![0x80, command]),
                "not answered yet"
            );
            setup.receive(&[0x81, command]);
        }
        assert_eq!(setup.message(), Some(vec![0x80, 0x04]));

        // Factory sticks: left above 1500, centre 2000, below 1400; right
        // centre 2100, below 1300, above 1700.
        let message = setup.message().unwrap();
        assert_eq!(message[..2], [0x01, 0]);
        assert_eq!(message[10..16], [0x10, 0x3d, 0x60, 0, 0, 0x12]);
        let mut sticks = vec![0x3d, 0x60, 0, 0, 0x12];
        for (a, b) in [
            (1500, 1500),
            (2000, 2000),
            (1400, 1400),
            (2100, 2100),
            (1300, 1300),
            (1700, 1700),
        ] {
            sticks.extend(pack(a, b));
        }
        setup.receive(&reply(0x10, &sticks));
        // No user calibration (erased flash), default IMU calibration.
        setup.message();
        let mut user = vec![0x10, 0x80, 0, 0, 0x16];
        user.extend([0xff; 22]);
        setup.receive(&reply(0x10, &user));
        setup.message();
        let mut imu = vec![0x20, 0x60, 0, 0, 0x18];
        imu.extend([0; 24]);
        setup.receive(&reply(0x10, &imu));
        for id in [0x03, 0x40, 0x48, 0x30] {
            let message = setup.message().unwrap();
            assert_eq!(message[10], id);
            setup.receive(&reply(id, &[]));
        }
        assert!(setup.is_done());
        assert_eq!(setup.message(), None);
        let calibration = setup.calibration();
        assert_eq!(
            calibration.left[0],
            StickAxis {
                center: 2000,
                below: 1400,
                above: 1500
            }
        );
        assert_eq!(
            calibration.right[1],
            StickAxis {
                center: 2100,
                below: 1300,
                above: 1700
            }
        );
        // The IMU keeps its defaults.
        assert_eq!(calibration.accel, Calibration::default().accel);
    }

    #[test]
    fn bluetooth_setup_skips_usb_commands() {
        let mut setup = Setup::new(Transport::Bluetooth, 2);
        assert_eq!(setup.message().unwrap()[0], 0x01);
        // A reply to another subcommand, or a NACK, does not count.
        setup.receive(&reply(0x03, &[]));
        let mut nack = reply(0x10, &[]);
        nack[13] = 0;
        setup.receive(&nack);
        assert_eq!(setup.message().unwrap()[10..12], [0x10, 0x3d]);
    }

    #[test]
    fn decodes_full_reports() {
        let mut report = vec![0u8; 64];
        report[0] = 0x30;
        report[2] = 0x71; // level 6, charging
        report[3] = 0x0c; // B, A
        report[4] = 0x20; // capture
        report[5] = 0x82; // ZL, up
        report[6..9].copy_from_slice(&pack(2048, 2048 + 1600));
        report[9..12].copy_from_slice(&pack(2048 - 800, 2048));
        // First IMU sample: accel Z = 4096 (1 g with the defaults).
        report[17..19].copy_from_slice(&4096i16.to_le_bytes());
        let calibration = Calibration::default();
        let input = decode(&report, &calibration).unwrap();
        let state = input.state;
        for button in [
            Button::South,
            Button::East,
            Button::Capture,
            Button::LeftTrigger,
            Button::DPadUp,
        ] {
            assert!(state.pressed(button), "{button}");
        }
        assert_eq!(state.buttons.len(), 5);
        assert_eq!(state.axis(Axis::LeftStickY), 1.0);
        assert_eq!(state.axis(Axis::RightStickX), -0.5);
        assert_eq!(
            state.power,
            Power {
                state: PowerState::Charging,
                percent: Some(75)
            }
        );
        let samples = MotionTracker::default().samples(&calibration, &input);
        // The pad's Z (up) is the canonical Y.
        let accel = samples[0].accel.unwrap();
        assert!((accel.y - STANDARD_GRAVITY).abs() < 1e-3, "{accel:?}");
        assert_eq!(samples[2].timestamp_us, 15000);
        assert_eq!(decode(&[0x21; 64], &calibration), None);
    }

    #[test]
    fn writer_counts_packets() {
        let mut writer = Writer::new();
        let first = writer.player_lights(3);
        assert_eq!(first[..2], [0x01, 0]);
        assert_eq!(first[10..12], [0x30, 0x04]);
        let rumble = writer.rumble(Rumble {
            low_frequency: 0.0,
            high_frequency: 0.0,
        });
        assert_eq!(rumble[..6], [0x10, 1, 0x00, 0x01, 0x40, 0x40]);
        for _ in 0..14 {
            writer.player_lights(0);
        }
        assert_eq!(writer.player_lights(0)[1], 0);
    }
}
