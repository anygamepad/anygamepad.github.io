//! Device-level status: power and transport.

use crate::macros::named_enum;

named_enum! {
    /// Power source and charging state.
    pub enum PowerState: "power state" {
        /// Not reported by the device or the provider.
        Unknown = "unknown",
        /// Running on its battery.
        OnBattery = "on_battery",
        /// Has no battery (wired-only device).
        NoBattery = "no_battery",
        /// Battery charging.
        Charging = "charging",
        /// Battery full, on external power.
        Charged = "charged",
    }
}

/// Power status of a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Power {
    /// Power source and charging state.
    pub state: PowerState,
    /// Battery level in percent (0-100), if the device reports it.
    pub percent: Option<u8>,
}

impl Default for Power {
    fn default() -> Self {
        Self {
            state: PowerState::Unknown,
            percent: None,
        }
    }
}

named_enum! {
    /// How a device is connected to the host.
    pub enum Transport: "transport" {
        /// Not known.
        Unknown = "unknown",
        /// USB cable.
        Usb = "usb",
        /// Bluetooth Classic.
        Bluetooth = "bluetooth",
        /// Bluetooth Low Energy.
        BluetoothLe = "bluetooth_le",
        /// Proprietary wireless receiver (Xbox Wireless Adapter, 2.4 GHz
        /// dongles).
        Dongle = "dongle",
    }
}
