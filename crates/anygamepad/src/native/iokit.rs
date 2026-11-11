//! What the macOS IOKit HID provider reads, sans-IO (C6): which devices it
//! matches and takes, and their identity from IOKit's device properties.
//! The provider itself is `macos::IoHidProvider`, on macOS.
//!
//! # Which devices
//!
//! IOKit lists every HID device whose primary usage is a Generic Desktop
//! Joystick, Game Pad or Multi-axis Controller ([`MATCHING`]). macOS's
//! GameController framework drives the PlayStation, Xbox and Switch Pro
//! pads itself, and switches the PlayStation pads to their extended
//! report, which leaves IOKit readers of the standard report with no input
//! (research note 06). Those pads are the GameController provider's by
//! default, as kernel-driven pads are the evdev provider's on Linux; the
//! IOKit provider takes them only when asked ([`takes`]), and then reads
//! them with their protocol driver ([`HidPad`](super::hid::HidPad)).
//!
//! # Privacy
//!
//! The provider reads the vendor and product IDs, the version, the product
//! and manufacturer names, the transport, the report descriptor and its
//! largest input report size ([`keys`]); never
//! `SerialNumber`, nor the PlayStation pads' pairing and factory reports.
//! The location ID locates the device on the host and is never serialized
//! ([`DeviceLocation`](crate::provider::DeviceLocation)).

use anygamepad_model::identity::{Backend, DescriptorHash, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::{Family, Transport};

use crate::provider::ProviderError;

/// Usage page and usage of the devices the provider matches: Generic
/// Desktop Joystick, Game Pad and Multi-axis Controller.
pub const MATCHING: [(u32, u32); 3] = [(0x01, 0x04), (0x01, 0x05), (0x01, 0x08)];

/// IOKit property keys the provider reads (`IOHIDKeys.h`). Every other key,
/// `SerialNumber` among them, stays unread.
pub mod keys {
    /// `kIOHIDVendorIDKey`.
    pub const VENDOR_ID: &str = "VendorID";
    /// `kIOHIDProductIDKey`.
    pub const PRODUCT_ID: &str = "ProductID";
    /// `kIOHIDVersionNumberKey`.
    pub const VERSION: &str = "VersionNumber";
    /// `kIOHIDProductKey`.
    pub const PRODUCT: &str = "Product";
    /// `kIOHIDManufacturerKey`.
    pub const MANUFACTURER: &str = "Manufacturer";
    /// `kIOHIDTransportKey`.
    pub const TRANSPORT: &str = "Transport";
    /// `kIOHIDReportDescriptorKey`.
    pub const REPORT_DESCRIPTOR: &str = "ReportDescriptor";
    /// `kIOHIDLocationIDKey`: never serialized.
    pub const LOCATION_ID: &str = "LocationID";
    /// `kIOHIDMaxInputReportSizeKey`.
    pub const MAX_INPUT_REPORT_SIZE: &str = "MaxInputReportSize";
    /// `kIOHIDPrimaryUsagePageKey`.
    pub const PRIMARY_USAGE_PAGE: &str = "PrimaryUsagePage";
    /// `kIOHIDPrimaryUsageKey`.
    pub const PRIMARY_USAGE: &str = "PrimaryUsage";
    /// `kIOHIDDeviceUsagePageKey`, in matching dictionaries.
    pub const DEVICE_USAGE_PAGE: &str = "DeviceUsagePage";
    /// `kIOHIDDeviceUsageKey`, in matching dictionaries.
    pub const DEVICE_USAGE: &str = "DeviceUsage";
}

/// The transport of an IOKit `Transport` property (`kIOHIDTransport*Value`).
pub fn transport(value: &str) -> Transport {
    match value {
        "USB" => Transport::Usb,
        "Bluetooth" => Transport::Bluetooth,
        "BluetoothLowEnergy" | "Bluetooth Low Energy" => Transport::BluetoothLe,
        _ => Transport::Unknown,
    }
}

/// `IOReturn` codes the provider tells apart (`IOKit/IOReturn.h`).
pub mod io_return {
    /// `kIOReturnSuccess`.
    pub const SUCCESS: i32 = 0;
    /// `kIOReturnNoDevice`.
    pub const NO_DEVICE: i32 = 0xe000_02c0_u32 as i32;
    /// `kIOReturnNotPrivileged`.
    pub const NOT_PRIVILEGED: i32 = 0xe000_02c1_u32 as i32;
    /// `kIOReturnExclusiveAccess`: another process seized the device.
    pub const EXCLUSIVE_ACCESS: i32 = 0xe000_02c5_u32 as i32;
    /// `kIOReturnNotAttached`.
    pub const NOT_ATTACHED: i32 = 0xe000_02d9_u32 as i32;
    /// `kIOReturnNotPermitted`: macOS's privacy settings (Input
    /// Monitoring) refused.
    pub const NOT_PERMITTED: i32 = 0xe000_02e2_u32 as i32;
    /// `kIOReturnNotResponding`.
    pub const NOT_RESPONDING: i32 = 0xe000_02ed_u32 as i32;
}

/// The provider error of a failed IOKit call.
pub fn error(code: i32) -> ProviderError {
    match code {
        io_return::NO_DEVICE | io_return::NOT_ATTACHED | io_return::NOT_RESPONDING => {
            ProviderError::Disconnected
        }
        io_return::NOT_PRIVILEGED | io_return::NOT_PERMITTED => ProviderError::PermissionDenied,
        io_return::EXCLUSIVE_ACCESS => ProviderError::Busy,
        code => ProviderError::Backend(format!("IOKit error {:#010x}", code as u32)),
    }
}

/// Whether macOS's GameController framework drives the pad with these USB
/// IDs: the PlayStation pads from the DualShock 4 on, Xbox pads and the
/// Switch Pro Controller.
pub fn game_controller_drives(vendor_id: u16, product_id: u16) -> bool {
    Family::from_ids(vendor_id, product_id).is_some_and(|family| family != Family::DualShock3)
}

/// Whether the provider takes a matched device: every pad GameController
/// does not drive, and those too when `take_game_controller_pads` is set.
pub fn takes(vendor_id: u16, product_id: u16, take_game_controller_pads: bool) -> bool {
    take_game_controller_pads || !game_controller_drives(vendor_id, product_id)
}

/// What the provider reads of a device's IOKit properties (see the
/// [module documentation](self) for what it never reads).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HidProperties {
    /// `VendorID`, 0 when missing.
    pub vendor_id: u16,
    /// `ProductID`, 0 when missing.
    pub product_id: u16,
    /// `VersionNumber`.
    pub version: Option<u16>,
    /// `Product`.
    pub product: Option<String>,
    /// `Manufacturer`.
    pub manufacturer: Option<String>,
    /// `Transport`, as the property says it.
    pub transport: Option<String>,
    /// `ReportDescriptor`.
    pub descriptor: Option<Vec<u8>>,
}

