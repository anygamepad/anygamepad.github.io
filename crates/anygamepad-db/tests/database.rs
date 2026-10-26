//! Tests for [`Database`]: compile-time embedded profiles and runtime overrides.

use anygamepad_db::{Database, Profile, ProfileSource};
use anygamepad_model::Transport;
use anygamepad_model::identity::{
    Backend, Browser, BrowserEngine, DeviceIdentity, Os, ProviderKind,
};

fn chrome_standard_pad() -> DeviceIdentity {
    let browser = Browser {
        engine: BrowserEngine::Blink,
        major_version: Some(141),
    };
    DeviceIdentity::from_gamepad_api(
        "Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)",
        "standard",
        browser,
        Os::Linux,
    )
}

fn linux_dualshock4_identity() -> DeviceIdentity {
    DeviceIdentity {
        transport: Transport::Usb,
        vendor_id: Some(0x054c),
        product_id: Some(0x09cc),
        version: Some(0x8111),
        vendor_name: Some("Sony Interactive Entertainment".into()),
        product_name: Some("Wireless Controller".into()),
        driver: Some("hid-playstation".into()),
        ..DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux)
    }
}

#[test]
fn embedded_database_contains_curated_and_sdl() {
    let db = Database::embedded();
    assert!(!db.is_empty());

    // On native with embedded-sdl, contains 1 curated + 2,279 SDL entries.
    #[cfg(all(feature = "embedded-sdl", not(target_arch = "wasm32")))]
    {
        assert_eq!(db.len(), 2280);
        let match_ds4 = db.best_match(&linux_dualshock4_identity());
        assert!(match_ds4.is_some());
        assert_eq!(match_ds4.unwrap().name, "PS4 Controller");
    }

    // Curated standard profile matches standard gamepad API device
    let standard = db.best_match(&chrome_standard_pad());
    assert!(standard.is_some());
    assert_eq!(standard.unwrap().name, "Gamepad API standard layout");
}

#[test]
fn embedded_curated_only_contains_curated_profiles() {
    let db = Database::embedded_curated();
    assert_eq!(db.len(), 1);
    let profile = db.best_match(&chrome_standard_pad()).unwrap();
    assert_eq!(profile.name, "Gamepad API standard layout");
    assert_eq!(profile.source, ProfileSource::Curated);

    // SDL device is not matched in curated-only db
    assert!(db.best_match(&linux_dualshock4_identity()).is_none());
}

#[test]
fn runtime_override_takes_precedence_over_embedded() {
    let mut db = Database::embedded();

    // The embedded standard profile matches first
    let initial = db.best_match(&chrome_standard_pad()).unwrap();
    assert_eq!(initial.name, "Gamepad API standard layout");

    // Add a custom override profile that matches the same device
    let override_toml = r#"
        schema_version = 1
        name = "My Custom Standard Override"
        source = "curated"
        verified = true

        [[match]]
        backend = "gamepad_api"
        gamepad_mapping = "standard"

        [[mapping]]
        buttons = { south = "b0", east = "b1" }
    "#;

    db.load_override_toml(override_toml)
        .expect("valid override");
    assert_eq!(db.overrides().len(), 1);

    // Now the override matches instead
    let matched = db.best_match(&chrome_standard_pad()).unwrap();
    assert_eq!(matched.name, "My Custom Standard Override");
    assert!(matched.verified);
}

#[test]
fn load_override_toml_rejects_invalid() {
    let mut db = Database::empty();
    let malformed = "this is not valid toml = =";
    assert!(db.load_override_toml(malformed).is_err());

    let invalid_schema = r#"
        schema_version = 999
        name = "Invalid Schema Version"
        source = "curated"
        verified = false
        [[match]]
        vendor_id = 0x054c
    "#;
    assert!(db.load_override_toml(invalid_schema).is_err());
    assert!(db.is_empty());
}

#[test]
fn empty_and_with_profiles() {
    let empty = Database::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.best_match(&chrome_standard_pad()).is_none());

    let toml = include_str!("../../../data/profiles/gamepad-api-standard.toml");
    let profile = Profile::from_toml(toml).unwrap();
    let custom = Database::with_profiles(vec![profile]);
    assert_eq!(custom.len(), 1);
    assert!(!custom.is_empty());
    assert!(custom.best_match(&chrome_standard_pad()).is_some());
}

#[test]
fn database_clone_shares_embedded_profiles() {
    let mut db1 = Database::embedded();
    let db2 = db1.clone();
    assert_eq!(db1, db2);

    // Adding override to db1 does not affect db2
    let override_toml = r#"
        schema_version = 1
        name = "Local Only"
        source = "curated"
        verified = false
        [[match]]
        vendor_id = 0x1234
    "#;
    db1.load_override_toml(override_toml).unwrap();
    assert_eq!(db1.len(), db2.len() + 1);
    assert_ne!(db1, db2);
}
