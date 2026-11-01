// The live view of a WebHID device: its report rate and form, and for the
// pads the library decodes (PlayStation) the drawing and the touchpad,
// motion and battery panels; then, folded, the newest raw report, a
// recording button and the report structure. Built once per device; the
// frame loop calls `update`, which only touches what changed (ADR 0005).

import type {
  DecodedInput,
  HidCollection,
  HidDeviceInfo,
  HidPoll,
  MotionSample,
  ReportForm,
} from "./wasm/anygamepad_wasm.js";
import { element, setText } from "./dom";
import { ControllerDrawing } from "./drawing";
import { layoutFor } from "./layouts";

/** What the view's buttons ask of the tester. */
export interface HidActions {
  /** Switches a Bluetooth pad to its full report. */
  switchToFull(): void;
  /** Records the next seconds of reports and downloads them. */
  record(): void;
}

/** What the tester tells the view besides the newest poll. */
export interface HidStatus {
  /** Reports per second. */
  rate: number;
  showValues: boolean;
  switching: boolean;
  recording: boolean;
}

const FORMS: Record<ReportForm, string> = {
  usb: "USB",
  bluetooth_reduced: "Bluetooth, reduced report: no touchpad, motion or battery",
  bluetooth_full: "Bluetooth, full report",
};

const hex = (value: number, digits: number): string => value.toString(16).padStart(digits, "0");

/** Report IDs and sizes of the input reports, per top-level collection. */
const structure = (collections: HidCollection[]): string[] =>
  collections.map((collection) => {
    const reports = collection.input_reports
      .map((report) => {
        const bits = report.items.reduce((sum, item) => sum + item.report_size * item.report_count, 0);
        return `0x${hex(report.report_id, 2)} (${bits / 8} bytes)`;
      })
      .join(", ");
    const usage = `usage 0x${hex(collection.usage_page, 4)}:0x${hex(collection.usage, 4)}`;
    return `${usage} · input reports ${reports || "none"}`;
  });

interface MotionRow {
  bar: HTMLElement;
  value: HTMLElement;
}

export class HidLiveView {
  readonly root: HTMLElement;
  private readonly rate: HTMLElement;
  private readonly form: HTMLElement;
  private readonly switchBox: HTMLElement;
  private readonly switchButton: HTMLButtonElement;
  private readonly drawing: ControllerDrawing | undefined;
  private readonly fingers: SVGCircleElement[] = [];
  private readonly touchText: HTMLElement | undefined;
  private readonly motion: MotionRow[] = [];
  private readonly battery: HTMLElement | undefined;
  private readonly batteryFill: HTMLElement | undefined;
  private readonly raw: HTMLDetailsElement;
  private readonly report: HTMLElement;
  private readonly error: HTMLElement;
  private readonly recordButton: HTMLButtonElement;
  private lastRate = -1;

  constructor(root: HTMLElement, info: HidDeviceInfo, actions: HidActions) {
    this.root = root;
    root.replaceChildren();

    this.rate = element("p", "rate", root, "Waiting for data…");
    this.form = element("p", "report-form", root);

    this.switchBox = element("div", "switch-box", root);
    this.switchBox.hidden = true;
    element(
      "p",
      "",
      this.switchBox,
      "Switching makes the pad send its touchpad, motion and battery until you turn it off " +
        "(hold PS for 10 s). Until then, other programs that read it as a plain gamepad " +
        "(DirectInput games, Chrome's gamepad support on Windows) may get no input from it.",
    );
    this.switchButton = element("button", "", this.switchBox, "Switch to the full report");
    this.switchButton.type = "button";
    this.switchButton.addEventListener("click", () => actions.switchToFull());

    if (info.decoded) {
      this.drawing = new ControllerDrawing(layoutFor(info.family), info.face_layout);
      root.append(this.drawing.element);

      const panels = element("div", "panels", root);
      if (info.touchpad) {
        const panel = element("section", "panel touchpad-panel", panels);
        element("h3", "", panel, "Touchpad");
        const area = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        area.setAttribute("viewBox", `0 0 ${info.touchpad.width} ${info.touchpad.height}`);
        area.setAttribute("class", "touch-area");
        area.setAttribute("role", "img");
        area.setAttribute("aria-label", "Fingers on the touchpad");
        for (let finger = 0; finger < info.touchpad.max_fingers; finger++) {
          const dot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
          dot.setAttribute("r", String(info.touchpad.height / 14));
          dot.setAttribute("visibility", "hidden");
          dot.setAttribute("class", "finger");
          area.append(dot);
          this.fingers.push(dot);
        }
        panel.append(area);
        this.touchText = element("p", "panel-text", panel, "No finger");
      }

      const motion = element("section", "panel motion-panel", panels);
      element("h3", "", motion, "Motion");
      const rows = element("ol", "axes motion", motion);
      for (const name of ["gyro x °/s", "gyro y °/s", "gyro z °/s", "accel x m/s²", "accel y m/s²", "accel z m/s²"]) {
        const row = element("li", "axis", rows);
        element("span", "name", row, name);
        const track = element("span", "track", row);
        const bar = element("span", "bar", track);
        const value = element("span", "value", row, "–");
        this.motion.push({ bar, value });
      }

      const battery = element("section", "panel battery-panel", panels);
      element("h3", "", battery, "Battery");
      this.battery = element("p", "panel-text", battery, "–");
      const meter = element("span", "meter", battery);
      this.batteryFill = element("span", "fill", meter);
    }

    this.raw = element("details", "raw-report", root);
    this.raw.open = !info.decoded;
    element("summary", "", this.raw, "Raw report");
    this.report = element("pre", "report-bytes", this.raw, "No report yet");
    this.error = element("p", "decode-error", this.raw);
    this.recordButton = element("button", "", this.raw, "Record 2 seconds");
    this.recordButton.type = "button";
    this.recordButton.addEventListener("click", () => actions.record());
    element("h3", "", this.raw, "Report structure");
    const list = element("ul", "structure", this.raw);
    for (const line of structure(info.collections)) element("li", "", list, line);
  }

