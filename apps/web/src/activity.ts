// The activity log: what the tester detects, kept in the tab from page load
// and downloaded as JSON on request, so a session can be shared for
// analysis. It holds the environment, pads coming and going with their
// identity as the browser exposes it, raw input changes, the canonical
// buttons the library derives, WebHID report forms, one sample of each
// report, decode errors, the viewer's actions and page errors.
//
// No serial numbers, Bluetooth addresses or other personal data: the Gamepad
// API and WebHID expose none, and the tester reads no feature report but the
// calibration one, which is not logged. Times are milliseconds since the
// page loaded.

import type {
  Axis,
  DecodedInput,
  Environment,
  GamepadSnapshot,
  GamepadState,
  HidPoll,
} from "./wasm/anygamepad_wasm.js";

export const LOG_FORMAT = "anygamepad-activity-log-v1";

/** Input events kept, the newest; device events are always kept. */
export const MAX_INPUT_EVENTS = 20_000;

/** Smallest change of an analog value that is logged. */
const STEP = 0.05;

export interface LogEvent {
  /** Milliseconds since the page loaded. */
  t: number;
  type: string;
  /** The pad, as the page keys it (`gamepad:<id>`, `webhid:<id>`). */
  pad?: string;
  [field: string]: unknown;
}

const round = (value: number, digits: number): number => {
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
};

/** Whether an analog value moved enough since `last` to log it. */
const moved = (value: number, last: number): boolean =>
  Math.abs(value - last) >= STEP || (value !== last && (value === 0 || Math.abs(value) === 1));

const hexOf = (data: Uint8Array): string => Array.from(data, (byte) => byte.toString(16).padStart(2, "0")).join("");

/** The last logged values of a Gamepad API pad. */
interface GamepadTrack {
  buttons: { pressed: boolean; value: number }[];
  axes: number[];
  pressed: string;
}

/** The last logged state of a WebHID pad. */
interface HidTrack {
  /** `<report id>:<length>` of the reports seen. */
  reports: Set<string>;
  form: string | undefined;
  pressed: string;
  axes: Partial<Record<Axis, number>>;
  touches: string;
  power: string;
  /** Times each decode error was seen, by message. */
  errors: Map<string, number>;
}

export class ActivityLog {
  private readonly startedAt = new Date().toISOString();
  private readonly events: LogEvent[] = [];
  private readonly inputs: LogEvent[] = [];
  private dropped = 0;
  private readonly gamepads = new Map<string, GamepadTrack>();
  private readonly hids = new Map<string, HidTrack>();

  /** A device or session event: always kept. */
  event(type: string, fields: Record<string, unknown> = {}): void {
    this.events.push({ t: this.now(), type, ...fields });
  }

  /** An input event: the oldest go once there are too many. */
  private input(type: string, fields: Record<string, unknown>): void {
    this.inputs.push({ t: this.now(), type, ...fields });
    if (this.inputs.length > MAX_INPUT_EVENTS) {
      const excess = this.inputs.length - MAX_INPUT_EVENTS * 0.9;
      this.inputs.splice(0, excess);
      this.dropped += excess;
    }
  }

  /** Logs what changed in a Gamepad API pad since the last call. */
  gamepad(pad: string, snapshot: GamepadSnapshot, state: GamepadState | undefined): void {
    let track = this.gamepads.get(pad);
    if (!track) {
      track = { buttons: [], axes: [], pressed: "" };
      this.gamepads.set(pad, track);
    }
    snapshot.buttons.forEach((button, index) => {
      const last = track.buttons[index] ?? { pressed: false, value: 0 };
      if (button.pressed !== last.pressed || moved(button.value, last.value)) {
        track.buttons[index] = { pressed: button.pressed, value: button.value };
        this.input("button", { pad, index, pressed: button.pressed, value: round(button.value, 3) });
      }
    });
    snapshot.axes.forEach((value, index) => {
      if (moved(value, track.axes[index] ?? 0)) {
        track.axes[index] = value;
        this.input("axis", { pad, index, value: round(value, 3) });
      }
    });
    if (state) this.canonicalButtons(pad, track, state);
  }

