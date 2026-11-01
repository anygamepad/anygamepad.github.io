//! The desktop app's test mode (H5, ADR 0008): a session of the library's
//! test runner on a connected pad, and its anonymous test report, written
//! to `test-reports/` in the app's data folder.
//!
//! The device worker runs the session ([`TestRun`]): it reads the pad at
//! its own pace, so the session's timing does not depend on the window
//! repainting. The Test page builds the session ([`session`]), sends what
//! the person says ([`TestCommand`]) and shows the worker's
//! [`TestState`]. Virtual pad steps run in the worker too
//! ([`crate::virtual_pads`]).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anygamepad::db::lab::Code;
use anygamepad::db::{Catalog, TestPlan};
use anygamepad::model::{FaceLayout, Transport};
use anygamepad::provider::{DeviceId, DeviceInfo, Provider};
use anygamepad::testing::{Prompt, Session, SessionSetup, Status};
use anygamepad_report::UserStatement;
use anygamepad_report::test_report::{
    Answer, Configuration, Selection, SessionStart, StepResult, StepStatus, TestReport,
};

use crate::collection::{DeviceFacts, build_report, os_version, today};
use crate::demo::Autopilot;
use crate::devices::{DeviceView, Open};

/// Without new input, the session still gets an update this often (in
/// milliseconds), so its timers run while the pad rests.
const IDLE_UPDATE_MS: f64 = 20.0;

/// One step's result, for the page.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultLine {
    /// The step's ID (`controls.south`).
    pub step: String,
    /// Its outcome.
    pub status: StepStatus,
    /// Expected, observed and measurements, in words.
    pub detail: String,
}

/// The results of a report, for the page.
pub fn result_lines(results: &[StepResult]) -> Vec<ResultLine> {
    results
        .iter()
        .map(|result| {
            let mut parts = Vec::new();
            if let Some(expected) = &result.expected {
                parts.push(format!("expected {expected}"));
            }
            if let Some(observed) = &result.observed {
                parts.push(format!("observed {observed}"));
            }
            if let Some(answer) = result.answer {
                parts.push(format!(
                    "answer {}",
                    match answer {
                        Answer::Yes => "yes",
                        Answer::No => "no",
                        Answer::NotSure => "not sure",
                    }
                ));
            }
            for (name, value) in &result.measurements {
                parts.push(format!("{name} {}", (value * 100.0).round() / 100.0));
            }
            ResultLine {
                step: result.step.clone(),
                status: result.status,
                detail: parts.join(" · "),
            }
        })
        .collect()
}

/// A step status in words.
pub fn status_name(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pass => "Pass",
        StepStatus::Fail => "Fail",
        StepStatus::Skipped => "Skipped",
        StepStatus::NotApplicable => "Not applicable",
        StepStatus::Error => "Error",
    }
}

/// The test session, as the worker publishes it.
#[derive(Clone, Debug)]
pub struct TestState {
    /// The pad under test; `None` while it is away.
    pub device: Option<DeviceId>,
    /// Where the session is.
    pub status: Status,
    /// What is printed on the face buttons, for the prompts.
    pub face: FaceLayout,
    /// The steps that ended.
    pub results: Arc<Vec<ResultLine>>,
    /// The anonymous report, once the session ended.
    pub finished: Option<Arc<TestReport>>,
}

/// What the Test page asks of the worker's session.
#[derive(Debug)]
pub enum TestCommand {
    /// Runs this session on this device, replacing any other.
    Start(DeviceId, Box<Session>),
    /// The person's answer to the current question.
    Answer(Answer),
    /// Skips the current step.
    Skip,
    /// The pad does not have what the step asks for.
    NotOnThisPad,
    /// The control asked for gets no response.
    NothingHappens,
    /// Runs the current output test again.
    Repeat,
    /// Ends the session now: the report holds the steps that ended.
    Stop,
    /// Forgets the session (ending it first).
    Close,
}

/// What the person confirmed or detection proposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The catalog model ID.
    pub model: String,
    /// The model's mode ID.
    pub mode: String,
    /// The transport.
    pub transport: Transport,
    /// The lab unit, if the person named one.
    pub unit: Option<Code>,
}

