# Research

Prior-art research for roadmap task F2 (ROADMAP/2026-10-10). Findings come
from reading the projects' source code at the commits named in each note,
from crates.io metadata, and, where no source was available, from web pages
(marked as such). Unverified claims are labelled.

| Note | Topic |
|------|-------|
| [01-sdl3.md](01-sdl3.md) | SDL3 gamepad model, GUID format, backend priority, HIDAPI drivers, hints |
| [02-sdl-gamecontrollerdb.md](02-sdl-gamecontrollerdb.md) | Mapping database format, coverage, license, import constraints |
| [03-rust-crates.md](03-rust-crates.md) | gilrs, hidapi, evdev, windows, objc2, sdl3 and related crates |
| [04-desktop-prior-art.md](04-desktop-prior-art.md) | DS4Windows, ViGEmBus, HidHide, Steam Input, JoyShockLibrary |
| [05-linux-kernel-drivers.md](05-linux-kernel-drivers.md) | xpad, hid-playstation, hid-nintendo, hid-microsoft, Linux gamepad spec |
| [06-browser-gamepad-api.md](06-browser-gamepad-api.md) | `Gamepad.id` formats, standard mapping, WebHID per browser |
| [07-rust-wasm-tooling.md](07-rust-wasm-tooling.md) | wasm-bindgen, web-sys, wasm-pack, Trunk, tsify |
| [08-provider-combination.md](08-provider-combination.md) | **Output:** how to combine native, web, SDL3 and gilrs providers |
| [09-dualsense-hid-protocol.md](09-dualsense-hid-protocol.md) | DualSense/DualShock 4 reports, CRC32, feature reports, from a WebHID tester (added 2026-10-14, for C7 and W3) |

## Key findings

1. Everyone converged on **positional** face buttons (SDL3, Linux gamepad
   spec, gilrs, W3C standard mapping). The roadmap's C1 direction holds.
2. SDL already solves provider arbitration with an **ordered driver list and
   identity claims**; C12 can follow the same pattern across our providers.
3. A device's identity changes with bus, OS backend, driver and firmware
   version (visible in SDL's GUID), and **VID/PID is sometimes missing**
   (XInput, Safari GameController devices) or **borrowed** (clones).
4. The Linux kernel already normalizes the big families, but splits one
   controller into several evdev nodes, and module parameters change the
   layout of unknown pads.
5. Browsers disagree on `Gamepad.id` format, on which pads get the
   `"standard"` mapping, and on WebHID (Chromium only). No public dataset
   covers browser remapping: the web tester's reports fill that gap.
6. SDL's `SDL_JOYSTICK_ENHANCED_REPORTS` default changes PlayStation pads'
   report mode irreversibly until power-cycle; the SDL provider must default it
   to `auto`.
7. ViGEmBus is retired; HidHide is maintained.
8. SDL, GameControllerDB and DS4Windows maintainers reject AI-generated
   contributions; upstream contributions must be human-authored.
