# Browser Gamepad API and WebHID

Sources: Chromium `device/gamepad/` (main, 2026-10-10), Firefox
`dom/gamepad/` (`mozilla-firefox/firefox` main), WebKit
`Source/WebCore/platform/gamepad/` (main), caniuse.com for WebHID.

## `Gamepad.id` formats

| Browser | Backend | `id` |
|---------|---------|------|
| Chromium | HID/evdev/IOKit (`GamepadDataFetcher::UpdateGamepadStrings`) | `"<product> (STANDARD GAMEPAD Vendor: 054c Product: 09cc)"`, without `STANDARD GAMEPAD ` when there is no standard mapping |
| Chromium, Windows | XInput | `"Xbox 360 Controller (XInput STANDARD GAMEPAD)"`: **no VID/PID** |
| Chromium, Windows | Windows.Gaming.Input | `"<display name> (STANDARD GAMEPAD Vendor: vvvv Product: pppp)"`; known XInput devices keep the XInput string above; unknown devices get `"<display name> (STANDARD GAMEPAD)"` |
| Firefox (Linux evdev, Windows Raw Input, macOS IOKit) | | `"054c-09cc-<product>"` (`%04x-%04x-%s`) |
| Firefox, Windows | XInput | `"xinput"` |
| Safari | IOKit HID (`HIDGamepad`) | `"<vid hex>-<pid hex>-<product>"` (comment: "matches Firefox"); uses WTF `hex()` without an explicit width, so leading zeros may be missing (to verify on a real device) |
| Safari | GameController framework | `"<vendorName> Extended Gamepad"` or `"<vendorName> Gamepad"`: **no VID/PID** |

Safari's HID provider skips devices that GameController.framework supports
(`GCController supportsHIDDevice:`), so the common pads (Xbox, PlayStation,
Switch Pro) appear without VID/PID.

## `"standard"` mapping

- Chromium maps known devices to the W3C standard layout with per-OS tables:
  65 device entries on Linux, 36 on Windows, 57 on macOS
  (`gamepad_standard_mappings_*.cc/mm`), plus XInput/WGI pads.
- Chromium also speaks some device protocols itself (`nintendo_controller.cc`
  for Switch Pro and Joy-Cons, `dualshock4_controller.cc`), for example to
  enable full reports and rumble.
- Firefox remaps 35 devices in `GamepadRemapping.cpp`; others are exposed with
  an empty mapping.
- Safari reports `"standard"` for GameController.framework "extended" gamepads.
- Result: the same pad on the same OS can be `"standard"` in one browser and
  unmapped in another, and the axis/button indices differ.

## Other API surface (`web-sys` 0.3.106)

- Stable: `id`, `index`, `connected`, `timestamp`, `mapping`, `axes`,
  `buttons`, `hand`, `haptic_actuators`, `pose`.
- Behind `--cfg=web_sys_unstable_apis`: `vibrationActuator`
  (`playEffect`, `reset`), `touchEvents` (`GamepadTouch`), and the whole WebHID
  API (`Hid`, `HidDevice`, ...).
- The API is poll-based: state must be read every frame
  (`navigator.getGamepads()` in `requestAnimationFrame`); browsers only expose
  pads after the user presses a button.

## WebHID

- Chromium-based browsers only (Chrome/Edge since 89). Firefox and Safari do
  not support it (caniuse, 2026); no change of position was found.
- Requires a user gesture and a chooser per device (`navigator.hid.requestDevice`).
- Gives VID/PID, report descriptors (collections) and raw input/feature
  reports: the data the desktop collector gets, without installing anything.

## DualSense in Chrome on macOS (added 2026-10-14)

- The owner's DualSense does not show in Chrome on macOS:
  `navigator.getGamepads()` returns `[null, null, null, null]`. The same pad
  works on the same Mac in `germadev/motorust`, which reads it through
  GameController.framework (`src/gamepads/apple.rs`).
- motorust's records (`RECORD/2026-10-09.foundation.completed.md`, item 31)
  explain why gilrs, an IOKit HID reader, sees the pad connected but gets no
  input: macOS's GameController switches the DualSense to the extended
  report (`0x31`), and the standard reports with buttons and axes stop.
- Chrome reads pads on macOS through IOKit HID too, and only exposes a pad
  after an input. A pad with no standard reports is therefore never exposed.
  This is inferred from the two observations above; Chrome's code was not
  re-read. Safari (GameController) and WebHID in Chrome (reading `0x31`, see
  note 09) should both see the pad.
- Safari on the same Mac shows it (owner's test, 2026-10-14): `id`
  `"DualSense Wireless Controller Extended Gamepad"` (the GameController
  format above, no VID/PID), `"standard"` mapping, 17 buttons (no touchpad
  click), 4 axes, `vibrationActuator` with `dual-rumble` but not
  `trigger-rumble`. The tester read 0 updates/s with the pad at rest; whether
  Safari only moves `timestamp` on changes is not yet checked.

## Implications for anygamepad (C2, C11, W2, W3)

- The web identity parser must handle all formats above and treat VID/PID as
  optional. Without VID/PID the identity is name + browser + mapping, so the
  web tester should offer WebHID (on Chromium) to resolve it.
- The browser (engine and version) and its backend are identity dimensions;
  profiles need per-browser overrides (D1), as planned.
- Reports from the web tester are the only source of browser remapping data;
  GameControllerDB has no browser platform.
- Using WebHID and Gamepad API on the same device: Chromium may already be
  driving the pad (Switch Pro handshake). Test W3 against Chromium's own
  drivers before sending handshakes ourselves.
- Avoid enabling `web_sys_unstable_apis` crate-wide; declare the few unstable
  bindings we need (vibration, WebHID) with our own `wasm-bindgen` externs, or
  confine the cfg to `anygamepad-wasm`.