impl Choice {
    fn selection(&self) -> Result<Selection, String> {
        let model = Catalog::embedded()
            .get(&self.model)
            .ok_or_else(|| format!("no model `{}`", self.model))?;
        let mode = model
            .mode(&self.mode)
            .ok_or_else(|| format!("`{}` has no mode `{}`", self.model, self.mode))?;
        Ok(Selection {
            unit: self.unit.clone(),
            model: self.model.clone(),
            mode: self.mode.clone(),
            kind: mode.kind,
            transport: self.transport,
        })
    }
}

/// A session of `plan` on `device`, as the person confirmed it.
pub fn session(
    device: &DeviceView,
    plan: &TestPlan,
    start: SessionStart,
    confirmed: &Choice,
    detected: Option<&Choice>,
) -> Result<Session, String> {
    let facts = DeviceFacts {
        info: &device.info,
        mapping: &device.mapping,
        capabilities: Some(device.capabilities.clone()),
        descriptor: None,
    };
    let (report, _) = build_report(&facts, UserStatement::default(), today(), os_version())?;
    let model = Catalog::embedded()
        .get(&confirmed.model)
        .ok_or_else(|| format!("no model `{}`", confirmed.model))?
        .clone();
    let configuration = Configuration {
        start,
        confirmed: confirmed.selection()?,
        detected: detected.map(Choice::selection).transpose()?,
    };
    // Only plans with virtual pad steps look at /dev/uinput and /dev/uhid.
    let virtual_targets = if plan.needs_virtual_pads() {
        crate::virtual_pads::targets()
    } else {
        Vec::new()
    };
    Session::new(SessionSetup {
        plan: plan.clone(),
        model,
        configuration,
        device: report,
        outputs: device.info.outputs,
        hid: false,
        unreadable: Default::default(),
        virtual_targets,
    })
    .map_err(|e| e.to_string())
}

/// A session in the device worker.
pub struct TestRun {
    /// `None` once it ended.
    session: Option<Session>,
    device: Option<DeviceId>,
    /// The raw state fed last, and when.
    fed: Option<(anygamepad::model::raw::RawState, f64)>,
    state: TestState,
    autopilot: Option<Autopilot>,
    #[cfg(target_os = "linux")]
    virtual_run: Option<crate::virtual_pads::VirtualRun>,
}

impl TestRun {
    /// Runs `session` on `device`; the demo pad follows it with
    /// `autopilot`.
    pub fn new(device: DeviceId, session: Session, autopilot: Option<Autopilot>) -> Self {
        log::info!("test session started on device {}", device.0);
        let state = TestState {
            device: Some(device),
            status: session.status(),
            face: session.face_layout(),
            results: Arc::default(),
            finished: None,
        };
        Self {
            session: Some(session),
            device: Some(device),
            fed: None,
            state,
            autopilot,
            #[cfg(target_os = "linux")]
            virtual_run: None,
        }
    }

    /// What the page shows.
    pub fn state(&self) -> &TestState {
        &self.state
    }

