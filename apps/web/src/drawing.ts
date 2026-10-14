// A controller drawing that shows the canonical state (ADR 0001 positions):
// pressed buttons, trigger travel, stick positions and touchpad fingers,
// and optionally their values. The shape comes from the pad's family
// (layouts.ts); the labels printed on the face buttons from the library's
// `FaceLayout`. Built once per device and updated by the frame loop,
// outside jq79's reactivity (ADR 0005).

import type { Axis, Button, ButtonLabel, FaceLayout, GamepadState } from "./wasm/anygamepad_wasm.js";
import type { ExtraControl, Layout, Point } from "./layouts";

const SVG_NS = "http://www.w3.org/2000/svg";

const svg = <K extends keyof SVGElementTagNameMap>(
  tag: K,
  attributes: Record<string, string | number>,
  parent: Element,
  title?: string,
): SVGElementTagNameMap[K] => {
  const element = document.createElementNS(SVG_NS, tag);
  if (title) {
    const label = document.createElementNS(SVG_NS, "title");
    label.textContent = title;
    element.append(label);
  }
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value));
  parent.append(element);
  return element;
};

/** Travel of a stick dot, in drawing units. */
const STICK_TRAVEL = 15;
/** Width of a trigger's bar at full press. */
const TRIGGER_WIDTH = 64;
/** Distance from the face buttons' centre to each of them. */
const FACE_SPREAD = 21;

/** A signed value that keeps its width. */
const signed = (value: number): string => `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(2)}`;

interface Stick {
  dot: SVGCircleElement;
  value: SVGTextElement;
  at: Point;
  axes: [Axis, Axis];
}

interface Trigger {
  fill: SVGRectElement;
  value: SVGTextElement;
  axis: Axis;
}

export class ControllerDrawing {
  readonly element: SVGSVGElement;
  private readonly controls = new Map<Button, SVGGElement>();
  private readonly triggers: Trigger[] = [];
  private readonly sticks: Stick[] = [];
  private readonly fingers: SVGCircleElement[] = [];
  private readonly layout: Layout;
  private lastButtons = "";
  private lastAxes = "";
  private lastTouches = "";

  constructor(layout: Layout, labels: FaceLayout | undefined) {
    this.layout = layout;
    const root = document.createElementNS(SVG_NS, "svg");
    root.setAttribute("viewBox", `0 0 400 ${layout.height}`);
    root.setAttribute("class", "diagram");
    root.setAttribute("role", "img");
    root.setAttribute("aria-label", `${layout.name}, canonical positions`);
    this.element = root;

    svg("path", { class: "body", d: layout.body }, root);
    if (layout.lightbar) svg("path", { class: "lightbar", d: layout.lightbar }, root);

    const [left, right] = layout.triggers;
    this.trigger(root, left, "left_trigger", "end");
    this.trigger(root, right, "right_trigger", "start");
    this.control("left_shoulder", (g) => svg("rect", { x: left - 4, y: 24, width: 72, height: 11, rx: 5.5 }, g));
    this.control("right_shoulder", (g) => svg("rect", { x: right - 4, y: 24, width: 72, height: 11, rx: 5.5 }, g));

    if (layout.touchpad) {
      const { x, y, width, height } = layout.touchpad;
      this.control("touchpad_click", (g) => svg("rect", { class: "touchpad", x, y, width, height, rx: 8 }, g));
      for (let finger = 0; finger < 4; finger++) {
        this.fingers.push(svg("circle", { class: "finger", cx: x, cy: y, r: 5, visibility: "hidden" }, root));
      }
    }

    this.stick(root, layout.sticks[0], "left_stick", ["left_stick_x", "left_stick_y"]);
    this.stick(root, layout.sticks[1], "right_stick", ["right_stick_x", "right_stick_y"]);

    const [dx, dy] = layout.dpad;
    const arms: [Button, number, number][] = [
      ["dpad_up", dx - 7.5, dy - 22],
      ["dpad_down", dx - 7.5, dy + 7],
      ["dpad_left", dx - 22, dy - 7.5],
      ["dpad_right", dx + 7, dy - 7.5],
    ];
    for (const [button, x, y] of arms) {
      this.control(button, (g) => svg("rect", { x, y, width: 15, height: 15, rx: 2 }, g));
    }

    const [fx, fy] = layout.face;
    const face: [Button, number, number, keyof FaceLayout][] = [
      ["north", fx, fy - FACE_SPREAD, "north"],
      ["south", fx, fy + FACE_SPREAD, "south"],
      ["west", fx - FACE_SPREAD, fy, "west"],
      ["east", fx + FACE_SPREAD, fy, "east"],
    ];
    for (const [button, cx, cy, position] of face) {
      this.control(button, (g) => {
        svg("circle", { cx, cy, r: 10 }, g);
        glyph(g, labels?.[position] ?? "unknown", cx, cy);
      });
    }

    this.control("select", (g) => pill(g, layout.select, 16, 9));
    this.control("start", (g) => pill(g, layout.start, 16, 9));
    this.control("home", (g) => svg("circle", { cx: layout.home[0], cy: layout.home[1], r: 8 }, g));
    for (const extra of layout.extras) this.extra(extra);
  }

