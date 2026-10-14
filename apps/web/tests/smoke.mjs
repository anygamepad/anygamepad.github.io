// Smoke test of the built site (dist/) in a browser: it loads under its
// Content Security Policy without errors or warnings, the WebAssembly module
// starts, and the live tester follows gamepads: hotplug, raw values, the
// controller drawing, the update rate and the vibration test, and the hint
// shown in Chromium on macOS while no gamepad shows. The Gamepad API has no
// test hooks, so the gamepads are fakes behind `navigator.getGamepads()`.
//
// BROWSER picks the engine: `chromium` (the default; CHROME_PATH must point at
// a Chrome or Chromium binary), `firefox` or `webkit` (Playwright's builds,
// from `npx playwright-core install firefox webkit`).

import assert from "node:assert/strict";
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

// Runs in the page: what a device card shows.
const readCard = (index) => {
  const card = document.querySelectorAll(".device")[index];
  if (!card) return null;
  const text = (element) => element?.textContent.replace(/\s+/g, " ").trim() ?? null;
  const diagram = card.querySelector(".diagram");
  return {
    name: text(card.querySelector("h2")),
    facts: text(card.querySelector(".facts")),
    gamepadId: card.querySelector(".gamepad-id code")?.textContent,
    rate: text(card.querySelector(".rate")),
    buttons: [...card.querySelectorAll(".raw-buttons .button")].map((cell) => ({
      name: text(cell.querySelector(".name")),
      value: text(cell.querySelector(".value")),
      pressed: cell.classList.contains("pressed"),
      touched: cell.classList.contains("touched"),
    })),
    axes: [...card.querySelectorAll(".raw-axes .axis")].map((row) => ({
      name: text(row.querySelector(".name")),
      value: text(row.querySelector(".value")),
      bar: row.querySelector(".bar").style.getPropertyValue("--value"),
    })),
    diagram: diagram && {
      on: [...diagram.querySelectorAll(".control.on")].map((element) => text(element.querySelector("title"))),
      triggers: [...diagram.querySelectorAll(".fill")].map((element) => element.getAttribute("width")),
      sticks: [...diagram.querySelectorAll(".dot")].map((dot) => `${dot.getAttribute("cx")},${dot.getAttribute("cy")}`),
    },
    vibration: [...card.querySelectorAll(".vibration button")].map(
      (button) => `${button.textContent}${button.disabled ? " (disabled)" : ""}`,
    ),
    note: text(card.querySelector(".note")),
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
try {
  const page = await browser.newPage();
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      problems.push(`console.${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => problems.push(`page error: ${error.message}`));
  await page.addInitScript(() =>
    document.addEventListener("securitypolicyviolation", (event) =>
      console.error(`CSP violation: ${event.violatedDirective} blocked ${event.blockedURI}`),
    ),
  );
  await page.addInitScript(fakeGamepads);

  await page.goto(url, { waitUntil: "networkidle" });
  const policy = await page.getAttribute('meta[http-equiv="Content-Security-Policy"]', "content");
  assert.match(policy ?? "", /script-src 'self' 'wasm-unsafe-eval'/);
  assert.doesNotMatch(policy ?? "", /'unsafe-eval'|'unsafe-inline'/);

  await page.waitForSelector(".message");
  assert.match(await page.innerText(".tagline"), /library \d+\.\d+\.\d+ in WebAssembly/);
  assert.match(await page.innerText(".environment"), new RegExp(`^${target.engine} \\d+ on \\w+$`));

  const setGamepads = (list) => page.evaluate((pads) => window.__setGamepads(pads), list);
  const card = (index) => page.evaluate(readCard, index);
  const haptics = () => page.evaluate(() => window.__haptics);
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

  // Two gamepads: the card's facts, raw layout, drawing and vibration test.
  await setGamepads([DUALSHOCK, GENERIC]);
  await until("two cards", () => page.locator(".device").count(), (count) => count === 2);
  let dualshock = await until("the update rate", () => card(0), (c) => /^[1-9]\d* updates\/s$/.test(c.rate));
  assert.equal(dualshock.name, "Wireless Controller");
  assert.equal(dualshock.facts, '054c:09cc · "standard" mapping · 18 buttons · 4 axes');
  assert.equal(dualshock.gamepadId, DUALSHOCK.id);
  assert.equal(dualshock.buttons.length, 18);
  assert.deepEqual(
    dualshock.buttons.slice(0, 4).map((button) => button.name),
    ["south", "east", "west", "north"],
  );
  assert.equal(dualshock.buttons[17].name, "extra_0");
  assert.deepEqual(
    dualshock.axes.map((axis) => axis.name),
    ["left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y"],
  );
  assert.deepEqual(dualshock.diagram, { on: [], triggers: ["0.0", "0.0"], sticks: ["100.0,100.0", "190.0,148.0"] });
  assert.deepEqual(dualshock.vibration, ["Strong motor", "Weak motor", "Both", "Triggers"]);
  assert.equal(dualshock.note, null);

  const generic = await until("the second card", () => card(1), (c) => c.rate !== "Waiting for data…");
  assert.equal(generic.name, "USB Gamepad");
  assert.equal(generic.facts, "0079:0011 · no mapping (the browser's own layout) · 10 buttons · 5 axes");
  assert.equal(generic.diagram, null);
  assert.ok(generic.buttons.every((button) => button.name === ""));
  assert.equal(generic.axes.length, 5);
  assert.deepEqual(generic.vibration, ["Strong motor (disabled)", "Weak motor (disabled)", "Both (disabled)"]);
  assert.equal(generic.note, "This browser cannot vibrate this gamepad.");

  // Raw values and the canonical drawing follow the gamepad. Raw Y grows
  // downwards; the drawing's canonical Y grows upwards (ADR 0001).
  await setGamepads([with_(DUALSHOCK, { buttons: { 0: 1, 7: 0.5, 12: 1 }, axes: [0.5, -1, 0, 0.25] }), GENERIC]);
  dualshock = await until("the pressed buttons", () => card(0), (c) => c.buttons[0].pressed && c.axes[1].bar === "-1");
  assert.deepEqual(dualshock.buttons[0], { name: "south", value: "1.00", pressed: true, touched: false });
  assert.deepEqual(dualshock.buttons[7], { name: "right_trigger", value: "0.50", pressed: false, touched: true });
  assert.deepEqual(
    dualshock.axes.map((axis) => [axis.value, axis.bar]),
    [["+0.500", "0.5"], ["−1.000", "-1"], ["+0.000", "0"], ["+0.250", "0.25"]],
  );
  assert.deepEqual(dualshock.diagram, {
    on: ["dpad_up", "south"],
    triggers: ["0.0", "25.0"],
    sticks: ["107.5,85.0", "190.0,151.8"],
  });

  // Vibration: a one-second test, then the motors stop.
  await page.locator(".device").nth(0).getByRole("button", { name: "Strong motor" }).click();
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
  await page.locator(".device").nth(0).getByRole("button", { name: "Triggers" }).click();
  await page.locator(".device").nth(0).getByRole("button", { name: "Strong motor" }).click();
  calls = await until("the motors to stop", haptics, (log) => log.some((call) => call.call === "reset"));
  assert.deepEqual(
    calls[0],
    { index: 0, call: "playEffect", type: "trigger-rumble", duration: 5000, startDelay: 0,
      strongMagnitude: 0, weakMagnitude: 0, leftTrigger: 1, rightTrigger: 1 },
  );
  assert.equal(calls.at(-1).call, "reset");

  // Unplugging one gamepad leaves the other card live.
  await setGamepads([with_(DUALSHOCK, { buttons: { 1: 1 } })]);
  await until("one card", () => page.locator(".device").count(), (count) => count === 1);
  dualshock = await until("the next press", () => card(0), (c) => c.buttons[1].pressed && !c.buttons[0].pressed);
  assert.deepEqual(dualshock.diagram.on, ["east"]);

  await setGamepads([]);
  await page.waitForSelector(".devices", { state: "detached" });
  await page.waitForSelector(".message");
  assert.equal(await page.locator(".hint").count(), 0);

  // Chromium on macOS (by its user agent) explains why a DualSense may not
  // show, until a gamepad does.
  const mac = await browser.newContext({ userAgent: MAC_CHROME });
  const macPage = await mac.newPage();
  macPage.on("pageerror", (error) => problems.push(`page error (macOS): ${error.message}`));
  await macPage.addInitScript(fakeGamepads);
  await macPage.goto(url, { waitUntil: "networkidle" });
  assert.match(await macPage.innerText(".environment"), /^blink \d+ on macos$/);
  assert.match(await macPage.locator(".hint").innerText(), /^A DualSense does not show in Chrome.*Try Safari/);
  await macPage.evaluate((pads) => window.__setGamepads(pads), [DUALSHOCK]);
  await macPage.waitForSelector(".hint", { state: "detached" });
  await mac.close();

  assert.deepEqual(problems, []);
  console.log(`smoke test passed in ${browserName} (${url})`);
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
