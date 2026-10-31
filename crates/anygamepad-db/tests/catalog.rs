//! The committed JSON Schemas of the catalog and the inventory are up to
//! date; every model in `data/controllers/` parses, validates, passes the
//! schema, sits at the path its ID names and survives a round trip through
//! `to_toml`; the example inventory validates against the catalog.

use std::path::{Path, PathBuf};

use anygamepad_db::Catalog;
use anygamepad_db::catalog::{self, Model};
use anygamepad_db::lab::{self, Inventory};
use anygamepad_model::Family;

fn data_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(name)
}

fn validator(schema: schemars::Schema) -> jsonschema::Validator {
    let schema = serde_json::to_value(schema).unwrap();
    jsonschema::validator_for(&schema).expect("the schema is a valid JSON Schema")
}

fn schema_errors(validator: &jsonschema::Validator, toml: &str) -> Vec<String> {
    let value: serde_json::Value = toml::from_str(toml).unwrap();
    validator
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect()
}

fn check_schema(path: PathBuf, schema: schemars::Schema) {
    let expected = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == expected,
        "{} is stale; regenerate it with \
         `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test catalog`",
        path.display()
    );
}

#[test]
fn schemas_are_up_to_date() {
    check_schema(data_dir("controllers/schema.json"), catalog::json_schema());
    check_schema(data_dir("lab/schema.json"), lab::json_schema());
}

fn model_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for vendor in std::fs::read_dir(data_dir("controllers")).unwrap() {
        let vendor = vendor.unwrap().path();
        if vendor.is_dir() {
            for entry in std::fs::read_dir(vendor).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|ext| ext == "toml") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

#[test]
fn models_are_valid() {
    let validator = validator(catalog::json_schema());
    let dir = data_dir("controllers");
    let files = model_files();
    assert!(!files.is_empty());
    let mut models = Vec::new();
    for path in files {
        let name = path.display();
        let text = std::fs::read_to_string(&path).unwrap();
        let model = Model::from_toml(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let errors = schema_errors(&validator, &text);
        assert!(errors.is_empty(), "{name}: {errors:#?}");
        let id = path.strip_prefix(&dir).unwrap().with_extension("");
        assert_eq!(model.id, id.to_string_lossy().replace('\\', "/"), "{name}");

        let written = model.to_toml().unwrap();
        assert_eq!(Model::from_toml(&written).unwrap(), model, "{name}");
        assert!(schema_errors(&validator, &written).is_empty(), "{name}");
        models.push(model);
    }
    // The embedded catalog is these files, in ID order.
    models.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(Catalog::embedded().models(), &models[..]);
}

#[test]
fn generic_models_cover_every_kind() {
    let catalog = Catalog::embedded();
    for id in [
        "generic/xinput",
        "generic/playstation",
        "generic/switch-pro",
        "generic/hid",
    ] {
        assert!(catalog.get(id).is_some_and(|model| model.generic), "{id}");
    }
}

#[test]
fn identities_agree_with_families() {
    // The family rules of the model crate and the catalog say the same of
    // the products both know.
    for model in Catalog::embedded().models() {
        let Some(family) = model.family else { continue };
        for connection in model.modes.iter().flat_map(|mode| &mode.connections) {
            for identity in &connection.identities {
                if let (Some(vendor), Some(product)) = (identity.vendor_id, identity.product_id) {
                    assert_eq!(
                        Family::from_ids(vendor, product),
                        Some(family),
                        "{} {vendor:04x}:{product:04x}",
                        model.id
                    );
                }
            }
        }
    }
}

#[test]
fn example_inventory_is_valid() {
    let validator = validator(lab::json_schema());
    let path = data_dir("lab/inventory.example.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let inventory = Inventory::from_toml(&text, Some(Catalog::embedded()))
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let errors = schema_errors(&validator, &text);
    assert!(errors.is_empty(), "{errors:#?}");
    let written = inventory.to_toml().unwrap();
    assert_eq!(Inventory::from_toml(&written, None).unwrap(), inventory);
}

#[test]
fn schemas_reject_what_the_types_reject() {
    let inventory_schema = validator(lab::json_schema());
    for code in ["", "-x", "Ana's pad", "0123456789ABCDEFG"] {
        let toml =
            format!("schema_version = 1\n[[unit]]\ncode = {code:?}\nmodel = \"sony/dualsense\"\n");
        assert!(
            !schema_errors(&inventory_schema, &toml).is_empty(),
            "{code}"
        );
        assert!(Inventory::from_toml(&toml, None).is_err(), "{code}");
    }
    let model_schema = validator(catalog::json_schema());
    let unknown_field = "schema_version = 1\nid = \"a/b\"\nvendor = \"A\"\nname = \"B\"\ncolour = \"red\"\nmodes = []\n[controls]\nbuttons = [\"south\"]\n";
    assert!(!schema_errors(&model_schema, unknown_field).is_empty());
    assert!(Model::from_toml(unknown_field).is_err());
}

#[test]
fn readme_examples_are_valid() {
    fn toml_block(path: PathBuf) -> String {
        let text = std::fs::read_to_string(path).unwrap();
        let start = text.find("```toml\n").unwrap() + "```toml\n".len();
        text[start..][..text[start..].find("```").unwrap()].to_owned()
    }
    let model = Model::from_toml(&toml_block(data_dir("controllers/README.md"))).unwrap();
    assert_eq!(model.modes.len(), 2);
    Inventory::from_toml(
        &toml_block(data_dir("lab/README.md")),
        Some(Catalog::embedded()),
    )
    .unwrap();
}
