// The test mode (roadmap task H4, ADR 0008): the list of controllers (the
// lab's units and the catalog's models), the ways to connect a picked one,
// automatic detection of a pad connected without picking, and a session that
// runs a test plan on the pad with the library's test runner (WebAssembly),
// ending with an anonymous test report saved in this browser and downloaded.
//
// The tester (tester.ts) owns the devices and the frame loop: it tells the
// test mode about pads that come and go, and calls `frame()` once a frame.
// Logic lives here; App.html only renders `view.test`.
//
// The runner, catalog and plans are a WebAssembly module of their own (W9),
// loaded when the tab first opens (`loadTesting()`). The tester's module
// describes each pad (`testDevice()`), gives what it read once a frame
// (`testInput()`) and sends the outputs the session asks for (`send()`).

import type {
  Environment,
  Gamepads,
  GamepadState,
  HidDevices,
  OutputCommand,
  Transport,
} from "./wasm/anygamepad_wasm.js";
import type {
  CatalogModel,
  ChoiceResult,
  DetectedCandidate,
  Detection,
  PlanInfo,
  TestDevice,
  TestSelection,
  TestSession,
  TestStatus,
} from "./wasm-testing/anygamepad_wasm_testing.js";
import { listTestReports, saveTestReport, type StoredTestReport } from "./storage";

/** The test mode's WebAssembly module. */
export type TestingModule = typeof import("./wasm-testing/anygamepad_wasm_testing.js");

/** Loads and starts the test mode's module (once; later calls share it). */
let loading: Promise<TestingModule> | undefined;
export const loadTesting = (): Promise<TestingModule> => {
  loading ??= import("./wasm-testing/anygamepad_wasm_testing.js").then(async (module) => {
    await module.default();
    return module;
  });
  return loading;
};

/** A connected pad, as the tester gives it. */
export interface TestPad {
  /** `gamepad:<id>` or `webhid:<id>`. */
  key: string;
  source: "gamepad" | "webhid";
  id: number;
  name: string;
  /** Whether it can be read now (WebHID devices open asynchronously). */
  open: boolean;
  /** Its canonical state at the last frame, when known. */
  state: GamepadState | undefined;
}

/** What the test mode needs from the tester. */
export interface TestHost {
  gamepads(): Gamepads | undefined;
  hids(): HidDevices | undefined;
  environment(): Environment | null;
  /** The pads the page lists (a decoded WebHID pad hides its Gamepad API twin). */
  pads(): TestPad[];
}

type Stage = "loading" | "list" | "connect" | "waiting" | "check" | "detected" | "plan" | "running" | "summary";

export interface UnitRow {
  code: string;
  model: string;
  title: string;
  notes: string;
}

export interface ModelRow {
  id: string;
  title: string;
  generic: boolean;
  units: string;
  candidate: boolean;
}

export interface ConnectionCard {
  key: string;
  mode: string;
  modeName: string;
  transport: Transport;
  transportName: string;
  modeHowTo: string;
  howTo: string;
  known: boolean;
  tested: boolean;
}

export interface CandidateRow {
  index: number;
  title: string;
}

export interface ResultRow {
  step: string;
  status: string;
  detail: string;
}

export interface ReviewRow {
  field: string;
  text: string;
  flagged: boolean;
}

export interface TestView {
  stage: Stage;
  search: string;
  units: UnitRow[];
  models: ModelRow[];
  /** The model picked or detected. */
  model: { id: string; title: string; notes: string } | null;
  cards: ConnectionCard[];
  /** What the session will test, once known. */
  choice: { title: string; transportKnown: boolean } | null;
  /** Detection's candidates, most likely first, and the one shown. */
  candidates: CandidateRow[];
  candidate: number;
  /** Transports the person picks from when the API does not tell. */
  transports: { transport: Transport; name: string }[];
  /** Units of the model this browser knows (the inventory's, the saved
   * reports'), the one picked last first. */
  unitOptions: string[];
  /** The unit's code; `null` for none. */
  unit: string | null;
  /** The code a new unit of the model would get (`DS5-03`, F4). */
  unitProposal: string;
  /** Why the code typed cannot be one. */
  unitError: string | null;
  padName: string;
  check: string | null;
  plans: PlanInfo[];
  status: TestStatus | null;
  /** The prompt as `kind[:detail]` (`press:south`, `ask:rumble`), for tests. */
  promptKey: string;
  progress: number;
  results: ResultRow[];
  summary: string;
  review: ReviewRow[];
  fileName: string;
  notes: string;
  saved: string | null;
  error: string | null;
}

