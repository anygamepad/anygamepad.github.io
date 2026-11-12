// The tester: reads the browser's gamepads and, on request, WebHID devices
// through anygamepad (WebAssembly), keeps the list the page renders, and
// drives the live view of the selected pad. Logic lives here, in TypeScript
// checked by `tsc`; components only render (ADR 0005).
//
// Two loops: a sampler every few milliseconds reads the Gamepad API (hotplug
// and how often each gamepad's data changes, which a once-per-frame read
// would cap at the display's refresh rate), and the frame loop drains the
// WebHID report queues and draws.

import {
  Gamepads,
  HidDevices,
  version,
  type DeviceInfo,
  type Environment,
  type GamepadSnapshot,
  type GamepadState,
  type HidDeviceInfo,
  type MappingWizard,
  type OutputCommand,
  type WizardStatus,
  type ScrubResult,
  type Redaction,
  type Transport,
  type Button,
} from "./wasm/anygamepad_wasm.js";
import { deleteReport, listReports, markReportShared, saveReport, type StoredReport } from "./storage";
import { ActivityLog } from "./activity";
import { ControllerDrawing } from "./drawing";
import { HidLiveView } from "./hid-live";
import { layoutFor } from "./layouts";
import { LiveView } from "./live";
import { CountMeter, RateMeter } from "./rate";
import { emptySettings, Sync, syncEndpoint, type SettingsView } from "./sync";
import { emptyTestView, loadTesting, TestMode, type TestHost, type TestPad, type TestView } from "./test-mode";
import { rawNames, WizardView } from "./wizard";
import { emptyPadInfo, factGroups, readController, type InfoGroup, type InfoRow, type PadInfoView } from "./controller-info";
import { emptyVibration, Vibration, type Motor, type VibrationView } from "./vibration";

/** The page's views, each a tab. */
export type Tab = "tester" | "activity" | "test";

/** The activity view: the log's counts and its newest events. */
export interface ActivityView {
  since: string;
  events: number;
  inputs: number;
  dropped: number;
  withInputs: boolean;
  recent: { key: number; time: string; type: string; pad: string; fields: string; kind: string }[];
}

export const emptyActivity = (): ActivityView => ({
  since: "",
  events: 0,
  inputs: 0,
  dropped: 0,
  withInputs: false,
  recent: [],
});

/** The player LED choices. */
const PLAYER_OPTIONS = [
  { value: 0, name: "Off" },
  { value: 1, name: "1" },
  { value: 2, name: "2" },
  { value: 3, name: "3" },
  { value: 4, name: "4" },
];

/** The URL of each tab. */
export const TAB_URLS: Record<Tab, string> = { tester: "/", activity: "/#activity", test: "/test/" };

/** The tab a URL shows. */
export const tabOf = (location: Location, page: string | undefined): Tab =>
  location.pathname.startsWith("/test") || page === "test"
    ? "test"
    : location.hash === "#activity"
      ? "activity"
      : "tester";

/** One connected pad, as the page lists it. */
export interface PadRow {
  /** `gamepad:<id>` or `webhid:<id>`. */
  key: string;
  source: "gamepad" | "webhid";
  name: string;
  /** `vvvv:pppp`, or a dash where the browser hides the IDs. */
  ids: string;
  /** One line of facts under the name. */
  facts: string;
  /** `Gamepad.id` verbatim, or the WebHID product name. */
  detail: string;
  rumble: boolean;
  triggerRumble: boolean;
  /** Whether the page can set its light bar colour. */
  lightbar: boolean;
  /** Whether the page can set its player LEDs. */
  playerLeds: boolean;
  /** Whether the guided wizard can identify its buttons. */
  wizard: boolean;
}

/** State of the active share modal. */
export interface ActiveShare {
  id: number;
  report: string;
  scrubbedReport: string;
  redactions: Redaction[];
  redactionPaths: string;
  copied: boolean;
  copyError: boolean;
}

/** The data the page renders. */
export interface TesterView {
  /** The tab: the tester at `/`, the activity log at `/#activity`, or the
   * test mode (H4) at `/test/`. */
  mode: Tab;
  test: TestView;
  settings: SettingsView;
  environment: Environment | null;
  pads: PadRow[];
  /** Key of the pad shown large. */
  selected: string | null;
  /** The selected pad's row. */
  current: PadRow | null;
  /** Whether the drawing shows the values. */
  showValues: boolean;
  /** Whether the selected pad shows the guided wizard. */
  wizardOpen: boolean;
  /** Whether the browser has WebHID. */
  webhid: boolean;
  /** The outcome of the last WebHID action, when it needs saying. */
  hidMessage: string | null;
  error: string | null;
  /** Why a gamepad may not show in this browser, while none does. */
  hint: string | null;
  /** Device reports saved locally in IndexedDB. */
  savedReports: StoredReport[];
  /** Active report in the share view modal. */
  activeShare: ActiveShare | null;
  /** Temporary notice shown after saving a report. */
  saveNotice: string | null;
  /** "This controller", beside the selected pad. */
  padInfo: PadInfoView;
  vibration: VibrationView;
  /** The light bar colour picked last, `#rrggbb`. */
  lightColour: string;
  /** The player LEDs set last; -1 for none yet. */
  playerLed: number;
  playerOptions: { value: number; name: string }[];
  activity: ActivityView;
}

export const emptyView = (): TesterView => ({
  mode: "tester",
  test: emptyTestView(),
  settings: emptySettings(),
  environment: null,
  pads: [],
  selected: null,
  current: null,
  showValues: false,
  wizardOpen: false,
  webhid: false,
  hidMessage: null,
  error: null,
  hint: null,
  savedReports: [],
  activeShare: null,
  saveNotice: null,
  padInfo: emptyPadInfo(),
  vibration: emptyVibration(),
  lightColour: "#0000ff",
  playerLed: -1,
  playerOptions: PLAYER_OPTIONS,
  activity: emptyActivity(),
});

