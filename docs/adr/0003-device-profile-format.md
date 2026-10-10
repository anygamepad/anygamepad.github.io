# 0003 — Device profile file format and schema tooling

- Status: accepted (2026-10-10)
- Date: 2026-10-10
- Roadmap: F3 (ADR 3); used by D1-D4, P4

## Context

Device profiles live in `data/profiles/`, are reviewed by humans in pull
requests (curated by hand and generated from reports by P4), are embedded in
the library and in the `.wasm` bundle, and are consumed by tools and the web
app. The roadmap asks for TOML or JSON plus a published JSON Schema.

## Decision

1. **Source format: TOML**, one file per device family. Reasons: comments
   (why a quirk exists, which report confirmed it), readable diffs, no
   trailing-comma or quoting noise in reviews, and `toml` is the standard
   Rust parser (1.1.8, TOML 1.1).
2. **Rust types are the single source of truth.** The profile structs in
   `anygamepad-db` derive `serde` and `schemars` (1.2); a test generates
   `data/profiles/schema.json` and fails if the committed file is stale.
3. **Validation**: CI parses every profile with the Rust types and validates
   it against the JSON Schema; editors can use the schema for TOML through
   Taplo.
4. **Embedding**: a build script converts all profiles to a compact binary
   (`postcard` 1.1, serde-based, `no_std`-friendly) included with
   `include_bytes!`. The same blob ships in the `.wasm` bundle (D4). Runtime
   overrides (user or game files) are TOML parsed at runtime.
5. Every profile has `schema_version`, `source` (`curated`,
   `sdl-gamecontrollerdb`, `report`) and `verified`.

## Alternatives

- JSON source: no comments, noisier reviews. JSON stays as the schema format
  and as the report format (P1).
- RON/YAML: less common in tooling; YAML is ambiguous.
- Embedding TOML text and parsing at startup: larger bundle and a TOML parser
  in every game and in the `.wasm`.
- `rkyv` zero-copy: faster loads, but a more complex format to evolve; the
  database is small enough that `postcard` decoding is cheap.

## Consequences

- P4 must generate TOML (with comments that link to the reports).
- Changing the profile structs requires regenerating the schema; CI enforces
  it.
- The binary blob is versioned with the library; the schema version guards
  runtime overrides.
