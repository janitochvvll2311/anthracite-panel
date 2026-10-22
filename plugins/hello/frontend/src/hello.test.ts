import { describe, expect, test } from "bun:test";

import { ANCHOR, AnthHello, describeFailure, escapeHtml } from "./hello";

describe("the anchor", () => {
  test("matches the route the manifest declares", () => {
    expect(ANCHOR).toBe("/greeting");
  });
});

describe("describeFailure", () => {
  test("surfaces the daemon error verbatim", async () => {
    const response = new Response(JSON.stringify({ error: "hostname is unreadable" }), {
      status: 500,
      headers: { "content-type": "application/json" },
    });
    expect(await describeFailure(response)).toBe("hostname is unreadable");
  });

  test("falls back to the status line for a non-JSON body", async () => {
    const response = new Response("<html>nope</html>", { status: 502, statusText: "Bad Gateway" });
    expect(await describeFailure(response)).toBe("502 Bad Gateway");
  });
});

describe("escapeHtml", () => {
  test("neutralizes markup in a host-supplied string", () => {
    expect(escapeHtml(`<img src=x onerror="alert(1)">`)).toBe(
      "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;",
    );
  });
});

describe("AnthHello", () => {
  test("is registered as the tag the manifest names", () => {
    expect(customElements.get("anth-hello")).toBe(AnthHello);
  });
});
