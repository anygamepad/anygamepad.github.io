// The tester's vibration test: a level per motor, a pattern, and an
// optional mode in which two buttons of the pad play it (one the strong
// motor, one the weak). Each motor is a channel that plays the pattern
// once, or over and over while its pad button is held; the tester sends
// the combined levels to the pad every frame they change.

import type { Button, OutputCommand } from "./wasm/anygamepad_wasm.js";

/** A vibration pattern: the share of the level at `t` ms into a cycle. */
interface Pattern {
  id: string;
  name: string;
  /** Milliseconds of one cycle. */
  duration: number;
  at(t: number): number;
}

const square = (on: number, period: number) => (t: number): number => (t % period < on ? 1 : 0);

export const PATTERNS: Pattern[] = [
  { id: "constant", name: "Constant", duration: 1000, at: () => 1 },
  { id: "pulse", name: "Pulses", duration: 1200, at: square(150, 300) },
  { id: "fast", name: "Fast pulses", duration: 1000, at: square(50, 100) },
  {
    id: "heartbeat",
    name: "Heartbeat",
    duration: 1000,
    at: (t) => (t < 120 || (t >= 220 && t < 340) ? 1 : 0),
  },
  { id: "ramp_up", name: "Ramp up", duration: 1000, at: (t) => t / 1000 },
  { id: "ramp_down", name: "Ramp down", duration: 1000, at: (t) => 1 - t / 1000 },
  { id: "wave", name: "Wave", duration: 1600, at: (t) => (1 - Math.cos((2 * Math.PI * t) / 800)) / 2 },
];

/** The buttons the pad mode offers. */
export const PAD_BUTTONS: { id: Button; name: string }[] = [
  { id: "left_shoulder", name: "Left shoulder (L1, LB)" },
  { id: "right_shoulder", name: "Right shoulder (R1, RB)" },
  { id: "left_trigger", name: "Left trigger (L2, LT)" },
  { id: "right_trigger", name: "Right trigger (R2, RT)" },
  { id: "south", name: "South (Cross, A)" },
  { id: "east", name: "East (Circle, B)" },
  { id: "west", name: "West (Square, X)" },
  { id: "north", name: "North (Triangle, Y)" },
  { id: "left_stick", name: "Left stick (L3)" },
  { id: "right_stick", name: "Right stick (R3)" },
];

export type Motor = "strong" | "weak" | "triggers";

/** What the page shows and sets. */
export interface VibrationView {
  /** Levels in percent. */
  strong: number;
  weak: number;
  triggers: number;
  pattern: string;
  patterns: { id: string; name: string }[];
  /** Whether two buttons of the pad play the pattern. */
  padButtons: boolean;
  strongButton: Button;
  weakButton: Button;
  buttons: { id: Button; name: string }[];
}

export const emptyVibration = (): VibrationView => ({
  strong: 100,
  weak: 100,
  triggers: 100,
  pattern: "constant",
  patterns: PATTERNS.map(({ id, name }) => ({ id, name })),
  padButtons: false,
  strongButton: "left_shoulder",
  weakButton: "right_shoulder",
  buttons: PAD_BUTTONS,
});

interface Channel {
  start: number;
  /** Repeats while true: its pad button is held. */
  held: boolean;
}

/** One pad's motors. */
export class Vibration {
  private readonly channels = new Map<Motor, Channel>();
  private sent = { strong: 0, weak: 0, triggers: 0 };
  /** Whether each pad button was down at the last frame. */
  private down = { strong: false, weak: false };

  constructor(private readonly view: VibrationView) {}

  /** Plays the pattern once on these motors. */
  play(motors: Motor[], now: number): void {
    for (const motor of motors) this.channels.set(motor, { start: now, held: false });
  }

  stop(): void {
    this.channels.clear();
  }

  /** Whether a motor plays. */
  get playing(): boolean {
    return this.channels.size > 0;
  }

  /**
   * Once a frame: reads the pad buttons (when the mode is on) and returns
   * the commands that change what the motors do.
   */
  frame(now: number, pressed: readonly Button[] | undefined, triggers: boolean): OutputCommand[] {
    if (this.view.padButtons && pressed) {
      for (const motor of ["strong", "weak"] as const) {
        const button = motor === "strong" ? this.view.strongButton : this.view.weakButton;
        const down = pressed.includes(button);
        const channel = this.channels.get(motor);
        if (down && !this.down[motor]) this.channels.set(motor, { start: now, held: true });
        else if (!down && channel) channel.held = false;
        this.down[motor] = down;
      }
    }
    const pattern = PATTERNS.find((p) => p.id === this.view.pattern) ?? (PATTERNS[0] as Pattern);
    const level = (motor: Motor): number => {
      const channel = this.channels.get(motor);
      if (!channel) return 0;
      const t = now - channel.start;
      if (t >= pattern.duration && !channel.held) {
        this.channels.delete(motor);
        return 0;
      }
      const share = Math.min(1, Math.max(0, pattern.at(t % pattern.duration)));
      return Math.round(share * this.view[motor]) / 100;
    };
    const next = { strong: level("strong"), weak: level("weak"), triggers: level("triggers") };
    const commands: OutputCommand[] = [];
    if (next.strong !== this.sent.strong || next.weak !== this.sent.weak) {
      commands.push({ rumble: { low_frequency: next.strong, high_frequency: next.weak } });
    }
    if (triggers && next.triggers !== this.sent.triggers) {
      commands.push({ trigger_rumble: { left: next.triggers, right: next.triggers } });
    }
    this.sent = next;
    return commands;
  }
}
