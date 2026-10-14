// The tester: reads the browser's gamepads and, on request, WebHID devices
// through anygamepad (WebAssembly), keeps the list the page renders, and
// drives the live view of the selected pad. Logic lives here, in TypeScript
// checked by `tsc`; components only render (ADR 0005).
//
// Two loops: a sampler every few milliseconds reads the Gamepad API (hotplug
// and how often each gamepad's data changes, which a once-per-frame read
// would cap at the display's refresh rate), and the frame loop drains the
// WebHID report queues and draws.

import {
  Gamepads,
  HidDevices,
  version,
  type DeviceInfo,
  type Environment,
  type GamepadSnapshot,
  type GamepadState,
  type HidDeviceInfo,
  type OutputCommand,
} from "./wasm/anygamepad_wasm.js";
import { ActivityLog } from "./activity";
import { ControllerDrawing } from "./drawing";
import { HidLiveView } from "./hid-live";
import { layoutFor } from "./layouts";
import { LiveView } from "./live";
import { CountMeter, RateMeter } from "./rate";

/** One connected pad, as the page lists it. */
export interface PadRow {
  /** `gamepad:<id>` or `webhid:<id>`. */
  key: string;
  source: "gamepad" | "webhid";
  name: string;
  /** `vvvv:pppp`, or a dash where the browser hides the IDs. */
  ids: string;
  /** One line of facts under the name. */
  facts: string;
  /** `Gamepad.id` verbatim, or the WebHID product name. */
  detail: string;
  rumble: boolean;
  triggerRumble: boolean;
}

/** The data the page renders. */
export interface TesterView {
  environment: Environment | null;
  pads: PadRow[];
  /** Key of the pad shown large. */
  selected: string | null;
  /** The selected pad's row. */
  current: PadRow | null;
  /** Whether the drawing shows the values. */
  showValues: boolean;
  /** Whether the browser has WebHID. */
  webhid: boolean;
  /** The outcome of the last WebHID action, when it needs saying. */
  hidMessage: string | null;
  error: string | null;
  /** Why a gamepad may not show in this browser, while none does. */
  hint: string | null;
}

export const emptyView = (): TesterView => ({
  environment: null,
  pads: [],
  selected: null,
  current: null,
  showValues: false,
  webhid: false,
  hidMessage: null,
  error: null,
  hint: null,
});

/**
 * A known reason for a gamepad not to show in this browser, if any.
 *
 * macOS switches the DualSense to its extended report, which Chromium's HID
 * reader cannot decode, so Chromium never sees an input and never exposes it
 * (docs/research/06-browser-gamepad-api.md). WebHID reads that report.
 */
export const missingGamepadHint = (environment: Environment): string | null =>
  environment.browser.engine === "blink" && environment.os === "macos"
    ? "A DualSense does not show in Chrome, Edge or other Chromium browsers on macOS: " +
      "macOS switches it to a report mode they cannot read. Connect it with WebHID below, " +
      "or try Safari, which reads it through macOS."
    : null;

/** Which motors a vibration test runs. */
export type RumbleKind = "strong" | "weak" | "both" | "triggers";

/** How long a vibration test runs, in milliseconds. */
export const RUMBLE_MS = 1000;

/** How long a recording of WebHID reports lasts, in milliseconds. */
export const RECORD_MS = 2000;

/** The sampler's period (browsers clamp timers to about 4 ms). */
const SAMPLE_MS = 4;

/** How often the activity log notes each pad's update rate, in milliseconds. */
const RATE_LOG_MS = 10_000;

const hex = (value: number | undefined): string =>
  value === undefined ? "----" : value.toString(16).padStart(4, "0");

const idsOf = (vendor: number | undefined, product: number | undefined): string =>
  vendor === undefined ? "–" : `${hex(vendor)}:${hex(product)}`;

export const gamepadRow = (id: number, info: DeviceInfo): PadRow => {
  const ids = idsOf(info.identity.vendor_id, info.identity.product_id);
  const mapping = info.identity.web?.mapping
    ? `"${info.identity.web.mapping}" mapping`
    : "no mapping (the browser's own layout)";
  return {
    key: `gamepad:${id}`,
    source: "gamepad",
    name: info.identity.product_name ?? info.identity.web?.id ?? "Unknown gamepad",
    ids,
    facts: `${ids} · ${mapping} · ${info.layout.buttons.length} buttons · ${info.layout.axes.length} axes`,
    detail: info.identity.web?.id ?? "",
    rumble: info.outputs.includes("rumble"),
    triggerRumble: info.outputs.includes("trigger_rumble"),
  };
};