export const emptyTestView = (): TestView => ({
  stage: "loading",
  search: "",
  units: [],
  models: [],
  model: null,
  cards: [],
  choice: null,
  candidates: [],
  candidate: 0,
  transports: [],
  unitOptions: [],
  unitProposal: "",
  unitError: null,
  unit: null,
  padName: "",
  check: null,
  plans: [],
  status: null,
  promptKey: "",
  progress: 0,
  results: [],
  summary: "",
  review: [],
  fileName: "",
  notes: "",
  saved: null,
  error: null,
});

const TRANSPORT_NAMES: Record<Transport, string> = {
  unknown: "Unknown",
  usb: "USB cable",
  bluetooth: "Bluetooth",
  bluetooth_le: "Bluetooth LE",
  dongle: "Wireless receiver",
};

const STATUS_NAMES: Record<string, string> = {
  pass: "Pass",
  fail: "Fail",
  skipped: "Skipped",
  not_applicable: "Not applicable",
  error: "Error",
};

/** A prompt as `kind[:detail]`, for the page's tests. */
const promptKey = (status: TestStatus): string => {
  const prompt = status.prompt;
  switch (prompt.kind) {
    case "press":
      return `press:${prompt.button}`;
    case "circle":
    case "pull":
      return `${prompt.kind}:${prompt.side}`;
    case "watch":
    case "ask":
      return `${prompt.kind}:${prompt.output}`;
    default:
      return prompt.kind;
  }
};

/** A model's title: its vendor and name, the vendor once (`Nintendo Switch Pro Controller`). */
const titleOf = (model: CatalogModel): string =>
  model.name.startsWith(model.vendor) ? model.name : `${model.vendor} ${model.name}`;

/** localStorage key of the unit picked last for a model, on this machine. */
const lastUnitKey = (model: string): string => `anygamepad.test.last-unit.${model}`;

const readLastUnit = (model: string): string | null => {
  try {
    return localStorage.getItem(lastUnitKey(model));
  } catch {
    return null;
  }
};

/** The configuration a session will test. */
interface Plan {
  start: "list" | "connecting";
  confirmed: TestSelection;
  detected: TestSelection | undefined;
}

export class TestMode {
  private readonly models: CatalogModel[];
  private readonly unitList: UnitRow[];
  private reports: StoredTestReport[] = [];
  /** The pad the next session runs on. */
  private padKey: string | null = null;
  /** Detection of that pad, when it ran. */
  private detection: Detection | undefined;
  /** The mode and transport picked from the cards. */
  private picked: { mode: string; transport: Transport } | null = null;
  private plan: Plan | null = null;
  private session: TestSession | undefined;
  private sessionPad: { key: string; source: "gamepad" | "webhid"; id: number } | null = null;
  private planId = "";
  /** The file name this session's report was saved under, if it was. */
  private savedName: string | null = null;
  private lastStep = -1;
  /** Whether each pad had a button pressed at the last frame. */
  private readonly pressed = new Map<string, boolean>();
  /** In the detected stage: answers from the pad count once it was released. */
  private armed = false;

  constructor(
    private readonly testing: TestingModule,
    private readonly host: TestHost,
    private readonly view: TestView,
  ) {
    this.models = testing.catalogModels();
    const names = new Map(this.models.map((model) => [model.id, titleOf(model)]));
    this.unitList = testing.labUnits().map((unit) => ({
      code: unit.code,
      model: unit.model,
      title: `${unit.code} · ${names.get(unit.model) ?? unit.model}`,
      notes: unit.notes ?? "",
    }));
    view.plans = testing.testPlans();
    view.units = this.unitList;
    this.filter();
    view.stage = "list";
    listTestReports()
      .then((reports) => {
        this.reports = reports;
        this.refreshCards();
      })
      .catch(() => {
        // Storage unavailable: nothing shows as tested.
      });
  }

