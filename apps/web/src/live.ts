// The live part of a device card: the update rate, the controller drawing
// (standard layout only), and every raw button and axis as the browser
// reports it. Built once per device; the frame loop calls `update`, which
// only touches what changed (ADR 0005: no reactivity per frame).

import type { DeviceInfo, GamepadSnapshot, GamepadState } from "./wasm/anygamepad_wasm.js";
import { ControllerDiagram } from "./diagram";

const element = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  parent: Element,
  text?: string,
): HTMLElementTagNameMap[K] => {
  const created = document.createElement(tag);
  created.className = className;
  if (text !== undefined) created.textContent = text;
  parent.append(created);
  return created;
};

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

/** A raw axis value with its sign, so the column does not jump. */
const axisText = (value: number): string => `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(3)}`;

export class LiveView {
  readonly root: HTMLElement;
  private readonly rate: HTMLElement;
  private readonly diagram: ControllerDiagram | undefined;
  private readonly buttons: ButtonCell[] = [];
  private readonly axes: AxisRow[] = [];
  private lastRate = -1;

  constructor(root: HTMLElement, info: DeviceInfo) {
    this.root = root;
    root.replaceChildren();

    this.rate = element("p", "rate", root, "Waiting for data…");

    if (info.canonical) {
      this.diagram = new ControllerDiagram();
      root.append(this.diagram.element);
    }

    const buttons = element("section", "raw raw-buttons", root);
    element("h3", "", buttons, `Buttons (${info.layout.buttons.length})`);
    const buttonList = element("ol", "buttons", buttons);
    info.layout.buttons.forEach((_, index) => {
      const cell = element("li", "button", buttonList);
      const name = info.canonical?.buttons[index] ?? "";
      element("span", "index", cell, String(index));
      // Names can be wider than the cell; the tooltip shows them whole.
      element("span", "name", cell, name).title = name;
      const meter = element("span", "meter", cell);
      const fill = element("span", "fill", meter);
      const value = element("span", "value", cell, "0.00");
      this.buttons.push({ cell, fill, value, last: "" });
    });

    const axes = element("section", "raw raw-axes", root);
    element("h3", "", axes, `Axes (${info.layout.axes.length})`);
    const axisList = element("ol", "axes", axes);
    info.layout.axes.forEach((_, index) => {
      const row = element("li", "axis", axisList);
      element("span", "index", row, String(index));
      element("span", "name", row, info.canonical?.axes[index] ?? "");
      const track = element("span", "track", row);
      const bar = element("span", "bar", track);
      const value = element("span", "value", row, axisText(0));
      this.axes.push({ bar, value, last: Number.NaN });
    });
  }

  /** Shows the latest values; `rate` is the update rate in Hz. */
  update(snapshot: GamepadSnapshot, state: GamepadState | undefined, rate: number): void {
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

    if (state && this.diagram) this.diagram.update(state);
  }
}
