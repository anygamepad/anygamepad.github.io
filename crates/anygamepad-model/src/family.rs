//! Controller families recognized from the identity alone.
//!
//! A family tells what a controller looks like and what is printed on its
//! face buttons. It is a stopgap: device profiles (D4), built from the
//! reports the web tester and the desktop app collect, will identify devices
//! better and replace these rules. Until then it is a hint for presentation
//! (drawings, glyphs), not for mapping: the canonical state never depends on
//! it.

use crate::FaceLayout;
use crate::identity::{DeviceIdentity, GamepadApiId};
use crate::macros::named_enum;

/// Sony's USB vendor ID.
const SONY: u16 = 0x054c;
/// Nintendo's USB vendor ID.
const NINTENDO: u16 = 0x057e;
/// Microsoft's USB vendor ID.
const MICROSOFT: u16 = 0x045e;

named_enum! {
    /// A family of controllers that share a shape and button labels.
    pub enum Family: "family" {
        /// Sony DualShock 3 (Sixaxis).
        DualShock3 = "dualshock3",
        /// Sony DualShock 4.
        DualShock4 = "dualshock4",
        /// Sony DualSense.
        DualSense = "dualsense",
        /// Sony DualSense Edge.
        DualSenseEdge = "dualsense_edge",
        /// Nintendo Switch Pro Controller.
        SwitchPro = "switch_pro",
        /// Microsoft Xbox controllers (360, One, Series) and XInput pads.
        Xbox = "xbox",
    }
}

impl Family {
    /// The family of a device, from its VID/PID when known, otherwise from
    /// what the browser shows without them (XInput, or Safari's
    /// GameController names). `None` for every other device.
    ///
    /// Vendor IDs are trusted: a third-party pad in a mode that borrows
    /// Microsoft's (XInput emulation) is drawn as an Xbox pad, which is what
    /// that mode imitates.
    pub fn from_identity(identity: &DeviceIdentity) -> Option<Self> {
        if let (Some(vendor), Some(product)) = (identity.vendor_id, identity.product_id) {
            return Self::from_ids(vendor, product);
        }
        let web = identity
            .web
            .as_ref()
            .map(|web| GamepadApiId::parse(&web.id));
        if web.as_ref().is_some_and(|id| id.xinput) {
            return Some(Self::Xbox);
        }
        identity.product_name.as_deref().and_then(Self::from_name)
    }

    /// The family of a USB vendor and product ID.
    pub const fn from_ids(vendor_id: u16, product_id: u16) -> Option<Self> {
        match (vendor_id, product_id) {
            (SONY, 0x0268) => Some(Self::DualShock3),
            (SONY, 0x05c4 | 0x09cc) => Some(Self::DualShock4),
            (SONY, 0x0ce6) => Some(Self::DualSense),
            (SONY, 0x0df2) => Some(Self::DualSenseEdge),
            (NINTENDO, 0x2009) => Some(Self::SwitchPro),
            (MICROSOFT, _) => Some(Self::Xbox),
            _ => None,
        }
    }

    /// The family of a product name, for browsers that hide the IDs.
    ///
    /// Only the DualSense name is confirmed on a device (Safari on macOS,
    /// 2026-10-14); the others are the names GameController gives these
    /// pads, to verify.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "DualSense Wireless Controller" => Some(Self::DualSense),
            "DualSense Edge Wireless Controller" => Some(Self::DualSenseEdge),
            "DUALSHOCK 4 Wireless Controller" => Some(Self::DualShock4),
            "Pro Controller" => Some(Self::SwitchPro),
            _ if name.starts_with("Xbox ") => Some(Self::Xbox),
            _ => None,
        }
    }

    /// What is printed on the face buttons.
    pub const fn face_layout(self) -> FaceLayout {
        match self {
            Self::DualShock3 | Self::DualShock4 | Self::DualSense | Self::DualSenseEdge => {
                FaceLayout::PLAYSTATION
            }
            Self::SwitchPro => FaceLayout::NINTENDO,
            Self::Xbox => FaceLayout::XBOX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Browser, BrowserEngine, Os};

    fn web(id: &str) -> DeviceIdentity {
        let browser = Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(141),
        };
        DeviceIdentity::from_gamepad_api(id, "standard", browser, Os::Windows)
    }

    #[test]
    fn from_ids() {
        assert_eq!(Family::from_ids(0x054c, 0x0268), Some(Family::DualShock3));
        assert_eq!(Family::from_ids(0x054c, 0x09cc), Some(Family::DualShock4));
        assert_eq!(Family::from_ids(0x054c, 0x0ce6), Some(Family::DualSense));
        assert_eq!(
            Family::from_ids(0x054c, 0x0df2),
            Some(Family::DualSenseEdge)
        );
        assert_eq!(Family::from_ids(0x057e, 0x2009), Some(Family::SwitchPro));
        assert_eq!(Family::from_ids(0x045e, 0x0b13), Some(Family::Xbox));
        assert_eq!(Family::from_ids(0x2dc8, 0x6012), None);
    }

    #[test]
    fn from_browser_ids() {
        let chromium =
            web("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)");
        assert_eq!(Family::from_identity(&chromium), Some(Family::DualSense));
        // VID/PID win over the name.
        let clone =
            web("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 2dc8 Product: 6012)");
        assert_eq!(Family::from_identity(&clone), None);

        let xinput = web("Xbox 360 Controller (XInput STANDARD GAMEPAD)");
        assert_eq!(Family::from_identity(&xinput), Some(Family::Xbox));
        assert_eq!(Family::from_identity(&web("xinput")), Some(Family::Xbox));

        let safari = web("DualSense Wireless Controller Extended Gamepad");
        assert_eq!(Family::from_identity(&safari), Some(Family::DualSense));
        let safari_xbox = web("Xbox Wireless Controller Extended Gamepad");
        assert_eq!(Family::from_identity(&safari_xbox), Some(Family::Xbox));

        assert_eq!(Family::from_identity(&web("Flydigi Vader 5 Pro")), None);
    }

    #[test]
    fn face_layouts() {
        assert_eq!(Family::DualSense.face_layout(), FaceLayout::PLAYSTATION);
        assert_eq!(Family::SwitchPro.face_layout(), FaceLayout::NINTENDO);
        assert_eq!(Family::Xbox.face_layout(), FaceLayout::XBOX);
        assert_eq!(Family::ALL.len(), 6);
        assert_eq!(Family::DualSenseEdge.as_str(), "dualsense_edge");
    }
}
