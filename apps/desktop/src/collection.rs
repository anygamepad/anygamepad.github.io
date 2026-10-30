//! Collection mode (A3): device reports (P1) written as files, one per
//! device, in the app's `reports` folder (ADR 0007). Nothing is sent; the
//! person shares a report through the same GitHub issue form as the web
//! tester, after seeing it (P3, `docs/privacy/README.md`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anygamepad::model::Capabilities;
use anygamepad::model::identity::{DescriptorHash, Os};
use anygamepad::provider::DeviceInfo;
use anygamepad_report::privacy::Redaction;
use anygamepad_report::{App, Date, Host, Report, Source, UserStatement, View};

use crate::devices::MappingSource;

/// The issue form reports are shared through.
const ISSUE_FORM: &str =
    "https://github.com/anygamepad/anygamepad.github.io/issues/new?template=device-report.yml";

/// What the app knows about a device when it writes its report.
pub struct DeviceFacts<'a> {
    /// Identity, layout and outputs from the provider.
    pub info: &'a DeviceInfo,
    /// Where its mapping comes from.
    pub mapping: &'a MappingSource,
    /// Canonical controls its mapping gives.
    pub capabilities: Option<Capabilities>,
    /// The HID report descriptor, when the OS shows it.
    pub descriptor: Option<Vec<u8>>,
}

/// The host OS of this build.
pub fn host_os() -> Os {
    match std::env::consts::OS {
        "linux" => Os::Linux,
        "windows" => Os::Windows,
        "macos" => Os::MacOs,
        _ => Os::Unknown,
    }
}

/// The OS release: the kernel release on Linux; not read elsewhere yet.
pub fn os_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Today's date (UTC).
pub fn today() -> Date {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    Date::from_unix_seconds(seconds).expect("the clock is past 1970")
}

/// Builds, scrubs and validates the report of one device; returns it with
/// what the scrub replaced.
pub fn build_report(
    facts: &DeviceFacts<'_>,
    user: UserStatement,
    created: Date,
    os_version: Option<String>,
) -> Result<(Report, Vec<Redaction>), String> {
    let source = Source {
        app: App::Desktop,
        app_version: env!("CARGO_PKG_VERSION")
            .parse()
            .map_err(|e| format!("{e}"))?,
        library_version: anygamepad::VERSION.parse().map_err(|e| format!("{e}"))?,
    };
    let host = Host {
        os: host_os(),
        os_version,
        browser: None,
    };
    let mut report = Report::new(source, host, created);
    report.user = user;
    let info = facts.info;
    let mut identity = info.identity.clone();
    if let Some(descriptor) = &facts.descriptor {
        // The hash must be the included descriptor's, read now.
        identity.descriptor_hash = Some(DescriptorHash::of(descriptor));
    }
    let mut view = View::new(identity, info.layout.clone(), info.outputs);
    view.profile = match facts.mapping {
        MappingSource::Profile(name) => Some(name.clone()),
        _ => None,
    };
    view.capabilities = facts.capabilities.clone();
    if let Some(descriptor) = &facts.descriptor {
        view.hid = Some(anygamepad_report::HidData {
            descriptor: Some(anygamepad_report::HexBytes(descriptor.clone())),
            collections: Vec::new(),
            input_samples: Vec::new(),
        });
    }
    report.views.push(view);
    let redactions = report.scrub();
    report.validate().map_err(|e| e.to_string())?;
    Ok((report, redactions))
}

/// The file name of a report: its device's VID/PID and fingerprint, so a
/// device keeps one file however often it is recorded.
pub fn file_name(report: &Report) -> String {
    let Some(view) = report.views.first() else {
        return "report.json".into();
    };
    let id = &view.identity;
    match (id.vendor_id, id.product_id) {
        (Some(v), Some(p)) => format!("{v:04x}-{p:04x}-{}.json", view.fingerprint),
        _ => format!("device-{}.json", view.fingerprint),
    }
}

