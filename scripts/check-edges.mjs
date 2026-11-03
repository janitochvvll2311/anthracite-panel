#!/usr/bin/env bun
// The forbidden edges of §10.1, checked instead of documented.
//
// Three rules, by zone:
//
//   engine/*  may not depend on anything outside engine/
//   panel/*   may not depend on engine/* directly: it goes through the SDK
//   apps/*    may not depend on sdk/rust/panel: that crate is for panels
//
// plus "engine/contract depends on nothing we own", because it is the base
// everything else builds on. A workspace that does not exist yet is skipped, so
// the check grows with the repository instead of failing on day one.
//
// Run from anywhere: `bun scripts/check-edges.mjs`.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const WORKSPACES = [
  "engine/Cargo.toml",
  "sdk/rust/Cargo.toml",
  "panel/backend/Cargo.toml",
  "apps/Cargo.toml",
];

const violations = [];
const packages = [];
const seen = [];

for (const manifest of WORKSPACES) {
  const absolute = resolve(root, manifest);
  if (!existsSync(absolute)) continue;
  seen.push(manifest.split("/")[0]);

  const result = spawnSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--manifest-path",
      absolute,
    ],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    console.error(`cargo metadata failed for ${manifest}:`);
    console.error(result.stderr);
    process.exit(1);
  }
  for (const pkg of JSON.parse(result.stdout).packages) {
    packages.push({ ...pkg, manifest: resolve(pkg.manifest_path) });
  }
}

// A gate that passes when it found nothing to check is worse than no gate.
if (packages.length === 0) {
  console.error("no workspace packages found; the edge check has nothing to read");
  process.exit(1);
}

/** The top-level directory a path lives in: engine, sdk, panel, apps, … */
function zone(path) {
  return relative(root, path).split(sep)[0];
}

/** The path relative to the repository root, with forward slashes. */
function rel(path) {
  return relative(root, path).split(sep).join("/");
}

for (const pkg of packages) {
  const from = zone(pkg.manifest);
  const manifest = rel(pkg.manifest);

  for (const dep of pkg.dependencies) {
    if (!dep.path) continue;
    const target = resolve(dep.path);
    const to = zone(target);

    if (from === "engine" && to !== "engine") {
      violations.push(
        `${pkg.name} -> ${dep.name} (${rel(target)}): the engine must not depend outside engine/`,
      );
    }
    if (from === "panel" && to === "engine") {
      violations.push(
        `${pkg.name} -> ${dep.name} (${rel(target)}): a panel depends on the SDK, never on the engine directly`,
      );
    }
    if (
      from === "apps" &&
      (rel(target) === "sdk/rust/panel" ||
        rel(target).startsWith("sdk/rust/panel/"))
    ) {
      violations.push(
        `${pkg.name} -> ${dep.name} (${rel(target)}): an app must not depend on the panel SDK`,
      );
    }
    if (manifest === "engine/contract/Cargo.toml") {
      violations.push(
        `${pkg.name} -> ${dep.name} (${rel(target)}): the contract is the base and depends on nothing we own`,
      );
    }
  }
}

if (violations.length > 0) {
  console.error("forbidden edges:");
  for (const line of violations) console.error(`  ${line}`);
  process.exit(1);
}

console.log(
  `edges ok: ${packages.length} packages in ${seen.join(", ") || "no workspace"}; no forbidden path dependency`,
);
