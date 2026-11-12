// Smoke test of the test mode (roadmap task H4) on the built site (dist/):
//
// - The test mode is a tab of the tester, at /test/;
//   its WebAssembly module (W9) is fetched there only.
// - Sending (the settings), to a fake database named by the page's
//   `anygamepad-sync` tag, as a build with ANYGAMEPAD_SYNC_URL names it:
//   once turned on, the activity log goes in parts and the saved test
//   report as downloaded; the choice survives a reload.
// - Gamepad API: a wired Xbox 360 controller connected without picking it is
//   detected; the Full plan runs with a person simulated from the prompts
//   (every button, sticks, triggers, rate, vibration answered yes, the
//   player lights the API cannot drive, unplugging and plugging back); the
//   summary, the review of the report's texts,
//   saving in the browser, the "Tested here" mark, the unit's code (proposed
//   for a pad this browser does not know, checked, kept), and the downloaded report
//   (format, configuration, results, evidence).
// - The pad drawn live in the detected and running stages, the control
//   each step asks for marked; the history of the tests saved in this
//   browser, with download and delete.
// - WebHID (Chromium only): a DualSense whose Gamepad API name hides its
//   USB IDs (as on macOS) is detected as the generic model, with an offer
//   to connect it through WebHID; picked in the chooser, the Bluetooth
//   DualSense, already in its full report, is detected again as itself;
//   its firmware check answers as Sony's pads do; the Quick plan runs on
//   its decoded reports, which the report keeps as evidence.
//
// BROWSER and CHROME_PATH as in smoke.mjs.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { chromium, firefox, webkit } from "playwright-core";
import { preview } from "vite";
import { fakeGamepads, fakeHid } from "./fakes.mjs";

const BROWSERS = { chromium, firefox, webkit };
const browserName = process.env.BROWSER ?? "chromium";
const type = BROWSERS[browserName];
if (!type) {
  console.error(`BROWSER must be one of ${Object.keys(BROWSERS).join(", ")}.`);
  process.exit(2);
}
const executablePath = process.env.CHROME_PATH;
if (browserName === "chromium" && !executablePath) {
  console.error("Set CHROME_PATH to a Chrome or Chromium binary.");
  process.exit(2);
}

// The standard Gamepad API index of each canonical button.
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