    /// Whether `id` is a virtual pad this session created.
    pub fn made_here(&self, id: DeviceId, info: &DeviceInfo) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.virtual_run
                .as_ref()
                .is_some_and(|run| run.checker.is_ours(id, info))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (id, info);
            false
        }
    }

    /// Applies a command of the page at `now`; `Start` and `Close` are the
    /// worker's.
    pub fn command(&mut self, command: TestCommand, provider: &mut dyn Provider, now: f64) {
        let Some(session) = &mut self.session else {
            return;
        };
        match command {
            TestCommand::Answer(answer) => session.answer(answer, now),
            TestCommand::Skip => session.skip(now),
            TestCommand::NotOnThisPad => session.not_on_this_pad(now),
            TestCommand::NothingHappens => session.nothing_happens(now),
            TestCommand::Repeat => session.repeat(now),
            TestCommand::Stop | TestCommand::Close => self.finish(provider),
            TestCommand::Start(..) => {}
        }
    }

    /// A device went away at `now`; whether the session cares.
    pub fn removed(&mut self, id: DeviceId, now: f64) -> bool {
        if self.device != Some(id) {
            return false;
        }
        if let Some(session) = &mut self.session {
            session.removed(now);
        }
        self.device = None;
        self.fed = None;
        self.state.device = None;
        true
    }

    /// A device appeared at `now`: the pad under test coming back, while
    /// it is away; whether the session cares.
    pub fn added(&mut self, id: DeviceId, info: &DeviceInfo, now: f64) -> bool {
        if self.device.is_some() || self.made_here(id, info) {
            return false;
        }
        let Some(session) = &mut self.session else {
            return false;
        };
        session.added(&info.identity, now);
        self.device = Some(id);
        self.state.device = Some(id);
        true
    }

    /// Feeds the session what the worker read at `now`, sends the outputs
    /// it asks for, runs the virtual pad steps; whether the page needs
    /// redrawing.
    pub fn tick(
        &mut self,
        provider: &mut dyn Provider,
        devices: &BTreeMap<DeviceId, (DeviceView, Option<Open>)>,
        now: f64,
    ) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        if let Some((_, Some(open))) = self.device.and_then(|id| devices.get(&id)) {
            let fresh = self
                .fed
                .as_ref()
                .is_none_or(|(raw, at)| *raw != open.raw || now - at >= IDLE_UPDATE_MS);
            if fresh {
                session.update(&open.raw, &open.state, now);
                self.fed = Some((open.raw.clone(), now));
            }
        }
        for command in session.take_commands() {
            if let Some(id) = self.device {
                if let Err(e) = provider.send(id, command) {
                    log::info!("test output to device {}: {e}", id.0);
                }
            }
        }
        self.virtual_pads(provider, devices, now);
        let Some(session) = &mut self.session else {
            return true;
        };
        let status = session.status();
        if let Some(autopilot) = &self.autopilot {
            autopilot.follow(status.prompt);
        }
        let changed = status != self.state.status;
        if status.step != self.state.status.step {
            self.state.results = Arc::new(result_lines(&session.report().results));
        }
        self.state.status = status;
        if session.is_done() {
            self.finish(provider);
            return true;
        }
        changed
    }

    /// Ends the session: stops its outputs, drops its virtual pad, keeps
    /// the anonymous report.
    fn finish(&mut self, provider: &mut dyn Provider) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        for command in session.take_commands() {
            if let Some(id) = self.device {
                let _ = provider.send(id, command);
            }
        }
        #[cfg(target_os = "linux")]
        {
            self.virtual_run = None;
        }
        if let Some(autopilot) = &self.autopilot {
            autopilot.release();
        }
        let report = session.finish();
        self.state.status = Status {
            step: self.state.status.total,
            total: self.state.status.total,
            step_id: None,
            prompt: Prompt::Done,
        };
        self.state.results = Arc::new(result_lines(&report.results));
        self.state.finished = Some(Arc::new(report));
        log::info!("test session ended");
    }

    /// The virtual pad steps: creates the pad the step asks for, checks
    /// it, passes on what a game sends it.
    #[cfg(target_os = "linux")]
    fn virtual_pads(
        &mut self,
        provider: &mut dyn Provider,
        devices: &BTreeMap<DeviceId, (DeviceView, Option<Open>)>,
        now: f64,
    ) {
        use anygamepad::testing::VirtualCheck;

        use crate::virtual_pads::{Seen, VirtualRun};

        let Some(session) = &mut self.session else {
            return;
        };
        let prompt = session.status().prompt;
        if let Prompt::CheckVirtual { name, .. } = prompt {
            if self.virtual_run.as_ref().is_none_or(|run| run.name != name) {
                // Drops the previous pad first: one virtual pad at a time.
                self.virtual_run = None;
                let known = devices.keys().copied().collect();
                match VirtualRun::create(name, known, now) {
                    Ok(run) => self.virtual_run = Some(run),
                    Err(reason) => session.virtual_checked(VirtualCheck::Failed(reason), now),
                }
            }
        }
        let Some(run) = &mut self.virtual_run else {
            // No pad: its vibration goes nowhere, and the person says so.
            session.take_virtual_commands();
            return;
        };
        let mirror = self
            .device
            .and_then(|id| devices.get(&id))
            .and_then(|(_, open)| open.as_ref())
            .map(|open| open.state)
            .unwrap_or_default();
        match run.update(&mirror) {
            Ok(commands) => {
                // What a game sent to the virtual pad, for the real one.
                for command in commands {
                    if let Some(id) = self.device {
                        let _ = provider.send(id, command);
                    }
                }
            }
            Err(e) => {
                log::warn!("virtual {}: {e}", run.name);
                if matches!(prompt, Prompt::CheckVirtual { .. }) {
                    let reason = format!("cannot write the virtual pad: {e}");
                    session.virtual_checked(VirtualCheck::Failed(reason), now);
                }
            }
        }
        if matches!(prompt, Prompt::CheckVirtual { .. }) {
            let seen = devices.iter().map(|(id, (view, open))| Seen {
                id: *id,
                info: &view.info,
                state: match (open, &view.error) {
                    (Some(open), None) => Ok(&open.state),
                    (_, Some(error)) => Err(error.as_str()),
                    (None, None) => Err("not open"),
                },
            });
            if let Some(outcome) = run.checker.tick(seen, now) {
                log::info!("virtual {}: {outcome:?}", run.name);
                session.virtual_checked(outcome, now);
            }
        }
        for command in session.take_virtual_commands() {
            if let Some(id) = run.checker.device() {
                if let Err(e) = provider.send(id, command) {
                    log::info!("output to the virtual {}: {e}", run.name);
                }
            }
        }
    }

    /// No virtual pads on this OS yet: their steps do not apply, so they
    /// never ask; answered anyway, in case.
    #[cfg(not(target_os = "linux"))]
    fn virtual_pads(
        &mut self,
        _provider: &mut dyn Provider,
        _devices: &BTreeMap<DeviceId, (DeviceView, Option<Open>)>,
        now: f64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if let Prompt::CheckVirtual { .. } = session.status().prompt {
            session.virtual_checked(
                anygamepad::testing::VirtualCheck::Failed("no virtual pads on this OS yet".into()),
                now,
            );
        }
        session.take_virtual_commands();
    }
}

