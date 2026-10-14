// A controller drawing that shows the canonical state (ADR 0001 positions):
// pressed buttons, trigger travel, stick positions and touchpad fingers,
// and optionally their values. The drawing comes from the pad's family
// (layouts.ts): shapes placed by coordinates, labelled with the library's
// `FaceLayout`, or an illustration whose data attributes mark its controls.
// Built once per device and updated by the frame loop, outside jq79's
// reactivity (ADR 0005).

import type { Axis, Button, ButtonLabel, FaceLayout, GamepadState } from "./wasm/anygamepad_wasm.js";
import type { ArtLayout, ExtraControl, Layout, Point, ShapeLayout } from "./layouts";

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

/** Width of the viewBox the shape layouts and the text sizes are made for. */
const BASE_WIDTH = 400;

/** A signed value that keeps its width. */
const signed = (value: number): string => `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(2)}`;

const STICK_AXES = {
  left: ["left_stick_x", "left_stick_y"],
  right: ["right_stick_x", "right_stick_y"],
} as const;

interface Stick {
  /** What moves: a dot (shapes) or a cap group (illustrations). */
  moving: SVGElement;
  /** Its resting centre, for a dot; the cap moves from its own origin. */
  at: Point | undefined;
  travel: number;
  value: SVGTextElement;
  axes: readonly [Axis, Axis];
}

interface Trigger {
  fill: SVGRectElement;
  value: SVGTextElement;
  axis: Axis;
  width: number;
}

interface Area {
  x: number;
  y: number;
  width: number;
  height: number;
}

export class ControllerDrawing {
  readonly element: SVGSVGElement;
  private readonly controls = new Map<Button, SVGGElement>();
  private readonly triggers: Trigger[] = [];
  private readonly sticks: Stick[] = [];
  private readonly fingers: SVGCircleElement[] = [];
  private touchpad: Area | undefined;
  /** Drawing units per unit of the 400-wide base: text and marks scale. */
  private scale = 1;
  private lastButtons = "";
  private lastAxes = "";
  private lastTouches = "";

  constructor(layout: Layout, labels: FaceLayout | undefined) {
    const root = document.createElementNS(SVG_NS, "svg");
    root.setAttribute("class", "diagram");
    root.setAttribute("role", "img");
    root.setAttribute("aria-label", `${layout.name}, canonical positions`);
    this.element = root;
    if (layout.kind === "art") this.illustration(layout);
    else this.shapes(layout, labels);

    if (this.touchpad) {
      const { x, y } = this.touchpad;
      for (let finger = 0; finger < 4; finger++) {
        const r = 5 * this.scale;
        this.fingers.push(svg("circle", { class: "finger", cx: x, cy: y, r, visibility: "hidden" }, root));
      }
    }
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
      for (const { fill, value, axis, width } of this.triggers) {
        fill.setAttribute("width", (state.axes[axis] * width).toFixed(1));
        value.textContent = state.axes[axis].toFixed(2);
      }
      for (const { moving, at, travel, value, axes: [x, y] } of this.sticks) {
        // Canonical Y grows upwards, SVG Y downwards.
        const dx = state.axes[x] * travel;
        const dy = -state.axes[y] * travel;
        if (at) {
          moving.setAttribute("cx", (at[0] + dx).toFixed(1));
          moving.setAttribute("cy", (at[1] + dy).toFixed(1));
        } else {
          moving.setAttribute("transform", `translate(${dx.toFixed(1)} ${dy.toFixed(1)})`);
        }
        value.textContent = `${signed(state.axes[x])} ${signed(state.axes[y])}`;
      }
    }

    const touchpad = this.touchpad;
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

