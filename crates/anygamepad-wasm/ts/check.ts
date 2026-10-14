// Type-checks the generated `.d.ts` against the way the web app uses it.
// CI copies this file next to the `wasm-bindgen` output and runs `tsc` on it;
// it is never executed.

import init, { Gamepads, HidDevices, version } from "./anygamepad_wasm.js";
import type {
  Axis,
  Button,
  ButtonLabel,
  DecodedInput,
  DeviceEvent,
  DeviceInfo,
  Environment,
  Family,
  GamepadSnapshot,
  GamepadState,
  HidDeviceInfo,
  HidFilter,
  HidInputReport,
  HidPoll,
  OutputCommand,
  RawImu,
  RawState,
  ReportForm,
  Transport,
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
      const family: Family | undefined = info.family;
      const south0: ButtonLabel | undefined = info.face_layout?.south;
      console.log(family === "dualsense", south0);
      const south: boolean = info.capabilities?.buttons.includes("south") ?? false;
      for (const source of info.layout.buttons) {
        if ("index" in source) console.log(source.index);
      }
      const firstButton: Button | undefined = info.canonical?.buttons[0] ?? undefined;
      const firstAxis0: Axis | undefined = info.canonical?.axes[0] ?? undefined;
      const updated: number | undefined = gamepads.timestamp(device);
      console.log(firstButton, firstAxis0, updated);
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

export async function webHid(button: HTMLButtonElement): Promise<void> {
  if (!HidDevices.isSupported()) return;
  const devices = new HidDevices();
  button.addEventListener("click", () => {
    const filters: HidFilter[] = [{ usage_page: 0x01, usage: 0x05 }, { vendor_id: 0x054c }];
    void devices.request(filters).then((picked: number) => console.log(picked));
    void devices.request();
  });
  for (const event of devices.update()) {
    if (event.type === "added") await devices.open(event.device);
  }
  for (const device of devices.devices()) {
    const info: HidDeviceInfo | undefined = devices.info(device);
    const vendor: number | undefined = info?.identity.vendor_id;
    const usage: number | undefined = info?.collections[0]?.usage;
    const item = info?.collections[0]?.input_reports[0]?.items[0];
    const variable: boolean = item?.flags.includes("variable") ?? false;
    const lastUsage: number | undefined = item?.usage_range?.maximum;
    const reports: HidInputReport[] = devices.reports(device);
    const firstByte: number | undefined = reports[0]?.data[0];
    const dropped: number | undefined = devices.droppedReports(device);
    const transport: Transport | undefined = info?.transport;
    const decodes: boolean = info?.decoded ?? false;
    const touchpadWidth: number | undefined = info?.touchpad?.width;
    const poll: HidPoll = devices.poll(device);
    const recorded: HidInputReport[] = devices.poll(device, true).reports;
    const input: DecodedInput | undefined = poll.input;
    const form: ReportForm | undefined = input?.form;
    const imu: RawImu | undefined = input?.imu;
    const finger = input?.state.touches[0];
    const failure: string | undefined = poll.error;
    if (form === "bluetooth_reduced") await devices.switchToFullReports(device);
    console.log(transport, decodes, touchpadWidth, poll.received, poll.last?.report_id, recorded.length);
    console.log(imu?.gyro[0], finger?.x, input?.state.power.percent, failure);
    const feature: DataView = await devices.receiveFeatureReport(device, 5);
    await devices.sendReport(device, 5, new Uint8Array([0x05, 0xff]));
    await devices.sendFeatureReport(device, 5, new Uint8Array([0x00]));
    console.log(vendor, usage, variable, lastUsage, firstByte, dropped, feature.byteLength);
    await devices.close(device);
  }
  devices.free();
}