  /** Whether the test mode reads this pad itself (its session's pad). */
  owns(key: string): boolean {
    return this.session !== undefined && this.sessionPad?.key === key;
  }

  // Actions of App.html.

  readonly search = (text: string): void => {
    this.view.search = text;
    this.filter();
  };

  readonly pickModel = (id: string, unit?: string): void => {
    const model = this.model(id);
    if (!model) return;
    this.view.model = { id, title: titleOf(model), notes: model.notes ?? "" };
    this.offerUnits(id, unit);
    this.picked = null;
    this.view.error = null;
    this.refreshCards();
    this.view.stage = "connect";
  };

  readonly pickCard = (key: string): void => {
    const card = this.view.cards.find((c) => c.key === key);
    const model = this.view.model;
    if (!card || !model || this.view.unitError) return;
    this.picked = { mode: card.mode, transport: card.transport };
    // Picked after "Choose another": the pad is already connected.
    if (this.padKey && this.detection) {
      this.plan = {
        start: "connecting",
        confirmed: this.selection(model.id, card.mode, card.transport, this.view.unit),
        detected: this.best(this.detection),
      };
      this.toPlan();
      return;
    }
    this.view.choice = { title: `${model.title} · ${card.modeName} · ${card.transportName}`, transportKnown: true };
    this.view.stage = "waiting";
    // A pad already connected answers the prompt by a press.
  };

  readonly back = (): void => {
    this.reset();
  };

  readonly backToCards = (): void => {
    if (this.view.model) this.pickModel(this.view.model.id, this.view.unit ?? undefined);
  };

  readonly goOn = (): void => {
    if (this.plan) this.toPlan();
  };

  readonly pickCandidate = (index: number): void => {
    if (!this.detection?.candidates[index]) return;
    this.view.candidate = index;
    this.showCandidate(this.detection.candidates[index]);
  };

  readonly setTransport = (transport: Transport): void => {
    const candidate = this.detection?.candidates[this.view.candidate];
    if (!candidate) return;
    candidate.transport = transport;
    this.showCandidate(candidate);
  };

  readonly setUnit = (code: string): void => {
    const trimmed = code.trim();
    this.view.unit = trimmed === "" ? null : trimmed;
    this.view.unitError =
      trimmed === "" || this.testing.isUnitCode(trimmed)
        ? null
        : "A code is 1 to 16 letters, digits and hyphens (DS5-03), never a name.";
  };

  /** Names the pad as a new unit, with the proposed code. */
  readonly newUnit = (): void => {
    this.setUnit(this.view.unitProposal);
  };

  readonly confirm = (): void => {
    const candidate = this.detection?.candidates[this.view.candidate];
    if (this.view.stage !== "detected" || !candidate || candidate.transport === undefined) return;
    if (this.view.unitError) return;
    const confirmed = this.selection(candidate.model, candidate.mode, candidate.transport, this.view.unit);
    this.plan = { start: "connecting", confirmed, detected: this.best(this.detection) };
    this.toPlan();
  };

  readonly chooseAnother = (): void => {
    if (this.view.stage !== "detected") return;
    this.view.stage = "list";
    this.filter();
  };

  readonly start = (planId: string): void => {
    const pad = this.host.pads().find((p) => p.key === this.padKey);
    const plan = this.plan;
    if (!pad || !plan) {
      this.view.error = "The pad is not connected any more.";
      return;
    }
    const setup = {
      plan: planId,
      start: plan.start,
      confirmed: plan.confirmed,
      detected: plan.detected,
    };
    try {
      const device = this.testDevice(pad);
      if (device) this.session = this.testing.startTest(device, setup);
    } catch (error) {
      this.view.error = `Cannot test this pad here (${String(error)}).`;
      return;
    }
    if (!this.session) return;
    if (plan.confirmed.unit) {
      try {
        localStorage.setItem(lastUnitKey(plan.confirmed.model), plan.confirmed.unit);
      } catch {
        // Private windows may refuse: the unit is asked again next time.
      }
    }
    this.sessionPad = { key: pad.key, source: pad.source, id: pad.id };
    this.planId = planId;
    this.savedName = null;
    this.lastStep = -1;
    this.view.results = [];
    this.view.notes = "";
    this.view.saved = null;
    this.view.error = null;
    this.view.stage = "running";
  };

