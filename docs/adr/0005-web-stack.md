# 0005 — Web stack

- Status: accepted (2026-10-10)
- Date: 2026-10-10
- Roadmap: F3 (ADR 5); used by W1-W7

## Context

Already decided: the web app runs the anygamepad library compiled to
WebAssembly and is a static site on GitHub Pages
(<https://anygamepad.github.io>). GitHub Pages serves static files only: it
cannot set COOP/COEP headers (no WASM threads), and a Content Security Policy
can only be set with a `<meta>` tag. The page has a live controller view
(60+ fps), a mapping wizard, long text (privacy policy, consent) and forms
(report preview and submission). Research:
[07-rust-wasm-tooling.md](../research/07-rust-wasm-tooling.md).

The server stack, first proposed in this ADR, moved to
[0006](0006-server-stack.md).

## Decision

1. **HTML UI in TypeScript, built with Vite**; the library is packaged with
   `wasm-pack build --target web` from `crates/anygamepad-wasm` and imported
   as an ES module.
2. **jq79** (<https://github.com/jgermade/jq79>, ISC) as the UI framework
   (owner decision). Components are single-file `.html` files with a
   `<script :setup>` block and fine-grained, proxy-based reactivity; the
   library has no dependencies. Usage rules:
   - Components are imported through the `jq79/vite` plugin, which bundles
     them and strips types from `<script lang="ts">` blocks.
   - The plugin runs with `safeEval` enabled, so the page's CSP does not need
     `'unsafe-eval'` (only `'wasm-unsafe-eval'`, which WebAssembly needs).
   - The plugin strips types but does not type-check component scripts, so
     non-trivial logic lives in `.ts` modules checked by `tsc --noEmit` in
     CI; component scripts stay thin.
   - The version is pinned (pre-1.0 library).
3. The live controller view is drawn by one `requestAnimationFrame` loop
   that calls into WASM and updates an SVG/canvas directly, outside the
   framework's reactivity, so the frame rate does not depend on the UI
   framework.
4. Data types crossing the boundary derive `tsify`, so the frontend never
   hand-writes the canonical model.
5. Deployment: GitHub Actions builds `apps/web` and publishes only the build
   output to GitHub Pages; bundle size budget enforced in CI (W1).

## Alternatives

- Svelte 5 (proposed first): small compiled bundles and a large ecosystem;
  the owner chose jq79, which offers a similar Svelte-style setup script
  without a compiler.
- Trunk + egui for the whole page: maximal sharing with the desktop app, but
  canvas-only UI is weak for long text, forms, accessibility and search
  engines. An egui canvas for the visualization only remains possible later.
- React or Preact: larger ecosystem, more runtime overhead than needed.
  Plain TypeScript without a framework: viable for a first version, harder to
  grow with the wizard and forms.
- Leptos/Yew (Rust UI frameworks with DOM): one language, but slower builds
  and smaller ecosystems for the content-heavy parts.

## Consequences

- The repository gains a Node toolchain under `apps/web` (Node 22, Vite,
  jq79, TypeScript); CI installs it for W1.
- `anygamepad-wasm` is the only boundary between Rust and the frontend; the
  frontend contains no input logic.
- Single-threaded WASM; heavy work (profile matching) must stay cheap per
  frame.
- jq79 is maintained by the project owner, so gaps found while building the
  web app can be fixed upstream.
