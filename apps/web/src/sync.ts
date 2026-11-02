// Sending to the anygamepad database (owner request of 2026-11-02; roadmap
// P6, the server is P2 in apps/server, ADR 0009): once the person turns it
// on in the settings, the activity log goes out in parts as it grows, and
// each test report when it is saved. Off by default, remembered in this
// browser only, and inert while the build names no database (vite.config.ts,
// ANYGAMEPAD_SYNC_URL).
//
// The API (apps/server/src/lib.rs):
//
// - `POST <endpoint>v1/activity-logs`: one part of a page's activity log, as
//   JSON (`anygamepad-activity-log-v1`): the page, the environment, a random
//   ID of this page load joining its parts, the part's number, the day
//   (UTC), and the events since the previous part. No start time: events
//   are timed from the page load.
// - `POST <endpoint>v1/test-reports`: one test report (H1), as the file the
//   test mode downloads. Answers `{"id": n}`.
// - `DELETE <endpoint>v1/activity-logs/<session>`, `DELETE
//   <endpoint>v1/test-reports/<id>`: withdraws what was sent.
//
// Every request carries `Authorization: Bearer <token>`, a random token
// made here: one per page load's log, one per test report. The server keeps
// only its hash; this browser keeps the tokens (`anygamepad.sync.sent`) so
// the person can withdraw everything it sent. No cookies, no referrer. A
// part that fails for a passing reason (network, 429, 5xx) is sent again
// with the next one; one the server refuses is dropped.

import { LOG_FORMAT, type ActivityLog } from "./activity";
import type { Environment } from "./wasm/anygamepad_wasm.js";

const STORAGE_KEY = "anygamepad.sync";
const SENT_KEY = "anygamepad.sync.sent";

/** Time between two parts of the activity log. */
export const PART_INTERVAL_MS = 30_000;

/** Events in one part at most. */
export const MAX_PART_EVENTS = 2_000;

/** The largest body sent while the page goes away: `keepalive` requests
 * share 64 KiB. */
const KEEPALIVE_BYTES = 60_000;

/** Uploads this browser remembers, to withdraw them; the oldest go first. */
const MAX_KEPT = 1_000;

/** The settings dialog. */
export interface SettingsView {
  open: boolean;
  /** Whether this build names a database to send to. */
  available: boolean;
  /** Whether the person turned sending on. */
  sending: boolean;
  /** What this page sent. */
  sent: { logs: number; tests: number };
  /** Uploads from this browser that can be withdrawn. */
  kept: number;
  /** The outcome of the last withdrawal. */
  withdrawn: string | null;
  error: string | null;
}

export const emptySettings = (): SettingsView => ({
  open: false,
  available: false,
  sending: false,
  sent: { logs: 0, tests: 0 },
  kept: 0,
  withdrawn: null,
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

/** Something this browser sent, and the token that withdraws it. */
interface Upload {
  kind: "activity-logs" | "test-reports";
  /** The log's session, or the test report's ID. */
  id: string;
  token: string;
}

// Storage failures (private mode, blocked storage) are ignored: the choice
// and the tokens then last until the page closes.
const read = (key: string): string | null => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};

const write = (key: string, value: string | null): void => {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // See above.
  }
};

const readUploads = (): Upload[] => {
  try {
    const list: unknown = JSON.parse(read(SENT_KEY) ?? "[]");
    return Array.isArray(list) ? (list as Upload[]) : [];
  } catch {
    return [];
  }
};

/** A random token or ID: 32 hexadecimal digits. (`randomUUID` needs a
 * secure context, which a page served over plain HTTP on a local network,
 * development on a phone, lacks.) */
const randomId = (): string =>
  Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");

/** The UTC day, as reports carry it. */
const today = (): string => new Date().toISOString().slice(0, 10);

/** A request that failed; `retry` when it may work later. */
class SendError extends Error {
  constructor(
    message: string,
    readonly retry: boolean,
  ) {
    super(message);
  }
}

/** What every part of the activity log says about the page. */
export interface SyncContext {
  library: string;
  page: "tester" | "test";
  environment(): Environment | null;
}

export class Sync {
  /** Joins the parts of this page load's log (a new one after a
   * withdrawal). */
  private session = randomId();
  /** Every part of that log carries it. */
  private token = randomId();
  private part = 0;
  private timer: number | undefined;
  /** Whether a part is on its way. */
  private busy = false;
  private uploads = readUploads();

