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
npm run wasm       # build both modules into src/wasm and src/wasm-testing (release; wasm:dev for a debug build)
npm run dev        # dev server with hot reload
npm run build      # type-check, then build into dist/
npm run size       # check dist/ against the bundle size budget
npm run preview    # serve dist/

# Smoke test of dist/ with fake gamepads: loads under its CSP, live values,
# drawing, update rate, vibration test, guided wizard, hotplug, header, settings,
# privacy page; then the test mode at /test/
CHROME_PATH=/path/to/chrome npm run test:smoke
npx playwright-core install firefox webkit   # once, for the other engines
BROWSER=firefox npm run test:smoke           # or BROWSER=webkit (Safari's engine)
```

`npm run build` emits a Content Security Policy `<meta>` tag (GitHub Pages
cannot send headers); the dev server runs without one.

## Pages

| URL | Page |
|-----|------|
| `/` | The tester (`index.html`) |
| `/test/` | The test mode (`test/index.html`, roadmap H4), linked from the tester's footer |
| `/privacy/` | The privacy policy (`privacy/index.html`, roadmap P3) |

The tester and the test mode are the same app (`src/main.ts`,
`src/App.html`); `<body data-page="test">` picks the test mode, whose
WebAssembly module only that page loads (W9).

## Sending to the database

The settings (the sliders icon in the header) hold a switch that sends the
activity log, in parts as it grows, and each saved test report to the
anygamepad sync server ([`apps/server`](../server/README.md), a Cloudflare
Worker; `src/sync.ts` is the client). Each upload carries a deletion token
the page makes and keeps in `localStorage`, and "Delete what this browser
sent" withdraws them all. Sending is off by default, and it cannot be
turned on while the build names no server:

```sh
ANYGAMEPAD_SYNC_URL=https://anygamepad-sync.example.workers.dev/ npm run build
```

names one in a `<meta name="anygamepad-sync">` tag and adds its origin to
the policy's `connect-src`. CI builds with the repository variable of the
same name, set once the server is deployed.

`npm run test:sync` runs the web app against the real Worker on a local D1
(see [`apps/server`](../server/README.md#development)).

## Layout

| Path | Content |
|------|---------|
| `src/main.ts`, `index.html`, `test/index.html` | Entry point: loads the WebAssembly module, mounts the app as the tester or the test mode |
| `privacy/index.html`, `src/privacy.ts` | The privacy policy at `/privacy/` (roadmap P3; its maintainer side is [`docs/privacy/`](../../docs/privacy/README.md)) |
| `src/tester.ts` | Sampler and frame loops, device list, vibration test, WebHID actions (logic stays in TypeScript) |
| `src/activity.ts` | Activity log: what the tester detects, downloaded as JSON to share for analysis, or sent in parts |
| `src/sync.ts` | Sending the activity log and test reports to the sync server, once the person turns it on in the settings, and withdrawing them |
| `src/live.ts` | Live view of a Gamepad API pad: update rate, drawing, raw buttons and axes |
| `src/hid-live.ts` | Live view of a WebHID pad: report form, drawing, touchpad, motion, battery, raw reports |
| `src/wizard.ts` | The guided wizard's view: asks for each control in turn (the library's `MappingWizard` decides) |
| `src/test-mode.ts` | The test mode (roadmap H4): the list of controllers, the ways to connect one, detection, the session (the library's `TestSession` decides) and its anonymous test report |
| `src/storage.ts` | IndexedDB: saved device reports (`reports`) and test reports (`test-reports`) |
| `src/drawing.ts`, `src/layouts.ts` | Controller drawings of the canonical state, one per family |
| `src/art/` | The owner's controller illustrations, marked up with data attributes |
| `src/rate.ts` | Update rate meters |
| `src/theme.ts` | Light, dark or system theme (the icons in the header) |
| `src/*.html` | jq79 components (rendering only) |
| `src/wasm/` | wasm-pack output of the tester's module, `anygamepad-wasm` (generated, not committed) |
| `src/wasm-testing/` | wasm-pack output of the test mode's module, `anygamepad-wasm-testing`, loaded by the test mode's page only (W9; generated, not committed) |
| `scripts/check-size.mjs` | Bundle size budget |
| `tests/smoke.mjs` | Smoke test of the built site (Chromium, Firefox, WebKit) |
| `tests/smoke-test-mode.mjs` | Smoke test of the test mode at `/test/`: a Full session on a Gamepad API pad, a Quick one on a WebHID DualSense (Chromium), and sending to a fake database |
| `tests/smoke-sync.mjs` | End-to-end test of sending, against the sync server's Worker on a local D1 |
| `tests/fakes.mjs` | The fake gamepads and WebHID device both smoke tests use |
