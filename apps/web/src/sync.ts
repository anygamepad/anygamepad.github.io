// Sending to the anygamepad database (owner request of 2026-11-02, ahead of
// roadmap P6): once the person turns it on in the settings, the activity log
// goes out in parts as it grows, and each test report when it is saved. Off
// by default, remembered in this browser only, and inert while the build
// names no database (vite.config.ts, ANYGAMEPAD_SYNC_URL): until the server
// runs (P2), nothing leaves the browser.
//
// The API the server implements:
//
// - `POST <endpoint>v1/activity-logs`: one part of a page's activity log, as
//   JSON (`anygamepad-activity-log-v1`): the page, the environment, a random
//   ID of this page load joining its parts, the part's number, the day
//   (UTC), and the events since the previous part. No start time: events
//   are timed from the page load.
// - `POST <endpoint>v1/test-reports`: one test report (H1), as the file the
//   test mode downloads.
//
// Any 2xx answer means stored. No cookies, no referrer; a part that fails
// is sent again with the next one.

import { LOG_FORMAT, type ActivityLog } from "./activity";
import type { Environment } from "./wasm/anygamepad_wasm.js";

const STORAGE_KEY = "anygamepad.sync";

/** Time between two parts of the activity log. */
export const PART_INTERVAL_MS = 30_000;

/** Events in one part at most. */
export const MAX_PART_EVENTS = 2_000;

/** The largest body sent while the page goes away: `keepalive` requests
 * share 64 KiB. */
const KEEPALIVE_BYTES = 60_000;

/** The settings dialog. */
export interface SettingsView {
  open: boolean;
  /** Whether this build names a database to send to. */
  available: boolean;
  /** Whether the person turned sending on. */
  sending: boolean;
  /** What this page sent. */
  sent: { logs: number; tests: number };
  error: string | null;
}

export const emptySettings = (): SettingsView => ({
  open: false,
  available: false,
  sending: false,
  sent: { logs: 0, tests: 0 },
  error: null,
});

/** The database this build sends to, from the page's `<meta>` tag. */
export const syncEndpoint = (): URL | null => {
  const content = document.querySelector<HTMLMetaElement>('meta[name="anygamepad-sync"]')?.content;
  if (!content) return null;
  try {
    return new URL(content, location.href);
  } catch {
    return null;
  }
};

const storedSending = (): boolean => {
  try {
    return localStorage.getItem(STORAGE_KEY) === "on";
  } catch {
    return false;
  }
};

const storeSending = (on: boolean): void => {
  try {
    if (on) localStorage.setItem(STORAGE_KEY, "on");
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Private mode or blocked storage: the choice lasts until the page closes.
  }
};

/** A random ID: `randomUUID` needs a secure context, a page served over
 * plain HTTP on a local network (development on a phone) has none. */
const randomId = (): string =>
  Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");

/** The UTC day, as reports carry it. */
const today = (): string => new Date().toISOString().slice(0, 10);

/** What every part of the activity log says about the page. */
export interface SyncContext {
  library: string;
  page: "tester" | "test";
  environment(): Environment | null;
}

export class Sync {
  /** Joins the parts of this page load's log; never stored. */
  private readonly session = randomId();
  private part = 0;
  private timer: number | undefined;
  /** Whether a part is on its way. */
  private busy = false;

  constructor(
    private readonly endpoint: URL | null,
    private readonly log: ActivityLog,
    private readonly context: SyncContext,
    private readonly view: SettingsView,
  ) {
    view.available = endpoint !== null;
    view.sending = view.available && storedSending();
    // Once the page has logged its start.
    if (view.sending) window.setTimeout(() => this.start(), 0);
    // The last part, while the page can still send it.
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") void this.flush(true);
    });
    window.addEventListener("pagehide", () => void this.flush(true));
  }

  /** Turns sending on or off: the person's choice, from the settings. */
  setSending(on: boolean): void {
    if (!this.view.available || on === this.view.sending) return;
    this.view.sending = on;
    this.view.error = null;
    storeSending(on);
    this.log.event("sending", { on });
    if (on) this.start();
    else window.clearInterval(this.timer);
  }

  /** Sends a test report the person saved. Resolves with whether it was
   * sent (false while sending is off); rejects when sending fails. */
  async sendTestReport(report: unknown): Promise<boolean> {
    if (!this.view.sending) return false;
    await this.post("v1/test-reports", JSON.stringify(report), false);
    this.view.sent.tests += 1;
    this.log.event("test_report_sent");
    return true;
  }

  /** Sends the log from the page load on, then a part now and then. */
  private start(): void {
    window.clearInterval(this.timer);
    this.timer = window.setInterval(() => void this.flush(false), PART_INTERVAL_MS);
    void this.flush(false);
  }

  /** Sends the events not sent yet, in parts; one part when `leaving`. */
  private async flush(leaving: boolean): Promise<void> {
    if (!this.view.sending || this.busy) return;
    this.busy = true;
    try {
      for (;;) {
        const part = this.log.unsent(MAX_PART_EVENTS);
        if (part.events.length === 0) break;
        const body = JSON.stringify({
          format: LOG_FORMAT,
          library: this.context.library,
          page: this.context.page,
          environment: this.context.environment(),
          session: this.session,
          part: this.part,
          day: today(),
          dropped_input_events: part.dropped,
          events: part.events,
        });
        await this.post("v1/activity-logs", body, leaving && body.length <= KEEPALIVE_BYTES);
        this.log.markSent(part.next);
        this.part += 1;
        this.view.sent.logs += 1;
        this.view.error = null;
        if (leaving) break;
      }
    } catch (error) {
      this.view.error = `The activity log was not sent (${String(error)}). The page tries again in a while.`;
    } finally {
      this.busy = false;
    }
  }

  private async post(path: string, body: string, keepalive: boolean): Promise<void> {
    if (!this.endpoint) throw new Error("no database");
    const response = await fetch(new URL(path, this.endpoint), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body,
      keepalive,
      credentials: "omit",
      referrerPolicy: "no-referrer",
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
  }
}
