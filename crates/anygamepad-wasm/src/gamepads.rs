//! The `Gamepads` class: anygamepad's web provider for JavaScript.

use std::collections::BTreeMap;

use crate::wizard::MappingWizard;
use anygamepad::model::identity::{Browser, DeviceIdentity, Fingerprint, Os};
use anygamepad::model::raw::{RawLayout, RawState};
use anygamepad::model::{
    Axis, Button, Capabilities, FaceLayout, Family, GamepadState, OutputCommand, OutputSet,
    Transport,
};
use anygamepad::provider::{Access, DeviceId, Provider, ProviderError, ProviderEvent};
use anygamepad::web::{self, WebProvider, standard};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_ACCESS: &str = r#"/**
 * How a gamepad is opened: `shared` reads input and drives outputs,
 * `observe_only` reads input only.
 */
export type Access = "shared" | "observe_only";"#;

/// A change in the connected devices.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceEvent {
    /// A device appeared.
    Added {
        /// Its id.
        device: u32,
    },
    /// A device disappeared; its id is no longer valid.
    Removed {
        /// Its id.
        device: u32,
    },
}

impl From<ProviderEvent> for DeviceEvent {
    fn from(event: ProviderEvent) -> Self {
        match event {
            ProviderEvent::Added(id) => Self::Added { device: js_id(id) },
            ProviderEvent::Removed(id) => Self::Removed { device: js_id(id) },
        }
    }
}

/// The browser and the OS it runs on, from the user agent.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
pub struct Environment {
    /// Browser engine and major version.
    pub browser: Browser,
    /// Operating system.
    pub os: Os,
}

/// What is known about a gamepad.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct DeviceInfo {
    /// Identity as the browser shows it (`web` holds `Gamepad.id` and
    /// `Gamepad.mapping`).
    pub identity: DeviceIdentity,
    /// Stable hash of the identity.
    pub fingerprint: Fingerprint,
    /// Controller family recognized from the identity alone, for drawings
    /// and labels; `undefined` when not recognized.
    pub family: Option<Family>,
    /// What is printed on the face buttons, when the family is known.
    pub face_layout: Option<FaceLayout>,
    /// The raw elements `poll()` fills: the Gamepad API's buttons and axes.
    pub layout: RawLayout,
    /// Outputs `send()` can drive.
    pub outputs: OutputSet,
    /// Canonical controls, for gamepads the active profile or standard layout maps;
    /// `undefined` for others.
    pub capabilities: Option<Capabilities>,
    /// Which canonical control each raw button and axis drives, under the
    /// same condition as `capabilities`.
    pub canonical: Option<CanonicalLayout>,
    /// Human-readable name of the matched profile or standard layout;
    /// `undefined` when not recognized.
    pub profile_name: Option<String>,
}

/// The canonical control of each raw element, by raw index (`undefined`
/// for an element that drives none).
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct CanonicalLayout {
    /// Per raw button. The analog trigger buttons also drive the trigger
    /// axes.
    pub buttons: Vec<Option<Button>>,
    /// Per raw axis. Stick Y axes are inverted (ADR 0001: Y positive = up).
    pub axes: Vec<Option<Axis>>,
}

/// One button as the Gamepad API reports it.
#[derive(Clone, Copy, Debug, Serialize, Tsify)]
pub struct ButtonSnapshot {
    /// `pressed`, as the browser decides it.
    pub pressed: bool,
    /// `touched`.
    pub touched: bool,
    /// `value`, in `[0, 1]`.
    pub value: f32,
}

/// A gamepad as the Gamepad API reports it, verbatim.
#[derive(Clone, Debug, Serialize, Tsify)]
pub struct GamepadSnapshot<'a> {
    /// `Gamepad.id`.
    pub id: &'a str,
    /// `Gamepad.mapping`.
    pub mapping: &'a str,
    /// `Gamepad.timestamp`: when the browser last received data from the
    /// gamepad, in milliseconds on the `performance.now()` clock. Its changes
    /// give the polling rate.
    pub timestamp: f64,
    /// `Gamepad.buttons`.
    pub buttons: Vec<ButtonSnapshot>,
    /// `Gamepad.axes` (Y grows downwards, as in the browser).
    pub axes: &'a [f32],
}

