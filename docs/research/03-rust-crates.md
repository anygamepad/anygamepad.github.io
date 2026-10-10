# Rust crates

Versions and download counts from the crates.io API on 2026-10-10.

## Input libraries

| Crate | Version | Downloads (total / 90 days) | License | Notes |
|-------|---------|-----------------------------|---------|-------|
| `gilrs` | 0.11.2 (2026-05-30) | 9.0M / 1.8M | Apache-2.0/MIT | See below |
| `sdl3` | 0.20.0 | 231k / 71k | MIT | Safe wrapper; features `build-from-source`, `build-from-source-static`, `static-link`, `use-pkg-config`, `use-vcpkg`, `hidapi`, `link-framework` |
| `sdl3-sys` | 0.7.2+SDL-3.4.18 | 295k / 111k | Zlib | Raw bindings; can build SDL from source and disable subsystems (`no-sdl-video`, `no-sdl-audio`, `sdl-joystick`, `sdl-hidapi`, `sdl-sensor`, `sdl-haptic`, ...) |

### gilrs

Source: `gitlab.com/gilrs-project/gilrs` at `65532a5` (2026-08-08).
MSRV 1.84 (`gilrs`), 1.80 (`gilrs-core`).

- Backends (`gilrs-core/src/platform/`): `linux` (evdev ioctls through `nix`,
  hotplug via `libudev-sys` and `inotify`), `macos` (IOKit through
  `objc2-io-kit`), `windows_wgi` (default feature `wgi`, Windows.Gaming.Input
  through `windows`), `windows_xinput` (feature `xinput`), `wasm` (Gamepad API
  through `web-sys`), `default` (no-op).
- Positional model: `Button::South/East/North/West` (values are the evdev
  `BTN_*` codes), plus `C`, `Z`, triggers, `Select/Start/Mode`, thumbs, D-pad.
- Embeds SDL GameControllerDB at build time (`include_str!` of
  `gamecontrollerdb.txt`) and reads `SDL_GAMECONTROLLERCONFIG`.
- Force feedback (rumble) and power info. No gyro, touchpad, LEDs or
  raw HID reports.

## Device access

| Crate | Version | Downloads | License | Notes |
|-------|---------|-----------|---------|-------|
| `hidapi` | 2.6.7 | 9.2M / 1.8M | MIT | Bindings to libusb/hidapi with backend features: `linux-native` (pure Rust hidraw), `linux-static-hidraw`, `linux-static-libusb`, `windows-native`, `macos-shared-device` (non-exclusive open on macOS) |
| `async-hid` | 0.5.3 | 58k | MIT | Async, pure Rust HID on Windows/Linux/macOS |
| `nusb` | 0.2.7 | 1.7M / 720k | Apache-2.0 OR MIT | Pure Rust USB (for devices that are not HID, e.g. raw GIP) |
| `evdev` | 0.13.2 (2025-09-15) | 1.9M / 715k | Apache-2.0 OR MIT | Pure Rust evdev, also creates uinput virtual devices |
| `udev` | 0.9.3 | 6.8M | MIT | libudev bindings (enumeration, hotplug monitor) |
| `input-linux` | 0.7.1 | 162k | MIT | evdev/uinput structs |
| `windows` | 0.62.2 | 357M | MIT OR Apache-2.0 | Official Microsoft bindings: `Gaming_Input`, `Devices_HumanInterfaceDevice`, Win32 HID, XInput, Raw Input |
| `objc2-io-kit` | 0.3.2 | 34M | Zlib OR Apache-2.0 OR MIT | IOKit / IOHIDManager |
| `objc2-game-controller` | 0.3.2 | 17k | Zlib OR Apache-2.0 OR MIT | Apple GameController framework |

No maintained Rust bindings exist for Microsoft GameInput (crates.io search
for "gameinput" finds nothing relevant). GameInput for PC ships as the
`Microsoft.GameInput` NuGet package with a redistributable installer
(`GameInputRedist.msi`, supports Windows 10 19H1+) that the app must bundle.

## Virtual controllers (desktop, later)

| Crate | Version | Notes |
|-------|---------|-------|
| `vigem-client` | 0.1.4 (2022) | Client for ViGEmBus, which is retired (see [04-desktop-prior-art.md](04-desktop-prior-art.md)) |
| `vigem-rust` | 0.2.0 (2026-08-20) | Newer, low adoption (651 downloads) |
| `evdev` / `input-linux` | | uinput on Linux |

## Implications for anygamepad

- Linux native provider (C4): `evdev` + `udev` for event devices, and hidraw
  (through `hidapi` with `linux-native`, or our own) for raw reports.
- Windows (C5): `windows` crate for Raw Input/HID, XInput and
  Windows.Gaming.Input; GameInput would need our own bindings plus a
  redistributable, so keep it as a later evaluation.
- macOS (C6): `objc2-io-kit`; `objc2-game-controller` for devices Apple
  claims.
- SDL3 provider (C13): `sdl3-sys` (zlib, can build SDL from source with only
  the joystick/HIDAPI/sensor/haptic subsystems). The `sdl3` wrapper is MIT and
  optional.
- gilrs provider (C14): `gilrs` with default features.
- Choose between `hidapi` and `async-hid` in F3 (sync vs async, C library vs
  pure Rust). The protocol drivers (C7) are sans-IO, so the choice stays
  local to the backends.
