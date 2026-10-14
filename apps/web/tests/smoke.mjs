// Smoke test of the built site (dist/) in a browser: it loads under its
// Content Security Policy without errors or warnings, the WebAssembly module
// starts, and the tester follows pads:
//
// - Gamepad API: the pad list and the selection, hotplug, raw values, the
//   drawing of each pad's family, the values shown on it, the update rate,
//   the vibration test, and the hint shown in Chromium on macOS while no
//   gamepad shows.
// - WebHID: a Bluetooth DualSense picked in the chooser hides its Gamepad API
//   twin, is decoded (reduced report, then the full one after the switch),
//   shows its touchpad, motion and battery, records its reports to a file,
//   and goes away on disconnection.
//
// Neither API has test hooks, so the devices are fakes: behind
// `navigator.getGamepads()`, and a `navigator.hid` of our own.
//
// BROWSER picks the engine: `chromium` (the default; CHROME_PATH must point at
// a Chrome or Chromium binary), `firefox` or `webkit` (Playwright's builds,
// from `npx playwright-core install firefox webkit`).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { chromium, firefox, webkit } from "playwright-core";
import { preview } from "vite";

const BROWSERS = {
  chromium: { type: chromium, engine: "blink" },
  firefox: { type: firefox, engine: "gecko" },
  webkit: { type: webkit, engine: "webkit" },
};
const browserName = process.env.BROWSER ?? "chromium";
const target = BROWSERS[browserName];
if (!target) {
  console.error(`BROWSER must be one of ${Object.keys(BROWSERS).join(", ")}.`);
  process.exit(2);
}
const executablePath = process.env.CHROME_PATH;
if (browserName === "chromium" && !executablePath) {
  console.error("Set CHROME_PATH to a Chrome or Chromium binary.");
  process.exit(2);
}

// Runs in the page before its scripts: `window.__setGamepads(list)` puts
// fake gamepads in the first slots of `navigator.getGamepads()`. They are
// real Gamepad objects as far as `instanceof` goes, with own properties.
// A gamepad with `rumble: true` gets a vibration actuator that records its
// calls in `window.__haptics`.
const fakeGamepads = () => {
  window.__haptics = [];
  const button = (value) =>
    Object.defineProperties(Object.create(GamepadButton.prototype), {
      pressed: { value: value > 0.5 },
      touched: { value: value > 0 },
      value: { value },
    });
  const actuator = (index) => ({
    type: "dual-rumble",
    effects: ["dual-rumble", "trigger-rumble"],
    playEffect: (type, params) => {
      window.__haptics.push({ index, call: "playEffect", type, ...params });
      return Promise.resolve("complete");
    },
    reset: () => {
      window.__haptics.push({ index, call: "reset" });
      return Promise.resolve("complete");
    },
  });
  const gamepad = (index, { id, mapping, buttons, axes, rumble }) =>
    Object.defineProperties(Object.create(Gamepad.prototype), {
      id: { value: id },
      index: { value: index },
      connected: { value: true },
      mapping: { value: mapping },
      // Fresh data on every read, so the update rate is the sampler's.
      timestamp: { get: () => performance.now() },
      axes: { value: axes },
      buttons: { value: buttons.map(button) },
      vibrationActuator: { value: rumble ? actuator(index) : null },
    });
  let slots = [null, null, null, null];
  window.__setGamepads = (list) => {
    slots = [0, 1, 2, 3].map((i) => (list[i] ? gamepad(i, list[i]) : null));
  };
  Navigator.prototype.getGamepads = () => slots;
};

