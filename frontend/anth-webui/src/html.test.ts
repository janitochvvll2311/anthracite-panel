import { describe, expect, test } from "bun:test";

import { escape } from "./html";

describe("escape", () => {
  test("neutralizes the five characters that can end a template", () => {
    expect(escape(`<script>&"'\u0060`)).toBe(
      "&lt;script&gt;&amp;&quot;&#39;\u0060",
    );
  });

  test("escapes a single quote, which an earlier per-module copy did not", () => {
    // `fab.ts` used to escape `& < >` and `"` but not `'`. A label interpolated
    // into a single-quoted attribute would have broken out of it.
    expect(escape("it's")).toBe("it&#39;s");
  });

  test("leaves ordinary text untouched", () => {
    expect(escape("Debian GNU/Linux 13")).toBe("Debian GNU/Linux 13");
  });

  test("escapes an ampersand once, not twice", () => {
    expect(escape("a &amp; b")).toBe("a &amp;amp; b");
  });
});
