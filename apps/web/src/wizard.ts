// The guided mapping wizard's view (W4): asks for each control of the usual
// layout in turn, on the pad's drawing, and says what answered. The logic
// is the library's (`MappingWizard`, fed by the tester's frame loop); this
// view only shows its status. Built once per run, updated per frame outside
// jq79's reactivity (ADR 0005).

import type {
  Axis,
  Button,
  ButtonLabel,
  FaceLayout,
  GamepadState,
  WizardResult,
  WizardStatus,
  WizardStep,
  WizardTarget,
} from "./wasm/anygamepad_wasm.js";
import { element, setText } from "./dom";
import { ControllerDrawing } from "./drawing";
import type { Layout } from "./layouts";

/** What the view's buttons do. */
export interface WizardActions {
  skip: () => void;
  back: () => void;
  restart: () => void;
  close: () => void;
}

/** Controls by position, as the prompts name them. */
const BUTTON_NAMES: Partial<Record<Button, string>> = {
  south: "the bottom face button",
  east: "the right face button",
  west: "the left face button",
  north: "the top face button",
  dpad_up: "up on the d-pad",
  dpad_down: "down on the d-pad",
  dpad_left: "left on the d-pad",
  dpad_right: "right on the d-pad",
  left_shoulder: "the left shoulder button (LB, L1, L)",
  right_shoulder: "the right shoulder button (RB, R1, R)",
  left_trigger: "the left trigger (LT, L2, ZL)",
  right_trigger: "the right trigger (RT, R2, ZR)",
  select: "the left menu button (View, Share or Create, −)",
  start: "the right menu button (Menu, Options, +)",
  home: "the system button in the middle (Xbox, PS, Home)",
  left_stick: "the left stick, pushed in like a button (L3)",
  right_stick: "the right stick, pushed in like a button (R3)",
};

const AXIS_NAMES: Record<Axis, string> = {
  left_stick_x: "the left stick, left and right",
  left_stick_y: "the left stick, up and down",
  right_stick_x: "the right stick, left and right",
  right_stick_y: "the right stick, up and down",
  left_trigger: "the left trigger (LT, L2, ZL)",
  right_trigger: "the right trigger (RT, R2, ZR)",
};

const LABELS: Record<ButtonLabel, string> = {
  a: "A",
  b: "B",
  x: "X",
  y: "Y",
  cross: "Cross",
  circle: "Circle",
  square: "Square",
  triangle: "Triangle",
  unknown: "",
};

const FACE: readonly Button[] = ["south", "east", "west", "north"];

/** What is printed on a face button, when the pad's family tells. */
const printed = (button: Button, labels: FaceLayout | undefined): string => {
  if (!labels || !FACE.includes(button)) return "";
  return LABELS[labels[button as keyof FaceLayout]];
};

/** A button's name, with what is printed on it. */
const buttonName = (button: Button, labels: FaceLayout | undefined): string => {
  const name = BUTTON_NAMES[button] ?? button;
  const label = printed(button, labels);
  return label ? `${name} (${label})` : name;
};

export const targetName = (target: WizardTarget, labels: FaceLayout | undefined): string =>
  target.kind === "button" ? buttonName(target.button, labels) : AXIS_NAMES[target.axis];

/** What a step asks the person to do. */
export const prompt = (step: WizardStep, labels: FaceLayout | undefined): string => {
  switch (step.kind) {
    case "button":
      return `Press ${buttonName(step.button, labels)}`;
    case "trigger":
      return `Pull ${AXIS_NAMES[step.axis]} all the way`;
    case "stick": {
      const stick = step.axis.startsWith("left") ? "left" : "right";
      return `Push the ${stick} stick ${step.axis.endsWith("_x") ? "to the right" : "up"}`;
    }
  }
};

/** The controls a step asks for, to mark on the drawing. */
const stepControls = (step: WizardStep | undefined): (Button | Axis)[] => {
  if (!step) return [];
  if (step.kind === "button") return [step.button];
  if (step.kind === "stick") return [step.axis];
  return [step.axis, step.axis === "left_trigger" ? "left_trigger" : "right_trigger"];
};

/** The step's control, short, for the list of results. */
const stepName = (step: WizardStep, labels: FaceLayout | undefined): string => {
  switch (step.kind) {
    case "button":
      return buttonName(step.button, labels);
    case "trigger":
      return AXIS_NAMES[step.axis];
    case "stick": {
      const stick = step.axis.startsWith("left") ? "left" : "right";
      return `the ${stick} stick, ${step.axis.endsWith("_x") ? "right" : "up"}`;
    }
  }
};

const capitalized = (text: string): string => text.charAt(0).toUpperCase() + text.slice(1);

export class WizardView {
  readonly root: HTMLElement;
  private readonly labels: FaceLayout | undefined;
  private readonly progress: HTMLElement;
  private readonly bar: HTMLElement;
  private readonly prompt: HTMLElement;
  private readonly note: HTMLElement;
  private readonly hint: HTMLElement;
  private readonly drawing: ControllerDrawing;
  private readonly skipButton: HTMLButtonElement;
  private readonly backButton: HTMLButtonElement;
  private readonly results: HTMLOListElement;
  private readonly summary: HTMLElement;
  private lastStep = -1;
  private lastKey = "";

