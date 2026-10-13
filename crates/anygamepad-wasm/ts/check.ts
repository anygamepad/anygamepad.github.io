// Type-checks the generated `.d.ts` against the way the web app uses it.
// CI copies this file next to the `wasm-bindgen` output and runs `tsc` on it;
// it is never executed.

import init, { Gamepads, version } from "./anygamepad_wasm.js";
import type {
  Axis,
  Button,
  DeviceEvent,
  DeviceInfo,
  Environment,
  GamepadSnapshot,
  GamepadState,
  OutputCommand,
  RawState,
} from "./anygamepad_wasm.js";

export async function main(): Promise<void> {
  await init();
  const libraryVersion: string = version();
  const gamepads = new Gamepads();
  const environment: Environment = gamepads.environment();
  const engine: "unknown" | "blink" | "gecko" | "webkit" = environment.browser.engine;
  const major: number | undefined = environment.browser.major_version;
  console.log(libraryVersion, engine, major, environment.os);

  const frame = (): void => {
    const events: DeviceEvent[] = gamepads.update();
    for (const event of events) {
      if (event.type === "added") {
        gamepads.open(event.device);
      } else {
        const gone: number = event.device;
        console.log("removed", gone);
      }
    }
    for (const device of gamepads.devices()) {
      const info: DeviceInfo | undefined = gamepads.info(device);
      if (info === undefined) continue;
      const id: string | undefined = info.identity.web?.id;
      const vendor: number | undefined = info.identity.vendor_id;
      const fingerprint: string = info.fingerprint;
      const south: boolean = info.capabilities?.buttons.includes("south") ?? false;
      for (const source of info.layout.buttons) {
        if ("index" in source) console.log(source.index);
      }
      console.log(id, vendor, fingerprint, south);

      const snapshot: GamepadSnapshot | undefined = gamepads.snapshot(device);
      const pressed: boolean = snapshot?.buttons[0]?.pressed ?? false;
      const timestamp: number = snapshot?.timestamp ?? 0;

      const raw: RawState = gamepads.poll(device);
      const firstAxis: number | undefined = raw.axes[0];

      const state: GamepadState | undefined = gamepads.state(device);
      if (state !== undefined) {
        const jump: Button = "south";
        const extra: Button = "extra_3";
        const axis: Axis = "left_stick_y";
        const up: number = state.axes[axis];
        const jumping: boolean = state.buttons.includes(jump) || state.buttons.includes(extra);
        console.log(up, jumping);
      }
      console.log(pressed, timestamp, firstAxis);

      if (info.outputs.includes("rumble")) {
        const rumble: OutputCommand = { rumble: { low_frequency: 1, high_frequency: 0.5 } };
        gamepads.send(device, rumble);
        gamepads.send(device, { rumble: { low_frequency: 0, high_frequency: 0 } });
      }
      gamepads.open(device, "observe_only");
      gamepads.close(device);
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  gamepads.free();
}
