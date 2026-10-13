// Smoke test of the built site (dist/) in Chrome: it loads under its Content
// Security Policy without errors or warnings, the WebAssembly module starts,
// and a gamepad shows up and goes away. Chrome's Gamepad API has no test
// hooks, so the gamepad is a fake behind `navigator.getGamepads()`.
//
// CHROME_PATH must point at a Chrome or Chromium binary.

import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { preview } from "vite";

const executablePath = process.env.CHROME_PATH;
if (!executablePath) {
  console.error("Set CHROME_PATH to a Chrome or Chromium binary.");
  process.exit(2);
}

// Runs in the page before its scripts: `window.__setGamepads(list)` puts
// fake gamepads in the first slots of `navigator.getGamepads()`. They are
// real Gamepad objects as far as `instanceof` goes, with own properties.
const fakeGamepads = () => {
  const button = (value) =>
    Object.defineProperties(Object.create(GamepadButton.prototype), {
      pressed: { value: value > 0.5 },
      touched: { value: value > 0 },
      value: { value },
    });
  const gamepad = (index, { id, mapping, buttons, axes }) =>
    Object.defineProperties(Object.create(Gamepad.prototype), {
      id: { value: id },
      index: { value: index },
      connected: { value: true },
      mapping: { value: mapping },
      timestamp: { get: () => performance.now() },
      axes: { value: axes },
      buttons: { value: buttons.map(button) },
      vibrationActuator: { value: null },
    });
  let slots = [null, null, null, null];
  window.__setGamepads = (list) => {
    slots = [0, 1, 2, 3].map((i) => (list[i] ? gamepad(i, list[i]) : null));
  };
  Navigator.prototype.getGamepads = () => slots;
};

const DUALSHOCK = {
  id: "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)",
  mapping: "standard",
  buttons: Array.from({ length: 18 }, () => 0),
  axes: [0, 0, 0, 0],
};

const server = await preview({ preview: { host: "127.0.0.1", port: 4317 }, logLevel: "warn" });
const url = server.resolvedUrls.local[0];
const browser = await chromium.launch({ executablePath });
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
  assert.match(await page.innerText(".environment"), /^blink \d+ on \w+$/);

  await page.evaluate((pad) => window.__setGamepads([pad]), DUALSHOCK);
  await page.waitForSelector(".device");
  const card = await page.innerText(".device");
  assert.match(card, /Wireless Controller/);
  assert.match(card, /054c:09cc/);
  assert.match(card, /standard/);
  assert.match(card, /18 \/ 4/);

  await page.evaluate(() => window.__setGamepads([]));
  await page.waitForSelector(".devices", { state: "detached" });
  await page.waitForSelector(".message");

  assert.deepEqual(problems, []);
  console.log(`smoke test passed (${url})`);
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
