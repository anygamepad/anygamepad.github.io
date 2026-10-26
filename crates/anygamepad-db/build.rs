//! Build script for `anygamepad-db`: compiles curated profiles and SDL
//! GameControllerDB entries into compact postcard binary files in `OUT_DIR`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/input.rs"]
mod input;
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

fn main() {
    let root = manifest_dir();
    let out = out_dir();

    let profiles_dir = root.join("../../data/profiles");
    build_curated(&profiles_dir, &out.join("curated.postcard"));

    let sdl_file = root.join("../../data/sdl-gamecontrollerdb/gamecontrollerdb.txt");
    build_sdl(&sdl_file, &out.join("sdl.postcard"));
}
