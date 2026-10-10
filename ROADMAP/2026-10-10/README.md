# anygamepad roadmap — 2026-10-10

Status: **active** (first roadmap of the project).

## Vision

Gamepads lie about themselves. The same physical controller can expose a
different identity and a different input layout depending on:

- **Its mode**: many controllers switch between XInput, DirectInput, Nintendo
  Switch Pro Controller, DualShock 4/DualSense emulation, vendor modes
  (GuliKit, 8BitDo, ...), each with a different VID/PID and HID report layout.
- **The host OS**: Windows (XInput/HID/GameInput), Linux (evdev nodes created by
  `xpad`, `hid-playstation`, `hid-nintendo`, generic HID), and macOS (IOKit HID,
  GameController framework) each expose the device differently.
- **The transport**: USB, Bluetooth Classic and BLE often produce different
  report formats for the same device.
- **The browser**: the Gamepad API applies its own per-browser remapping, so
  Chromium, Firefox and Safari can disagree about the same pad on the same OS.

anygamepad normalizes all of this into **one canonical gamepad model**, and builds
a data loop to keep improving detection over time.

## Deliverables

| # | Deliverable | Detail |
|---|-------------|--------|
| 1 | **Rust library** (`anygamepad` crate) that Rust games embed to get normalized gamepad input, isolating them from device/mode/OS quirks. | [01-core-library.md](01-core-library.md), [02-device-database.md](02-device-database.md) |
| 2 | **Desktop app** (Windows, Linux, macOS) that detects controllers and exposes a **virtual controller** of a selectable type (Xbox 360, DualShock 4, DualSense, Switch Pro Controller, ...), similar to DS4Windows but multi-target and multi-OS. Performance-first native UI with an **always-on-top widget**. | [05-desktop-app.md](05-desktop-app.md) |
| 3 | **Web app** similar to hardwaretester.com/gamepad, built on the **anygamepad library compiled to WebAssembly**, backed by a database that **collects reports of detected controllers**. Published on GitHub Pages at <https://anygamepad.github.io>. | [04-web-app.md](04-web-app.md), [03-collection-pipeline.md](03-collection-pipeline.md) |
| 4 | **Collection mode** in the desktop app, which gathers richer reports than the browser can (raw HID descriptors, OS driver info). | [05-desktop-app.md](05-desktop-app.md), [03-collection-pipeline.md](03-collection-pipeline.md) |

Cross-cutting groundwork lives in [00-foundations.md](00-foundations.md).

## Priorities

1. **Web app first.** The web tester (milestone M2) is the top priority: it
   starts the data collection loop and is the first real consumer of the
   library (through WebAssembly). Library work the web needs (C1, C2, C3, C11,
   D1, D2, D4) and the collection pipeline come first.
2. **Library 0.1.0** (M3): native providers (C4-C6, C13, C14) and protocol
   drivers progress in parallel, since the library is a deliverable on its own.
3. **Desktop app** (M4): its first tasks (A1, A4) are blocked by M2, so desktop
   work starts once the web tester is live, and reuses what the web built.

## Input provider strategy

To support as many gamepads as possible, the library does not pick a single
input stack: it aggregates several **providers** and arbitrates between them
per physical device (C12).

| Provider | Platforms | Why |
|----------|-----------|-----|
| Native, own (C4-C7) | Linux, Windows, macOS | Full control: modes, clones, own profiles, protocol drivers. |
| Web, own (C11) | Browser (WASM) | Gamepad API + WebHID; SDL3 does not target `wasm32-unknown-unknown`. |
| SDL3 (C13) | Linux, Windows, macOS | Widest existing coverage (SDL HIDAPI drivers + GameControllerDB). |
| gilrs (C14) | Linux, Windows, macOS | Pure-Rust fallback for games that cannot link C libraries. |

Each provider is a Cargo feature. Only one provider owns a given physical
device at a time (no duplicates, no competing handshakes); profiles can state
which provider handles a device best. In collection mode, every provider's view
of the device is recorded, so disagreements between them become data.

## The data loop

```mermaid
flowchart LR
    U[Users] -->|test controller| WEB[Web tester]
    U -->|collection mode| DESK[Desktop app]
    WEB -->|device report| API[Ingestion API]
    DESK -->|device report| API
    API --> DB[(Reports DB)]
    DB --> CUR[Curation tooling]
    CUR -->|reviewed profile PRs| PROF[Device profile database]
    PROF -->|embedded in| LIB[anygamepad library]
    LIB --> DESK
    LIB -->|WASM build| WEB
    LIB --> GAMES[Rust games]
```

Every iteration of the project should feed collected reports back into the
curated profile database, so detection gets better with each release.

## Planning assumptions

- Schedule starts on **2026-10-12**; durations are **calendar days** of focused
  work per task.
