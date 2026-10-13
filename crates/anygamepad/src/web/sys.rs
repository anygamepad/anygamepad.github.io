//! The browser's Gamepad API, through web-sys.

use std::cell::RefCell;
use std::rc::Rc;

use anygamepad_model::{Output, OutputSet};
use js_sys::{Array, JsString, Object, Promise, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Gamepad, GamepadButton, GamepadEvent, Navigator, Performance, Window};

use super::provider::{ButtonSnapshot, GamepadSnapshot, GamepadSource, HapticEffect, HapticKind};
use super::{WebProvider, detect};
use crate::provider::ProviderError;

// Bindings web-sys only has behind `web_sys_unstable_apis` (haptics), or
// with types that lose information.
#[wasm_bindgen]
extern "C" {
    /// The members of `Gamepad` that web-sys lacks; see [`ext`].
    #[wasm_bindgen(js_name = Gamepad)]
    type GamepadExt;

    /// `Gamepad.id`, as a JavaScript string, to compare it without copying.
    #[wasm_bindgen(method, getter, js_name = id)]
    fn id_js(this: &GamepadExt) -> JsString;

    /// `Gamepad.mapping`, verbatim (web-sys turns it into an enum).
    #[wasm_bindgen(method, getter, js_name = mapping)]
    fn mapping_string(this: &GamepadExt) -> String;

    /// `Gamepad.vibrationActuator`: missing in Firefox.
    #[wasm_bindgen(method, getter, js_name = vibrationActuator)]
    fn vibration_actuator(this: &GamepadExt) -> Option<VibrationActuator>;

    /// `GamepadHapticActuator`.
    type VibrationActuator;

    /// The effect types it supports (recent browsers).
    #[wasm_bindgen(method, getter)]
    fn effects(this: &VibrationActuator) -> Option<Array>;

    /// Its only effect type (older browsers).
    #[wasm_bindgen(method, getter, js_name = type)]
    fn kind(this: &VibrationActuator) -> Option<String>;

    #[wasm_bindgen(method, catch, js_name = playEffect)]
    fn play_effect(
        this: &VibrationActuator,
        kind: &str,
        params: &Object,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn reset(this: &VibrationActuator) -> Result<Promise, JsValue>;
}

fn ext(gamepad: &Gamepad) -> &GamepadExt {
    gamepad.unchecked_ref()
}

/// `navigator.getGamepads()`, with `gamepaddisconnected` events counting
/// the disconnections of each slot.
///
/// Counting disconnections rather than connections: browsers can show a new
/// gamepad in `getGamepads()` a frame before its `gamepadconnected` event,
/// which would then look like a reconnection.
pub(super) struct BrowserGamepads {
    window: Window,
    navigator: Navigator,
    performance: Option<Performance>,
    /// Disconnections per slot, counted by `on_disconnected`.
    disconnections: Rc<RefCell<Vec<u32>>>,
    on_disconnected: Closure<dyn FnMut(GamepadEvent)>,
    /// `Gamepad.id` per slot at the last read.
    ids: Vec<Option<JsString>>,
    /// Swallows the rejections of haptic promises: haptics are best effort.
    ignore: Closure<dyn FnMut(JsValue)>,
}

impl BrowserGamepads {
    pub(super) fn new() -> Result<Self, ProviderError> {
        let window = web_sys::window().ok_or(ProviderError::Unsupported)?;
        let navigator = window.navigator();
        if !Reflect::has(&navigator, &"getGamepads".into()).unwrap_or(false) {
            return Err(ProviderError::Unsupported);
        }
        let disconnections = Rc::new(RefCell::new(Vec::<u32>::new()));
        let counters = Rc::clone(&disconnections);
        let on_disconnected =
            Closure::<dyn FnMut(GamepadEvent)>::new(move |event: GamepadEvent| {
                if let Some(gamepad) = event.gamepad() {
                    let slot = gamepad.index() as usize;
                    let mut counters = counters.borrow_mut();
                    if counters.len() <= slot {
                        counters.resize(slot + 1, 0);
                    }
                    counters[slot] = counters[slot].wrapping_add(1);
                }
            });
        window
            .add_event_listener_with_callback(
                "gamepaddisconnected",
                on_disconnected.as_ref().unchecked_ref(),
            )
            .map_err(|_| ProviderError::Backend("cannot listen to gamepad events".into()))?;
        Ok(Self {
            performance: window.performance(),
            window,
            navigator,
            disconnections,
            on_disconnected,
            ids: Vec::new(),
            ignore: Closure::new(|_: JsValue| {}),
        })
    }

    fn actuator(&self, slot: usize) -> Option<VibrationActuator> {
        let gamepads = self.navigator.get_gamepads().ok()?;
        let gamepad = gamepads.get(slot as u32).dyn_into::<Gamepad>().ok()?;
        ext(&gamepad).vibration_actuator()
    }

    fn ignore_rejection(&self, promise: Result<Promise, JsValue>) {
        if let Ok(promise) = promise {
            let _ = promise.catch(&self.ignore);
        }
    }
}

impl Drop for BrowserGamepads {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(
            "gamepaddisconnected",
            self.on_disconnected.as_ref().unchecked_ref(),
        );
    }
}

