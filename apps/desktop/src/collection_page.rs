//! The Collection page (A3): starting collection mode, what each connected
//! controller is (in the person's words), the saved report files, and the
//! share view. Texts follow `docs/privacy/README.md` ("Consent texts").

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anygamepad::provider::DeviceId;
use anygamepad_report::privacy::Redaction;
use anygamepad_report::{ControllerMode, UserStatement};
use eframe::egui;

use crate::collection::{
    DeviceFacts, ReportStore, SavedReport, build_report, issue_url, open_path, os_version, today,
};
use crate::devices::{DeviceView, Snapshot};
use crate::settings::Settings;

/// Modes the person can pick, with their labels.
const MODES: [(ControllerMode, &str); 11] = [
    (ControllerMode::Xinput, "XInput (X)"),
    (ControllerMode::Dinput, "DirectInput (D)"),
    (ControllerMode::Switch, "Switch (S)"),
    (ControllerMode::Ps3, "PlayStation 3"),
    (ControllerMode::Ps4, "PlayStation 4"),
    (ControllerMode::Ps5, "PlayStation 5"),
    (ControllerMode::Xbox, "Xbox One / Series"),
    (ControllerMode::Android, "Android (A)"),
    (ControllerMode::Apple, "Apple (MFi)"),
    (ControllerMode::KeyboardMouse, "Keyboard and mouse"),
    (ControllerMode::Other, "Other"),
];

/// What the person says about a device, as typed.
#[derive(Clone, Default)]
struct Statement {
    product: String,
    mode: Option<ControllerMode>,
}

/// The page's state.
pub struct CollectionPage {
    store: Option<ReportStore>,
    saved: Vec<SavedReport>,
    /// Devices whose report was written this session.
    recorded: BTreeSet<DeviceId>,
    redactions: BTreeMap<PathBuf, Vec<Redaction>>,
    statements: BTreeMap<DeviceId, Statement>,
    sharing: Option<PathBuf>,
    copied: bool,
    deleting: Option<PathBuf>,
    error: Option<String>,
}

impl CollectionPage {
    /// The page, writing to `folder` (none without a home directory).
    pub fn new(folder: Option<PathBuf>) -> Self {
        let store = folder.map(ReportStore::new);
        let saved = store.as_ref().map(ReportStore::list).unwrap_or_default();
        Self {
            store,
            saved,
            recorded: BTreeSet::new(),
            redactions: BTreeMap::new(),
            statements: BTreeMap::new(),
            sharing: None,
            copied: false,
            deleting: None,
            error: None,
        }
    }

    /// Writes the report of every device not recorded yet, while
    /// collection mode is on. Called every frame, whatever page shows.
    pub fn collect(&mut self, settings: &Settings, snapshot: &Snapshot) {
        if !settings.collecting || self.store.is_none() {
            return;
        }
        let new: Vec<&DeviceView> = snapshot
            .devices
            .iter()
            .filter(|device| !self.recorded.contains(&device.id))
            .collect();
        for device in new {
            self.recorded.insert(device.id);
            self.write(device);
        }
    }

    /// Builds and saves a device's report with what the person said.
    fn write(&mut self, device: &DeviceView) {
        let Some(store) = &self.store else {
            return;
        };
        let statement = self.statements.get(&device.id).cloned().unwrap_or_default();
        let user = UserStatement {
            product: Some(statement.product.trim().to_owned()).filter(|p| !p.is_empty()),
            mode: statement.mode,
            ..UserStatement::default()
        };
        let facts = DeviceFacts {
            info: &device.info,
            mapping: &device.mapping,
            capabilities: Some(device.capabilities.clone()),
            descriptor: descriptor(device),
        };
        let result =
            build_report(&facts, user, today(), os_version()).and_then(|(report, redactions)| {
                let path = store.save(&report).map_err(|e| e.to_string())?;
                Ok((path, redactions))
            });
        match result {
            Ok((path, redactions)) => {
                log::info!("report saved for device {}", device.id.0);
                self.redactions.insert(path, redactions);
                self.saved = store.list();
                self.error = None;
            }
            Err(e) => {
                log::warn!("report not saved: {e}");
                self.error = Some(format!("The report could not be saved: {e}"));
            }
        }
    }

