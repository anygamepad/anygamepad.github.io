# 00 — Foundations

Groundwork that unblocks every other workstream. See the
[master schedule](README.md#master-schedule) for the global view.

## Schedule

```mermaid
gantt
    dateFormat YYYY-MM-DD
    axisFormat %b %d
    title Foundations (critical path in red)
    section Foundations
    F1 Cargo workspace, licensing, CI matrix :f1, 2026-10-12, 5d
    F2 Prior-art research spike :crit, f2, 2026-10-12, 10d
    F3 Architecture decision records :crit, f3, after f2, 5d
    F4 Hardware test lab inventory :f4, after f2, 5d
```

## Tasks

| ID | Task | Days | Blocked by | Blocks | Start | End | Slack |
|----|------|-----:|------------|--------|-------|-----|------:|
| F1 | Cargo workspace, licensing, CI matrix | 5 | - | C1, W1 | 2026-10-12 | 2026-10-17 | 10 |
| F2 | **Prior-art research spike** | 10 | - | F3, F4 | 2026-10-12 | 2026-10-22 | 0 |
| F3 | **Architecture decision records** | 5 | F2 | C1, C2, W1, A1 | 2026-10-22 | 2026-10-27 | 0 |
| F4 | Hardware test lab inventory | 5 | F2 | D3 | 2026-10-22 | 2026-10-27 | 90 |

### F1 — Cargo workspace, licensing, CI matrix

- Create the Cargo workspace with the proposed layout (final layout confirmed by
  F3):

  ```text
  crates/
    rustpad/          # public facade crate used by games
    rustpad-model/    # canonical gamepad model types
    rustpad-db/       # device profile format and embedded database
    rustpad-report/   # device report schema shared by apps and server
    rustpad-virtual/  # virtual controller outputs (desktop app only)
    rustpad-wasm/     # wasm-bindgen JS/TS bindings used by the web app
  apps/
    desktop/
    web/
    server/
  data/
    profiles/         # curated device profiles
  docs/adr/           # architecture decision records
  ```

  Crate names are provisional: `rustpad` is taken on crates.io, so the final
  prefix is chosen here (see the risks in the [overview](README.md#top-risks)).

- Choose and add the license (proposal: `MIT OR Apache-2.0`, the Rust
  ecosystem default; compatible with importing the zlib-licensed SDL
  GameControllerDB).
- CI on Linux, Windows and macOS: `cargo fmt --check`, `cargo clippy
  -- -D warnings`, `cargo test`, plus a `wasm32-unknown-unknown` build and
  `wasm-bindgen-test` tests in a headless browser.
- Choose the published crate name (`rustpad` is taken).

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
4. Desktop UI toolkit. Recommendation: **Tauri 2**, reusing the web app's
   frontend components (device view, wizard) with Tauri commands instead of the
   WASM bindings as data source. Alternatives considered: egui, Slint, iced.
5. Web stack and server stack. Already decided: the web app uses the rustpad
   library compiled to WebAssembly. To decide: frontend framework (a TypeScript
   framework is preferred if the desktop app uses Tauri, so components can be
   shared) and bundler; server proposal is Rust
   `axum` + `sqlx` + PostgreSQL.

**Done when:** ADRs are merged with status `accepted`.

### F4 — Hardware test lab inventory

List the controllers physically available, their supported modes and
transports, and which OS machines are available to test them. This feeds the
initial curated profiles (D3) and the testing matrix.
