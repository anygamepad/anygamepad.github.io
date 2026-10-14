# Web app

Gamepad tester built on the anygamepad library compiled to WebAssembly,
published on GitHub Pages at <https://anygamepad.github.io>.

Vite + TypeScript + [jq79](https://github.com/jgermade/jq79)
([ADR 0005](../../docs/adr/0005-web-stack.md)). All device access runs in
`crates/anygamepad-wasm`; the frontend renders and handles interaction.

## Development

Needs Node 22.12+, the Rust toolchain of the repository and
[wasm-pack](https://github.com/wasm-bindgen/wasm-pack) 0.15.

```sh
npm ci
npm run wasm       # build the bindings into src/wasm (release; wasm:dev for a debug build)
npm run dev        # dev server with hot reload
npm run build      # type-check, then build into dist/
npm run size       # check dist/ against the bundle size budget
npm run preview    # serve dist/

# Smoke test of dist/ with fake gamepads: loads under its CSP, live values,
# drawing, update rate, vibration test, guided wizard, hotplug, privacy page
CHROME_PATH=/path/to/chrome npm run test:smoke
npx playwright-core install firefox webkit   # once, for the other engines
BROWSER=firefox npm run test:smoke           # or BROWSER=webkit (Safari's engine)
```

`npm run build` emits a Content Security Policy `<meta>` tag (GitHub Pages
cannot send headers); the dev server runs without one.

## Layout

| Path | Content |
|------|---------|
| `src/main.ts` | Entry point: loads the WebAssembly module, mounts the app |
| `privacy/index.html`, `src/privacy.ts` | The privacy policy at `/privacy/` (roadmap P3; its maintainer side is [`docs/privacy/`](../../docs/privacy/README.md)) |
| `src/tester.ts` | Sampler and frame loops, device list, vibration test, WebHID actions (logic stays in TypeScript) |
| `src/activity.ts` | Activity log: what the tester detects, downloaded as JSON to share for analysis |
| `src/live.ts` | Live view of a Gamepad API pad: update rate, drawing, raw buttons and axes |
| `src/hid-live.ts` | Live view of a WebHID pad: report form, drawing, touchpad, motion, battery, raw reports |
| `src/wizard.ts` | The guided wizard's view: asks for each control in turn (the library's `MappingWizard` decides) |
| `src/drawing.ts`, `src/layouts.ts` | Controller drawings of the canonical state, one per family |
| `src/art/` | The owner's controller illustrations, marked up with data attributes |
| `src/rate.ts` | Update rate meters |
| `src/theme.ts` | Light, dark or system theme |
| `src/*.html` | jq79 components (rendering only) |
| `src/wasm/` | wasm-pack output (generated, not committed) |
| `scripts/check-size.mjs` | Bundle size budget |
| `tests/smoke.mjs` | Smoke test of the built site (Chromium, Firefox, WebKit) |
