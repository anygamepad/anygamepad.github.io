# 00 — Foundations

Groundwork that unblocks every other workstream. See the
[master schedule](README.md#master-schedule) for the global view.

## Schedule

```mermaid
gantt
    dateFormat YYYY-MM-DD
    axisFormat %b %d
    title Foundations (critical path in red, in progress in blue, done in grey)
    section Upstream blockers
    H2 done :milestone, h2, 2026-10-31, 0d
    section Foundations
    F1 Cargo workspace, licensing, CI matrix :done, f1, 2026-10-10, 1d
    F2 Prior-art research spike :done, f2, 2026-10-10, 1d
    F3 Architecture decision records :done, f3, 2026-10-10, 1d
    F4 Hardware test lab inventory :f4, after f2 h2, 3d
```

## Tasks

| ID | Task | Days | Blocked by | Blocks | Start | End | Slack |
|----|------|-----:|------------|--------|-------|-----|------:|
| F1 | Cargo workspace, licensing, CI matrix | 5 | - | C1, W1 | 2026-10-10 | 2026-10-10 | done |
| F2 | Prior-art research spike | 10 | - | F3, F4 | 2026-10-10 | 2026-10-10 | done |
| F3 | Architecture decision records | 5 | F2 | C1, C2, W1, A1 | 2026-10-10 | 2026-10-10 | done |
| F4 | Hardware test lab inventory | 3 | F2, H2 | D3, H6, H8 | 2026-10-31 | 2026-11-03 | 27 |

### F1 — Cargo workspace, licensing, CI matrix

- Create the Cargo workspace with the proposed layout (final layout confirmed by
  F3):

  ```text
  crates/
    anygamepad/          # public facade crate used by games
    anygamepad-model/    # canonical gamepad model types
    anygamepad-db/       # device profile format and embedded database
    anygamepad-report/   # device report schema shared by apps and server
    anygamepad-virtual/  # virtual controller outputs (desktop app only)
    anygamepad-wasm/     # wasm-bindgen JS/TS bindings used by the web app
  apps/
    desktop/
    web/
    server/
  data/
    profiles/            # curated device profiles
  docs/adr/              # architecture decision records
  ```

  The project, repository and crates are all named anygamepad (`rustpad` is
  taken on crates.io, `omnipad` on GitHub).

- Choose and add the license (proposal: `MIT OR Apache-2.0`, the Rust
  ecosystem default; compatible with importing the zlib-licensed SDL
  GameControllerDB).
- CI on Linux, Windows and macOS: `cargo fmt --check`, `cargo clippy
  -- -D warnings`, `cargo test`, plus a `wasm32-unknown-unknown` build and
  `wasm-bindgen-test` tests in a headless browser.
- Reserve the names early with placeholder releases: `anygamepad` on crates.io
  and on npm (for the WASM package).

**Done when:** an empty workspace builds and passes CI on the three OSes.

### F2 — Prior-art research spike

Study and summarize (in `docs/research/`):

- SDL3 gamepad subsystem: positional button naming (`SOUTH/EAST/WEST/NORTH`),
  GUID format, HIDAPI drivers for PlayStation/Nintendo/Xbox controllers.
- SDL GameControllerDB (community mapping database).
- Rust crates: `gilrs`, `hidapi`, `evdev`, `windows`, `objc2`/IOKit bindings,
  `sdl3` bindings.
- DS4Windows, ViGEmBus, HidHide, Steam Input, JoyShockLibrary.
- Linux kernel drivers `xpad`, `hid-playstation`, `hid-nintendo`,
  `hid-microsoft`, and how they rename/remap axes.
- Browser Gamepad API differences (Chromium, Firefox, Safari): `id` string
  format, `"standard"` mapping availability, WebHID support.
- Rust to WebAssembly tooling: `wasm-bindgen`, `web-sys` Gamepad and WebHID
  bindings, `wasm-pack`/Trunk, TypeScript type generation.

Output: how to combine the providers (native, SDL3, gilrs, web): which devices
each handles best, how to avoid two providers opening the same device, SDL
hints needed to coexist with native drivers.

### F3 — Architecture decision records

Write ADRs in `docs/adr/NNNN-title.md` for:

1. Canonical model naming (positional vs label-based buttons).
2. Input provider strategy. Direction agreed: aggregate native, web, SDL3 and
   gilrs providers with per-device arbitration (see the
   [overview](README.md#input-provider-strategy)); the ADR fixes priorities
   and Cargo features.
3. Device profile file format (TOML vs JSON) and schema tooling.
4. Desktop UI toolkit. Requirements: **performance first** (low input-to-pixel
   latency, low idle CPU and memory) and an **always-on-top widget** (A11).
   Recommendation: **egui** (`eframe` with the `wgpu` renderer): pure Rust in
   the same process as the library (no IPC, no webview), immediate mode suited
   to live input at high refresh rates, multiple native windows (viewports),
   and per-window always-on-top, transparency, no decorations and mouse
   pass-through through winit. Alternatives: iced (similar, less mature
   multi-window), Slint (good performance, but its licensing options need
   review), Tauri (rejected: webview memory/CPU cost, and transparent overlay
   windows are fragile on Linux). Validate with a small prototype measuring
   idle CPU, memory and latency on the three OSes before accepting the ADR.
5. Web stack and server stack. Already decided: the web app uses the anygamepad
   library compiled to WebAssembly. To decide: frontend framework and bundler
   (an egui gamepad visualization could also be embedded in the page as a WASM
   canvas to share it with the desktop app); server proposal is Rust
   `axum` + `sqlx` + PostgreSQL.

**Done when:** ADRs are merged with status `accepted`.

Status (2026-10-10): ADRs 0001, 0002, 0003 and 0005 accepted, which unblocks
C1, C2 and W1. By owner decision, ADR 0004 (desktop UI) stays `proposed`
until its prototype meets the targets, before A1 starts; the server stack
was split into ADR 0006, accepted the same day.

### F4 — Hardware test lab inventory

Re-planned on 2026-10-31 ([06-hardware-testing.md](06-hardware-testing.md)):
blocked by H2, which defines the formats, and down from 5 to 3 days.

The owner's controllers (several DualSense, Xbox Series, DualShock 4,
DualShock 3, 8BitDo, GuliKit, GameSir and other brands) listed as units in
`data/lab/inventory.toml`, each pointing at a catalog model in
`data/controllers/` (added for every model that is not there yet, with its
modes and transports), and the machines available to test them (OS and
version, browsers). The test modes (H4, H5) offer these units, and the
coverage matrix (H8) shows which of their configurations are untested.
It feeds the first test campaign (H6) and the curated profiles (D3).

**Done when:** every controller the owner wants to test is in the
inventory, the catalog entries validate against H2's schema, and the
machines are listed.
