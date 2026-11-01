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
    H4 done :milestone, h4, 2026-11-01, 0d
    H5 done :milestone, h5, 2026-11-01, 0d
    section Foundations
    F1 Cargo workspace, licensing, CI matrix :done, f1, 2026-10-10, 1d
    F2 Prior-art research spike :done, f2, 2026-10-10, 1d
    F3 Architecture decision records :done, f3, 2026-10-10, 1d
    F4 Lab inventory from test reports :f4, after f2 h2 h4 h5, 3d
```

## Tasks

| ID | Task | Days | Blocked by | Blocks | Start | End | Slack |
|----|------|-----:|------------|--------|-------|-----|------:|
| F1 | Cargo workspace, licensing, CI matrix | 5 | - | C1, W1 | 2026-10-10 | 2026-10-10 | done |
| F2 | Prior-art research spike | 10 | - | F3, F4 | 2026-10-10 | 2026-10-10 | done |
| F3 | Architecture decision records | 5 | F2 | C1, C2, W1, A1 | 2026-10-10 | 2026-10-10 | done |
| F4 | Lab inventory from test reports | 3 | F2, H2, H4, H5 | D3, H6, H8 | 2026-11-01 | 2026-11-04 | 4 |

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

### F4 — Lab inventory from test reports

Re-planned on 2026-11-01 (owner): no list is written beforehand. The owner
tests the controllers in the web and desktop test modes, and the inventory
is built from the anonymous test reports. Blocked by H2 (the formats) and
by the test modes (H4, H5).

- The test modes name the unit under test when the inventory does not have
  it yet: a code proposed from the model (`DS5-01`: the model's short name
  and the next number free on this machine) or typed by the person,
  remembered on this machine and written into the report's configuration.
  Units of the inventory are offered as before.
- A tool reads a folder of test reports and adds to
  `data/lab/inventory.toml` each unit it finds (its code and catalog
  model), keeping the entries and notes already there. Machines are not in
  the reports (they would identify the person's computers); they are added
  by hand when wanted.
- Models the reports name that the catalog lacks are added to
  `data/controllers/` (8BitDo, GuliKit, GameSir...), as H9 corrects the
  catalog from what reports show.

The inventory fills up as the first campaign (H6) and the collection
campaign (H9) run; the test modes then offer its units, and the coverage
matrix (H8) shows which of their configurations are untested.

**Done when:** both test modes name units, and the tool builds from a
folder of test reports an inventory that validates against H2's schema.
