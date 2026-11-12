// Smoke test of the built site (dist/) in a browser: it loads under its
// Content Security Policy without errors or warnings, the WebAssembly module
// starts, and the tester follows pads:
//
// - Gamepad API: the pad list and the selection, hotplug, raw values, the
//   drawing of each pad's family, the values shown on it, the update rate,
//   the vibration test, the guided wizard on a pad without the standard
//   mapping, and the hint shown in Chromium on macOS while no gamepad
//   shows.
// - WebHID: a Bluetooth DualSense picked in the chooser hides its Gamepad API
//   twin, is decoded (reduced report, then the full one after the switch),
//   shows its touchpad, motion and battery, records its reports to a file,
//   and goes away on disconnection.
// - The header: the theme switch (icons) and the settings stay on the
//   title's right, also on a phone; sending is off, and cannot be turned on
//   unless the build names a database.
// - The privacy page: linked under the tester, under the same policy, in the
//   theme the viewer picked.
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
import { fakeGamepads, fakeHid } from "./fakes.mjs";

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
    normalized: detail.querySelector(".normalized-values") && {
      open: detail.querySelector(".normalized-values").open,
      title: text(detail.querySelector(".normalized-title")),
      badge: text(detail.querySelector(".profile-badge")),
      buttons: [...detail.querySelectorAll(".normalized-buttons .button")].map((cell) => ({
        control: cell.dataset.control,
        value: text(cell.querySelector(".value")),
        pressed: cell.classList.contains("pressed"),
        disagreement: cell.classList.contains("disagreement"),
      })),
      axes: [...detail.querySelectorAll(".normalized-axes .axis")].map((row) => ({
        control: row.dataset.control,
        value: text(row.querySelector(".value")),
        disagreement: row.classList.contains("disagreement"),
      })),
    },
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
    disagreements: detail.querySelectorAll(".disagreement").length,
    diagram: diagram && {
      label: diagram.getAttribute("aria-label"),
      on: [...diagram.querySelectorAll(".control.on")].map((element) => text(element.querySelector("title"))),
      triggers: [...diagram.querySelectorAll(".fill")].map((element) => element.getAttribute("width")),
      // Shapes move a dot; illustrations move a cap.
      sticks: [...diagram.querySelectorAll(".dot, [data-cap]")].map((stick) =>
        stick.matches(".dot") ? `${stick.getAttribute("cx")},${stick.getAttribute("cy")}` : stick.getAttribute("transform"),
      ),
      values: diagram.classList.contains("show-values")
        ? [...diagram.querySelectorAll(".value")].map((value) => value.textContent)
        : null,
      glyphs: [...diagram.querySelectorAll(".glyph-text")].map((glyph) => glyph.textContent),
      // Labels printed on an illustration's face buttons.
      labels: [...diagram.querySelectorAll("[data-label]")].map((mark) => mark.dataset.label),
      controls: [...diagram.querySelectorAll(".control")].map((element) => text(element.querySelector("title"))),
      // Optional controls the pad lacks, dimmed.
      absent: [...diagram.querySelectorAll(".absent")].map((element) => element.dataset.button),
      fingers: [...diagram.querySelectorAll(".finger")].filter((dot) => dot.getAttribute("visibility") === "visible").length,
    },
    vibration: [...detail.querySelectorAll("[data-play]")].map((button) => button.textContent),
    lights: {
      lightbar: detail.querySelector("[data-lightbar]") !== null,
      players: [...detail.querySelectorAll("[data-player]")].map((button) => button.textContent),
    },
    note: text(detail.querySelector(".vibration-box > .note")),
    form: text(detail.querySelector(".report-form")),
    switchShown: detail.querySelector(".switch-box") ? !detail.querySelector(".switch-box").hidden : null,
    touch: text(detail.querySelector(".touchpad-panel .panel-text")),
    motion: [...detail.querySelectorAll(".motion .value")].map((value) => value.textContent),
    battery: text(detail.querySelector(".battery-panel .panel-text")),
  };
};

