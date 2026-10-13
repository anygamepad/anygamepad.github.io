# 0006 — Server stack

- Status: accepted (2026-10-10)
- Date: 2026-10-10
- Roadmap: F3 (ADR 5, server part); used by P2, P5

## Context

The ingestion API receives device reports from the web app (GitHub Pages,
<https://anygamepad.github.io>) and from the desktop app's collection mode,
stores them, and serves the curation tooling (P4). GitHub Pages cannot host
it. This decision was first part of ADR 0005 and was split out when the web
part was accepted, so each part has its own status.

## Decision

1. Rust **`axum`** HTTP server in `apps/server`, sharing the
   `anygamepad-report` crate (report schema and validation, P1) with the
   clients.
2. **PostgreSQL** through **`sqlx`** (async, compile-time checked queries,
   migrations in the repository).
3. CORS allows only `https://anygamepad.github.io` (plus local development
   origins in debug builds).
4. Hosting, backups and monitoring are chosen in P5.

## Alternatives

- A hosted backend (Supabase, Firebase): less code to run, but the report
  validation would not share the Rust types with the clients, and the data
  would depend on a vendor's schema and pricing.
- SQLite: simpler to operate, but weaker for concurrent writes and for the
  analytical queries the curation tooling needs.

## Consequences

- One language and one report type definition for clients and server.
- The server needs its own host and a managed or self-run PostgreSQL (P5).
