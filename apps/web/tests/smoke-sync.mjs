// End-to-end smoke test of sending (the settings, src/sync.ts) to the real
// sync server (apps/server, ADR 0009), on the Workers runtime with a fresh
// local D1:
//
// - builds the site naming the local Worker (ANYGAMEPAD_SYNC_URL) into a
//   temporary folder, and serves it on an origin the Worker allows;
// - in the test mode, turns sending on and runs the Quick plan on a fake
//   Xbox 360 controller, then saves the report;
// - reads D1: the test report, stored as the page sent it with its
//   columns, and the page load's log, scrubbed; then withdraws both from
//   the settings and finds D1 empty.
//
// Needs the WebAssembly bindings (`npm run wasm`), `npm ci` in apps/server,
// cargo with the wasm32 target (wrangler builds the Worker with
// worker-build), and CHROME_PATH as in smoke.mjs.

import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { fakeGamepads } from "./fakes.mjs";

const executablePath = process.env.CHROME_PATH;
if (!executablePath) {
  console.error("Set CHROME_PATH to a Chrome or Chromium binary.");
  process.exit(2);
}

const WORKER_PORT = 8788;
const SITE_PORT = 4319;
const SITE = `http://127.0.0.1:${SITE_PORT}`;
const web = fileURLToPath(new URL("..", import.meta.url));
const server = fileURLToPath(new URL("../../server/", import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), "anygamepad-sync-"));
const state = join(scratch, "state");
const dist = join(scratch, "dist");

const STANDARD = [
  "south", "east", "west", "north", "left_shoulder", "right_shoulder", "left_trigger",
  "right_trigger", "select", "start", "left_stick", "right_stick", "dpad_up", "dpad_down",
  "dpad_left", "dpad_right", "home",
];
const XBOX360 = {
  id: "Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)",
  mapping: "standard",
  buttons: Array.from({ length: 17 }, () => 0),
  axes: [0, 0, 0, 0],
  rumble: true,
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (what, read, done, ms = 15000) => {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = await read();
    if (done(value)) return value;
    if (Date.now() > deadline) assert.fail(`timed out waiting for ${what}; last read: ${JSON.stringify(value)}`);
    await sleep(50);
  }
};

