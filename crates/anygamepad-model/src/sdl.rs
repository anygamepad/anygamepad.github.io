//! SDL joystick GUIDs, to look up SDL GameControllerDB entries and to
//! correlate devices with the SDL3 provider.
//!
//! Layout (`SDL_CreateJoystickGUID`, 16 bytes, 16-bit fields little-endian):
//!
//! | Bytes | Field |
//! |-------|-------|
//! | 0-1 | bus (`0x03` USB, `0x05` Bluetooth, `0xFF` virtual, `0x00` unknown) |
//! | 2-3 | CRC-16 of `"<vendor name> <product name>"` or of the product name |
//! | 4-5, 8-9, 12-13 | vendor ID, product ID, version (6-7 and 10-11 are 0) |
//! | 14, 15 | driver signature (`b'h'` HIDAPI, `b'x'` XInput, ...), driver data |
//!
//! Without a vendor ID, bytes 4 onwards hold the product name instead.

use core::fmt;
use core::str::FromStr;

use crate::hash::crc16;
use crate::{ParseNameError, Transport};

/// SDL bus value for an unknown bus.
pub const SDL_BUS_UNKNOWN: u16 = 0x00;
/// SDL bus value for USB.
pub const SDL_BUS_USB: u16 = 0x03;
/// SDL bus value for Bluetooth (Classic and Low Energy).
pub const SDL_BUS_BLUETOOTH: u16 = 0x05;
/// SDL bus value for virtual devices.
pub const SDL_BUS_VIRTUAL: u16 = 0xff;

/// The SDL bus value of a transport. Dongles are USB devices for SDL.
pub const fn sdl_bus(transport: Transport) -> u16 {
    match transport {
        Transport::Usb | Transport::Dongle => SDL_BUS_USB,
        Transport::Bluetooth | Transport::BluetoothLe => SDL_BUS_BLUETOOTH,
        Transport::Unknown => SDL_BUS_UNKNOWN,
    }
}

/// An SDL joystick GUID.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SdlGuid(pub [u8; 16]);

/// Vendor ID, product ID and version decoded from an [`SdlGuid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SdlGuidIds {
    /// USB/Bluetooth vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Version (often the firmware or `bcdDevice`).
    pub version: u16,
}

/// The SDL name fields of a device, as passed to `SDL_CreateJoystickGUID`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SdlGuidNames<'a> {
    /// Vendor name; only used when the product name is also present.
    pub vendor: Option<&'a str>,
    /// Product name.
    pub product: Option<&'a str>,
}

impl SdlGuid {
    /// Builds a GUID exactly like `SDL_CreateJoystickGUID`.
    ///
    /// A `vendor_id` of 0 selects the name form, where the product name is
    /// stored instead of the IDs.
    pub fn create(
        bus: u16,
        vendor_id: u16,
        product_id: u16,
        version: u16,
        names: SdlGuidNames<'_>,
        driver_signature: u8,
        driver_data: u8,
    ) -> Self {
        let crc = match names {
            SdlGuidNames {
                vendor: Some(vendor),
                product: Some(product),
            } if !vendor.is_empty() && !product.is_empty() => {
                crc16(crc16(crc16(0, vendor.as_bytes()), b" "), product.as_bytes())
            }
            SdlGuidNames {
                product: Some(product),
                ..
            } => crc16(0, product.as_bytes()),
            _ => 0,
        };

        let mut data = [0u8; 16];
        data[0..2].copy_from_slice(&bus.to_le_bytes());
        data[2..4].copy_from_slice(&crc.to_le_bytes());
        if vendor_id != 0 {
            data[4..6].copy_from_slice(&vendor_id.to_le_bytes());
            data[8..10].copy_from_slice(&product_id.to_le_bytes());
            data[12..14].copy_from_slice(&version.to_le_bytes());
            data[14] = driver_signature;
            data[15] = driver_data;
        } else {
            let mut available = 12;
            if driver_signature != 0 {
                available -= 2;
                data[14] = driver_signature;
                data[15] = driver_data;
            }
            if let Some(product) = names.product {
                // `SDL_strlcpy`: at most `available - 1` bytes, then a NUL.
                let len = product.len().min(available - 1);
                data[4..4 + len].copy_from_slice(&product.as_bytes()[..len]);
                data[4 + len] = 0;
            }
        }
        Self(data)
    }

    fn field(&self, index: usize) -> u16 {
        u16::from_le_bytes([self.0[index * 2], self.0[index * 2 + 1]])
    }

    /// The bus value (see the `SDL_BUS_*` constants).
    pub fn bus(&self) -> u16 {
        self.field(0)
    }

    /// The CRC-16 of the device name; 0 when not set.
    pub fn crc(&self) -> u16 {
        self.field(1)
    }

    /// The driver signature byte (`b'h'` HIDAPI, `b'x'` XInput, ...); 0 for
    /// OS backends without one (Linux evdev, ...).
    pub fn driver_signature(&self) -> u8 {
        self.0[14]
    }

    /// The driver-dependent byte.
    pub fn driver_data(&self) -> u8 {
        self.0[15]
    }

    fn bus_is_valid(&self) -> bool {
        let bus = self.bus();
        bus < 0x20 || bus == SDL_BUS_VIRTUAL
    }

