# How to combine the providers

The F2 output. It builds on notes 01-07 and is the input for ADR 2 (input
provider strategy) in F3 and for C12 (arbitration).

## Which provider handles what best

| Device family | Linux | Windows | macOS | Browser |
|---------------|-------|---------|-------|---------|
| Xbox 360 / One / Series | Native evdev (`xpad`, `hid-microsoft` over BT) | Native XInput, correlated with the HID/PnP node for VID/PID | Native IOKit, or GameController.framework | Gamepad API (no VID/PID for XInput pads on Windows Chromium/Firefox) |
| DualShock 4 / DualSense | Native evdev (`hid-playstation`, 3 nodes grouped) + hidraw read-only for raw data | Native protocol driver (C7) over HID | Native protocol driver over IOKit HID | Gamepad API; WebHID for full reports (Chromium) |
| Switch Pro / Joy-Cons | Native evdev (`hid-nintendo`) | Native protocol driver (C7): needs the handshake | Native protocol driver | Gamepad API (Chromium speaks the protocol itself); WebHID |
| Generic HID / DirectInput pads | Native evdev + profile (D1/D2) | Native Raw Input HID + profile | Native IOKit + profile | Gamepad API + per-browser profile |
| Pads with only an SDL HIDAPI driver (Steam Controller, Stadia, Luna, Shield, GameCube adapter, 8BitDo/GameSir/Flydigi vendor modes, ...) | SDL3 | SDL3 | SDL3 | not available (SDL3 does not target `wasm32-unknown-unknown`) |
| Anything else when the game cannot link C | gilrs | gilrs | gilrs | gilrs `wasm` backend is redundant with ours |

## Arbitration rules (C12)

1. **Ordered providers with claims**, as SDL does internally
   (`SDL_JoystickHandledByAnotherDriver`): each provider, in priority order,
   says whether it claims a device by identity (VID, PID, version, bus, name,
   OS device path). Lower-priority providers skip claimed devices. Default
   order: native protocol driver > SDL3 HIDAPI driver > native generic > SDL3
   generic > gilrs. Profiles may override per device.
2. **One physical device = one canonical device.** Group the nodes the OS
   splits a controller into (hid-playstation's gamepad/motion/touchpad,
   hid-nintendo's IMU, Windows' XInput slot + HID interface) before
   arbitration.
3. **Keep SDL away from our devices.** When the native provider owns a
   device, add its VID/PID to `SDL_HIDAPI_IGNORE_DEVICES` and
   `SDL_GAMECONTROLLER_IGNORE_DEVICES` before SDL opens it. Set
   `SDL_JOYSTICK_ENHANCED_REPORTS=auto` so SDL does not switch PlayStation pads
   into a mode that only a power cycle undoes and that breaks DirectInput for
   other apps. Disable SDL's own OS backends that duplicate ours where SDL
   would otherwise list the same pad twice.
4. **Never handshake twice.** On Linux, when `hid-nintendo` or
   `hid-playstation` is bound, do not send our own handshake/output reports
   over hidraw; read raw input only. In the browser, check what Chromium
   already does before sending WebHID handshakes.
5. **Steam Input.** Recognize Steam virtual pads (VID `0x28de`, the
   `SteamVirtualGamepadInfo` file) and either prefer the real device or map
   the virtual one back to it, so the same controller is not counted twice.
6. **Observe-only collection mode.** Collection mode (A3) opens every provider
   without claims and records each provider's view of the same device; the
   disagreements are report data (P1).

## Identity consequences (C2)

- Identity fields: bus, VID, PID, version, product name, OS, OS driver
  (`xpad`, `hid-playstation`, XUSB, HID, GameController.framework, ...),
  provider/backend, browser engine and version, `Gamepad.id`, `mapping`, HID
  report descriptor hash.
- VID/PID are optional: Windows XInput (all browsers and native XInput) and
  Safari's GameController devices hide them.
- Be able to compute the SDL GUID from these fields (GameControllerDB lookup,
  correlation with the SDL provider).
- Clones reuse genuine IDs (8BitDo SN30 Pro+ listed under Microsoft's VID in
  `hid-microsoft`), so VID/PID never decides a profile alone (C8).

## Canonical model consequences (C1)

- Positional face buttons (SDL3, Linux gamepad spec, gilrs, W3C standard
  mapping all agree) + label layer; Nintendo A = east.
- Slots SDL3 needed that the roadmap list already covers: share/capture/mic
  (SDL squeezes them into `MISC1`; give them distinct names), four paddles,
  touchpad click, gyro/accelerometer, touch points.
- Report calibrated gyro in physical units and support accumulation between
  polls (JoyShockLibrary).

## Data consequences (D1, D2)

- GameControllerDB covers Windows, Linux, macOS, Android and iOS per SDL
  backend; its indices are SDL backend element orders, so the importer must
  translate them to order-independent sources (evdev codes, HID usages).
- No dataset covers browser remapping; the web tester's reports are the only
  source, which makes the web app (M2) the right first data collector.

## Ecosystem consequence

SDL, GameControllerDB and the last DS4Windows maintainer reject AI-generated
contributions. anygamepad can import their data and learn from their code
(within licenses), but anything sent upstream must be written and submitted
by humans.
