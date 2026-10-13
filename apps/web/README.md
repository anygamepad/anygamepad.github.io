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

# Smoke test of dist/ in Chrome (loads under its CSP, a fake gamepad shows up)
CHROME_PATH=/path/to/chrome npm run test:smoke
```

`npm run build` emits a Content Security Policy `<meta>` tag (GitHub Pages
cannot send headers); the dev server runs without one.

## Layout

| Path | Content |
|------|---------|
| `src/main.ts` | Entry point: loads the WebAssembly module, mounts the app |
| `src/tester.ts` | Frame loop and view data (logic stays in TypeScript) |
| `src/*.html` | jq79 components (rendering only) |
| `src/wasm/` | wasm-pack output (generated, not committed) |
| `scripts/check-size.mjs` | Bundle size budget |
| `tests/smoke.mjs` | Smoke test of the built site in Chrome |