  readonly answer = (answer: "yes" | "no" | "not_sure"): void => {
    this.session?.answer(answer, performance.now());
  };

  readonly skip = (): void => {
    this.session?.skip(performance.now());
  };

  readonly notOnThisPad = (): void => {
    this.session?.notOnThisPad(performance.now());
  };

  readonly nothingHappens = (): void => {
    this.session?.nothingHappens(performance.now());
  };

  readonly repeat = (): void => {
    this.session?.repeat(performance.now());
  };

  /** Ends the session early: the report holds the steps that ended. */
  readonly stop = (): void => {
    if (this.view.stage === "running") this.finish();
  };

  readonly setNotes = (text: string): void => {
    this.view.notes = text;
    this.session?.setNotes(text);
    this.outcome();
  };

  readonly save = async (): Promise<void> => {
    const outcome = this.outcome();
    const plan = this.plan;
    if (!outcome || !plan) return;
    const report = outcome.report as {
      device?: { host?: { os?: string; browser?: { engine?: string } }; views?: { identity?: { backend?: string } }[] };
    };
    try {
      const saved = await saveTestReport({
        report: JSON.stringify(outcome.report),
        fileName: outcome.file_name,
        savedAt: Date.now(),
        model: plan.confirmed.model,
        mode: plan.confirmed.mode,
        transport: plan.confirmed.transport,
        unit: plan.confirmed.unit ?? null,
        os: report.device?.host?.os ?? "unknown",
        browser: report.device?.host?.browser?.engine ?? "unknown",
        api: report.device?.views?.[0]?.identity?.backend ?? "unknown",
        plan: this.planId,
        pass: outcome.summary.pass,
        fail: outcome.summary.fail,
      });
      this.reports.unshift(saved);
      this.savedName = saved.fileName;
      this.view.saved = `Saved in this browser as ${saved.fileName}.`;
    } catch (error) {
      this.view.saved = `This browser did not save the report (${String(error)}): download it instead.`;
    }
  };

  readonly download = (): void => {
    const outcome = this.outcome();
    if (!outcome) return;
    const blob = new Blob([`${JSON.stringify(outcome.report, null, 2)}\n`], { type: "application/json" });
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = outcome.file_name;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  };

  /** After a session: the same model's ways to connect, to test the next. */
  readonly next = (): void => {
    const model = this.view.model?.id;
    const unit = this.view.unit;
    this.endSession();
    this.reset();
    if (model) this.pickModel(model, unit ?? undefined);
  };

  // From the tester.

  /** A pad connected. */
  added(pad: TestPad, now: number): void {
    if (this.session && this.sessionPad === null) {
      this.reconnected(pad, now);
      return;
    }
    this.answeredBy(pad);
  }

  /** A pad went away. */
  removed(key: string, now: number): void {
    this.pressed.delete(key);
    if (this.session && this.sessionPad?.key === key) {
      this.session.removed(now);
      this.sessionPad = null;
      return;
    }
    if (this.padKey === key && this.view.stage !== "running" && this.view.stage !== "summary") {
      this.reset();
      this.view.error = "The pad went away.";
    }
  }

  /** Once a frame. */
  frame(now: number): void {
    if (this.session && this.view.stage === "running") {
      this.feed(now);
      return;
    }
    for (const pad of this.host.pads()) {
      const down = (pad.state?.buttons.length ?? 0) > 0;
      const was = this.pressed.get(pad.key) ?? true;
      this.pressed.set(pad.key, down);
      if (this.view.stage === "detected" && pad.key === this.padKey) {
        if (!down) this.armed = true;
        else if (!was && this.armed) {
          if (pad.state?.buttons.includes("south")) this.confirm();
          else if (pad.state?.buttons.includes("east")) this.chooseAnother();
        }
        continue;
      }
      if (down && !was) this.answeredBy(pad);
    }
  }

