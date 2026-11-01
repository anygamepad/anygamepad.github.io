//! The Test page (H5, ADR 0008): the test mode of the desktop app, with
//! the web app's flow (H4). It opens on the list of controllers (the lab's
//! units and the catalog's models); picking one shows the ways to connect
//! it; a pad connected without picking is detected and shown, to confirm
//! or to change. A session runs a test plan in the device worker
//! ([`crate::test_mode`]); its anonymous report is reviewed and saved to
//! `test-reports/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anygamepad::db::catalog::Model;
use anygamepad::db::detect::ChoiceCheck;
use anygamepad::db::lab::Code;
use anygamepad::db::{Catalog, Inventory, TestPlan};
use anygamepad::model::{Button, Transport};
use anygamepad::provider::DeviceId;
use anygamepad::testing::Prompt;
use anygamepad_report::test_report::{Answer, ReviewItem, SessionStart, StepStatus, TestReport};
use eframe::egui::{self, RichText};

use crate::collection::open_path;
use crate::devices::{Command, DeviceView, Devices, Snapshot};
use crate::test_mode::{
    Choice, ResultLine, SavedTest, TestCommand, TestStore, result_lines, session, status_name,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// The list of controllers.
    List,
    /// The ways to connect the model picked.
    Connect,
    /// Waiting for the pad picked to connect.
    Waiting,
    /// The pad connected does not look like the one picked.
    Check,
    /// A pad connected without picking: what it looks like.
    Detected,
    /// The plans to run.
    Plan,
    /// A session runs.
    Running,
    /// The session ended: its report.
    Summary,
}

/// What detection says a pad can be.
#[derive(Clone, Debug)]
struct Candidate {
    model: &'static Model,
    mode: String,
    /// `None` when the person must say.
    transport: Option<Transport>,
}

/// The configuration a session will test.
#[derive(Clone, Debug)]
struct Planned {
    start: SessionStart,
    confirmed: Choice,
    detected: Option<Choice>,
}

/// The page's state.
pub struct TestPage {
    catalog: &'static Catalog,
    inventory: &'static Inventory,
    store: Option<TestStore>,
    /// Saved test reports, read the first time the page shows.
    saved: Option<Vec<SavedTest>>,
    stage: Stage,
    search: String,
    /// Devices seen while the page showed, and whether each had a button
    /// down at the last frame.
    pressed: BTreeMap<DeviceId, bool>,
    /// The pad the next session runs on.
    pad: Option<DeviceId>,
    /// The model picked or detected.
    model: Option<&'static Model>,
    unit: Option<Code>,
    /// The unit picked last, per model, this run of the app.
    last_units: BTreeMap<String, Code>,
    /// The mode and transport picked from the cards.
    picked: Option<(String, Transport)>,
    candidates: Vec<Candidate>,
    candidate: usize,
    /// In the detected stage: answers from the pad count once it was
    /// released.
    armed: bool,
    planned: Option<Planned>,
    check: Option<String>,
    /// The session's report once it ended, with the person's notes.
    report: Option<TestReport>,
    finished: Option<Arc<TestReport>>,
    review: Vec<ReviewItem>,
    results: Vec<ResultLine>,
    notes: String,
    saved_path: Option<PathBuf>,
    /// The notes of the saved file.
    saved_notes: Option<String>,
    error: Option<String>,
}

/// A transport in words.
fn transport_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Usb => "USB cable",
        Transport::Bluetooth => "Bluetooth",
        Transport::BluetoothLe => "Bluetooth LE",
        Transport::Dongle => "Wireless receiver",
        _ => "Unknown",
    }
}

/// A catalog text without its Markdown code marks.
fn plain(text: &str) -> String {
    text.replace('`', "")
}

/// A model's title: its vendor and name, the vendor once.
fn title(model: &Model) -> String {
    if model.name.starts_with(&model.vendor) {
        model.name.clone()
    } else {
        format!("{} {}", model.vendor, model.name)
    }
}

fn pad_name(device: &DeviceView) -> String {
    device
        .info
        .identity
        .product_name
        .clone()
        .unwrap_or_else(|| "Unnamed controller".into())
}

/// The pads a session can run on: readable, not made by the app.
fn pads(snapshot: &Snapshot) -> impl Iterator<Item = &DeviceView> {
    snapshot
        .devices
        .iter()
        .filter(|device| !device.made_here && device.error.is_none())
}

