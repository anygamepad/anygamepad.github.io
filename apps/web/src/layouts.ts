// Controller drawings as data: one layout per family the library recognizes
// (`Family`), and a generic one for every other pad. A layout is either
// shapes placed by coordinates in a 400-wide viewBox, or an illustration
// (an SVG file whose interactive parts carry data attributes, described in
// art/dualsense.svg); drawing.ts renders both.
//
// The generic layout follows pads like the Flydigi Vader 5 Pro: Xbox-style
// positions, two extra buttons by the face buttons (C and Z) and four back
// buttons. Which extra index C and Z report is a guess until device
// profiles (D4) say it.

import type { Button, Family } from "./wasm/anygamepad_wasm.js";
import dualsense from "./art/dualsense.svg?raw";
import dualshock4 from "./art/dualshock4.svg?raw";

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
}

export type Layout = ShapeLayout | ArtLayout;

/** Xbox-style body: offset sticks, wide grips. */
const OFFSET_BODY =
  "M118 44 C150 38 250 38 282 44 C330 52 358 74 372 118 C386 162 394 208 372 236 " +
  "C352 258 322 250 302 222 C288 202 270 190 246 190 H154 C130 190 112 202 98 222 " +
  "C78 250 48 258 28 236 C6 208 14 162 28 118 C42 74 70 52 118 44 Z";

const XBOX: ShapeLayout = {
  kind: "shapes",
  name: "Xbox controller",
  height: 260,
  body: OFFSET_BODY,
  triggers: [88, 252],
  sticks: [[112, 100], [242, 152]],
  dpad: [158, 152],
  face: [288, 100],
  select: [172, 92],
  start: [228, 92],
  home: [200, 64],
  extras: [{ button: "share", at: [200, 114], shape: "pill" }],
};

const GENERIC: ShapeLayout = {
  kind: "shapes",
  name: "Controller",
  height: 290,
  body: OFFSET_BODY,
  triggers: [88, 252],
  sticks: [[112, 100], [242, 152]],
  dpad: [158, 152],
  face: [288, 100],
  select: [172, 92],
  start: [228, 92],
  home: [200, 64],
  extras: [
    { button: "extra_0", at: [318, 62], shape: "circle", label: "C" },
    { button: "extra_1", at: [340, 78], shape: "circle", label: "Z" },
    // Back buttons, drawn below the body.
    { button: "left_paddle_1", at: [70, 272], shape: "pill" },
    { button: "left_paddle_2", at: [110, 272], shape: "pill" },
    { button: "right_paddle_2", at: [290, 272], shape: "pill" },
    { button: "right_paddle_1", at: [330, 272], shape: "pill" },
  ],
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

const DUALSHOCK3: ShapeLayout = {
  kind: "shapes",
  name: "DualShock 3",
  height: 260,
  body:
    "M104 54 C124 46 152 44 200 44 C248 44 276 46 296 54 C336 62 358 86 370 128 " +
    "C384 176 392 220 368 242 C348 260 318 250 298 222 C286 204 274 196 254 196 H146 " +
    "C126 196 114 204 102 222 C82 250 52 260 32 242 C8 220 16 176 30 128 C42 86 64 62 104 54 Z",
  triggers: [76, 264],
  sticks: [[154, 156], [246, 156]],
  dpad: [88, 108],
  face: [312, 108],
  select: [180, 96],
  start: [220, 96],
  home: [200, 124],
  extras: [],
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
