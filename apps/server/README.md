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

By the owner, with a Cloudflare account. The free plan is enough; Workers
Paid keeps 30 days of D1 restore points instead of 7, as the
[server rules](../../docs/privacy/README.md#server-rules) ask.

### From GitHub Actions

The workflow [`deploy-server.yml`](../../.github/workflows/deploy-server.yml)
creates the D1 database in the EU jurisdiction the first time (and finds it
by name later), applies the migrations, deploys, makes the rate limiter's
secret once, and checks that the Worker answers. Once:

1. In the Cloudflare dashboard, open **Workers & Pages** once: it gives the
   account its `workers.dev` subdomain, which a deployment from CI cannot
   register by itself. Note the **Account ID** shown there.
2. **My Profile, API Tokens, Create Token**: the "Edit Cloudflare Workers"
   template, plus the permission **Account, D1, Edit**, for this account.
3. In GitHub, **Settings, Secrets and variables, Actions**: the repository
   secrets `CLOUDFLARE_API_TOKEN` (the token) and `CLOUDFLARE_ACCOUNT_ID`.
4. **Actions, Deploy server, Run workflow** (on `main`). Its summary shows
   the Worker's URL (`https://anygamepad-sync.<subdomain>.workers.dev/`).
5. The repository variable `ANYGAMEPAD_SYNC_URL` (same page as the secrets,
   **Variables** tab) set to that URL, then **Actions, Build, Run workflow**
   on `main`: it publishes the site with sending available.
6. In the dashboard, check that the account accepted the
   [Data Processing Addendum](https://www.cloudflare.com/cloudflare-customer-dpa/)
   the privacy policy names, and that Workers Logs stay off for the Worker.

Later changes to the server: run **Deploy server** again; it applies new
files in `migrations/` first.

### From a computer

With the tools of [Development](#development), from this folder:

1. `npx wrangler login`.
2. `npx wrangler d1 create anygamepad-sync --jurisdiction eu`, and put the
   `database_id` it prints into `wrangler.toml` (it is not a secret).
3. `npm run migrate`, then `npm run deploy`; wrangler prints the Worker's
   URL. `curl` it to see `{"service":"anygamepad-sync",...}`.
4. `openssl rand -hex 32 | npx wrangler secret put RATE_LIMIT_KEY`.
5. Steps 5 and 6 above.

### Data

Curation (P4) reads an export:
`npx wrangler d1 export anygamepad-sync --remote --output sync.sql`.