impl HidProperties {
    /// The transport, from the `Transport` property.
    pub fn transport(&self) -> Transport {
        self.transport
            .as_deref()
            .map_or(Transport::Unknown, transport)
    }

    /// The device's identity as the IOKit HID backend sees it.
    pub fn identity(&self) -> DeviceIdentity {
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::IoKitHid, Os::MacOs);
        identity.transport = self.transport();
        identity.vendor_id = Some(self.vendor_id).filter(|&id| id != 0);
        identity.product_id = Some(self.product_id).filter(|&id| id != 0);
        identity.version = self.version;
        identity.vendor_name = self.manufacturer.clone().filter(|name| !name.is_empty());
        identity.product_name = self.product.clone().filter(|name| !name.is_empty());
        identity.descriptor_hash = self
            .descriptor
            .as_deref()
            .filter(|descriptor| !descriptor.is_empty())
            .map(DescriptorHash::of);
        identity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transports() {
        assert_eq!(transport("USB"), Transport::Usb);
        assert_eq!(transport("Bluetooth"), Transport::Bluetooth);
        assert_eq!(transport("BluetoothLowEnergy"), Transport::BluetoothLe);
        assert_eq!(transport("SPI"), Transport::Unknown);
    }

    #[test]
    fn errors() {
        assert_eq!(error(io_return::NO_DEVICE), ProviderError::Disconnected);
        assert_eq!(
            error(io_return::NOT_PERMITTED),
            ProviderError::PermissionDenied
        );
        assert_eq!(error(io_return::EXCLUSIVE_ACCESS), ProviderError::Busy);
        assert_eq!(
            error(0xe000_02bc_u32 as i32),
            ProviderError::Backend("IOKit error 0xe00002bc".into())
        );
    }

    #[test]
    fn game_controller_pads_are_left_to_it_by_default() {
        // DualSense, Switch Pro, an Xbox Wireless Controller.
        for (vendor, product) in [(0x054c, 0x0ce6), (0x057e, 0x2009), (0x045e, 0x0b13)] {
            assert!(game_controller_drives(vendor, product));
            assert!(!takes(vendor, product, false));
            assert!(takes(vendor, product, true));
        }
        // A DualShock 3 and an unknown pad.
        for (vendor, product) in [(0x054c, 0x0268), (0x2dc8, 0x6001)] {
            assert!(!game_controller_drives(vendor, product));
            assert!(takes(vendor, product, false));
        }
    }

    #[test]
    fn identity_from_properties() {
        let properties = HidProperties {
            vendor_id: 0x054c,
            product_id: 0x0ce6,
            version: Some(0x0100),
            product: Some("DualSense Wireless Controller".into()),
            manufacturer: Some("Sony Interactive Entertainment".into()),
            transport: Some("Bluetooth".into()),
            descriptor: Some(vec![0x05, 0x01]),
        };
        let identity = properties.identity();
        assert_eq!(identity.backend, Backend::IoKitHid);
        assert_eq!(identity.os, Os::MacOs);
        assert_eq!(identity.transport, Transport::Bluetooth);
        assert_eq!(
            (identity.vendor_id, identity.product_id),
            (Some(0x054c), Some(0x0ce6))
        );
        assert_eq!(identity.version, Some(0x0100));
        assert_eq!(
            identity.product_name.as_deref(),
            Some("DualSense Wireless Controller")
        );
        assert_eq!(
            identity.descriptor_hash,
            Some(DescriptorHash::of(&[0x05, 0x01]))
        );

        let empty = HidProperties::default().identity();
        assert_eq!((empty.vendor_id, empty.product_id), (None, None));
        assert_eq!(empty.transport, Transport::Unknown);
        assert_eq!(empty.descriptor_hash, None);
    }
}
