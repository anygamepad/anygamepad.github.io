// Controller drawings as data: one layout per family the library recognizes
// (`Family`), and a generic one for every other pad. A layout is either
// shapes placed by coordinates in a 400-wide viewBox, or an illustration
// (an SVG file whose interactive parts carry data attributes, described in
// art/dualsense.svg); drawing.ts renders both.
//
// The generic layout follows pads like the Flydigi Vader 5 Pro: Xbox-style
// positions and two extra buttons by the face buttons (C and Z). It is the
// Xbox illustration with both. Back buttons are not drawn: pads map them to
// front buttons (owner, 2026-10-30). Which extra index
// C and Z report is a guess until device profiles (D4) say it.

import type { Button, Family } from "./wasm/anygamepad_wasm.js";
import dualsense from "./art/dualsense.svg?raw";
import dualshock3 from "./art/dualshock3.svg?raw";
import dualshock4 from "./art/dualshock4.svg?raw";
import xbox from "./art/xbox.svg?raw";

export type Point = readonly [number, number];

/** A control drawn besides the common ones. */
export interface ExtraControl {
  button: Button;
  at: Point;
  shape: "circle" | "pill" | "square";
  /** Printed on the control. */
  label?: string;
}

/** A drawing made of shapes placed by coordinates. */
export interface ShapeLayout {
  kind: "shapes";
  /** What the drawing shows, for assistive technology. */
  name: string;
  /** Height of the viewBox. */
  height: number;
  /** Outline of the body. */
  body: string;
  /** Left edges of the trigger and shoulder bars. */
  triggers: readonly [number, number];
  /** The left and right sticks' centres. */
  sticks: readonly [Point, Point];
  dpad: Point;
  /** Centre of the four face buttons. */
  face: Point;
  select: Point;
  start: Point;
  home: Point;
  /** Touchpad (PlayStation): its click is the touchpad button. */
  touchpad?: { x: number; y: number; width: number; height: number };
  /** Light bar (DualShock 4), drawn only. */
  lightbar?: string;
  extras: readonly ExtraControl[];
}

/** A drawing from an illustration marked up with data attributes. */
export interface ArtLayout {
  kind: "art";
  /** What the drawing shows, for assistive technology. */
  name: string;
  /** The SVG document. */
  svg: string;
  /** Controls of the illustration this family does not have. */
  hide?: readonly Button[];
  /**
   * Optional controls (`data-optional`) this family does not have, dimmed
   * whatever buttons the pad reports.
   */
  dim?: readonly Button[];
}

export type Layout = ShapeLayout | ArtLayout;

// The owner's illustration (art/xbox.svg). C and Z show dimmed on Xbox pads:
// Chromium reports an Xbox Series pad's share button as the first extra
// button, so the buttons the pad reports cannot tell.
const XBOX: ArtLayout = {
  kind: "art",
  name: "Xbox controller",
  svg: xbox,
  dim: ["extra_0", "extra_1"],
};

const GENERIC: ArtLayout = {
  kind: "art",
  name: "Controller",
  svg: xbox,
};

const SWITCH_PRO: ShapeLayout = {
  kind: "shapes",
  name: "Switch Pro Controller",
  height: 260,
  body:
    "M112 46 C150 40 250 40 288 46 C334 54 360 78 372 124 C384 170 388 214 364 238 " +
    "C344 256 316 248 298 222 C286 204 268 194 246 194 H154 C132 194 114 204 102 222 " +
    "C84 248 56 256 36 238 C12 214 16 170 28 124 C40 78 66 54 112 46 Z",
  triggers: [88, 252],
  sticks: [[112, 102], [242, 154]],
  dpad: [158, 154],
  face: [288, 102],
  select: [170, 72],
  start: [230, 72],
  home: [224, 104],
  extras: [{ button: "capture", at: [176, 104], shape: "square" }],
};

// The owner's illustration (art/dualsense.svg).
const DUALSENSE: ArtLayout = {
  kind: "art",
  name: "DualSense",
  svg: dualsense,
};

// The owner's illustration (art/dualshock3.svg).
const DUALSHOCK3: ArtLayout = {
  kind: "art",
  name: "DualShock 3",
  svg: dualshock3,
};

// The owner's illustration (art/dualshock4.svg).
const DUALSHOCK4: ArtLayout = {
  kind: "art",
  name: "DualShock 4",
  svg: dualshock4,
};

/** The drawing for a family; the generic one when it is not recognized. */
export const layoutFor = (family: Family | undefined): Layout => {
  switch (family) {
    case "dualshock3":
      return DUALSHOCK3;
    case "dualshock4":
      return DUALSHOCK4;
    case "dualsense":
      return DUALSENSE;
    case "dualsense_edge":
      return { ...DUALSENSE, name: "DualSense Edge" };
    case "switch_pro":
      return SWITCH_PRO;
    case "xbox":
      return XBOX;
    case undefined:
      return GENERIC;
  }
};