  /** Logs what changed in a WebHID pad's reports since the last call. */
  hid(pad: string, poll: HidPoll): void {
    let track = this.hids.get(pad);
    if (!track) {
      track = { reports: new Set(), form: undefined, pressed: "", axes: {}, touches: "", power: "", errors: new Map() };
      this.hids.set(pad, track);
    }
    const report = poll.last;
    if (report) {
      const kind = `${report.report_id}:${report.data.length}`;
      if (!track.reports.has(kind)) {
        track.reports.add(kind);
        this.event("hid_report_sample", {
          pad,
          report_id: report.report_id,
          length: report.data.length,
          data: hexOf(report.data),
        });
      }
    }
    if (poll.error !== undefined) {
      const seen = track.errors.get(poll.error) ?? 0;
      track.errors.set(poll.error, seen + 1);
      if (seen === 0) this.event("hid_decode_error", { pad, message: poll.error });
    }
    if (poll.input) this.decoded(pad, track, poll.input);
  }

  private decoded(pad: string, track: HidTrack, input: DecodedInput): void {
    if (input.form !== track.form) {
      track.form = input.form;
      this.event("hid_report_form", { pad, form: input.form, transport: input.transport });
    }
    const { state } = input;
    this.canonicalButtons(pad, track, state);
    for (const [axis, value] of Object.entries(state.axes) as [Axis, number][]) {
      if (moved(value, track.axes[axis] ?? 0)) {
        track.axes[axis] = value;
        this.input("canonical_axis", { pad, axis, value: round(value, 3) });
      }
    }
    const touches = state.touches
      .filter((point) => point !== undefined)
      .map(({ id, x, y }) => ({ id, x: round(x, 2), y: round(y, 2) }));
    const touchKey = JSON.stringify(touches);
    if (touchKey !== track.touches) {
      track.touches = touchKey;
      this.input("touches", { pad, touches });
    }
    const power = `${state.power.state}:${state.power.percent ?? ""}`;
    if (power !== track.power) {
      track.power = power;
      this.event("power", { pad, state: state.power.state, percent: state.power.percent });
    }
  }

  private canonicalButtons(pad: string, track: { pressed: string }, state: GamepadState): void {
    const pressed = state.buttons.join(" ");
    if (pressed !== track.pressed) {
      track.pressed = pressed;
      this.input("canonical_buttons", { pad, pressed: [...state.buttons] });
    }
  }

  /** Forgets a pad's last values, once it is gone. */
  forget(pad: string): void {
    this.gamepads.delete(pad);
    this.flushErrors(pad);
    this.hids.delete(pad);
  }

  /** The log as a JSON document: device and input events in time order. */
  toJSON(library: string, environment: Environment | null): object {
    for (const pad of this.hids.keys()) this.flushErrors(pad);
    const events: LogEvent[] = [];
    let i = 0;
    let j = 0;
    while (i < this.events.length || j < this.inputs.length) {
      const a = this.events[i];
      const b = this.inputs[j];
      if (b === undefined || (a !== undefined && a.t <= b.t)) {
        events.push(a as LogEvent);
        i++;
      } else {
        events.push(b);
        j++;
      }
    }
    return {
      format: LOG_FORMAT,
      library,
      environment,
      started_at: this.startedAt,
      duration_ms: this.now(),
      dropped_input_events: this.dropped,
      events,
    };
  }

  /** Logs how often each decode error repeated after its first time. */
  private flushErrors(pad: string): void {
    const track = this.hids.get(pad);
    if (!track) return;
    for (const [message, count] of track.errors) {
      if (count > 1) this.event("hid_decode_error_repeats", { pad, message, count: count - 1 });
      track.errors.set(message, 1);
    }
  }

  private now(): number {
    return round(performance.now(), 1);
  }
}
