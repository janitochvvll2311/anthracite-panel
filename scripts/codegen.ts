#!/usr/bin/env bun
// WIT -> typed clients and the app's command table.
//
// The engine reads the route signature at runtime; this generator keeps the
// Rust and TypeScript clients from inventing a second mapping. It is deliberately
// small and deterministic: the output is checked in and CI runs `--check`.
//
// The phases live in `scripts/codegen/`: `wit` parses and maps types, `util`
// owns names and formatting, and `generate` assembles the files.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

import { generate } from "./codegen/generate";

const root = resolve(import.meta.dir, "..");
const args = process.argv.slice(2);
const check = args.includes("--check");
const appArg = args.find((arg) => !arg.startsWith("--")) ?? "apps/hello";
const appDir = resolve(root, appArg);
const catalog = process.env.ANTH_GUEST_WIT ?? join(root, "sdk/rust/guest/wit/deps");

const { files } = generate(appDir, catalog);
let stale = false;
for (const [relative, content] of Object.entries(files)) {
  const path = join(root, relative);
  if (check) {
    let actual = "";
    try {
      actual = readFileSync(path, "utf8");
    } catch {
      actual = "";
    }
    if (actual !== content) {
      console.error(`${relative} is stale; run \`bun scripts/codegen.ts ${appArg}\``);
      stale = true;
    }
  } else {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
}
if (stale) process.exit(1);
if (!check) console.log(`generated ${Object.keys(files).length} files for ${appArg}`);
