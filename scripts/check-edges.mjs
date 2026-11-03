#!/usr/bin/env bun
// The forbidden edges of §10.1, checked instead of documented.
//
// The engine is the platform: it may depend on crates from crates.io, never on
// the SDK, the panel or an app. Those live in other workspaces, so the rule is
// "no path dependency leaves engine/", plus "the contract depends on nothing we
// own" — it is the base everything else builds on.
//
// Run from anywhere: `bun scripts/check-edges.mjs`.

import { spawnSync } from "node:child_process";
import { dirname, relative, resolve } from "node:path";

const root = resolve(dirname(new URL(import.meta.url).pathname), "..");
const engine = resolve(root, "engine");

const cargo = spawnSync(
  "cargo",
  [
    "metadata",
    "--format-version",
    "1",
    "--no-deps",
    "--manifest-path",
    `${engine}/Cargo.toml`,
  ],
  { encoding: "utf8" },
);
if (cargo.status !== 0) {
  console.error("cargo metadata failed:");
  console.error(cargo.stderr);
  process.exit(1);
}

const metadata = JSON.parse(cargo.stdout);
const packages = metadata.packages;
const names = new Set(packages.map((pkg) => pkg.name));
const violations = [];

for (const pkg of packages) {
  const manifest = resolve(pkg.manifest_path);
  const rel = relative(engine, manifest);
  if (rel.startsWith("..")) {
    violations.push(`${pkg.name}: lives outside engine/ (${manifest})`);
  }

  for (const dep of pkg.dependencies) {
    if (!dep.path) continue;
    const depDir = resolve(dep.path);
    const outside = relative(engine, depDir).startsWith("..");
    if (outside) {
      violations.push(
        `${pkg.name} -> ${dep.name} (${depDir}): the engine must not depend outside its workspace`,
      );
    }
    if (pkg.name === "anth-contract") {
      violations.push(
        `${pkg.name} -> ${dep.name}: the contract is the base and depends on nothing we own`,
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
  `edges ok: ${packages.length} workspace packages (${[...names].sort().join(", ")}), no path dependency leaves engine/`,
);
