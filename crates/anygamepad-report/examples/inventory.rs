//! Builds the lab inventory from test reports (roadmap task F4): adds to an
//! inventory file each unit that the test reports of a folder name and the
//! file lacks. The new `[[unit]]` tables go at the end of the file, so what
//! it has (notes, machines, comments) stays as written.
//!
//! ```sh
//! cargo run -p anygamepad-report --features json --example inventory -- <folder of test reports> data/lab/inventory.toml
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anygamepad_db::lab::INVENTORY_SCHEMA_VERSION;
use anygamepad_db::{Catalog, Inventory};
use anygamepad_report::test_report::{TestReport, add_units};

const HEADER: &str = "#:schema ./schema.json

# The lab inventory (roadmap task F4), built from test reports with
# `cargo run -p anygamepad-report --features json --example inventory`.
# Notes and machines are written by hand; the tool keeps them.

";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [reports, inventory] = args.as_slice() else {
        eprintln!("usage: inventory <folder of test reports> <inventory.toml>");
        return ExitCode::from(2);
    };
    match run(Path::new(reports), Path::new(inventory)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(folder: &Path, path: &Path) -> Result<(), String> {
    let catalog = Catalog::embedded();
    let mut text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            format!("{HEADER}schema_version = {INVENTORY_SCHEMA_VERSION}\n")
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut inventory = Inventory::from_toml(&text, Some(catalog))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    let mut reports = Vec::new();
    for file in &files {
        let read = std::fs::read_to_string(file).map_err(|e| e.to_string());
        match read.and_then(|text| TestReport::from_json(&text).map_err(|e| e.to_string())) {
            Ok(report) => reports.push(report),
            Err(error) => eprintln!("skipped {}: {error}", file.display()),
        }
    }
    let update = add_units(&mut inventory, &reports);
    for unit in &update.added {
        println!("added {} ({})", unit.code, unit.model);
    }
    for (code, kept, other) in &update.conflicts {
        eprintln!("left out: {code} is {kept} here, but a report says {other}");
    }
    if update.added.is_empty() {
        println!("{} test reports read; nothing to add", reports.len());
        return Ok(());
    }
    for unit in &update.added {
        // Codes and catalog IDs never need TOML escapes.
        text.push_str(&format!(
            "\n[[unit]]\ncode = \"{}\"\nmodel = \"{}\"\n",
            unit.code, unit.model
        ));
    }
    let written = Inventory::from_toml(&text, Some(catalog))
        .map_err(|e| format!("the inventory would not be valid: {e}"))?;
    if written != inventory {
        return Err("the appended units do not read back".into());
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{} test reports read; {} units added to {}",
        reports.len(),
        update.added.len(),
        path.display()
    );
    Ok(())
}
