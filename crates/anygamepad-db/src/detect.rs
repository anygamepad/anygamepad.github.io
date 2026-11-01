//! Automatic detection (roadmap task H10, ADR 0008): which catalog model,
//! mode and transport a connected device is, and which unit of the lab.
//!
//! [`Catalog::identify`] matches the identity a provider gives with the
//! identities the catalog expects for each model, mode and transport, and
//! returns the [`Candidate`]s, most likely first: the most specific match
//! first, then models the lab has units of, then catalog order. When no
//! model matches, it falls back on the generic model of the device's
//! family (`generic/xinput`, `generic/playstation`, `generic/switch-pro`,
//! `generic/hid`). The person confirms the first candidate or chooses
//! another; both go into the test report, so detection is measured.
//!
//! [`Catalog::check_choice`] tells whether a device that connects is the
//! model, mode and transport the person picked from the list.
//! [`Inventory::proposed_units`] orders the units of a model, since two
//! pads of one model are told apart by the person, never by serial
//! numbers.
//!
//! Mode and clone detection (C8) will refine the matching behind the same
//! calls.

use anygamepad_model::identity::DeviceIdentity;
use anygamepad_model::{Family, Transport};

use crate::catalog::{Catalog, Connection, Mode, Model};
use crate::lab::{Code, Inventory, Unit};

/// One thing a connected device can be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate<'a> {
    /// The catalog model.
    pub model: &'a Model,
    /// The model's mode.
    pub mode: &'a Mode,
    /// The transport, when the identity tells it or the mode's matching
    /// connections share one; `None` when the person must say (the
    /// Gamepad API does not tell).
    pub transport: Option<Transport>,
    /// Conditions of the catalog identity that matched; 0 for a generic
    /// model, reached by family.
    pub specificity: u32,
    /// Whether the inventory has units of this model.
    pub in_inventory: bool,
}

/// What a connected device can be, most likely first.
#[derive(Clone, Debug, PartialEq)]
pub struct Identification<'a> {
    /// Never empty when the catalog has the generic models.
    pub candidates: Vec<Candidate<'a>>,
    /// Whether no model matched and the candidate is the generic model of
    /// the device's family.
    pub generic: bool,
}

impl<'a> Identification<'a> {
    /// The most likely candidate.
    pub fn best(&self) -> Option<&Candidate<'a>> {
        self.candidates.first()
    }
}

/// Whether a picked model, mode and transport fits a connected device.
#[derive(Clone, Debug, PartialEq)]
pub enum ChoiceCheck<'a> {
    /// The device shows an identity the catalog expects for the choice.
    Matches,
    /// The catalog has no identity for the choice yet (or the model is
    /// generic): nothing to compare. The session's report will show one.
    NotKnown,
    /// The device shows something else; what it looks like instead.
    Differs(Identification<'a>),
    /// The catalog has no such model or mode.
    NoSuchChoice,
}

/// Whether a device on `device` can be a connection over `catalog`: the
/// same transport, or Bluetooth and Bluetooth Low Energy, which most
/// providers cannot tell apart. An unknown device transport fits any.
pub fn transports_fit(device: Transport, catalog: Transport) -> bool {
    use Transport::{Bluetooth, BluetoothLe, Unknown};
    device == Unknown
        || device == catalog
        || matches!(
            (device, catalog),
            (Bluetooth, BluetoothLe) | (BluetoothLe, Bluetooth)
        )
}

/// The ID of the generic model for a family.
pub fn generic_model_id(family: Option<Family>) -> &'static str {
    match family {
        Some(Family::Xbox) => "generic/xinput",
        Some(
            Family::DualShock3 | Family::DualShock4 | Family::DualSense | Family::DualSenseEdge,
        ) => "generic/playstation",
        Some(Family::SwitchPro) => "generic/switch-pro",
        _ => "generic/hid",
    }
}

/// The transport of a candidate from the connections of its mode that
/// matched: the device's when it tells, else the one they share.
fn candidate_transport(device: Transport, matched: &[&Connection]) -> Option<Transport> {
    if device != Transport::Unknown {
        return matched
            .iter()
            .find(|connection| transports_fit(device, connection.transport))
            .map(|connection| connection.transport);
    }
    let first = matched.first()?.transport;
    matched
        .iter()
        .all(|connection| connection.transport == first)
        .then_some(first)
}

