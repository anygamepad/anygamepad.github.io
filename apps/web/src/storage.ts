/**
 * IndexedDB storage for device reports (ADR 0007, task W6).
 * Database `anygamepad`, object store `reports`.
 */

export interface StoredReport {
  id: number;
  /** The compact JSON document of the report. */
  report: string;
  /** Timestamp when the report was saved in this browser. */
  savedAt: number;
  /** Whether the report was copied/shared to an issue. */
  shared: boolean;
  /** Controller product name or label from the first view. */
  name: string;
  /** Stable fingerprint of the controller identity. */
  fingerprint: string;
  /** Date the report was created (UTC YYYY-MM-DD). */
  created: string;
  /** Redactions made when the report was built. */
  redactions?: Array<{ field: string; finding: string; message: string }>;
}

const DB_NAME = "anygamepad";
const STORE_NAME = "reports";
const DB_VERSION = 1;

let dbPromise: Promise<IDBDatabase> | undefined;

/** Opens the `anygamepad` database, requesting persistent storage. */
export function openDb(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise;

  if (typeof navigator !== "undefined" && navigator.storage?.persist) {
    navigator.storage.persist().catch(() => {
      // Browsers may refuse or ignore; private windows and Safari may drop storage.
    });
  }

  dbPromise = new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: "id", autoIncrement: true });
      }
    };

    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

  return dbPromise;
}

/** Saves a device report JSON to IndexedDB. */
export async function saveReport(
  reportJson: string,
  redactions?: Array<{ field: string; finding: string; message: string }>,
): Promise<StoredReport> {
  const db = await openDb();
  const parsed = JSON.parse(reportJson);
  const firstView = parsed.views?.[0];
  const name =
    parsed.user?.product ||
    firstView?.identity?.product_name ||
    firstView?.identity?.web?.id ||
    "Game Controller";
  const fingerprint = firstView?.fingerprint || "";
  const created = parsed.created || new Date().toISOString().slice(0, 10);

  const entry: Omit<StoredReport, "id"> = {
    report: reportJson,
    savedAt: Date.now(),
    shared: false,
    name,
    fingerprint,
    created,
    redactions: redactions || [],
  };

  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.add(entry);

    req.onsuccess = () => {
      const id = req.result as number;
      resolve({ ...entry, id });
    };
    req.onerror = () => reject(req.error);
  });
}

/** Lists all saved reports from IndexedDB, newest first. */
export async function listReports(): Promise<StoredReport[]> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const req = store.getAll();

    req.onsuccess = () => {
      const reports = (req.result as StoredReport[]) || [];
      reports.sort((a, b) => b.savedAt - a.savedAt);
      resolve(reports);
    };
    req.onerror = () => reject(req.error);
  });
}

/** Fetches a single saved report by id. */
export async function getReport(id: number): Promise<StoredReport | undefined> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const req = store.get(id);

    req.onsuccess = () => resolve(req.result as StoredReport | undefined);
    req.onerror = () => reject(req.error);
  });
}

/** Deletes a saved report by id. */
export async function deleteReport(id: number): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.delete(id);

    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

/** Marks a report as copied/shared to GitHub. */
export async function markReportShared(id: number): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.get(id);

    req.onsuccess = () => {
      const report = req.result as StoredReport | undefined;
      if (!report) return resolve();
      report.shared = true;
      const updateReq = store.put(report);
      updateReq.onsuccess = () => resolve();
      updateReq.onerror = () => reject(updateReq.error);
    };
    req.onerror = () => reject(req.error);
  });
}