// Runs in the page before its scripts: a `navigator.hid` whose chooser picks
// one Bluetooth DualSense. The pad streams its current report every 5 ms
// while open: the reduced report until a feature report is read, then the
// full one (`0x31`, with its CRC-32), as real pads do. `window.__hid` drives
// it: `state` (buttons, touch, battery), `features` (the feature reports
// read), `disconnect()`.
const fakeHid = () => {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc32 = (bytes) => {
    let crc = 0xffffffff;
    for (const byte of bytes) crc = (crc >>> 8) ^ crcTable[(crc ^ byte) & 0xff];
    return (crc ^ 0xffffffff) >>> 0;
  };
  const items = (bytes) => [{ reportSize: 8, reportCount: bytes, usages: [], isAbsolute: true }];
  const collections = [
    {
      usagePage: 0x01,
      usage: 0x05,
      type: 1,
      children: [],
      inputReports: [
        { reportId: 0x01, items: items(9) },
        { reportId: 0x31, items: items(77) },
      ],
      outputReports: [],
      featureReports: [{ reportId: 0x05, items: items(40) }],
    },
  ];
  const state = { buttons: [0x08, 0, 0], l2: 0, r2: 0, touch: null, battery: 0x15 };
  const report = (full) => {
    if (!full) {
      return [0x01, new Uint8Array([128, 128, 128, 128, ...state.buttons, state.l2, state.r2])];
    }
    const data = new Uint8Array(77);
    data[0] = 0x01;
    const d = data.subarray(1);
    d.set([128, 128, 128, 128, state.l2, state.r2], 0);
    d.set(state.buttons, 7);
    d.set([0x9c, 0xff], 15); // gyro x = -100
    d.set([0x00, 0x20], 21); // accel x = 8192
    d.set(state.touch ?? [0x80, 0, 0, 0], 32);
    d[36] = 0x80;
    d[52] = state.battery;
    const crc = crc32([0xa1, 0x31, ...data.subarray(0, 73)]);
    data.set([crc & 0xff, (crc >>> 8) & 0xff, (crc >>> 16) & 0xff, crc >>> 24], 73);
    return [0x31, data];
  };

  class FakeDevice extends EventTarget {
    vendorId = 0x054c;
    productId = 0x0ce6;
    productName = "DualSense Wireless Controller";
    collections = collections;
    opened = false;
    full = false;
    timer = undefined;
    open() {
      this.opened = true;
      this.timer = setInterval(() => {
        const [reportId, data] = report(this.full);
        const event = new Event("inputreport");
        Object.defineProperties(event, {
          reportId: { value: reportId },
          data: { value: new DataView(data.buffer) },
        });
        this.dispatchEvent(event);
      }, 5);
      return Promise.resolve();
    }
    close() {
      this.opened = false;
      clearInterval(this.timer);
      return Promise.resolve();
    }
    receiveFeatureReport(reportId) {
      window.__hid.features.push(reportId);
      this.full = true;
      return Promise.resolve(new DataView(new ArrayBuffer(41)));
    }
    sendReport() {
      return Promise.reject(new Error("unexpected output report"));
    }
    sendFeatureReport() {
      return Promise.reject(new Error("unexpected feature report"));
    }
  }

  class FakeHid extends EventTarget {
    granted = [];
    getDevices() {
      return Promise.resolve([...this.granted]);
    }
    requestDevice(options) {
      window.__hid.requests.push(options.filters);
      const device = new FakeDevice();
      this.granted.push(device);
      return Promise.resolve([device]);
    }
  }

  const hid = new FakeHid();
  window.__hid = {
    state,
    features: [],
    requests: [],
    disconnect() {
      const device = hid.granted.pop();
      device.close();
      const event = new Event("disconnect");
      Object.defineProperty(event, "device", { value: device });
      hid.dispatchEvent(event);
    },
  };
  Object.defineProperty(Navigator.prototype, "hid", { get: () => hid, configurable: true });
};

