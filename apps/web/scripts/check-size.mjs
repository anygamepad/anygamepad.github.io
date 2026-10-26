// Bundle size budget of the built site (W1): gzip sizes per file type, summed
// over dist/. Raise a budget on purpose, with the reason in a RECORD file,
// never just to make CI pass.

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const KIB = 1024;
const BUDGETS = {
  ".wasm": 160 * KIB,
  ".js": 80 * KIB,
  ".css": 16 * KIB,
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
  const ext = extname(file);
  if (!(ext in totals)) continue;
  const size = gzipSync(readFileSync(file), { level: 9 }).length;
  totals[ext] += size;
  console.log(`${(size / KIB).toFixed(1).padStart(8)} KiB  ${relative(dist, file)}`);
}

let failed = totals[".wasm"] === 0;
if (failed) console.error("No .wasm file in dist/.");
console.log("\ngzip total vs budget:");
for (const [ext, budget] of Object.entries(BUDGETS)) {
  const over = totals[ext] > budget;
  failed ||= over;
  const line = `${ext.padEnd(6)} ${(totals[ext] / KIB).toFixed(1).padStart(7)} / ${budget / KIB} KiB`;
  console.log(over ? `${line}  OVER BUDGET` : line);
}
process.exit(failed ? 1 : 0);