    /// Draws the page; returns whether `settings` changed.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        snapshot: &Snapshot,
    ) -> bool {
        let mut changed = false;
        ui.heading("Collection");
        let Some(folder) = self.store.as_ref().map(|s| s.dir().to_path_buf()) else {
            ui.label("This system has no home folder to keep reports in.");
            return false;
        };
        if !settings.collection_started {
            ui.label(format!(
                "Collection mode records how each input provider sees your controllers and saves \
                 the reports as files in {}. Nothing is sent: share a report yourself, from its \
                 file, when you want to.",
                folder.display()
            ));
            ui.horizontal(|ui| {
                if ui.button("Start collecting").clicked() {
                    settings.collection_started = true;
                    settings.collecting = true;
                    changed = true;
                }
                if ui.button("Open the folder").clicked() {
                    let _ = std::fs::create_dir_all(&folder);
                    open_path(&folder);
                }
            });
            return changed;
        }
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut settings.collecting, "Collect reports")
                .on_hover_text("Saves a report file for every controller that connects.")
                .changed()
            {
                changed = true;
            }
            if ui.button("Open the folder").clicked() {
                let _ = std::fs::create_dir_all(&folder);
                open_path(&folder);
            }
        });
        ui.label(
            egui::RichText::new(folder.display().to_string())
                .small()
                .weak(),
        );
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        self.connected(ui, settings, snapshot);
        ui.separator();
        self.saved_list(ui);
        self.share_window(ui.ctx());
        changed
    }

    /// The connected controllers and what the person says about them.
    fn connected(&mut self, ui: &mut egui::Ui, settings: &Settings, snapshot: &Snapshot) {
        ui.strong("Connected controllers");
        if snapshot.devices.is_empty() {
            ui.label("None.");
        }
        for device in &snapshot.devices {
            let name = device
                .info
                .identity
                .product_name
                .clone()
                .unwrap_or_else(|| "Unnamed controller".into());
            let mut write = false;
            ui.push_id(device.id.0, |ui| {
                egui::CollapsingHeader::new(&name)
                    .default_open(snapshot.devices.len() == 1)
                    .show(ui, |ui| {
                        let statement = self.statements.entry(device.id).or_default();
                        ui.horizontal(|ui| {
                            ui.label("What is it?");
                            ui.add(
                                egui::TextEdit::singleline(&mut statement.product)
                                    .hint_text("8BitDo Ultimate 2C")
                                    .char_limit(anygamepad_report::MAX_USER_TEXT_CHARS),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Mode");
                            let selected = MODES
                                .iter()
                                .find(|(mode, _)| Some(*mode) == statement.mode)
                                .map_or("Not said", |(_, label)| label);
                            egui::ComboBox::from_id_salt("mode")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut statement.mode, None, "Not said");
                                    for (mode, label) in MODES {
                                        ui.selectable_value(&mut statement.mode, Some(mode), label);
                                    }
                                });
                        });
                        let label = if self.recorded.contains(&device.id) {
                            "Update its report"
                        } else {
                            "Save its report"
                        };
                        write = ui
                            .add_enabled(settings.collecting, egui::Button::new(label))
                            .clicked();
                    });
            });
            if write {
                self.recorded.insert(device.id);
                self.write(device);
            }
        }
    }

    /// The report files.
    fn saved_list(&mut self, ui: &mut egui::Ui) {
        ui.strong("Saved reports");
        if self.saved.is_empty() {
            ui.label("None yet.");
        }
        let mut delete = None;
        egui::Grid::new("saved")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for saved in &self.saved {
                    let view = saved.report.views.first();
                    let name = view
                        .and_then(|v| v.identity.product_name.clone())
                        .unwrap_or_else(|| "Unnamed controller".into());
                    ui.label(name);
                    ui.label(saved.report.created.to_string());
                    ui.horizontal(|ui| {
                        if ui.button("Share").clicked() {
                            self.sharing = Some(saved.path.clone());
                            self.copied = false;
                        }
                        if ui.button("Open the file").clicked() {
                            open_path(&saved.path);
                        }
                        if self.deleting.as_ref() == Some(&saved.path) {
                            ui.label("Delete this report file?");
                            if ui.button("Delete").clicked() {
                                delete = Some(saved.path.clone());
                            }
                            if ui.button("Keep").clicked() {
                                self.deleting = None;
                            }
                        } else if ui.button("Delete").clicked() {
                            self.deleting = Some(saved.path.clone());
                        }
                    });
                    ui.end_row();
                }
            });
        if let (Some(path), Some(store)) = (delete, &self.store) {
            if let Err(e) = store.delete(&path) {
                self.error = Some(format!("The report could not be deleted: {e}"));
            }
            self.deleting = None;
            self.saved = store.list();
        }
    }

    /// The share view (`docs/privacy/README.md`, "Sharing a report").
    fn share_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.sharing.clone() else {
            return;
        };
        let Some(saved) = self.saved.iter().find(|s| s.path == path) else {
            self.sharing = None;
            return;
        };
        let json = saved.report.to_json().unwrap_or_default();
        let url = issue_url(&saved.report);
        let redactions = self.redactions.get(&path).cloned().unwrap_or_default();
        let mut open = true;
        let mut close = false;
        egui::Window::new("Share this report")
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(
                    "Sharing posts this report as an issue on GitHub, from your GitHub account. \
                     Issues are public: anyone can read the report and see who posted it, and it \
                     stays public until you ask for its deletion.",
                );
                ui.label(
                    "The report holds no serial number, Bluetooth address or other personal data. \
                     Below is exactly what you will share: read it before you post.",
                );
                ui.label(
                    "By posting it, you dedicate the report to the public domain under CC0 1.0, \
                     so that anyone can use it, the anygamepad library included.",
                );
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut json.as_str())
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        );
                    });
                if !redactions.is_empty() {
                    let fields: Vec<&str> = redactions.iter().map(|r| r.field.as_str()).collect();
                    ui.label(format!(
                        "Replaced with [redacted] before sharing: {} texts that looked like \
                         identifiers ({}).",
                        redactions.len(),
                        fields.join(", ")
                    ));
                }
                ui.hyperlink_to("Privacy policy", "https://anygamepad.github.io/privacy/");
                ui.horizontal(|ui| {
                    if ui.button("Copy and open the issue form").clicked() {
                        ui.ctx().copy_text(json.clone());
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                        self.copied = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    // In place of the web's copy fallback.
                    if ui.button("Open the file").clicked() {
                        open_path(&path);
                    }
                });
                if self.copied {
                    ui.label(
                        "The report is copied. Paste it into the Report field of the form, check \
                         it and submit the issue: nothing is shared until you submit it.",
                    );
                }
                ui.label("No GitHub account? Keep the report file instead: it stays yours.");
            });
        if !open || close {
            self.sharing = None;
        }
    }
}

/// A device's HID report descriptor, which Linux shows in sysfs next to
/// the device's nodes.
fn descriptor(device: &DeviceView) -> Option<Vec<u8>> {
    let container = device.info.location.container()?;
    std::fs::read(Path::new(container).join("report_descriptor"))
        .ok()
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= anygamepad_report::MAX_DESCRIPTOR_BYTES)
}