  // Internals.

  /** A pad connected or pressed while the page waits for one. */
  private answeredBy(pad: TestPad): void {
    const stage = this.view.stage;
    if (stage === "waiting") this.checkPad(pad);
    else if (stage === "list" && !this.padKey) this.detect(pad);
  }

  private detect(pad: TestPad): void {
    const detection = this.identify(pad);
    const best = detection?.candidates[0];
    if (!detection || !best) return;
    this.padKey = pad.key;
    this.detection = detection;
    this.view.padName = pad.name;
    this.view.candidates = detection.candidates.map((candidate, index) => ({
      index,
      title: this.describe(candidate.model, candidate.mode, candidate.transport),
    }));
    this.view.candidate = 0;
    this.armed = false;
    this.showCandidate(best);
    this.view.stage = "detected";
    // "Choose another" lists the candidates first.
    this.filter();
  }

  private checkPad(pad: TestPad): void {
    const model = this.view.model;
    const picked = this.picked;
    if (!model || !picked) return;
    const transport = picked.transport;
    let result: ChoiceResult | undefined;
    try {
      const device = this.testDevice(pad);
      result = device && this.testing.checkChoice(device, model.id, picked.mode, transport);
    } catch {
      return;
    }
    if (!result) return;
    this.padKey = pad.key;
    this.view.padName = pad.name;
    this.detection = this.identify(pad);
    this.plan = {
      start: "list",
      confirmed: this.selection(model.id, picked.mode, transport, this.view.unit),
      detected: this.best(this.detection),
    };
    if (result.result === "differs") {
      const seen = result.seen.candidates[0];
      this.view.check = seen
        ? `It looks like ${this.describe(seen.model, seen.mode, seen.transport)} instead.`
        : "It does not look like the controller picked.";
      this.view.stage = "check";
      return;
    }
    this.view.check = result.result === "not_known" ? "The catalog does not know how this connection looks yet: this report will show it." : null;
    this.toPlan();
  }

  private identify(pad: TestPad): Detection | undefined {
    try {
      const device = this.testDevice(pad);
      return device && this.testing.identify(device);
    } catch {
      return undefined;
    }
  }

  private showCandidate(candidate: DetectedCandidate): void {
    const model = this.model(candidate.model);
    const mode = model?.modes.find((m) => m.id === candidate.mode);
    this.view.model = model
      ? { id: model.id, title: titleOf(model), notes: model.notes ?? "" }
      : null;
    this.view.transports =
      mode?.connections.map((connection) => ({
        transport: connection.transport,
        name: TRANSPORT_NAMES[connection.transport],
      })) ?? [];
    this.offerUnits(candidate.model);
    this.view.choice = {
      title: this.describe(candidate.model, candidate.mode, candidate.transport),
      transportKnown: candidate.transport !== undefined,
    };
  }

  private toPlan(): void {
    const plan = this.plan;
    if (!plan) return;
    this.view.choice = {
      title: this.describe(plan.confirmed.model, plan.confirmed.mode, plan.confirmed.transport),
      transportKnown: true,
    };
    this.view.stage = "plan";
  }

  private feed(now: number): void {
    const session = this.session;
    if (!session) return;
    const pad = this.sessionPad && this.host.pads().find((p) => p.key === this.sessionPad?.key);
    try {
      if (pad && this.sessionPad?.source === "gamepad") {
        const input = this.host.gamepads()?.testInput(pad.id);
        if (input) session.feed(input, now);
      } else if (pad?.open) {
        const input = this.host.hids()?.testInput(pad.id);
        if (input) session.feed(input, now);
      }
    } catch {
      // Unplugged since the last sample: its removal comes with the next one.
    }
    this.sendCommands();
    const status = session.status();
    this.view.status = status;
    this.view.promptKey = promptKey(status);
    this.view.progress = status.total === 0 ? 100 : Math.round((status.step / status.total) * 100);
    if (status.step !== this.lastStep) {
      this.lastStep = status.step;
      this.view.results = this.resultRows();
    }
    if (status.prompt.kind === "done") this.finish();
  }

