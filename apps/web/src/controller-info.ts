// "This controller", beside the selected pad: what the browser tells about
// it, and for a PlayStation pad open through WebHID, what the pad tells
// about itself when asked (firmware, board revision, colour, battery
// voltage, usage counters), read with the library's ControllerReader in
// the test mode's module, loaded on request.
//
// The pad is never asked for its serial number or addresses (the library
// does not send those requests), and the usage counters are shown, not
// logged or saved: they are specific enough to recognize a unit.

import type { HidDevices } from "./wasm/anygamepad_wasm.js";
import type { ControllerFacts, ReaderStep } from "./wasm-testing/anygamepad_wasm_testing.js";
import { loadTesting } from "./test-mode";

export interface InfoRow {
  label: string;
  value: string;
}

export interface InfoGroup {
  title: string;
  rows: InfoRow[];
  open: boolean;
  /** Short rows, laid out in two columns. */
  columns: boolean;
}

export interface ReaderView {
  state: "offer" | "reading" | "done" | "failed";
  bluetooth: boolean;
  text: string;
}

export interface PadInfoView {
  identity: InfoRow[];
  reader: ReaderView | null;
  groups: InfoGroup[];
}

export const emptyPadInfo = (): PadInfoView => ({ identity: [], reader: null, groups: [] });

/** A collection's feature report, as `HidDeviceInfo.collections` gives it. */
interface Collection {
  feature_reports?: { report_id: number; items: { report_size: number; report_count: number }[] }[];
  children?: Collection[];
}

/** Data bytes of feature report `id`, as the descriptor declares them. */
export const featureSize = (collections: readonly Collection[], id: number): number | undefined => {
  for (const collection of collections) {
    const report = collection.feature_reports?.find((r) => r.report_id === id);
    if (report) return report.items.reduce((sum, item) => sum + item.report_size * item.report_count, 0) / 8;
    const nested = featureSize(collection.children ?? [], id);
    if (nested !== undefined) return nested;
  }
  return undefined;
};

const sleep = (ms: number): Promise<void> => new Promise((resolve) => window.setTimeout(resolve, ms));

const bytesOf = (view: DataView): Uint8Array => new Uint8Array(view.buffer, view.byteOffset, view.byteLength);

/** Reads what a WebHID PlayStation pad tells about itself. */
export const readController = async (
  hids: HidDevices,
  device: number,
  ids: { vendor: number; product: number; transport: string; collections: readonly Collection[] },
  progress: (done: number) => void,
): Promise<ControllerFacts> => {
  const testing = await loadTesting();
  const reader = new testing.ControllerReader(
    ids.vendor,
    ids.product,
    ids.transport as never,
    featureSize(ids.collections, 0x80),
  );
  try {
    let step: ReaderStep = reader.step(performance.now());
    let commands = 0;
    // A bound on the exchange, whatever the pad answers.
    for (let i = 0; i < 5000 && step.kind !== "done"; i++) {
      switch (step.kind) {
        case "send":
          commands++;
          progress(commands);
          try {
            await hids.sendFeatureReport(device, step.report_id, Uint8Array.from(step.data));
          } catch {
            reader.failed();
          }
          step = reader.step(performance.now());
          break;
        case "read":
          try {
            const answer = (await hids.receiveFeatureReport(device, step.report_id)) as DataView;
            step = reader.answer(bytesOf(answer)) ?? reader.step(performance.now());
          } catch {
            reader.failed();
            step = reader.step(performance.now());
          }
          break;
        case "wait":
          await sleep(step.ms);
          step = reader.step(performance.now());
          break;
      }
    }
    return reader.finish();
  } finally {
    reader.free();
  }
};

const hex = (value: number, digits: number): string => `0x${value.toString(16).toUpperCase().padStart(digits, "0")}`;
const threePart = (v: number): string => `${(v >>> 24) & 0xff}.${(v >>> 16) & 0xff}.${v & 0xffff}`;
const updateVersion = (v: number): string =>
  `${((v >> 8) & 0xff).toString(16).toUpperCase()}.${(v & 0xff).toString(16).toUpperCase().padStart(2, "0")}`;
const dspVersion = (v: number): string =>
  `${((v >>> 16) & 0xffff).toString(16).toUpperCase().padStart(4, "0")}_${(v & 0xffff).toString(16).toUpperCase().padStart(4, "0")}`;

/** `17h 52m 14s`. */
export const duration = (seconds: number): string => {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const parts: string[] = [];
  if (d) parts.push(`${d}d`);
  if (d || h) parts.push(`${h}h`);
  if (d || h || m) parts.push(`${m}m`);
  parts.push(`${s}s`);
  return parts.join(" ");
};

// PlayStation names of the canonical controls.
const BUTTONS: Record<string, string> = {
  dpad_up: "D-pad up",
  dpad_down: "D-pad down",
  dpad_left: "D-pad left",
  dpad_right: "D-pad right",
  north: "Triangle",
  south: "Cross",
  west: "Square",
  east: "Circle",
  left_shoulder: "L1",
  left_trigger: "L2",
  left_stick: "L3",
  right_shoulder: "R1",
  right_trigger: "R2",
  right_stick: "R3",
  touchpad_click: "Touchpad clicks",
  start: "Options",
  select: "Create",
  home: "PS",
  mute: "Mute",
  left_paddle_1: "Left back",
  right_paddle_1: "Right back",
};
const AXES: Record<string, string> = {
  left_stick_x: "Left stick X",
  left_stick_y: "Left stick Y",
  right_stick_x: "Right stick X",
  right_stick_y: "Right stick Y",
  left_trigger: "L2",
  right_trigger: "R2",
};

