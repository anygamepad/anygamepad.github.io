-- The sync server's tables (roadmap P2, ADR 0009). Applied with
-- `npx wrangler d1 migrations apply anygamepad-sync` (add `--local` for
-- `wrangler dev`). Days are UTC, `YYYY-MM-DD`; nothing holds a time of day,
-- an address or anything about a person.

-- Test reports (H1), as their compact JSON re-serialized by
-- anygamepad-report, stored once per content (the SHA-256 of that JSON),
-- next to the columns curation (P4) queries by.
CREATE TABLE test_reports (
  id INTEGER PRIMARY KEY,
  sha256 TEXT NOT NULL UNIQUE,
  -- The day it arrived, and the day it was made.
  received TEXT NOT NULL,
  created TEXT NOT NULL,
  -- `web` or `desktop`.
  app TEXT NOT NULL,
  -- What the person confirmed: catalog model, its mode, the transport, and
  -- the unit's code in their inventory.
  model TEXT NOT NULL,
  mode TEXT NOT NULL,
  transport TEXT NOT NULL,
  unit TEXT,
  os TEXT NOT NULL,
  -- The browser engine, on the web.
  browser TEXT,
  -- The API the session read the device with (`gamepad_api`, `webhid`, ...).
  api TEXT NOT NULL,
  plan TEXT NOT NULL,
  pass INTEGER NOT NULL,
  fail INTEGER NOT NULL,
  report TEXT NOT NULL
);

CREATE INDEX test_reports_by_model ON test_reports (model, mode, transport);

-- Who may withdraw a test report: the SHA-256 of the deletion token of
-- everyone who sent it.
CREATE TABLE test_report_tokens (
  report_id INTEGER NOT NULL REFERENCES test_reports (id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL,
  PRIMARY KEY (report_id, token_hash)
);

-- A page load's activity log: its ID (random, made by the page), the token
-- its first part came with, which every later part and the withdrawal must
-- carry, and what the page said about itself.
CREATE TABLE activity_sessions (
  session TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL,
  received TEXT NOT NULL,
  -- `tester` or `test`.
  page TEXT NOT NULL,
  library TEXT NOT NULL,
  -- `{"browser": {"engine": ..., "major_version": ...}, "os": ...}`, or null.
  environment TEXT
);

CREATE INDEX activity_sessions_by_day ON activity_sessions (received);

-- The parts of each log: events as a JSON array, scrubbed by the server.
CREATE TABLE activity_log_parts (
  session TEXT NOT NULL REFERENCES activity_sessions (session) ON DELETE CASCADE,
  part INTEGER NOT NULL,
  received TEXT NOT NULL,
  -- The day the page says it sent it.
  day TEXT NOT NULL,
  dropped_input_events INTEGER NOT NULL,
  event_count INTEGER NOT NULL,
  -- Texts the server's scrub changed.
  redactions INTEGER NOT NULL,
  events TEXT NOT NULL,
  PRIMARY KEY (session, part)
);
