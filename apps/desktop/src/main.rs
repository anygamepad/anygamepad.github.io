//! The anygamepad desktop app (A1: the shell; device pages follow in A2,
//! A3 and the virtual pad tasks).
//!
//! UI toolkit: egui through eframe (ADR 0004).

#![forbid(unsafe_code)]
// A GUI app on Windows: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod collection;
mod collection_page;
mod demo;
mod devices;
mod devices_page;
mod logging;
mod paths;
mod settings;
mod style;
mod tray;
mod widget;

use std::sync::Arc;

use eframe::egui;

use crate::paths::Paths;
use crate::settings::Settings;

/// The app icon: the logo's mark, 64×64 RGBA, rendered from
/// `apps/web/public/favicon.svg`.
pub struct Icon {
    /// Width and height in pixels.
    pub size: u32,
    /// RGBA pixels, row by row.
    pub rgba: &'static [u8],
}

impl Icon {
    /// The app icon.
    pub fn app() -> Self {
        Self {
            size: 64,
            rgba: include_bytes!("../assets/icon-64.rgba"),
        }
    }
}

fn main() -> eframe::Result {
    let paths = Paths::from_env();
    let settings_file = paths.as_ref().map(Paths::settings_file);
    let log_file = paths.as_ref().map(Paths::log_file);
    let reports = paths.as_ref().map(Paths::reports);
    let (settings, settings_error) = match settings_file.as_deref().map(Settings::load) {
        Some(Ok(settings)) => (settings, None),
        Some(Err(error)) => (
            Settings::default(),
            Some(format!(
                "The settings file could not be read, defaults are in use: {error}"
            )),
        ),
        None => (Settings::default(), None),
    };
    logging::init(settings.log_level.filter(), log_file.as_deref());
    log::info!("AnyGamepad {} starting", env!("CARGO_PKG_VERSION"));
    if let Some(error) = &settings_error {
        log::warn!("{error}");
    }
    // A settings file that does not parse is kept as it is: saving only
    // happens when the user changes something.

    let icon = Icon::app();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyGamepad")
            .with_app_id("anygamepad")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([480.0, 320.0])
            .with_icon(Arc::new(egui::IconData {
                rgba: icon.rgba.to_vec(),
                width: icon.size,
                height: icon.size,
            })),
        ..Default::default()
    };
    let result = eframe::run_native(
        "AnyGamepad",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::DesktopApp::new(
                cc,
                settings,
                settings_file,
                settings_error,
                log_file,
                reports,
            )))
        }),
    );
    log::info!("AnyGamepad stopped");
    result
}
