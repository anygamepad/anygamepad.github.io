//! Device identity: what a provider knows about a device, shared by device
//! profiles (D1) and device reports (P1).
//!
//! Identity is more than VID/PID: the same controller looks different per
//! transport, OS, OS driver, input API and browser, and VID/PID can be
//! missing (XInput, Safari's GameController devices) or borrowed by clones.
//!
//! A [`DeviceIdentity`] never holds serial numbers, Bluetooth addresses or OS
//! device paths, so it can be stored in reports. Keys that locate a device on
//! the host live in the provider layer and are never serialized.

use core::fmt;
use core::str::FromStr;

use crate::hash::Fnv64;
use crate::macros::named_enum;
use crate::sdl::{SdlGuid, SdlGuidNames, sdl_bus};
use crate::{ParseNameError, Transport};

named_enum! {
    /// Operating system of the host.
    pub enum Os: "operating system" {
        /// Not known.
        Unknown = "unknown",
        /// Linux (including SteamOS).
        Linux = "linux",
        /// Windows.
        Windows = "windows",
        /// macOS.
        MacOs = "macos",
        /// Android.
        Android = "android",
        /// iOS and iPadOS.
        Ios = "ios",
    }
}

impl Os {
    /// The OS this code was compiled for; [`Os::Unknown`] in WebAssembly,
    /// where the browser tells the OS instead.
    pub const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else {
            Self::Unknown
        }
    }
}

named_enum! {
    /// Input provider that produced a view of a device (ADR 0002).
    pub enum ProviderKind: "provider" {
        /// anygamepad's own native backends.
        Native = "native",
        /// anygamepad's own browser backend (Gamepad API, WebHID).
        Web = "web",
        /// SDL3.
        Sdl3 = "sdl3",
        /// gilrs.
        Gilrs = "gilrs",
    }
}

named_enum! {
    /// OS or browser API through which a provider sees a device.
    pub enum Backend: "backend" {
        /// Not known.
        Unknown = "unknown",
        /// Linux input subsystem (`/dev/input/event*`).
        Evdev = "evdev",
        /// Linux raw HID (`/dev/hidraw*`).
        Hidraw = "hidraw",
        /// Windows HID (Raw Input, `hid.dll`).
        WindowsHid = "windows_hid",
        /// Windows XInput.
        XInput = "xinput",
        /// Windows.Gaming.Input.
        WindowsGamingInput = "windows_gaming_input",
        /// Microsoft GameInput.
        GameInput = "game_input",
        /// DirectInput.
        DirectInput = "direct_input",
        /// macOS IOKit HID.
        IoKitHid = "iokit_hid",
        /// Apple GameController framework.
        GameController = "game_controller",
        /// Browser Gamepad API.
        GamepadApi = "gamepad_api",
        /// Browser WebHID.
        WebHid = "webhid",
    }
}

named_enum! {
    /// Browser engine, since each one remaps gamepads differently.
    pub enum BrowserEngine: "browser engine" {
        /// Not known.
        Unknown = "unknown",
        /// Chromium-based browsers (Chrome, Edge, Opera, ...).
        Blink = "blink",
        /// Firefox.
        Gecko = "gecko",
        /// Safari and every iOS browser.
        WebKit = "webkit",
    }
}

/// The browser a web view of a device comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct Browser {
    /// Engine.
    pub engine: BrowserEngine,
    /// Major version only, which is all that remapping changes need and
    /// identifies users less than a full version.
    pub major_version: Option<u16>,
}

/// What the Gamepad API says about a device.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct WebGamepad {
    /// `Gamepad.id`, verbatim (its format differs per browser; see
    /// [`GamepadApiId`]).
    pub id: String,
    /// `Gamepad.mapping`, verbatim: `"standard"`, `""` or `"xr-standard"`.
    pub mapping: String,
}