  /** Shows the newest poll. */
  update(poll: HidPoll, status: HidStatus): void {
    if (status.rate !== this.lastRate) {
      this.lastRate = status.rate;
      this.rate.textContent = `${status.rate} reports/s`;
    }

    const input = poll.input;
    setText(this.form, input ? FORMS[input.form] : "");
    this.switchBox.hidden = input?.form !== "bluetooth_reduced";
    this.switchButton.disabled = status.switching;
    setText(this.switchButton, status.switching ? "Switching…" : "Switch to the full report");
    this.recordButton.disabled = status.recording;
    setText(this.recordButton, status.recording ? "Recording…" : "Record 2 seconds");

    if (input && this.drawing) {
      this.drawing.update(input.state, status.showValues);
      this.panels(input, poll.motion);
    }

    setText(this.error, poll.error ?? "");
    if (this.raw.open && poll.last) {
      const { report_id, data } = poll.last;
      const bytes = Array.from(data, (byte) => hex(byte, 2)).join(" ");
      setText(this.report, `0x${hex(report_id, 2)} · ${data.length} bytes\n${bytes}`);
    }
  }

  private panels(input: DecodedInput, sample: MotionSample | undefined): void {
    const touches = input.state.touches;
    if (this.touchText) {
      const area = this.fingers[0]?.ownerSVGElement?.viewBox.baseVal;
      const down: string[] = [];
      this.fingers.forEach((dot, slot) => {
        const point = touches[slot];
        dot.setAttribute("visibility", point ? "visible" : "hidden");
        if (!point || !area) return;
        dot.setAttribute("cx", (point.x * area.width).toFixed(0));
        dot.setAttribute("cy", (point.y * area.height).toFixed(0));
        down.push(`#${point.id} ${point.x.toFixed(2)}, ${point.y.toFixed(2)}`);
      });
      setText(this.touchText, input.imu ? down.join(" · ") || "No finger" : "Needs the full report");
    }

    // In SI units from the library, shown in °/s and m/s²; bars span
    // ±2000 °/s and ±4 g, the pads' ranges.
    const values: number[] = [];
    if (input.imu && sample?.gyro && sample.accel) {
      const { gyro, accel } = sample;
      values.push(...[gyro.x, gyro.y, gyro.z].map((rad) => (rad * 180) / Math.PI), accel.x, accel.y, accel.z);
    }
    this.motion.forEach(({ bar, value }, index) => {
      const reading = values[index];
      const range = index < 3 ? 2000 : 4 * 9.80665;
      bar.style.setProperty("--value", reading === undefined ? "0" : (reading / range).toFixed(3));
      setText(value, reading === undefined ? "–" : reading.toFixed(index < 3 ? 0 : 2));
    });

    const power = input.state.power;
    if (this.battery && this.batteryFill) {
      const states: Record<string, string> = {
        on_battery: "on battery",
        charging: "charging",
        charged: "charged",
        no_battery: "no battery",
        unknown: "",
      };
      const text =
        power.percent === undefined
          ? input.imu ? "Not reported" : "Needs the full report"
          : [`${power.percent}%`, states[power.state]].filter(Boolean).join(" · ");
      setText(this.battery, text);
      this.batteryFill.style.setProperty("--value", String((power.percent ?? 0) / 100));
    }
  }
}
