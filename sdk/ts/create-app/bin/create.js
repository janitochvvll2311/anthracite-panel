#!/usr/bin/env bun
// `bun create @anth-panel/app org.example.notes` delegates to `anth app new`.
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error("usage: bun create @anth-panel/app <org.example.notes> [--lang rust|bun]");
  process.exit(2);
}
const hasLang = args.some((arg) => arg === "--lang" || arg.startsWith("--lang="));
const argv = ["app", "new", ...args, ...(hasLang ? [] : ["--lang", "bun"])];
const bin = process.env.ANTH_BIN || "anth";
const result = spawnSync(bin, argv, { stdio: "inherit" });
if (result.error?.code === "ENOENT") {
  console.error(
    "anth: no Anthracite CLI found; install it from the GitHub Release, put it on PATH, or set ANTH_BIN",
  );
  process.exit(1);
}
process.exit(result.status ?? 1);
