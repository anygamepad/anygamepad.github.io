//! The committed JSON Schema of test plans is up to date, and every plan in
//! `data/test-plans/` parses, validates, passes the schema, is named after
//! its ID, survives a round trip through `to_toml` and is embedded.

use std::path::{Path, PathBuf};

use anygamepad_db::TestPlan;
use anygamepad_db::plan::json_schema;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/test-plans")
}

fn schema_errors(toml: &str) -> Vec<String> {
    let schema = serde_json::to_value(json_schema()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let value: serde_json::Value = toml::from_str(toml).unwrap();
    validator
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

#[test]
fn schema_is_up_to_date() {
    let path = dir().join("schema.json");
    let expected = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == expected,
        "data/test-plans/schema.json is stale; regenerate it with \
         `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test plans`"
    );
}

#[test]
fn plans_are_valid_and_embedded() {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut plans = Vec::new();
    for path in paths {
        let name = path.display();
        let text = std::fs::read_to_string(&path).unwrap();
        let plan = TestPlan::from_toml(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            plan.id,
            path.file_stem().unwrap().to_string_lossy(),
            "{name}"
        );
        let errors = schema_errors(&text);
        assert!(errors.is_empty(), "{name}: {errors:#?}");
        let written = plan.to_toml().unwrap();
        assert_eq!(TestPlan::from_toml(&written).unwrap(), plan, "{name}");
        plans.push(plan);
    }
    assert_eq!(TestPlan::embedded(), &plans[..]);
}

#[test]
fn schema_rejects_unknown_checks() {
    let toml = "schema_version = 1\nid = \"x\"\nversion = 1\nname = \"X\"\ndescription = \"X.\"\n[[steps]]\nid = \"a\"\ncheck = \"jump\"\n";
    assert!(!schema_errors(toml).is_empty());
    assert!(TestPlan::from_toml(toml).is_err());
}
