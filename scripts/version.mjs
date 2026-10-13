#!/usr/bin/env node
// The project version, kept in one place per tool: the Cargo workspace
// (`[workspace.package]`, the internal `[workspace.dependencies]`, Cargo.lock)
// and the web app (package.json, package-lock.json). Used by the release
// workflow; needs no dependencies.
//
//   node scripts/version.mjs current                 print the version
//   node scripts/version.mjs next major|minor|patch  print the next version
//   node scripts/version.mjs set X.Y.Z               write X.Y.Z everywhere
//   node scripts/version.mjs check                   fail if the files disagree

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const path = (file) => root + file;
const read = (file) => readFileSync(path(file), "utf8");

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

const fail = (message) => {
  console.error(`version: ${message}`);
  process.exit(1);
};

/** Every place that holds the version, read and written the same way. */
const PLACES = [
  {
    file: "Cargo.toml",
    // `version = "…"` of [workspace.package], then the internal path
    // dependencies' requirements, which must match it exactly.
    pattern: /(\[workspace\.package\][^[]*?\nversion = ")([^"]+)(")|(\nanygamepad[\w-]* = \{ path = "[^"]+", version = ")([^"]+)(")/g,
  },
  {
    file: "Cargo.lock",
    // The workspace members (path packages: no `source` line follows).
    pattern: /(\nname = "anygamepad[\w-]*"\nversion = ")([^"]+)("\n(?!source))/g,
  },
  { file: "apps/web/package.json", json: (data) => [data] },
  { file: "apps/web/package-lock.json", json: (data) => [data, data.packages?.[""]] },
];

/** The versions each place holds, as [file, version] pairs. */
const found = () =>
  PLACES.flatMap(({ file, pattern, json }) => {
    const text = read(file);
    if (json) return json(JSON.parse(text)).map((entry) => [file, entry?.version]);
    return [...text.matchAll(pattern)].map((match) => [file, match[2] ?? match[5]]);
  });

const current = () => {
  const versions = found();
  const distinct = [...new Set(versions.map(([, version]) => version))];
  if (distinct.length !== 1) {
    fail(`files disagree:\n${versions.map(([file, v]) => `  ${file}: ${v}`).join("\n")}`);
  }
  if (!SEMVER.test(distinct[0])) fail(`not a version: ${distinct[0]}`);
  return distinct[0];
};

const next = (bump) => {
  const [major, minor, patch] = current().split(".").map(Number);
  switch (bump) {
    case "major":
      return `${major + 1}.0.0`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    default:
      return fail(`expected major, minor or patch, got ${bump}`);
  }
};

const set = (version) => {
  if (!SEMVER.test(version ?? "")) fail(`not a version: ${version}`);
  const before = found().length;
  current(); // the files must agree before they are rewritten
  for (const { file, pattern, json } of PLACES) {
    const text = read(file);
    let updated;
    if (json) {
      const data = JSON.parse(text);
      for (const entry of json(data)) entry.version = version;
      updated = `${JSON.stringify(data, null, 2)}\n`;
    } else {
      updated = text.replace(pattern, (...m) =>
        m[1] !== undefined ? `${m[1]}${version}${m[3]}` : `${m[4]}${version}${m[6]}`,
      );
    }
    writeFileSync(path(file), updated);
  }
  if (found().length !== before || current() !== version) fail("rewriting the version failed");
};

const summary = () => `${current()} (${found().length} places)`;

const [command, argument] = process.argv.slice(2);
switch (command) {
  case "current":
    console.log(current());
    break;
  case "next":
    console.log(next(argument));
    break;
  case "set":
    set(argument);
    console.log(summary());
    break;
  case "check":
    console.log(summary());
    break;
  default:
    fail("usage: version.mjs current | next major|minor|patch | set X.Y.Z | check");
}
