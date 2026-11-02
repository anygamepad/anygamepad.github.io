# Sync server

The anygamepad sync server (roadmap P2,
[ADR 0009](../../docs/adr/0009-sync-server-on-cloudflare-workers.md)): a
Cloudflare Worker written in Rust that stores what the web app sends once
the person turns sending on in its settings — the activity log, in parts,
and the test reports they save — in a D1 database. Every upload carries a
deletion token the app makes, so the person can withdraw it.

The API is documented in [`src/lib.rs`](src/lib.rs); the client is
[`apps/web/src/sync.ts`](../web/src/sync.ts). What the server may store and
for how long: [`docs/privacy/`](../../docs/privacy/README.md#server-rules).

## Layout

| Path | Content |
|------|---------|
| `src/api.rs` | Routing, CORS, deletion tokens, test report validation (`anygamepad-report`), the `Store` trait; tested natively |
| `src/activity.rs` | Activity log parts: shape checks and the scrub of identifiers |
| `src/day.rs` | UTC days from the clock, and the retention cut-off |
| `src/runtime.rs` | `wasm32` only: the Worker's `fetch` and `scheduled` handlers, D1 as the store, the rate limiter |
| `migrations/` | D1 tables, applied with `wrangler d1 migrations apply` |
| `wrangler.toml` | The Worker: build, bindings (`DB`, `UPLOADS`), variables, the daily cron |
| `package.json` | Wrangler, pinned |

## Development

Needs the repository's Rust toolchain (with `wasm32-unknown-unknown`),
Node 22.12+ and [worker-build](https://crates.io/crates/worker-build) 0.8.7
(`cargo install worker-build@0.8.7 --locked`; wrangler's build command
installs it too).

```sh
cargo test -p anygamepad-server   # request handling, natively
npm ci
# Local secrets and origins for `wrangler dev` (never committed):
printf 'ALLOWED_ORIGINS="http://127.0.0.1:4319 http://localhost:5173"\nRATE_LIMIT_KEY="local"\n' > .dev.vars
npm run migrate:local             # tables in a local D1 (.wrangler/)
npm run dev                       # the Worker on http://127.0.0.1:8787
```

The web app sends to it when built with its URL, for example
`ANYGAMEPAD_SYNC_URL=http://127.0.0.1:8787/ npm run dev` in `apps/web`.
`npm run test:sync` in `apps/web` does all of it in a fresh folder: it
starts the Worker on a new local D1, builds the site naming it, runs a test
session in Chrome with sending on, checks what D1 holds, and withdraws it
(`CHROME_PATH` must point at Chrome or Chromium). CI runs it in the
`server` job.

## Deployment (P5)

Once, by the owner, with a Cloudflare account (the free plan is enough;
Workers Paid keeps 30 days of D1 restore points instead of 7, as the
[server rules](../../docs/privacy/README.md#server-rules) ask):

1. `npx wrangler login`, from this folder.
2. `npx wrangler d1 create anygamepad-sync --jurisdiction eu`, and put the
   `database_id` it prints into `wrangler.toml` (commit it: it is not a
   secret).
3. `openssl rand -hex 32 | npx wrangler secret put RATE_LIMIT_KEY`.
4. `npm run migrate`: the tables in the remote database.
5. `npm run deploy`. Wrangler prints the Worker's URL
   (`https://anygamepad-sync.<account>.workers.dev`); `curl` it to see
   `{"service":"anygamepad-sync",...}`.
6. In the Cloudflare dashboard, check that the account accepted the
   [Data Processing Addendum](https://www.cloudflare.com/cloudflare-customer-dpa/)
   the privacy policy names, and that Workers Logs stay off for the Worker.
7. In GitHub, set the repository variable `ANYGAMEPAD_SYNC_URL` (Settings,
   Secrets and variables, Actions, Variables) to that URL. The next build
   of `main` publishes the site with sending available.

Later changes: `npm run migrate` for new files in `migrations/`, then
`npm run deploy`. Curation (P4) reads an export:
`npx wrangler d1 export anygamepad-sync --remote --output sync.sql`.
