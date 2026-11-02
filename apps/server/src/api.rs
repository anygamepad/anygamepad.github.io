//! Request handling over a [`Store`]: routing, CORS, tokens, validation,
//! canonical JSON and hashes. Plain Rust: the Worker (`worker` module) runs
//! it with D1 as the store, the tests with one in memory.

use anygamepad_report::Date;
use anygamepad_report::test_report::TestReport;
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::activity::{LogPart, MAX_PART_BYTES, is_session_id};

/// Bytes of a test report as stored: a D1 row holds 2,000,000 bytes, with
/// the other columns.
pub const MAX_TEST_REPORT_BYTES: usize = 1_900_000;

/// Bytes of a request body the Worker reads at most.
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// The service's name, answered at `GET /`.
pub const SERVICE: &str = "anygamepad-sync";

/// A request, as the Worker read it.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// `GET`, `POST`, `DELETE`, `OPTIONS`, ...
    pub method: &'a str,
    /// The URL's path.
    pub path: &'a str,
    /// The `Origin` header: browsers send it, other apps do not.
    pub origin: Option<&'a str>,
    /// The `Authorization` header.
    pub authorization: Option<&'a str>,
    /// The `Content-Type` header.
    pub content_type: Option<&'a str>,
    /// The body (empty for `GET`, `DELETE` and `OPTIONS`).
    pub body: &'a str,
    /// The UTC day the request arrived.
    pub today: Date,
}

/// An answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// HTTP status.
    pub status: u16,
    /// JSON body, if any.
    pub body: Option<String>,
    /// Headers besides `Content-Type` (set for a body).
    pub headers: Vec<(&'static str, String)>,
}

impl Response {
    fn json(status: u16, body: serde_json::Value) -> Self {
        Self {
            status,
            body: Some(body.to_string()),
            headers: Vec::new(),
        }
    }

    fn empty(status: u16) -> Self {
        Self {
            status,
            body: None,
            headers: Vec::new(),
        }
    }

    fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, json!({ "error": message.into() }))
    }
}

/// What the Worker is configured with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Origins browsers may call from (`https://anygamepad.github.io`).
    pub allowed_origins: Vec<String>,
}

impl Config {
    /// From `ALLOWED_ORIGINS`: origins separated by commas or spaces.
    pub fn from_origins(list: &str) -> Self {
        Self {
            allowed_origins: list
                .split([',', ' '])
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(|origin| origin.trim_end_matches('/').to_owned())
                .collect(),
        }
    }

    fn allows(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|allowed| allowed == origin)
    }
}

/// The SHA-256 of a deletion token, in hexadecimal: what the store keeps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TokenHash(pub String);

/// Reads `Authorization: Bearer <token>`: 32 to 64 lowercase hexadecimal
/// digits.
pub fn token_hash(authorization: Option<&str>) -> Option<TokenHash> {
    let token = authorization?.strip_prefix("Bearer ")?.trim();
    let valid = (32..=64).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    valid.then(|| TokenHash(sha256_hex(token.as_bytes())))
}

/// SHA-256 in lowercase hexadecimal.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The rate limiter's key for a client address: an HMAC of the address
/// and the day with the Worker's secret, so the limiter never sees the
/// address, and a key says nothing the next day.
pub fn rate_limit_key(secret: &str, today: Date, address: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key");
    mac.update(today.to_string().as_bytes());
    mac.update(b"|");
    mac.update(address.as_bytes());
    hex(&mac.finalize().into_bytes()[..16])
}

/// A test report, ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTestReport {
    /// SHA-256 of `json`: the same report is stored once.
    pub sha256: String,
    /// The day it was made.
    pub created: Date,
    /// `web` or `desktop`.
    pub app: String,
    /// The catalog model, mode and transport the person confirmed.
    pub model: String,
    /// See `model`.
    pub mode: String,
    /// See `model`.
    pub transport: String,
    /// The unit's code in the person's inventory.
    pub unit: Option<String>,
    /// The host's OS.
    pub os: String,
    /// The browser engine, on the web.
    pub browser: Option<String>,
    /// The API the session read the device with (`gamepad_api`, `webhid`).
    pub api: String,
    /// The plan's ID.
    pub plan: String,
    /// Steps that passed.
    pub pass: u32,
    /// Steps that failed.
    pub fail: u32,
    /// The report, re-serialized as compact JSON.
    pub json: String,
}

/// The name a value serializes to (`"blink"`, `"usb"`).
fn name_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