/// The browser's gamepads, read through anygamepad's web provider.
///
/// Call `update()` once per frame, from `requestAnimationFrame`, then read
/// each gamepad. Browsers only show a gamepad once the user presses one of
/// its buttons. Gamepad ids are numbers, valid until a `removed` event.
#[wasm_bindgen]
pub struct Gamepads {
    provider: WebProvider,
    events: Vec<ProviderEvent>,
    /// Raw state of each open gamepad, reused by every poll.
    raw: BTreeMap<DeviceId, RawState>,
    db: anygamepad::db::Database,
    mappers: BTreeMap<DeviceId, anygamepad::mapping::Mapper>,
}

#[wasm_bindgen]
impl Gamepads {
    /// Starts reading the browser's gamepads. Throws outside a browser
    /// window.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Gamepads, JsError> {
        Ok(Self {
            provider: WebProvider::new().map_err(error)?,
            events: Vec::new(),
            raw: BTreeMap::new(),
            db: anygamepad::db::Database::embedded(),
            mappers: BTreeMap::new(),
        })
    }

    /// The browser and the OS, from the user agent.
    #[wasm_bindgen(unchecked_return_type = "Environment")]
    pub fn environment(&self) -> Result<JsValue, JsError> {
        to_js(&Environment {
            browser: self.provider.browser(),
            os: self.provider.os(),
        })
    }

    /// Reads the Gamepad API and returns which gamepads appeared and
    /// disappeared since the last call. Call it once per frame.
    #[wasm_bindgen(unchecked_return_type = "DeviceEvent[]")]
    pub fn update(&mut self) -> Result<JsValue, JsError> {
        self.events.clear();
        self.provider.update(&mut self.events).map_err(error)?;
        for event in &self.events {
            if let ProviderEvent::Removed(id) = event {
                self.raw.remove(id);
                self.mappers.remove(id);
            }
        }
        let events: Vec<DeviceEvent> = self.events.iter().map(|&event| event.into()).collect();
        to_js(&events)
    }

    /// Ids of the connected gamepads.
    pub fn devices(&self) -> Vec<u32> {
        self.provider.devices().map(js_id).collect()
    }

    /// What is known about a gamepad; `undefined` for an unknown id.
    #[wasm_bindgen(unchecked_return_type = "DeviceInfo | undefined")]
    pub fn info(&self, device: u32) -> Result<JsValue, JsError> {
        let Some(info) = self.provider.info(device_id(device)) else {
            return Ok(JsValue::UNDEFINED);
        };
        let (buttons, axes) = (info.layout.buttons.len(), info.layout.axes.len());
        if let Some((mapper, profile)) =
            anygamepad::mapping::Mapper::from_database(&self.db, &info.identity, &info.layout)
        {
            let mut capabilities = mapper.capabilities().clone();
            capabilities.outputs = info.outputs;
            let standard = info
                .identity
                .web
                .as_ref()
                .is_some_and(|web| web.mapping == standard::MAPPING);
            if standard {
                for i in 0..buttons {
                    if let Some(b) = standard::button(i) {
                        capabilities.buttons.insert(b);
                    }
                }
            }
            let family = profile
                .family
                .or_else(|| Family::from_identity(&info.identity));
            let face_layout = profile.face.or_else(|| family.map(Family::face_layout));
            let mapping = profile.mapping_for(&info.identity);
            let canonical = canonical_layout_from_mapping(&mapping, &info.layout, standard);
            return to_js(&DeviceInfo {
                fingerprint: info.identity.fingerprint(),
                family,
                face_layout,
                identity: info.identity.clone(),
                layout: info.layout.clone(),
                outputs: info.outputs,
                capabilities: Some(capabilities),
                canonical: Some(canonical),
                profile_name: Some(profile.name.clone()),
            });
        }
        let standard = info
            .identity
            .web
            .as_ref()
            .is_some_and(|web| web.mapping == standard::MAPPING);
        let family = Family::from_identity(&info.identity);
        to_js(&DeviceInfo {
            fingerprint: info.identity.fingerprint(),
            family,
            face_layout: family.map(Family::face_layout),
            identity: info.identity.clone(),
            layout: info.layout.clone(),
            outputs: info.outputs,
            capabilities: standard.then(|| standard::capabilities(buttons, axes, info.outputs)),
            canonical: standard.then(|| CanonicalLayout {
                buttons: (0..buttons).map(standard::button).collect(),
                axes: (0..axes)
                    .map(|index| standard::AXES.get(index).copied())
                    .collect(),
            }),
            profile_name: standard.then(|| "Gamepad API standard layout".to_string()),
        })
    }

    /// Opens a gamepad, to poll it and drive its outputs (`shared` by
    /// default).
    pub fn open(
        &mut self,
        device: u32,
        #[wasm_bindgen(unchecked_optional_param_type = "Access")] access: Option<String>,
    ) -> Result<(), JsError> {
        let id = device_id(device);
        let access = match access.as_deref() {
            None | Some("shared") => Access::Shared,
            Some("observe_only") => Access::ObserveOnly,
            Some(other) => return Err(JsError::new(&format!("unknown access mode `{other}`"))),
        };
        self.provider.open(id, access).map_err(error)?;
        let info = self.provider.info(id).ok_or_else(unknown)?;
        self.raw
            .entry(id)
            .or_insert_with(|| RawState::for_layout(&info.layout));
        if let Some((mapper, _)) =
            anygamepad::mapping::Mapper::from_database(&self.db, &info.identity, &info.layout)
        {
            self.mappers.insert(id, mapper);
        }
        Ok(())
    }

    /// Closes a gamepad and stops its rumble.
    pub fn close(&mut self, device: u32) {
        let id = device_id(device);
        self.provider.close(id);
        self.raw.remove(&id);
        self.mappers.remove(&id);
    }

    /// Builds a device report for `device`.
    #[wasm_bindgen(js_name = buildReport, unchecked_return_type = "ScrubResult")]
    pub fn build_report(
        &self,
        device: u32,
        user_product: Option<String>,
        controller_mode: Option<String>,
        now_ms: Option<f64>,
    ) -> Result<JsValue, JsError> {
        self.build_report_inner(device, None, user_product, controller_mode, now_ms)
    }

    /// Builds a device report for `device` including wizard findings.
    #[wasm_bindgen(js_name = buildReportWithWizard, unchecked_return_type = "ScrubResult")]
    pub fn build_report_with_wizard(
        &self,
        device: u32,
        wizard: &MappingWizard,
        user_product: Option<String>,
        controller_mode: Option<String>,
        now_ms: Option<f64>,
    ) -> Result<JsValue, JsError> {
        self.build_report_inner(device, Some(wizard), user_product, controller_mode, now_ms)
    }

    fn build_report_inner(
        &self,
        device: u32,
        wizard: Option<&MappingWizard>,
        user_product: Option<String>,
        controller_mode: Option<String>,
        now_ms: Option<f64>,
    ) -> Result<JsValue, JsError> {
        let id = device_id(device);
        let mut report = crate::report::base_web_report(
            self.provider.os(),
            Some(self.provider.browser()),
            now_ms,
            user_product,
            controller_mode,
        )?;
        let mut view = self.report_view(id).ok_or_else(unknown)?;
        if let Some(w) = wizard {
            let (missing_buttons, missing_axes) = w.missing_controls();
            report.user.missing_buttons = missing_buttons;
            report.user.missing_axes = missing_axes;
            if let Some(confirmed) = w.confirmed_mapping() {
                view.confirmed = Some(confirmed);
            }
        }
        report.views.push(view);
        crate::report::finish_report(report)
    }

    /// Raw values of an open gamepad, as read by the last `update()`: each
    /// button's `value` and each axis, in the order of its layout.
    #[wasm_bindgen(unchecked_return_type = "RawState")]
    pub fn poll(&mut self, device: u32) -> Result<JsValue, JsError> {
        to_js(self.poll_raw(device_id(device))?)
    }

    /// Canonical state of an open gamepad, mapped according to its profile or
    /// standard layout, as read by the last `update()`; `undefined` for
    /// unmapped gamepads.
    #[wasm_bindgen(unchecked_return_type = "GamepadState | undefined")]
    pub fn state(&mut self, device: u32) -> Result<JsValue, JsError> {
        let id = device_id(device);
        // Unknown devices are an error, as in `poll` and `send`.
        if self.provider.info(id).is_none() {
            return Err(error(ProviderError::UnknownDevice));
        }
        let standard = self
            .provider
            .info(id)
            .and_then(|info| info.identity.web.as_ref())
            .is_some_and(|web| web.mapping == standard::MAPPING);
        let has_mapper = self.mappers.contains_key(&id);
        if !has_mapper && !standard {
            return Ok(JsValue::UNDEFINED);
        }
        self.poll_raw(id)?;
        to_js(&self.canonical(id))
    }

    /// `Gamepad.timestamp` at the last `update()`: when the browser last
    /// received data from the gamepad, in milliseconds on the
    /// `performance.now()` clock; `undefined` for an unknown id. Cheaper than
    /// `snapshot()`, to measure how often the data changes.
    pub fn timestamp(&self, device: u32) -> Option<f64> {
        self.provider
            .snapshot(device_id(device))
            .map(|snapshot| snapshot.timestamp)
    }

    /// What the Gamepad API reported for a gamepad at the last `update()`,
    /// verbatim; `undefined` for an unknown id. The gamepad does not need to
    /// be open.
    #[wasm_bindgen(unchecked_return_type = "GamepadSnapshot | undefined")]
    pub fn snapshot(&self, device: u32) -> Result<JsValue, JsError> {
        let Some(snapshot) = self.provider.snapshot(device_id(device)) else {
            return Ok(JsValue::UNDEFINED);
        };
        to_js(&GamepadSnapshot {
            id: &snapshot.id,
            mapping: &snapshot.mapping,
            timestamp: snapshot.timestamp,
            buttons: snapshot.buttons.iter().map(button_snapshot).collect(),
            axes: &snapshot.axes,
        })
    }

    /// Starts the guided mapping wizard on a gamepad's raw buttons and axes.
    pub fn wizard(&self, device: u32) -> Result<MappingWizard, JsError> {
        let info = self.provider.info(device_id(device)).ok_or_else(unknown)?;
        Ok(MappingWizard::new(info.layout.clone()))
    }

    /// Feeds `wizard` with an open gamepad's raw values as read by the last
    /// `update()`, at `now` (`performance.now()`). Call it once a frame.
    #[wasm_bindgen(js_name = feedWizard)]
    pub fn feed_wizard(
        &mut self,
        device: u32,
        wizard: &mut MappingWizard,
        now: f64,
    ) -> Result<(), JsError> {
        let raw = self.poll_raw(device_id(device))?;
        wizard.feed(raw, now);
        Ok(())
    }

    /// Drives an output of an open gamepad, for example
    /// `{ rumble: { low_frequency: 1, high_frequency: 0.5 } }`; zero stops it.
    /// Throws if the gamepad cannot.
    pub fn send(
        &mut self,
        device: u32,
        #[wasm_bindgen(unchecked_param_type = "OutputCommand")] command: JsValue,
    ) -> Result<(), JsError> {
        let command: OutputCommand =
            serde_wasm_bindgen::from_value(command).map_err(|e| JsError::new(&e.to_string()))?;
        self.provider
            .send(device_id(device), command)
            .map_err(error)
    }

    /// What catalog model, mode and transport a gamepad can be, most likely
    /// first (test mode, H10).
    #[wasm_bindgen(unchecked_return_type = "Detection")]
    pub fn identify(&self, device: u32) -> Result<JsValue, JsError> {
        let info = self.provider.info(device_id(device)).ok_or_else(unknown)?;
        to_js(&crate::testing::detect(&info.identity))
    }

    /// Whether a gamepad is the model, mode and transport picked from the
    /// test mode's list.
    #[wasm_bindgen(js_name = checkChoice, unchecked_return_type = "ChoiceResult")]
    pub fn check_choice(
        &self,
        device: u32,
        model: &str,
        mode: &str,
        #[wasm_bindgen(unchecked_param_type = "Transport")] transport: JsValue,
    ) -> Result<JsValue, JsError> {
        let info = self.provider.info(device_id(device)).ok_or_else(unknown)?;
        let transport: Transport =
            serde_wasm_bindgen::from_value(transport).map_err(|e| JsError::new(&e.to_string()))?;
        to_js(&crate::testing::check_choice(
            &info.identity,
            model,
            mode,
            transport,
        ))
    }

    /// Starts a test session on an open gamepad (test mode, H4); `now_ms`
    /// (`Date.now()`) dates the report.
    #[wasm_bindgen(js_name = startTest)]
    pub fn start_test(
        &self,
        device: u32,
        #[wasm_bindgen(unchecked_param_type = "TestSetup")] setup: JsValue,
        now_ms: f64,
    ) -> Result<crate::testing::TestSession, JsError> {
        let id = device_id(device);
        let info = self.provider.info(id).ok_or_else(unknown)?;
        let mut report = crate::report::base_web_report(
            self.provider.os(),
            Some(self.provider.browser()),
            Some(now_ms),
            None,
            None,
        )?;
        report.views.push(self.report_view(id).ok_or_else(unknown)?);
        crate::testing::start(setup, report, info.outputs, false, Default::default())
    }

    /// Feeds `session` with an open gamepad's values as read by the last
    /// `update()`, at `now` (`performance.now()`), and drives the outputs
    /// the session asks for. Call it once a frame.
    #[wasm_bindgen(js_name = feedTest)]
    pub fn feed_test(
        &mut self,
        device: u32,
        session: &mut crate::testing::TestSession,
        now: f64,
    ) -> Result<(), JsError> {
        let id = device_id(device);
        self.poll_raw(id)?;
        let state = self.canonical(id);
        session.session.update(&self.raw[&id], &state, now);
        for command in session.session.take_commands() {
            // An output that fails shows in the person's answer.
            let _ = self.provider.send(id, command);
        }
        Ok(())
    }

    /// Tells `session` that a gamepad connected while it waits for the pad
    /// under test to come back (the hotplug step), at `now`.
    #[wasm_bindgen(js_name = testReconnected)]
    pub fn test_reconnected(
        &self,
        device: u32,
        session: &mut crate::testing::TestSession,
        now: f64,
    ) -> Result<(), JsError> {
        let info = self.provider.info(device_id(device)).ok_or_else(unknown)?;
        session.session.added(&info.identity, now);
        Ok(())
    }
}

