//! The anygamepad sync server (roadmap P2, [ADR 0009]): a Cloudflare Worker
//! that stores what the web app sends once the person turns sending on in
//! its settings (`apps/web/src/sync.ts`), in D1.
//!
//! # API
//!
//! Every request that writes or deletes carries `Authorization: Bearer
//! <token>`: 32 to 64 lowercase hexadecimal digits the app made at random.
//! The server keeps only the token's SHA-256, and the same token withdraws
//! what it sent ([server rules]).
//!
//! | Request | Body | Answer |
//! |---------|------|--------|
//! | `POST /v1/test-reports` | A test report (H1), JSON | `201 {"id": n}`; `200` when the same report was there |
//! | `DELETE /v1/test-reports/{id}` | | `204`; `404` for another token |
//! | `POST /v1/activity-logs` | One part of a page load's activity log ([`activity`]) | `201 {"session": …, "part": n}`; `200` when the part was there; `403` for another token than the session's first part |
//! | `DELETE /v1/activity-logs/{session}` | | `204`; `404` for another token |
//! | `GET /` | | `200`, the service's name and version |
//!
//! Errors answer `{"error": "…"}`: `400` (not a valid document), `401` (no
//! token), `413` (too large), `415` (not JSON), `429` (too many requests),
//! `500` (storage). Browsers may call it from the allowed origins only
//! (`ALLOWED_ORIGINS`); requests without an `Origin` (other apps) are
//! accepted.
//!
//! # Layout
//!
//! [`api`] handles requests over a [`api::Store`], and [`activity`] reads
//! activity logs: plain Rust, tested natively. On `wasm32`, the `runtime`
//! module runs them on the Workers runtime, with D1 as the store, the
//! rate limiter, and the daily deletion of old activity logs.
//!
//! [ADR 0009]: https://github.com/anygamepad/anygamepad.github.io/blob/main/docs/adr/0009-sync-server-on-cloudflare-workers.md
//! [server rules]: https://github.com/anygamepad/anygamepad.github.io/blob/main/docs/privacy/README.md#server-rules
#![forbid(unsafe_code)]

pub mod activity;
pub mod api;
pub mod day;

#[cfg(target_arch = "wasm32")]
mod runtime;