- Core/database/collection and web workstreams are assumed to progress **in
  parallel**; the desktop workstream starts after M2 (see Priorities). With a
  single contributor the dates stretch, but the dependency graph (what blocks
  what) stays valid. Use the graph, not the dates, to decide what to pick up
  next.
- The critical path runs through the web launch (M2) and ends at milestone M4
  (desktop 0.1.0). Projected end: **2027-03-16**.

## Master schedule

Task IDs are referenced by every detailed roadmap file. `after X Y` means the
task is **blocked** until X and Y are done. Critical-path tasks are marked
`crit`.

```mermaid
gantt
    dateFormat YYYY-MM-DD
    axisFormat %b %d
    title anygamepad master schedule (critical path in red)
    section Foundations
    F1 Cargo workspace, licensing, CI matrix :f1, 2026-10-12, 5d
    F2 Prior-art research spike :crit, f2, 2026-10-12, 10d
    F3 Architecture decision records :crit, f3, after f2, 5d
    F4 Hardware test lab inventory :f4, after f2, 5d
    section Core library
    C1 Canonical gamepad model :crit, c1, after f1 f3, 10d
    C2 Device identity, fingerprint and provider trait :crit, c2, after f3, 10d
    C4 Linux input backend (evdev + hidraw) :c4, after c1 c2, 20d
    C5 Windows input backend (HID + XInput) :c5, after c1 c2, 30d
    C6 macOS input backend (IOKit HID) :c6, after c1 c2, 25d
    C11 Web backend (Gamepad API + WebHID) and JS bindings :crit, c11, after c1 c2, 20d
    C13 SDL3 provider (incl. SDL HIDAPI drivers) :c13, after c1 c2, 15d
    C14 gilrs provider (pure-Rust fallback) :c14, after c1 c2, 10d
    C3 Mapping engine :c3, after c1 d1, 20d
    C7 Native protocol drivers (DualSense, DS4, Switch Pro) :c7, after c4, 30d
    C12 Multi-provider arbitration (dedup, priority) :c12, after c4 c13, 15d
    C9 Public API, docs and examples :c9, after c3, 15d
    C8 Mode and clone detection heuristics :c8, after c3 c4 d3, 20d
    C10 Library 0.1.0 release :c10, after c5 c6 c7 c8 c9 c11 c12 c14 d4, 5d
    section Device database
    D1 Device profile format and JSON Schema :d1, after c2, 10d
    D2 SDL GameControllerDB importer :d2, after d1, 7d
    D3 Initial curated profiles (top devices) :d3, after d1 c4 f4, 20d
    D4 Profile embedding and runtime overrides :d4, after d1 c3, 7d
    section Collection pipeline
    P1 Device report schema (shared crate) :p1, after c2 d1, 7d
    P2 Ingestion API and PostgreSQL storage :p2, after p1, 20d
    P3 Privacy, consent, data retention policy :p3, after p1, 10d
    P4 Curation tooling (reports to profile PRs) :p4, after p2 d1, 20d
    P5 Hosting, backups, monitoring :p5, after p2, 10d
    section Web app
    W1 Web app scaffolding and WASM build pipeline :w1, after f1 f3, 7d
    W2 Live tester on the core web backend :crit, w2, after w1 c11, 15d
    W3 WebHID raw mode (descriptors, raw reports) :w3, after w2, 10d
    W4 Guided mapping wizard (shared with desktop) :crit, w4, after w2, 15d
    W5 Normalized view (mapping engine + profiles) :w5, after w2 c3 d2 d4, 10d
    W6 Report submission and consent flow :crit, w6, after w4 p1 p2 p3, 10d
    W7 Public launch of the web tester :crit, w7, after w5 w6 p5, 5d
    section Desktop app
    A1 App shell (UI toolkit, tray, settings) :crit, a1, after f3 m2, 15d
    A4 Virtual pad abstraction and emulated profiles :crit, a4, after c1 m2, 10d
    A5 Linux virtual output (uinput + UHID) :crit, a5, after a4 c4, 20d
    A7 Windows virtual output via ViGEmBus (X360, DS4) :a7, after a4 c5, 15d
    A8 macOS virtual output (IOHIDUserDevice) :a8, after a4 c6, 25d
    A2 Device list and live view :crit, a2, after a1 c4, 15d
    A3 Collection mode :crit, a3, after a2 w4 p2 p3, 15d
    A6 Physical device hiding (grab/HidHide/seize) :crit, a6, after a5 c5 c6, 15d
    A11 Always-on-top input widget (overlay) :a11, after a2, 10d
    A9 Packaging and code signing :crit, a9, after a3 a6 a7 a8 a11, 15d
    A10 Desktop app 0.1.0 release :crit, a10, after a9 c10, 5d
    section Milestones
    M1 Core runs in the browser :milestone, m1, after c3 c11 d4, 0d
    M2 Web tester live and collecting :milestone, crit, m2, after w7, 0d
    M3 Library 0.1.0 :milestone, m3, after c10, 0d
    M4 Desktop app 0.1.0 :milestone, crit, m4, after a10, 0d
```

