# anygamepad

Gamepad input normalization: one canonical gamepad model for every controller,
mode, transport and operating system.

- Web app: <https://anygamepad.github.io> (not live yet)
- Plan: [ROADMAP/2026-10-10/](ROADMAP/2026-10-10/README.md)
- Contributor and agent rules: [AGENTS.md](AGENTS.md)

## Layout

| Path | Content |
|------|---------|
| `crates/anygamepad` | Public facade crate used by games |
| `crates/anygamepad-model` | Canonical gamepad model types |
| `crates/anygamepad-db` | Device profile format and embedded database |
| `crates/anygamepad-report` | Device report schema shared by apps and server |
| `crates/anygamepad-virtual` | Virtual controller outputs (desktop app only) |
| `crates/anygamepad-wasm` | `wasm-bindgen` bindings used by the web app |
| `apps/` | Web app, desktop app and ingestion server |
| `data/profiles/` | Curated device profiles |
| `docs/adr/` | Architecture decision records |

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# WebAssembly: build, then run the browser tests in headless Chrome.
cargo build --workspace --exclude anygamepad-virtual --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
cargo test -p anygamepad-wasm --target wasm32-unknown-unknown
```

The browser tests need `chromedriver` (or `geckodriver` for Firefox) in `PATH`,
or its path in the `CHROMEDRIVER` (`GECKODRIVER`) environment variable.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