impl Catalog {
    /// What `identity` can be, most likely first (see the
    /// [module documentation](self)); `inventory` puts the models the lab
    /// has first among equally specific matches.
    pub fn identify(
        &self,
        identity: &DeviceIdentity,
        inventory: Option<&Inventory>,
    ) -> Identification<'_> {
        let has_units = |model: &Model| {
            inventory.is_some_and(|inventory| inventory.units_of(&model.id).next().is_some())
        };
        let mut candidates = Vec::new();
        for model in self.models.iter().filter(|model| !model.generic) {
            for mode in &model.modes {
                let mut specificity = 0;
                let mut matched = Vec::new();
                for connection in &mode.connections {
                    if !transports_fit(identity.transport, connection.transport) {
                        continue;
                    }
                    let best = connection
                        .identities
                        .iter()
                        .filter(|expected| expected.matches(identity))
                        .map(|expected| expected.specificity())
                        .max();
                    if let Some(best) = best {
                        specificity = specificity.max(best);
                        matched.push(connection);
                    }
                }
                if !matched.is_empty() {
                    candidates.push(Candidate {
                        model,
                        mode,
                        transport: candidate_transport(identity.transport, &matched),
                        specificity,
                        in_inventory: has_units(model),
                    });
                }
            }
        }
        // Stable: catalog order among equals.
        candidates.sort_by(|a, b| {
            b.specificity
                .cmp(&a.specificity)
                .then(b.in_inventory.cmp(&a.in_inventory))
        });
        if !candidates.is_empty() {
            return Identification {
                candidates,
                generic: false,
            };
        }
        let generic = self
            .get(generic_model_id(Family::from_identity(identity)))
            .or_else(|| self.get(generic_model_id(None)));
        let candidates = generic
            .and_then(|model| {
                let mode = model.modes.first()?;
                let matched: Vec<&Connection> = mode
                    .connections
                    .iter()
                    .filter(|connection| transports_fit(identity.transport, connection.transport))
                    .collect();
                Some(Candidate {
                    model,
                    mode,
                    transport: candidate_transport(identity.transport, &matched),
                    specificity: 0,
                    in_inventory: has_units(model),
                })
            })
            .into_iter()
            .collect();
        Identification {
            candidates,
            generic: true,
        }
    }

    /// Whether `identity` is the model, mode and transport the person
    /// picked from the list.
    pub fn check_choice(
        &self,
        identity: &DeviceIdentity,
        model: &str,
        mode: &str,
        transport: Transport,
        inventory: Option<&Inventory>,
    ) -> ChoiceCheck<'_> {
        let Some(model) = self.get(model) else {
            return ChoiceCheck::NoSuchChoice;
        };
        let Some(mode) = model.mode(mode) else {
            return ChoiceCheck::NoSuchChoice;
        };
        if model.generic {
            return ChoiceCheck::NotKnown;
        }
        let expected: Vec<_> = mode
            .connections
            .iter()
            .filter(|connection| transports_fit(transport, connection.transport))
            .flat_map(|connection| &connection.identities)
            .collect();
        if expected.is_empty() {
            ChoiceCheck::NotKnown
        } else if expected.iter().any(|expected| expected.matches(identity)) {
            ChoiceCheck::Matches
        } else {
            ChoiceCheck::Differs(self.identify(identity, inventory))
        }
    }
}