  constructor(
    private readonly endpoint: URL | null,
    private readonly log: ActivityLog,
    private readonly context: SyncContext,
    private readonly view: SettingsView,
  ) {
    view.available = endpoint !== null;
    view.sending = view.available && read(STORAGE_KEY) === "on";
    view.kept = this.uploads.length;
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
    write(STORAGE_KEY, on ? "on" : null);
    this.log.event("sending", { on });
    if (on) this.start();
    else window.clearInterval(this.timer);
  }

  /** Sends a test report the person saved. Resolves with whether it was
   * sent (false while sending is off); rejects when sending fails. */
  async sendTestReport(report: unknown): Promise<boolean> {
    if (!this.view.sending) return false;
    const token = randomId();
    const answer = (await this.request("POST", "v1/test-reports", token, JSON.stringify(report), false)) as {
      id?: unknown;
    };
    if (typeof answer?.id === "number") this.keep({ kind: "test-reports", id: String(answer.id), token });
    this.view.sent.tests += 1;
    this.log.event("test_report_sent");
    return true;
  }

  /** Withdraws everything this browser sent and remembers. */
  async withdrawAll(): Promise<void> {
    if (!this.endpoint) return;
    this.view.withdrawn = null;
    this.view.error = null;
    const pending = [...this.uploads];
    const failed: Upload[] = [];
    let deleted = 0;
    // What the page sends from now on is a new log, withdrawable apart.
    this.session = randomId();
    this.token = randomId();
    this.part = 0;
    for (const upload of pending) {
      try {
        await this.request("DELETE", `v1/${upload.kind}/${upload.id}`, upload.token, undefined, false);
        deleted += 1;
      } catch (error) {
        // 404: gone already (deleted, or past its retention).
        if (error instanceof SendError && error.message.startsWith("HTTP 404")) deleted += 1;
        else failed.push(upload);
      }
    }
    // Kept: what failed, and what was sent meanwhile.
    this.uploads = [...failed, ...this.uploads.filter((upload) => !pending.includes(upload))];
    this.saveUploads();
    this.log.event("sent_withdrawn", { deleted, failed: failed.length });
    this.view.withdrawn = `Deleted from the database: ${deleted} of ${deleted + failed.length}.`;
    if (failed.length > 0) this.view.error = "Some could not be deleted now: try again later.";
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
        // A withdrawal may start a new log while this part is on its way.
        const { session, token, part: number } = this;
        const body = JSON.stringify({
          format: LOG_FORMAT,
          library: this.context.library,
          page: this.context.page,
          environment: this.context.environment(),
          session,
          part: number,
          day: today(),
          dropped_input_events: part.dropped,
          events: part.events,
        });
        try {
          await this.request("POST", "v1/activity-logs", token, body, leaving && body.length <= KEEPALIVE_BYTES);
          if (number === 0) this.keep({ kind: "activity-logs", id: session, token });
          this.view.sent.logs += 1;
          this.view.error = null;
        } catch (error) {
          if (!(error instanceof SendError) || error.retry) throw error;
          // Refused: sending it again would not help.
          this.view.error = `A part of the activity log was refused (${error.message}).`;
        }
        this.log.markSent(part.next);
        if (session === this.session) this.part += 1;
        if (leaving) break;
      }
    } catch (error) {
      this.view.error = `The activity log was not sent (${String(error)}). The page tries again in a while.`;
    } finally {
      this.busy = false;
    }
  }

  private keep(upload: Upload): void {
    if (this.uploads.some((kept) => kept.kind === upload.kind && kept.id === upload.id)) return;
    this.uploads.push(upload);
    this.uploads.splice(0, this.uploads.length - MAX_KEPT);
    this.saveUploads();
  }

  private saveUploads(): void {
    write(SENT_KEY, this.uploads.length > 0 ? JSON.stringify(this.uploads) : null);
    this.view.kept = this.uploads.length;
  }

  /** Sends a request; resolves with the JSON answer, if any. */
  private async request(
    method: "POST" | "DELETE",
    path: string,
    token: string,
    body: string | undefined,
    keepalive: boolean,
  ): Promise<unknown> {
    if (!this.endpoint) throw new SendError("no database", false);
    let response: Response;
    try {
      response = await fetch(new URL(path, this.endpoint), {
        method,
        headers: { Authorization: `Bearer ${token}`, ...(body ? { "Content-Type": "application/json" } : {}) },
        body,
        keepalive,
        credentials: "omit",
        referrerPolicy: "no-referrer",
      });
    } catch (error) {
      throw new SendError(String(error), true);
    }
    const answer: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const reason = (answer as { error?: unknown } | null)?.error;
      const status = response.status;
      throw new SendError(
        `HTTP ${status}${typeof reason === "string" ? `: ${reason}` : ""}`,
        status === 408 || status === 429 || status >= 500,
      );
    }
    return answer;
  }
}
