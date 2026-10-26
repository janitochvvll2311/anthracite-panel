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
  test("mounts the gate and the shell, and no prompt", () => {
    const html = page("index.html");

    // The gate is what a visitor sees first: every data route answers `401`
    // until a session exists, so a page that mounted only the shell would show
    // an error card where the login form belongs.
    expect(html).toContain("<anth-auth-gate>");
    // The shell is created by the composition root once a session exists, so
    // the page itself must not carry one: a shell in the markup would fetch a
    // snapshot before the cookie and show an error card behind the form.
    expect(html).not.toContain("<anth-app");
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
