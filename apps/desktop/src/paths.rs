//! Where the app keeps its settings and its log, per OS conventions.

use std::path::PathBuf;

/// The directory name under each base directory.
const APP: &str = "anygamepad";

/// The app's directories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// Settings (`settings.toml`).
    pub config: PathBuf,
    /// Logs (`anygamepad.log`).
    pub logs: PathBuf,
    /// Data the user keeps: device reports in `reports/` (A3).
    pub data: PathBuf,
}

impl Paths {
    /// The directories for this OS, from the environment; `None` without a
    /// home directory.
    pub fn from_env() -> Option<Self> {
        Self::for_os(std::env::consts::OS, |name| {
            std::env::var_os(name).map(PathBuf::from)
        })
    }

    /// The directories on `os` (`std::env::consts::OS`), reading
    /// environment variables through `var`.
    pub fn for_os(os: &str, var: impl Fn(&str) -> Option<PathBuf>) -> Option<Self> {
        let non_empty = |name: &str| var(name).filter(|path| !path.as_os_str().is_empty());
        match os {
            "windows" => {
                let roaming = non_empty("APPDATA")?;
                let local = non_empty("LOCALAPPDATA").unwrap_or_else(|| roaming.clone());
                Some(Self {
                    config: roaming.join(APP),
                    logs: local.join(APP).join("logs"),
                    data: roaming.join(APP),
                })
            }
            "macos" => {
                let home = non_empty("HOME")?;
                Some(Self {
                    config: home.join("Library/Application Support").join(APP),
                    logs: home.join("Library/Logs").join(APP),
                    data: home.join("Library/Application Support").join(APP),
                })
            }
            // Linux and the other Unix systems: XDG base directories.
            _ => {
                let home = non_empty("HOME");
                let config =
                    non_empty("XDG_CONFIG_HOME").or_else(|| Some(home.clone()?.join(".config")))?;
                let state = non_empty("XDG_STATE_HOME")
                    .or_else(|| Some(home.clone()?.join(".local/state")))?;
                let data =
                    non_empty("XDG_DATA_HOME").or_else(|| Some(home?.join(".local/share")))?;
                Some(Self {
                    config: config.join(APP),
                    logs: state.join(APP),
                    data: data.join(APP),
                })
            }
        }
    }

    /// The settings file.
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    /// The log file.
    pub fn log_file(&self) -> PathBuf {
        self.logs.join("anygamepad.log")
    }

    /// The folder of device report files.
    pub fn reports(&self) -> PathBuf {
        self.data.join("reports")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| PathBuf::from(value))
        }
    }

    #[test]
    fn linux_follows_xdg() {
        let paths = Paths::for_os("linux", env(&[("HOME", "/home/a")])).unwrap();
        assert_eq!(
            paths.settings_file(),
            PathBuf::from("/home/a/.config/anygamepad/settings.toml")
        );
        assert_eq!(
            paths.log_file(),
            PathBuf::from("/home/a/.local/state/anygamepad/anygamepad.log")
        );
        let paths = Paths::for_os(
            "linux",
            env(&[
                ("HOME", "/home/a"),
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_STATE_HOME", ""),
            ]),
        )
        .unwrap();
        assert_eq!(paths.config, PathBuf::from("/c/anygamepad"));
        // An empty variable counts as unset.
        assert_eq!(paths.logs, PathBuf::from("/home/a/.local/state/anygamepad"));
        assert_eq!(
            paths.reports(),
            PathBuf::from("/home/a/.local/share/anygamepad/reports")
        );
        assert_eq!(Paths::for_os("linux", env(&[])), None);
    }

    #[test]
    fn windows_and_macos() {
        let paths = Paths::for_os(
            "windows",
            env(&[
                ("APPDATA", r"C:\Users\a\AppData\Roaming"),
                ("LOCALAPPDATA", r"C:\L"),
            ]),
        )
        .unwrap();
        assert_eq!(
            paths.config,
            PathBuf::from(r"C:\Users\a\AppData\Roaming").join("anygamepad")
        );
        assert_eq!(
            paths.logs,
            PathBuf::from(r"C:\L").join("anygamepad").join("logs")
        );
        let paths = Paths::for_os("macos", env(&[("HOME", "/Users/a")])).unwrap();
        assert_eq!(
            paths.logs,
            PathBuf::from("/Users/a/Library/Logs/anygamepad")
        );
    }
}
