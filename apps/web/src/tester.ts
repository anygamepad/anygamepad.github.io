// The frame loop: reads the browser's gamepads through anygamepad's web
// provider (WebAssembly) and keeps the view's data up to date. Logic lives
// here, in TypeScript checked by `tsc`; components only render (ADR 0005).

import { Gamepads, type DeviceInfo, type Environment } from "./wasm/anygamepad_wasm.js";

/** One connected gamepad, ready to render. */
export interface DeviceRow {
  id: number;
  name: string;
  /** `vvvv:pppp`, or a dash where the browser hides the IDs. */
  ids: string;
  /** `Gamepad.mapping`: "standard" or "" (the browser's own layout). */
  mapping: string;
  buttons: number;
  axes: number;
  rumble: boolean;
}

/** The data the page renders. */
export interface TesterView {
  environment: Environment | null;
  devices: DeviceRow[];
  error: string | null;
}

export const emptyView = (): TesterView => ({ environment: null, devices: [], error: null });

const hex = (value: number | undefined): string =>
  value === undefined ? "----" : value.toString(16).padStart(4, "0");

export const deviceRow = (id: number, info: DeviceInfo): DeviceRow => ({
  id,
  name: info.identity.product_name ?? info.identity.web?.id ?? "Unknown gamepad",
  ids:
    info.identity.vendor_id === undefined
      ? "–"
      : `${hex(info.identity.vendor_id)}:${hex(info.identity.product_id)}`,
  mapping: info.identity.web?.mapping || "browser layout",
  buttons: info.layout.buttons.length,
  axes: info.layout.axes.length,
  rumble: info.outputs.includes("rumble"),
});

/**
 * Starts the frame loop, writing into `view` (a component's reactive data).
 * Returns a function that stops it.
 */
export function startTester(view: TesterView): () => void {
  let gamepads: Gamepads;
  try {
    gamepads = new Gamepads();
  } catch (error) {
    view.error = `This browser cannot read gamepads (${String(error)}).`;
    return () => {};
  }
  view.environment = gamepads.environment();

  let frame = 0;
  const tick = (): void => {
    try {
      if (gamepads.update().length > 0) {
        view.devices = [...gamepads.devices()].flatMap((id) => {
          const info = gamepads.info(id);
          return info === undefined ? [] : [deviceRow(id, info)];
        });
      }
    } catch (error) {
      view.error = `Reading the gamepads failed (${String(error)}).`;
    }
    frame = requestAnimationFrame(tick);
  };
  frame = requestAnimationFrame(tick);

  return () => {
    cancelAnimationFrame(frame);
    gamepads.free();
  };
}