### Critical path

Zero-slack chains (they share most tasks):

1. F2 -> F3 -> C1 -> C11 -> W2 -> W4 -> W6 -> W7 -> M2 -> A1 -> A2 -> A3 -> A9 -> A10 -> M4
2. F2 -> F3 -> C1 -> C11 -> W2 -> W4 -> W6 -> W7 -> M2 -> A4 -> A5 -> A6 -> A9 -> A10 -> M4
3. F2 -> F3 -> C2 -> C11 -> W2 -> W4 -> W6 -> W7 -> M2 -> A1 -> A2 -> A3 -> A9 -> A10 -> M4
4. F2 -> F3 -> C2 -> C11 -> W2 -> W4 -> W6 -> W7 -> M2 -> A4 -> A5 -> A6 -> A9 -> A10 -> M4

Any delay on these tasks delays the web launch (M2) and therefore the desktop
0.1.0 release. Everything else has slack (see the `Slack` column in each
detailed file).

## Dependency graph (who blocks whom)

An arrow `X --> Y` means **X blocks Y**. Red nodes are on the critical path.

```mermaid
flowchart LR
    subgraph foundations[Foundations]
        f1[F1]
        f2[F2]
        f3[F3]
        f4[F4]
    end
    subgraph core[Core library]
        c1[C1]
        c2[C2]
        c3[C3]
        c4[C4]
        c5[C5]
        c6[C6]
        c7[C7]
        c8[C8]
        c9[C9]
        c11[C11]
        c13[C13]
        c12[C12]
        c14[C14]
        c10[C10]
    end
    subgraph database[Device database]
        d1[D1]
        d3[D3]
        d4[D4]
        d2[D2]
    end
    subgraph collection[Collection pipeline]
        p1[P1]
        p2[P2]
        p3[P3]
        p4[P4]
        p5[P5]
    end
    subgraph web[Web app]
        w1[W1]
        w2[W2]
        w3[W3]
        w4[W4]
        w5[W5]
        w6[W6]
        w7[W7]
    end
    subgraph desktop[Desktop app]
        a1[A1]
        a2[A2]
        a3[A3]
        a4[A4]
        a5[A5]
        a6[A6]
        a7[A7]
        a8[A8]
        a11[A11]
        a9[A9]
        a10[A10]
    end
    subgraph milestones[Milestones]
        m2{{M2}}
        m1{{M1}}
        m3{{M3}}
        m4{{M4}}
    end
    f2 --> f3
    f2 --> f4
    f1 --> c1
    f3 --> c1
    f3 --> c2
    c2 --> d1
    c1 --> c3
    d1 --> c3
    c1 --> c4
    c2 --> c4
    c1 --> c5
    c2 --> c5
    c1 --> c6
    c2 --> c6
    c4 --> c7
    d1 --> d3
    c4 --> d3
    f4 --> d3
    c3 --> c8
    c4 --> c8
    d3 --> c8
    c3 --> c9
    c1 --> c11
    c2 --> c11
    c1 --> c13
    c2 --> c13
    c4 --> c12
    c13 --> c12
    c1 --> c14
    c2 --> c14
    d1 --> d4
    c3 --> d4
    c5 --> c10
    c6 --> c10
    c7 --> c10
    c8 --> c10
    c9 --> c10
    c11 --> c10
    c12 --> c10
    c14 --> c10
    d4 --> c10
    d1 --> d2
    c2 --> p1
    d1 --> p1
    p1 --> p2
    p1 --> p3
    p2 --> p4
    d1 --> p4
    p2 --> p5
    f1 --> w1
    f3 --> w1
    w1 --> w2
    c11 --> w2
    w2 --> w3
    w2 --> w4
    w2 --> w5
    c3 --> w5
    d2 --> w5
    d4 --> w5
    w4 --> w6
    p1 --> w6
    p2 --> w6
    p3 --> w6
    w5 --> w7
    w6 --> w7
    p5 --> w7
    w7 --> m2
    f3 --> a1
    m2 --> a1
    a1 --> a2
    c4 --> a2
    a2 --> a3
    w4 --> a3
    p2 --> a3
    p3 --> a3
    c1 --> a4
    m2 --> a4
    a4 --> a5
    c4 --> a5
    a5 --> a6
    c5 --> a6
    c6 --> a6
    a4 --> a7
    c5 --> a7
    a4 --> a8
    c6 --> a8
    a2 --> a11
    a3 --> a9
    a6 --> a9
    a7 --> a9
    a8 --> a9
    a11 --> a9
    a9 --> a10
    c10 --> a10
    c3 --> m1
    c11 --> m1
    d4 --> m1
    c10 --> m3
    a10 --> m4
    classDef crit fill:#fdd,stroke:#c00,stroke-width:2px
    class f2,f3,c1,c2,c11,w2,w4,w6,w7,m2,a1,a2,a3,a4,a5,a6,a9,a10,m4 crit
```

