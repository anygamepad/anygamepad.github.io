//! Motion sensors, in SI units and in the device frame (ADR 0001).
//!
//! # Device frame
//!
//! Right-handed frame fixed to the controller, as SDL3 defines it for
//! gamepads. Hold the controller in front of you with both hands, face
//! buttons up:
//!
//! - **+X** points to the right,
//! - **+Y** points up, out of the face of the controller,
//! - **+Z** points towards you (from the shoulder buttons to the grips).
//!
//! The gyroscope reports angular velocity around each axis in **rad/s**,
//! positive counter-clockwise when looking from the positive end of the axis
//! towards the origin (right-hand rule):
//!
//! - **+X** (pitch): the far edge (shoulder buttons) goes up,
//! - **+Y** (yaw): the far edge turns to the left, seen from above,
//! - **+Z** (roll): the right side goes up, seen by the player.
//!
//! The accelerometer reports proper acceleration in **m/s²**, gravity
//! included: a controller lying flat and still reads about
//! `(0, +STANDARD_GRAVITY, 0)`.

use crate::macros::{enum_set, named_enum};

/// Standard gravity, in m/s².
pub const STANDARD_GRAVITY: f32 = 9.80665;

/// A vector in the [device frame](self).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct Vec3 {
    /// Component on the X axis (right).
    pub x: f32,
    /// Component on the Y axis (up, out of the face).
    pub y: f32,
    /// Component on the Z axis (towards the player).
    pub z: f32,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// A vector from its components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

named_enum! {
    /// A motion sensor.
    pub enum Sensor: "sensor" {
        /// Angular velocity, in rad/s.
        Gyroscope = "gyroscope",
        /// Proper acceleration including gravity, in m/s².
        Accelerometer = "accelerometer",
    }
}

enum_set! {
    /// A set of [`Sensor`]s: the sensors a device has.
    pub struct SensorSet(u8): Sensor;
}

/// One motion reading.
///
/// Devices report several readings per frame at high rates; providers keep
/// them in order so that gyro input can be integrated without losing motion
/// between polls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct MotionSample {
    /// Time of the reading in microseconds, from the device clock when it has
    /// one. Only differences between samples of the same device are
    /// meaningful.
    pub timestamp_us: u64,
    /// Angular velocity in rad/s, if the device has a gyroscope.
    pub gyro: Option<Vec3>,
    /// Acceleration in m/s², if the device has an accelerometer.
    pub accel: Option<Vec3>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_length() {
        assert_eq!(Vec3::new(3.0, 4.0, 0.0).length(), 5.0);
        assert_eq!(Vec3::ZERO.length(), 0.0);
    }

    #[test]
    fn sensor_set() {
        let set = SensorSet::from([Sensor::Gyroscope]);
        assert!(set.contains(Sensor::Gyroscope));
        assert!(!set.contains(Sensor::Accelerometer));
        assert_eq!(SensorSet::ALL.len(), 2);
    }
}
