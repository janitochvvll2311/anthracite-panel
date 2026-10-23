import { afterEach, describe, expect, test } from "bun:test";

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
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("is registered as the tag the manifest names", () => {
    expect(customElements.get("anth-hello")).toBe(AnthHello);
  });

  test("asks its own anchor, not the panel's root", () => {
    const element = document.createElement("anth-hello") as AnthHello;
    expect(element.endpoints.url(ANCHOR)).toBe(
      `${location.origin}/api/plugins/org.anth.hello/greeting`,
    );
  });

  test("renders the greeting the daemon returns", async () => {
    const element = document.createElement("anth-hello") as AnthHello;
    const body = { message: "Hola desde turing" };

    // The widget's own transport is its endpoint client's; replacing `get` keeps
    // the request path real and the answer controlled.
    element.endpoints.get = (async () => body) as typeof element.endpoints.get;

    document.body.append(element);
    await element.handle.refresh();

    expect(element.handle.state).toBe("ready");
    expect(element.shadowRoot?.querySelector(".message")?.textContent).toBe(
      "Hola desde turing",
    );
  });

  test("shows a refused capability and offers a retry", async () => {
    const element = document.createElement("anth-hello") as AnthHello;
    element.endpoints.get = (async () => {
      throw new Error("the host refused `host.hostname`");
    }) as typeof element.endpoints.get;

    document.body.append(element);
    await element.handle.refresh();

    expect(element.handle.state).toBe("error");
    expect(element.shadowRoot?.textContent).toContain("the host refused");
    expect(element.shadowRoot?.querySelector(".anth-plugin-retry")).not.toBeNull();
  });
});