## Milestones

| Milestone | Blocked by | Target date |
|-----------|------------|-------------|
| M1 Core runs in the browser | C3, C11, D4 | 2026-12-13 |
| M2 Web tester live and collecting | W7 | 2027-01-10 |
| M3 Library 0.1.0 | C10 | 2027-01-10 |
| M4 Desktop app 0.1.0 | A10 | 2027-03-16 |

## Top risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Windows virtual controller drivers: ViGEmBus is retired (no longer maintained) and only emulates Xbox 360 and DualShock 4. A custom driver needs kernel/UMDF development plus EV signing and Microsoft attestation. | No DualSense/Switch Pro emulation on Windows in 0.1. | Ship ViGEmBus-based output first (A7); investigate an own VHF/UMDF driver after 0.1 (see "Beyond 0.1"). |
| macOS virtual HID devices require the restricted `com.apple.developer.hid.virtual.device` entitlement (or DriverKit entitlements). | macOS virtual output may be blocked by Apple approval. | Request the entitlement as soon as A9 starts; ship macOS as input/collection-only if not granted. |
| Clone controllers that impersonate genuine VID/PIDs (e.g. a third-party pad in Switch mode reporting as `057e:2009`). | Wrong profile, wrong calibration, failed handshakes. | Fingerprinting beyond VID/PID (C8), community reports. |
| Privacy of collected data (serial numbers, Bluetooth MAC addresses, IPs). | Legal exposure (GDPR), loss of trust. | Strip identifiers client-side and server-side, explicit consent, retention policy (P3). |
| WebAssembly constraints: Gamepad API is poll-only (must be driven from `requestAnimationFrame`), WebHID is Chromium-only, browsers remap pads differently, and embedding profiles grows the `.wasm` bundle. | Web tester behaves differently from native, slow page load. | Browser is an identity dimension in profiles (C2, D1); bundle size budget in CI (W1); WebHID features are opt-in. |
| Hardware availability for testing many controllers on three OSes. | Profiles verified only on one OS. | Hardware lab inventory (F4) and crowd-sourced reports. |
| Project name: `rustpad` is taken on crates.io and `omnipad` is taken on GitHub, so the project and library are named `anygamepad` (free on crates.io and npm on 2026-10-10). | Someone else takes `anygamepad` before the first release. | Publish placeholder releases early (F1). |
| GitHub Pages only serves static files and does not allow custom HTTP headers (no COOP/COEP, so no `SharedArrayBuffer` or WASM threads; CSP only through a `<meta>` tag). | The ingestion API needs its own host; some WASM features unavailable. | Keep the web app single-threaded; host the API separately with CORS limited to `https://anygamepad.github.io` (P5). |
| Always-on-top widget (A11): Wayland has no standard always-on-top for regular windows, and exclusive full-screen games cover every window on all OSes. | Widget hidden in some setups. | Layer-shell where available, documented fallbacks; recommend borderless windowed mode in games. |
| Multiple providers (native, SDL3, gilrs) opening the same device: duplicates, exclusive-access conflicts, competing handshakes (e.g. two drivers configuring a Switch Pro Controller). SDL3 is also a C dependency (build and cross-compilation complexity). | Duplicate or broken input. | Arbitration layer (C12) lets one provider own each device; SDL hints disable SDL HIDAPI drivers for devices a native driver owns; SDL3 provider is an optional feature. |

## Beyond 0.1 (not scheduled yet)

- Own Windows virtual HID driver (VHF/UMDF2) to emulate DualSense and Switch Pro
  Controller; signing and attestation pipeline.
- Second data-loop iteration: use collected reports to extend the curated
  profiles and re-release the library.
- Remapping UI, per-game profiles, macros, gyro-to-mouse/stick in the desktop app.
- Bindings for other languages (C ABI, then C#/Godot/Unity) once the Rust API is
  stable.
- Steam Deck / handheld built-in controller support.

## Files in this roadmap

- [00-foundations.md](00-foundations.md)
- [01-core-library.md](01-core-library.md)
- [02-device-database.md](02-device-database.md)
- [03-collection-pipeline.md](03-collection-pipeline.md)
- [04-web-app.md](04-web-app.md)
- [05-desktop-app.md](05-desktop-app.md)
