//! Motion calibration of the DualShock 4 and DualSense (C7): the
//! calibration feature report, and raw IMU counts to SI units.
//!
//! The scaling follows `hid-playstation`: the gyroscope is
//! `raw * speed_2x / (|plus - bias| + |minus - bias|)` in °/s with the bias
//! left to the pad, the accelerometer `(raw - centre) * 2 / (plus - minus)`
//! in g. Axes are the canonical device frame (SDL's, which the pads use).

use anygamepad_model::{MotionSample, STANDARD_GRAVITY, Transport, Vec3};

use super::{Model, RawImu};

/// Scale of one axis: `value = (raw - bias) * numerator / denominator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scale {
    bias: i32,
    numerator: i32,
    denominator: i32,
}

impl Scale {
    fn apply(self, raw: i16) -> f32 {
        (i32::from(raw) - self.bias) as f32 * self.numerator as f32 / self.denominator as f32
    }
}

/// A pad's motion calibration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calibration {
    /// Gyroscope pitch, yaw, roll, to °/s.
    gyro: [Scale; 3],
    /// Accelerometer X, Y, Z, to g.
    accel: [Scale; 3],
}

impl Calibration {
    /// The nominal scales (16 counts per °/s, 8192 counts per g), used when
    /// a report is missing or invalid.
    pub const NOMINAL: Self = Self {
        gyro: [Scale {
            bias: 0,
            numerator: 1,
            denominator: 16,
        }; 3],
        accel: [Scale {
            bias: 0,
            numerator: 1,
            denominator: 8192,
        }; 3],
    };

    /// The calibration report's ID: `0x02` for the DualShock 4 on USB,
    /// `0x05` otherwise. Reading it over Bluetooth switches the pad to its
    /// full report (see the [module documentation](super)).
    pub const fn report_id(model: Model, transport: Transport) -> u8 {
        match (model, transport) {
            (Model::DualShock4, Transport::Usb) => 0x02,
            _ => 0x05,
        }
    }

    /// Parses the calibration report (`data` without its ID). Axes whose
    /// values make no sense keep the nominal scale, as `hid-playstation`
    /// does.
    pub fn parse(model: Model, transport: Transport, data: &[u8]) -> Self {
        let mut calibration = Self::NOMINAL;
        if data.len() < 34 {
            return calibration;
        }
        let at = |i: usize| i32::from(i16::from_le_bytes([data[2 * i], data[2 * i + 1]]));
        // Gyroscope plus and minus readings: interleaved per axis, except
        // the DualShock 4 over Bluetooth, which lists the three plus
        // readings first.
        let (plus, minus) = if model == Model::DualShock4 && transport != Transport::Usb {
            ([at(3), at(4), at(5)], [at(6), at(7), at(8)])
        } else {
            ([at(3), at(5), at(7)], [at(4), at(6), at(8)])
        };
        let speed_2x = at(9) + at(10);
        for axis in 0..3 {
            let bias = at(axis);
            let denominator = (plus[axis] - bias).abs() + (minus[axis] - bias).abs();
            if denominator != 0 && speed_2x != 0 {
                calibration.gyro[axis] = Scale {
                    bias: 0,
                    numerator: speed_2x,
                    denominator,
                };
            }
            let (plus, minus) = (at(11 + 2 * axis), at(12 + 2 * axis));
            let range = plus - minus;
            if range != 0 {
                calibration.accel[axis] = Scale {
                    bias: plus - range / 2,
                    numerator: 2,
                    denominator: range,
                };
            }
        }
        calibration
    }

    /// Angular velocity in rad/s and acceleration in m/s² of raw counts.
    pub fn apply(&self, imu: &RawImu) -> (Vec3, Vec3) {
        let radians = core::f32::consts::PI / 180.0;
        let gyro = Vec3::new(
            self.gyro[0].apply(imu.gyro[0]) * radians,
            self.gyro[1].apply(imu.gyro[1]) * radians,
            self.gyro[2].apply(imu.gyro[2]) * radians,
        );
        let accel = Vec3::new(
            self.accel[0].apply(imu.accel[0]) * STANDARD_GRAVITY,
            self.accel[1].apply(imu.accel[1]) * STANDARD_GRAVITY,
            self.accel[2].apply(imu.accel[2]) * STANDARD_GRAVITY,
        );
        (gyro, accel)
    }
}

/// Turns a pad's raw IMU readings into motion samples: calibration, and
/// its sensor clock into microseconds since the first sample.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionTracker {
    model: Model,
    calibration: Calibration,
    last_clock: Option<u32>,
    timestamp_us: f64,
}

