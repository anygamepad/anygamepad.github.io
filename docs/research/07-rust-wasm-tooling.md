# Rust to WebAssembly tooling

Versions from crates.io on 2026-10-10.

| Tool / crate | Version | Notes |
|--------------|---------|-------|
| `wasm-bindgen` | 0.2.129 (2026-09-25) | JS interop and `.d.ts` generation for exported items. Repository now under the `wasm-bindgen` GitHub organization |
| `web-sys`, `js-sys` | 0.3.106 | Generated Web API bindings; Gamepad and WebHID available, some behind `web_sys_unstable_apis` (see [06-browser-gamepad-api.md](06-browser-gamepad-api.md)) |
| `wasm-bindgen-test` | 0.3.79 | Tests in Node or headless browsers; used by our CI in Chrome and Firefox (F1) |
| `wasm-bindgen-cli` | = `wasm-bindgen` version | Post-processes the `.wasm`, provides `wasm-bindgen-test-runner`; must match the library version exactly |
| `wasm-pack` | 0.15.0 (2026-05-15) | Build + package as npm module (`--target web/bundler/nodejs`); also under the `wasm-bindgen` organization |
| `trunk` | 0.21.14 (2026-10-06) | Rust-first web bundler (HTML entry point, asset pipeline, `wasm-opt`) |
| `tsify` | 0.5.8 (2026-08-23) | Derives TypeScript types for serde structs passed across the boundary. `tsify-next` (0.5.6, 2025-04) is an older fork |
| `serde-wasm-bindgen` | 0.6.5 (2024-02-27) | Converts serde values to/from `JsValue` |

## Findings

- F1 already proves the chain: `anygamepad-wasm` builds for
  `wasm32-unknown-unknown` and its tests run in headless Chrome and Firefox
  with `wasm-bindgen-test-runner` installed at the `Cargo.lock` version.
- TypeScript types for the frontend come from two places: `wasm-bindgen`'s
  generated `.d.ts` (functions, classes) and `tsify` (data structs such as
  device state, identity and reports).
- Two packaging routes:
  1. `wasm-pack build --target web` (or `wasm-bindgen` CLI directly) + a JS
     bundler (Vite) and a JS/TS UI framework.
  2. Trunk + a Rust UI (e.g. egui on a canvas, which could share the gamepad
     visualization with the desktop app).
- Both produce static files, fine for GitHub Pages. GitHub Pages cannot set
  COOP/COEP headers, so no `SharedArrayBuffer`/WASM threads: the WASM module
  stays single-threaded (already in the roadmap risks).
- Bundle size: the embedded profile database will dominate. Use `wasm-opt -Oz`
  (Binaryen, run by Trunk/wasm-pack), `opt-level = "z"`, LTO, and a compact
  database encoding (D4); measure in CI (W1).
- Safari is not covered by `wasm-bindgen-test` in CI yet; `safaridriver` on
  macOS runners is an option.

## Implications for anygamepad (F3 ADR 5, W1, C11)

- F3 picks route 1 or 2. Route 1 is the usual choice for a content-heavy page
  (tester UI, consent forms, report preview); route 2 maximizes sharing with
  the egui desktop app. A middle path: route 1 for the page, with an optional
  egui canvas for the controller visualization.
- Use `tsify` for exported data types so the TypeScript side never
  hand-writes the canonical model.