  /** Builds a drawing from an illustration (see art/dualsense.svg). */
  private illustration(layout: ArtLayout): void {
    const root = this.element;
    const source = new DOMParser().parseFromString(layout.svg, "image/svg+xml").documentElement;
    const viewBox = source.getAttribute("viewBox") ?? `0 0 ${BASE_WIDTH} 260`;
    root.setAttribute("viewBox", viewBox);
    this.scale = (Number(viewBox.split(/\s+/)[2]) || BASE_WIDTH) / BASE_WIDTH;
    for (const child of Array.from(source.childNodes)) root.append(document.importNode(child, true));

    for (const element of root.querySelectorAll<SVGGElement>("[data-button]")) {
      const button = element.dataset.button as Button;
      const title = document.createElementNS(SVG_NS, "title");
      title.textContent = button;
      element.prepend(title);
      element.classList.add("control");
      this.controls.set(button, element);
    }

    for (const slot of root.querySelectorAll<SVGGElement>("[data-trigger]")) {
      const [x, y, width, height] = ["x", "y", "width", "height"].map((key) => Number(slot.dataset[key]));
      this.trigger(slot, slot.dataset.trigger as Axis, x ?? 0, y ?? 0, width ?? TRIGGER_WIDTH, height ?? 12);
    }

    for (const stick of root.querySelectorAll<SVGGElement>("[data-stick]")) {
      const cap = stick.querySelector<SVGGElement>("[data-cap]");
      if (!cap) continue;
      // Below the body, between the grips: on the page's background.
      const valueY = Number(stick.dataset.valueY ?? 70);
      const value = svg("text", { class: "value", x: 0, y: valueY, "text-anchor": "middle" }, stick);
      value.style.fontSize = `${9 * this.scale}px`;
      const axes = STICK_AXES[stick.dataset.stick === "right" ? "right" : "left"];
      this.sticks.push({ moving: cap, at: undefined, travel: Number(stick.dataset.travel ?? 7), value, axes });
    }

    const area = root.querySelector<SVGElement>("[data-touchpad]")?.dataset.touchpad?.split(" ").map(Number);
    if (area?.length === 4) {
      const [x = 0, y = 0, width = 0, height = 0] = area;
      this.touchpad = { x, y, width, height };
    }
  }

  /** Builds a drawing from shapes placed by coordinates. */
  private shapes(layout: ShapeLayout, labels: FaceLayout | undefined): void {
    const root = this.element;
    root.setAttribute("viewBox", `0 0 400 ${layout.height}`);
    svg("path", { class: "body", d: layout.body }, root);
    if (layout.lightbar) svg("path", { class: "lightbar", d: layout.lightbar }, root);

    const [left, right] = layout.triggers;
    this.trigger(root, "left_trigger", left, 6, TRIGGER_WIDTH, 12);
    this.trigger(root, "right_trigger", right, 6, TRIGGER_WIDTH, 12);
    this.control("left_shoulder", (g) => svg("rect", { x: left - 4, y: 24, width: 72, height: 11, rx: 5.5 }, g));
    this.control("right_shoulder", (g) => svg("rect", { x: right - 4, y: 24, width: 72, height: 11, rx: 5.5 }, g));

    if (layout.touchpad) {
      const { x, y, width, height } = layout.touchpad;
      this.control("touchpad_click", (g) => svg("rect", { class: "touchpad", x, y, width, height, rx: 8 }, g));
      this.touchpad = layout.touchpad;
    }

    this.stick(layout.sticks[0], "left_stick", STICK_AXES.left);
    this.stick(layout.sticks[1], "right_stick", STICK_AXES.right);

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

  /** A control: a group titled with the canonical name, lit when pressed. */
  private control(button: Button, draw: (group: SVGGElement) => void): SVGGElement {
    const group = svg("g", { class: "control" }, this.element, button);
    draw(group);
    this.controls.set(button, group);
    return group;
  }

  /** A trigger's travel bar, with its value beside it, outwards. */
  private trigger(parent: Element, axis: Axis, x: number, y: number, width: number, height: number): void {
    const rx = Math.min(4 * this.scale, height / 2);
    svg("rect", { class: "track", x, y, width, height, rx }, parent);
    const fill = svg("rect", { class: "fill", x, y, width: 0, height, rx }, parent, axis);
    const left = axis === "left_trigger";
    const gap = 6 * this.scale;
    const valueX = left ? x - gap : x + width + gap;
    const value = svg("text", { class: "value", x: valueX, y: y + height - 3, "text-anchor": left ? "end" : "start" }, parent);
    value.style.fontSize = `${9 * this.scale}px`;
    value.textContent = "0.00";
    this.triggers.push({ fill, value, axis, width });
  }

  private stick(at: Point, click: Button, axes: readonly [Axis, Axis]): void {
    this.control(click, (g) => svg("circle", { cx: at[0], cy: at[1], r: 24 }, g));
    const dot = svg("circle", { class: "dot", cx: at[0], cy: at[1], r: 9 }, this.element);
    const value = svg("text", { class: "value", x: at[0], y: at[1] + 37, "text-anchor": "middle" }, this.element);
    this.sticks.push({ moving: dot, at, travel: STICK_TRAVEL, value, axes });
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