macro_rules! hex_u64 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Shown and serialized in human-readable formats as 16 hexadecimal
        /// digits (JSON numbers cannot hold every `u64` exactly).
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:016x}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self)
            }
        }

        impl FromStr for $name {
            type Err = ParseNameError;

            /// Parses exactly 16 hexadecimal digits.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                    u64::from_str_radix(s, 16)
                        .map(Self)
                        .map_err(|_| ParseNameError::new(stringify!($name), s))
                } else {
                    Err(ParseNameError::new(stringify!($name), s))
                }
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    serializer.collect_str(self)
                } else {
                    serializer.serialize_u64(self.0)
                }
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                if deserializer.is_human_readable() {
                    let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                    s.parse().map_err(serde::de::Error::custom)
                } else {
                    u64::deserialize(deserializer).map(Self)
                }
            }
        }

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": "^[0-9a-fA-F]{16}$",
                })
            }
        }

        crate::macros::ts_decl!(
            "/** 16 hexadecimal digits. */\nexport type ", stringify!($name), " = string;"
        );
    };
}

hex_u64! {
    /// Hash (FNV-1a, 64 bits) of a HID report descriptor: tells apart devices
    /// that share a VID/PID but report differently (modes, clones).
    DescriptorHash
}

impl DescriptorHash {
    /// Hashes the raw bytes of a HID report descriptor.
    pub fn of(descriptor: &[u8]) -> Self {
        let mut hash = Fnv64::new();
        hash.bytes(descriptor);
        Self(hash.finish())
    }
}

hex_u64! {
    /// Stable hash of a [`DeviceIdentity`]: two views with the same
    /// fingerprint are the same kind of device seen through the same
    /// software stack. See [`DeviceIdentity::fingerprint`].
    Fingerprint
}

/// Identity of a device as one provider sees it.
///
/// Holds no serial number, Bluetooth address or OS device path (see the
/// [module documentation](self)).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct DeviceIdentity {
    /// Provider that produced this view.
    pub provider: ProviderKind,
    /// API the provider read the device through.
    pub backend: Backend,
    /// Host OS.
    pub os: Os,
    /// How the device is connected.
    pub transport: Transport,
    /// USB/Bluetooth vendor ID, when the API exposes it.
    pub vendor_id: Option<u16>,
    /// Product ID, when the API exposes it.
    pub product_id: Option<u16>,
    /// Version (`bcdDevice`, HID or evdev version), when exposed.
    pub version: Option<u16>,
    /// Manufacturer string, as reported by this API.
    pub vendor_name: Option<String>,
    /// Product string, as reported by this API.
    pub product_name: Option<String>,
    /// OS driver that owns the device (`xpad`, `hid-playstation`,
    /// `hid-generic`, `xusb22`, `HidUsb`, ...).
    pub driver: Option<String>,
    /// Hash of the HID report descriptor, when readable.
    pub descriptor_hash: Option<DescriptorHash>,
    /// Browser, for web views.
    pub browser: Option<Browser>,
    /// Gamepad API data, for Gamepad API views.
    pub web: Option<WebGamepad>,
}

impl DeviceIdentity {
    /// An identity with only the provider, backend and OS known.
    pub const fn new(provider: ProviderKind, backend: Backend, os: Os) -> Self {
        Self {
            provider,
            backend,
            os,
            transport: Transport::Unknown,
            vendor_id: None,
            product_id: None,
            version: None,
            vendor_name: None,
            product_name: None,
            driver: None,
            descriptor_hash: None,
            browser: None,
            web: None,
        }
    }

    /// The identity of a Gamepad API device, from `Gamepad.id` and
    /// `Gamepad.mapping`. VID, PID and product name are parsed from the id
    /// when the browser includes them.
    pub fn from_gamepad_api(id: &str, mapping: &str, browser: Browser, os: Os) -> Self {
        let parsed = GamepadApiId::parse(id);
        Self {
            vendor_id: parsed.vendor_id,
            product_id: parsed.product_id,
            product_name: parsed.name,
            browser: Some(browser),
            web: Some(WebGamepad {
                id: id.to_owned(),
                mapping: mapping.to_owned(),
            }),
            ..Self::new(ProviderKind::Web, Backend::GamepadApi, os)
        }
    }

