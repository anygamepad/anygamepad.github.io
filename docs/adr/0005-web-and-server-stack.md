# 0005 — Web stack and server stack

- Status: proposed
- Date: 2026-10-10
- Roadmap: F3 (ADR 5); used by W1-W7, P2, P5

## Context

Already decided: the web app runs the anygamepad library compiled to
WebAssembly and is a static site on GitHub Pages
(<https://anygamepad.github.io>). GitHub Pages serves static files only and
cannot set COOP/COEP headers (no WASM threads). The page has a live
controller view (60+ fps), a mapping wizard, long text (privacy policy,
consent) and forms (report preview and submission). Research:
[07-rust-wasm-tooling.md](../research/07-rust-wasm-tooling.md).

## Decision

### Web

1. **HTML UI in TypeScript, built with Vite**; the library is packaged with
   `wasm-pack build --target web` from `crates/anygamepad-wasm` and imported
   as an ES module.
2. **Svelte 5** as the UI framework. Reasons: compiles to small bundles
   without a runtime virtual DOM, simple reactivity for forms and lists, and
   accessible HTML for consent and privacy text.
3. The live controller view is drawn by one `requestAnimationFrame` loop
   that calls into WASM and updates an SVG/canvas directly, outside the
   framework's reactivity, so the frame rate does not depend on the UI
   framework.
4. Data types crossing the boundary derive `tsify`, so the frontend never
   hand-writes the canonical model.
5. Deployment: GitHub Actions builds `apps/web` and publishes only the build
   output to GitHub Pages; bundle size budget enforced in CI (W1).

### Server

6. Rust `axum` + `sqlx` + PostgreSQL, sharing `anygamepad-report` with the
   clients. CORS allows `https://anygamepad.github.io`. Hosting is chosen in
   P5.

## Alternatives

- Trunk + egui for the whole page: maximal sharing with the desktop app, but
  canvas-only UI is weak for long text, forms, accessibility and search
  engines. An egui canvas for the visualization only remains possible later.
- React or Preact: larger ecosystem, more runtime overhead than needed;
  Preact would also work. Plain TypeScript without a framework: viable for a
  first version, harder to grow with the wizard and forms.
- Leptos/Yew (Rust UI frameworks with DOM): one language, but slower builds
  and smaller ecosystems for the content-heavy parts.

## Consequences

- The repository gains a Node toolchain under `apps/web` (Node 22, Vite,
  Svelte, TypeScript); CI installs it for W1.
- `anygamepad-wasm` is the only boundary between Rust and the frontend; the
  frontend contains no input logic.
- Single-threaded WASM; heavy work (profile matching) must stay cheap per
  frame.