  /** Sends the outputs the session asks for to the pad under test. */
  private sendCommands(): void {
    const session = this.session;
    const pad = this.sessionPad;
    if (!session || !pad) return;
    for (const command of session.takeCommands() as OutputCommand[]) {
      // An output that fails shows in the person's answer.
      try {
        if (pad.source === "gamepad") this.host.gamepads()?.send(pad.id, command);
        else this.host.hids()?.send(pad.id, command).catch(() => undefined);
      } catch {
        // The pad lacks this output, or went away.
      }
    }
  }

  /** A pad as the test mode's module reads it; throws when it cannot be tested here. */
  private testDevice(pad: TestPad): TestDevice | undefined {
    return pad.source === "gamepad"
      ? this.host.gamepads()?.testDevice(pad.id, Date.now())
      : this.host.hids()?.testDevice(pad.id, Date.now());
  }

  private reconnected(pad: TestPad, now: number): void {
    const session = this.session;
    if (!session) return;
    try {
      const device = this.testDevice(pad);
      if (device) session.added(device, now);
    } catch {
      return;
    }
    this.sessionPad = { key: pad.key, source: pad.source, id: pad.id };
  }

  private finish(): void {
    // Stops the outputs a stopped step left on.
    this.sendCommands();
    this.view.results = this.resultRows();
    this.view.stage = "summary";
    this.outcome();
  }

  /** The report so far, with a file name no saved report uses. */
  private outcome(): ReturnType<TestSession["outcome"]> | undefined {
    const session = this.session;
    if (!session) return undefined;
    // Names other saved reports use; this session's own keeps its name.
    const names = new Set(this.reports.map((report) => report.fileName));
    if (this.savedName) names.delete(this.savedName);
    let outcome = session.outcome(1);
    for (let n = 2; names.has(outcome.file_name) && n < 1000; n += 1) outcome = session.outcome(n);
    const { pass, fail, skipped, not_applicable: notApplicable, error } = outcome.summary;
    this.view.summary = `${pass} passed, ${fail} failed, ${skipped} skipped, ${notApplicable} not applicable${error ? `, ${error} errors` : ""}.`;
    this.view.review = outcome.review.map((item) => ({
      field: item.field,
      text: item.text,
      // Absent values come as `null` from the JSON-compatible serializer.
      flagged: item.concern != null,
    }));
    this.view.fileName = outcome.file_name;
    return outcome;
  }

  private resultRows(): ResultRow[] {
    return (this.session?.results() ?? []).map((result) => {
      const parts = [];
      if (result.expected != null) parts.push(`expected ${result.expected}`);
      if (result.observed != null) parts.push(`observed ${result.observed}`);
      for (const [name, value] of Object.entries(result.measurements)) {
        parts.push(`${name} ${Math.round(Number(value) * 100) / 100}`);
      }
      return {
        step: result.step,
        status: STATUS_NAMES[result.status] ?? result.status,
        detail: parts.join(" · "),
      };
    });
  }

  private endSession(): void {
    this.session?.free();
    this.session = undefined;
    this.sessionPad = null;
  }

  private reset(): void {
    if (this.view.stage === "summary") this.endSession();
    // The stage first, so the template never renders a stage without its data.
    this.view.stage = "list";
    this.padKey = null;
    this.detection = undefined;
    this.picked = null;
    this.plan = null;
    this.view.model = null;
    this.view.cards = [];
    this.view.choice = null;
    this.view.candidates = [];
    this.view.check = null;
    this.view.status = null;
    this.view.error = null;
    this.view.stage = "list";
    this.filter();
  }

  private model(id: string): CatalogModel | undefined {
    return this.models.find((model) => model.id === id);
  }