    /// The stable fingerprint of this identity.
    ///
    /// Covers every field except the browser version, so that a browser
    /// update alone does not make a device look new. The value is stored in
    /// reports and must not change between library versions (pinned by
    /// tests).
    ///
    /// Encoding: FNV-1a (64 bits) over, in order, the string
    /// `anygamepad-fingerprint-v1`; the names of `provider`, `backend`, `os`
    /// and `transport`; `vendor_id`, `product_id`, `version`; `vendor_name`,
    /// `product_name`, `driver`; `descriptor_hash`; the browser engine name;
    /// the Gamepad API `id` and `mapping`. A string is its UTF-8 length as a
    /// little-endian `u32` followed by its bytes. An optional value is a `0`
    /// byte when absent, or a `1` byte followed by the value (integers
    /// little-endian).
    pub fn fingerprint(&self) -> Fingerprint {
        let mut hash = Fnv64::new();
        hash.str("anygamepad-fingerprint-v1");
        hash.str(self.provider.as_str());
        hash.str(self.backend.as_str());
        hash.str(self.os.as_str());
        hash.str(self.transport.as_str());
        hash.opt_u16(self.vendor_id);
        hash.opt_u16(self.product_id);
        hash.opt_u16(self.version);
        hash.opt_str(self.vendor_name.as_deref());
        hash.opt_str(self.product_name.as_deref());
        hash.opt_str(self.driver.as_deref());
        hash.opt_u64(self.descriptor_hash.map(|hash| hash.0));
        hash.opt_str(self.browser.map(|browser| browser.engine.as_str()));
        hash.opt_str(self.web.as_ref().map(|web| web.id.as_str()));
        hash.opt_str(self.web.as_ref().map(|web| web.mapping.as_str()));
        Fingerprint(hash.finish())
    }

    /// The SDL GUID SDL would compute from these fields with the given SDL
    /// driver signature and data (0 and 0 for SDL's OS backends without a
    /// signature, such as Linux evdev).
    ///
    /// The CRC uses the vendor and product names of this identity; SDL
    /// backends differ in which names they pass, so compare with
    /// [`SdlGuid::without_crc`] when the names may differ.
    pub fn sdl_guid(&self, driver_signature: u8, driver_data: u8) -> SdlGuid {
        SdlGuid::create(
            sdl_bus(self.transport),
            self.vendor_id.unwrap_or(0),
            self.product_id.unwrap_or(0),
            self.version.unwrap_or(0),
            SdlGuidNames {
                vendor: self.vendor_name.as_deref(),
                product: self.product_name.as_deref(),
            },
            driver_signature,
            driver_data,
        )
    }
}

/// What a browser's `Gamepad.id` string says about the device.
///
/// Formats (`docs/research/06-browser-gamepad-api.md`):
///
/// | Browser | `id` |
/// |---------|------|
/// | Chromium | `"<name> (STANDARD GAMEPAD Vendor: 054c Product: 09cc)"`, `"<name> (Vendor: 054c Product: 09cc)"`, `"<name> (STANDARD GAMEPAD)"` |
/// | Chromium, XInput | `"Xbox 360 Controller (XInput STANDARD GAMEPAD)"` |
/// | Firefox | `"054c-09cc-<name>"` |
/// | Firefox, XInput | `"xinput"` |
/// | Safari, HID | `"54c-9cc-<name>"` (hexadecimal, maybe without leading zeros) |
/// | Safari, GameController | `"<name> Extended Gamepad"`, `"<name> Gamepad"` |
///
/// Unrecognized formats give the whole id as the name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GamepadApiId {
    /// Vendor ID, if the id includes it.
    pub vendor_id: Option<u16>,
    /// Product ID, if the id includes it.
    pub product_id: Option<u16>,
    /// Product name, if any.
    pub name: Option<String>,
    /// Whether the browser read the device through XInput (so it is an
    /// Xbox-compatible pad without VID/PID).
    pub xinput: bool,
}

impl GamepadApiId {
    /// Parses a `Gamepad.id` string.
    pub fn parse(id: &str) -> Self {
        let id = id.trim();
        if id == "xinput" {
            return Self {
                xinput: true,
                ..Self::default()
            };
        }
        if let Some(parsed) = Self::parse_chromium(id) {
            return parsed;
        }
        if let Some(parsed) = Self::parse_vid_pid_prefix(id) {
            return parsed;
        }
        // Safari, GameController: the controller's name and its profile.
        // Only the extended profile is recognized: a plain `" Gamepad"`
        // suffix is too common in real product names.
        let name = id.strip_suffix(" Extended Gamepad").unwrap_or(id);
        Self {
            name: non_empty(name),
            ..Self::default()
        }
    }

