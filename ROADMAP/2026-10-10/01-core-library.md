# 01 — Core library

The `rustpad` crate: what Rust games import. It hides devices, modes, transports
and OS differences behind one canonical gamepad model.

## Schedule

Upstream blockers from other files are shown as milestones on the date they are
expected to be done.

```mermaid
gantt
    dateFormat YYYY-MM-DD
    axisFormat %b %d
    title Core library (critical path in red)
    section Upstream blockers
    F1 done :milestone, f1, 2026-10-17, 0d
    F3 done :milestone, crit, f3, 2026-10-27, 0d
    D1 done :milestone, d1, 2026-11-13, 0d
    D4 done :milestone, d4, 2026-12-10, 0d
    D3 done :milestone, d3, 2026-12-16, 0d
    section Core library
    C1 Canonical gamepad model :crit, c1, after f1 f3, 10d
    C2 Device identity and fingerprint model :c2, after f3, 7d
    C4 Linux input backend (evdev + hidraw) :crit, c4, after c1 c2, 20d
    C5 Windows input backend (HID + XInput) :c5, after c1 c2, 30d
    C6 macOS input backend (IOKit HID) :c6, after c1 c2, 25d
    C3 Mapping engine :c3, after c1 d1, 20d
    C7 Native protocol drivers (DualSense, DS4, Switch Pro) :c7, after c4, 30d
    C9 Public API, docs, examples, WASM target :c9, after c3, 15d
    C8 Mode and clone detection heuristics :c8, after c3 c4 d3, 20d
    C10 Library 0.1.0 release :c10, after c5 c6 c7 c8 c9 d4, 5d
```

## Tasks

| ID | Task | Days | Blocked by | Blocks | Start | End | Slack |
|----|------|-----:|------------|--------|-------|-----|------:|
| C1 | **Canonical gamepad model** | 10 | F1, F3 | C3, C4, C5, C6, W3, A4 | 2026-10-27 | 2026-11-06 | 0 |
| C2 | Device identity and fingerprint model | 7 | F3 | C4, C5, C6, D1, P1 | 2026-10-27 | 2026-11-03 | 3 |
| C3 | Mapping engine | 20 | C1, D1 | C8, C9, D4, M1 | 2026-11-13 | 2026-12-03 | 18 |
| C4 | **Linux input backend (evdev + hidraw)** | 20 | C1, C2 | C7, C8, D3, A2, A5, M1 | 2026-11-06 | 2026-11-26 | 0 |
| C5 | Windows input backend (HID + XInput) | 30 | C1, C2 | C10, A6, A7 | 2026-11-06 | 2026-12-06 | 10 |
| C6 | macOS input backend (IOKit HID) | 25 | C1, C2 | C10, A6, A8 | 2026-11-06 | 2026-12-01 | 5 |
| C7 | Native protocol drivers (DualSense, DS4, Switch Pro) | 30 | C4 | C10 | 2026-11-26 | 2026-12-26 | 15 |
| C8 | Mode and clone detection heuristics | 20 | C3, C4, D3 | C10 | 2026-12-16 | 2027-01-05 | 5 |
| C9 | Public API, docs, examples, WASM target | 15 | C3 | C10, W4 | 2026-12-03 | 2026-12-18 | 23 |
| C10 | Library 0.1.0 release | 5 | C5, C6, C7, C8, C9, D4 | A10, M3 | 2027-01-05 | 2027-01-10 | 5 |

### C1 — Canonical gamepad model

- **Positional** face buttons (`South`, `East`, `West`, `North`) so game logic
  does not depend on the label printed on the button; plus a **label/glyph**
  layer (`A/B/X/Y`, `Cross/Circle/Square/Triangle`, Nintendo `B/A/Y/X`) for UI
  prompts.
- D-pad, shoulders, triggers (analog + digital), sticks and stick clicks,
  `Start/Select/Home/Share/Capture`, back paddles, touchpad (points + click),
  motion sensors (gyro/accelerometer in SI units), battery, connection type.
- Output capabilities: rumble (including dual-motor and HD rumble as optional
  capabilities), lightbar/player LEDs, adaptive triggers (optional).
- Every feature is described by a capability set, so games can query what a
  device supports.

### C2 — Device identity and fingerprint model

Identity is more than VID/PID: bus type (USB, Bluetooth, BLE), version, product
string, HID report descriptor hash, OS, OS driver that owns the device
(`xpad`, `hid-playstation`, XUSB, HID, ...), and detected mode. Shared with the
device database (D1) and the report schema (P1).

### C3 — Mapping engine

Applies a device profile to raw elements: button remap, axis
inversion/range/center calibration, combined axes (e.g. Xbox triggers on a
single HID axis in DirectInput mode), hat-to-dpad, dead zones. Pure and
deterministic so it can be unit-tested and compiled to WASM (C9).

### C4 — Linux input backend

`evdev` for devices bound to kernel drivers and `hidraw` for devices we drive
ourselves; hotplug via udev. Document required permissions (udev rules).

### C5 — Windows input backend

Raw Input / HID for generic devices, XInput for XUSB devices (note: XInput does
not expose VID/PID, correlate with the HID/PnP device tree). Evaluate
GameInput as a future replacement.

### C6 — macOS input backend

IOKit `IOHIDManager` for HID devices; evaluate the GameController framework
for devices Apple claims exclusively.

### C7 — Native protocol drivers

Talk directly to devices over HID to unlock features hidden by generic
drivers:

- DualSense / DualShock 4: full Bluetooth reports (these pads start with a
  reduced report until a calibration feature report is read), IMU calibration,
  lightbar, rumble.
- Switch Pro Controller: handshake and subcommands to enable full report mode
  and IMU, player LEDs, rumble.

### C8 — Mode and clone detection heuristics

Decide which profile applies when VID/PID is ambiguous: report descriptor
fingerprints, feature-report probing, product strings, per-OS quirks. Built on
the initial curated profiles (D3).

### C9 — Public API, docs, examples, WASM target

Polling and event APIs, hotplug events, capability queries, examples (minimal
game loop, Bevy integration example), and a WASM build of the model + mapping
engine for the web app (W4).

### C10 — Library 0.1.0 release

Publish to crates.io; semver policy; changelog.

**Done when:** a sample game reads the same canonical input from at least one
Xbox, one PlayStation and one Switch-style controller on Linux, Windows and
macOS.