const TRANSPORTS: Record<string, string> = { usb: "USB", bluetooth: "Bluetooth" };

export const hidRow = (id: number, info: HidDeviceInfo): PadRow => {
  const ids = idsOf(info.identity.vendor_id, info.identity.product_id);
  const facts = [ids, "WebHID", TRANSPORTS[info.transport], info.decoded ? "decoded" : "raw reports"];
  return {
    key: `webhid:${id}`,
    source: "webhid",
    name: info.identity.product_name ?? "HID device",
    ids,
    facts: facts.filter(Boolean).join(" · "),
    detail: info.identity.product_name ?? "",
    rumble: false,
    triggerRumble: false,
  };
};

const rumbleCommand = (kind: RumbleKind, strength: number): OutputCommand => {
  switch (kind) {
    case "strong":
      return { rumble: { low_frequency: strength, high_frequency: 0 } };
    case "weak":
      return { rumble: { low_frequency: 0, high_frequency: strength } };
    case "both":
      return { rumble: { low_frequency: strength, high_frequency: strength } };
    case "triggers":
      return { trigger_rumble: { left: strength, right: strength } };
  }
};

interface GamepadPad {
  source: "gamepad";
  id: number;
  info: DeviceInfo;
  row: PadRow;
  rate: RateMeter;
  rumbleTimer: number | undefined;
}

interface Recording {
  start: number;
  reports: { t: number; report_id: number; data: string }[];
}

interface HidPad {
  source: "webhid";
  id: number;
  info: HidDeviceInfo;
  row: PadRow;
  rate: CountMeter;
  open: boolean;
  switching: boolean;
  recording: Recording | undefined;
}

type Pad = GamepadPad | HidPad;

const toHex = (data: Uint8Array): string => Array.from(data, (byte) => byte.toString(16).padStart(2, "0")).join("");