/// The issue form, its title filled in for `report`.
pub fn issue_url(report: &Report) -> String {
    let name = report
        .views
        .first()
        .and_then(|view| view.identity.product_name.clone())
        .unwrap_or_else(|| "Game Controller".into());
    let fingerprint = report
        .views
        .first()
        .map(|view| view.fingerprint.to_string())
        .unwrap_or_default();
    let title = format!("Report: {name} ({fingerprint})");
    format!("{ISSUE_FORM}&title={}", percent_encode(&title))
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// A saved report file.
#[derive(Clone, Debug)]
pub struct SavedReport {
    /// The file.
    pub path: PathBuf,
    /// Its report.
    pub report: Report,
}

/// The `reports` folder.
pub struct ReportStore {
    dir: PathBuf,
}

impl ReportStore {
    /// The store in `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `report` (formatted JSON), replacing the device's previous
    /// file; returns its path.
    pub fn save(&self, report: &Report) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(file_name(report));
        let text = report.to_json_pretty().map_err(io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, &path)?;
        Ok(path)
    }

    /// The reports in the folder, newest first; files that are not
    /// reports are skipped.
    pub fn list(&self) -> Vec<SavedReport> {
        let Ok(dir) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut reports: Vec<(SystemTime, SavedReport)> = dir
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let path = entry.path();
                let text = fs::read_to_string(&path).ok()?;
                let report = Report::from_json(&text).ok()?;
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH);
                Some((modified, SavedReport { path, report }))
            })
            .collect();
        reports.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        reports.into_iter().map(|(_, report)| report).collect()
    }

    /// Deletes a report file of this folder.
    pub fn delete(&self, path: &Path) -> io::Result<()> {
        if path.parent() != Some(self.dir.as_path()) {
            return Err(io::Error::other("not a report of this folder"));
        }
        fs::remove_file(path)
    }
}

/// Opens a folder or file with the system's default app.
pub fn open_path(path: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("could not open the folder: {e}");
    }
}

#[cfg(test)]
mod tests {
    use anygamepad::model::identity::{Backend, DeviceIdentity, ProviderKind};
    use anygamepad::model::raw::{RawLayout, RawSource};
    use anygamepad::model::{OutputSet, Transport};
    use anygamepad::provider::DeviceLocation;

    use super::*;

    fn info(name: &str) -> DeviceInfo {
        let mut identity = DeviceIdentity::new(ProviderKind::Native, Backend::Evdev, Os::Linux);
        identity.vendor_id = Some(0x054c);
        identity.product_id = Some(0x0ce6);
        identity.product_name = Some(name.into());
        identity.transport = Transport::Usb;
        identity.driver = Some("playstation".into());
        DeviceInfo {
            identity,
            location: DeviceLocation::new(Some("/dev/input/event5".into()), None),
            layout: RawLayout {
                buttons: vec![RawSource::Evdev(0x130)],
                ..RawLayout::default()
            },
            outputs: OutputSet::EMPTY,
        }
    }

    fn report_of(info: &DeviceInfo) -> (Report, Vec<Redaction>) {
        let facts = DeviceFacts {
            info,
            mapping: &MappingSource::LinuxSpec,
            capabilities: None,
            descriptor: Some(vec![0x05, 0x01, 0x09, 0x05]),
        };
        let user = UserStatement {
            product: Some("My DualSense".into()),
            ..UserStatement::default()
        };
        let date = Date::from_unix_seconds(1_793_000_000).unwrap();
        build_report(&facts, user, date, Some("6.8.0-45-generic".into())).unwrap()
    }

    #[test]
    fn builds_a_valid_report() {
        let info = info("DualSense Wireless Controller");
        let (report, redactions) = report_of(&info);
        assert!(redactions.is_empty());
        let json = report.to_json().unwrap();
        // No device path leaves the app.
        assert!(!json.contains("/dev/input"), "{json}");
        assert!(json.contains("\"desktop\""), "{json}");
        assert!(json.contains("6.8.0-45-generic"));
        let name = file_name(&report);
        assert!(
            name.starts_with("054c-0ce6-") && name.ends_with(".json"),
            "{name}"
        );
        let url = issue_url(&report);
        assert!(url.contains("template=device-report.yml&title=Report%3A%20DualSense%20Wireless"));
    }

    #[test]
    fn scrubs_identifiers() {
        // A name with what looks like a Bluetooth address.
        let info = info("Pad A4:AE:12:34:56:78");
        let (report, redactions) = report_of(&info);
        assert!(!redactions.is_empty());
        assert!(!report.to_json().unwrap().contains("A4:AE:12"));
    }

    #[test]
    fn store_round_trip() {
        let dir = std::env::temp_dir().join(format!("anygamepad-reports-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = ReportStore::new(dir.clone());
        assert!(store.list().is_empty());
        let (report, _) = report_of(&info("DualSense Wireless Controller"));
        let path = store.save(&report).unwrap();
        // Saving again replaces the device's file.
        store.save(&report).unwrap();
        fs::write(dir.join("notes.json"), "{}").unwrap();
        let saved = store.list();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].path, path);
        assert!(store.delete(Path::new("/etc/passwd")).is_err());
        store.delete(&path).unwrap();
        assert!(store.list().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