  /** Shows `state`, and the values when `showValues` is set. */
  update(state: GamepadState, showValues: boolean): void {
    this.element.classList.toggle("show-values", showValues);

    const buttons = state.buttons.join(" ");
    if (buttons !== this.lastButtons) {
      this.lastButtons = buttons;
      const pressed = new Set(state.buttons);
      for (const [button, element] of this.controls) element.classList.toggle("on", pressed.has(button));
    }

    const axes = Object.values(state.axes).join(" ");
    if (axes !== this.lastAxes) {
      this.lastAxes = axes;
      for (const { fill, value, axis } of this.triggers) {
        fill.setAttribute("width", (state.axes[axis] * TRIGGER_WIDTH).toFixed(1));
        value.textContent = state.axes[axis].toFixed(2);
      }
      for (const { dot, value, at, axes: [x, y] } of this.sticks) {
        // Canonical Y grows upwards, SVG Y downwards.
        dot.setAttribute("cx", (at[0] + state.axes[x] * STICK_TRAVEL).toFixed(1));
        dot.setAttribute("cy", (at[1] - state.axes[y] * STICK_TRAVEL).toFixed(1));
        value.textContent = `${signed(state.axes[x])} ${signed(state.axes[y])}`;
      }
    }

    const touchpad = this.layout.touchpad;
    if (!touchpad) return;
    const touches = JSON.stringify(state.touches);
    if (touches === this.lastTouches) return;
    this.lastTouches = touches;
    this.fingers.forEach((finger, slot) => {
      const point = state.touches[slot];
      finger.setAttribute("visibility", point ? "visible" : "hidden");
      if (!point) return;
      finger.setAttribute("cx", (touchpad.x + point.x * touchpad.width).toFixed(1));
      finger.setAttribute("cy", (touchpad.y + point.y * touchpad.height).toFixed(1));
    });
  }

  /** A control: a group titled with the canonical name, lit when pressed. */
  private control(button: Button, draw: (group: SVGGElement) => void): SVGGElement {
    const group = svg("g", { class: "control" }, this.element, button);
    draw(group);
    this.controls.set(button, group);
    return group;
  }

  private trigger(root: SVGSVGElement, x: number, axis: Axis, side: "start" | "end"): void {
    svg("rect", { class: "track", x, y: 6, width: TRIGGER_WIDTH, height: 12, rx: 4 }, root);
    const fill = svg("rect", { class: "fill", x, y: 6, width: 0, height: 12, rx: 4 }, root, axis);
    const valueX = side === "end" ? x - 6 : x + TRIGGER_WIDTH + 6;
    const value = svg("text", { class: "value", x: valueX, y: 16, "text-anchor": side }, root);
    value.textContent = "0.00";
    this.triggers.push({ fill, value, axis });
  }

  private stick(root: SVGSVGElement, at: Point, click: Button, axes: [Axis, Axis]): void {
    this.control(click, (g) => svg("circle", { cx: at[0], cy: at[1], r: 24 }, g));
    const dot = svg("circle", { class: "dot", cx: at[0], cy: at[1], r: 9 }, root);
    const value = svg("text", { class: "value", x: at[0], y: at[1] + 37, "text-anchor": "middle" }, root);
    this.sticks.push({ dot, value, at, axes });
  }

  private extra({ button, at, shape, label }: ExtraControl): void {
    this.control(button, (g) => {
      if (shape === "circle") svg("circle", { cx: at[0], cy: at[1], r: 7.5 }, g);
      else if (shape === "pill") pill(g, at, 20, 8);
      else svg("rect", { x: at[0] - 5, y: at[1] - 5, width: 10, height: 10, rx: 1.5 }, g);
      if (label) svg("text", { class: "glyph-text", x: at[0], y: at[1] }, g).textContent = label;
    });
  }
}

const pill = (parent: Element, [x, y]: Point, width: number, height: number): SVGRectElement =>
  svg("rect", { x: x - width / 2, y: y - height / 2, width, height, rx: height / 2 }, parent);

/** What is printed on a face button: a letter or a PlayStation symbol. */
const glyph = (parent: Element, label: ButtonLabel, x: number, y: number): void => {
  switch (label) {
    case "a":
    case "b":
    case "x":
    case "y":
      svg("text", { class: "glyph-text", x, y }, parent).textContent = label.toUpperCase();
      return;
    case "cross":
      svg("path", { class: "glyph", d: `M${x - 4} ${y - 4} L${x + 4} ${y + 4} M${x + 4} ${y - 4} L${x - 4} ${y + 4}` }, parent);
      return;
    case "circle":
      svg("circle", { class: "glyph", cx: x, cy: y, r: 4.5 }, parent);
      return;
    case "square":
      svg("rect", { class: "glyph", x: x - 4, y: y - 4, width: 8, height: 8 }, parent);
      return;
    case "triangle":
      svg("path", { class: "glyph", d: `M${x} ${y - 5} L${x + 5} ${y + 3.5} L${x - 5} ${y + 3.5} Z` }, parent);
      return;
    case "unknown":
      return;
  }
};
