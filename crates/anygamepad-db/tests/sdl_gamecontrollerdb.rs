//! The copy of SDL GameControllerDB in `data/sdl-gamecontrollerdb/`
//! imports without errors, and its profiles pass the schema and match the
//! devices they are for.

use std::collections::BTreeMap;
use std::path::Path;

use anygamepad_db::sdl::{self, VENDORED_ATTRIBUTION};
use anygamepad_db::{Input, Profile, best_match, json_schema};
use anygamepad_model::identity::{Backend, DeviceIdentity, Os, ProviderKind};
use anygamepad_model::{Axis, Button, Family, Transport};

fn vendored() -> Vec<Profile> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/sdl-gamecontrollerdb/gamecontrollerdb.txt");
    let text = std::fs::read_to_string(path).unwrap();
    let mut profiles = Vec::new();
    let mut errors = Vec::new();
    for (line, result) in sdl::import(&text, VENDORED_ATTRIBUTION) {
        match result {
            Ok(profile) => profiles.push(profile),
            Err(error) => errors.push(format!("line {line}: {error}")),
        }
    }
    assert!(errors.is_empty(), "{errors:#?}");
    profiles
}

#[test]
fn imports_every_entry() {
    let profiles = vendored();
    let mut per_os = BTreeMap::new();
    for profile in &profiles {
        *per_os.entry(profile.matches[0].os.unwrap()).or_insert(0) += 1;
    }
    // Counts of `docs/research/02-sdl-gamecontrollerdb.md`.
    assert_eq!(
        per_os,
        BTreeMap::from([
            (Os::Linux, 744),
            (Os::Windows, 869),
            (Os::MacOs, 325),
            (Os::Android, 299),
            (Os::Ios, 42),
        ])
    );
}

#[test]
fn profiles_pass_the_schema_and_round_trip() {
    let schema = serde_json::to_value(json_schema()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for profile in vendored() {
        let toml = profile.to_toml().unwrap();
        let value: serde_json::Value = toml::from_str(&toml).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{toml}\n{errors:#?}");
        assert_eq!(Profile::from_toml(&toml).unwrap(), profile, "{toml}");
    }
}

#[test]
fn linux_dualshock4_entry_matches_its_device() {
    let profiles = vendored();
    // The DualShock 4 v2 over USB, as the Linux evdev backend sees it with
    // the `hid-playstation` driver (evdev version 0x8111).
    let identity = DeviceIdentity {
        transport: Transport::Usb,
        vendor_id: Some(0x054c),
        product_id: Some(0x09cc),
        version: Some(0x8111),
        vendor_name: Some("Sony Interactive Entertainment".into()),
        product_name: Some("Wireless Controller".into()),
        driver: Some("hid-playstation".into()),
        ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
    };
    let profile = best_match(&profiles, &identity).expect("an entry for the device");
    assert_eq!(profile.name, "PS4 Controller");
    assert_eq!(profile.family, Some(Family::DualShock4));
    assert_eq!(
        profile.matches[0].sdl_guid.unwrap().to_string(),
        "030000004c050000cc09000011810000"
    );
    let mapping = profile.mapping_for(&identity);
    let input = |s: &str| s.parse::<Input>().unwrap();
    // hid-playstation reports BTN_SOUTH first, unlike the older hid-sony.
    assert_eq!(mapping.buttons[&Button::South], input("b0"));
    assert_eq!(mapping.buttons[&Button::North], input("b2"));
    assert_eq!(mapping.axes[&Axis::RightTrigger].input, Some(input("a5")));

    // Over Bluetooth the bus differs: another entry, or none.
    let bluetooth = DeviceIdentity {
        transport: Transport::Bluetooth,
        ..identity.clone()
    };
    assert!(best_match(&profiles, &bluetooth).is_none_or(|other| !std::ptr::eq(other, profile)));
    // Other OSes never match a Linux entry.
    let windows = DeviceIdentity {
        os: Os::Windows,
        ..identity
    };
    assert!(
        best_match(&profiles, &windows)
            .is_none_or(|other| other.matches[0].os == Some(Os::Windows))
    );
}