    /// `"<name> (<details>)"`.
    fn parse_chromium(id: &str) -> Option<Self> {
        let open = id.rfind(" (")?;
        let details = id[open + 2..].strip_suffix(')')?;
        let name = non_empty(&id[..open]);
        if details == "XInput STANDARD GAMEPAD" {
            return Some(Self {
                name,
                xinput: true,
                ..Self::default()
            });
        }
        if details == "STANDARD GAMEPAD" {
            return Some(Self {
                name,
                ..Self::default()
            });
        }
        let ids = details.strip_prefix("STANDARD GAMEPAD ").unwrap_or(details);
        let ids = ids.strip_prefix("Vendor: ")?;
        let (vendor, product) = ids.split_once(" Product: ")?;
        Some(Self {
            vendor_id: Some(parse_hex_u16(vendor)?),
            product_id: Some(parse_hex_u16(product)?),
            name,
            xinput: false,
        })
    }

    /// `"<vid>-<pid>-<name>"` (Firefox, Safari HID).
    fn parse_vid_pid_prefix(id: &str) -> Option<Self> {
        let (vendor, rest) = id.split_once('-')?;
        let (product, name) = rest.split_once('-')?;
        Some(Self {
            vendor_id: Some(parse_hex_u16(vendor)?),
            product_id: Some(parse_hex_u16(product)?),
            name: non_empty(name),
            xinput: false,
        })
    }
}

