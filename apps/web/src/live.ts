// The live view of a Gamepad API pad: the update rate, the controller
// drawing (standard layout, or the mapping the guided wizard identified;
// shaped by the pad's family), the normalized values from the mapping engine
// and active profile, and every raw button and axis as the browser
// reports it, folded under the drawing.
// Built once per device; the frame loop calls `update`, which only touches
// what changed (ADR 0005: no reactivity per frame).

import type { Axis, Button, DeviceInfo, GamepadSnapshot, GamepadState } from "./wasm/anygamepad_wasm.js";
import { element } from "./dom";
import { ControllerDrawing } from "./drawing";
import { layoutFor } from "./layouts";

interface ButtonCell {
  cell: HTMLElement;
  fill: HTMLElement;
  value: HTMLElement;
  last: string;
}

interface AxisRow {
  bar: HTMLElement;
  value: HTMLElement;
  last: number;
}

interface CanonicalButtonCell {
  cell: HTMLElement;
  fill: HTMLElement;
  value: HTMLElement;
  button: Button;
  last: string;
}

interface CanonicalAxisRow {
  bar: HTMLElement;
  value: HTMLElement;
  axis: Axis;
  last: number;
}

/** A raw axis value with its sign, so the column does not jump. */
const axisText = (value: number): string => `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(3)}`;

export class LiveView {
  readonly root: HTMLElement;
  private readonly rate: HTMLElement;
  private readonly drawing: ControllerDrawing | undefined;
  private readonly buttons: ButtonCell[] = [];
  private readonly axes: AxisRow[] = [];
  private readonly canonicalButtons: CanonicalButtonCell[] = [];
  private readonly canonicalAxes: CanonicalAxisRow[] = [];
  private lastRate = -1;

  /**
   * `identified` names the raw elements a finished guided wizard mapped
   * (see `rawNames`); the drawing then shows the wizard's mapping.
   */
  constructor(root: HTMLElement, info: DeviceInfo, identified?: { buttons: string[]; axes: string[] }) {
    this.root = root;
    root.replaceChildren();

    this.rate = element("p", "rate", root, "Waiting for data…");

    const names = identified ?? info.canonical;
    if (names) {
      // Buttons the pad reports, when the browser's standard mapping says.
      const buttons = identified ? undefined : info.capabilities?.buttons;
      this.drawing = new ControllerDrawing(layoutFor(info.family), info.face_layout, buttons);
      root.append(this.drawing.element);
    }

    // Normalized view: canonical output from mapping engine and active profile.
    if (info.canonical || info.capabilities || info.profile_name) {
      const normalized = element("details", "normalized-values", root);
      normalized.open = true;
      const summary = element("summary", "", normalized);
      element("span", "normalized-title", summary, "Normalized values");
      if (info.profile_name) {
        element("span", "profile-badge", summary, info.profile_name);
      }

      const capButtons: Button[] =
        info.capabilities?.buttons ??
        ((info.canonical?.buttons.filter((b): b is Button => b !== null && b !== undefined) as Button[]) ?? []);
      if (capButtons.length > 0) {
        const normButtons = element("section", "normalized normalized-buttons", normalized);
        element("h3", "", normButtons, `Buttons (${capButtons.length})`);
        const normButtonList = element("ol", "buttons", normButtons);
        capButtons.forEach((button) => {
          const cell = element("li", "button canonical-button", normButtonList);
          cell.dataset.control = button;

          const profileIdx = info.canonical ? info.canonical.buttons.indexOf(button) : -1;
          const wizardIdx = identified ? identified.buttons.findIndex((n) => n.split(" ").includes(button)) : -1;
          const mismatch = identified !== undefined && profileIdx !== wizardIdx;
          if (mismatch) {
            cell.classList.add("disagreement");
            const badge = element("span", "disagreement-badge", cell, "⚠️");
            badge.title = `Mismatch: wizard confirmed ${wizardIdx >= 0 ? "b" + wizardIdx : "(missing)"}, profile has ${profileIdx >= 0 ? "b" + profileIdx : "(unmapped)"}`;
          }

          element("span", "name", cell, button).title = button;
          const meter = element("span", "meter", cell);
          const fill = element("span", "fill", meter);
          const value = element("span", "value", cell, "0.00");
          this.canonicalButtons.push({ cell, fill, value, button, last: "" });
        });
      }

      const capAxes: Axis[] =
        info.capabilities?.axes ??
        ((info.canonical?.axes.filter((a): a is Axis => a !== null && a !== undefined) as Axis[]) ?? []);
      if (capAxes.length > 0) {
        const normAxes = element("section", "normalized normalized-axes", normalized);
        element("h3", "", normAxes, `Axes (${capAxes.length})`);
        const normAxisList = element("ol", "axes", normAxes);
        capAxes.forEach((axis) => {
          const row = element("li", "axis canonical-axis", normAxisList);
          row.dataset.control = axis;

          const profileAxisIdx = info.canonical ? info.canonical.axes.indexOf(axis) : -1;
          const wizardAxisIdx = identified ? identified.axes.findIndex((n) => n.split(" ").includes(axis)) : -1;
          const mismatch = identified !== undefined && profileAxisIdx !== wizardAxisIdx;
          if (mismatch) {
            row.classList.add("disagreement");
            const badge = element("span", "disagreement-badge", row, "⚠️");
            badge.title = `Mismatch: wizard confirmed ${wizardAxisIdx >= 0 ? "a" + wizardAxisIdx : "(missing)"}, profile has ${profileAxisIdx >= 0 ? "a" + profileAxisIdx : "(unmapped)"}`;
          }

          element("span", "name", row, axis);
          const track = element("span", "track", row);
          const bar = element("span", "bar", track);
          const value = element("span", "value", row, axisText(0));
          this.canonicalAxes.push({ bar, value, axis, last: Number.NaN });
        });
      }
    }

    // Folded under a drawing; open when there is none to show.
    const raw = element("details", "raw-values", root);
    raw.open = !names;
    element("summary", "", raw, "Raw values");
    const buttons = element("section", "raw raw-buttons", raw);
    element("h3", "", buttons, `Buttons (${info.layout.buttons.length})`);
    const buttonList = element("ol", "buttons", buttons);
    info.layout.buttons.forEach((_, index) => {
      const cell = element("li", "button", buttonList);
      const name = names?.buttons[index] ?? "";

      const profileName = info.canonical?.buttons[index] ?? "";
      const wizardName = identified?.buttons[index] ?? "";
      const mismatch =
        identified !== undefined && profileName !== wizardName && (profileName !== "" || wizardName !== "");
      if (mismatch) {
        cell.classList.add("disagreement");
        const badge = element("span", "disagreement-badge", cell, "⚠️");
        badge.title = `Mismatch: wizard confirmed '${wizardName || "(none)"}', profile has '${profileName || "(none)"}'`;
      }

      element("span", "index", cell, String(index));
      // Names can be wider than the cell; the tooltip shows them whole.
      element("span", "name", cell, name).title = name;
      const meter = element("span", "meter", cell);
      const fill = element("span", "fill", meter);
      const value = element("span", "value", cell, "0.00");
      this.buttons.push({ cell, fill, value, last: "" });
    });

    const axes = element("section", "raw raw-axes", raw);
    element("h3", "", axes, `Axes (${info.layout.axes.length})`);
    const axisList = element("ol", "axes", axes);
    info.layout.axes.forEach((_, index) => {
      const row = element("li", "axis", axisList);
      const name = names?.axes[index] ?? "";

      const profileAxis = info.canonical?.axes[index] ?? "";
      const wizardAxis = identified?.axes[index] ?? "";
      const mismatch =
        identified !== undefined && profileAxis !== wizardAxis && (profileAxis !== "" || wizardAxis !== "");
      if (mismatch) {
        row.classList.add("disagreement");
        const badge = element("span", "disagreement-badge", row, "⚠️");
        badge.title = `Mismatch: wizard confirmed '${wizardAxis || "(none)"}', profile has '${profileAxis || "(none)"}'`;
      }

      element("span", "index", row, String(index));
      element("span", "name", row, name);
      const track = element("span", "track", row);
      const bar = element("span", "bar", track);
      const value = element("span", "value", row, axisText(0));
      this.axes.push({ bar, value, last: Number.NaN });
    });
  }

