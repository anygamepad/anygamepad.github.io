//! The Worker on the Workers runtime (`wasm32` only, ADR 0009): requests go
//! through [`api::handle`] with D1 as the [`Store`], after the rate
//! limiter; a daily schedule deletes old activity logs.
//!
//! Bindings and variables (`wrangler.toml`): the D1 database `DB`, the rate
//! limiter `UPLOADS`, `ALLOWED_ORIGINS`, `ACTIVITY_LOG_RETENTION_DAYS`, and
//! the secret `RATE_LIMIT_KEY`. Nothing here logs addresses, user agents or
//! contents.

use ::worker::wasm_bindgen::JsValue;
use ::worker::{
    Context, D1Database, D1PreparedStatement, Date as Clock, Env, Error, Headers, Request,
    Response, Result, ScheduleContext, ScheduledEvent, console_error, event,
};
use anygamepad_report::Date;
use serde::Deserialize;

use crate::activity::LogPart;
use crate::api::{self, Config, NewTestReport, PartStored, Store, StoreError, Stored, TokenHash};
use crate::day;

const DB: &str = "DB";
const LIMITER: &str = "UPLOADS";

fn today() -> Date {
    day::from_unix_ms(Clock::now().as_millis() as f64)
}

#[event(fetch)]
async fn fetch(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let today = today();
    let method = req.method().to_string();
    let path = req.path();
    let header = |name: &str| req.headers().get(name);
    let origin = header("Origin")?;
    let authorization = header("Authorization")?;
    let content_type = header("Content-Type")?;
    let length = header("Content-Length")?.and_then(|length| length.parse::<usize>().ok());
    // Never stored or logged: only an HMAC of it reaches the limiter.
    let address = header("CF-Connecting-IP")?;
    let config = Config::from_origins(&env.var("ALLOWED_ORIGINS")?.to_string());

    if method == "POST" || method == "DELETE" {
        let secret = env.secret("RATE_LIMIT_KEY")?.to_string();
        let key = api::rate_limit_key(&secret, today, address.as_deref().unwrap_or("unknown"));
        if !env.rate_limiter(LIMITER)?.limit(key).await?.success {
            let refused = api::refuse(
                429,
                "too many requests: try again in a minute",
                origin.as_deref(),
                &config,
            );
            return into_response(refused);
        }
    }
    let body = if method == "POST" {
        let too_large = || {
            let message = format!("a request holds {} bytes at most", api::MAX_BODY_BYTES);
            into_response(api::refuse(413, &message, origin.as_deref(), &config))
        };
        if length.is_some_and(|length| length > api::MAX_BODY_BYTES) {
            return too_large();
        }
        let body = req.text().await?;
        if body.len() > api::MAX_BODY_BYTES {
            return too_large();
        }
        body
    } else {
        String::new()
    };

    let request = api::Request {
        method: &method,
        path: &path,
        origin: origin.as_deref(),
        authorization: authorization.as_deref(),
        content_type: content_type.as_deref(),
        body: &body,
        today,
    };
    let store = D1Store(env.d1(DB)?);
    into_response(api::handle(&request, &config, &store).await)
}

/// Deletes the activity logs that arrived `ACTIVITY_LOG_RETENTION_DAYS`
/// days ago or before (`[triggers] crons` in `wrangler.toml`).
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(error) = delete_old_logs(&env).await {
        console_error!("deleting old activity logs failed: {error}");
    }
}

async fn delete_old_logs(env: &Env) -> Result<()> {
    let days = env.var("ACTIVITY_LOG_RETENTION_DAYS")?.to_string();
    let days: u32 = days.parse().map_err(|_| {
        Error::RustError(format!(
            "ACTIVITY_LOG_RETENTION_DAYS is not a number: {days}"
        ))
    })?;
    let before = text(&day::days_before(today(), days).to_string());
    let db = env.d1(DB)?;
    db.batch(vec![
        db.prepare(
            "DELETE FROM activity_log_parts WHERE session IN \
             (SELECT session FROM activity_sessions WHERE received < ?1)",
        )
        .bind(core::slice::from_ref(&before))?,
        db.prepare("DELETE FROM activity_sessions WHERE received < ?1")
            .bind(&[before])?,
    ])
    .await?;
    Ok(())
}

fn into_response(response: api::Response) -> Result<Response> {
    let headers = Headers::new();
    for (name, value) in &response.headers {
        headers.set(name, value)?;
    }
    let out = match response.body {
        Some(body) => {
            headers.set("Content-Type", "application/json; charset=utf-8")?;
            Response::ok(body)?
        }
        None => Response::empty()?,
    };
    Ok(out.with_status(response.status).with_headers(headers))
}

fn text(value: &str) -> JsValue {
    JsValue::from_str(value)
}

fn number(value: f64) -> JsValue {
    JsValue::from_f64(value)
}

fn optional(value: Option<&str>) -> JsValue {
    value.map_or(JsValue::NULL, JsValue::from_str)
}

fn failed(error: Error) -> StoreError {
    StoreError(error.to_string())
}

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct Owner {
    owner: i64,
}

/// The [`Store`] on D1 (`migrations/`). Each operation is one batch, which
/// D1 runs as a transaction.
struct D1Store(D1Database);

impl D1Store {
    fn statement(
        &self,
        sql: &str,
        values: &[JsValue],
    ) -> core::result::Result<D1PreparedStatement, StoreError> {
        self.0.prepare(sql).bind(values).map_err(failed)
    }

    /// Rows changed by the statement at `index` of a batch.
    fn changes(
        results: &[::worker::D1Result],
        index: usize,
    ) -> core::result::Result<usize, StoreError> {
        let result = results
            .get(index)
            .ok_or_else(|| StoreError("a statement had no result".into()))?;
        Ok(result
            .meta()
            .map_err(failed)?
            .and_then(|meta| meta.changes)
            .unwrap_or(0))
    }
}