    /// The IDs, if the GUID has the vendor/product form; `None` for the name
    /// form and for GUIDs SDL does not understand (as
    /// `SDL_GetJoystickGUIDInfo`).
    pub fn ids(&self) -> Option<SdlGuidIds> {
        if self.bus_is_valid() && self.field(3) == 0 && self.field(5) == 0 {
            Some(SdlGuidIds {
                vendor_id: self.field(2),
                product_id: self.field(4),
                version: self.field(6),
            })
        } else {
            None
        }
    }

    /// This GUID with the CRC field cleared. SDL also matches mappings
    /// without the CRC, and GameControllerDB entries usually have none.
    pub fn without_crc(self) -> Self {
        let mut data = self.0;
        data[2] = 0;
        data[3] = 0;
        Self(data)
    }
}

impl fmt::Display for SdlGuid {
    /// 32 lowercase hexadecimal digits, as in GameControllerDB.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SdlGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SdlGuid({self})")
    }
}

impl FromStr for SdlGuid {
    type Err = ParseNameError;

    /// Parses 32 hexadecimal digits (either case).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseNameError::new("SDL GUID", s);
        if s.len() != 32 || !s.is_ascii() {
            return Err(error());
        }
        let mut data = [0u8; 16];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| error())?;
        }
        Ok(Self(data))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for SdlGuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            self.0.serialize(serializer)
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for SdlGuid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
            s.parse().map_err(serde::de::Error::custom)
        } else {
            <[u8; 16]>::deserialize(deserializer).map(Self)
        }
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for SdlGuid {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SdlGuid".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9a-fA-F]{32}$",
        })
    }
}

crate::macros::ts_decl!("/** 32 hexadecimal digits. */\nexport type SdlGuid = string;");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_evdev_dualshock4_matches_gamecontrollerdb() {
        // GameControllerDB, Linux: "030000004c050000c405000011010000,PS4 Controller,..."
        let guid = SdlGuid::create(
            SDL_BUS_USB,
            0x054c,
            0x05c4,
            0x0111,
            SdlGuidNames::default(),
            0,
            0,
        );
        assert_eq!(guid.to_string(), "030000004c050000c405000011010000");
        assert_eq!(
            guid.ids(),
            Some(SdlGuidIds {
                vendor_id: 0x054c,
                product_id: 0x05c4,
                version: 0x0111,
            })
        );
    }

    #[test]
    fn crc_covers_vendor_and_product_names() {
        let names = SdlGuidNames {
            vendor: Some("Sony"),
            product: Some("Wireless Controller"),
        };
        let guid = SdlGuid::create(SDL_BUS_BLUETOOTH, 0x054c, 0x09cc, 0, names, b'h', 0);
        assert_eq!(guid.crc(), crc16(0, b"Sony Wireless Controller"));
        assert_eq!(guid.bus(), SDL_BUS_BLUETOOTH);
        assert_eq!(guid.driver_signature(), b'h');
        assert_eq!(guid.without_crc().crc(), 0);
        assert_eq!(guid.without_crc().ids(), guid.ids());

        // An empty vendor name falls back to the product name alone.
        let product_only = SdlGuidNames {
            vendor: Some(""),
            product: Some("Wireless Controller"),
        };
        let guid = SdlGuid::create(SDL_BUS_USB, 0x054c, 0x09cc, 0, product_only, 0, 0);
        assert_eq!(guid.crc(), crc16(0, b"Wireless Controller"));
    }

    #[test]
    fn name_form_without_vendor_id() {
        let names = SdlGuidNames {
            vendor: None,
            product: Some("Pro Controller"),
        };
        let guid = SdlGuid::create(SDL_BUS_BLUETOOTH, 0, 0, 0, names, 0, 0);
        assert_eq!(&guid.0[4..16], b"Pro Control\0");
        assert_eq!(guid.ids(), None);

        // A driver signature takes the last two bytes.
        let guid = SdlGuid::create(SDL_BUS_BLUETOOTH, 0, 0, 0, names, b'm', 7);
        assert_eq!(&guid.0[4..16], b"Pro Contr\0m\x07");
    }

    #[test]
    fn parse_and_display() {
        let text = "03008FE54C050000CC09000000006800";
        let guid: SdlGuid = text.parse().unwrap();
        assert_eq!(guid.to_string(), text.to_ascii_lowercase());
        assert_eq!(guid.driver_signature(), b'h');
        assert_eq!(guid.ids().unwrap().product_id, 0x09cc);
        assert!("0300".parse::<SdlGuid>().is_err());
        assert!(
            "zz008fe54c050000cc09000000006800"
                .parse::<SdlGuid>()
                .is_err()
        );
        // 32 bytes, but a multi-byte character must not split a pair.
        assert!(
            "0é8fe54c050000cc09000000006800a"
                .parse::<SdlGuid>()
                .is_err()
        );
    }

    #[test]
    fn transports_map_to_sdl_buses() {
        assert_eq!(sdl_bus(Transport::Dongle), SDL_BUS_USB);
        assert_eq!(sdl_bus(Transport::BluetoothLe), SDL_BUS_BLUETOOTH);
        assert_eq!(sdl_bus(Transport::Unknown), SDL_BUS_UNKNOWN);
    }
}
