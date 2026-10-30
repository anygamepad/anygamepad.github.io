//! The app's settings, kept in `settings.toml`.
//!
//! Unknown keys are ignored and missing ones take their default, so files
//! written by other versions load. Settings are saved when they change.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::widget::WidgetSkin;

/// Colour theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the system.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// How much goes into the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Errors and warnings.
    Warn,
    /// Also what the app does (devices found, settings saved).
    #[default]
    Info,
    /// Everything, for bug reports.
    Debug,
}

impl LogLevel {
    /// The `log` crate's filter.
    pub fn filter(self) -> log::LevelFilter {
        match self {
            Self::Error => log::LevelFilter::Error,
            Self::Warn => log::LevelFilter::Warn,
            Self::Info => log::LevelFilter::Info,
            Self::Debug => log::LevelFilter::Debug,
        }
    }
}

/// Everything the user can set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Colour theme.
    pub theme: Theme,
    /// Start hidden in the system tray (when there is one).
    pub start_minimized: bool,
    /// Closing the window hides it in the tray instead of quitting (when
    /// there is one).
    pub close_to_tray: bool,
    /// How much goes into the log.
    pub log_level: LogLevel,
    /// The person started collection mode once, after reading what it
    /// does (A3): the explanation is not shown again.
    pub collection_started: bool,
    /// Collection mode is on: every controller gets its report file.
    pub collecting: bool,
    /// The always-on-top widget is shown (A11).
    pub widget: bool,
    /// How the widget draws the controller.
    pub widget_skin: WidgetSkin,
    /// The widget's background opacity, 0-100.
    pub widget_opacity: u8,
    /// Clicks go through the widget to the window below.
    pub widget_click_through: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            start_minimized: false,
            close_to_tray: false,
            log_level: LogLevel::default(),
            collection_started: false,
            collecting: false,
            widget: false,
            widget_skin: WidgetSkin::default(),
            widget_opacity: 80,
            widget_click_through: false,
        }
    }
}

impl Settings {
    /// Reads `path`; the defaults when it does not exist. A file that does
    /// not parse is an error, so that it is not overwritten unnoticed.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Writes `path` (through a temporary file, so a crash never leaves
    /// half a file).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string(self).map_err(io::Error::other)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("anygamepad-settings-{}", std::process::id()));
        let path = dir.join("nested/settings.toml");
        assert_eq!(Settings::load(&path), Ok(Settings::default()));
        let settings = Settings {
            theme: Theme::Dark,
            start_minimized: true,
            close_to_tray: true,
            log_level: LogLevel::Debug,
            collection_started: true,
            collecting: false,
            widget: true,
            widget_skin: WidgetSkin::Compact,
            widget_opacity: 50,
            widget_click_through: true,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Ok(settings));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("theme = \"dark\""), "{text}");

        // Unknown keys are ignored, missing ones take their default.
        fs::write(&path, "theme = \"light\"\nfuture_option = 3\n").unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        assert!(!loaded.close_to_tray);
        fs::write(&path, "theme = \"purple\"").unwrap();
        assert!(Settings::load(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
