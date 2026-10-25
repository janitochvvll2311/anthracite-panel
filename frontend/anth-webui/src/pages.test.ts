import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * The two HTML pages, read as text.
 *
 * Reading the files rather than rendering them is what makes the check mean
 * something: the failure this guards against is a tag going missing from a page
 * nobody mounts in a test, and a page that is never loaded cannot be asserted on
 * after the fact.
 */
function page(name: string): string {
  return readFileSync(join(import.meta.dir, "..", name), "utf8");
}

describe("the shipped page", () => {
  test("mounts the shell and nothing else", () => {
    const html = page("index.html");

    expect(html).toContain("<anth-app>");
    expect(html).not.toContain("<anth-repl");
  });
});

describe("the harness page", () => {
  test("mounts the browser prompt", () => {
    // The prompt has no other home: if this tag goes, `repl-element.ts` is
    // unreachable in development and the element is dead code.
    expect(page("harness.html")).toContain("<anth-repl");
  });
});