impl GamepadSource for BrowserGamepads {
    fn read(&mut self, slots: &mut Vec<Option<GamepadSnapshot>>) -> Result<(), ProviderError> {
        let gamepads = self
            .navigator
            .get_gamepads()
            .map_err(|_| ProviderError::PermissionDenied)?;
        let count = gamepads.length() as usize;
        slots.resize_with(count, || None);
        self.ids.resize(count, None);
        let disconnections = self.disconnections.borrow();
        for (slot, entry) in slots.iter_mut().enumerate() {
            let gamepad = gamepads
                .get(slot as u32)
                .dyn_into::<Gamepad>()
                .ok()
                .filter(Gamepad::connected);
            let Some(gamepad) = gamepad else {
                *entry = None;
                self.ids[slot] = None;
                continue;
            };
            let connection = disconnections.get(slot).copied().unwrap_or(0);
            let id = ext(&gamepad).id_js();
            let same = entry
                .as_ref()
                .is_some_and(|snapshot| snapshot.connection == connection)
                && self.ids[slot].as_ref() == Some(&id);
            if !same {
                *entry = Some(GamepadSnapshot {
                    connection,
                    id: String::from(&id),
                    mapping: ext(&gamepad).mapping_string(),
                    haptics: haptics(&gamepad),
                    ..GamepadSnapshot::default()
                });
                self.ids[slot] = Some(id);
            }
            let snapshot = entry.get_or_insert_with(GamepadSnapshot::default);
            snapshot.timestamp = gamepad.timestamp();
            read_buttons(&gamepad, &mut snapshot.buttons);
            read_axes(&gamepad, &mut snapshot.axes);
        }
        Ok(())
    }

    fn now(&self) -> f64 {
        self.performance.as_ref().map_or(0.0, Performance::now)
    }

    fn play_effect(&mut self, slot: usize, effect: HapticEffect) {
        let Some(actuator) = self.actuator(slot) else {
            return;
        };
        let params = Object::new();
        let set = |key: &str, value: f64| {
            let _ = Reflect::set(&params, &key.into(), &value.into());
        };
        set("duration", effect.duration_ms);
        set("startDelay", 0.0);
        set("strongMagnitude", effect.strong_magnitude.into());
        set("weakMagnitude", effect.weak_magnitude.into());
        if effect.kind == HapticKind::TriggerRumble {
            set("leftTrigger", effect.left_trigger.into());
            set("rightTrigger", effect.right_trigger.into());
        }
        self.ignore_rejection(actuator.play_effect(effect.kind.as_str(), &params));
    }

    fn stop_effects(&mut self, slot: usize) {
        if let Some(actuator) = self.actuator(slot) {
            self.ignore_rejection(actuator.reset());
        }
    }
}

/// The effects a gamepad's `vibrationActuator` can play.
fn haptics(gamepad: &Gamepad) -> OutputSet {
    let Some(actuator) = ext(gamepad).vibration_actuator() else {
        return OutputSet::EMPTY;
    };
    let output = |kind: &str| match kind {
        "dual-rumble" => Some(Output::Rumble),
        "trigger-rumble" => Some(Output::TriggerRumble),
        _ => None,
    };
    match actuator.effects() {
        Some(effects) => effects
            .iter()
            .filter_map(|kind| kind.as_string().as_deref().and_then(output))
            .collect(),
        None => actuator
            .kind()
            .as_deref()
            .and_then(output)
            .into_iter()
            .collect(),
    }
}

fn read_buttons(gamepad: &Gamepad, buttons: &mut Vec<ButtonSnapshot>) {
    let list = gamepad.buttons();
    buttons.resize(list.length() as usize, ButtonSnapshot::default());
    for (i, button) in buttons.iter_mut().enumerate() {
        *button = match list.get(i as u32).dyn_into::<GamepadButton>() {
            Ok(js) => ButtonSnapshot {
                pressed: js.pressed(),
                touched: js.touched(),
                value: js.value() as f32,
            },
            Err(_) => ButtonSnapshot::default(),
        };
    }
}

fn read_axes(gamepad: &Gamepad, axes: &mut Vec<f32>) {
    let list = gamepad.axes();
    axes.resize(list.length() as usize, 0.0);
    for (i, axis) in axes.iter_mut().enumerate() {
        *axis = list.get(i as u32).as_f64().unwrap_or(0.0) as f32;
    }
}

impl WebProvider {
    /// The browser's gamepads, with the browser and OS detected from the
    /// user agent.
    ///
    /// Fails with [`ProviderError::Unsupported`] outside a window (in a
    /// worker, or without the Gamepad API).
    pub fn new() -> Result<Self, ProviderError> {
        let source = BrowserGamepads::new()?;
        let user_agent = source.navigator.user_agent().unwrap_or_default();
        let touch_points = u32::try_from(source.navigator.max_touch_points()).unwrap_or(0);
        let (browser, os) = detect(&user_agent, touch_points);
        Ok(Self::with_source(Box::new(source), browser, os))
    }
}