/**
 * A known reason for a gamepad not to show in this browser, if any.
 *
 * macOS switches the DualSense to its extended report, which Chromium's HID
 * reader cannot decode, so Chromium never sees an input and never exposes it
 * (docs/research/06-browser-gamepad-api.md). WebHID reads that report.
 */
export const missingGamepadHint = (environment: Environment): string | null =>
  environment.browser.engine === "blink" && environment.os === "macos"
    ? "A DualSense does not show in Chrome, Edge or other Chromium browsers on macOS: " +
      "macOS switches it to a report mode they cannot read. Connect it with WebHID below, " +
      "or try Safari, which reads it through macOS."
    : null;

/** The least time between two light bar writes, in milliseconds. */
const LIGHT_MS = 50;

/** How often the activity view refreshes, in milliseconds. */
const ACTIVITY_MS = 500;

/** Events the activity view lists. */
const ACTIVITY_EVENTS = 200;

/** How long a recording of WebHID reports lasts, in milliseconds. */
export const RECORD_MS = 2000;

/** The document title of each tab. */
const TITLES: Record<Tab, string> = {
  tester: "anygamepad: gamepad tester",
  activity: "anygamepad: activity log",
  test: "anygamepad: test mode",
};

const FAMILY_NAMES: Record<string, string> = {
  dualshock3: "DualShock 3",
  dualshock4: "DualShock 4",
  dualsense: "DualSense",
  dualsense_edge: "DualSense Edge",
  switch_pro: "Switch Pro Controller",
  xbox: "Xbox",
};

/** What the browser tells about a pad, for "This controller". */
const identityRows = (pad: Pad): InfoRow[] => {
  const { identity, family } = pad.info;
  const rows: InfoRow[] = [{ label: "Read through", value: pad.source === "gamepad" ? "Gamepad API" : "WebHID" }];
  rows.push({ label: "USB IDs", value: pad.row.ids === "–" ? "Hidden by the browser" : pad.row.ids });
  if (family) rows.push({ label: "Family", value: FAMILY_NAMES[family] ?? family });
  if (pad.source === "gamepad") {
    rows.push({ label: "Mapping", value: identity.web?.mapping ? `"${identity.web.mapping}"` : "None (the browser's own layout)" });
    rows.push({ label: "Buttons, axes", value: `${pad.info.layout.buttons.length}, ${pad.info.layout.axes.length}` });
  } else {
    rows.push({ label: "Connection", value: TRANSPORTS[pad.info.transport] ?? "Not told" });
    rows.push({ label: "Reports", value: pad.info.decoded ? "Decoded by the library" : "Raw" });
  }
  const outputs = pad.source === "gamepad" ? pad.info.outputs : pad.info.outputs;
  rows.push({ label: "Outputs", value: outputs.length ? outputs.join(", ").replaceAll("_", " ") : "None" });
  return rows;
};

/** The sampler's period (browsers clamp timers to about 4 ms). */
const SAMPLE_MS = 4;

/** How often the activity log notes each pad's update rate, in milliseconds. */
const RATE_LOG_MS = 10_000;

const hex = (value: number | undefined): string =>
  value === undefined ? "----" : value.toString(16).padStart(4, "0");

const idsOf = (vendor: number | undefined, product: number | undefined): string =>
  vendor === undefined ? "–" : `${hex(vendor)}:${hex(product)}`;

export const gamepadRow = (id: number, info: DeviceInfo): PadRow => {
  const ids = idsOf(info.identity.vendor_id, info.identity.product_id);
  const mapping = info.identity.web?.mapping
    ? `"${info.identity.web.mapping}" mapping`
    : "no mapping (the browser's own layout)";
  return {
    key: `gamepad:${id}`,
    source: "gamepad",
    name: info.identity.product_name ?? info.identity.web?.id ?? "Unknown gamepad",
    ids,
    facts: `${ids} · ${mapping} · ${info.layout.buttons.length} buttons · ${info.layout.axes.length} axes`,
    detail: info.identity.web?.id ?? "",
    rumble: info.outputs.includes("rumble"),
    triggerRumble: info.outputs.includes("trigger_rumble"),
    // The Gamepad API drives no lights.
    lightbar: false,
    playerLeds: false,
    wizard: true,
  };
};

const TRANSPORTS: Record<string, string> = { usb: "USB", bluetooth: "Bluetooth" };

export const hidRow = (id: number, info: HidDeviceInfo): PadRow => {
  const ids = idsOf(info.identity.vendor_id, info.identity.product_id);
  const facts = [ids, "WebHID", TRANSPORTS[info.transport], info.decoded ? "decoded" : "raw reports"];
  return {
    key: `webhid:${id}`,
    source: "webhid",
    name: info.identity.product_name ?? "HID device",
    ids,
    facts: facts.filter(Boolean).join(" · "),
    detail: info.identity.product_name ?? "",
    // The library drives the outputs of the pads it decodes.
    rumble: info.outputs.includes("rumble"),
    triggerRumble: false,
    lightbar: info.outputs.includes("lightbar"),
    playerLeds: info.outputs.includes("player_leds"),
    // The library checks its decoding of the pads it decodes; raw devices
    // need a HID report parser first.
    wizard: info.decoded,
  };
};

/** A run of the guided wizard on a pad, kept until the pad goes. */
interface WizardRun {
  wizard: MappingWizard;
  /** Steps done, as last logged. */
  logged: number;
  done: boolean;
}

interface GamepadPad {
  source: "gamepad";
  id: number;
  info: DeviceInfo;
  row: PadRow;
  rate: RateMeter;
  vibration: Vibration;
  run: WizardRun | undefined;
}

interface Recording {
  start: number;
  reports: { t: number; report_id: number; data: string }[];
}

interface HidPad {
  source: "webhid";
  id: number;
  info: HidDeviceInfo;
  row: PadRow;
  rate: CountMeter;
  open: boolean;
  switching: boolean;
  recording: Recording | undefined;
  vibration: Vibration;
  run: WizardRun | undefined;
  /** What the pad told about itself, when asked. */
  facts: InfoGroup[];
  reader: PadInfoView["reader"];
  /** The light bar colour waiting for the next write, and its timer. */
  light: { pending: OutputCommand | undefined; timer: number | undefined };
}