  constructor(root: HTMLElement, layout: Layout, labels: FaceLayout | undefined, actions: WizardActions) {
    this.root = root;
    this.labels = labels;
    root.replaceChildren();
    const box = element("section", "wizard", root);
    box.setAttribute("aria-label", "Identify the buttons");

    const head = element("div", "wizard-head", box);
    this.progress = element("p", "wizard-progress", head);
    const track = element("span", "wizard-bar", head);
    this.bar = element("span", "fill", track);
    this.prompt = element("p", "wizard-prompt", box);
    this.prompt.setAttribute("aria-live", "polite");
    this.note = element("p", "wizard-note", box);
    this.hint = element("p", "wizard-hint", box);

    // Every control shows: the wizard asks for the ones the pad may lack.
    this.drawing = new ControllerDrawing(layout, labels);
    box.append(this.drawing.element);

    const buttons = element("div", "wizard-actions", box);
    const button = (label: string, action: () => void, className = "action"): HTMLButtonElement => {
      const created = element("button", className, buttons, label);
      created.type = "button";
      created.addEventListener("click", action);
      return created;
    };
    this.skipButton = button("Not on this pad", actions.skip);
    this.backButton = button("Back", actions.back);
    button("Start over", actions.restart);
    button("Close", actions.close);

    this.summary = element("p", "wizard-summary", box);
    this.results = element("ol", "wizard-results", box);
  }

  /** Shows the wizard's status; `results` when the steps done changed. */
  update(status: WizardStatus, results: () => WizardResult[], state: GamepadState, showValues: boolean): void {
    const done = status.phase === "done";
    const key = JSON.stringify([status.step, status.phase, status.detected, status.skipping, status.conflict]);
    if (key !== this.lastKey) {
      this.lastKey = key;
      this.show(status);
    }
    if (status.step !== this.lastStep) {
      this.lastStep = status.step;
      this.showResults(results(), done);
      this.drawing.highlight(stepControls(status.current));
    }
    this.drawing.update(state, showValues);
  }

  private show(status: WizardStatus): void {
    const labels = this.labels;
    const done = status.phase === "done";
    setText(this.progress, done ? `All ${status.steps} steps done` : `Step ${status.step + 1} of ${status.steps}`);
    this.bar.style.setProperty("--value", String(status.step / status.steps));
    setText(this.prompt, done ? "All set." : status.current ? `${prompt(status.current, labels)}.` : "");

    let note = "";
    if (status.phase === "release") note = "Let go of every control…";
    else if (status.skipping) note = "Not on this pad: let go to confirm.";
    else if (status.phase === "held" && status.detected) note = `Detected ${status.detected}. Let go to confirm.`;
    else if (status.conflict) note = `That one is already ${targetName(status.conflict, labels)}.`;
    setText(this.note, note);

    let hint = "";
    if (!done && status.chord) {
      const [first, second] = status.chord.map((button) => printed(button, labels));
      hint = first && second
        ? `Not on this pad? Press ${first} and ${second} together.`
        : "Not on this pad? Press the bottom and right face buttons together.";
    }
    setText(this.hint, hint);
    this.skipButton.hidden = done;
    this.backButton.disabled = status.step === 0;
  }

  private showResults(results: WizardResult[], done: boolean): void {
    const labels = this.labels;
    const found = results.filter((result) => !result.missing).length;
    setText(
      this.summary,
      done ? `${found} controls identified, ${results.length - found} not on this pad.` : "",
    );
    this.results.replaceChildren();
    for (const result of results) {
      const row = element("li", result.missing ? "missing" : "", this.results);
      const name = capitalized(stepName(result.step, labels));
      // Long names are cut; the tooltip shows them whole.
      element("span", "control-name", row, name).title = name;
      // An analog trigger's button is its axis too: one input, twice.
      const inputs = [...new Set(result.inputs.map((input) => input.input))];
      element("code", "control-input", row, result.missing ? "not on this pad" : inputs.join(" "));
    }
  }
}

/**
 * Canonical names of the raw buttons and axes a finished run identified, by
 * raw index, for the live view's raw values (`b3`, `-a1`, `a0~` inputs).
 */
export const rawNames = (
  results: WizardResult[],
  buttons: number,
  axes: number,
): { buttons: string[]; axes: string[] } => {
  const names = { buttons: Array<string>(buttons).fill(""), axes: Array<string>(axes).fill("") };
  const add = (list: string[], index: number, name: string): void => {
    const current = list[index];
    if (current === undefined || current.split(" ").includes(name)) return;
    list[index] = current ? `${current} ${name}` : name;
  };
  for (const result of results) {
    for (const { target, input } of result.inputs) {
      const name = target.kind === "button" ? target.button : target.axis;
      const button = /^b(\d+)$/.exec(input);
      const axis = /^[+-]?a(\d+)~?$/.exec(input);
      if (button) add(names.buttons, Number(button[1]), name);
      else if (axis) add(names.axes, Number(axis[1]), name);
    }
  }
  return names;
};