/** Downloads `content` as a JSON file. */
const saveJson = (name: string, content: unknown): void => {
  const blob = new Blob([JSON.stringify(content, null, 1)], { type: "application/json" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = name;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(link.href), 1000);
};

/** A timestamp for file names: `2026-10-14T08-47-11`. */
const fileTime = (): string => new Date().toISOString().slice(0, 19).replaceAll(":", "-");

/** A Gamepad API pad as read in one frame. */
interface GamepadRead {
  snapshot: GamepadSnapshot;
  state: GamepadState | undefined;
}

export class Tester {
  private gamepads: Gamepads | undefined;
  private hids: HidDevices | undefined;
  private view: TesterView = emptyView();
  private readonly pads = new Map<string, Pad>();
  private live: { key: string; root: HTMLElement; view: LiveView | HidLiveView } | undefined;
  private sampler: number | undefined;
  private frame = 0;
  private readonly log = new ActivityLog();
  private lastRateLog = 0;

  /** Starts reading gamepads into `view` (a component's reactive data). */
  start(view: TesterView): void {
    this.view = view;
    window.addEventListener("error", (event) => this.log.event("page_error", { message: event.message }));
    window.addEventListener("unhandledrejection", (event) =>
      this.log.event("page_error", { message: String(event.reason) }),
    );
    try {
      this.gamepads = new Gamepads();
    } catch (error) {
      this.fail(`This browser cannot read gamepads (${String(error)}).`);
      return;
    }
    view.environment = this.gamepads.environment();
    view.hint = missingGamepadHint(view.environment);
    if (HidDevices.isSupported()) {
      try {
        this.hids = new HidDevices();
        view.webhid = true;
      } catch {
        // WebHID present but unusable (a policy): the Gamepad API still works.
      }
    }
    this.log.event("session", {
      library: version(),
      environment: view.environment,
      webhid: view.webhid,
      hint: view.hint,
    });
    document.addEventListener("visibilitychange", this.onVisibility);
    this.onVisibility();
    this.frame = requestAnimationFrame(this.draw);
  }

  /** Shows a pad large. */
  readonly select = (key: string): void => {
    this.view.selected = key;
    this.view.current = this.pads.get(key)?.row ?? null;
  };

  /** Shows or hides the values on the drawing. */
  readonly toggleValues = (): void => {
    this.view.showValues = !this.view.showValues;
  };

  /** Opens the browser's WebHID chooser (call it from a click). */
  readonly connectHid = (): void => {
    const hids = this.hids;
    if (!hids) return;
    this.view.hidMessage = null;
    hids.request().then(
      (picked) => {
        this.log.event("hid_chooser", { picked });
        if (picked === 0) this.view.hidMessage = "No device picked.";
      },
      (error: unknown) => this.hidNote(`WebHID refused (${String(error)}).`),
    );
  };

  /** Downloads the activity log (call it from a click). */
  readonly downloadLog = (): void => {
    saveJson(`anygamepad-activity-${fileTime()}.json`, this.log.toJSON(version(), this.view.environment));
  };

  /** Runs a short vibration test on a Gamepad API pad. */
  readonly rumble = (key: string, kind: RumbleKind): void => {
    const pad = this.pads.get(key);
    if (!this.gamepads || pad?.source !== "gamepad") return;
    this.log.event("vibration_test", { pad: key, kind });
    try {
      this.gamepads.send(pad.id, rumbleCommand(kind, 1));
    } catch (error) {
      this.fail(`Vibration failed (${String(error)}).`);
      return;
    }
    // One timer stops every motor, including those of a test it cut short.
    window.clearTimeout(pad.rumbleTimer);
    pad.rumbleTimer = window.setTimeout(() => {
      pad.rumbleTimer = undefined;
      try {
        this.gamepads?.send(pad.id, rumbleCommand("both", 0));
        if (pad.info.outputs.includes("trigger_rumble")) {
          this.gamepads?.send(pad.id, rumbleCommand("triggers", 0));
        }
      } catch {
        // The gamepad went away; its rumble stopped with it.
      }
    }, RUMBLE_MS);
  };

  /** Stops both loops and releases the devices. */
  stop(): void {
    document.removeEventListener("visibilitychange", this.onVisibility);
    window.clearInterval(this.sampler);
    cancelAnimationFrame(this.frame);
    for (const pad of this.pads.values()) {
      if (pad.source === "gamepad") window.clearTimeout(pad.rumbleTimer);
    }
    this.gamepads?.free();
    this.gamepads = undefined;
    this.hids?.free();
    this.hids = undefined;
  }

  /** Shows an error, and logs it. */
  private fail(message: string): void {
    this.view.error = message;
    this.log.event("error", { message });
  }

  /** Shows the outcome of a WebHID action, and logs it. */
  private hidNote(message: string): void {
    this.view.hidMessage = message;
    this.log.event("hid_message", { message });
  }

  // A hidden page needs neither loop (the frame loop pauses by itself).
  private readonly onVisibility = (): void => {
    this.log.event(document.hidden ? "page_hidden" : "page_visible");
    window.clearInterval(this.sampler);
    this.sampler = document.hidden ? undefined : window.setInterval(this.sample, SAMPLE_MS);
  };

  private readonly sample = (): void => {
    const gamepads = this.gamepads;
    if (!gamepads) return;
    try {
      let changed = false;
      for (const event of gamepads.update()) {
        changed = true;
        if (event.type === "added") this.addGamepad(gamepads, event.device);
        else this.remove(`gamepad:${event.device}`);
      }
      for (const event of this.hids?.update() ?? []) {
        changed = true;
        if (event.type === "added") this.addHid(event.device);
        else this.remove(`webhid:${event.device}`);
      }
      if (changed) this.refresh();
      const now = performance.now();
      for (const pad of this.pads.values()) {
        if (pad.source !== "gamepad") continue;
        const timestamp = gamepads.timestamp(pad.id);
        if (timestamp !== undefined) pad.rate.sample(timestamp, now);
      }
    } catch (error) {
      this.fail(`Reading the gamepads failed (${String(error)}).`);
      window.clearInterval(this.sampler);
    }
  };

  private addGamepad(gamepads: Gamepads, id: number): void {
    const info = gamepads.info(id);
    if (info === undefined) return;
    // Shared access: the live view polls it, and the vibration test drives it.
    gamepads.open(id);
    const row = gamepadRow(id, info);
    this.pads.set(row.key, { source: "gamepad", id, info, row, rate: new RateMeter(), rumbleTimer: undefined });
    this.log.event("gamepad_added", {
      pad: row.key,
      identity: info.identity,
      fingerprint: info.fingerprint,
      family: info.family,
      face_layout: info.face_layout,
      buttons: info.layout.buttons.length,
      axes: info.layout.axes.length,
      outputs: info.outputs,
      canonical: info.canonical,
    });
  }

  private addHid(id: number): void {
    const hids = this.hids;
    const info = hids?.info(id);
    if (!hids || info === undefined) return;
    const row = hidRow(id, info);
    const pad: HidPad = {
      source: "webhid",
      id,
      info,
      row,
      rate: new CountMeter(),
      open: false,
      switching: false,
      recording: undefined,
    };
    this.pads.set(row.key, pad);
    this.log.event("hid_added", {
      pad: row.key,
      identity: info.identity,
      fingerprint: info.fingerprint,
      family: info.family,
      transport: info.transport,
      decoded: info.decoded,
      touchpad: info.touchpad,
      collections: info.collections,
    });
    // Opening sends nothing to the device; it starts queueing its reports.
    hids.open(id).then(
      () => {
        pad.open = true;
        this.log.event("hid_opened", { pad: row.key });
        this.refresh();
      },
      (error: unknown) => this.hidNote(`Cannot open ${row.name} (${String(error)}).`),
    );
  }

  private remove(key: string): void {
    const pad = this.pads.get(key);
    if (pad?.source === "gamepad") window.clearTimeout(pad.rumbleTimer);
    this.pads.delete(key);
    this.log.event(key.startsWith("webhid:") ? "hid_removed" : "gamepad_removed", { pad: key });
    this.log.forget(key);
  }

  /**
   * Rebuilds the list. A pad open through WebHID and decoded hides its
   * Gamepad API twin (same VID/PID): one card per pad.
   */
  private refresh(): void {
    const decoded = new Set<string>();
    for (const pad of this.pads.values()) {
      if (pad.source === "webhid" && pad.open && pad.info.decoded) decoded.add(pad.row.ids);
    }
    this.view.pads = [...this.pads.values()]
      .filter((pad) => pad.source === "webhid" || !decoded.has(pad.row.ids))
      .map((pad) => pad.row);
    const selected = this.view.selected;
    if (!this.view.pads.some((row) => row.key === selected)) {
      // A pad just opened through WebHID takes the place of its twin.
      const next = this.view.pads.find((row) => row.source === "webhid" && decoded.has(row.ids)) ?? this.view.pads[0];
      if (next) this.select(next.key);
      else {
        this.view.selected = null;
        this.view.current = null;
      }
    }
  }

  private switchToFull(pad: HidPad): void {
    if (!this.hids || pad.switching) return;
    pad.switching = true;
    this.log.event("hid_switch_to_full", { pad: pad.row.key });
    this.hids
      .switchToFullReports(pad.id)
      .catch((error: unknown) => this.hidNote(`Switching failed (${String(error)}).`))
      .finally(() => {
        pad.switching = false;
      });
  }

  private record(pad: HidPad): void {
    if (pad.recording) return;
    pad.recording = { start: performance.now(), reports: [] };
    this.log.event("hid_recording", { pad: pad.row.key });
  }

  /** Downloads a recording: the device's identity and structure, and its reports. */
  private download(pad: HidPad, recording: Recording): void {
    const { identity, family, transport, collections } = pad.info;
    const content = {
      format: "anygamepad-hid-recording-v1",
      library: version(),
      environment: this.view.environment,
      device: {
        vendor_id: identity.vendor_id,
        product_id: identity.product_id,
        product_name: identity.product_name,
        family,
        transport,
        collections,
      },
      reports: recording.reports,
    };
    saveJson(`anygamepad-${pad.row.ids.replace(":", "-")}-${new Date().toISOString().slice(0, 19)}.json`, content);
  }

  private readonly draw = (): void => {
    try {
      const now = performance.now();
      this.drainHid(now);
      const reads = this.readGamepads();
      this.drawSelected(now, reads);
      this.logRates(now);
    } catch (error) {
      this.fail(`Drawing the gamepads failed (${String(error)}).`);
      return;
    }
    this.frame = requestAnimationFrame(this.draw);
  };

  /** Reads every Gamepad API pad once a frame, for the log and the live view. */
  private readGamepads(): Map<string, GamepadRead> {
    const reads = new Map<string, GamepadRead>();
    const gamepads = this.gamepads;
    if (!gamepads) return reads;
    for (const pad of this.pads.values()) {
      if (pad.source !== "gamepad") continue;
      const snapshot = gamepads.snapshot(pad.id);
      if (snapshot === undefined) continue;
      const state = pad.info.canonical ? gamepads.state(pad.id) : undefined;
      this.log.gamepad(pad.row.key, snapshot, state);
      reads.set(pad.row.key, { snapshot, state });
    }
    return reads;
  }

  /** Notes each pad's update rate in the log, every few seconds. */
  private logRates(now: number): void {
    if (now - this.lastRateLog < RATE_LOG_MS) return;
    this.lastRateLog = now;
    for (const pad of this.pads.values()) {
      this.log.event("rate", { pad: pad.row.key, per_second: pad.rate.rate(now) });
    }
  }

  /** Polls every open WebHID device, so queues drain and rates count. */
  private drainHid(now: number): void {
    const hids = this.hids;
    if (!hids) return;
    for (const pad of this.pads.values()) {
      if (pad.source !== "webhid" || !pad.open) continue;
      const recording = pad.recording;
      let poll;
      try {
        poll = hids.poll(pad.id, recording !== undefined);
      } catch {
        // Unplugged since the last sample: its removal comes with the next
        // one. Not a reason to stop drawing the others.
        pad.open = false;
        continue;
      }
      pad.rate.add(poll.received, now);
      this.log.hid(pad.row.key, poll);
      if (recording) {
        for (const report of poll.reports) {
          recording.reports.push({
            t: Math.round((report.timestamp - recording.start) * 1000) / 1000,
            report_id: report.report_id,
            data: toHex(report.data),
          });
        }
        if (now - recording.start >= RECORD_MS) {
          pad.recording = undefined;
          this.download(pad, recording);
        }
      }
      if (this.live?.key === pad.row.key && this.live.view instanceof HidLiveView) {
        this.live.view.update(poll, {
          rate: pad.rate.rate(now),
          showValues: this.view.showValues,
          switching: pad.switching,
          recording: pad.recording !== undefined,
        });
      }
    }
  }

  /** While no pad is selected: the generic drawing, at rest, in its place. */
  private drawPlaceholder(): void {
    const slot = document.querySelector<HTMLElement>(".placeholder-art");
    if (slot && !slot.firstChild) slot.append(new ControllerDrawing(layoutFor(undefined), undefined).element);
  }

  /** Builds the live view of the selected pad when needed, and updates it. */
  private drawSelected(now: number, reads: Map<string, GamepadRead>): void {
    const key = this.view.selected;
    const pad = key ? this.pads.get(key) : undefined;
    const root = document.querySelector<HTMLElement>(".live");
    if (!pad || !root) {
      this.live = undefined;
      this.drawPlaceholder();
      return;
    }
    // jq79 renders the detail; the live view moves in once it exists, and
    // again if it was rendered anew or another pad was selected.
    if (this.live?.key !== pad.row.key || this.live.root !== root || !root.isConnected) {
      const view =
        pad.source === "gamepad"
          ? new LiveView(root, pad.info)
          : new HidLiveView(root, pad.info, {
              switchToFull: () => this.switchToFull(pad),
              record: () => this.record(pad),
            });
      this.live = { key: pad.row.key, root, view };
      // Which pad the view shows, next to the selection jq79 renders.
      root.dataset.view = pad.row.key;
    }
    const read = reads.get(pad.row.key);
    if (read && this.live.view instanceof LiveView) {
      this.live.view.update(read.snapshot, read.state, pad.rate.rate(now), this.view.showValues);
    }
  }
}
