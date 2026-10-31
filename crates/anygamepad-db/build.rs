//! Build script for `anygamepad-db`: compiles curated profiles and SDL
//! GameControllerDB entries into compact postcard binary files in `OUT_DIR`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/catalog.rs"]
mod catalog;
#[path = "src/input.rs"]
mod input;
#[path = "src/lab.rs"]
mod lab;
#[path = "src/plan.rs"]
mod plan;
#[path = "src/profile.rs"]
mod profile;
#[path = "src/sdl.rs"]
mod sdl;
#[path = "src/wire.rs"]
mod wire;

pub use input::{AxisRange, Input, ParseInputError};
pub use profile::{
    AxisMapping, Indices, InvalidMapping, InvalidProfile, Mapping, MatchRule, MatchScore, Profile,
    ProfileSource, SCHEMA_VERSION,
};

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").unwrap())
}

fn build_curated(profiles_dir: &Path, out_file: &Path) {
    println!("cargo:rerun-if-changed={}", profiles_dir.display());
    let mut entries: Vec<PathBuf> = fs::read_dir(profiles_dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", profiles_dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    entries.sort();

    let mut profiles = Vec::with_capacity(entries.len());
    for path in entries {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let profile: Profile =
            toml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        profile
            .validate()
            .unwrap_or_else(|e| panic!("validating {}: {e}", path.display()));
        profiles.push(profile);
    }

    let bytes = wire::encode_profiles(&profiles)
        .unwrap_or_else(|e| panic!("encoding curated profiles: {e}"));
    fs::write(out_file, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", out_file.display()));
}

fn build_sdl(sdl_file: &Path, out_file: &Path) {
    println!("cargo:rerun-if-changed={}", sdl_file.display());
    let text = fs::read_to_string(sdl_file)
        .unwrap_or_else(|e| panic!("reading {}: {e}", sdl_file.display()));

    let mut profiles = Vec::new();
    for (line, result) in sdl::import(&text, sdl::VENDORED_ATTRIBUTION) {
        match result {
            Ok(profile) => profiles.push(profile),
            Err(e) => panic!("{}:{line}: {e}", sdl_file.display()),
        }
    }

    let bytes =
        wire::encode_profiles(&profiles).unwrap_or_else(|e| panic!("encoding SDL profiles: {e}"));
    fs::write(out_file, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", out_file.display()));
}

/// Every `<vendor>/<model>.toml` under `dir`, sorted.
fn model_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for vendor in fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display())) {
        let vendor = vendor.unwrap().path();
        if !vendor.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&vendor).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The catalog of `data/controllers/`: every model parsed, validated, its ID
/// equal to its path, IDs unique.
fn build_catalog(dir: &Path, out_file: &Path) -> catalog::Catalog {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut models = Vec::new();
    for path in model_files(dir) {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let model: catalog::Model =
            toml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        let id = path
            .strip_prefix(dir)
            .unwrap()
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(
            model.id,
            id,
            "{}: the ID is not the file's path",
            path.display()
        );
        models.push(model);
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    let catalog = catalog::Catalog::new(models).unwrap_or_else(|e| panic!("{e}"));
    let bytes = catalog::encode_models(catalog.models())
        .unwrap_or_else(|e| panic!("encoding the catalog: {e}"));
    fs::write(out_file, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", out_file.display()));
    catalog
}

/// The inventory of `data/lab/inventory.toml`, checked against the
/// catalog; an empty one when the file does not exist.
fn build_inventory(dir: &Path, catalog: &catalog::Catalog, out_file: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let path = dir.join("inventory.toml");
    let inventory = match fs::read_to_string(&path) {
        Ok(text) => {
            let inventory: lab::Inventory =
                toml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
            inventory
                .validate(Some(catalog))
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            inventory
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => lab::Inventory {
            schema_version: lab::INVENTORY_SCHEMA_VERSION,
            ..lab::Inventory::default()
        },
        Err(e) => panic!("reading {}: {e}", path.display()),
    };
    let bytes =
        lab::encode_inventory(&inventory).unwrap_or_else(|e| panic!("encoding the inventory: {e}"));
    fs::write(out_file, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", out_file.display()));
}

/// The test plans of `data/test-plans/`: every plan parsed, validated, its
/// ID equal to its file name, in ID order.
fn build_plans(dir: &Path, out_file: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut plans = Vec::new();
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let plan: plan::TestPlan =
            toml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        plan.validate()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let stem = path.file_stem().unwrap().to_string_lossy();
        assert_eq!(
            plan.id,
            stem,
            "{}: the ID is not the file's name",
            path.display()
        );
        plans.push(plan);
    }
    let bytes = plan::encode_plans(&plans).unwrap_or_else(|e| panic!("encoding the plans: {e}"));
    fs::write(out_file, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", out_file.display()));
}

fn main() {
    let root = manifest_dir();
    let out = out_dir();

    build_plans(
        &root.join("../../data/test-plans"),
        &out.join("plans.postcard"),
    );

    let catalog = build_catalog(
        &root.join("../../data/controllers"),
        &out.join("catalog.postcard"),
    );
    build_inventory(
        &root.join("../../data/lab"),
        &catalog,
        &out.join("inventory.postcard"),
    );

    let profiles_dir = root.join("../../data/profiles");
    build_curated(&profiles_dir, &out.join("curated.postcard"));

    let sdl_file = root.join("../../data/sdl-gamecontrollerdb/gamecontrollerdb.txt");
    build_sdl(&sdl_file, &out.join("sdl.postcard"));
}