type Pad = GamepadPad | HidPad;

const toHex = (data: Uint8Array): string => Array.from(data, (byte) => byte.toString(16).padStart(2, "0")).join("");

/** Downloads `content` as a JSON file. */
const saveJson = (name: string, content: unknown): void => {
  const blob = new Blob([JSON.stringify(content, null, 1)], { type: "application/json" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = name;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(link.href), 1000);
};

/** A timestamp for file names: `2026-10-14T08-47-11`. */
const fileTime = (): string => new Date().toISOString().slice(0, 19).replaceAll(":", "-");

/** A Gamepad API pad as read in one frame. */
interface GamepadRead {
  snapshot: GamepadSnapshot;
  state: GamepadState | undefined;
}

export class Tester {
  private gamepads: Gamepads | undefined;
  private hids: HidDevices | undefined;
  private view: TesterView = emptyView();
  private readonly pads = new Map<string, Pad>();
  private live: { key: string; root: HTMLElement; view: LiveView | HidLiveView | WizardView } | undefined;
  private sampler: number | undefined;
  private frame = 0;
  private readonly log = new ActivityLog();
  private lastRateLog = 0;
  private testMode: TestMode | undefined;
  /** Whether the test mode's module is loading. */
  private testLoading = false;
  /** Each pad's canonical state at the last frame, for the test mode. */
  private readonly states = new Map<string, GamepadState | undefined>();

  private sync: Sync | undefined;
  /** When the activity view last refreshed. */
  private lastActivity = 0;

  /** The settings' actions, for App.html. */
  readonly settingsActions = {
    openSettings: (): void => {
      this.view.settings.open = true;
    },
    closeSettings: (): void => {
      this.view.settings.open = false;
    },
    setSending: (on: boolean): void => this.sync?.setSending(on),
    withdrawSent: (): void => void this.sync?.withdrawAll(),
  };

  /** The test mode's actions, for App.html; they wait for `start()`. */
  readonly testActions = {
    testSearch: (text: string): void => this.testMode?.search(text),
    testPickModel: (id: string, unit?: string): void => this.testMode?.pickModel(id, unit),
    testPickCard: (key: string): void => this.testMode?.pickCard(key),
    testBack: (): void => this.testMode?.back(),
    testBackToCards: (): void => this.testMode?.backToCards(),
    testGoOn: (): void => this.testMode?.goOn(),
    testPickCandidate: (index: number): void => this.testMode?.pickCandidate(index),
    testSetTransport: (transport: Transport): void => this.testMode?.setTransport(transport),
    testSetUnit: (code: string): void => this.testMode?.setUnit(code),
    testNewUnit: (): void => this.testMode?.newUnit(),
    testConfirm: (): void => this.testMode?.confirm(),
    testChooseAnother: (): void => this.testMode?.chooseAnother(),
    testStart: (plan: string): void => this.testMode?.start(plan),
    testAnswer: (answer: "yes" | "no" | "not_sure"): void => this.testMode?.answer(answer),
    testSkip: (): void => this.testMode?.skip(),
    testNotOnThisPad: (): void => this.testMode?.notOnThisPad(),
    testNothingHappens: (): void => this.testMode?.nothingHappens(),
    testRepeat: (): void => this.testMode?.repeat(),
    testStop: (): void => this.testMode?.stop(),
    testSetNotes: (text: string): void => this.testMode?.setNotes(text),
    testSave: (): void => void this.testMode?.save(),
    testDownload: (): void => this.testMode?.download(),
    testNext: (): void => this.testMode?.next(),
    testDownloadSaved: (id: number): void => this.testMode?.downloadSaved(id),
    testDeleteSaved: (id: number): void => this.testMode?.deleteSaved(id),
    testCheckFirmware: (): void => this.testMode?.checkFirmware(),
  };

  /** Starts reading gamepads into `view` (a component's reactive data). */
  start(view: TesterView): void {
    this.view = view;
    window.addEventListener("error", (event) => this.log.event("page_error", { message: event.message }));
    window.addEventListener("unhandledrejection", (event) =>
      this.log.event("page_error", { message: String(event.reason) }),
    );
    window.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      if (view.settings.open) view.settings.open = false;
      else if (view.activeShare) this.closeShare();
    });
    this.sync = new Sync(
      syncEndpoint(),
      this.log,
      // The page loaded; the activity tab is the tester's page.
      { library: version(), page: view.mode === "test" ? "test" : "tester", environment: () => this.view.environment },
      view.settings,
    );
    try {
      this.gamepads = new Gamepads();
    } catch (error) {
      this.fail(`This browser cannot read gamepads (${String(error)}).`);
      return;
    }
    view.environment = this.gamepads.environment();
    view.hint = missingGamepadHint(view.environment);
    if (HidDevices.isSupported()) {
      try {
        this.hids = new HidDevices();
        view.webhid = true;
      } catch {
        // WebHID present but unusable (a policy): the Gamepad API still works.
      }
    }
    this.log.event("session", {
      library: version(),
      page: view.mode,
      environment: view.environment,
      webhid: view.webhid,
      hint: view.hint,
    });
    document.addEventListener("visibilitychange", this.onVisibility);
    this.onVisibility();
    listReports()
      .then((reports) => {
        this.view.savedReports = reports;
      })
      .catch(() => {
        // Storage unavailable or blocked
      });
    this.frame = requestAnimationFrame(this.draw);
    window.addEventListener("popstate", () => this.showTab(tabOf(window.location, undefined)));
    this.showTab(view.mode);
  }

  /** Shows a tab, loading the test mode's module the first time. */
  private showTab(tab: Tab): void {
    if (this.view.mode !== tab) this.log.event("tab", { tab });
    this.view.mode = tab;
    document.title = TITLES[tab];
    if (tab === "test") this.openTestMode();
    if (tab === "activity") this.lastActivity = 0;
  }

  /** "This controller" for the selected pad. */
  private showInfo(): void {
    const pad = this.view.selected ? this.pads.get(this.view.selected) : undefined;
    if (!pad) {
      this.view.padInfo = emptyPadInfo();
      return;
    }
    this.view.padInfo = {
      identity: identityRows(pad),
      reader: pad.source === "webhid" && pad.reader ? { ...pad.reader } : null,
      groups: pad.source === "webhid" ? pad.facts : [],
    };
  }

  /** Refreshes the activity view, every so often while it shows. */
  private showActivity(now: number): void {
    if (this.view.mode !== "activity" || now - this.lastActivity < ACTIVITY_MS) return;
    this.lastActivity = now;
    const activity = this.view.activity;
    const stats = this.log.stats();
    activity.since = stats.since;
    activity.events = stats.events;
    activity.inputs = stats.inputs;
    activity.dropped = stats.dropped;
    activity.recent = this.log.recent(ACTIVITY_EVENTS, activity.withInputs).map((event, index) => {
      const { t, type, pad, input, ...fields } = event;
      return {
        key: stats.total - index,
        time: `${(t / 1000).toFixed(3)} s`,
        type,
        pad: typeof pad === "string" ? pad : "",
        fields: Object.keys(fields).length ? JSON.stringify(fields) : "",
        kind: type.includes("error") ? "error" : input ? "input" : "device",
      };
    });
  }

  /** Plays the vibration patterns of every pad, and listens to the pad
   * buttons of the selected one when that mode is on. */
  private vibrate(now: number): void {
    for (const pad of this.pads.values()) {
      const listening = this.view.vibration.padButtons && pad.row.key === this.view.selected && this.view.mode === "tester";
      if (!listening && !pad.vibration.playing) continue;
      const pressed = listening ? (this.states.get(pad.row.key)?.buttons ?? []) : undefined;
      for (const command of pad.vibration.frame(now, pressed, pad.row.triggerRumble)) this.output(pad, command);
    }
  }

  /** Shows a pad large. */
  readonly select = (key: string): void => {
    if (key !== this.view.selected) {
      this.view.wizardOpen = false;
      this.view.playerLed = -1;
    }
    this.view.selected = key;
    this.view.current = this.pads.get(key)?.row ?? null;
    this.showInfo();
  };

  /** Opens the guided wizard on a pad: a new run, or the one in progress. */
  readonly startWizard = (key: string): void => {
    const pad = this.pads.get(key);
    if (!pad || this.view.selected !== key) return;
    if (!pad.run) {
      let wizard: MappingWizard | undefined;
      try {
        wizard = pad.source === "gamepad" ? this.gamepads?.wizard(pad.id) : this.hids?.wizard(pad.id);
      } catch (error) {
        this.fail(`The guided wizard cannot start (${String(error)}).`);
        return;
      }
      if (!wizard) return;
      pad.run = { wizard, logged: 0, done: false };
    }
    this.log.event("wizard_opened", { pad: key, step: pad.run.logged });
    this.view.wizardOpen = true;
    this.live = undefined;
  };

  /** Closes the guided wizard's view; its run stays for the pad. */
  readonly closeWizard = (): void => {
    this.log.event("wizard_closed", { pad: this.view.selected });
    this.view.wizardOpen = false;
    this.live = undefined;
  };

  /** Shows or hides the values on the drawing. */
  readonly toggleValues = (): void => {
    this.view.showValues = !this.view.showValues;
  };

  /** Opens the browser's WebHID chooser (call it from a click). */
  readonly connectHid = (): void => {
    const hids = this.hids;
    if (!hids) return;
    this.view.hidMessage = null;
    hids.request().then(
      (picked) => {
        this.log.event("hid_chooser", { picked });
        if (picked === 0) this.view.hidMessage = "No device picked.";
      },
      (error: unknown) => this.hidNote(`WebHID refused (${String(error)}).`),
    );
  };

  /** Downloads the activity log (call it from a click). */
  readonly downloadLog = (): void => {
    saveJson(`anygamepad-activity-${fileTime()}.json`, this.log.toJSON(version(), this.view.environment));
  };

  /** Saves a device report of the currently selected pad to IndexedDB. */
  readonly saveCurrentReport = async (): Promise<void> => {
    if (!this.view.current || !this.view.selected) return;
    const pad = this.pads.get(this.view.selected);
    if (!pad) return;

    try {
      let scrubResult: ScrubResult;
      const wizard = pad.run?.wizard;
      if (pad.source === "gamepad") {
        if (!this.gamepads) throw new Error("Gamepads provider not available");
        if (wizard) {
          scrubResult = this.gamepads.buildReportWithWizard(pad.id, wizard, undefined, undefined, Date.now());
        } else {
          scrubResult = this.gamepads.buildReport(pad.id, undefined, undefined, Date.now());
        }
      } else {
        if (!this.hids) throw new Error("WebHID provider not available");
        if (wizard) {
          scrubResult = this.hids.buildReportWithWizard(pad.id, wizard, undefined, undefined, Date.now());
        } else {
          scrubResult = this.hids.buildReport(pad.id, undefined, undefined, Date.now());
        }
      }

      const reportJson = JSON.stringify(scrubResult.report);
      if (reportJson.length > 60000) {
        throw new Error(`Report exceeds maximum size of 60,000 bytes (${reportJson.length} bytes)`);
      }
      const saved = await saveReport(reportJson, scrubResult.redactions);
      this.view.savedReports = await listReports();
      this.view.saveNotice = `Report saved for ${saved.name}.`;
      this.log.event("report_saved", { pad: pad.row.key, id: saved.id });
      setTimeout(() => {
        if (this.view.saveNotice) this.view.saveNotice = null;
      }, 4000);
    } catch (err) {
      this.fail(`Failed to save report: ${String(err)}`);
    }
  };

  /** Downloads a saved report as a JSON file. */
  readonly downloadSavedReport = (id: number): void => {
    const report = this.view.savedReports.find((r) => r.id === id);
    if (!report) return;
    this.log.event("report_downloaded", { id });
    const filename = `${report.fingerprint || "device"}-${report.created}.json`;
    const blob = new Blob([report.report], { type: "application/json" });
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = filename;
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  };

  /** Deletes a saved report from this browser. */
  readonly deleteSavedReport = async (id: number): Promise<void> => {
    if (!window.confirm("Delete this report from this browser?")) return;
    try {
      await deleteReport(id);
      this.view.savedReports = await listReports();
      this.log.event("report_deleted", { id });
      if (this.view.activeShare?.id === id) {
        this.view.activeShare = null;
      }
    } catch (err) {
      this.fail(`Failed to delete report: ${String(err)}`);
    }
  };

  /** Opens the "Share this report" view. */
  readonly openShare = (id: number): void => {
    const report = this.view.savedReports.find((r) => r.id === id);
    if (!report) return;
    try {
      const redactions = report.redactions || [];
      const redactionPaths = redactions.map((r: Redaction) => r.field).join(", ");
      this.view.activeShare = {
        id,
        report: report.report,
        scrubbedReport: report.report,
        redactions,
        redactionPaths,
        copied: false,
        copyError: false,
      };
      this.log.event("report_share_opened", { id });
    } catch (err) {
      this.fail(`Failed to prepare report for sharing: ${String(err)}`);
    }
  };

  /** Closes the share modal. */
  readonly closeShare = (): void => {
    this.view.activeShare = null;
  };

  /** Copies the scrubbed report and opens the GitHub issue form. */
  readonly copyAndOpenIssue = async (): Promise<void> => {
    if (!this.view.activeShare) return;
    const share = this.view.activeShare;
    const report = this.view.savedReports.find((r) => r.id === share.id);
    const title = `Report: ${report?.name || "Game Controller"} (${report?.fingerprint || ""})`;
    const issueUrl = `https://github.com/anygamepad/anygamepad.github.io/issues/new?template=device-report.yml&title=${encodeURIComponent(title)}`;

    try {
      await navigator.clipboard.writeText(share.scrubbedReport);
      share.copied = true;
      share.copyError = false;
      // Before the storage writes: browsers block pop-ups opened too long
      // after the click.
      window.open(issueUrl, "_blank", "noopener,noreferrer");
      this.log.event("report_shared_issue", { id: share.id });
      await markReportShared(share.id);
      this.view.savedReports = await listReports();
    } catch {
      share.copyError = true;
    }
  };

  /** Sets a motor's level, in percent. */
  readonly setVibration = (motor: Motor, value: string): void => {
    this.view.vibration[motor] = Number(value);
  };

  /** Picks the vibration pattern. */
  readonly setPattern = (id: string): void => {
    this.view.vibration.pattern = id;
  };

  /** Turns the pad buttons that play the pattern on or off. */
  readonly setPadButtons = (on: boolean): void => {
    this.view.vibration.padButtons = on;
    this.log.event("vibration_pad_buttons", { on });
  };

  /** Picks the pad button that plays a motor. */
  readonly setPadButton = (motor: "strong" | "weak", button: string): void => {
    if (motor === "strong") this.view.vibration.strongButton = button as Button;
    else this.view.vibration.weakButton = button as Button;
  };

  /** Plays the pattern once on some motors. */
  readonly playVibration = (key: string, which: "strong" | "weak" | "both" | "triggers"): void => {
    const pad = this.pads.get(key);
    if (!pad) return;
    const motors: Motor[] = which === "both" ? ["strong", "weak"] : [which];
    const { pattern, strong, weak, triggers } = this.view.vibration;
    this.log.event("vibration_test", { pad: key, motors, pattern, levels: { strong, weak, triggers } });
    pad.vibration.play(motors, performance.now());
  };

  /** Stops every motor of a pad. */
  readonly stopVibration = (key: string): void => {
    this.pads.get(key)?.vibration.stop();
  };

  /** Sets the light bar of a pad the library drives through WebHID, at
   * most every few frames while the picker moves. */
  readonly setLight = (key: string, colour: string): void => {
    const pad = this.pads.get(key);
    const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(colour);
    if (!pad || pad.source !== "webhid" || !match) return;
    this.view.lightColour = colour;
    const [r, g, b] = match.slice(1).map((part) => parseInt(part, 16)) as [number, number, number];
    pad.light.pending = { lightbar: { r, g, b } };
    if (pad.light.timer !== undefined) return;
    const write = (): void => {
      const command = pad.light.pending;
      pad.light.pending = undefined;
      if (!command) {
        pad.light.timer = undefined;
        return;
      }
      this.output(pad, command, "Setting the light bar");
      pad.light.timer = window.setTimeout(write, LIGHT_MS);
    };
    this.log.event("light_test", { pad: key, colour });
    write();
  };

  /** Shows a player number on a pad's LEDs; 0 turns them off. */
  readonly player = (key: string, player: number): void => {
    const pad = this.pads.get(key);
    if (!pad) return;
    this.view.playerLed = player;
    this.log.event("player_leds_test", { pad: key, player });
    this.output(pad, { player_number: player }, "Setting the player LEDs");
  };

  /** Asks a WebHID PlayStation pad what it tells about itself. */
  readonly readInfo = (key: string): void => {
    const pad = this.pads.get(key);
    const hids = this.hids;
    const { vendor_id: vendor, product_id: product } = pad?.info.identity ?? {};
    if (!pad || pad.source !== "webhid" || !hids || !pad.reader || vendor == null || product == null) return;
    if (pad.reader.state === "reading") return;
    const reader = (pad.reader = { ...pad.reader, state: "reading" as const, text: "" });
    this.showInfo();
    this.log.event("controller_info_asked", { pad: key });
    const ids = { vendor, product, transport: pad.info.transport, collections: pad.info.collections };
    // Over Bluetooth, the calibration first: the pad switches to its full
    // report either way, and its motion then comes calibrated.
    const switched = pad.info.transport === "bluetooth" ? hids.switchToFullReports(pad.id).catch(() => undefined) : Promise.resolve();
    switched
      .then(() =>
        readController(hids, pad.id, ids, (done) => {
          reader.text = `(${done})`;
          this.showInfo();
        }),
      )
      .then(
        (facts) => {
          pad.facts = factGroups(facts);
          pad.reader = facts.firmware
            ? { ...reader, state: "done", text: "" }
            : {
                ...reader,
                state: "done",
                text: "It does not answer with firmware information, as Sony's pads do: most likely a compatible pad.",
              };
          // The firmware, board and colour describe the model; the usage
          // counters are specific to the unit and are not logged.
          this.log.event("controller_info", {
            pad: key,
            firmware: facts.firmware,
            bluetooth_patch: facts.bluetooth_patch,
            board: facts.board,
            colour: facts.colour_code,
            trigger_motors: facts.trigger_motors,
            touchpad_firmware: facts.touchpad_firmware,
            usage_counters: facts.usage.length,
          });
          this.showInfo();
        },
        (error: unknown) => {
          pad.reader = { ...reader, state: "failed", text: `The pad did not answer (${String(error)}).` };
          this.log.event("controller_info_failed", { pad: key, message: String(error) });
          this.showInfo();
        },
      );
  };

  /** Shows a tab; from a link's click, in place of following it. */
  readonly setTab = (tab: Tab, event?: Event): void => {
    event?.preventDefault();
    if (tab === this.view.mode) return;
    window.history.pushState(null, "", TAB_URLS[tab]);
    this.showTab(tab);
  };

  /** Shows or hides the input events in the activity view. */
  readonly setActivityInputs = (on: boolean): void => {
    this.view.activity.withInputs = on;
    this.lastActivity = 0;
  };

  /**
   * Sends a command to a pad; failures show as "`what` failed", or pass
   * silently without `what`. Returns whether the command went out.
   */
  private output(pad: Pad, command: OutputCommand, what?: string): boolean {
    const failed = (error: unknown): void => {
      if (what) this.fail(`${what} failed (${String(error)}).`);
    };
    try {
      if (pad.source === "gamepad") this.gamepads?.send(pad.id, command);
      else this.hids?.send(pad.id, command).catch(failed);
    } catch (error) {
      failed(error);
      return false;
    }
    return true;
  }

  /** Stops both loops and releases the devices. */
  stop(): void {
    document.removeEventListener("visibilitychange", this.onVisibility);
    window.clearInterval(this.sampler);
    cancelAnimationFrame(this.frame);
    for (const pad of this.pads.values()) {
      if (pad.source === "webhid") window.clearTimeout(pad.light.timer);
      pad.run?.wizard.free();
    }
    this.gamepads?.free();
    this.gamepads = undefined;
    this.hids?.free();
    this.hids = undefined;
  }

  /** Shows an error, and logs it. */
  private fail(message: string): void {
    this.view.error = message;
    this.log.event("error", { message });
  }

  /** Shows the outcome of a WebHID action, and logs it. */
  private hidNote(message: string): void {
    this.view.hidMessage = message;
    this.log.event("hid_message", { message });
  }

  // A hidden page needs neither loop (the frame loop pauses by itself).
  private readonly onVisibility = (): void => {
    this.log.event(document.hidden ? "page_hidden" : "page_visible");
    window.clearInterval(this.sampler);
    this.sampler = document.hidden ? undefined : window.setInterval(this.sample, SAMPLE_MS);
  };

  private readonly sample = (): void => {
    const gamepads = this.gamepads;
    if (!gamepads) return;
    try {
      let changed = false;
      const added: string[] = [];
      const removed: string[] = [];
      for (const event of gamepads.update()) {
        changed = true;
        const key = `gamepad:${event.device}`;
        if (event.type === "added") {
          this.addGamepad(gamepads, event.device);
          added.push(key);
        } else {
          this.remove(key);
          removed.push(key);
        }
      }
      for (const event of this.hids?.update() ?? []) {
        changed = true;
        const key = `webhid:${event.device}`;
        if (event.type === "added") {
          this.addHid(event.device);
          added.push(key);
        } else {
          this.remove(key);
          removed.push(key);
        }
      }
      if (changed) this.refresh();
      const now = performance.now();
      for (const key of removed) this.testMode?.removed(key, now);
      for (const key of this.view.mode === "test" ? added : []) {
        const pad = this.testPads().find((p) => p.key === key);
        if (pad) this.testMode?.added(pad, now);
      }
      for (const pad of this.pads.values()) {
        if (pad.source !== "gamepad") continue;
        const timestamp = gamepads.timestamp(pad.id);
        if (timestamp !== undefined) pad.rate.sample(timestamp, now);
      }
    } catch (error) {
      this.fail(`Reading the gamepads failed (${String(error)}).`);
      window.clearInterval(this.sampler);
    }
  };

  private addGamepad(gamepads: Gamepads, id: number): void {
    const info = gamepads.info(id);
    if (info === undefined) return;
    // Shared access: the live view polls it, and the vibration test drives it.
    gamepads.open(id);
    const row = gamepadRow(id, info);
    this.pads.set(row.key, {
      source: "gamepad",
      id,
      info,
      row,
      rate: new RateMeter(),
      vibration: new Vibration(this.view.vibration),
      run: undefined,
    });
    this.log.event("gamepad_added", {
      pad: row.key,
      identity: info.identity,
      fingerprint: info.fingerprint,
      family: info.family,
      face_layout: info.face_layout,
      buttons: info.layout.buttons.length,
      axes: info.layout.axes.length,
      outputs: info.outputs,
      canonical: info.canonical,
    });
  }

  private addHid(id: number): void {
    const hids = this.hids;
    const info = hids?.info(id);
    if (!hids || info === undefined) return;
    const row = hidRow(id, info);
    const pad: HidPad = {
      source: "webhid",
      id,
      info,
      row,
      rate: new CountMeter(),
      open: false,
      switching: false,
      recording: undefined,
      vibration: new Vibration(this.view.vibration),
      run: undefined,
      facts: [],
      reader:
        info.decoded
          ? { state: "offer", bluetooth: info.transport === "bluetooth", text: "" }
          : null,
      light: { pending: undefined, timer: undefined },
    };
    this.pads.set(row.key, pad);
    this.log.event("hid_added", {
      pad: row.key,
      identity: info.identity,
      fingerprint: info.fingerprint,
      family: info.family,
      transport: info.transport,
      decoded: info.decoded,
      touchpad: info.touchpad,
      collections: info.collections,
    });
    // Opening sends nothing to the device; it starts queueing its reports.
    hids.open(id).then(
      () => {
        pad.open = true;
        this.log.event("hid_opened", { pad: row.key });
        this.refresh();
      },
      (error: unknown) => this.hidNote(`Cannot open ${row.name} (${String(error)}).`),
    );
  }

  /** Loads the test mode's module, on the test mode's page only (W9). */
  private openTestMode(): void {
    if (this.testMode || this.testLoading || !this.gamepads) return;
    this.testLoading = true;
    loadTesting()
      .then((testing) => {
        this.testMode = new TestMode(testing, this.testHost(), this.view.test);
      })
      .catch((error: unknown) => {
        this.view.test.error = `The test mode did not load (${String(error)}).`;
        this.log.event("error", { message: `test mode: ${String(error)}` });
      })
      .finally(() => {
        this.testLoading = false;
      });
  }

  private testHost(): TestHost {
    return {
      gamepads: () => this.gamepads,
      hids: () => this.hids,
      environment: () => this.view.environment,
      pads: () => this.testPads(),
      sendTestReport: (report) => this.sync?.sendTestReport(report) ?? Promise.resolve(false),
    };
  }

  /** The pads the page lists, for the test mode. */
  private testPads(): TestPad[] {
    return this.view.pads.flatMap((row) => {
      const pad = this.pads.get(row.key);
      if (!pad) return [];
      return [
        {
          key: row.key,
          source: pad.source,
          id: pad.id,
          name: row.name,
          open: pad.source === "gamepad" || pad.open,
          state: this.states.get(row.key),
        },
      ];
    });
  }

  private remove(key: string): void {
    this.states.delete(key);
    const pad = this.pads.get(key);
    if (pad?.source === "webhid") window.clearTimeout(pad.light.timer);
    pad?.run?.wizard.free();
    if (key === this.view.selected) this.view.wizardOpen = false;
    this.pads.delete(key);
    this.log.event(key.startsWith("webhid:") ? "hid_removed" : "gamepad_removed", { pad: key });
    this.log.forget(key);
  }

  /**
   * Rebuilds the list. A pad open through WebHID and decoded hides its
   * Gamepad API twin (same VID/PID): one card per pad.
   */
  private refresh(): void {
    const decoded = new Set<string>();
    for (const pad of this.pads.values()) {
      if (pad.source === "webhid" && pad.open && pad.info.decoded) decoded.add(pad.row.ids);
    }
    this.view.pads = [...this.pads.values()]
      .filter((pad) => pad.source === "webhid" || !decoded.has(pad.row.ids))
      .map((pad) => pad.row);
    const selected = this.view.selected;
    if (!this.view.pads.some((row) => row.key === selected)) {
      // A pad just opened through WebHID takes the place of its twin.
      const next = this.view.pads.find((row) => row.source === "webhid" && decoded.has(row.ids)) ?? this.view.pads[0];
      if (next) this.select(next.key);
      else {
        this.view.selected = null;
        this.view.current = null;
        this.view.wizardOpen = false;
      }
    }
  }

  private switchToFull(pad: HidPad): void {
    if (!this.hids || pad.switching) return;
    pad.switching = true;
    this.log.event("hid_switch_to_full", { pad: pad.row.key });
    this.hids
      .switchToFullReports(pad.id)
      .catch((error: unknown) => this.hidNote(`Switching failed (${String(error)}).`))
      .finally(() => {
        pad.switching = false;
      });
  }

  private record(pad: HidPad): void {
    if (pad.recording) return;
    pad.recording = { start: performance.now(), reports: [] };
    this.log.event("hid_recording", { pad: pad.row.key });
  }

  /** Downloads a recording: the device's identity and structure, and its reports. */
  private download(pad: HidPad, recording: Recording): void {
    const { identity, family, transport, collections } = pad.info;
    const content = {
      format: "anygamepad-hid-recording-v1",
      library: version(),
      environment: this.view.environment,
      device: {
        vendor_id: identity.vendor_id,
        product_id: identity.product_id,
        product_name: identity.product_name,
        family,
        transport,
        collections,
      },
      reports: recording.reports,
    };
    saveJson(`anygamepad-${pad.row.ids.replace(":", "-")}-${new Date().toISOString().slice(0, 19)}.json`, content);
  }

  private readonly draw = (): void => {
    try {
      const now = performance.now();
      this.drainHid(now);
      const reads = this.readGamepads();
      this.drawSelected(now, reads);
      this.vibrate(now);
      this.showActivity(now);
      // Off its tab, the test mode only goes on with a session it runs.
      if (this.view.mode === "test" || this.testMode?.busy()) this.testMode?.frame(now);
      this.logRates(now);
    } catch (error) {
      this.fail(`Drawing the gamepads failed (${String(error)}).`);
      return;
    }
    this.frame = requestAnimationFrame(this.draw);
  };

  /** Reads every Gamepad API pad once a frame, for the log and the live view. */
  private readGamepads(): Map<string, GamepadRead> {
    const reads = new Map<string, GamepadRead>();
    const gamepads = this.gamepads;
    if (!gamepads) return reads;
    for (const pad of this.pads.values()) {
      if (pad.source !== "gamepad") continue;
      const snapshot = gamepads.snapshot(pad.id);
      if (snapshot === undefined) continue;
      const state = pad.info.canonical ? gamepads.state(pad.id) : undefined;
      this.states.set(pad.row.key, state);
      this.log.gamepad(pad.row.key, snapshot, state);
      reads.set(pad.row.key, { snapshot, state });
    }
    return reads;
  }

  /** Notes each pad's update rate in the log, every few seconds. */
  private logRates(now: number): void {
    if (now - this.lastRateLog < RATE_LOG_MS) return;
    this.lastRateLog = now;
    for (const pad of this.pads.values()) {
      this.log.event("rate", { pad: pad.row.key, per_second: pad.rate.rate(now) });
    }
  }

  /** Polls every open WebHID device, so queues drain and rates count. */
  private drainHid(now: number): void {
    const hids = this.hids;
    if (!hids) return;
    for (const pad of this.pads.values()) {
      if (pad.source !== "webhid" || !pad.open) continue;
      // Under test, the test mode reads it (its reports are evidence).
      if (this.testMode?.owns(pad.row.key)) continue;
      const recording = pad.recording;
      let poll;
      try {
        poll = hids.poll(pad.id, recording !== undefined);
      } catch {
        // Unplugged since the last sample: its removal comes with the next
        // one. Not a reason to stop drawing the others.
        pad.open = false;
        continue;
      }
      pad.rate.add(poll.received, now);
      this.states.set(pad.row.key, poll.input?.state);
      this.log.hid(pad.row.key, poll);
      if (recording) {
        for (const report of poll.reports) {
          recording.reports.push({
            t: Math.round((report.timestamp - recording.start) * 1000) / 1000,
            report_id: report.report_id,
            data: toHex(report.data),
          });
        }
        if (now - recording.start >= RECORD_MS) {
          pad.recording = undefined;
          this.download(pad, recording);
        }
      }
      if (this.live?.key === pad.row.key && this.live.view instanceof HidLiveView) {
        this.live.view.update(poll, {
          rate: pad.rate.rate(now),
          showValues: this.view.showValues,
          switching: pad.switching,
          recording: pad.recording !== undefined,
        });
      }
    }
  }

  /** While no pad is selected: the generic drawing, at rest, in its place. */
  private drawPlaceholder(): void {
    const slot = document.querySelector<HTMLElement>(".placeholder-art");
    if (slot && !slot.firstChild) slot.append(new ControllerDrawing(layoutFor(undefined), undefined).element);
  }

  /** Builds the live view of the selected pad when needed, and updates it. */
  private drawSelected(now: number, reads: Map<string, GamepadRead>): void {
    const key = this.view.selected;
    const pad = key ? this.pads.get(key) : undefined;
    const root = document.querySelector<HTMLElement>(".live");
    if (!pad || !root) {
      this.live = undefined;
      this.drawPlaceholder();
      return;
    }
    const run = pad.run;
    const status = run ? this.feedWizard(pad, run, now) : undefined;
    const wizardOpen = this.view.wizardOpen && run !== undefined;
    // jq79 renders the detail; the live view moves in once it exists, and
    // again if it was rendered anew, another pad was selected, or the
    // wizard opened or closed.
    if (
      this.live?.key !== pad.row.key ||
      this.live.root !== root ||
      !root.isConnected ||
      (this.live.view instanceof WizardView) !== wizardOpen
    ) {
      this.live = { key: pad.row.key, root, view: this.buildView(pad, root, wizardOpen) };
      // Which pad the view shows, next to the selection jq79 renders.
      root.dataset.view = pad.row.key;
    }
    const view = this.live.view;
    if (view instanceof WizardView && run && status) {
      view.update(status, () => run.wizard.results(), run.wizard.mappedState(), this.view.showValues);
      return;
    }
    const read = reads.get(pad.row.key);
    if (read && view instanceof LiveView) {
      // After a finished run, the drawing shows the mapping it identified.
      const state = run?.done ? run.wizard.mappedState() : read.state;
      view.update(read.snapshot, state, pad.rate.rate(now), this.view.showValues);
    }
  }

  /** The view of the selected pad: the guided wizard, or its live view. */
  private buildView(pad: Pad, root: HTMLElement, wizardOpen: boolean): LiveView | HidLiveView | WizardView {
    const run = pad.run;
    if (wizardOpen && run) {
      return new WizardView(root, layoutFor(pad.info.family), pad.info.face_layout, {
        skip: () => {
          this.log.event("wizard_skip", { pad: pad.row.key, step: run.logged });
          run.wizard.skip();
        },
        back: () => run.wizard.back(),
        restart: () => {
          this.log.event("wizard_restart", { pad: pad.row.key });
          run.wizard.restart();
        },
        close: this.closeWizard,
      });
    }
    if (pad.source === "gamepad") {
      const { buttons, axes } = pad.info.layout;
      const identified = run?.done ? rawNames(run.wizard.results(), buttons.length, axes.length) : undefined;
      return new LiveView(root, pad.info, identified);
    }
    return new HidLiveView(root, pad.info, {
      switchToFull: () => this.switchToFull(pad),
      record: () => this.record(pad),
    });
  }

  /**
   * Feeds a pad's wizard run with its latest values, and logs the steps it
   * completed or undid. Returns its status.
   */
  private feedWizard(pad: Pad, run: WizardRun, now: number): WizardStatus | undefined {
    try {
      if (pad.source === "gamepad") this.gamepads?.feedWizard(pad.id, run.wizard, now);
      else this.hids?.feedWizard(pad.id, run.wizard, now);
    } catch {
      // Unplugged since the last sample: its removal comes with the next one.
      return undefined;
    }
    const status = run.wizard.status();
    if (status.step > run.logged) {
      const results = run.wizard.results();
      for (const result of results.slice(run.logged, status.step)) {
        this.log.event("wizard_step", { pad: pad.row.key, ...result });
      }
    } else if (status.step < run.logged) {
      this.log.event("wizard_back", { pad: pad.row.key, step: status.step });
    }
    run.logged = status.step;
    const done = status.phase === "done";
    if (done && !run.done) {
      this.log.event("wizard_done", { pad: pad.row.key, missing: run.wizard.missing() });
    }
    run.done = done;
    return status;
  }
}