const wrangler = (...args) =>
  execFileSync("npx", ["wrangler", ...args], { cwd: server, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
/** Rows of a query on the local D1. */
const query = (sql) =>
  JSON.parse(wrangler("d1", "execute", "anygamepad-sync", "--local", "--persist-to", state, "--json", "--command", sql))[0]
    .results;

let worker;
let site;
let browser;
try {
  // The Worker, on a fresh database.
  wrangler("d1", "migrations", "apply", "anygamepad-sync", "--local", "--persist-to", state);
  worker = spawn(
    "npx",
    [
      "wrangler", "dev", "--ip", "127.0.0.1", "--port", String(WORKER_PORT), "--persist-to", state,
      "--var", `ALLOWED_ORIGINS:${SITE}`, "--var", "RATE_LIMIT_KEY:smoke-test",
    ],
    { cwd: server, detached: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let output = "";
  worker.stdout.on("data", (chunk) => (output += chunk));
  worker.stderr.on("data", (chunk) => (output += chunk));
  await until("the Worker", () => output, (text) => text.includes("Ready on") || worker.exitCode !== null, 600_000);
  assert.equal(worker.exitCode, null, output);

  // The site, naming it.
  process.env.ANYGAMEPAD_SYNC_URL = `http://127.0.0.1:${WORKER_PORT}/`;
  const { build, preview } = await import("vite");
  await build({ root: web, logLevel: "warn", build: { outDir: dist, emptyOutDir: true } });
  site = await preview({ root: web, logLevel: "warn", build: { outDir: dist }, preview: { host: "127.0.0.1", port: SITE_PORT, strictPort: true } });

  browser = await chromium.launch({ executablePath });
  const page = await browser.newPage();
  const problems = [];
  page.on("pageerror", (error) => problems.push(`page error: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console.error: ${message.text()}`);
  });
  const sentReports = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/v1/test-reports")) sentReports.push(request.postDataJSON());
  });
  await page.addInitScript(fakeGamepads);
  await page.goto(`${SITE}/test/`, { waitUntil: "networkidle" });
  const stage = () => page.getAttribute(".test-detail", "data-stage");
  const prompt = () => page.getAttribute(".test-prompt", "data-prompt").catch(() => null);
  const setPad = (pad) => page.evaluate((p) => window.__setGamepads(p ? [p] : []), pad);
  await until("the list", stage, (s) => s === "list");

  // Sending on: the log so far reaches D1.
  await page.click(".settings-btn");
  await page.check("[data-sending]");
  await until("the first part", () => page.textContent("[data-sent]").catch(() => ""), (t) => /1 part of/.test(t));
  await page.keyboard.press("Escape");

  // The Quick plan on an Xbox 360 controller.
  await setPad(XBOX360);
  await until("detection", stage, (s) => s === "detected");
  await page.click(".test-detected >> text=Confirm");
  await page.click('[data-plan="quick"]');
  await until("the session", stage, (s) => s === "running");
  for (let guard = 0; guard < 200 && (await stage()) === "running"; guard += 1) {
    const key = await prompt();
    const [kind, detail] = key?.split(":") ?? [];
    if (kind === "press") {
      await setPad({ ...XBOX360, buttons: XBOX360.buttons.map((v, i) => (i === STANDARD.indexOf(detail) ? 1 : v)) });
      await sleep(150);
      await setPad(XBOX360);
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind === "ask") {
      await page.click('[data-answer="yes"]');
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind) {
      await page.click("text=Skip this step");
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else {
      await sleep(30);
    }
  }
  await until("the summary", stage, (s) => s === "summary");
  await page.click("[data-save]");
  await until("the report sent", () => page.textContent("[data-saved]").catch(() => ""), (t) => /Sent to the anygamepad database\.$/.test(t));

  // D1 holds the report as sent, with its columns.
  const [row, ...others] = query("SELECT model, mode, transport, unit, os, browser, api, plan, pass, fail, report FROM test_reports");
  assert.deepEqual(others, []);
  assert.deepEqual(
    { ...row, report: undefined },
    { model: "microsoft/xbox-360", mode: "xinput", transport: "usb", unit: "X360-01", os: row.os, browser: "blink", api: "gamepad_api", plan: "quick", pass: row.pass, fail: 0, report: undefined },
  );
  const passed = sentReports[0].results.filter((result) => result.status === "pass").length;
  assert.ok(passed > 0 && row.pass === passed, `${row.pass} steps passed, ${passed} in the report`);
  assert.deepEqual(JSON.parse(row.report), sentReports[0]);
  assert.deepEqual(query("SELECT count(*) AS n FROM test_report_tokens"), [{ n: 1 }]);

  // And the page load's log, joined by its ID, scrubbed, without a time.
  // Turning sending off and on sends what is new at once.
  await page.click(".settings-btn");
  await page.uncheck("[data-sending]");
  await page.check("[data-sending]");
  await until("the latest part", () => page.textContent("[data-sent]").catch(() => ""), (t) => /[2-9] parts of/.test(t));
  await page.keyboard.press("Escape");
  const [session] = query("SELECT session, page, library, environment FROM activity_sessions");
  assert.equal(session.page, "test");
  assert.equal(JSON.parse(session.environment).browser.engine, "blink");
  const parts = query("SELECT part, event_count, redactions, events FROM activity_log_parts ORDER BY part");
  assert.equal(parts[0].part, 0);
  const types = parts.flatMap((part) => JSON.parse(part.events).map((event) => event.type));
  assert.deepEqual(types.slice(0, 3), ["session", "page_visible", "sending"]);
  assert.ok(types.includes("gamepad_added") && types.includes("test_report_sent"), types.join(" "));
  assert.ok(parts.every((part) => part.redactions === 0));

  // Withdrawn from the settings: nothing is left.
  await page.click(".settings-btn");
  await page.click("[data-withdraw]");
  await until("the withdrawal", () => page.textContent("[data-withdrawn]").catch(() => ""), (t) => /2 of 2/.test(t));
  assert.deepEqual(
    query(
      "SELECT (SELECT count(*) FROM test_reports) AS reports, (SELECT count(*) FROM test_report_tokens) AS tokens, " +
        "(SELECT count(*) FROM activity_sessions WHERE session = '" + session.session + "') AS logs, " +
        "(SELECT count(*) FROM activity_log_parts WHERE session = '" + session.session + "') AS parts",
    ),
    [{ reports: 0, tokens: 0, logs: 0, parts: 0 }],
  );
  assert.deepEqual(problems, []);
  console.log("sync smoke test passed (chromium, local Worker and D1)");
} finally {
  await browser?.close();
  if (site) await new Promise((resolve) => site.httpServer.close(resolve));
  if (worker && worker.exitCode === null) process.kill(-worker.pid, "SIGTERM");
  rmSync(scratch, { recursive: true, force: true });
}
