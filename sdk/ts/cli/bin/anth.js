#!/usr/bin/env bun
// The npm facade for the Rust `anth` binary. Release installs put `anth` on
// PATH; `ANTH_BIN` overrides it and `cargo anth` is the fallback for a Rust
// toolchain without the release binary.
import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";

const args = process.argv.slice(2);
const candidates = [];
if (process.env.ANTH_BIN) candidates.push([process.env.ANTH_BIN, args]);
const self = realpathSync(import.meta.path);
const found = Bun.which("anth");
if (found && realpathSync(found) !== self) candidates.push(["anth", args]);
candidates.push(["cargo", ["anth", ...args]]);

let last;
for (const [command, argv] of candidates) {
  const result = spawnSync(command, argv, { stdio: "inherit" });
  if (result.error?.code === "ENOENT") {
    last = result.error;
    continue;
  }
  process.exit(result.status ?? 1);
}
console.error(
  "anth: no Anthracite CLI found; install it from the GitHub Release, put it on PATH, or set ANTH_BIN",
);
console.error(last?.message ?? "");
process.exit(1);