  /**
   * Shows the latest values; `rate` is the update rate in Hz, `showValues`
   * whether the drawing shows numbers.
   */
  update(snapshot: GamepadSnapshot, state: GamepadState | undefined, rate: number, showValues: boolean): void {
    if (rate !== this.lastRate) {
      this.lastRate = rate;
      this.rate.textContent = `${rate} updates/s`;
    }

    snapshot.buttons.forEach((button, index) => {
      const cell = this.buttons[index];
      if (!cell) return;
      const key = `${button.pressed} ${button.touched} ${button.value}`;
      if (key === cell.last) return;
      cell.last = key;
      cell.cell.classList.toggle("pressed", button.pressed);
      cell.cell.classList.toggle("touched", button.touched && !button.pressed);
      cell.fill.style.setProperty("--value", String(button.value));
      cell.value.textContent = button.value.toFixed(2);
    });

    snapshot.axes.forEach((value, index) => {
      const row = this.axes[index];
      if (!row || value === row.last) return;
      row.last = value;
      // The bar spans the right half and scales from the centre, so a
      // negative value mirrors it into the left half (style.css).
      row.bar.style.setProperty("--value", String(value));
      row.value.textContent = axisText(value);
    });

    if (state) {
      if (this.drawing) this.drawing.update(state, showValues);

      const pressedSet = new Set(state.buttons);
      for (const btn of this.canonicalButtons) {
        const isPressed = pressedSet.has(btn.button);
        const axisVal = (state.axes as Record<string, number>)[btn.button];
        const val = axisVal !== undefined ? axisVal : isPressed ? 1.0 : 0.0;
        const key = `${isPressed} ${val}`;
        if (key !== btn.last) {
          btn.last = key;
          btn.cell.classList.toggle("pressed", isPressed);
          btn.fill.style.setProperty("--value", String(val));
          btn.value.textContent = val.toFixed(2);
        }
      }

      for (const ax of this.canonicalAxes) {
        const val = (state.axes as Record<string, number>)[ax.axis] ?? 0;
        if (val !== ax.last) {
          ax.last = val;
          ax.bar.style.setProperty("--value", String(val));
          ax.value.textContent = axisText(val);
        }
      }
    }
  }
}
