# Desktop prior art: DS4Windows, ViGEmBus, HidHide, Steam Input, JoyShockLibrary

## DS4Windows

- The original (`Jays2Kings/DS4Windows`) was archived in 2021.
  `Ryochan7/DS4Windows` could not be cloned on 2026-10-10 (repository not
  publicly reachable); third-party pages describe it as archived.
- `schmaldeo/DS4Windows` (GPL-3.0, last commit 2026-03-05) now opens with a
  **discontinuation** notice. Its author cites lack of time and low-quality
  AI-generated contributions flooding open source.
- What it does: reads DualShock 4 / DualSense over HID, exposes a virtual
  Xbox 360 or DualShock 4 controller through ViGEmBus, and hides the real
  device from games through HidHide so they do not see double input.
- License: GPL-3.0. Do not copy code; use it only as a feature reference.

## ViGEmBus

- `nefarius/ViGEmBus` (BSD-3-Clause), last commit 2023-09-08. The README says
  "THIS PROJECT HAS BEEN RETIRED"; the end-of-life statement
  (docs.nefarius.at) links the retirement to a trademark conflict and says
  existing installations keep working.
- Kernel-mode bus driver that emulates **Xbox 360 and DualShock 4** USB
  controllers only.
- A successor named "VirtualPad" is mentioned by third parties; not
  verified.

## HidHide

- `nefarius/HidHide` (MIT), last commit 2026-07-05, still maintained.
- "Gaming Input Peripherals Device Firewall for Windows": a filter driver that
  hides chosen HID devices from all processes except an allow-list, so a
  remapper can read the real pad while games see only the virtual one.

## Steam Input

- Closed source. Observable effects that matter to us:
  - Steam can expose its own virtual gamepads (Valve VID `0x28de`) in place
    of the real controller. SDL maps them back through the file named by the
    `SteamVirtualGamepadInfo` environment variable (VID, PID, type and handle
    of the real pad).
  - `ValveSoftware/steam-devices` ships `60-steam-input.rules`: 101 udev
    rules that give the logged-in user (`TAG+="uaccess"`) access to
    `hidraw` and `uinput` for Valve, Sony, Nintendo, PDP, Razer, HORI, ... pads.
    Many Linux users already have them; others need equivalent rules for our
    hidraw access.

## JoyShockLibrary

- `JibbSmart/JoyShockLibrary` (MIT), last commit 2026-07-13. C++ over HIDAPI.
- Supports DualShock 4, DualSense, Joy-Cons (as pairs) and Switch Pro
  Controller. Features: gyro in degrees per second with continuous and
  automatic calibration (GamepadMotionHelpers), accumulated gyro between polls,
  touchpad, lightbar colour, player LEDs, rumble.
- Reference for C7 (protocol drivers) and for the motion part of C1 (gyro in
  physical units, calibration state exposed to the app).

## Implications for anygamepad

- Desktop virtual output (A7): ViGEmBus is retired and limited to Xbox 360 and
  DualShock 4, as the roadmap already states. HidHide is maintained and is the
  standard way to avoid double input on Windows.
- Linux (A8): uinput for virtual devices; Steam's udev rules show what
  permissions users need.
- Detect Steam virtual pads (VID `0x28de`, `SteamVirtualGamepadInfo`) so the
  same physical pad is not counted twice and reports record the real device.
- Gyro API: expose calibrated angular velocity, plus accumulated motion between
  polls (JoyShockLibrary's lesson for mouse-like aiming).
- Several maintainers of this ecosystem (DS4Windows, SDL, GameControllerDB)
  are openly hostile to AI-generated contributions. Upstream interaction must
  be human-driven.