impl TestPage {
    /// The page, writing to `folder` (none without a home directory).
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self {
            catalog: Catalog::embedded(),
            inventory: Inventory::embedded(),
            store: folder.map(TestStore::new),
            saved: None,
            stage: Stage::List,
            search: String::new(),
            pressed: BTreeMap::new(),
            pad: None,
            model: None,
            unit: None,
            last_units: BTreeMap::new(),
            picked: None,
            candidates: Vec::new(),
            candidate: 0,
            armed: false,
            planned: None,
            check: None,
            report: None,
            finished: None,
            review: Vec::new(),
            results: Vec::new(),
            notes: String::new(),
            saved_path: None,
            saved_notes: None,
            error: None,
        }
    }

    /// Draws the page.
    pub fn show(&mut self, ui: &mut egui::Ui, devices: &Devices) {
        let snapshot = devices.snapshot();
        if self.saved.is_none() {
            self.saved = Some(self.store.as_ref().map(TestStore::list).unwrap_or_default());
        }
        self.observe(&snapshot);
        ui.heading("Test");
        ui.label(
            RichText::new(
                "Test a controller step by step. The report is anonymous and stays on this \
                 computer until you share it.",
            )
            .weak(),
        );
        if snapshot.backend.is_none() {
            ui.add_space(8.0);
            ui.label(
                "This system has no controller backend in anygamepad yet: Linux works today, \
                 Windows (C5) and macOS (C6) come next. ANYGAMEPAD_DEMO=1 runs a session on a \
                 demo pad.",
            );
            return;
        }
        if let Some(error) = &self.error {
            ui.add_space(6.0);
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.stage {
                Stage::List => self.list(ui, &snapshot),
                Stage::Connect => self.connect(ui, &snapshot),
                Stage::Waiting => self.waiting(ui, &snapshot),
                Stage::Check => self.check_view(ui),
                Stage::Detected => self.detected(ui, &snapshot),
                Stage::Plan => self.plans(ui, &snapshot, devices),
                Stage::Running => self.running(ui, &snapshot, devices),
                Stage::Summary => self.summary(ui, devices),
            });
    }

    /// Pads that come, go and press buttons; the session ending.
    fn observe(&mut self, snapshot: &Snapshot) {
        if self.stage == Stage::Running {
            if let Some(report) = snapshot.test.as_ref().and_then(|t| t.finished.clone()) {
                self.finish(report);
            }
            if snapshot.test.is_none() {
                // The worker lost the session (it never does unless asked).
                self.stage = Stage::List;
            }
            return;
        }
        let present: BTreeSet<DeviceId> = pads(snapshot).map(|d| d.id).collect();
        let gone: Vec<DeviceId> = self
            .pressed
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.pressed.remove(&id);
            if self.pad == Some(id) && self.stage != Stage::Summary {
                self.reset();
                self.error = Some("The pad went away.".into());
            }
        }
        for device in pads(snapshot) {
            let down = !device.state.buttons.is_empty();
            let new = !self.pressed.contains_key(&device.id);
            // A pad seen for the first time counts as held, not pressed.
            let was = self.pressed.insert(device.id, down).unwrap_or(true);
            if self.stage == Stage::Detected && Some(device.id) == self.pad {
                if !down {
                    self.armed = true;
                } else if !was && self.armed {
                    if device.state.pressed(Button::South) {
                        self.confirm();
                    } else if device.state.pressed(Button::East) {
                        self.choose_another();
                    }
                }
                continue;
            }
            if new || (down && !was) {
                self.answered_by(device);
            }
        }
    }

    /// A pad connected or pressed while the page waits for one.
    fn answered_by(&mut self, device: &DeviceView) {
        match self.stage {
            Stage::Waiting => self.check_pad(device),
            Stage::List if self.pad.is_none() => self.detect(device),
            _ => {}
        }
    }

    fn detect(&mut self, device: &DeviceView) {
        let identification = self
            .catalog
            .identify(&device.info.identity, Some(self.inventory));
        let candidates: Vec<Candidate> = identification
            .candidates
            .iter()
            .map(|candidate| Candidate {
                model: self.model_of(&candidate.model.id),
                mode: candidate.mode.id.clone(),
                transport: candidate.transport,
            })
            .collect();
        let Some(best) = candidates.first().cloned() else {
            return;
        };
        self.pad = Some(device.id);
        self.candidates = candidates;
        self.candidate = 0;
        self.armed = false;
        self.show_candidate(&best);
        self.error = None;
        self.stage = Stage::Detected;
    }

    fn model_of(&self, id: &str) -> &'static Model {
        self.catalog
            .get(id)
            .expect("detection proposes catalog models")
    }

    fn show_candidate(&mut self, candidate: &Candidate) {
        self.model = Some(candidate.model);
        self.unit = self.units_of(&candidate.model.id).into_iter().next();
    }

    /// The units of a model, the one picked last first.
    fn units_of(&self, model: &str) -> Vec<Code> {
        let last = self.last_units.get(model);
        self.inventory
            .proposed_units(model, last)
            .into_iter()
            .map(|unit| unit.code.clone())
            .collect()
    }

    fn check_pad(&mut self, device: &DeviceView) {
        let (Some(model), Some((mode, transport))) = (self.model, self.picked.clone()) else {
            return;
        };
        let check = self.catalog.check_choice(
            &device.info.identity,
            &model.id,
            &mode,
            transport,
            Some(self.inventory),
        );
        self.pad = Some(device.id);
        let detected = self.best_of(device);
        self.planned = Some(Planned {
            start: SessionStart::List,
            confirmed: Choice {
                model: model.id.clone(),
                mode,
                transport,
                unit: self.unit.clone(),
            },
            detected,
        });
        match check {
            ChoiceCheck::Differs(seen) => {
                self.check = Some(match seen.best() {
                    Some(seen) => format!(
                        "It looks like {} instead.",
                        self.describe(&seen.model.id, &seen.mode.id, seen.transport)
                    ),
                    None => "It does not look like the controller picked.".into(),
                });
                self.stage = Stage::Check;
            }
            ChoiceCheck::NotKnown => {
                self.check = Some(
                    "The catalog does not know how this connection looks yet: this report will \
                     show it."
                        .into(),
                );
                self.stage = Stage::Plan;
            }
            _ => {
                self.check = None;
                self.stage = Stage::Plan;
            }
        }
    }

    /// Detection's best candidate for `device`, when it tells a transport.
    fn best_of(&self, device: &DeviceView) -> Option<Choice> {
        let identification = self
            .catalog
            .identify(&device.info.identity, Some(self.inventory));
        let best = identification.best()?;
        Some(Choice {
            model: best.model.id.clone(),
            mode: best.mode.id.clone(),
            transport: best.transport?,
            unit: self
                .inventory
                .proposed_units(&best.model.id, None)
                .first()
                .map(|unit| unit.code.clone()),
        })
    }

    fn describe(&self, model: &str, mode: &str, transport: Option<Transport>) -> String {
        let model = self.catalog.get(model);
        let mode_name = model
            .and_then(|m| m.mode(mode))
            .map_or(mode.to_owned(), |m| m.name.clone());
        let transport = transport.map_or("connection to confirm", transport_name);
        let model = model.map_or("Unknown model".into(), title);
        format!("{model} · {mode_name} · {transport}")
    }

    fn pick_model(&mut self, model: &'static Model, unit: Option<Code>) {
        self.model = Some(model);
        self.unit = unit.or_else(|| self.units_of(&model.id).into_iter().next());
        self.picked = None;
        self.error = None;
        self.stage = Stage::Connect;
    }

    fn pick_card(&mut self, mode: String, transport: Transport, snapshot: &Snapshot) {
        let Some(model) = self.model else {
            return;
        };
        self.picked = Some((mode.clone(), transport));
        // Picked after "Choose another": the pad is already connected.
        if let Some(device) = self.pad.and_then(|id| pads(snapshot).find(|d| d.id == id)) {
            self.planned = Some(Planned {
                start: SessionStart::Connecting,
                confirmed: Choice {
                    model: model.id.clone(),
                    mode,
                    transport,
                    unit: self.unit.clone(),
                },
                detected: self.best_of(device),
            });
            self.check = None;
            self.stage = Stage::Plan;
            return;
        }
        self.stage = Stage::Waiting;
    }

    fn confirm(&mut self) {
        let Some(candidate) = self.candidates.get(self.candidate).cloned() else {
            return;
        };
        let Some(transport) = candidate.transport else {
            return;
        };
        let detected = self.candidates.first().and_then(|best| {
            Some(Choice {
                model: best.model.id.clone(),
                mode: best.mode.clone(),
                transport: best.transport?,
                unit: None,
            })
        });
        self.planned = Some(Planned {
            start: SessionStart::Connecting,
            confirmed: Choice {
                model: candidate.model.id.clone(),
                mode: candidate.mode,
                transport,
                unit: self.unit.clone(),
            },
            detected,
        });
        self.check = None;
        self.stage = Stage::Plan;
    }

    fn choose_another(&mut self) {
        if self.stage == Stage::Detected {
            self.stage = Stage::List;
        }
    }

    fn start(&mut self, plan: &TestPlan, snapshot: &Snapshot, devices: &Devices) {
        let device = self.pad.and_then(|id| pads(snapshot).find(|d| d.id == id));
        let (Some(device), Some(planned)) = (device, &self.planned) else {
            self.error = Some("The pad is not connected any more.".into());
            return;
        };
        let session = match session(
            device,
            plan,
            planned.start,
            &planned.confirmed,
            planned.detected.as_ref(),
        ) {
            Ok(session) => session,
            Err(e) => {
                self.error = Some(format!("Cannot test this pad: {e}."));
                return;
            }
        };
        if let Some(unit) = &planned.confirmed.unit {
            self.last_units
                .insert(planned.confirmed.model.clone(), unit.clone());
        }
        devices.send(Command::Test(TestCommand::Start(
            device.id,
            Box::new(session),
        )));
        self.report = None;
        self.finished = None;
        self.results.clear();
        self.notes.clear();
        self.saved_path = None;
        self.saved_notes = None;
        self.error = None;
        self.stage = Stage::Running;
    }

    fn finish(&mut self, report: Arc<TestReport>) {
        self.results = result_lines(&report.results);
        self.review = report.review(self.catalog);
        self.report = Some((*report).clone());
        self.finished = Some(report);
        self.stage = Stage::Summary;
    }

    /// The report with the person's notes, scrubbed and checked.
    fn final_report(&self) -> Option<Result<TestReport, String>> {
        let mut report = self.finished.as_deref()?.clone();
        let notes = self.notes.trim();
        report.notes = (!notes.is_empty()).then(|| notes.to_owned());
        report.scrub();
        Some(
            report
                .validate()
                .map(|()| report)
                .map_err(|e| e.to_string()),
        )
    }

    fn save(&mut self) {
        let Some(store) = &self.store else {
            self.error = Some("No folder to save to: this system has no home folder.".into());
            return;
        };
        let report = match self.final_report() {
            Some(Ok(report)) => report,
            Some(Err(e)) => {
                self.error = Some(format!("The report is not valid: {e}."));
                return;
            }
            None => return,
        };
        match store.save(&report, self.saved_path.as_deref()) {
            Ok(path) => {
                log::info!("test report saved");
                self.saved_path = Some(path);
                self.saved_notes = Some(self.notes.trim().to_owned());
                self.saved = Some(store.list());
                self.report = Some(report);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("The report was not saved: {e}.")),
        }
    }

    /// Leaves the summary: forgets the session.
    fn close(&mut self, devices: &Devices) {
        devices.send(Command::Test(TestCommand::Close));
        self.finished = None;
        self.report = None;
    }

    fn reset(&mut self) {
        self.stage = Stage::List;
        self.pad = None;
        self.model = None;
        self.unit = None;
        self.picked = None;
        self.candidates.clear();
        self.planned = None;
        self.check = None;
        self.error = None;
    }

    // Stages.

    fn list(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let connected: Vec<&DeviceView> = pads(snapshot).collect();
        if connected.is_empty() {
            ui.label("Connect a controller to detect it, or pick it from the list.");
        } else {
            ui.label(RichText::new("Connected").strong());
            for device in connected {
                ui.horizontal(|ui| {
                    ui.label(pad_name(device));
                    if ui.button("Detect").clicked() {
                        self.detect(device);
                    }
                });
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("DualSense, 8BitDo, xbox...")
                    .desired_width(260.0),
            );
        });
        let text = self.search.trim().to_lowercase();
        let units: Vec<_> = self
            .inventory
            .units
            .iter()
            .filter(|unit| {
                text.is_empty()
                    || format!(
                        "{} {} {}",
                        unit.code.as_str(),
                        unit.model,
                        unit.notes.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&text)
            })
            .collect();
        if !units.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Units of the lab").strong());
            for unit in units {
                let Some(model) = self.catalog.get(&unit.model) else {
                    continue;
                };
                let label = format!("{} · {}", unit.code.as_str(), title(model));
                if ui.selectable_label(false, label).clicked() {
                    self.pick_model(model, Some(unit.code.clone()));
                }
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Models").strong());
        let candidates: BTreeSet<&str> = self
            .candidates
            .iter()
            .map(|c| c.model.id.as_str())
            .collect();
        let mut models: Vec<&'static Model> = self
            .catalog
            .models()
            .iter()
            .filter(|model| {
                text.is_empty()
                    || format!("{} {} {}", model.vendor, model.name, model.id)
                        .to_lowercase()
                        .contains(&text)
            })
            .collect();
        // Detection's candidates first.
        models.sort_by_key(|model| !candidates.contains(model.id.as_str()));
        for model in models {
            let units: Vec<&str> = self
                .inventory
                .units_of(&model.id)
                .map(|unit| unit.code.as_str())
                .collect();
            let mut label = title(model);
            if model.generic {
                label.push_str(" (generic)");
            }
            if !units.is_empty() {
                label.push_str(&format!(" · {}", units.join(", ")));
            }
            let text = if candidates.contains(model.id.as_str()) {
                RichText::new(label).strong()
            } else {
                RichText::new(label)
            };
            if ui.selectable_label(false, text).clicked() {
                self.pick_model(model, None);
            }
        }
    }

    fn unit_picker(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.model else {
            return;
        };
        let units = self.units_of(&model.id);
        if units.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Unit");
            let selected = self
                .unit
                .as_ref()
                .map_or("None".to_owned(), |u| u.as_str().to_owned());
            egui::ComboBox::from_id_salt("test-unit")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.unit, None, "None");
                    for unit in units {
                        let label = unit.as_str().to_owned();
                        ui.selectable_value(&mut self.unit, Some(unit), label);
                    }
                });
        });
    }

    fn connect(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let Some(model) = self.model else {
            self.reset();
            return;
        };
        if ui.button("← All controllers").clicked() {
            self.reset();
            return;
        }
        ui.add_space(6.0);
        ui.heading(title(model));
        if let Some(notes) = &model.notes {
            ui.label(RichText::new(plain(notes)).weak());
        }
        self.unit_picker(ui);
        ui.add_space(8.0);
        ui.label("How will you connect it?");
        let saved = self.saved.clone().unwrap_or_default();
        let mut picked = None;
        for mode in &model.modes {
            for connection in &mode.connections {
                let tested = saved.iter().any(|test| {
                    test.confirmed.model == model.id
                        && test.confirmed.mode == mode.id
                        && test.confirmed.transport == connection.transport
                });
                let response = crate::style::card(ui, false)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    mode.name,
                                    transport_name(connection.transport)
                                ))
                                .strong(),
                            );
                            if tested {
                                ui.label(RichText::new("Tested here").small().weak());
                            }
                            if connection.identities.is_empty() {
                                ui.label(RichText::new("New to the catalog").small().weak());
                            }
                        });
                        if let Some(how_to) = &mode.how_to {
                            ui.label(plain(how_to));
                        }
                        if let Some(how_to) = &connection.how_to {
                            ui.label(plain(how_to));
                        }
                    })
                    .response
                    .interact(egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    picked = Some((mode.id.clone(), connection.transport));
                }
                ui.add_space(6.0);
            }
        }
        if let Some((mode, transport)) = picked {
            self.pick_card(mode, transport, snapshot);
        }
    }

    fn waiting(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let (Some(model), Some((mode, transport))) = (self.model, self.picked.clone()) else {
            self.reset();
            return;
        };
        if ui.button("← Ways to connect it").clicked() {
            self.pick_model(model, self.unit.clone());
            return;
        }
        ui.add_space(6.0);
        ui.heading(self.describe(&model.id, &mode, Some(transport)));
        ui.label(
            "Connect it now, or press a button on it if it is connected already. Or pick it here:",
        );
        let connected: Vec<&DeviceView> = pads(snapshot).collect();
        for device in connected {
            ui.horizontal(|ui| {
                ui.label(pad_name(device));
                if ui.button("This one").clicked() {
                    self.check_pad(device);
                }
            });
        }
    }

    fn check_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("It does not look like the controller picked");
        if let Some(check) = &self.check {
            ui.label(check);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Go on anyway").clicked() {
                self.stage = Stage::Plan;
            }
            if ui.button("Back to the ways to connect it").clicked() {
                self.pad = None;
                if let Some(model) = self.model {
                    self.pick_model(model, self.unit.clone());
                }
            }
        });
    }

    fn detected(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let name = self
            .pad
            .and_then(|id| snapshot.devices.iter().find(|d| d.id == id))
            .map(pad_name)
            .unwrap_or_default();
        ui.label(RichText::new(format!("Detected: {name}")).weak());
        let Some(candidate) = self.candidates.get(self.candidate).cloned() else {
            self.reset();
            return;
        };
        ui.heading(self.describe(&candidate.model.id, &candidate.mode, candidate.transport));
        if self.candidates.len() > 1 {
            ui.add_space(6.0);
            ui.label("It can also be:");
            let mut pick = None;
            for (index, other) in self.candidates.iter().enumerate() {
                let text = self.describe(&other.model.id, &other.mode, other.transport);
                if ui.radio(index == self.candidate, text).clicked() {
                    pick = Some(index);
                }
            }
            if let Some(index) = pick {
                self.candidate = index;
                let candidate = self.candidates[index].clone();
                self.show_candidate(&candidate);
            }
        }
        if candidate.transport.is_none() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Connected by");
                let transports: Vec<Transport> = candidate
                    .model
                    .mode(&candidate.mode)
                    .map(|mode| mode.connections.iter().map(|c| c.transport).collect())
                    .unwrap_or_default();
                for transport in transports {
                    if ui.button(transport_name(transport)).clicked() {
                        self.candidates[self.candidate].transport = Some(transport);
                    }
                }
            });
        }
        self.unit_picker(ui);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ready = self.candidates[self.candidate].transport.is_some();
            if ui
                .add_enabled(ready, egui::Button::new("Confirm"))
                .clicked()
            {
                self.confirm();
            }
            if ui.button("Choose another").clicked() {
                self.choose_another();
            }
        });
        ui.label(
            RichText::new("On the pad: south confirms, east chooses another.")
                .small()
                .weak(),
        );
    }

    fn plans(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, devices: &Devices) {
        let Some(planned) = self.planned.clone() else {
            self.reset();
            return;
        };
        if ui.button("← All controllers").clicked() {
            self.reset();
            return;
        }
        ui.add_space(6.0);
        let confirmed = &planned.confirmed;
        ui.heading(self.describe(&confirmed.model, &confirmed.mode, Some(confirmed.transport)));
        if let Some(unit) = &confirmed.unit {
            ui.label(format!("Unit {}", unit.as_str()));
        }
        if let Some(check) = &self.check {
            ui.label(RichText::new(check).weak());
        }
        ui.add_space(8.0);
        ui.label("Which test?");
        let mut plans: Vec<&TestPlan> = TestPlan::embedded().iter().collect();
        // The quick one first, the desktop's own last.
        plans.sort_by_key(|plan| match plan.id.as_str() {
            "quick" => 0,
            "full" => 1,
            _ if plan.needs_virtual_pads() => 3,
            _ => 2,
        });
        for plan in plans {
            let response = crate::style::card(ui, false)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&plan.name).strong());
                    ui.label(&plan.description);
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                self.start(plan, snapshot, devices);
            }
            ui.add_space(6.0);
        }
    }

    fn running(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, devices: &Devices) {
        let Some(test) = &snapshot.test else {
            ui.label("Starting…");
            return;
        };
        let status = &test.status;
        let progress = if status.total == 0 {
            1.0
        } else {
            status.step as f32 / status.total as f32
        };
        ui.add(
            egui::ProgressBar::new(progress)
                .text(format!("Step {} of {}", status.step + 1, status.total))
                .desired_width(ui.available_width().min(480.0)),
        );
        if let Some(step) = &status.step_id {
            ui.label(RichText::new(step).small().weak());
        }
        ui.add_space(12.0);
        let prompt = status.prompt;
        ui.label(RichText::new(prompt.text(test.face)).size(22.0).strong());
        if test.device.is_none() && !matches!(prompt, Prompt::Unplug | Prompt::Reconnect) {
            ui.label(RichText::new("The pad is not connected.").weak());
        }
        ui.add_space(12.0);
        let send = |command| devices.send(Command::Test(command));
        ui.horizontal_wrapped(|ui| {
            match prompt {
                Prompt::Ask(_) => {
                    if ui.button("Yes").clicked() {
                        send(TestCommand::Answer(Answer::Yes));
                    }
                    if ui.button("No").clicked() {
                        send(TestCommand::Answer(Answer::No));
                    }
                    if ui.button("Not sure").clicked() {
                        send(TestCommand::Answer(Answer::NotSure));
                    }
                    if ui.button("Again").clicked() {
                        send(TestCommand::Repeat);
                    }
                }
                Prompt::Press(_) => {
                    if ui.button("Not on this pad").clicked() {
                        send(TestCommand::NotOnThisPad);
                    }
                    if ui.button("Nothing happens").clicked() {
                        send(TestCommand::NothingHappens);
                    }
                }
                Prompt::Circle(_) | Prompt::Pull(_) | Prompt::Touch { .. }
                    if ui.button("Not on this pad").clicked() =>
                {
                    send(TestCommand::NotOnThisPad);
                }
                _ => {}
            }
            if !matches!(prompt, Prompt::Wait | Prompt::Done) && ui.button("Skip").clicked() {
                send(TestCommand::Skip);
            }
            if ui.button("Stop").clicked() {
                send(TestCommand::Stop);
            }
        });
        if !test.results.is_empty() {
            ui.add_space(12.0);
            egui::CollapsingHeader::new(format!("Results so far ({})", test.results.len()))
                .show(ui, |ui| results_grid(ui, &test.results));
        }
    }

    fn summary(&mut self, ui: &mut egui::Ui, devices: &Devices) {
        let Some(report) = self.report.clone() else {
            self.reset();
            return;
        };
        let summary = report.summary();
        ui.heading("Done");
        let mut text = format!(
            "{} passed, {} failed, {} skipped, {} not applicable",
            summary.pass, summary.fail, summary.skipped, summary.not_applicable
        );
        if summary.error > 0 {
            text.push_str(&format!(", {} errors", summary.error));
        }
        ui.label(text + ".");
        ui.add_space(8.0);
        ui.label(RichText::new("Notes").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.notes)
                .hint_text("Anything worth knowing: firmware, a sticky button... Never about you.")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);
        ui.label(RichText::new("Texts in the report").strong());
        ui.label(
            RichText::new(
                "The report carries no serial numbers, addresses or paths. Check these texts \
                 before you share it:",
            )
            .weak(),
        );
        for item in &self.review {
            let line = format!("{}: {}", item.field, item.text);
            if item.concern.is_some() {
                ui.label(RichText::new(format!(
                    "{line} (not a name the catalog knows for this model: keep it if the pad \
                     calls itself so, or edit the file)"
                )))
                .highlight();
            } else {
                ui.label(RichText::new(line).weak());
            }
        }
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            // Saved already: saving again (new notes) replaces the file.
            let changed = self.saved_notes.as_deref() != Some(self.notes.trim());
            let label = if self.saved_path.is_some() {
                "Save again"
            } else {
                "Save"
            };
            if ui
                .add_enabled(changed && self.store.is_some(), egui::Button::new(label))
                .clicked()
            {
                self.save();
            }
            if let Some(store) = &self.store {
                if ui.button("Open the folder").clicked() {
                    open_path(store.dir());
                }
            }
            if ui.button("Test another way to connect it").clicked() {
                let model = self.model;
                let unit = self.unit.clone();
                self.close(devices);
                self.reset();
                if let Some(model) = model {
                    self.pick_model(model, unit);
                }
            }
            if ui.button("Back to the list").clicked() {
                self.close(devices);
                self.reset();
            }
        });
        if let Some(path) = &self.saved_path {
            ui.label(format!("Saved as {}.", path.display()));
        } else {
            ui.label(
                RichText::new("Not saved yet: the report is lost when you leave this page.")
                    .small()
                    .weak(),
            );
        }
        ui.add_space(10.0);
        egui::CollapsingHeader::new(format!("Results ({})", self.results.len()))
            .default_open(true)
            .show(ui, |ui| results_grid(ui, &self.results));
    }
}

fn results_grid(ui: &mut egui::Ui, results: &[ResultLine]) {
    egui::Grid::new(ui.next_auto_id())
        .num_columns(3)
        .striped(true)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for line in results {
                ui.label(&line.step);
                let status = status_name(line.status);
                let text = match line.status {
                    StepStatus::Pass => RichText::new(status),
                    StepStatus::Fail | StepStatus::Error => {
                        RichText::new(status).color(ui.visuals().error_fg_color)
                    }
                    _ => RichText::new(status).weak(),
                };
                ui.label(text);
                ui.label(RichText::new(&line.detail).weak());
                ui.end_row();
            }
        });
}