impl NewTestReport {
    /// Reads and validates a test report with `anygamepad-report`.
    pub fn from_json(text: &str) -> Result<Self, Response> {
        let report =
            TestReport::from_json(text).map_err(|error| Response::error(400, error.to_string()))?;
        let json = report
            .to_json()
            .map_err(|error| Response::error(400, error.to_string()))?;
        if json.len() > MAX_TEST_REPORT_BYTES {
            return Err(Response::error(
                413,
                format!("a test report holds {MAX_TEST_REPORT_BYTES} bytes at most"),
            ));
        }
        let device = &report.device;
        let confirmed = &report.configuration.confirmed;
        let summary = report.summary();
        Ok(Self {
            sha256: sha256_hex(json.as_bytes()),
            created: device.created,
            app: name_of(&device.source.app),
            model: confirmed.model.clone(),
            mode: confirmed.mode.clone(),
            transport: name_of(&confirmed.transport),
            unit: confirmed.unit.as_ref().map(name_of),
            os: name_of(&device.host.os),
            browser: device
                .host
                .browser
                .as_ref()
                .map(|browser| name_of(&browser.engine)),
            api: device.views.first().map_or_else(
                || "unknown".to_owned(),
                |view| name_of(&view.identity.backend),
            ),
            plan: report.plan.id.clone(),
            pass: u32::try_from(summary.pass).unwrap_or(u32::MAX),
            fail: u32::try_from(summary.fail).unwrap_or(u32::MAX),
            json,
        })
    }
}

/// What storing a test report did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The report's ID.
    pub id: i64,
    /// Whether it is new (or was there, sent before).
    pub new: bool,
}

/// What storing a part of an activity log did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStored {
    /// Stored.
    New,
    /// The same part was there (a retry).
    Duplicate,
    /// The page load's log was started with another token.
    OtherToken,
}

/// The storage failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

/// Where documents go: D1 in the Worker, memory in the tests.
#[allow(async_fn_in_trait)] // One thread in the Worker; no `Send` bound.
pub trait Store {
    /// Stores a test report, or finds the same one, and lets `token`
    /// withdraw it.
    async fn add_test_report(
        &self,
        report: &NewTestReport,
        received: Date,
        token: &TokenHash,
    ) -> Result<Stored, StoreError>;
    /// Deletes a test report if `token` sent it; whether it did.
    async fn delete_test_report(&self, id: i64, token: &TokenHash) -> Result<bool, StoreError>;
    /// Stores a part of a page load's log. The first part names the token
    /// every later part, and the withdrawal, must carry.
    async fn add_log_part(
        &self,
        part: &LogPart,
        received: Date,
        token: &TokenHash,
    ) -> Result<PartStored, StoreError>;
    /// Deletes a page load's log if `token` started it; whether it did.
    async fn delete_log(&self, session: &str, token: &TokenHash) -> Result<bool, StoreError>;
}

const TEST_REPORTS: &str = "/v1/test-reports";
const ACTIVITY_LOGS: &str = "/v1/activity-logs";

/// Answers a request.
pub async fn handle(request: &Request<'_>, config: &Config, store: &impl Store) -> Response {
    let allowed = match request.origin {
        Some(origin) if !config.allows(origin) => {
            return finish(
                Response::error(403, "this origin may not call the server"),
                None,
            );
        }
        origin => origin,
    };
    let response = route(request, store).await;
    finish(response, allowed)
}

/// An answer the Worker gives before [`handle`] (too many requests, too
/// large a body), with the headers of every answer.
pub fn refuse(status: u16, message: &str, origin: Option<&str>, config: &Config) -> Response {
    let allowed = origin.filter(|origin| config.allows(origin));
    finish(Response::error(status, message), allowed)
}

/// Adds the headers every answer carries.
fn finish(mut response: Response, origin: Option<&str>) -> Response {
    if let Some(origin) = origin {
        response
            .headers
            .push(("Access-Control-Allow-Origin", origin.to_owned()));
        response.headers.push(("Vary", "Origin".to_owned()));
    }
    response
        .headers
        .push(("Cache-Control", "no-store".to_owned()));
    response
        .headers
        .push(("X-Content-Type-Options", "nosniff".to_owned()));
    response
}