// Runs in the page: what the selected pad's detail shows.
const readDetail = () => {
  const detail = document.querySelector(".detail");
  if (!detail) return null;
  const text = (element) => element?.textContent.replace(/\s+/g, " ").trim() ?? null;
  const diagram = detail.querySelector(".diagram");
  const live = detail.querySelector(".live");
  return {
    // The live view shows the selected pad (it is rebuilt a frame after
    // the selection changes).
    ready: live?.dataset.view === live?.dataset.key,
    name: text(detail.querySelector("h2")),
    facts: text(detail.querySelector(".facts")),
    gamepadId: detail.querySelector(".gamepad-id code")?.textContent,
    rate: text(detail.querySelector(".rate")),
    rawOpen: detail.querySelector(".raw-values")?.open ?? null,
    buttons: [...detail.querySelectorAll(".raw-buttons .button")].map((cell) => ({
      name: text(cell.querySelector(".name")),
      value: text(cell.querySelector(".value")),
      pressed: cell.classList.contains("pressed"),
      touched: cell.classList.contains("touched"),
    })),
    axes: [...detail.querySelectorAll(".raw-axes .axis")].map((row) => ({
      name: text(row.querySelector(".name")),
      value: text(row.querySelector(".value")),
      bar: row.querySelector(".bar").style.getPropertyValue("--value"),
    })),
    diagram: diagram && {
      label: diagram.getAttribute("aria-label"),
      on: [...diagram.querySelectorAll(".control.on")].map((element) => text(element.querySelector("title"))),
      triggers: [...diagram.querySelectorAll(".fill")].map((element) => element.getAttribute("width")),
      sticks: [...diagram.querySelectorAll(".dot")].map((dot) => `${dot.getAttribute("cx")},${dot.getAttribute("cy")}`),
      values: diagram.classList.contains("show-values")
        ? [...diagram.querySelectorAll(".value")].map((value) => value.textContent)
        : null,
      glyphs: [...diagram.querySelectorAll(".glyph-text")].map((glyph) => glyph.textContent),
      fingers: [...diagram.querySelectorAll(".finger")].filter((dot) => dot.getAttribute("visibility") === "visible").length,
    },
    vibration: [...detail.querySelectorAll(".vibration button")].map(
      (button) => `${button.textContent}${button.disabled ? " (disabled)" : ""}`,
    ),
    note: text(detail.querySelector(".note")),
    form: text(detail.querySelector(".report-form")),
    switchShown: detail.querySelector(".switch-box") ? !detail.querySelector(".switch-box").hidden : null,
    touch: text(detail.querySelector(".touchpad-panel .panel-text")),
    motion: [...detail.querySelectorAll(".motion .value")].map((value) => value.textContent),
    battery: text(detail.querySelector(".battery-panel .panel-text")),
  };
};

const DUALSHOCK = {
  id: "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)",
  mapping: "standard",
  buttons: Array.from({ length: 18 }, () => 0),
  axes: [0, 0, 0, 0],
  rumble: true,
};

// A DragonRise SNES-style pad: no standard mapping, no vibration.
const GENERIC = {
  id: "USB Gamepad (Vendor: 0079 Product: 0011)",
  mapping: "",
  buttons: Array.from({ length: 10 }, () => 0),
  axes: [0, 0, 0, 0, 0],
};

// A pad of no known family, in the standard mapping, with two extra buttons.
const FLYDIGI = {
  id: "Flydigi Vader 5 Pro (STANDARD GAMEPAD Vendor: 04b4 Product: 2412)",
  mapping: "standard",
  buttons: Array.from({ length: 19 }, (_, index) => (index === 17 ? 1 : 0)),
  axes: [0, 0, 0, 0],
};

const XBOX = {
  id: "Xbox 360 Controller (XInput STANDARD GAMEPAD)",
  mapping: "standard",
  buttons: Array.from({ length: 17 }, () => 0),
  axes: [0, 0, 0, 0],
};

const DUALSENSE = {
  id: "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
  mapping: "standard",
  buttons: Array.from({ length: 18 }, () => 0),
  axes: [0, 0, 0, 0],
};

const MAC_CHROME =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

const with_ = (pad, { buttons = {}, axes = pad.axes }) => ({
  ...pad,
  buttons: pad.buttons.map((value, index) => buttons[index] ?? value),
  axes,
});