// Runs in the page: what the guided wizard shows.
const readWizard = () => {
  const box = document.querySelector(".wizard");
  if (!box) return null;
  const text = (element) => element?.textContent.replace(/\s+/g, " ").trim() || null;
  return {
    progress: text(box.querySelector(".wizard-progress")),
    prompt: text(box.querySelector(".wizard-prompt")),
    note: text(box.querySelector(".wizard-note")),
    hint: text(box.querySelector(".wizard-hint")),
    targets: [...box.querySelectorAll(".diagram .target")].map((element) => text(element.querySelector("title"))),
    on: [...box.querySelectorAll(".diagram .control.on")].map((element) => text(element.querySelector("title"))),
    summary: text(box.querySelector(".wizard-summary")),
    results: [...box.querySelectorAll(".wizard-results li")].map((row) =>
      [...row.children].map((part) => part.textContent).join(" "),
    ),
    back: box.querySelector(".wizard-actions button:nth-child(2)").disabled ? "disabled" : "enabled",
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

// A pad of no known family that reports no extra buttons.
const SN30 = {
  id: "8BitDo SN30 Pro (STANDARD GAMEPAD Vendor: 2dc8 Product: 6101)",
  mapping: "standard",
  buttons: Array.from({ length: 17 }, () => 0),
  axes: [0, 0, 0, 0],
};

const DUALSHOCK3 = {
  id: "Sony PLAYSTATION(R)3 Controller (STANDARD GAMEPAD Vendor: 054c Product: 0268)",
  mapping: "standard",
  buttons: Array.from({ length: 17 }, (_, index) => (index === 0 ? 1 : 0)),
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
  // No pad yet: the generic drawing stands in for the selected one.
  await page.waitForSelector(".placeholder .diagram");
  const envText = await page.innerText(".environment");
  assert.match(envText, new RegExp(`^${target.engine} \\d+ on \\w+$`));
  const expectsHint = envText.startsWith("blink") && envText.endsWith("macos");
  assert.equal(await page.locator(".hint").count(), expectsHint ? 1 : 0);

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
  assert.equal(await page.locator(".placeholder").count(), 0);
  assert.equal(pad.facts, '054c:09cc · "standard" mapping · 18 buttons · 4 axes');
  assert.equal(pad.gamepadId, DUALSHOCK.id);
  assert.equal(pad.rawOpen, false);
  assert.equal(pad.buttons.length, 18);
  assert.equal(pad.normalized.open, true);
  // PlayStation pads: Chromium's button 17 is the touchpad click.
  assert.equal(pad.normalized.badge, "PlayStation pad, Gamepad API standard layout");
  assert.equal(pad.normalized.buttons.length, 18);
  assert.equal(pad.normalized.axes.length, 6);
  assert.deepEqual(
    pad.buttons.slice(0, 4).map((button) => button.name),
    ["south", "east", "west", "north"],
  );
  assert.equal(pad.buttons[17].name, "touchpad_click");
  assert.deepEqual(
    pad.axes.map((axis) => axis.name),
    ["left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y"],
  );
  assert.deepEqual(pad.diagram, {
    label: "DualShock 4, canonical positions",
    on: [],
    triggers: ["0.0", "0.0"],
    sticks: ["translate(0.0 0.0)", "translate(0.0 0.0)"],
    values: null,
    glyphs: [],
    labels: [],
    controls: [
      "left_shoulder", "right_shoulder", "touchpad_click", "select", "start", "dpad_up",
      "dpad_right", "dpad_down", "dpad_left", "north", "west", "east", "south",
      "left_stick", "right_stick", "home",
    ],
    absent: [],
    fingers: 0,
  });
  assert.deepEqual(pad.vibration, ["Strong", "Weak", "Both", "Triggers", "Stop"]);
  assert.deepEqual(pad.lights, { lightbar: false, players: [] });
  assert.equal(pad.note, null);

  await select("USB Gamepad");
  const generic = await until("the second pad", detail, (d) => d.ready && d.name === "USB Gamepad" && d.rate !== "Waiting for data…");
  assert.equal(generic.facts, "0079:0011 · no mapping (the browser's own layout) · 10 buttons · 5 axes");
  assert.equal(generic.diagram, null);
  assert.equal(generic.rawOpen, true);
  assert.ok(generic.buttons.every((button) => button.name === ""));
  assert.equal(generic.axes.length, 5);
  assert.deepEqual(generic.vibration, []);
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
  assert.deepEqual(pad.diagram.triggers, ["0.0", "55.0"]);
  assert.deepEqual(pad.diagram.sticks, ["translate(12.0 -24.0)", "translate(0.0 6.0)"]);

  // The values on the drawing.
  await page.locator(".toggle").click();
  pad = await until("the values", detail, (d) => d.diagram.values !== null);
  assert.deepEqual(pad.diagram.values, ["0.00", "0.50", "+0.50 +1.00", "+0.00 −0.25"]);
  assert.equal(await page.getAttribute(".toggle", "aria-pressed"), "true");
  await page.locator(".toggle").click();
  await until("the values hidden", detail, (d) => d.diagram.values === null);

  // Vibration: the constant pattern for a second at the strong motor's
  // level, then the motors stop.
  await page.click("[data-play=strong]");
  let calls = await until("the strong motor test", haptics, (log) => log.some((call) => call.call === "reset"));
  assert.deepEqual(
    calls.map(({ index, call, type, strongMagnitude, weakMagnitude }) => ({ index, call, type, strongMagnitude, weakMagnitude })),
    [
      { index: 0, call: "playEffect", type: "dual-rumble", strongMagnitude: 1, weakMagnitude: 0 },
      { index: 0, call: "reset", type: undefined, strongMagnitude: undefined, weakMagnitude: undefined },
    ],
  );
  // Levels and patterns: the weak motor at half, in pulses that switch it
  // on and off, and the trigger motors at the same time.
  await page.evaluate(() => (window.__haptics = []));
  await page.locator("[data-level=weak]").fill("50");
  await page.selectOption("[data-pattern]", "pulse");
  await page.click("[data-play=triggers]");
  await page.click("[data-play=weak]");
  calls = await until("the pulses to end", haptics, (log) => log.filter((call) => call.call === "reset").length >= 3);
  const played = calls.filter((call) => call.call === "playEffect");
  assert.ok(played.some((call) => call.type === "trigger-rumble" && call.leftTrigger === 1 && call.rightTrigger === 1), JSON.stringify(played));
  assert.ok(played.some((call) => call.type === "dual-rumble" && call.weakMagnitude === 0.5 && call.strongMagnitude === 0), JSON.stringify(played));
  assert.equal(calls.at(-1).call, "reset");
  // From the pad: holding the right shoulder plays the weak motor until it
  // is let go.
  await page.selectOption("[data-pattern]", "constant");
  await page.click("[data-pad-buttons]");
  await page.evaluate(() => (window.__haptics = []));
  await setGamepads([with_(DUALSHOCK, { buttons: { 5: 1 } }), GENERIC]);
  await until("the weak motor from the pad", haptics, (log) => log.some((call) => call.weakMagnitude === 0.5));
  await new Promise((resolve) => setTimeout(resolve, 1300));
  assert.ok(!(await haptics()).some((call) => call.call === "reset"), "held: the pattern repeats");
  await setGamepads([DUALSHOCK, GENERIC]);
  await until("the motor to stop", haptics, (log) => log.at(-1)?.call === "reset");
  await page.click("[data-pad-buttons]");

  // The guided wizard on the pad without the standard mapping: buttons by
  // index, the d-pad on axes 3 and 4, sticks on axes 0 to 2. Pressing the
  // first two buttons it identified together says a control is not there,
  // as the "Not on this pad" button does.
  await select("USB Gamepad");
  await until("the generic pad", detail, (d) => d.ready && d.name === "USB Gamepad");
  await page.getByRole("button", { name: "Identify the buttons" }).click();
  const wizard = () => page.evaluate(readWizard);
  let step = await until("the first step", wizard, (w) => w?.note === null && w.prompt !== null);
  assert.deepEqual(step, {
    progress: "Step 1 of 21",
    prompt: "Press the bottom face button.",
    note: null,
    hint: null,
    targets: ["south"],
    on: [],
    summary: null,
    results: [],
    back: "disabled",
  });
  assert.equal(await page.locator(".vibration-box").count(), 0);
  const axesWith = (index, value) => GENERIC.axes.map((rest, i) => (i === index ? value : rest));
  // Uses the pad as `change` says, waits for `note`, lets go and waits for
  // the next step.
  const answer = async (change, note) => {
    const before = (await wizard()).progress;
    await setGamepads([DUALSHOCK, with_(GENERIC, change)]);
    await until(`"${note}" at ${before}`, wizard, (w) => w.note === note);
    await setGamepads([DUALSHOCK, GENERIC]);
    await until(`the step after ${before}`, wizard, (w) => w.progress !== before);
  };
  const skipButton = async () => {
    const before = (await wizard()).progress;
    await page.getByRole("button", { name: "Not on this pad" }).click();
    await until(`the step after ${before}`, wizard, (w) => w.progress !== before);
  };
  const chord = { buttons: { 2: 1, 1: 1 } };
  const skipping = "Not on this pad: let go to confirm.";
  await answer({ buttons: { 2: 1 } }, "Detected b2. Let go to confirm.");
  await answer({ buttons: { 1: 1 } }, "Detected b1. Let go to confirm.");
  step = await wizard();
  assert.equal(step.prompt, "Press the left face button.");
  assert.equal(step.hint, "Not on this pad? Press the bottom and right face buttons together.");
  // The bottom face button alone is taken.
  await setGamepads([DUALSHOCK, with_(GENERIC, { buttons: { 2: 1 } })]);
  await until("the conflict", wizard, (w) => w.note === "That one is already the bottom face button.");
  await setGamepads([DUALSHOCK, GENERIC]);
  await answer(chord, skipping);
  await answer({ buttons: { 0: 1 } }, "Detected b0. Let go to confirm.");
  await answer({ axes: axesWith(4, -1) }, "Detected -a4. Let go to confirm.");
  await answer({ axes: axesWith(4, 1) }, "Detected +a4. Let go to confirm.");
  await answer({ axes: axesWith(3, -1) }, "Detected -a3. Let go to confirm.");
  await answer({ axes: axesWith(3, 1) }, "Detected +a3. Let go to confirm.");
  for (const index of [4, 5, 6, 7, 8, 9]) {
    await answer({ buttons: { [index]: 1 } }, `Detected b${index}. Let go to confirm.`);
  }
  await skipButton(); // home
  await answer({ axes: axesWith(0, 1) }, "Detected a0. Let go to confirm.");
  await answer({ axes: axesWith(1, -1) }, "Detected a1~. Let go to confirm.");
  await answer({ buttons: { 3: 1 } }, "Detected b3. Let go to confirm.");
  await answer({ axes: axesWith(2, 1) }, "Detected a2. Let go to confirm.");
  await answer(chord, skipping); // right stick, up
  step = await wizard();
  assert.equal(step.prompt, "Press the right stick, pushed in like a button (R3).");
  await skipButton();
  step = await until("the summary", wizard, (w) => w.summary !== null);
  assert.equal(step.progress, "All 21 steps done");
  assert.equal(step.prompt, "All set.");
  assert.equal(step.summary, "17 controls identified, 4 not on this pad.");
  assert.deepEqual(step.results.slice(0, 5), [
    "The bottom face button b2",
    "The right face button b1",
    "The left face button not on this pad",
    "The top face button b0",
    "Up on the d-pad -a4",
  ]);
  assert.deepEqual(step.results.slice(10, 12), [
    "The left trigger (LT, L2, ZL) b6",
    "The right trigger (RT, R2, ZR) b7",
  ]);
  assert.equal(step.results.length, 21);
  // Back to the last step, and done again.
  await page.getByRole("button", { name: "Back" }).click();
  await until("the last step again", wizard, (w) => w.progress === "Step 21 of 21");
  await skipButton();
  // What was identified drives the drawing, here and once closed.
  await setGamepads([DUALSHOCK, with_(GENERIC, { buttons: { 2: 1 }, axes: axesWith(4, -1) })]);
  step = await until("the identified controls", wizard, (w) => w.on.length === 2);
  assert.deepEqual(step.on, ["dpad_up", "south"]);
  await page.getByRole("button", { name: "Close" }).click();
  pad = await until("the live view", detail, (d) => d.ready && d.buttons.length === 10 && d.diagram?.on.length === 2);
  assert.equal(pad.diagram.label, "Controller, canonical positions");
  assert.deepEqual(pad.diagram.on, ["dpad_up", "south"]);
  assert.deepEqual(
    pad.buttons.map((button) => button.name),
    ["north", "east", "south", "left_stick", "left_shoulder", "right_shoulder", "left_trigger", "right_trigger", "select", "start"],
  );
  assert.deepEqual(
    pad.axes.map((axis) => axis.name),
    ["left_stick_x", "left_stick_y", "right_stick_x", "dpad_left dpad_right", "dpad_up dpad_down"],
  );
  assert.equal(pad.rawOpen, false);
  await setGamepads([DUALSHOCK, GENERIC]);
  await select("Wireless Controller");

  // Unplugging one gamepad leaves the other live.
  await setGamepads([with_(DUALSHOCK, { buttons: { 1: 1 } })]);
  await until("one pad", padNames, (names) => names.length === 1);
  pad = await until("the next press", detail, (d) => d.buttons[1]?.pressed && !d.buttons[0].pressed);
  assert.deepEqual(pad.diagram.on, ["east"]);

  // The Xbox illustration for all three: Xbox labels and C and Z dimmed on
  // the Xbox pad; no labels on the generic pads, and C and Z dimmed unless
  // the pad reports them. No back buttons on any.
  const xboxControls = [
    "left_shoulder", "right_shoulder", "home", "select", "start", "share", "left_stick", "right_stick",
    "dpad_up", "dpad_left", "dpad_right", "dpad_down", "north", "west", "east", "south",
  ];
  await setGamepads([XBOX, FLYDIGI, SN30]);
  await until("the Xbox pad", detail, (d) => d?.ready && d.diagram?.label === "Xbox controller, canonical positions");
  pad = await detail();
  assert.deepEqual(pad.diagram.labels, ["y", "x", "b", "a"]);
  assert.deepEqual(pad.diagram.controls, xboxControls);
  assert.deepEqual(pad.diagram.absent, ["extra_0", "extra_1"]);
  await select("Flydigi Vader 5 Pro");
  pad = await until("the generic pad", detail, (d) => d.ready && d.diagram?.label === "Controller, canonical positions");
  assert.deepEqual(pad.diagram.on, ["extra_0"]);
  assert.deepEqual(pad.diagram.labels, []);
  assert.deepEqual(pad.diagram.controls, [...xboxControls, "extra_0", "extra_1"]);
  assert.deepEqual(pad.diagram.absent, []);
  await select("8BitDo SN30 Pro");
  pad = await until("the pad without C and Z", detail, (d) => d.ready && d.name === "8BitDo SN30 Pro" && d.diagram);
  assert.deepEqual(pad.diagram.controls, [...xboxControls]);
  assert.deepEqual(pad.diagram.absent, ["extra_0", "extra_1"]);

  // The DualShock 3 illustration.
  await setGamepads([DUALSHOCK3]);
  pad = await until("the DualShock 3", detail, (d) => d?.ready && d.diagram?.label === "DualShock 3, canonical positions");
  assert.deepEqual(pad.diagram.controls, [
    "left_shoulder", "right_shoulder", "select", "start", "home", "dpad_up", "dpad_down", "dpad_left",
    "dpad_right", "north", "west", "east", "south", "left_stick", "right_stick",
  ]);
  pad = await until("the DualShock 3's press", detail, (d) => d.diagram.on.length > 0);
  assert.deepEqual(pad.diagram.on, ["south"]);

  // W5: Guided wizard on a profiled pad that confirms a disagreeing mapping
  // highlights the disagreement in both raw values and normalized values.
  assert.equal(pad.normalized.badge, "Gamepad API standard layout");
  assert.equal(pad.disagreements, 0);
  await setGamepads([with_(DUALSHOCK3, { buttons: { 0: 0 } })]);
  await page.getByRole("button", { name: "Identify the buttons" }).click();
  await until("the wizard prompt", wizard, (w) => w?.prompt !== null && w?.note === null);
  // Map South to b1 instead of b0 (disagrees with active profile!)
  await setGamepads([with_(DUALSHOCK3, { buttons: { 0: 0, 1: 1 } })]);
  await until("detected b1", wizard, (w) => w?.note === "Detected b1. Let go to confirm.");
  await setGamepads([with_(DUALSHOCK3, { buttons: { 0: 0, 1: 0 } })]);
  await until("step 2", wizard, (w) => w?.progress === "Step 2 of 21");
  // Map East to b0 instead of b1 (disagrees with active profile!)
  await setGamepads([with_(DUALSHOCK3, { buttons: { 0: 1, 1: 0 } })]);
  await until("detected b0", wizard, (w) => w?.note === "Detected b0. Let go to confirm.");
  await setGamepads([with_(DUALSHOCK3, { buttons: { 0: 0, 1: 0 } })]);
  await until("step 3", wizard, (w) => w?.progress === "Step 3 of 21");
  // Skip the remaining steps to finish the wizard
  for (let s = 3; s <= 21; s++) {
    await page.getByRole("button", { name: "Not on this pad" }).click();
    if (s < 21) {
      await until(`step ${s + 1}`, wizard, (w) => w?.progress === `Step ${s + 1} of 21`);
    }
  }
  await until("summary ready", wizard, (w) => w?.summary !== null);
  await page.getByRole("button", { name: "Close" }).click();
  pad = await until("disagreement highlighted", detail, (d) => d?.ready && d.disagreements > 0);
  assert.ok(pad.disagreements >= 2);
  assert.equal(pad.normalized.buttons.find((b) => b.control === "south")?.disagreement, true);
  assert.equal(pad.normalized.buttons.find((b) => b.control === "east")?.disagreement, true);

  // W6: Local report store, sharing and consent flow.
  assert.equal(
    await page.locator(".save-report-note").innerText(),
    "Saves a report of this pad in this browser. Nothing is sent: you can look at it, download it or delete it at any time.",
  );
  assert.equal(
    await page.locator(".saved-reports-note").innerText(),
    "Saved reports stay in this browser. A private window drops them when it closes, and Safari may delete them after seven days without a visit: download the ones you want to keep.",
  );

  await page.locator(".save-report-btn").click();
  await until("report saved notification", () => page.locator(".save-notice").innerText(), (t) => t?.includes("Report saved for"));
  await page.waitForSelector(".saved-reports .report-row");

  const reportRow = page.locator(".saved-reports .report-row").first();
  assert.ok((await reportRow.locator(".report-name").innerText()).length > 0);
  assert.match(await reportRow.locator(".report-facts").innerText(), /[a-f0-9]+ · \d{4}-\d\d-\d\d/);

  // Download saved report: check filename and JSON validity
  const [reportDownload] = await Promise.all([
    page.waitForEvent("download", { timeout: 10000 }),
    reportRow.locator(".report-btn", { hasText: "Download" }).click(),
  ]);
  assert.match(reportDownload.suggestedFilename(), /^[a-f0-9]+-\d{4}-\d\d-\d\d\.json$/);
  const downloadedReport = JSON.parse(await readFile(await reportDownload.path(), "utf8"));
  assert.equal(downloadedReport.format, "anygamepad-device-report");
  assert.equal(downloadedReport.schema_version, 1);
  assert.equal(downloadedReport.source.app, "web");
  // The web app's version is the workspace's (scripts/version.mjs).
  const { version: appVersion } = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(downloadedReport.source.app_version, appVersion);
  assert.ok(downloadedReport.views.length >= 1);
  assert.match(downloadedReport.views[0].fingerprint, /^[a-f0-9]{16}$/);
  // Confirmed wizard results should be present in the saved report
  assert.ok(downloadedReport.views[0].confirmed !== undefined);
  assert.ok(downloadedReport.user.missing_buttons.includes("west"));
  assert.ok(downloadedReport.user.missing_buttons.includes("right_stick"));
  assert.ok(downloadedReport.user.missing_buttons.includes("home"));

  // Open "Share this report" modal dialog
  await reportRow.locator(".report-btn", { hasText: "Share" }).click();
  await page.waitForSelector(".share-modal");
  const modalText = await page.locator(".share-modal").innerText();
  // Verbatim P3 consent text assertions
  assert.match(modalText, /Sharing posts this report as an issue on GitHub, from your GitHub account\./);
  assert.match(modalText, /The report holds no serial number, Bluetooth address or other personal data\./);
  assert.match(modalText, /By posting it, you dedicate the report to the public domain under CC0 1\.0/);
  assert.match(modalText, /No GitHub account\? Download the report instead: it stays yours\./);

  // Test "Copy and open the issue form"
  await page.evaluate(() => {
    window.__openedUrls = [];
    window.open = (url) => { window.__openedUrls.push(url); return null; };
  });
  await page.locator(".copy-open-issue-btn").click();
  await until("copied notice displayed", () => page.locator(".share-modal").innerText(), (t) => t?.includes("The report is copied. Paste it into the Report field of the form") || t?.includes("Select the report above and copy it"));
  const openedUrls = await page.evaluate(() => window.__openedUrls);
  assert.equal(openedUrls.length, 1);
  assert.match(openedUrls[0], /github\.com\/anygamepad\/anygamepad\.github\.io\/issues\/new\?template=device-report\.yml/);

  // Close share modal
  await page.locator(".close-btn").click();
  await page.waitForSelector(".share-modal", { state: "detached" });

  // Marked as shared badge now visible on the report row
  assert.equal((await reportRow.locator(".report-shared-badge").innerText()).toLowerCase(), "shared");

  // Delete saved report with confirmation dialog
  page.once("dialog", (dialog) => {
    assert.equal(dialog.message(), "Delete this report from this browser?");
    dialog.accept();
  });
  await reportRow.locator(".report-btn", { hasText: "Delete" }).click();
  await page.waitForSelector(".saved-reports .report-row", { state: "detached" });
  assert.equal(await page.locator(".empty-reports").innerText(), "No reports saved in this browser yet.");

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

  // The guided wizard checks the decoding: the decoded controls answer.
  const hidButtons = (buttons) => page.evaluate((value) => (window.__hid.state.buttons = value), buttons);
  await hidButtons([0x08, 0, 0]);
  await page.getByRole("button", { name: "Identify the buttons" }).click();
  step = await until("the DualSense's first step", wizard, (w) => w?.note === null && w.prompt !== null);
  assert.equal(step.prompt, "Press the bottom face button (Cross).");
  await hidButtons([0x28, 0, 0]); // Cross
  await until("Cross", wizard, (w) => w.note === "Detected b{button:south}. Let go to confirm.");
  await hidButtons([0x08, 0, 0]);
  step = await until("the second step", wizard, (w) => w.progress === "Step 2 of 21");
  assert.equal(step.prompt, "Press the right face button (Circle).");
  await page.getByRole("button", { name: "Close" }).click();
  await until("the WebHID view", detail, (d) => d.form !== null && d.diagram !== null);
  await hidButtons([0x20 | 0x00, 0x01, 0x01]);

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
  // In °/s and m/s²: the fake calibration report is empty, so the nominal
  // scales apply (16 counts per °/s, 8192 per g).
  assert.deepEqual(pad.motion, ["-6", "0", "0", "9.81", "0.00", "0.00"]);
  assert.equal(pad.battery, "55% · charging");

  // Outputs: the library writes the DualSense's Bluetooth output report
  // (0x31, numbered, with the light bar and player LED flags), sending
  // nothing until asked.
  assert.deepEqual(await page.evaluate(() => window.__hid.outputs), []);
  assert.deepEqual(pad.vibration, ["Strong", "Weak", "Both", "Stop"]);
  assert.deepEqual(pad.lights, { lightbar: true, players: ["Off", "1", "2", "3", "4"] });
  await page.locator("[data-lightbar]").fill("#0000ff");
  await until("the light bar", () => page.evaluate(() => window.__hid.outputs.length), (n) => n === 1);
  await page.click('[data-player="2"]');
  const outputs = await until(
    "two output reports",
    () => page.evaluate(() => window.__hid.outputs),
    (sent) => sent.length === 2,
  );
  const [blue, player] = outputs;
  assert.equal(blue.length, 78);
  assert.deepEqual(blue.slice(0, 5), [0x31, 0x00, 0x10, 0x00, 0x04]);
  assert.deepEqual(blue.slice(47, 50), [0, 0, 255]);
  assert.deepEqual(player.slice(0, 5), [0x31, 0x10, 0x10, 0x00, 0x14]);
  assert.equal(player[46], 0x0a);

  // The controller's information, asked for: firmware, board and colour
  // (the serial number's two characters that tell them, and nothing
  // else), the battery voltage and the usage counters. Only the reader's
  // commands go out.
  await page.evaluate(() => {
    const ascii = (text) => [...text].map((c) => c.charCodeAt(0));
    const firmware = new Array(64).fill(0);
    firmware[0] = 0x20;
    firmware.splice(1, 11, ...ascii("Jun 24 2024"));
    firmware.splice(12, 8, ...ascii("11:16:21"));
    firmware.splice(20, 12, 2, 0, 4, 0, 0x10, 0x04, 0, 0, 0, 0, 0x0f, 0x01);
    firmware.splice(44, 2, 0x20, 0x05);
    const motor = new Array(43).fill(0);
    motor.splice(19, 8, ...ascii("M1418AB3"));
    const usage = new Array(1 + 4 * 56).fill(0);
    usage.splice(1, 17, ...ascii("A21600XYZ12345678"));
    usage.splice(1 + 21, 3, 0x4e, 0xfb, 0x00); // 64334 s
    usage.splice(1 + 130, 2, 0xb9, 0x04); // 1209 presses of Cross
    window.__hid.firmware = firmware;
    window.__hid.answers = {
      "1:19:0": { bytes: [...ascii("A21600XYZ12345678"), ...new Array(15).fill(0)] },
      "7:37:1": { bytes: motor },
      "7:37:2": { bytes: motor },
      "4:3:0": { bytes: [0x30, 0x0c, 0, 0] },
      "112:1:0": { bytes: usage, open: true },
    };
  });
  await page.click("[data-read-info]");
  await until("the controller's information", () => page.getAttribute("[data-info]", "data-info"), (state) => state === "done");
  const info = (
    await page.evaluate(() =>
      [...document.querySelectorAll(".pad-info dl > div")].map((row) => `${row.firstChild.textContent} ${row.lastChild.textContent}`),
    )
  ).join(" | ");
  for (const fact of ["Built Jun 24 2024 11:16:21", "Update version 5.20", "Main firmware 1.15.0", "Board BDM-020",
    "Colour White", "Trigger motors M1418AB3 (L), M1418AB3 (R)", "Battery voltage 3.12 V", "Time in use 17h 52m 14s", "Cross 1209"]) {
    assert.ok(info.includes(fact), `${fact} in ${info}`);
  }
  assert.doesNotMatch(info, /XYZ|12345678/);
  assert.deepEqual(await page.evaluate(() => window.__hid.commands), [[1, 19, 0], [7, 37, 1], [7, 37, 2], [4, 3, 0], [112, 1, 0]]);
  await page.evaluate(() => (window.__hid.answers = undefined));

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
  await page.waitForSelector(".placeholder .diagram");

  // The activity tab lists the newest events, and downloads the log: the
  // whole session, in time order.
  await page.click("[data-tab=activity]");
  await page.waitForURL(/#activity$/);
  await until("the events listed", () => page.locator("[data-activity] .event").count(), (n) => n > 10);
  assert.equal(await page.locator(".detail").count(), 0);
  const [logDownload] = await Promise.all([
    page.waitForEvent("download", { timeout: 10000 }),
    page.getByRole("button", { name: "Download the activity log" }).click(),
  ]);
  assert.match(logDownload.suggestedFilename(), /^anygamepad-activity-\d{4}-\d\d-\d\dT\d\d-\d\d-\d\d\.json$/);
  const log = JSON.parse(await readFile(await logDownload.path(), "utf8"));
  assert.equal(log.format, "anygamepad-activity-log-v1");
  assert.match(log.library, /^\d+\.\d+\.\d+$/);
  assert.equal(log.environment.browser.engine, target.engine);
  assert.equal(log.dropped_input_events, 0);
  assert.ok(log.events.every((event, i) => i === 0 || log.events[i - 1].t <= event.t), "events in time order");
  const of = (type) => log.events.filter((event) => event.type === type);
  assert.equal(log.events[0].type, "session");
  assert.deepEqual(
    of("gamepad_added").map((event) => event.identity.web.id),
    // The DualSense's Gamepad API twin stays connected while WebHID reads
    // the pad: the list only hides it.
    [DUALSHOCK.id, GENERIC.id, XBOX.id, FLYDIGI.id, SN30.id, DUALSHOCK3.id, DUALSENSE.id],
  );
  assert.equal(of("gamepad_removed").length, of("gamepad_added").length);
  const dualshock = of("gamepad_added")[0].pad;
  assert.ok(of("button").some((event) => event.pad === dualshock && event.index === 0 && event.pressed));
  assert.ok(of("axis").some((event) => event.pad === dualshock && event.index === 1 && event.value === -1));
  assert.ok(of("canonical_buttons").some((event) => event.pad === dualshock && event.pressed.join() === "south,dpad_up,right_trigger"));
  assert.deepEqual(of("vibration_test").map((event) => event.motors.join()), ["strong", "triggers", "weak"]);
  // The controller's information: the model's facts, not the counters.
  const [controllerInfo] = of("controller_info");
  assert.equal(controllerInfo.board, "BDM-020");
  assert.equal(controllerInfo.usage_counters > 0, true);
  assert.doesNotMatch(JSON.stringify(log), /XYZ12345678|64334/);
  const [hid] = of("hid_added");
  assert.equal(hid.identity.product_id, 0x0ce6);
  assert.deepEqual(of("hid_chooser").map((event) => event.picked), [1]);
  assert.deepEqual(
    of("hid_report_sample").map((event) => [event.report_id, event.length]),
    [[0x01, 9], [0x31, 77]],
  );
  assert.deepEqual(of("hid_report_form").map((event) => event.form), ["bluetooth_reduced", "bluetooth_full"]);
  assert.ok(["hid_opened", "hid_switch_to_full", "hid_recording", "hid_removed", "power", "touches"].every((type) => of(type).length > 0));
  // The guided wizard: every step, the one done again, and the result.
  const genericPad = of("gamepad_added")[1].pad;
  const steps = of("wizard_step").filter((event) => event.pad === genericPad);
  assert.equal(steps.length, 22);
  assert.deepEqual(steps[0].step, { kind: "button", button: "south" });
  assert.deepEqual(steps[0].inputs, [{ target: { kind: "button", button: "south" }, input: "b2" }]);
  assert.equal(steps[2].missing, true);
  assert.equal(of("wizard_back").length, 1);
  assert.deepEqual(of("wizard_done").find((event) => event.pad === genericPad).missing, { buttons: ["west", "right_stick", "home"], axes: ["right_stick_y"] });
  assert.deepEqual(
    of("wizard_step").filter((event) => event.pad === hid.pad).map((event) => event.inputs[0].input),
    ["b{button:south}"],
  );
  await context.close();

  // The theme follows the system until the viewer picks one, which a reload
  // keeps.
  const themed = await browser.newContext({ colorScheme: "light" });
  const themePage = await themed.newPage();
  watch(themePage, "(theme)");
  await themePage.goto(url, { waitUntil: "networkidle" });
  await themePage.waitForSelector(".theme");
  const theme = () =>
    themePage.evaluate(() => ({
      attribute: document.documentElement.dataset.theme ?? null,
      background: getComputedStyle(document.body).backgroundColor,
      pressed: [...document.querySelectorAll(".theme button")]
        .filter((button) => button.getAttribute("aria-pressed") === "true")
        .map((button) => button.getAttribute("aria-label")),
    }));
  const LIGHT = "rgb(247, 247, 251)";
  const DARK = "rgb(20, 20, 28)";
  const expect = (want) => until(JSON.stringify(want), theme, (have) => JSON.stringify(have) === JSON.stringify(want));
  const pick = (name) => themePage.getByRole("button", { name, exact: true }).click();
  await expect({ attribute: null, background: LIGHT, pressed: ["Auto"] });
  // Icons only, in the footer; the tabs on the title's right on a wide
  // screen, under it on a phone.
  assert.deepEqual(await themePage.locator(".theme button").allTextContents(), ["", "", ""]);
  assert.equal(await themePage.locator(".site .tools").count(), 1);
  for (const width of [1280, 360, 320]) {
    await themePage.setViewportSize({ width, height: 800 });
    const [title, brand, tabs] = await Promise.all(
      [".title", ".brand a > span", ".tabs"].map((s) => themePage.locator(s).boundingBox()),
    );
    if (width > 640) {
      assert.ok(tabs.x >= title.x + title.width && tabs.y < title.y + title.height, `${width}px: ${JSON.stringify({ title, tabs })}`);
    } else {
      assert.ok(tabs.y >= title.y + title.height, `${width}px: ${JSON.stringify({ title, tabs })}`);
    }
    assert.ok(brand.x + brand.width <= width, `${width}px: the title fits (${JSON.stringify({ brand })})`);
    const overflow = await themePage.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    assert.equal(overflow, 0, `${width}px: no horizontal scroll`);
  }
  // The settings: sending is off, and cannot be turned on unless the build
  // names a database (ANYGAMEPAD_SYNC_URL).
  const database = await themePage.locator('meta[name="anygamepad-sync"]').count();
  await themePage.getByRole("button", { name: "Settings", exact: true }).click();
  assert.equal(await themePage.isDisabled("[data-sending]"), database === 0);
  assert.equal(await themePage.isChecked("[data-sending]"), false);
  if (database === 0) await themePage.waitForSelector("[data-sending-unavailable]");
  await themePage.click(".settings-modal .close-btn");
  await themePage.waitForSelector(".settings-modal", { state: "detached" });
  await pick("Dark");
  await expect({ attribute: "dark", background: DARK, pressed: ["Dark"] });
  await themePage.reload({ waitUntil: "networkidle" });
  await themePage.waitForSelector(".theme");
  await expect({ attribute: "dark", background: DARK, pressed: ["Dark"] });
  // The privacy policy, linked under the tester, loads under the same policy
  // and keeps the theme.
  await themePage.getByRole("link", { name: "Privacy", exact: true }).click();
  await themePage.waitForURL(/\/privacy\/$/, { waitUntil: "networkidle" });
  assert.equal(await themePage.locator("h1").innerText(), "Privacy");
  assert.equal(await themePage.title(), "anygamepad: privacy");
  assert.equal(
    await themePage.getAttribute('meta[http-equiv="Content-Security-Policy"]', "content"),
    policy,
  );
  await until(
    "the privacy page in the dark theme",
    () => themePage.evaluate(() => [document.documentElement.dataset.theme, getComputedStyle(document.body).backgroundColor]),
    ([attribute, background]) => attribute === "dark" && background === DARK,
  );
  assert.match(await themePage.locator(".policy").innerText(), /CC0 1\.0.*IP address/s);
  await themePage.goBack({ waitUntil: "networkidle" });
  await themePage.waitForSelector(".theme");
  await themePage.emulateMedia({ colorScheme: "dark" });
  await pick("Light");
  await expect({ attribute: "light", background: LIGHT, pressed: ["Light"] });
  await pick("Auto");
  await expect({ attribute: null, background: DARK, pressed: ["Auto"] });
  await themed.close();

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