async fn route(request: &Request<'_>, store: &impl Store) -> Response {
    let path = request.path;
    let (collection, item) = if let Some(rest) = path.strip_prefix(TEST_REPORTS) {
        (TEST_REPORTS, rest)
    } else if let Some(rest) = path.strip_prefix(ACTIVITY_LOGS) {
        (ACTIVITY_LOGS, rest)
    } else if path == "/" {
        return match request.method {
            "GET" | "HEAD" => Response::json(
                200,
                json!({ "service": SERVICE, "version": env!("CARGO_PKG_VERSION") }),
            ),
            _ => method_not_allowed("GET"),
        };
    } else {
        return Response::error(404, "no such resource");
    };
    let item = match item {
        "" => None,
        rest => match rest.strip_prefix('/') {
            Some(id) if !id.is_empty() && !id.contains('/') => Some(id),
            _ => return Response::error(404, "no such resource"),
        },
    };
    match (request.method, item) {
        ("OPTIONS", None) => preflight("POST"),
        ("OPTIONS", Some(_)) => preflight("DELETE"),
        ("POST", None) if collection == TEST_REPORTS => post_test_report(request, store).await,
        ("POST", None) => post_log_part(request, store).await,
        ("DELETE", Some(id)) if collection == TEST_REPORTS => {
            delete_test_report(request, id, store).await
        }
        ("DELETE", Some(session)) => delete_log(request, session, store).await,
        (_, None) => method_not_allowed("POST, OPTIONS"),
        (_, Some(_)) => method_not_allowed("DELETE, OPTIONS"),
    }
}

fn method_not_allowed(allow: &str) -> Response {
    let mut response = Response::error(405, "method not allowed");
    response.headers.push(("Allow", allow.to_owned()));
    response
}

fn preflight(methods: &str) -> Response {
    let mut response = Response::empty(204);
    response.headers.extend([
        ("Access-Control-Allow-Methods", methods.to_owned()),
        (
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type".to_owned(),
        ),
        ("Access-Control-Max-Age", "86400".to_owned()),
    ]);
    response
}

fn unauthorized() -> Response {
    let mut response = Response::error(
        401,
        "a deletion token is required: Authorization: Bearer <32 to 64 hexadecimal digits>",
    );
    response
        .headers
        .push(("WWW-Authenticate", "Bearer".to_owned()));
    response
}

/// The cause stays out of the answer: it may quote data.
fn storage_failed(_: StoreError) -> Response {
    Response::error(500, "the storage failed; try again later")
}

/// Checks a POST: JSON, a token.
fn posted(request: &Request<'_>) -> Result<TokenHash, Response> {
    let json = request.content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
    });
    if !json {
        return Err(Response::error(
            415,
            "send JSON (Content-Type: application/json)",
        ));
    }
    token_hash(request.authorization).ok_or_else(unauthorized)
}

async fn post_test_report(request: &Request<'_>, store: &impl Store) -> Response {
    let token = match posted(request) {
        Ok(token) => token,
        Err(response) => return response,
    };
    let report = match NewTestReport::from_json(request.body) {
        Ok(report) => report,
        Err(response) => return response,
    };
    match store.add_test_report(&report, request.today, &token).await {
        Ok(stored) => Response::json(
            if stored.new { 201 } else { 200 },
            json!({ "id": stored.id }),
        ),
        Err(error) => storage_failed(error),
    }
}

async fn delete_test_report(request: &Request<'_>, id: &str, store: &impl Store) -> Response {
    let Some(token) = token_hash(request.authorization) else {
        return unauthorized();
    };
    let Some(id) = id.parse::<i64>().ok().filter(|id| *id > 0) else {
        return Response::error(404, "no such test report");
    };
    match store.delete_test_report(id, &token).await {
        Ok(true) => Response::empty(204),
        Ok(false) => Response::error(404, "no test report sent with this token"),
        Err(error) => storage_failed(error),
    }
}

async fn post_log_part(request: &Request<'_>, store: &impl Store) -> Response {
    let token = match posted(request) {
        Ok(token) => token,
        Err(response) => return response,
    };
    if request.body.len() > MAX_PART_BYTES {
        return Response::error(413, format!("a part holds {MAX_PART_BYTES} bytes at most"));
    }
    let part = match LogPart::from_json(request.body) {
        Ok(part) => part,
        Err(error) => return Response::error(400, error.0),
    };
    let body = json!({ "session": part.session, "part": part.part });
    match store.add_log_part(&part, request.today, &token).await {
        Ok(PartStored::New) => Response::json(201, body),
        Ok(PartStored::Duplicate) => Response::json(200, body),
        Ok(PartStored::OtherToken) => {
            Response::error(403, "this page load's log was started with another token")
        }
        Err(error) => storage_failed(error),
    }
}

