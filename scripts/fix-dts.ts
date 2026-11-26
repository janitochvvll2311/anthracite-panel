// Rewrite relative specifiers in emitted declarations to explicit `.js`, so a
// consumer with `moduleResolution: nodenext` can resolve them.
//
// Usage: bun scripts/fix-dts.ts <dist-dir>
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: bun scripts/fix-dts.ts <dist-dir>");
  process.exit(2);
}

function fix(text: string): string {
  return text.replace(
    /(from\s+["'])(\.\/[^"']+)(["'])/g,
    (_match, before: string, specifier: string, after: string) => {
      if (specifier.endsWith(".js") || specifier.endsWith(".json")) return `${before}${specifier}${after}`;
      return `${before}${specifier}.js${after}`;
    },
  );
}

function walk(directory: string): void {
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) {
      walk(path);
      continue;
    }
    if (!name.endsWith(".d.ts")) continue;
    const text = readFileSync(path, "utf8");
    const fixed = fix(text);
    if (fixed !== text) writeFileSync(path, fixed);
  }
}

walk(dir);
