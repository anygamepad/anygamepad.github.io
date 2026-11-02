# 0009 — Sync server on Cloudflare Workers and D1

- Status: accepted (2026-11-02, owner)
- Date: 2026-11-02
- Roadmap: P2, P5, P6 (ROADMAP/2026-11-01/)
- Supersedes the HTTP framework, storage and hosting of
  [ADR 0006](0006-server-stack.md); the rest of ADR 0006 (Rust, the shared
  `anygamepad-report` crate, CORS limited to the site) and the timing of
  [ADR 0007](0007-local-first-device-reports.md) stay.

## Context

The owner asked on 2026-11-02 for the web app's activity log and test
reports to reach an online database, with a switch in the settings to turn
sending on (`apps/web/src/sync.ts`). ADR 0006 planned an `axum` service
with PostgreSQL on a host to choose in P5: a server to keep up to date,
with TLS, backups and monitoring, for one contributor and a small volume
(the owner's test campaigns, then whoever turns sending on).

What the server must do does not change: validate what it receives with
the same Rust code as the clients, store the re-serialized documents,
deduplicate, limit request rates without storing IP addresses, keep 30
days of backups, let a person withdraw what they sent, and accept only the
site's origin ([server rules](../privacy/README.md#server-rules)).

## Decision

1. **A Cloudflare Worker written in Rust** with
   [`workers-rs`](https://github.com/cloudflare/workers-rs) (`worker`
   crate), in `apps/server` (crate `anygamepad-server`). It validates with
   `anygamepad-report` compiled to WebAssembly, as the web app does. The
   request handling (validation, canonical JSON, hashes, routing, CORS) is
   plain Rust tested natively with `cargo test`; only a thin layer talks
   to the Workers runtime, on `wasm32` only.
2. **Storage in D1** (Cloudflare's SQLite), created in the EU jurisdiction.
   Schema changes are SQL migrations in `apps/server/migrations/`, applied
   with `wrangler d1 migrations apply`. Documents are stored as compact
   JSON text next to the columns curation queries by.
3. **Rate limiting** with the Workers rate limiting binding, keyed by an
   HMAC of the client's address with a secret key and the day: counters
   live in Cloudflare's memory, the address is never stored or logged.
4. **Backups**: D1 Time Travel (point-in-time restore of the last 30 days
   on the Workers Paid plan, 7 on the free one) is the 30-day backup of
   the server rules; exports (`wrangler d1 export`) feed curation (P4).
5. **No logs of requests**: Workers observability stays off, and the
   Worker logs no addresses, user agents or contents.
6. **Deployment**: `wrangler deploy` from `apps/server`, by the owner, with
   the account's API token; the web app sends to the Worker once its build
   names the Worker's URL (`ANYGAMEPAD_SYNC_URL`, a repository variable).

## Alternatives

- **ADR 0006 as written** (`axum`, PostgreSQL, a VPS or a platform such as
  Fly.io): full control and PostgreSQL's queries, but a server to keep
  patched, TLS, backups and monitoring to run (P5), and a monthly cost.
- **Supabase** (PostgreSQL, Edge Functions): PostgreSQL without running it,
  but validation in Deno, free projects pause after a week without
  traffic, and the platform's API logs hold addresses.
- **Firebase**: client-side writes under security rules, no shared Rust
  validation.
- **A repository as the database** (a Worker committing files): public and
  versioned, but too slow and heavy for activity logs.
- **Cloudflare R2 for documents**: not needed while documents fit D1's
  2,000,000-byte rows; the Worker refuses larger ones (`413`), and R2 is
  the way out if test reports grow past that.

## Consequences

- SQLite instead of PostgreSQL: curation (P4) queries an export (SQLite or
  DuckDB) or the JSON columns with SQLite's JSON functions.
- One vendor for compute and storage. The API is the project's
  (`apps/web/src/sync.ts` documents it, `apps/server` implements it) and
  the data exports, so moving to ADR 0006's stack later means rewriting
  the thin layer and copying the tables.
- Cloudflare processes the requests (and sees addresses) under its data
  processing addendum: the privacy policy names it.
- P5 shrinks to the account setup (D1 in the EU, the rate limiter, the
  secret, Time Travel) and the deployment; there is no server to monitor
  beyond the Worker's error rate in the dashboard.
- `apps/server` joins the Cargo workspace. Its `worker` dependency is
  `wasm32`-only, so native builds and tests skip the runtime layer.