impl Inventory {
    /// The units of `model` to propose: the one picked last for that model
    /// on this machine first (the app keeps it, never a report), then in
    /// inventory order.
    pub fn proposed_units<'a>(&'a self, model: &str, last_picked: Option<&Code>) -> Vec<&'a Unit> {
        let mut units: Vec<&Unit> = self
            .units
            .iter()
            .filter(|unit| unit.model == model)
            .collect();
        if let Some(last) = last_picked {
            if let Some(position) = units.iter().position(|unit| &unit.code == last) {
                let unit = units.remove(position);
                units.insert(0, unit);
            }
        }
        units
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::identity::{Backend, Browser, BrowserEngine, Os, ProviderKind};
    use anygamepad_model::{Button, ButtonSet, ControllerMode};

    use super::*;
    use crate::catalog::{Controls, ExpectedIdentity};
    use crate::lab::Unit;

    fn evdev(vendor: u16, product: u16, transport: Transport) -> DeviceIdentity {
        DeviceIdentity {
            vendor_id: Some(vendor),
            product_id: Some(product),
            transport,
            ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
        }
    }

    fn chrome(id: &str) -> DeviceIdentity {
        let browser = Browser {
            engine: BrowserEngine::Blink,
            major_version: Some(141),
        };
        DeviceIdentity::from_gamepad_api(id, "standard", browser, Os::Windows)
    }

    fn ids(identification: &Identification<'_>) -> Vec<(String, String, Option<Transport>)> {
        identification
            .candidates
            .iter()
            .map(|c| (c.model.id.clone(), c.mode.id.clone(), c.transport))
            .collect()
    }

    fn one(
        model: &str,
        mode: &str,
        transport: Option<Transport>,
    ) -> Vec<(String, String, Option<Transport>)> {
        vec![(model.into(), mode.into(), transport)]
    }

    #[test]
    fn known_pads() {
        let catalog = Catalog::embedded();
        let found = catalog.identify(&evdev(0x054c, 0x0ce6, Transport::Usb), None);
        assert_eq!(
            ids(&found),
            one("sony/dualsense", "ps5", Some(Transport::Usb))
        );
        assert!(!found.generic);

        let found = catalog.identify(&evdev(0x057e, 0x2009, Transport::Bluetooth), None);
        assert_eq!(
            ids(&found),
            one("nintendo/switch-pro", "switch", Some(Transport::Bluetooth))
        );

        // Most providers cannot tell Bluetooth LE from Bluetooth.
        let found = catalog.identify(&evdev(0x045e, 0x0b13, Transport::Bluetooth), None);
        assert_eq!(
            ids(&found),
            one(
                "microsoft/xbox-series",
                "xbox",
                Some(Transport::BluetoothLe)
            )
        );
    }

    #[test]
    fn every_catalog_entry_is_identified_from_its_identities() {
        let catalog = Catalog::embedded();
        let mut checked = 0;
        for model in catalog.models() {
            for mode in &model.modes {
                for connection in &mode.connections {
                    for expected in &connection.identities {
                        let identity = DeviceIdentity {
                            vendor_id: expected.vendor_id,
                            product_id: expected.product_id,
                            product_name: expected.name.clone(),
                            driver: expected.driver.clone(),
                            descriptor_hash: expected.descriptor_hash,
                            os: expected.os.unwrap_or(Os::Linux),
                            backend: expected.backend.unwrap_or(Backend::Evdev),
                            transport: connection.transport,
                            ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
                        };
                        let found = catalog.identify(&identity, None);
                        assert!(
                            found.candidates.iter().any(|candidate| {
                                candidate.model.id == model.id
                                    && candidate.mode.id == mode.id
                                    && candidate.transport == Some(connection.transport)
                            }),
                            "{} {} {}",
                            model.id,
                            mode.id,
                            connection.transport
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked >= 12);
    }

    #[test]
    fn transports_the_api_does_not_tell() {
        let catalog = Catalog::embedded();
        // A DualSense connects over USB or Bluetooth with the same IDs:
        // the person says which.
        let found = catalog.identify(
            &chrome("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)"),
            None,
        );
        assert_eq!(ids(&found), one("sony/dualsense", "ps5", None));
        // The wired Xbox 360 controller connects one way only.
        let found = catalog.identify(
            &chrome("Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)"),
            None,
        );
        assert_eq!(
            ids(&found),
            one("microsoft/xbox-360", "xinput", Some(Transport::Usb))
        );
    }

    #[test]
    fn generic_fallback_by_family() {
        let catalog = Catalog::embedded();
        for (identity, generic) in [
            (evdev(0x045e, 0x0b00, Transport::Usb), "generic/xinput"),
            (evdev(0x2dc8, 0x6012, Transport::Usb), "generic/hid"),
            (
                chrome("Xbox 360 Controller (XInput STANDARD GAMEPAD)"),
                "generic/xinput",
            ),
        ] {
            let found = catalog.identify(&identity, None);
            assert!(found.generic);
            assert_eq!(found.best().unwrap().model.id, generic);
            assert_eq!(found.best().unwrap().specificity, 0);
        }
    }

    fn acme() -> Model {
        let identity = |vendor, product| ExpectedIdentity {
            vendor_id: Some(vendor),
            product_id: Some(product),
            ..ExpectedIdentity::default()
        };
        let connection = |transport, identities| Connection {
            transport,
            how_to: None,
            identities,
        };
        let mode = |id: &str, kind, connections| Mode {
            id: id.into(),
            name: id.to_uppercase(),
            kind,
            how_to: None,
            controls: None,
            connections,
        };
        Model {
            schema_version: 1,
            id: "acme/pad".into(),
            vendor: "Acme".into(),
            name: "Pad".into(),
            generic: false,
            family: None,
            labels: None,
            controls: Controls {
                buttons: ButtonSet::from([Button::South]),
                ..Controls::default()
            },
            modes: vec![
                mode(
                    "x",
                    ControllerMode::Xinput,
                    vec![
                        connection(Transport::Usb, vec![identity(0x045e, 0x028e)]),
                        connection(Transport::Dongle, vec![identity(0x045e, 0x028e)]),
                    ],
                ),
                mode(
                    "s",
                    ControllerMode::Switch,
                    vec![connection(
                        Transport::Bluetooth,
                        vec![identity(0x057e, 0x2009)],
                    )],
                ),
            ],
            notes: None,
        }
    }

    fn with_acme() -> Catalog {
        let mut models = Catalog::embedded().models().to_vec();
        models.push(acme());
        Catalog::new(models).unwrap()
    }

    fn inventory(units: &[(&str, &str)]) -> Inventory {
        Inventory {
            schema_version: 1,
            units: units
                .iter()
                .map(|(code, model)| Unit {
                    code: Code::new(code).unwrap(),
                    model: (*model).into(),
                    notes: None,
                })
                .collect(),
            machines: Vec::new(),
        }
    }

    #[test]
    fn shared_identities_list_every_model_inventory_first() {
        let catalog = with_acme();
        let identity = evdev(0x045e, 0x028e, Transport::Usb);
        let found = catalog.identify(&identity, None);
        assert_eq!(
            ids(&found),
            vec![
                (
                    "microsoft/xbox-360".into(),
                    "xinput".into(),
                    Some(Transport::Usb)
                ),
                ("acme/pad".into(), "x".into(), Some(Transport::Usb)),
            ]
        );
        let lab = inventory(&[("ACME-1", "acme/pad")]);
        let found = catalog.identify(&identity, Some(&lab));
        assert_eq!(found.best().unwrap().model.id, "acme/pad");
        assert!(found.best().unwrap().in_inventory);

        // A dongle and a cable look alike: the person says which.
        let found = catalog.identify(
            &chrome("Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)"),
            Some(&lab),
        );
        assert_eq!(found.best().unwrap().transport, None);

        // In Switch mode it looks like a Switch Pro Controller.
        let found = catalog.identify(&evdev(0x057e, 0x2009, Transport::Bluetooth), Some(&lab));
        assert_eq!(
            ids(&found)[0],
            ("acme/pad".into(), "s".into(), Some(Transport::Bluetooth))
        );
    }

    #[test]
    fn checking_a_choice() {
        let catalog = with_acme();
        let dualsense = evdev(0x054c, 0x0ce6, Transport::Usb);
        assert_eq!(
            catalog.check_choice(&dualsense, "sony/dualsense", "ps5", Transport::Usb, None),
            ChoiceCheck::Matches
        );
        // Picked Switch mode over Bluetooth, but the pad is still in XInput
        // mode on its dongle: the app says what it sees instead.
        let xinput = evdev(0x045e, 0x028e, Transport::Dongle);
        let lab = inventory(&[("ACME-1", "acme/pad")]);
        let ChoiceCheck::Differs(seen) =
            catalog.check_choice(&xinput, "acme/pad", "s", Transport::Bluetooth, Some(&lab))
        else {
            panic!("a different mode");
        };
        let best = seen.best().unwrap();
        assert_eq!(
            (best.model.id.as_str(), best.mode.id.as_str()),
            ("acme/pad", "x")
        );
        assert_eq!(best.transport, Some(Transport::Dongle));
        // No identity known for the Xbox Wireless Adapter yet.
        assert_eq!(
            catalog.check_choice(
                &xinput,
                "microsoft/xbox-series",
                "xbox",
                Transport::Dongle,
                None
            ),
            ChoiceCheck::NotKnown
        );
        assert_eq!(
            catalog.check_choice(&xinput, "generic/xinput", "xinput", Transport::Usb, None),
            ChoiceCheck::NotKnown
        );
        assert_eq!(
            catalog.check_choice(&xinput, "acme/pad", "d", Transport::Usb, None),
            ChoiceCheck::NoSuchChoice
        );
    }

    #[test]
    fn units_last_picked_first() {
        let lab = inventory(&[
            ("DS5-01", "sony/dualsense"),
            ("XSX-01", "microsoft/xbox-series"),
            ("DS5-02", "sony/dualsense"),
        ]);
        let codes =
            |units: Vec<&Unit>| units.iter().map(|u| u.code.to_string()).collect::<Vec<_>>();
        assert_eq!(
            codes(lab.proposed_units("sony/dualsense", None)),
            ["DS5-01", "DS5-02"]
        );
        let last = Code::new("DS5-02").unwrap();
        assert_eq!(
            codes(lab.proposed_units("sony/dualsense", Some(&last))),
            ["DS5-02", "DS5-01"]
        );
        assert!(lab.proposed_units("nintendo/switch-pro", None).is_empty());
    }

    #[test]
    fn transport_fit() {
        assert!(transports_fit(Transport::Unknown, Transport::Dongle));
        assert!(transports_fit(Transport::Bluetooth, Transport::BluetoothLe));
        assert!(!transports_fit(Transport::Usb, Transport::Bluetooth));
    }
}