const server = await preview({ preview: { host: "127.0.0.1", port: 4317 }, logLevel: "warn" });
const url = server.resolvedUrls.local[0];
const browser = await target.type.launch(browserName === "chromium" ? { executablePath } : {});
const problems = [];
const watch = (page, label) => {
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      problems.push(`${label} console.${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => problems.push(`${label} page error: ${error.message}`));
};
try {
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  watch(page, "");
  await page.addInitScript(() =>
    document.addEventListener("securitypolicyviolation", (event) =>
      console.error(`CSP violation: ${event.violatedDirective} blocked ${event.blockedURI}`),
    ),
  );
  await page.addInitScript(fakeGamepads);
  await page.addInitScript(fakeHid);

  await page.goto(url, { waitUntil: "networkidle" });
  const policy = await page.getAttribute('meta[http-equiv="Content-Security-Policy"]', "content");
  assert.match(policy ?? "", /script-src 'self' 'wasm-unsafe-eval'/);
  assert.doesNotMatch(policy ?? "", /'unsafe-eval'|'unsafe-inline'/);

  await page.waitForSelector(".message");
  assert.match(await page.innerText(".tagline"), /library \d+\.\d+\.\d+ in WebAssembly/);
  assert.match(await page.innerText(".environment"), new RegExp(`^${target.engine} \\d+ on \\w+$`));
  assert.equal(await page.locator(".hint").count(), 0);

  const setGamepads = (list) => page.evaluate((pads) => window.__setGamepads(pads), list);
  const detail = () => page.evaluate(readDetail);
  const haptics = () => page.evaluate(() => window.__haptics);
  const padNames = () => page.locator(".pad .pad-name").allTextContents();
  const select = (name) => page.locator(".pad", { hasText: name }).click();
  // Polls `read` until `done` holds; on timeout, shows the last value read.
  const until = async (what, read, done) => {
    const deadline = Date.now() + 5000;
    for (;;) {
      const value = await read();
      if (done(value)) return value;
      if (Date.now() > deadline) assert.fail(`timed out waiting for ${what}; last read: ${JSON.stringify(value)}`);
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  };

  // Two gamepads: the list, the first one selected, its facts, raw layout,
  // drawing and vibration test.
  await setGamepads([DUALSHOCK, GENERIC]);
  assert.deepEqual(await until("two pads", padNames, (names) => names.length === 2), ["Wireless Controller", "USB Gamepad"]);
  let pad = await until("the update rate", detail, (d) => d?.ready && /^[1-9]\d* updates\/s$/.test(d.rate ?? ""));
  assert.equal(pad.name, "Wireless Controller");
  assert.equal(pad.facts, '054c:09cc · "standard" mapping · 18 buttons · 4 axes');
  assert.equal(pad.gamepadId, DUALSHOCK.id);
  assert.equal(pad.rawOpen, false);
  assert.equal(pad.buttons.length, 18);
  assert.deepEqual(
    pad.buttons.slice(0, 4).map((button) => button.name),
    ["south", "east", "west", "north"],
  );
  assert.equal(pad.buttons[17].name, "extra_0");
  assert.deepEqual(
    pad.axes.map((axis) => axis.name),
    ["left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y"],
  );
  assert.deepEqual(pad.diagram, {
    label: "DualShock 4, canonical positions",
    on: [],
    triggers: ["0.0", "0.0"],
    sticks: ["154.0,156.0", "246.0,156.0"],
    values: null,
    glyphs: [],
    fingers: 0,
  });
  assert.deepEqual(pad.vibration, ["Strong motor", "Weak motor", "Both", "Triggers"]);
  assert.equal(pad.note, null);

  await select("USB Gamepad");
  const generic = await until("the second pad", detail, (d) => d.ready && d.name === "USB Gamepad" && d.rate !== "Waiting for data…");
  assert.equal(generic.facts, "0079:0011 · no mapping (the browser's own layout) · 10 buttons · 5 axes");
  assert.equal(generic.diagram, null);
  assert.equal(generic.rawOpen, true);
  assert.ok(generic.buttons.every((button) => button.name === ""));
  assert.equal(generic.axes.length, 5);
  assert.deepEqual(generic.vibration, ["Strong motor (disabled)", "Weak motor (disabled)", "Both (disabled)"]);
  assert.equal(generic.note, "This browser cannot vibrate this gamepad.");

  // Raw values and the canonical drawing follow the gamepad. Raw Y grows
  // downwards; the drawing's canonical Y grows upwards (ADR 0001).
  await select("Wireless Controller");
  await setGamepads([with_(DUALSHOCK, { buttons: { 0: 1, 7: 0.5, 12: 1 }, axes: [0.5, -1, 0, 0.25] }), GENERIC]);
  pad = await until("the pressed buttons", detail, (d) => d.ready && d.name === "Wireless Controller" && d.buttons[0]?.pressed && d.axes[1].bar === "-1");
  assert.deepEqual(pad.buttons[0], { name: "south", value: "1.00", pressed: true, touched: false });
  assert.deepEqual(pad.buttons[7], { name: "right_trigger", value: "0.50", pressed: false, touched: true });
  assert.deepEqual(
    pad.axes.map((axis) => [axis.value, axis.bar]),
    [["+0.500", "0.5"], ["−1.000", "-1"], ["+0.000", "0"], ["+0.250", "0.25"]],
  );
  assert.deepEqual(pad.diagram.on, ["dpad_up", "south"]);
  assert.deepEqual(pad.diagram.triggers, ["0.0", "32.0"]);
  assert.deepEqual(pad.diagram.sticks, ["161.5,141.0", "246.0,159.8"]);

  // The values on the drawing.
  await page.locator(".toggle").click();
  pad = await until("the values", detail, (d) => d.diagram.values !== null);
  assert.deepEqual(pad.diagram.values, ["0.00", "0.50", "+0.50 +1.00", "+0.00 −0.25"]);
  assert.equal(await page.getAttribute(".toggle", "aria-pressed"), "true");
  await page.locator(".toggle").click();
  await until("the values hidden", detail, (d) => d.diagram.values === null);

  // Vibration: a one-second test, then the motors stop.
  await page.locator(".detail").getByRole("button", { name: "Strong motor" }).click();
  let calls = await until("the strong motor test", haptics, (log) => log.some((call) => call.call === "reset"));
  assert.deepEqual(
    calls.map(({ index, call, type, strongMagnitude, weakMagnitude }) => ({ index, call, type, strongMagnitude, weakMagnitude })),
    [
      { index: 0, call: "playEffect", type: "dual-rumble", strongMagnitude: 1, weakMagnitude: 0 },
      { index: 0, call: "reset", type: undefined, strongMagnitude: undefined, weakMagnitude: undefined },
    ],
  );
  // A test started during another one stops every motor at its end.
  await page.evaluate(() => (window.__haptics = []));
  await page.locator(".detail").getByRole("button", { name: "Triggers" }).click();
  await page.locator(".detail").getByRole("button", { name: "Strong motor" }).click();
  calls = await until("the motors to stop", haptics, (log) => log.some((call) => call.call === "reset"));
  assert.deepEqual(
    calls[0],
    { index: 0, call: "playEffect", type: "trigger-rumble", duration: 5000, startDelay: 0,
      strongMagnitude: 0, weakMagnitude: 0, leftTrigger: 1, rightTrigger: 1 },
  );
  assert.equal(calls.at(-1).call, "reset");

  // Unplugging one gamepad leaves the other live.
  await setGamepads([with_(DUALSHOCK, { buttons: { 1: 1 } })]);
  await until("one pad", padNames, (names) => names.length === 1);
  pad = await until("the next press", detail, (d) => d.buttons[1]?.pressed && !d.buttons[0].pressed);
  assert.deepEqual(pad.diagram.on, ["east"]);

  // The drawing of each family: Xbox labels, the generic pad's C and Z.
  await setGamepads([XBOX, FLYDIGI]);
  await until("the Xbox pad", detail, (d) => d?.ready && d.diagram?.label === "Xbox controller, canonical positions");
  pad = await detail();
  assert.deepEqual(pad.diagram.glyphs, ["Y", "A", "X", "B"]);
  await select("Flydigi Vader 5 Pro");
  pad = await until("the generic pad", detail, (d) => d.ready && d.diagram?.label === "Controller, canonical positions");
  assert.deepEqual(pad.diagram.on, ["extra_0"]);
  assert.deepEqual(pad.diagram.glyphs, ["C", "Z"]);

  // WebHID: a DualSense picked in the chooser takes the place of its Gamepad
  // API twin, decoded: the reduced Bluetooth report first.
  await setGamepads([DUALSENSE]);
  await until("the DualSense", padNames, (names) => names.join() === "DualSense Wireless Controller");
  await page.evaluate(() => {
    window.__hid.state.buttons = [0x20 | 0x00, 0x01, 0x01]; // Cross + D-pad up, L1, PS
  });
  await page.getByRole("button", { name: "Connect a pad with WebHID" }).click();
  pad = await until("the WebHID pad", detail, (d) => d?.ready && d.facts?.includes("WebHID") && d.form);
  assert.deepEqual(await page.evaluate(() => window.__hid.requests), [[
    { usagePage: 1, usage: 5 },
    { usagePage: 1, usage: 4 },
    { usagePage: 1, usage: 8 },
  ]]);
  assert.deepEqual(await padNames(), ["DualSense Wireless Controller"]);
  assert.equal(pad.facts, "054c:0ce6 · WebHID · Bluetooth · decoded");
  assert.equal(pad.form, "Bluetooth, reduced report: no touchpad, motion or battery");
  assert.equal(pad.switchShown, true);
  pad = await until("the report rate", detail, (d) => /^[1-9]\d* reports\/s$/.test(d.rate));
  assert.equal(pad.diagram.label, "DualSense, canonical positions");
  assert.deepEqual(pad.diagram.on, ["left_shoulder", "home", "dpad_up", "south"]);
  assert.equal(pad.touch, "Needs the full report");
  assert.equal(pad.battery, "Needs the full report");
  assert.deepEqual(await page.evaluate(() => window.__hid.features), []);

  // The switch reads the calibration report, and only it; then the full
  // report brings the touchpad, motion and battery.
  await page.getByRole("button", { name: "Switch to the full report" }).click();
  await page.evaluate(() => {
    window.__hid.state.touch = [0x05, 0xbf, 0xc3, 0x21]; // finger 5 in the middle
  });
  pad = await until("the full report", detail, (d) => d.form === "Bluetooth, full report" && d.diagram.fingers === 1);
  assert.deepEqual(await page.evaluate(() => window.__hid.features), [0x05]);
  assert.equal(pad.switchShown, false);
  assert.equal(pad.touch, "#5 0.50, 0.50");
  assert.deepEqual(pad.motion, ["-100", "0", "0", "8192", "0", "0"]);
  assert.equal(pad.battery, "55% · charging");

  // Recording: two seconds of reports, downloaded as JSON.
  await page.locator(".raw-report summary").click();
  const [download] = await Promise.all([
    page.waitForEvent("download", { timeout: 10000 }),
    page.getByRole("button", { name: "Record 2 seconds" }).click(),
  ]);
  const recording = JSON.parse(await readFile(await download.path(), "utf8"));
  assert.equal(recording.format, "anygamepad-hid-recording-v1");
  assert.deepEqual(
    { ...recording.device, collections: recording.device.collections.length },
    { vendor_id: 0x054c, product_id: 0x0ce6, product_name: "DualSense Wireless Controller",
      family: "dualsense", transport: "bluetooth", collections: 1 },
  );
  assert.ok(recording.reports.length > 10, `${recording.reports.length} reports recorded`);
  assert.ok(recording.reports.every((report) => report.report_id === 0x31 && report.data.length === 154));

  // Disconnected, the pad leaves the list and its Gamepad API twin returns.
  await page.evaluate(() => window.__hid.disconnect());
  await until("the twin back", detail, (d) => d?.ready && d.facts?.includes('"standard" mapping'));
  assert.deepEqual(await padNames(), ["DualSense Wireless Controller"]);

  await setGamepads([]);
  await page.waitForSelector(".pads", { state: "detached" });
  await page.waitForSelector(".detail", { state: "detached" });
  await page.waitForSelector(".message");
  await context.close();

  // Chromium on macOS (by its user agent) explains why a DualSense may not
  // show, until a gamepad does.
  const mac = await browser.newContext({ userAgent: MAC_CHROME });
  const macPage = await mac.newPage();
  watch(macPage, "(macOS)");
  await macPage.addInitScript(fakeGamepads);
  await macPage.goto(url, { waitUntil: "networkidle" });
  assert.match(await macPage.innerText(".environment"), /^blink \d+ on macos$/);
  assert.match(await macPage.locator(".hint").innerText(), /^A DualSense does not show in Chrome.*WebHID.*Safari/);
  await macPage.evaluate((pads) => window.__setGamepads(pads), [DUALSHOCK]);
  await macPage.waitForSelector(".hint", { state: "detached" });
  await mac.close();

  assert.deepEqual(problems, []);
  console.log(`smoke test passed in ${browserName} (${url})`);
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