  /** The units of a model this browser knows: the inventory's and those
   * of the saved reports, the one picked last on this machine first. */
  private unitsOf(model: string): string[] {
    const codes = [
      ...this.unitList.filter((unit) => unit.model === model).map((unit) => unit.code),
      ...this.reports.filter((report) => report.model === model).flatMap((report) => (report.unit ? [report.unit] : [])),
    ].filter((code, index, all) => all.indexOf(code) === index);
    const last = readLastUnit(model);
    if (last && codes.includes(last)) return [last, ...codes.filter((code) => code !== last)];
    return codes;
  }

  /** Offers a model's units: `unit`, else the one picked last, else a new
   * one with the proposed code (F4: units are named as they are tested). */
  private offerUnits(model: string, unit?: string): void {
    const used = [
      ...this.unitList.map((known) => known.code),
      ...this.reports.flatMap((report) => (report.unit ? [report.unit] : [])),
    ];
    this.view.unitOptions = this.unitsOf(model);
    try {
      this.view.unitProposal = this.testing.proposeUnitCode(model, used);
    } catch {
      this.view.unitProposal = "";
    }
    this.view.unitError = null;
    this.view.unit = unit ?? this.view.unitOptions[0] ?? (this.view.unitProposal || null);
  }

  private selection(model: string, mode: string, transport: Transport, unit: string | null): TestSelection {
    return { model, mode, transport, unit: unit ?? undefined };
  }

  /** Detection's best candidate as a selection, when it tells a transport. */
  private best(detection: Detection | undefined): TestSelection | undefined {
    const best = detection?.candidates[0];
    if (!best || best.transport === undefined) return undefined;
    return { model: best.model, mode: best.mode, transport: best.transport, unit: best.units[0] };
  }

  private describe(modelId: string, modeId: string, transport: Transport | undefined): string {
    const model = this.model(modelId);
    const mode = model?.modes.find((m) => m.id === modeId);
    const parts = [model ? titleOf(model) : modelId, mode?.name ?? modeId];
    parts.push(transport === undefined ? "connection to confirm" : TRANSPORT_NAMES[transport]);
    return parts.join(" · ");
  }

  /** The list for the search box; detection's candidates first. */
  private filter(): void {
    const text = this.view.search.trim().toLowerCase();
    const candidates = new Set(this.detection?.candidates.map((candidate) => candidate.model) ?? []);
    const rows = this.models
      .filter((model) => !text || `${model.vendor} ${model.name} ${model.id}`.toLowerCase().includes(text))
      .map((model) => ({
        id: model.id,
        title: titleOf(model),
        generic: model.generic,
        units: this.unitList
          .filter((unit) => unit.model === model.id)
          .map((unit) => unit.code)
          .join(", "),
        candidate: candidates.has(model.id),
      }));
    rows.sort((a, b) => Number(b.candidate) - Number(a.candidate));
    this.view.models = rows;
    this.view.units = this.unitList.filter(
      (unit) => !text || `${unit.title} ${unit.notes}`.toLowerCase().includes(text),
    );
  }

  /** The picked model's ways to connect, marked tested in this environment. */
  private refreshCards(): void {
    const model = this.view.model && this.model(this.view.model.id);
    if (!model) {
      this.view.cards = [];
      return;
    }
    const environment = this.host.environment();
    const tested = (mode: string, transport: string): boolean =>
      this.reports.some(
        (report) =>
          report.model === model.id &&
          report.mode === mode &&
          report.transport === transport &&
          report.os === environment?.os &&
          report.browser === environment?.browser.engine,
      );
    this.view.cards = model.modes.flatMap((mode) =>
      mode.connections.map((connection) => ({
        key: `${mode.id}/${connection.transport}`,
        mode: mode.id,
        modeName: mode.name,
        transport: connection.transport,
        transportName: TRANSPORT_NAMES[connection.transport],
        modeHowTo: mode.how_to ?? "",
        howTo: connection.how_to ?? "",
        known: connection.known,
        tested: tested(mode.id, connection.transport),
      })),
    );
  }
}