/// A saved test report, as the page lists it.
#[derive(Clone, Debug)]
pub struct SavedTest {
    /// The file.
    pub path: PathBuf,
    /// What was tested.
    pub confirmed: Selection,
}

/// The `test-reports` folder.
pub struct TestStore {
    dir: PathBuf,
}

impl TestStore {
    /// The store in `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `report` (formatted JSON) to `existing`, the file it was saved
    /// to before, else under a name no other report uses; returns its path.
    pub fn save(&self, report: &TestReport, existing: Option<&Path>) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let path = match existing.filter(|path| path.parent() == Some(self.dir.as_path())) {
            Some(path) => path.to_owned(),
            None => (1..1000)
                .map(|n| self.dir.join(report.file_name(n)))
                .find(|path| !path.exists())
                .ok_or_else(|| io::Error::other("too many reports of this configuration today"))?,
        };
        let text = report.to_json_pretty().map_err(io::Error::other)? + "\n";
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, &path)?;
        Ok(path)
    }

    /// The test reports in the folder; files that are not test reports
    /// are skipped.
    pub fn list(&self) -> Vec<SavedTest> {
        let Ok(dir) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut saved: Vec<SavedTest> = dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .filter_map(|path| {
                let text = fs::read_to_string(&path).ok()?;
                let report = TestReport::from_json(&text).ok()?;
                Some(SavedTest {
                    path,
                    confirmed: report.configuration.confirmed,
                })
            })
            .collect();
        saved.sort_by(|a, b| a.path.cmp(&b.path));
        saved
    }
}

#[cfg(test)]
mod tests {
    use anygamepad::model::identity::{Backend, DeviceIdentity, ProviderKind};
    use anygamepad::model::raw::{RawLayout, RawSource};
    use anygamepad::model::{Button, Capabilities, OutputSet};
    use anygamepad::provider::DeviceLocation;

    use super::*;
    use crate::devices::MappingSource;

