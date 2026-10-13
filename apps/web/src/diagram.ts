// A controller drawing that shows the canonical state (ADR 0001 positions):
// pressed buttons, trigger travel and stick positions. Built once per device
// and updated by the frame loop, outside jq79's reactivity (ADR 0005).

import type { Axis, Button, GamepadState } from "./wasm/anygamepad_wasm.js";

const SVG_NS = "http://www.w3.org/2000/svg";

const svg = <K extends keyof SVGElementTagNameMap>(
  tag: K,
  attributes: Record<string, string | number>,
  parent: Element,
  title?: string,
): SVGElementTagNameMap[K] => {
  const element = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value));
  if (title) {
    const label = document.createElementNS(SVG_NS, "title");
    label.textContent = title;
    element.append(label);
  }
  parent.append(element);
  return element;
};

/** Travel of a stick dot, in drawing units. */
const STICK_TRAVEL = 15;
/** Width of a trigger's bar at full press. */
const TRIGGER_WIDTH = 50;

interface Stick {
  ring: SVGCircleElement;
  dot: SVGCircleElement;
  x: number;
  y: number;
  axes: [Axis, Axis];
}

export class ControllerDiagram {
  readonly element: SVGSVGElement;
  private readonly buttons = new Map<Button, SVGElement>();
  private readonly triggers: [SVGRectElement, Axis][] = [];
  private readonly sticks: Stick[] = [];

  constructor() {
    const root = document.createElementNS(SVG_NS, "svg");
    root.setAttribute("viewBox", "0 0 320 200");
    root.setAttribute("class", "diagram");
    root.setAttribute("role", "img");
    root.setAttribute("aria-label", "Controller, canonical positions");
    this.element = root;

    svg("path", {
      class: "body",
      d: "M70 50 H250 C285 50 305 80 312 125 C318 165 300 190 278 190 C258 190 248 172 236 156 H84 C72 172 62 190 42 190 C20 190 2 165 8 125 C15 80 35 50 70 50 Z",
    }, root);

    for (const [x, axis] of [[60, "left_trigger"], [210, "right_trigger"]] as const) {
      svg("rect", { class: "track", x, y: 10, width: TRIGGER_WIDTH, height: 14, rx: 4 }, root);
      const fill = svg("rect", { class: "fill", x, y: 10, width: 0, height: 14, rx: 4 }, root, axis);
      this.triggers.push([fill, axis]);
    }
    this.button("left_shoulder", svg("rect", { x: 55, y: 32, width: 60, height: 12, rx: 6 }, root, "left_shoulder"));
    this.button("right_shoulder", svg("rect", { x: 205, y: 32, width: 60, height: 12, rx: 6 }, root, "right_shoulder"));

    this.stick(root, 100, 100, "left_stick", ["left_stick_x", "left_stick_y"]);
    this.stick(root, 190, 148, "right_stick", ["right_stick_x", "right_stick_y"]);

    const dpad: [Button, number, number][] = [
      ["dpad_up", 118, 127],
      ["dpad_down", 118, 155],
      ["dpad_left", 104, 141],
      ["dpad_right", 132, 141],
    ];
    for (const [button, x, y] of dpad) {
      this.button(button, svg("rect", { x, y, width: 14, height: 14, rx: 2 }, root, button));
    }

    const face: [Button, number, number][] = [
      ["north", 228, 75],
      ["south", 228, 113],
      ["west", 209, 94],
      ["east", 247, 94],
    ];
    for (const [button, cx, cy] of face) {
      this.button(button, svg("circle", { cx, cy, r: 9 }, root, button));
    }

    this.button("select", svg("rect", { x: 135, y: 82, width: 16, height: 9, rx: 4 }, root, "select"));
    this.button("start", svg("rect", { x: 169, y: 82, width: 16, height: 9, rx: 4 }, root, "start"));
    this.button("home", svg("circle", { cx: 160, cy: 108, r: 8 }, root, "home"));
  }

  /** Shows `state`. */
  update(state: GamepadState): void {
    const pressed = new Set(state.buttons);
    for (const [button, element] of this.buttons) {
      element.classList.toggle("on", pressed.has(button));
    }
    for (const [fill, axis] of this.triggers) {
      fill.setAttribute("width", (state.axes[axis] * TRIGGER_WIDTH).toFixed(1));
    }
    for (const { dot, x, y, axes } of this.sticks) {
      // Canonical Y grows upwards, SVG Y downwards.
      dot.setAttribute("cx", (x + state.axes[axes[0]] * STICK_TRAVEL).toFixed(1));
      dot.setAttribute("cy", (y - state.axes[axes[1]] * STICK_TRAVEL).toFixed(1));
    }
  }

  private button(button: Button, element: SVGElement): void {
    element.classList.add("control");
    this.buttons.set(button, element);
  }

  private stick(root: SVGSVGElement, x: number, y: number, click: Button, axes: [Axis, Axis]): void {
    const ring = svg("circle", { cx: x, cy: y, r: 24 }, root, click);
    this.button(click, ring);
    const dot = svg("circle", { class: "dot", cx: x, cy: y, r: 9 }, root);
    this.sticks.push({ ring, dot, x, y, axes });
  }
}