impl Gamepads {
    /// The canonical state of an open gamepad from its raw values as last
    /// polled: through its profile, else the standard layout; at rest for
    /// gamepads with neither.
    pub(crate) fn canonical(&self, id: DeviceId) -> GamepadState {
        let mut state = GamepadState::new();
        let Some(raw) = self.raw.get(&id) else {
            return state;
        };
        let standard = self
            .provider
            .info(id)
            .and_then(|info| info.identity.web.as_ref())
            .is_some_and(|web| web.mapping == standard::MAPPING);
        if let Some(mapper) = self.mappers.get(&id) {
            mapper.apply(raw, &mut state);
            if standard {
                for (index, &value) in raw.buttons.iter().enumerate().skip(17) {
                    if let Some(b) = standard::button(index) {
                        state.set_pressed(b, value >= anygamepad::mapping::PRESS_THRESHOLD);
                    }
                }
            }
        } else if standard {
            standard::map(raw, &mut state);
        }
        state
    }

    /// The device report view of a gamepad: identity, layout, outputs, and
    /// the profile and capabilities the library maps it with.
    pub(crate) fn report_view(&self, id: DeviceId) -> Option<anygamepad_report::View> {
        let info = self.provider.info(id)?;
        let mut view =
            anygamepad_report::View::new(info.identity.clone(), info.layout.clone(), info.outputs);
        let standard = info
            .identity
            .web
            .as_ref()
            .is_some_and(|web| web.mapping == standard::MAPPING);
        if let Some((mapper, profile)) =
            anygamepad::mapping::Mapper::from_database(&self.db, &info.identity, &info.layout)
        {
            let mut capabilities = mapper.capabilities().clone();
            capabilities.outputs = info.outputs;
            if standard {
                for i in 0..info.layout.buttons.len() {
                    if let Some(b) = standard::button(i) {
                        capabilities.buttons.insert(b);
                    }
                }
            }
            view.profile = Some(profile.name.clone());
            view.capabilities = Some(capabilities);
        } else if standard {
            view.profile = Some("Gamepad API standard layout".into());
            view.capabilities = Some(standard::capabilities(
                info.layout.buttons.len(),
                info.layout.axes.len(),
                info.outputs,
            ));
        }
        Some(view)
    }

