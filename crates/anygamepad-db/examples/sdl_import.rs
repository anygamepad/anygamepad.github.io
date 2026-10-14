//! Prints SDL GameControllerDB entries as anygamepad profiles (TOML), to
//! start a curated profile from SDL's data or to review the conversion.
//!
//! ```text
//! cargo run -p anygamepad-db --features toml --example sdl_import -- \
//!     data/sdl-gamecontrollerdb/gamecontrollerdb.txt [VID:PID]
//! ```
//!
//! With `VID:PID` (hexadecimal, `054c:09cc`), only the entries for that
//! device are printed.

use std::process::ExitCode;

use anygamepad_db::sdl::{self, VENDORED_ATTRIBUTION};

fn parse_ids(arg: &str) -> Option<(u16, u16)> {
    let (vendor, product) = arg.split_once(':')?;
    Some((
        u16::from_str_radix(vendor, 16).ok()?,
        u16::from_str_radix(product, 16).ok()?,
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, ids) = match args.as_slice() {
        [path] => (path, None),
        [path, ids] => match parse_ids(ids) {
            Some(ids) => (path, Some(ids)),
            None => {
                eprintln!("expected VID:PID in hexadecimal, got `{ids}`");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: sdl_import <gamecontrollerdb.txt> [VID:PID]");
            return ExitCode::FAILURE;
        }
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut failed = false;
    for (line, result) in sdl::import(&text, VENDORED_ATTRIBUTION) {
        let profile = match result {
            Ok(profile) => profile,
            Err(error) => {
                eprintln!("{path}:{line}: {error}");
                failed = true;
                continue;
            }
        };
        let rule = &profile.matches[0];
        if ids.is_some_and(|(vendor, product)| {
            rule.vendor_id != Some(vendor) || rule.product_id != Some(product)
        }) {
            continue;
        }
        match profile.to_toml() {
            Ok(toml) => println!("# {path}:{line}\n{toml}"),
            Err(error) => {
                eprintln!("{path}:{line}: {error}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