/// One to four hexadecimal digits.
fn parse_hex_u16(s: &str) -> Option<u16> {
    if (1..=4).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        u16::from_str_radix(s, 16).ok()
    } else {
        None
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(
        vendor_id: Option<u16>,
        product_id: Option<u16>,
        name: Option<&str>,
        xinput: bool,
    ) -> GamepadApiId {
        GamepadApiId {
            vendor_id,
            product_id,
            name: name.map(str::to_owned),
            xinput,
        }
    }

    #[test]
    fn parses_chromium_ids() {
        let cases = [
            (
                "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)",
                parsed(
                    Some(0x054c),
                    Some(0x09cc),
                    Some("Wireless Controller"),
                    false,
                ),
            ),
            (
                "USB Gamepad (Vendor: 0079 Product: 0011)",
                parsed(Some(0x0079), Some(0x0011), Some("USB Gamepad"), false),
            ),
            (
                "Controller (XBOX 360 For Windows) (STANDARD GAMEPAD Vendor: 045e Product: 028e)",
                parsed(
                    Some(0x045e),
                    Some(0x028e),
                    Some("Controller (XBOX 360 For Windows)"),
                    false,
                ),
            ),
            (
                "Xbox 360 Controller (XInput STANDARD GAMEPAD)",
                parsed(None, None, Some("Xbox 360 Controller"), true),
            ),
            (
                "Some Pad (STANDARD GAMEPAD)",
                parsed(None, None, Some("Some Pad"), false),
            ),
        ];
        for (id, expected) in cases {
            assert_eq!(GamepadApiId::parse(id), expected, "{id}");
        }
    }

    #[test]
    fn parses_firefox_and_safari_ids() {
        let cases = [
            (
                "054c-09cc-Wireless Controller",
                parsed(
                    Some(0x054c),
                    Some(0x09cc),
                    Some("Wireless Controller"),
                    false,
                ),
            ),
            (
                "54c-9cc-Wireless Controller",
                parsed(
                    Some(0x054c),
                    Some(0x09cc),
                    Some("Wireless Controller"),
                    false,
                ),
            ),
            (
                "057e-2009-Pro-Controller",
                parsed(Some(0x057e), Some(0x2009), Some("Pro-Controller"), false),
            ),
            ("xinput", parsed(None, None, None, true)),
        ];
        for (id, expected) in cases {
            assert_eq!(GamepadApiId::parse(id), expected, "{id}");
        }
    }

    #[test]
    fn unknown_formats_keep_the_whole_id_as_name() {
        for id in ["8BitDo-Pro-2", "Pad (Vendor: 12345 Product: 0001)"] {
            assert_eq!(
                GamepadApiId::parse(id),
                parsed(None, None, Some(id), false),
                "{id}"
            );
        }
        assert_eq!(GamepadApiId::parse("  "), GamepadApiId::default());
    }

    #[test]
    fn safari_game_controller_ids_keep_the_name() {
        assert_eq!(
            GamepadApiId::parse("DualSense Wireless Controller Extended Gamepad"),
            parsed(None, None, Some("DualSense Wireless Controller"), false)
        );
        assert_eq!(
            GamepadApiId::parse("Generic USB Gamepad"),
            parsed(None, None, Some("Generic USB Gamepad"), false)
        );
    }

    #[test]
    fn gamepad_api_identity() {
        let browser = Browser {
            engine: BrowserEngine::Gecko,
            major_version: Some(140),
        };
        let identity = DeviceIdentity::from_gamepad_api(
            "054c-09cc-Wireless Controller",
            "standard",
            browser,
            Os::Linux,
        );
        assert_eq!(identity.provider, ProviderKind::Web);
        assert_eq!(identity.backend, Backend::GamepadApi);
        assert_eq!(identity.vendor_id, Some(0x054c));
        assert_eq!(
            identity.product_name.as_deref(),
            Some("Wireless Controller")
        );
        assert_eq!(identity.web.as_ref().unwrap().mapping, "standard");
    }

    fn dualsense() -> DeviceIdentity {
        DeviceIdentity {
            transport: Transport::Usb,
            vendor_id: Some(0x054c),
            product_id: Some(0x0ce6),
            version: Some(0x8111),
            vendor_name: Some("Sony Interactive Entertainment".into()),
            product_name: Some("DualSense Wireless Controller".into()),
            driver: Some("hid-playstation".into()),
            descriptor_hash: Some(DescriptorHash::of(&[0x05, 0x01, 0x09, 0x05])),
            ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
        }
    }

    #[test]
    fn fingerprint_is_pinned() {
        // These values are stored in reports: changing them is a breaking
        // change of the report format. They were computed by an independent
        // implementation of the encoding documented on `fingerprint`.
        assert_eq!(
            DeviceIdentity::new(ProviderKind::Native, Backend::Unknown, Os::Unknown)
                .fingerprint()
                .to_string(),
            "5def3c91a1ad9330"
        );
        assert_eq!(dualsense().fingerprint().to_string(), "8917ced29ecfbd36");
        assert_eq!(
            DescriptorHash::of(&[0x05, 0x01, 0x09, 0x05]).to_string(),
            "36015c55f3bd94d1"
        );
    }

    #[test]
    fn fingerprint_ignores_browser_version_only() {
        let browser = |major_version| Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(major_version),
        };
        let id = "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)";
        let old = DeviceIdentity::from_gamepad_api(id, "standard", browser(120), Os::Windows);
        let new = DeviceIdentity::from_gamepad_api(id, "standard", browser(141), Os::Windows);
        assert_eq!(old.fingerprint(), new.fingerprint());

        let unmapped = DeviceIdentity::from_gamepad_api(id, "", browser(141), Os::Windows);
        assert_ne!(old.fingerprint(), unmapped.fingerprint());

        let mut other = dualsense();
        other.driver = Some("hid-generic".into());
        assert_ne!(other.fingerprint(), dualsense().fingerprint());

        // Moving text between fields changes the fingerprint.
        let mut a = dualsense();
        a.vendor_name = Some("ab".into());
        a.product_name = Some("c".into());
        let mut b = dualsense();
        b.vendor_name = Some("a".into());
        b.product_name = Some("bc".into());
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn sdl_guid_from_identity() {
        let identity = DeviceIdentity {
            vendor_name: None,
            ..dualsense()
        };
        let guid = identity.sdl_guid(0, 0);
        assert_eq!(
            guid.without_crc().to_string(),
            "030000004c050000e60c000011810000"
        );
        assert_ne!(guid.crc(), 0);
    }

    #[test]
    fn hex_values_parse_and_display() {
        let hash: Fingerprint = "00ff00ff00ff00ff".parse().unwrap();
        assert_eq!(hash.0, 0x00ff_00ff_00ff_00ff);
        assert_eq!(hash.to_string(), "00ff00ff00ff00ff");
        assert!("ff".parse::<Fingerprint>().is_err());
        assert!("+0ff00ff00ff00ff".parse::<Fingerprint>().is_err());
    }

    #[test]
    fn current_os() {
        if cfg!(target_os = "linux") {
            assert_eq!(Os::current(), Os::Linux);
        }
    }
}