    fn device() -> DeviceView {
        let mut identity = DeviceIdentity::new(
            ProviderKind::Native,
            Backend::Evdev,
            crate::collection::host_os(),
        );
        identity.vendor_id = Some(0x045e);
        identity.product_id = Some(0x028e);
        identity.product_name = Some("Microsoft X-Box 360 pad".into());
        identity.transport = Transport::Usb;
        identity.driver = Some("xpad".into());
        let info = DeviceInfo {
            identity,
            location: DeviceLocation::new(Some("/dev/input/event7".into()), None),
            layout: RawLayout {
                buttons: vec![RawSource::Evdev(0x130)],
                ..RawLayout::default()
            },
            outputs: OutputSet::EMPTY,
        };
        DeviceView {
            id: DeviceId(3),
            raw: anygamepad::model::raw::RawState::for_layout(&info.layout),
            info,
            mapping: MappingSource::LinuxSpec,
            state: Default::default(),
            capabilities: Capabilities {
                buttons: [Button::South].into_iter().collect(),
                ..Capabilities::default()
            },
            error: None,
            made_here: false,
        }
    }

    fn choice() -> Choice {
        Choice {
            model: "microsoft/xbox-360".into(),
            mode: "xinput".into(),
            transport: Transport::Usb,
            unit: Some(Code::new("X360-01").unwrap()),
        }
    }

