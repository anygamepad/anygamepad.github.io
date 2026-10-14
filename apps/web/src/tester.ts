// The tester: reads the browser's gamepads through anygamepad's web provider
// (WebAssembly), keeps the list the page renders, and drives the live views.
// Logic lives here, in TypeScript checked by `tsc`; components only render
// (ADR 0005).
//
// Two loops: a sampler every few milliseconds reads the Gamepad API (hotplug
// and how often each gamepad's data changes, which a once-per-frame read
// would cap at the display's refresh rate), and the frame loop draws.

import { Gamepads, type DeviceInfo, type Environment, type OutputCommand } from "./wasm/anygamepad_wasm.js";
import { LiveView } from "./live";
import { RateMeter } from "./rate";

/** One connected gamepad, as the page lists it. */
export interface DeviceRow {
  id: number;
  name: string;
  /** `vvvv:pppp`, or a dash where the browser hides the IDs. */
  ids: string;
  /** `Gamepad.id`, verbatim. */
  gamepadId: string;
  /** `Gamepad.mapping`, explained. */
  mapping: string;
  buttons: number;
  axes: number;
  rumble: boolean;
  triggerRumble: boolean;
}

/** The data the page renders. */
export interface TesterView {
  environment: Environment | null;
  devices: DeviceRow[];
  error: string | null;
  /** Why a gamepad may not show in this browser, while none does. */
  hint: string | null;
}

export const emptyView = (): TesterView => ({ environment: null, devices: [], error: null, hint: null });

/**
 * A known reason for a gamepad not to show in this browser, if any.
 *
 * macOS switches the DualSense to its extended report, which Chromium's HID
 * reader cannot decode, so Chromium never sees an input and never exposes it
 * (docs/research/06-browser-gamepad-api.md).
 */
export const missingGamepadHint = (environment: Environment): string | null =>
  environment.browser.engine === "blink" && environment.os === "macos"
    ? "A DualSense does not show in Chrome, Edge or other Chromium browsers on macOS: " +
      "macOS switches it to a report mode they cannot read. Try Safari, which reads it through macOS."
    : null;

/** Which motors a vibration test runs. */
export type RumbleKind = "strong" | "weak" | "both" | "triggers";

/** How long a vibration test runs, in milliseconds. */
export const RUMBLE_MS = 1000;

/** The sampler's period (browsers clamp timers to about 4 ms). */
const SAMPLE_MS = 4;

const hex = (value: number | undefined): string =>
  value === undefined ? "----" : value.toString(16).padStart(4, "0");

export const deviceRow = (id: number, info: DeviceInfo): DeviceRow => ({
  id,
  name: info.identity.product_name ?? info.identity.web?.id ?? "Unknown gamepad",
  ids:
    info.identity.vendor_id === undefined
      ? "–"
      : `${hex(info.identity.vendor_id)}:${hex(info.identity.product_id)}`,
  gamepadId: info.identity.web?.id ?? "",
  mapping: info.identity.web?.mapping
    ? `"${info.identity.web.mapping}" mapping`
    : "no mapping (the browser's own layout)",
  buttons: info.layout.buttons.length,
  axes: info.layout.axes.length,
  rumble: info.outputs.includes("rumble"),
  triggerRumble: info.outputs.includes("trigger_rumble"),
});

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

interface Device {
  info: DeviceInfo;
  rate: RateMeter;
  live: LiveView | undefined;
  rumbleTimer: number | undefined;
}

export class Tester {
  private gamepads: Gamepads | undefined;
  private view: TesterView = emptyView();
  private readonly devices = new Map<number, Device>();
  private sampler: number | undefined;
  private frame = 0;

  /** Starts reading gamepads into `view` (a component's reactive data). */
  start(view: TesterView): void {
    this.view = view;
    try {
      this.gamepads = new Gamepads();
    } catch (error) {
      view.error = `This browser cannot read gamepads (${String(error)}).`;
      return;
    }
    view.environment = this.gamepads.environment();
    view.hint = missingGamepadHint(view.environment);
    document.addEventListener("visibilitychange", this.onVisibility);
    this.onVisibility();
    this.frame = requestAnimationFrame(this.draw);
  }