const USAGE: Record<string, [string, "time" | "count"]> = {
  records: ["Usage log records", "count"],
  active_seconds: ["Time in use", "time"],
  charging_seconds: ["Time charging", "time"],
  haptics_seconds: ["Haptic motors active", "time"],
  left_trigger_seconds: ["Left adaptive trigger active", "time"],
  right_trigger_seconds: ["Right adaptive trigger active", "time"],
};

const CONNECTION: Record<string, string> = {
  usb_connections: "USB connections",
  bluetooth_connections: "Bluetooth connections",
  bluetooth_timeouts: "Bluetooth timeouts",
  usb_sdp: "USB SDP",
  usb_cdp: "USB CDP",
  usb_dcp: "USB DCP",
  usb_type_c_1500: "USB Type-C 1.5 A",
  usb_type_c_3000: "USB Type-C 3 A",
  usb_unknown: "USB unknown",
  charger_connections: "Charger connections",
  charge_cycles: "Charge cycles",
  full_charges: "Full charges",
  headset_detections: "Headsets plugged",
  headphone_detections: "Headphones plugged",
  voltage_errors: "Voltage errors",
  temperature_errors: "Temperature errors",
  pmic_errors: "PMIC errors",
  sub_cpu_errors: "Sub-CPU errors",
  auth_challenges: "Authentications asked",
  auth_successes: "Authentications passed",
  auth_failures: "Authentications failed",
};

/** The groups "This controller" shows for what the pad told. */
export const factGroups = (facts: ControllerFacts): InfoGroup[] => {
  const groups: InfoGroup[] = [];
  const firmware: InfoRow[] = [];
  const f = facts.firmware;
  if (f) {
    firmware.push({ label: "Built", value: f.build });
    firmware.push({ label: "Hardware", value: hex(f.hardware, 8) });
    if (f.kind != null) firmware.push({ label: "Firmware type", value: hex(f.kind, 4) });
    if (f.series != null) firmware.push({ label: "Software series", value: hex(f.series, 4) });
    if (f.update != null) firmware.push({ label: "Update version", value: updateVersion(f.update) });
    firmware.push({ label: "Main firmware", value: f.update != null ? threePart(f.firmware) : hex(f.firmware, 4) });
    if (f.sbl != null) firmware.push({ label: "Bootloader", value: threePart(f.sbl) });
    if (f.dsp != null) firmware.push({ label: "DSP firmware", value: dspVersion(f.dsp) });
    if (f.mcu_dsp != null) firmware.push({ label: "MCU DSP firmware", value: threePart(f.mcu_dsp) });
  }
  if (facts.bluetooth_patch != null) firmware.push({ label: "Bluetooth patch", value: hex(facts.bluetooth_patch, 8) });
  if (firmware.length) groups.push({ title: "Firmware", rows: firmware, open: true, columns: false });

  const hardware: InfoRow[] = [];
  if (facts.board) hardware.push({ label: "Board", value: facts.board });
  if (facts.colour_code) hardware.push({ label: "Colour", value: facts.colour ?? `Unknown (${facts.colour_code})` });
  const [left, right] = facts.trigger_motors;
  if (left || right) hardware.push({ label: "Trigger motors", value: `${left ?? "–"} (L), ${right ?? "–"} (R)` });
  if (facts.touchpad_firmware) hardware.push({ label: "Touchpad firmware", value: `0x${facts.touchpad_firmware}` });
  if (facts.battery_millivolts != null)
    hardware.push({ label: "Battery voltage", value: `${(facts.battery_millivolts / 1000).toFixed(2)} V` });
  if (hardware.length) groups.push({ title: "Hardware", rows: hardware, open: true, columns: false });

  const usage: InfoRow[] = [];
  const connection: InfoRow[] = [];
  const presses: InfoRow[] = [];
  const sticks = new Map<string, { cycles?: number; range?: number }>();
  for (const { name, value } of facts.usage) {
    const [kind, a, b] = name.split(".");
    if (kind && USAGE[kind]) {
      const [label, unit] = USAGE[kind];
      usage.push({ label, value: unit === "time" ? duration(value) : String(value) });
    } else if (kind && CONNECTION[kind]) {
      connection.push({ label: CONNECTION[kind], value: String(value) });
    } else if (kind === "presses" && a) {
      presses.push({ label: BUTTONS[a] ?? a, value: String(value) });
    } else if (kind === "touchpad_touches") {
      presses.push({ label: "Touchpad touches", value: String(value) });
    } else if (kind === "function_presses") {
      presses.push({ label: `Function ${a === "right" ? "right" : "left"}`, value: String(value) });
    } else if (kind === "trigger_strokes" && a && b) {
      presses.push({ label: `${AXES[a] ?? a}, ${b} stroke`, value: String(value) });
    } else if ((kind === "stick_cycles" || kind === "stick_range") && a) {
      const entry = sticks.get(a) ?? {};
      if (kind === "stick_cycles") entry.cycles = value;
      else entry.range = value;
      sticks.set(a, entry);
    }
  }
  if (usage.length) groups.push({ title: "Use", rows: usage, open: true, columns: false });
  if (connection.length) groups.push({ title: "Connections and battery", rows: connection, open: false, columns: true });
  if (presses.length) groups.push({ title: "Presses", rows: presses, open: false, columns: true });
  if (sticks.size) {
    const rows = [...sticks].map(([axis, { cycles, range }]) => ({
      label: AXES[axis] ?? axis,
      value: `${cycles ?? "–"} cycles, range ${range ?? "–"}`,
    }));
    groups.push({ title: "Sticks", rows, open: false, columns: false });
  }
  return groups;
};