    #[test]
    fn sessions_and_files() {
        let plan = TestPlan::get("quick").unwrap();
        let session = session(&device(), plan, SessionStart::Connecting, &choice(), None).unwrap();
        let mut bad = choice();
        bad.mode = "turbo".into();
        assert!(super::session(&device(), plan, SessionStart::List, &bad, None).is_err());
        let report = session.finish();
        // The device report is anonymous: no device path.
        let json = report.to_json().unwrap();
        assert!(!json.contains("/dev/input"), "{json}");

        let dir = std::env::temp_dir().join(format!("anygamepad-tests-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = TestStore::new(dir.clone());
        assert!(store.list().is_empty());
        let first = store.save(&report, None).unwrap();
        let second = store.save(&report, None).unwrap();
        assert_ne!(first, second);
        // Saving again replaces the file.
        assert_eq!(store.save(&report, Some(&second)).unwrap(), second);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("x360-01-xinput-usb-"), "{name}");
        assert!(name.ends_with("-1.json"), "{name}");
        fs::write(dir.join("notes.json"), "{}").unwrap();
        let saved = store.list();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].confirmed.model, "microsoft/xbox-360");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Runs `plan` on the demo pad through the device worker, its pad
    /// following the prompts; returns the report saved and read back.
    fn demo_session(plan: &str) -> TestReport {
        use std::time::{Duration, Instant};

        use crate::devices::{Command, Devices, demo_backend};

        let devices = Devices::start(Some(demo_backend()), eframe::egui::Context::default());
        let deadline = Instant::now() + Duration::from_secs(5);
        let device = loop {
            if let Some(device) = devices.snapshot().devices.first().cloned() {
                break device;
            }
            assert!(Instant::now() < deadline, "no demo pad");
            std::thread::sleep(Duration::from_millis(10));
        };
        // Detection, as the page proposes it.
        let identification = Catalog::embedded().identify(&device.info.identity, None);
        let best = identification.best().unwrap();
        let choice = Choice {
            model: best.model.id.clone(),
            mode: best.mode.id.clone(),
            transport: best.transport.unwrap(),
            unit: None,
        };
        assert_eq!(
            (choice.model.as_str(), choice.transport),
            ("generic/hid", Transport::Usb)
        );
        let plan = TestPlan::get(plan).unwrap();
        let session = session(
            &device,
            plan,
            SessionStart::Connecting,
            &choice,
            Some(&choice),
        )
        .unwrap();
        devices.send(Command::Test(TestCommand::Start(
            device.id,
            Box::new(session),
        )));
        let deadline = Instant::now() + Duration::from_secs(120);
        let report = loop {
            let snapshot = devices.snapshot();
            if let Some(report) = snapshot.test.and_then(|test| test.finished) {
                break report;
            }
            assert!(Instant::now() < deadline, "the session never ended");
            std::thread::sleep(Duration::from_millis(20));
        };
        devices.send(Command::Test(TestCommand::Close));

        let dir = std::env::temp_dir().join(format!(
            "anygamepad-demo-{}-{}",
            plan.id,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let store = TestStore::new(dir.clone());
        let path = store.save(&report, None).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_dir_all(&dir);
        let read = TestReport::from_json(&text).unwrap();
        read.validate().unwrap();
        assert_eq!(read, *report);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with(&format!(
                "generic-hid-dinput-usb-{}-desktop-",
                crate::collection::host_os()
            )),
            "{name}"
        );
        read
    }

    fn outcomes(report: &TestReport) -> Vec<(&str, StepStatus, Option<&str>)> {
        report
            .results
            .iter()
            .map(|r| (r.step.as_str(), r.status, r.observed.as_deref()))
            .collect()
    }

    #[test]
    fn quick_session_on_the_demo_pad() {
        let report = demo_session("quick");
        assert_eq!(report.plan.id, "quick");
        let outcomes = outcomes(&report);
        // Detection and the 17 buttons of a generic HID pad, every one passed.
        assert_eq!(outcomes.len(), 18, "{outcomes:?}");
        assert_eq!(outcomes[0].0, "detection");
        assert!(
            outcomes
                .iter()
                .all(|(_, status, _)| *status == StepStatus::Pass),
            "{outcomes:?}"
        );
        let south = &report.results[1];
        assert_eq!(south.step, "controls.south");
        // The evidence is the pad's raw states.
        assert!(!south.evidence.as_ref().unwrap().frames.is_empty());
    }

    #[test]
    fn full_session_on_the_demo_pad() {
        let report = demo_session("full");
        let outcomes = outcomes(&report);
        let steps: Vec<&str> = outcomes.iter().map(|(step, ..)| *step).collect();
        // A generic HID pad has no analog triggers, motion, touchpad,
        // battery or motors: their steps are left out. The autopilot unplugs
        // the demo pad and connects it again for the hotplug step.
        for step in ["sticks.left.rest", "sticks.right.circle", "rate", "hotplug"] {
            assert!(steps.contains(&step), "{step} in {steps:?}");
        }
        assert!(!steps.iter().any(|step| step.starts_with("triggers.")));
        assert!(
            outcomes
                .iter()
                .all(|(_, status, _)| *status == StepStatus::Pass),
            "{outcomes:?}"
        );
    }

    #[test]
    fn virtual_session_on_the_demo_pad() {
        let report = demo_session("virtual");
        let outcomes = outcomes(&report);
        for target in crate::virtual_pads::targets() {
            let id = format!("virtual.{}", target.name);
            let (_, status, observed) = outcomes
                .iter()
                .find(|(step, ..)| *step == id)
                .unwrap_or_else(|| panic!("{id} in {outcomes:?}"));
            match target.unavailable {
                // Skipped with the reason where the machine cannot.
                Some(reason) => {
                    assert_eq!(
                        (*status, *observed),
                        (StepStatus::NotApplicable, Some(reason))
                    )
                }
                // Where it can, the check runs: the demo provider never
                // shows the virtual pad.
                None => assert_eq!(
                    (*status, *observed),
                    (StepStatus::Error, Some("the virtual pad did not appear"))
                ),
            }
        }
        // The pad under test, a generic HID pad, has no motors: no
        // vibration steps.
        assert!(!outcomes.iter().any(|(step, ..)| step.ends_with(".rumble")));
    }

    #[test]
    fn result_lines_in_words() {
        let mut result = StepResult {
            step: "controls.south".into(),
            check: "button".into(),
            status: StepStatus::Fail,
            expected: Some("south".into()),
            observed: Some("east".into()),
            answer: Some(Answer::NotSure),
            measurements: BTreeMap::from([("noise".into(), 0.0123)]),
            duration_ms: 10,
            note: None,
            evidence: None,
        };
        assert_eq!(
            result_lines(std::slice::from_ref(&result))[0].detail,
            "expected south · observed east · answer not sure · noise 0.01"
        );
        result.expected = None;
        assert_eq!(status_name(result.status), "Fail");
    }
}