  /** Runs a short vibration test on a gamepad. */
  readonly rumble = (id: number, kind: RumbleKind): void => {
    const device = this.devices.get(id);
    if (!this.gamepads || !device) return;
    try {
      this.gamepads.send(id, rumbleCommand(kind, 1));
    } catch (error) {
      this.view.error = `Vibration failed (${String(error)}).`;
      return;
    }
    // One timer stops every motor, including those of a test it cut short.
    window.clearTimeout(device.rumbleTimer);
    device.rumbleTimer = window.setTimeout(() => {
      device.rumbleTimer = undefined;
      try {
        this.gamepads?.send(id, rumbleCommand("both", 0));
        if (device.info.outputs.includes("trigger_rumble")) {
          this.gamepads?.send(id, rumbleCommand("triggers", 0));
        }
      } catch {
        // The gamepad went away; its rumble stopped with it.
      }
    }, RUMBLE_MS);
  };

  /** Stops both loops and releases the gamepads. */
  stop(): void {
    document.removeEventListener("visibilitychange", this.onVisibility);
    window.clearInterval(this.sampler);
    cancelAnimationFrame(this.frame);
    for (const device of this.devices.values()) window.clearTimeout(device.rumbleTimer);
    this.gamepads?.free();
    this.gamepads = undefined;
  }

  // A hidden page needs neither loop (the frame loop pauses by itself).
  private readonly onVisibility = (): void => {
    window.clearInterval(this.sampler);
    this.sampler = document.hidden ? undefined : window.setInterval(this.sample, SAMPLE_MS);
  };

  private readonly sample = (): void => {
    const gamepads = this.gamepads;
    if (!gamepads) return;
    try {
      const events = gamepads.update();
      if (events.length > 0) {
        for (const event of events) {
          if (event.type === "added") this.added(gamepads, event.device);
          else this.removed(event.device);
        }
        this.view.devices = [...this.devices].map(([id, device]) => deviceRow(id, device.info));
      }
      const now = performance.now();
      for (const [id, device] of this.devices) {
        const timestamp = gamepads.timestamp(id);
        if (timestamp !== undefined) device.rate.sample(timestamp, now);
      }
    } catch (error) {
      this.view.error = `Reading the gamepads failed (${String(error)}).`;
      window.clearInterval(this.sampler);
    }
  };

  private added(gamepads: Gamepads, id: number): void {
    const info = gamepads.info(id);
    if (info === undefined) return;
    // Shared access: the live view polls it, and the vibration test drives it.
    gamepads.open(id);
    this.devices.set(id, { info, rate: new RateMeter(), live: undefined, rumbleTimer: undefined });
  }

  private removed(id: number): void {
    window.clearTimeout(this.devices.get(id)?.rumbleTimer);
    this.devices.delete(id);
  }

  private readonly draw = (): void => {
    const gamepads = this.gamepads;
    if (!gamepads) return;
    try {
      const now = performance.now();
      for (const [id, device] of this.devices) {
        // jq79 renders the card; the live view moves in once it exists, and
        // again if the card was rendered anew.
        if (!device.live?.root.isConnected) {
          const root = document.querySelector<HTMLElement>(`.live[data-device="${id}"]`);
          if (!root) continue;
          device.live = new LiveView(root, device.info);
        }
        const snapshot = gamepads.snapshot(id);
        if (snapshot === undefined) continue;
        const state = device.info.canonical ? gamepads.state(id) : undefined;
        device.live.update(snapshot, state, device.rate.rate(now));
      }
    } catch (error) {
      this.view.error = `Drawing the gamepads failed (${String(error)}).`;
      return;
    }
    this.frame = requestAnimationFrame(this.draw);
  };
}