async fn delete_log(request: &Request<'_>, session: &str, store: &impl Store) -> Response {
    let Some(token) = token_hash(request.authorization) else {
        return unauthorized();
    };
    if !is_session_id(session) {
        return Response::error(404, "no such activity log");
    }
    match store.delete_log(session, &token).await {
        Ok(true) => Response::empty(204),
        Ok(false) => Response::error(404, "no activity log sent with this token"),
        Err(error) => storage_failed(error),
    }
}

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};
    use std::collections::HashMap;

    use serde_json::{Value, json};

    use super::*;
    use crate::activity::LOG_FORMAT;

    /// The store, in memory, with D1's rules (`migrations/`).
    #[derive(Default)]
    struct Memory {
        reports: RefCell<Vec<(i64, NewTestReport, Vec<TokenHash>)>>,
        logs: RefCell<HashMap<String, (TokenHash, Vec<u32>)>>,
    }

    impl Store for Memory {
        async fn add_test_report(
            &self,
            report: &NewTestReport,
            _: Date,
            token: &TokenHash,
        ) -> Result<Stored, StoreError> {
            let mut reports = self.reports.borrow_mut();
            if let Some((id, _, tokens)) = reports
                .iter_mut()
                .find(|(_, stored, _)| stored.sha256 == report.sha256)
            {
                if !tokens.contains(token) {
                    tokens.push(token.clone());
                }
                return Ok(Stored {
                    id: *id,
                    new: false,
                });
            }
            let id = reports.last().map_or(1, |(id, _, _)| id + 1);
            reports.push((id, report.clone(), vec![token.clone()]));
            Ok(Stored { id, new: true })
        }

        async fn delete_test_report(&self, id: i64, token: &TokenHash) -> Result<bool, StoreError> {
            let mut reports = self.reports.borrow_mut();
            let before = reports.len();
            reports.retain(|(stored, _, tokens)| *stored != id || !tokens.contains(token));
            Ok(reports.len() < before)
        }

        async fn add_log_part(
            &self,
            part: &LogPart,
            _: Date,
            token: &TokenHash,
        ) -> Result<PartStored, StoreError> {
            let mut logs = self.logs.borrow_mut();
            let (owner, parts) = logs
                .entry(part.session.clone())
                .or_insert_with(|| (token.clone(), Vec::new()));
            if owner != token {
                return Ok(PartStored::OtherToken);
            }
            if parts.contains(&part.part) {
                return Ok(PartStored::Duplicate);
            }
            parts.push(part.part);
            Ok(PartStored::New)
        }

        async fn delete_log(&self, session: &str, token: &TokenHash) -> Result<bool, StoreError> {
            let mut logs = self.logs.borrow_mut();
            let owned = logs.get(session).is_some_and(|(owner, _)| owner == token);
            if owned {
                logs.remove(session);
            }
            Ok(owned)
        }
    }

    /// Runs a future whose store answers at once.
    fn run<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("the memory store never waits"),
        }
    }

    const SITE: &str = "https://anygamepad.github.io";
    const TOKEN: &str = "Bearer 0123456789abcdef0123456789abcdef";
    const OTHER: &str = "Bearer fedcba9876543210fedcba9876543210";
    const EXAMPLE: &str = include_str!("../../../data/test-reports/example.json");

    fn config() -> Config {
        Config::from_origins("https://anygamepad.github.io, http://127.0.0.1:4318/")
    }

    fn request<'a>(
        method: &'a str,
        path: &'a str,
        authorization: Option<&'a str>,
        body: &'a str,
    ) -> Request<'a> {
        Request {
            method,
            path,
            origin: Some(SITE),
            authorization,
            content_type: (method == "POST").then_some("application/json; charset=utf-8"),
            body,
            today: "2026-11-02".parse().unwrap(),
        }
    }

    fn send(store: &Memory, request: Request<'_>) -> (u16, Value) {
        let response = run(handle(&request, &config(), store));
        let body = response
            .body
            .as_deref()
            .map_or(Value::Null, |body| serde_json::from_str(body).unwrap());
        (response.status, body)
    }

    fn header(response: &Response, name: &str) -> Option<String> {
        response
            .headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone())
    }

    fn part(session: &str, number: u32) -> String {
        json!({
            "format": LOG_FORMAT,
            "library": "0.0.5",
            "page": "tester",
            "environment": null,
            "session": session,
            "part": number,
            "day": "2026-11-02",
            "dropped_input_events": 0,
            "events": [{ "t": 1, "type": "session" }],
        })
        .to_string()
    }

    #[test]
    fn cors_allows_the_site_only() {
        let store = Memory::default();
        let preflight = request("OPTIONS", TEST_REPORTS, None, "");
        let response = run(handle(&preflight, &config(), &store));
        assert_eq!(response.status, 204);
        assert_eq!(
            header(&response, "Access-Control-Allow-Origin").as_deref(),
            Some(SITE)
        );
        assert_eq!(
            header(&response, "Access-Control-Allow-Methods").as_deref(),
            Some("POST")
        );
        assert_eq!(
            header(&response, "Access-Control-Allow-Headers").as_deref(),
            Some("Authorization, Content-Type")
        );
        let local = Request {
            origin: Some("http://127.0.0.1:4318"),
            ..preflight
        };
        assert_eq!(run(handle(&local, &config(), &store)).status, 204);
        let other = Request {
            origin: Some("https://example.org"),
            ..preflight
        };
        let response = run(handle(&other, &config(), &store));
        assert_eq!(response.status, 403);
        assert_eq!(header(&response, "Access-Control-Allow-Origin"), None);
        // Other apps send no origin.
        let app = Request {
            origin: None,
            ..request("GET", "/", None, "")
        };
        let response = run(handle(&app, &config(), &store));
        assert_eq!(response.status, 200);
        assert_eq!(header(&response, "Access-Control-Allow-Origin"), None);
        assert_eq!(
            header(&response, "Cache-Control").as_deref(),
            Some("no-store")
        );
    }

    #[test]
    fn routes() {
        let store = Memory::default();
        let (status, body) = send(&store, request("GET", "/", None, ""));
        assert_eq!((status, body["service"].as_str()), (200, Some(SERVICE)));
        assert_eq!(
            send(&store, request("GET", "/v2/test-reports", None, "")).0,
            404
        );
        assert_eq!(
            send(&store, request("GET", "/v1/test-reports/1/2", None, "")).0,
            404
        );
        assert_eq!(send(&store, request("GET", TEST_REPORTS, None, "")).0, 405);
        assert_eq!(
            send(
                &store,
                request("POST", "/v1/activity-logs/abc", Some(TOKEN), "")
            )
            .0,
            405
        );
        assert_eq!(
            send(
                &store,
                request("DELETE", "/v1/test-reports/0", Some(TOKEN), "")
            )
            .0,
            404
        );
        assert_eq!(
            send(
                &store,
                request("DELETE", "/v1/activity-logs/zz", Some(TOKEN), "")
            )
            .0,
            404
        );
    }

    #[test]
    fn test_reports_are_stored_once_and_withdrawn_by_their_tokens() {
        let store = Memory::default();
        // Not JSON, no token, not a test report.
        let plain = Request {
            content_type: Some("text/plain"),
            ..request("POST", TEST_REPORTS, Some(TOKEN), EXAMPLE)
        };
        assert_eq!(send(&store, plain).0, 415);
        assert_eq!(
            send(&store, request("POST", TEST_REPORTS, None, EXAMPLE)).0,
            401
        );
        assert_eq!(
            send(
                &store,
                request("POST", TEST_REPORTS, Some("Bearer short"), EXAMPLE)
            )
            .0,
            401
        );
        let (status, body) = send(&store, request("POST", TEST_REPORTS, Some(TOKEN), "{}"));
        assert_eq!(status, 400, "{body}");

        let (status, body) = send(&store, request("POST", TEST_REPORTS, Some(TOKEN), EXAMPLE));
        assert_eq!((status, &body), (201, &json!({ "id": 1 })));
        {
            let reports = store.reports.borrow();
            let stored = &reports[0].1;
            assert_eq!(stored.model, "sony/dualsense");
            assert_eq!(
                (stored.mode.as_str(), stored.transport.as_str()),
                ("ps5", "usb")
            );
            assert_eq!(stored.unit.as_deref(), Some("DS5-01"));
            assert_eq!(
                (stored.app.as_str(), stored.os.as_str()),
                ("web", "windows")
            );
            assert_eq!(
                (stored.browser.as_deref(), stored.api.as_str()),
                (Some("blink"), "gamepad_api")
            );
            assert_eq!(stored.plan, "quick");
            assert_eq!(stored.created.to_string(), "2026-10-14");
            // Stored re-serialized: compact, the same document.
            assert!(!stored.json.contains('\n'));
            let sent: Value = serde_json::from_str(EXAMPLE).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&stored.json).unwrap(), sent);
            assert_eq!(stored.sha256, sha256_hex(stored.json.as_bytes()));
        }
        // The same report again, indented or not, from another browser.
        let compact = serde_json::from_str::<Value>(EXAMPLE).unwrap().to_string();
        assert_eq!(
            send(&store, request("POST", TEST_REPORTS, Some(OTHER), &compact)),
            (200, json!({ "id": 1 }))
        );
        assert_eq!(store.reports.borrow().len(), 1);

        // Withdrawn by a token that sent it, only.
        let third = "Bearer 00000000000000000000000000000000";
        assert_eq!(
            send(
                &store,
                request("DELETE", "/v1/test-reports/1", Some(third), "")
            )
            .0,
            404
        );
        assert_eq!(
            send(&store, request("DELETE", "/v1/test-reports/1", None, "")).0,
            401
        );
        assert_eq!(
            send(
                &store,
                request("DELETE", "/v1/test-reports/1", Some(OTHER), "")
            ),
            (204, Value::Null)
        );
        assert!(store.reports.borrow().is_empty());
        assert_eq!(
            send(
                &store,
                request("DELETE", "/v1/test-reports/1", Some(TOKEN), "")
            )
            .0,
            404
        );
    }

    #[test]
    fn activity_logs_belong_to_their_first_token() {
        let store = Memory::default();
        let session = "0123456789abcdef0123456789abcdef";
        let first = part(session, 0);
        assert_eq!(
            send(&store, request("POST", ACTIVITY_LOGS, None, &first)).0,
            401
        );
        let (status, body) = send(&store, request("POST", ACTIVITY_LOGS, Some(TOKEN), &first));
        assert_eq!(
            (status, body),
            (201, json!({ "session": session, "part": 0 }))
        );
        // A retry, the next part, and someone else's.
        assert_eq!(
            send(&store, request("POST", ACTIVITY_LOGS, Some(TOKEN), &first)).0,
            200
        );
        assert_eq!(
            send(
                &store,
                request("POST", ACTIVITY_LOGS, Some(TOKEN), &part(session, 1))
            )
            .0,
            201
        );
        assert_eq!(
            send(
                &store,
                request("POST", ACTIVITY_LOGS, Some(OTHER), &part(session, 2))
            )
            .0,
            403
        );
        let (status, body) = send(
            &store,
            request("POST", ACTIVITY_LOGS, Some(TOKEN), "{\"format\":1}"),
        );
        assert_eq!(status, 400, "{body}");
        let big = first.clone() + &" ".repeat(MAX_PART_BYTES);
        assert_eq!(
            send(&store, request("POST", ACTIVITY_LOGS, Some(TOKEN), &big)).0,
            413
        );

        let path = format!("{ACTIVITY_LOGS}/{session}");
        assert_eq!(
            send(&store, request("DELETE", &path, Some(OTHER), "")).0,
            404
        );
        assert_eq!(
            send(&store, request("DELETE", &path, Some(TOKEN), "")).0,
            204
        );
        assert!(store.logs.borrow().is_empty());
    }

    #[test]
    fn rate_limit_keys_hide_the_address() {
        let today: Date = "2026-11-02".parse().unwrap();
        let tomorrow: Date = "2026-11-03".parse().unwrap();
        let key = rate_limit_key("secret", today, "203.0.113.7");
        assert_eq!(key.len(), 32);
        assert!(!key.contains("203"));
        assert_eq!(key, rate_limit_key("secret", today, "203.0.113.7"));
        assert_ne!(key, rate_limit_key("secret", tomorrow, "203.0.113.7"));
        assert_ne!(key, rate_limit_key("other", today, "203.0.113.7"));
        assert_ne!(key, rate_limit_key("secret", today, "203.0.113.8"));
    }

    #[test]
    fn tokens() {
        assert!(token_hash(Some(&TOKEN.replace("0123", "ABCD"))).is_none());
        assert!(token_hash(Some("0123456789abcdef0123456789abcdef")).is_none());
        assert!(token_hash(Some(&format!("Bearer {}", "a".repeat(65)))).is_none());
        let hash = token_hash(Some(TOKEN)).unwrap();
        assert_eq!(hash.0.len(), 64);
        assert_ne!(hash, token_hash(Some(OTHER)).unwrap());
    }
}
