// Fake devices for the smoke tests: neither the Gamepad API nor WebHID has
// test hooks, so these replace `navigator.getGamepads()` and
// `navigator.hid`. Each runs in the page before its scripts
// (`page.addInitScript`).

// Runs in the page before its scripts: `window.__setGamepads(list)` puts
// fake gamepads in the first slots of `navigator.getGamepads()`. They are
// real Gamepad objects as far as `instanceof` goes, with own properties.
// A gamepad with `rumble: true` gets a vibration actuator that records its
// calls in `window.__haptics`.
export const fakeGamepads = () => {
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
export const fakeHid = () => {
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
      featureReports: [
        { reportId: 0x05, items: items(40) },
        { reportId: 0x80, items: items(63) },
      ],
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
    // Already in its full report (as macOS, or an earlier page, left it)
    // when the page sets `__hid.fullReport` before picking it.
    full = window.__hid.fullReport;
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
    // The test command in progress, and the pages of its answer left.
    command = undefined;
    receiveFeatureReport(reportId) {
      window.__hid.features.push(reportId);
      this.full = true;
      // The firmware information, when the page set one.
      if (reportId === 0x20 && window.__hid.firmware) {
        return Promise.resolve(new DataView(new Uint8Array(window.__hid.firmware).buffer));
      }
      const answers = window.__hid.answers;
      if (reportId === 0x22 && answers) {
        const report = new Uint8Array(64);
        report[0] = 0x22;
        report[31] = 0x18;
        return Promise.resolve(new DataView(report.buffer));
      }
      // The answer to a test command (`__hid.answers`, keyed
      // "device:action:parameter"), 56 bytes a page; the usage block
      // (`open`) never says it ended, as real pads do.
      if (reportId === 0x81 && answers) {
        const report = new Uint8Array(64);
        report[0] = 0x81;
        const command = this.command;
        if (command && command.left.length > 0) {
          report.set([command.device, command.action], 1);
          const page = command.left.slice(0, 56);
          command.left = command.left.slice(56);
          report[3] = command.left.length > 0 || command.open ? 3 : 2;
          report.set(page, 4);
        } else {
          this.command = undefined;
        }
        return Promise.resolve(new DataView(report.buffer));
      }
      return Promise.resolve(new DataView(new ArrayBuffer(41)));
    }
    sendReport(reportId, data) {
      window.__hid.outputs.push([reportId, ...data]);
      return Promise.resolve();
    }
    sendFeatureReport(reportId, data) {
      const answers = window.__hid.answers;
      if (reportId !== 0x80 || !answers) return Promise.reject(new Error("unexpected feature report"));
      window.__hid.commands.push([data[0], data[1], data[2]]);
      const answer = answers[`${data[0]}:${data[1]}:${data[2]}`] ?? answers[`${data[0]}:${data[1]}:0`];
      this.command = answer && { device: data[0], action: data[1], left: [...answer.bytes], open: answer.open };
      return Promise.resolve();
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
    fullReport: false,
    features: [],
    commands: [],
    outputs: [],
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