    fn poll_raw(&mut self, id: DeviceId) -> Result<&RawState, JsError> {
        if self.provider.info(id).is_none() {
            return Err(unknown());
        }
        let raw = self
            .raw
            .get_mut(&id)
            .ok_or_else(|| error(ProviderError::NotOpen))?;
        self.provider.poll(id, raw).map_err(error)?;
        Ok(raw)
    }
}

fn button_snapshot(button: &web::ButtonSnapshot) -> ButtonSnapshot {
    ButtonSnapshot {
        pressed: button.pressed,
        touched: button.touched,
        value: button.value,
    }
}

fn canonical_layout_from_mapping(
    mapping: &anygamepad::db::Mapping,
    layout: &RawLayout,
    is_standard: bool,
) -> CanonicalLayout {
    let mut buttons = vec![None; layout.buttons.len()];
    for (&button, &input) in &mapping.buttons {
        if let anygamepad::db::Input::Button(anygamepad::model::raw::RawSource::Index(i)) = input {
            if let Some(slot) = buttons.get_mut(i as usize) {
                *slot = Some(button);
            }
        }
    }

    let mut axes = vec![None; layout.axes.len()];
    for (&axis, axis_mapping) in &mapping.axes {
        let input = axis_mapping
            .input
            .or(axis_mapping.positive)
            .or(axis_mapping.negative);
        if let Some(anygamepad::db::Input::Axis {
            source: anygamepad::model::raw::RawSource::Index(i),
            ..
        }) = input
        {
            if let Some(slot) = axes.get_mut(i as usize) {
                *slot = Some(axis);
            }
        }
    }

    for (i, &source) in layout.buttons.iter().enumerate() {
        if buttons[i].is_none() {
            if let anygamepad::model::raw::RawSource::Button(b) = source {
                buttons[i] = Some(b);
            } else if is_standard {
                buttons[i] = standard::button(i);
            }
        }
    }
    for (i, info) in layout.axes.iter().enumerate() {
        if axes[i].is_none() {
            if let anygamepad::model::raw::RawSource::Axis(a) = info.source {
                axes[i] = Some(a);
            } else if is_standard {
                axes[i] = standard::AXES.get(i).copied();
            }
        }
    }

    CanonicalLayout { buttons, axes }
}

/// The JavaScript form of a device id. The web providers count ids up from
/// 1, so they fit.
pub(crate) fn js_id(id: DeviceId) -> u32 {
    u32::try_from(id.0).unwrap_or(u32::MAX)
}

pub(crate) fn device_id(device: u32) -> DeviceId {
    DeviceId(device.into())
}

pub(crate) fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value).map_err(|e| JsError::new(&e.to_string()))
}

pub(crate) fn error(error: ProviderError) -> JsError {
    JsError::new(&error.to_string())
}

pub(crate) fn unknown() -> JsError {
    error(ProviderError::UnknownDevice)
}
