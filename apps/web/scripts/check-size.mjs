// Bundle size budget of the built site (W1): gzip sizes per file type, summed
// over dist/. Raise a budget on purpose, with the reason in a RECORD file,
// never just to make CI pass.

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const KIB = 1024;
// Budgets per kind of file; each WebAssembly module has its own.
const BUDGETS = {
  // The tester's module: what every visit loads. 224 KiB while it held the
  // test mode (H4), 160 again since the test mode is a module of its own
  // (W9, RECORD/2026-11-01.w9-test-mode-wasm-module).
  ".wasm": 160 * KIB,
  // The test mode's module, loaded by the test mode's page only (W9): the
  // runner, catalog, plans and report format, and its own copy of the
  // standard library, serde and the model (about 160 KiB, measured).
  "testing.wasm": 176 * KIB,
  ".js": 80 * KIB,
  ".css": 16 * KIB,
};

/** The budget a file counts against. */
const kindOf = (file) => {
  const ext = extname(file);
  return ext === ".wasm" && /anygamepad_wasm_testing/.test(file) ? "testing.wasm" : ext;
};

const dist = fileURLToPath(new URL("../dist/", import.meta.url));

const files = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? files(join(dir, entry.name)) : [join(dir, entry.name)],
  );

let all;
try {
  all = files(dist);
} catch {
  console.error("dist/ is missing: run `npm run build` first.");
  process.exit(1);
}

const totals = Object.fromEntries(Object.keys(BUDGETS).map((ext) => [ext, 0]));
for (const file of all) {
  const ext = kindOf(file);
  if (!(ext in totals)) continue;
  const size = gzipSync(readFileSync(file), { level: 9 }).length;
  totals[ext] += size;
  console.log(`${(size / KIB).toFixed(1).padStart(8)} KiB  ${relative(dist, file)}`);
}

let failed = totals[".wasm"] === 0 || totals["testing.wasm"] === 0;
if (failed) console.error("A .wasm module is missing from dist/.");
console.log("\ngzip total vs budget:");
for (const [ext, budget] of Object.entries(BUDGETS)) {
  const over = totals[ext] > budget;
  failed ||= over;
  const line = `${ext.padEnd(13)} ${(totals[ext] / KIB).toFixed(1).padStart(7)} / ${budget / KIB} KiB`;
  console.log(over ? `${line}  OVER BUDGET` : line);
}
process.exit(failed ? 1 : 0);