impl Store for D1Store {
    async fn add_test_report(
        &self,
        report: &NewTestReport,
        received: Date,
        token: &TokenHash,
    ) -> core::result::Result<Stored, StoreError> {
        let sha = text(&report.sha256);
        let results = self
            .0
            .batch(vec![
                self.statement(
                    "INSERT INTO test_reports (sha256, received, created, app, model, mode, transport, \
                     unit, os, browser, api, plan, pass, fail, report) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
                     ON CONFLICT (sha256) DO NOTHING",
                    &[
                        sha.clone(),
                        text(&received.to_string()),
                        text(&report.created.to_string()),
                        text(&report.app),
                        text(&report.model),
                        text(&report.mode),
                        text(&report.transport),
                        optional(report.unit.as_deref()),
                        text(&report.os),
                        optional(report.browser.as_deref()),
                        text(&report.api),
                        text(&report.plan),
                        number(f64::from(report.pass)),
                        number(f64::from(report.fail)),
                        text(&report.json),
                    ],
                )?,
                self.statement(
                    "INSERT OR IGNORE INTO test_report_tokens (report_id, token_hash) \
                     SELECT id, ?2 FROM test_reports WHERE sha256 = ?1",
                    &[sha.clone(), text(&token.0)],
                )?,
                self.statement("SELECT id FROM test_reports WHERE sha256 = ?1", &[sha])?,
            ])
            .await
            .map_err(failed)?;
        let new = Self::changes(&results, 0)? > 0;
        let id = results
            .get(2)
            .map(|result| result.results::<Id>())
            .transpose()
            .map_err(failed)?
            .and_then(|rows| rows.first().map(|row| row.id))
            .ok_or_else(|| StoreError("the report has no id".into()))?;
        Ok(Stored { id, new })
    }

    async fn delete_test_report(
        &self,
        id: i64,
        token: &TokenHash,
    ) -> core::result::Result<bool, StoreError> {
        let id = number(id as f64);
        let results = self
            .0
            .batch(vec![
                self.statement(
                    "DELETE FROM test_reports WHERE id = ?1 AND EXISTS \
                     (SELECT 1 FROM test_report_tokens WHERE report_id = ?1 AND token_hash = ?2)",
                    &[id.clone(), text(&token.0)],
                )?,
                self.statement(
                    "DELETE FROM test_report_tokens WHERE report_id = ?1 AND NOT EXISTS \
                     (SELECT 1 FROM test_reports WHERE id = ?1)",
                    &[id],
                )?,
            ])
            .await
            .map_err(failed)?;
        Ok(Self::changes(&results, 0)? > 0)
    }

    async fn add_log_part(
        &self,
        part: &LogPart,
        received: Date,
        token: &TokenHash,
    ) -> core::result::Result<PartStored, StoreError> {
        let session = text(&part.session);
        let token = text(&token.0);
        let received = text(&received.to_string());
        let environment = part
            .environment
            .map(|environment| serde_json::to_string(&environment))
            .transpose()
            .map_err(|error| StoreError(error.to_string()))?;
        let results = self
            .0
            .batch(vec![
                self.statement(
                    "INSERT INTO activity_sessions (session, token_hash, received, page, library, environment) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (session) DO NOTHING",
                    &[
                        session.clone(),
                        token.clone(),
                        received.clone(),
                        text(part.page.as_str()),
                        text(part.library.as_str()),
                        optional(environment.as_deref()),
                    ],
                )?,
                self.statement(
                    "INSERT INTO activity_log_parts (session, part, received, day, dropped_input_events, \
                     event_count, redactions, events) \
                     SELECT ?1, ?3, ?4, ?5, ?6, ?7, ?8, ?9 WHERE EXISTS \
                     (SELECT 1 FROM activity_sessions WHERE session = ?1 AND token_hash = ?2) \
                     ON CONFLICT (session, part) DO NOTHING",
                    &[
                        session.clone(),
                        token.clone(),
                        number(f64::from(part.part)),
                        received,
                        text(&part.day.to_string()),
                        number(part.dropped_input_events as f64),
                        number(part.event_count as f64),
                        number(part.redactions as f64),
                        text(&part.events),
                    ],
                )?,
                self.statement(
                    "SELECT token_hash = ?2 AS owner FROM activity_sessions WHERE session = ?1",
                    &[session, token],
                )?,
            ])
            .await
            .map_err(failed)?;
        let owner = results
            .get(2)
            .map(|result| result.results::<Owner>())
            .transpose()
            .map_err(failed)?
            .and_then(|rows| rows.first().map(|row| row.owner == 1))
            .unwrap_or(false);
        Ok(if !owner {
            PartStored::OtherToken
        } else if Self::changes(&results, 1)? > 0 {
            PartStored::New
        } else {
            PartStored::Duplicate
        })
    }

    async fn delete_log(
        &self,
        session: &str,
        token: &TokenHash,
    ) -> core::result::Result<bool, StoreError> {
        let session = text(session);
        let token = text(&token.0);
        let results = self
            .0
            .batch(vec![
                self.statement(
                    "DELETE FROM activity_log_parts WHERE session = ?1 AND EXISTS \
                     (SELECT 1 FROM activity_sessions WHERE session = ?1 AND token_hash = ?2)",
                    &[session.clone(), token.clone()],
                )?,
                self.statement(
                    "DELETE FROM activity_sessions WHERE session = ?1 AND token_hash = ?2",
                    &[session, token],
                )?,
            ])
            .await
            .map_err(failed)?;
        Ok(Self::changes(&results, 1)? > 0)
    }
}
