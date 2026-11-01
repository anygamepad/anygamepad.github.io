// Smoke test of the test mode (roadmap task H4) on the built site (dist/):
//
// - Gamepad API: a wired Xbox 360 controller connected without picking it is
//   detected; the Full plan runs with a person simulated from the prompts
//   (every button, sticks, triggers, rate, vibration answered yes, the
//   player lights the API cannot drive, unplugging and plugging back); the
//   summary, the review of the report's texts,
//   saving in the browser, the "Tested here" mark, and the downloaded report
//   (format, configuration, results, evidence).
// - WebHID (Chromium only): a Bluetooth DualSense picked in the chooser and
//   while the tester shows is detected, and the Quick plan runs on
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
  await page.addInitScript(fakeGamepads);
  await page.addInitScript(fakeHid);
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

  await page.click(".mode-tab >> text=Test mode");
  await until("the list", stage, (s) => s === "list");
  assert.ok((await page.locator(".test-list [data-model]").count()) >= 13);
  // The search filters the catalog.
  await page.fill(".test-search input", "dualshock");
  assert.equal(await page.locator(".test-list [data-model]").count(), 3);
  await page.fill(".test-search input", "");

  // Connecting a pad without picking it: detection.
  await setPad(XBOX360);
  await until("detection", stage, (s) => s === "detected");
  assert.equal(await page.textContent("[data-detected]"), "Microsoft Xbox 360 Controller · XInput · USB cable");
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

  await page.click("[data-save]");
  await until("the saved note", () => page.textContent("[data-saved]").catch(() => ""), (t) => /^Saved in this browser/.test(t));
  const [download] = await Promise.all([page.waitForEvent("download"), page.click("[data-download]")]);
  assert.match(download.suggestedFilename(), /^microsoft-xbox-360-xinput-usb-\w+-web-gamepad_api-\d{4}-\d{2}-\d{2}-1\.json$/);
  const report = JSON.parse(await readFile(await download.path(), "utf8"));
  assert.equal(report.format, "anygamepad-test-report");
  assert.equal(report.plan.id, "full");
  assert.equal(report.configuration.start, "connecting");
  assert.deepEqual(report.configuration.confirmed, report.configuration.detected);
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

  // The same model's ways to connect: this one is now tested here.
  await page.click("text=Test another way to connect it");
  await until("the cards", stage, (s) => s === "connect");
  assert.match(await page.textContent('[data-card="xinput/usb"]'), /Tested here/);
  await page.click(".test-connect >> text=Back to the list");
  await setPad(null);

  // WebHID (Chromium): a DualSense picked in the chooser runs the Quick plan
  // on its decoded reports.
  if (browserName === "chromium") {
    await page.click(".mode-tab >> text=Tester");
    await page.click("text=Connect a pad with WebHID");
    await until("the WebHID pad", () => page.locator(".pad .pad-name").allTextContents(), (names) => names.length === 1);
    // On Bluetooth, the reduced report lacks the mute button (and the
    // touchpad, motion and battery): switch to the full report first.
    await page.getByRole("button", { name: "Switch to the full report" }).click();
    await until("the full report", () => page.evaluate(() => window.__hid.features), (f) => f.includes(0x05));
    await sleep(200);
    // Connected while the tester showed, it is detected all the same.
    await page.click(".mode-tab >> text=Test mode");
    await until("detection", stage, (s) => s === "detected");
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