// The DualSense's report bits (byte, mask) of each canonical button; the
// d-pad is a hat in the low nibble of the first byte (8: centred).
const DUALSENSE_BITS = {
  west: [0, 0x18], south: [0, 0x28], east: [0, 0x48], north: [0, 0x88],
  dpad_up: [0, 0x00], dpad_right: [0, 0x02], dpad_down: [0, 0x04], dpad_left: [0, 0x06],
  left_shoulder: [1, 0x01], right_shoulder: [1, 0x02], left_trigger: [1, 0x04], right_trigger: [1, 0x08],
  select: [1, 0x10], start: [1, 0x20], left_stick: [1, 0x40], right_stick: [1, 0x80],
  home: [2, 0x01], touchpad_click: [2, 0x02], mute: [2, 0x04],
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const server = await preview({ preview: { host: "127.0.0.1", port: 4318 }, logLevel: "warn" });
const url = server.resolvedUrls.local[0];
const browser = await type.launch(browserName === "chromium" ? { executablePath } : {});
const problems = [];
try {
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      problems.push(`console.${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => problems.push(`page error: ${error.message}`));
  const fetched = [];
  page.on("request", (request) => fetched.push(request.url()));
  await page.addInitScript(fakeGamepads);
  await page.addInitScript(fakeHid);
  // The fake database: the test mode's page names it, as a build with
  // ANYGAMEPAD_SYNC_URL would, and it stores what it gets.
  const uploads = [];
  await page.route(`${url}test/`, async (route) => {
    const response = await route.fetch();
    const html = (await response.text()).replace("<head>", '<head><meta name="anygamepad-sync" content="/__sync/">');
    await route.fulfill({ response, body: html });
  });
  await page.route(`${url}__sync/**`, async (route) => {
    const request = route.request();
    const upload = {
      method: request.method(),
      path: new URL(request.url()).pathname,
      body: request.method() === "POST" ? request.postDataJSON() : null,
      headers: await request.allHeaders(),
    };
    uploads.push(upload);
    const tests = upload.path === "/__sync/v1/test-reports" && upload.method === "POST";
    await route.fulfill(
      upload.method === "DELETE"
        ? { status: 204 }
        : { status: 201, contentType: "application/json", body: JSON.stringify(tests ? { id: 7 } : {}) },
    );
  });
  const uploaded = (path) =>
    uploads.filter((upload) => upload.method === "POST" && upload.path === `/__sync/v1/${path}`);
  const bearer = (upload) => upload.headers.authorization?.match(/^Bearer ([0-9a-f]{32})$/)?.[1];
  await page.goto(url, { waitUntil: "networkidle" });

  const setPad = (pad) => page.evaluate((p) => window.__setGamepads(p ? [p] : []), pad);
  const stage = () => page.getAttribute(".test-detail", "data-stage");
  const prompt = () => page.getAttribute(".test-prompt", "data-prompt").catch(() => null);
  const until = async (what, read, done, ms = 8000) => {
    const deadline = Date.now() + ms;
    for (;;) {
      const value = await read();
      if (done(value)) return value;
      if (Date.now() > deadline) assert.fail(`timed out waiting for ${what}; last read: ${JSON.stringify(value)}`);
      await sleep(25);
    }
  };

  const testingModule = () => fetched.filter((u) => /anygamepad_wasm_testing.*\.wasm/.test(u));
  assert.ok(fetched.some((u) => /anygamepad_wasm_bg.*\.wasm/.test(u)), "the tester's module loads");
  assert.deepEqual(testingModule(), [], "the tester does not load the test mode's module");
  // The test mode is a tab: it loads its module in place.
  await page.click("[data-tab=test]");
  await page.waitForURL(/\/test\/$/);
  assert.equal(await page.title(), "anygamepad: test mode");
  await until("the list", stage, (s) => s === "list");
  assert.equal(testingModule().length, 1, "the test mode's tab loads its module");
  assert.equal(await page.getAttribute("[data-tab=tester]", "href"), "/");
  // Its own address, which this test's fake database is named on.
  await page.goto(`${url}test/`, { waitUntil: "networkidle" });
  await until("the list", stage, (s) => s === "list");

  // Sending, turned on in the settings: the log so far goes at once.
  assert.deepEqual(uploads, [], "nothing is sent before the person turns it on");
  await page.click(".settings-btn");
  assert.equal(await page.isEnabled("[data-sending]"), true);
  await page.check("[data-sending]");
  const firstPart = (await until("the first part of the log", () => uploaded("activity-logs"), (u) => u.length > 0))[0];
  assert.equal(firstPart.body.format, "anygamepad-activity-log-v1");
  assert.equal(firstPart.body.page, "test");
  assert.equal(firstPart.body.part, 0);
  assert.match(firstPart.body.day, /^\d{4}-\d{2}-\d{2}$/);
  assert.match(firstPart.body.session, /^[0-9a-f]{32}$/);
  assert.equal(firstPart.body.started_at, undefined, "no time of day");
  assert.deepEqual(
    firstPart.body.events.map((event) => event.type),
    ["session", "page_visible", "sending"],
  );
  assert.equal(firstPart.headers.cookie, undefined);
  assert.ok(bearer(firstPart), "a deletion token");
  assert.equal(firstPart.headers.referer, undefined);
  await until(
    "the count of parts sent",
    async () => (await page.textContent("[data-sent]")).replace(/\s+/g, " ").trim(),
    (text) => text === "Sent since this page loaded: 1 part of the activity log, 0 test reports.",
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".settings-modal", { state: "detached" });
  assert.equal(await page.getAttribute(".settings-btn", "class"), "icon-btn settings-btn on");

  assert.ok((await page.locator(".test-list [data-model]").count()) >= 13);
  // The search filters the catalog.
  await page.fill(".test-search input", "dualshock");
  assert.equal(await page.locator(".test-list [data-model]").count(), 3);
  await page.fill(".test-search input", "");

  // Connecting a pad without picking it: detection.
  await setPad(XBOX360);
  await until("detection", stage, (s) => s === "detected");
  assert.equal(await page.textContent("[data-detected]"), "Microsoft Xbox 360 Controller · XInput · USB cable");
  // Drawn by the next frame once the stage shows.
  await until("the detected pad's drawing", () => page.locator(".test-drawing svg.diagram").count(), (n) => n === 1);
  assert.equal(await page.locator("[data-ids-hidden]").count(), 0, "its USB IDs show");
  // Nothing saved yet: the history is empty.
  assert.equal(await page.locator("[data-history-empty]").count(), 0, "no history while detected");
  // A pad this browser does not know yet gets a new unit code (F4).
  assert.equal(await page.inputValue("[data-unit]"), "X360-01");
  await page.fill("[data-unit]", "not a code!");
  await page.press("[data-unit]", "Tab");
  await until("the code's error", () => page.textContent(".test-unit .error").catch(() => ""), (t) => /letters, digits and hyphens/.test(t));
  await page.click("[data-new-unit]");
  assert.equal(await page.inputValue("[data-unit]"), "X360-01");
  await page.click(".test-detected >> text=Confirm");
  await until("the plans", stage, (s) => s === "plan");
  await page.click('[data-plan="full"]');
  await until("the session", stage, (s) => s === "running");

  // The simulated person: does what each prompt asks.
  const seen = new Set();
  let pad = XBOX360;
  const press = async (index, value = 1) => {
    await setPad({ ...pad, buttons: pad.buttons.map((v, i) => (i === index ? value : v)) });
  };
  for (let guard = 0; guard < 400; guard += 1) {
    if ((await stage()) !== "running") break;
    const key = await prompt();
    if (!key) {
      await sleep(30);
      continue;
    }
    seen.add(key.split(":")[0]);
    const [kind, detail] = key.split(":");
    if (kind === "press" && !seen.has("drawn")) {
      // The drawing marks the control the step asks for.
      seen.add("drawn");
      await until("the marked control", () => page.locator(".test-drawing .target").count(), (n) => n === 1);
    }
    if (kind === "press") {
      await press(STANDARD.indexOf(detail));
      await sleep(150);
      await setPad(pad);
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind === "circle") {
      const first = detail === "left" ? 0 : 2;
      for (let step = 0; step <= 32; step += 1) {
        const angle = (step / 32) * 2 * Math.PI;
        const axes = [0, 0, 0, 0];
        axes[first] = Math.cos(angle);
        axes[first + 1] = -Math.sin(angle);
        await setPad({ ...pad, axes });
        await sleep(30);
      }
      await setPad(pad);
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind === "pull") {
      const index = detail === "left" ? 6 : 7;
      for (let step = 1; step <= 10; step += 1) {
        await press(index, step / 10);
        await sleep(40);
      }
      await setPad(pad);
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind === "move_sticks") {
      for (let step = 0; step < 140 && (await prompt()) === key; step += 1) {
        const angle = step / 5;
        await setPad({ ...pad, axes: [Math.cos(angle), Math.sin(angle), 0, 0] });
        await sleep(30);
      }
      await setPad(pad);
    } else if (kind === "ask") {
      await page.click('[data-answer="yes"]');
      await until(`the end of ${key}`, prompt, (p) => p !== key);
    } else if (kind === "unplug") {
      await setPad(null);
      await until("the reconnect prompt", prompt, (p) => p !== key);
    } else if (kind === "reconnect") {
      pad = XBOX360;
      await setPad(pad);
      await until(`the end of ${key}`, stage, (s) => s !== "running" || true);
      await sleep(200);
    } else {
      await sleep(50);
    }
  }
  await until("the summary", stage, (s) => s === "summary", 15000);
  for (const kind of ["press", "circle", "pull", "move_sticks", "ask", "unplug", "reconnect"]) {
    assert.ok(seen.has(kind), `the session asked to ${kind}`);
  }
  const summary = await page.textContent("[data-summary]");
  // The Gamepad API cannot drive the player lights: that step does not apply.
  assert.match(summary, /^\d+ passed, 0 failed, 0 skipped, 1 not applicable\.$/, summary);
  const haptics = await page.evaluate(() => window.__haptics);
  assert.ok(haptics.some((call) => call.call === "playEffect" && call.strongMagnitude === 1), JSON.stringify(haptics));
  // The review lists the report's texts; nothing is flagged for this pad.
  assert.ok((await page.locator(".test-review li").count()) > 0);
  const flagged = await page.locator(".test-review li.flagged").allTextContents();
  assert.deepEqual(flagged, []);

  // Sending is on: saving sends the report too.
  assert.equal((await page.textContent("[data-save]")).trim(), "Save and send");
  await page.click("[data-save]");
  const saved = await until("the saved note", () => page.textContent("[data-saved]").catch(() => ""), (t) => /Sent/.test(t));
  assert.match(saved, /^Saved in this browser as \S+\.json\. Sent to the anygamepad database\.$/);
  assert.ok(seen.has("drawn"), "the session drew the pad");
  // The history lists it.
  const history = () => page.locator("[data-history] .report-row").allTextContents();
  const rows = await until("the history", history, (h) => h.length === 1);
  assert.match(rows[0], /Microsoft Xbox 360 Controller · XInput · USB cable/);
  assert.match(rows[0], /X360-01 · Full · \w+ \w+ Gamepad API/);
  assert.match(rows[0], /\d+ passed, 0 failed/);
  const [download] = await Promise.all([page.waitForEvent("download"), page.click("[data-download]")]);
  assert.match(download.suggestedFilename(), /^x360-01-xinput-usb-\w+-web-gamepad_api-\d{4}-\d{2}-\d{2}-1\.json$/);
  const report = JSON.parse(await readFile(await download.path(), "utf8"));
  assert.equal(report.format, "anygamepad-test-report");
  assert.equal(report.plan.id, "full");
  assert.equal(report.configuration.start, "connecting");
  // The unit is named, not detected.
  const { unit, ...confirmed } = report.configuration.confirmed;
  assert.equal(unit, "X360-01");
  assert.deepEqual(confirmed, report.configuration.detected);
  assert.equal(report.configuration.confirmed.model, "microsoft/xbox-360");
  const notPassed = report.results.filter((result) => result.status !== "pass");
  assert.deepEqual(
    notPassed.map((result) => [result.step, result.status, result.observed]),
    [["outputs.player-leds", "not_applicable", "the API cannot drive it"]],
  );
  assert.equal(report.results[1].step, "controls.south");
  assert.ok(report.results[1].evidence.frames.length >= 2);
  const circle = report.results.find((result) => result.step === "sticks.left.circle");
  assert.ok(circle.measurements.reach_up > 0.9, JSON.stringify(circle.measurements));
  assert.ok(!/\d{2}:\d{2}:\d{2}/.test(JSON.stringify(report)), "no time of day");
  // What was sent is what was downloaded.
  assert.equal(uploaded("test-reports").length, 1);
  assert.deepEqual(uploaded("test-reports")[0].body, report);
  // Its own token; every part of the log shares the log's.
  const reportToken = bearer(uploaded("test-reports")[0]);
  assert.ok(reportToken && reportToken !== bearer(firstPart));
  assert.ok(uploaded("activity-logs").every((upload) => bearer(upload) === bearer(firstPart)));

  // The same model's ways to connect: this one is now tested here.
  await page.click("text=Test another way to connect it");
  await until("the cards", stage, (s) => s === "connect");
  assert.match(await page.textContent('[data-card="xinput/usb"]'), /Tested here/);
  // The same unit stays picked; a new one would be the next number.
  assert.equal(await page.inputValue("[data-unit]"), "X360-01");
  assert.equal((await page.textContent("[data-new-unit]")).trim(), "New unit: X360-02");
  await page.click(".test-connect >> text=Back to the list");
  await setPad(null);

  // The choice stays for the next visit: a new page load sends its own log
  // (another ID). Going away, the page sends what is left of its log as
  // the next part (Firefox and WebKit send it on reload, Chromium not
  // always, so the test leaves the page itself first).
  const before = uploaded("activity-logs").length;
  await page.evaluate(() => window.dispatchEvent(new Event("pagehide")));
  await until("the last part", () => uploaded("activity-logs").length, (n) => n > before);
  await page.reload({ waitUntil: "networkidle" });
  await until("the list", stage, (s) => s === "list");
  const next = await until(
    "a part of the new page's log",
    () => uploaded("activity-logs").filter((upload) => upload.body.session !== firstPart.body.session),
    (u) => u.length > 0,
  );
  const old = uploaded("activity-logs").filter((upload) => upload.body.session === firstPart.body.session);
  assert.ok(old.length > before, "the last part of the old log");
  assert.ok(
    old.every((upload, n) => upload.body.part === n && bearer(upload) === bearer(firstPart)),
    JSON.stringify(old.map((upload) => upload.body.part)),
  );
  assert.equal(next[0].body.part, 0);
  assert.notEqual(next[0].body.session, firstPart.body.session);
  // The history stays: downloaded as saved, then deleted after asking.
  await until("the history after a reload", history, (h) => h.length === 1);
  const [again] = await Promise.all([
    page.waitForEvent("download"),
    page.click("[data-history] .report-row >> text=Download"),
  ]);
  assert.deepEqual(JSON.parse(await readFile(await again.path(), "utf8")), report);
  page.once("dialog", (dialog) => dialog.accept());
  await page.click("[data-history] .report-row >> text=Delete");
  await until("the empty history", () => page.locator("[data-history-empty]").count(), (n) => n === 1);
  assert.notEqual(bearer(next[0]), bearer(firstPart));
  assert.equal(next[0].body.events[0].type, "session");

  // Withdrawing: this browser kept a token for each log and the report.
  await page.click(".settings-btn");
  await page.click("[data-withdraw]");
  await until("the withdrawal", () => page.textContent("[data-withdrawn]").catch(() => ""), (t) => /3 of 3/.test(t));
  const deletes = uploads.filter((upload) => upload.method === "DELETE");
  assert.deepEqual(
    deletes.map((upload) => [upload.path, bearer(upload)]).sort(),
    [
      [`/__sync/v1/activity-logs/${firstPart.body.session}`, bearer(firstPart)],
      [`/__sync/v1/activity-logs/${next[0].body.session}`, bearer(next[0])],
      ["/__sync/v1/test-reports/7", reportToken],
    ].sort(),
  );
  assert.equal(await page.isDisabled("[data-withdraw]"), true);
  await page.keyboard.press("Escape");

  // WebHID (Chromium): a DualSense picked in the chooser runs the Quick plan
  // on its decoded reports.
  if (browserName === "chromium") {
    // On Bluetooth, the reduced report lacks the mute button (and the
    // touchpad, motion and battery): this one was switched to its full
    // report before (in the tester, or by macOS).
    await page.evaluate(() => {
      window.__hid.fullReport = true;
    });
    // Its Gamepad API twin, as Chrome on macOS names it: no USB IDs, so the
    // generic model, in the mode of the family the name tells.
    await setPad({ ...XBOX360, id: "DualSense Wireless Controller (STANDARD GAMEPAD)", buttons: Array.from({ length: 18 }, () => 0) });
    await until("detection of the twin", stage, (s) => s === "detected");
    assert.equal(
      await page.textContent("[data-detected]"),
      "Generic PlayStation-compatible controller · PS5 · connection to confirm",
    );
    await until("the offer to read the USB IDs", () => page.locator("[data-ids-hidden]").count(), (n) => n === 1);
    await page.click(".test-detected [data-connect-hid]");
    await until("detection through WebHID", () => page.textContent("[data-detected]"), (t) => /^Sony/.test(t));
    const setButtons = (name, on) =>
      page.evaluate(
        ([bits, on]) => {
          const state = window.__hid.state;
          state.buttons = [0x08, 0, 0];
          state.l2 = 0;
          state.r2 = 0;
          if (!on || !bits) return;
          const [byte, mask] = bits;
          if (byte === 0 && mask < 0x10) state.buttons[0] = mask;
          else state.buttons[byte] |= mask;
          if (byte === 1 && mask === 0x04) state.l2 = 255;
          if (byte === 1 && mask === 0x08) state.r2 = 255;
        },
        [DUALSENSE_BITS[name], on],
      );
    assert.equal(await page.textContent("[data-detected]"), "Sony DualSense Wireless Controller · PS5 · Bluetooth");
    await until("no offer any more", () => page.locator("[data-ids-hidden]").count(), (n) => n === 0);
    // The firmware check: this one answers as Sony's pads do.
    await page.evaluate(() => {
      const reply = new Uint8Array(64);
      reply[0] = 0x20;
      reply.set([..."Jun 12 2023"].map((c) => c.charCodeAt(0)), 1);
      reply.set([..."10:44:25"].map((c) => c.charCodeAt(0)), 12);
      reply[24] = 0x13;
      reply[25] = 0x03;
      window.__hid.firmware = [...reply];
    });
    await until(
      "the firmware offer",
      () => page.textContent("[data-firmware]").catch(() => ""),
      (text) => /Over Bluetooth, asking switches the pad/.test(text),
    );
    await page.click("[data-check-firmware]");
    const firmware = await until(
      "the firmware",
      () => page.getAttribute("[data-firmware]", "data-firmware"),
      (state) => state === "answered" || state === "silent",
    );
    assert.equal(firmware, "answered");
    assert.match((await page.textContent("[data-firmware]")).trim(), /^Firmware built Jun 12 2023 10:44:25 \(hardware 0x0313, firmware 0x0000, update 0x0000\): it answers as Sony's pads do\.$/);
    assert.equal(await page.textContent("[data-detected]"), "Sony DualSense Wireless Controller · PS5 · Bluetooth");
    await page.click(".test-detected >> text=Confirm");
    await page.click('[data-plan="quick"]');
    await until("the session", stage, (s) => s === "running");
    for (let guard = 0; guard < 200 && (await stage()) === "running"; guard += 1) {
      const key = await prompt();
      if (key?.startsWith("press:")) {
        const name = key.slice("press:".length);
        await setButtons(name, true);
        await sleep(150);
        await setButtons(name, false);
        await until(`the end of ${key}`, prompt, (p) => p !== key);
      } else {
        await sleep(30);
      }
    }
    await until("the summary", stage, (s) => s === "summary");
    const hidSummary = await page.textContent("[data-summary]");
    assert.equal(hidSummary, "20 passed, 0 failed, 0 skipped, 0 not applicable.", hidSummary);
    const [hidDownload] = await Promise.all([page.waitForEvent("download"), page.click("[data-download]")]);
    const hidReport = JSON.parse(await readFile(await hidDownload.path(), "utf8"));
    assert.equal(hidReport.device.views[0].identity.backend, "webhid");
    const south = hidReport.results.find((result) => result.step === "controls.south");
    assert.ok(south.evidence.reports.length > 0 && !south.evidence.frames, "HID evidence holds reports");
  }

  assert.deepEqual(problems, []);
  console.log(`test mode smoke test passed (${browserName})`);
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
