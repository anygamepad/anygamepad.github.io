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
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
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

named_enum! {
    /// A mode a controller can be switched to, as printed on mode switches
    /// or told in manuals. Device reports carry the mode the person says a
    /// controller is in; the controller catalog lists the modes of each
    /// model.
    pub enum ControllerMode: "controller mode" {
        /// XInput ("X"): an Xbox 360 controller to Windows.
        Xinput = "xinput",
        /// DirectInput ("D"): a generic HID game controller.
        Dinput = "dinput",
        /// Nintendo Switch ("S"): a Switch Pro Controller.
        Switch = "switch",
        /// PlayStation 3: a DualShock 3.
        Ps3 = "ps3",
        /// PlayStation 4: a DualShock 4.
        Ps4 = "ps4",
        /// PlayStation 5: a DualSense.
        Ps5 = "ps5",
        /// Xbox One or Series controller (GIP), not XInput emulation.
        Xbox = "xbox",
        /// Android ("A").
        Android = "android",
        /// iOS, iPadOS or macOS (MFi).
        Apple = "apple",
        /// Keyboard and mouse emulation.
        KeyboardMouse = "keyboard_mouse",
        /// Another mode.
        Other = "other",
    }
}