impl MotionTracker {
    /// A tracker for a pad of `model` with `calibration`.
    pub const fn new(model: Model, calibration: Calibration) -> Self {
        Self {
            model,
            calibration,
            last_clock: None,
            timestamp_us: 0.0,
        }
    }

    /// The motion sample of one report's readings.
    pub fn sample(&mut self, imu: &RawImu) -> MotionSample {
        // The DualSense counts thirds of a microsecond on 32 bits, the
        // DualShock 4 units of 16/3 µs on 16 bits.
        let (mask, us_per_count) = match self.model {
            Model::DualShock4 => (0xffff, 16.0 / 3.0),
            Model::DualSense | Model::DualSenseEdge => (u32::MAX, 1.0 / 3.0),
        };
        let clock = imu.timestamp & mask;
        if let Some(last) = self.last_clock {
            let elapsed = clock.wrapping_sub(last) & mask;
            self.timestamp_us += f64::from(elapsed) * us_per_count;
        }
        self.last_clock = Some(clock);
        let (gyro, accel) = self.calibration.apply(imu);
        MotionSample {
            timestamp_us: self.timestamp_us as u64,
            gyro: Some(gyro),
            accel: Some(accel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(values: [i16; 17]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn usb_layout() {
        let data = report([
            0, 0, 0, // biases
            8640, -8640, 4320, -4320, 8640, -8640, // pitch, yaw, roll
            540, 540, // speeds
            8192, -8192, 8292, -8092, 4096, -4096, // accel X, Y, Z
        ]);
        let calibration = Calibration::parse(Model::DualSense, Transport::Usb, &data);
        let imu = RawImu {
            gyro: [16, 16, -16],
            accel: [8192, 100, 2048],
            timestamp: 0,
        };
        let (gyro, accel) = calibration.apply(&imu);
        let degree = core::f32::consts::PI / 180.0;
        assert!((gyro.x - degree).abs() < 1e-6);
        // Yaw has half the counts per °/s.
        assert!((gyro.y - 2.0 * degree).abs() < 1e-6);
        assert!((accel.x - STANDARD_GRAVITY).abs() < 1e-4);
        // Y is centred on 100.
        assert!(accel.y.abs() < 1e-4);
        assert!((accel.z - STANDARD_GRAVITY / 2.0).abs() < 1e-4);
    }

    #[test]
    fn dualshock4_bluetooth_lists_plus_first() {
        let data = report([
            0, 0, 0, 8640, 4320, 8640, -8640, -4320, -8640, 540, 540, 8192, -8192, 8192, -8192,
            8192, -8192,
        ]);
        let calibration = Calibration::parse(Model::DualShock4, Transport::Bluetooth, &data);
        let imu = RawImu {
            gyro: [0, 16, 0],
            ..RawImu::default()
        };
        let degree = core::f32::consts::PI / 180.0;
        assert!((calibration.apply(&imu).0.y - 2.0 * degree).abs() < 1e-6);
        assert_eq!(
            Calibration::report_id(Model::DualShock4, Transport::Usb),
            0x02
        );
        assert_eq!(
            Calibration::report_id(Model::DualSense, Transport::Usb),
            0x05
        );
    }

    #[test]
    fn invalid_values_keep_the_nominal_scale() {
        assert_eq!(
            Calibration::parse(Model::DualSense, Transport::Usb, &[0; 40]),
            Calibration::NOMINAL
        );
        assert_eq!(
            Calibration::parse(Model::DualSense, Transport::Usb, &[1; 4]),
            Calibration::NOMINAL
        );
    }

    #[test]
    fn sensor_clocks() {
        let mut dualsense = MotionTracker::new(Model::DualSense, Calibration::NOMINAL);
        let imu = |timestamp| RawImu {
            timestamp,
            ..RawImu::default()
        };
        assert_eq!(dualsense.sample(&imu(u32::MAX - 2)).timestamp_us, 0);
        // Six counts later, across the wrap: 2 µs.
        assert_eq!(dualsense.sample(&imu(3)).timestamp_us, 2);

        let mut dualshock4 = MotionTracker::new(Model::DualShock4, Calibration::NOMINAL);
        dualshock4.sample(&imu(0xfffd));
        // Three counts of 16/3 µs across the 16-bit wrap.
        assert_eq!(dualshock4.sample(&imu(0x0000)).timestamp_us, 16);
    }
}
