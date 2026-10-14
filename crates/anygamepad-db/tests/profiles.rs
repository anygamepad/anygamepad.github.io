//! The committed JSON Schema is up to date, and every profile in
//! `data/profiles/` parses, validates, passes the schema and survives a
//! round trip through `to_toml`.

use std::path::{Path, PathBuf};

use anygamepad_db::{Profile, json_schema};

fn profiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/profiles")
}

fn validator() -> jsonschema::Validator {
    let schema = serde_json::to_value(json_schema()).unwrap();
    jsonschema::validator_for(&schema).expect("the schema is a valid JSON Schema")
}

/// The schema errors of a TOML document, read as JSON.
fn schema_errors(validator: &jsonschema::Validator, toml: &str) -> Vec<String> {
    let value: serde_json::Value = toml::from_str(toml).unwrap();
    validator
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

#[test]
fn schema_is_up_to_date() {
    let path = profiles_dir().join("schema.json");
    let expected = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    // Git may check the file out with CRLF line ends (Windows).
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == expected,
        "data/profiles/schema.json is stale; regenerate it with \
         `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test profiles`"
    );
}

#[test]
fn profiles_are_valid() {
    let validator = validator();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(profiles_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());

    for path in paths {
        let name = path.display();
        let text = std::fs::read_to_string(&path).unwrap();
        let profile = Profile::from_toml(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let errors = schema_errors(&validator, &text);
        assert!(errors.is_empty(), "{name}: {errors:#?}");

        let written = profile.to_toml().unwrap();
        assert_eq!(Profile::from_toml(&written).unwrap(), profile, "{name}");
        assert!(schema_errors(&validator, &written).is_empty(), "{name}");
    }
}

/// The schema rejects what the types reject, for the parts written by hand.
#[test]
fn schema_rejects_invalid_profiles() {
    let validator = validator();
    let valid = r#"
        schema_version = 1
        name = "Pad"
        source = "curated"
        verified = false

        [[match]]
        vendor_id = 0x054c

        [[mapping]]
        buttons = { south = "b0", extra_3 = "b{evdev:0x0130}" }
        axes = { left_stick_y = "a1~", left_trigger = { input = "+a2", deadzone = 0.1 } }
    "#;
    assert_eq!(schema_errors(&validator, valid), Vec::<String>::new());
    Profile::from_toml(valid).unwrap();

    for (from, to) in [
        ("source = \"curated\"", "source = \"handmade\""),
        ("vendor_id = 0x054c", "vendor = 0x054c"),
        ("vendor_id = 0x054c", "vendor_id = 0x10000"),
        ("south = \"b0\"", "cross = \"b0\""),
        ("south = \"b0\"", "extra_32 = \"b0\""),
        ("south = \"b0\"", "south = \"b00\""),
        ("south = \"b0\"", "south = \"h0.16\""),
        ("left_stick_y = \"a1~\"", "left_stick_y = \"a1~~\""),
        ("left_stick_y = \"a1~\"", "left_stick_z = \"a1\""),
        ("input = \"+a2\"", "input = \"+a2\", positive = \"b6\""),
        ("input = \"+a2\"", "inputs = \"+a2\""),
        ("deadzone = 0.1", "deadzone = 1.0"),
    ] {
        assert!(valid.contains(from), "{from}");
        let invalid = valid.replace(from, to);
        assert!(
            !schema_errors(&validator, &invalid).is_empty(),
            "the schema accepts `{to}`"
        );
        assert!(
            Profile::from_toml(&invalid).is_err(),
            "the types accept `{to}`"
        );
    }
}

/// The example in `data/profiles/README.md` is a valid profile.
#[test]
fn readme_example_is_valid() {
    let readme = std::fs::read_to_string(profiles_dir().join("README.md")).unwrap();
    let readme = readme.replace("\r\n", "\n");
    let start = readme.find("```toml\n").expect("a TOML example") + "```toml\n".len();
    let example = &readme[start..start + readme[start..].find("```").unwrap()];
    let profile = Profile::from_toml(example).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(profile.matches.len(), 2);
    assert_eq!(schema_errors(&validator(), example), Vec::<String>::new());
}
