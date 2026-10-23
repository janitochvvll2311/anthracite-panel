import { describe, expect, test } from "bun:test";

import { PluginEndpoints, anchorError } from "./endpoints";

/** A `fetch` that records calls and answers with a scripted response. */
function recorder(answer: () => Response) {
  const calls: { url: string; init: RequestInit | undefined }[] = [];
  const fetchImpl = (async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return answer();
  }) as unknown as typeof fetch;
  return { calls, fetchImpl };
}

function client(fetchImpl: typeof fetch, pluginId = "org.anth.hello") {
  return new PluginEndpoints({ pluginId, origin: "http://panel.test", fetchImpl });
}

describe("PluginEndpoints.url", () => {
  test("builds the path the host serves", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    const anchors = client(fetchImpl);
    expect(anchors.url("/greeting")).toBe("http://panel.test/api/plugins/org.anth.hello/greeting");
  });

  test("adds a leading slash rather than concatenating twice", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    expect(client(fetchImpl).url("greeting")).toBe(
      "http://panel.test/api/plugins/org.anth.hello/greeting",
    );
  });

  test("encodes the plugin id so a slash cannot escape the prefix", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    const anchors = client(fetchImpl, "org/evil");
    expect(anchors.url("/x")).toBe("http://panel.test/api/plugins/org%2Fevil/x");
  });

  test("puts query pairs on the URL", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    expect(client(fetchImpl).url("/list", { page: "2", q: "a b" })).toBe(
      "http://panel.test/api/plugins/org.anth.hello/list?page=2&q=a+b",
    );
  });

  test("refuses an absolute URL instead of prefixing it", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    const anchors = client(fetchImpl);
    expect(() => anchors.url("https://evil.test/x")).toThrow(/must be relative/);
    expect(() => anchors.url("//evil.test/x")).toThrow(/must be relative/);
  });

  test("refuses to exist without a plugin id", () => {
    expect(() => new PluginEndpoints({ pluginId: "" })).toThrow(/plugin id or alias/);
  });
});

describe("PluginEndpoints requests", () => {
  test("get parses JSON from the anchor", async () => {
    const { calls, fetchImpl } = recorder(
      () => new Response(JSON.stringify({ message: "hola" }), { status: 200 }),
    );
    const body = await client(fetchImpl).get<{ message: string }>("/greeting");

    expect(body.message).toBe("hola");
    expect(calls[0]?.url).toBe("http://panel.test/api/plugins/org.anth.hello/greeting");
    expect(calls[0]?.init?.credentials).toBe("same-origin");
  });

  test("send serializes the body and sets the content type", async () => {
    const { calls, fetchImpl } = recorder(() => new Response(JSON.stringify({ ok: true })));
    await client(fetchImpl).send("post", "/run", { job: "x" });

    expect(calls[0]?.init?.method).toBe("POST");
    expect(calls[0]?.init?.body).toBe(JSON.stringify({ job: "x" }));
    expect((calls[0]?.init?.headers as Record<string, string>)["content-type"]).toBe(
      "application/json",
    );
  });

  test("send without a body sends none, so the host sees no body at all", async () => {
    const { calls, fetchImpl } = recorder(() => new Response(JSON.stringify({})));
    await client(fetchImpl).send("POST", "/ping");

    expect(calls[0]?.init?.body).toBeUndefined();
  });

  test("a failed anchor throws the daemon's own detail", async () => {
    const { fetchImpl } = recorder(
      () => new Response(JSON.stringify({ error: "the host refused" }), { status: 403 }),
    );
    await expect(client(fetchImpl).get("/x")).rejects.toThrow("the host refused");
  });

  test("204 answers without trying to parse a body", async () => {
    const { fetchImpl } = recorder(() => new Response(null, { status: 204 }));
    await expect(client(fetchImpl).get("/x")).resolves.toBeUndefined();
  });
});

describe("anchorError", () => {
  test("uses the documented error field", async () => {
    const response = new Response(JSON.stringify({ error: "nope" }), { status: 400 });
    expect(await anchorError(response)).toBe("nope");
  });

  test("falls back to the status line for a non-JSON body", async () => {
    const response = new Response("<html>bad gateway</html>", {
      status: 502,
      statusText: "Bad Gateway",
    });
    expect(await anchorError(response)).toBe("502 Bad Gateway");
  });
});

describe("an alias as the plugin id", () => {
  test("is used verbatim, because the daemon resolves it", () => {
    const { fetchImpl } = recorder(() => new Response("{}"));
    const endpoints = new PluginEndpoints({
      pluginId: "hello",
      origin: "http://panel.test",
      fetchImpl,
    });

    expect(endpoints.url("/greeting")).toBe("http://panel.test/api/plugins/hello/greeting");
  });
});

describe("configured headers", () => {
  test("are sent on every request", async () => {
    const { calls, fetchImpl } = recorder(() => new Response("{}"));
    const endpoints = new PluginEndpoints({
      pluginId: "org.anth.hello",
      origin: "http://panel.test",
      headers: { "x-anth-trace": "abc" },
      fetchImpl,
    });

    await endpoints.get("/greeting");

    const headers = calls[0]?.init?.headers as Record<string, string>;
    expect(headers["x-anth-trace"]).toBe("abc");
    expect(headers.accept).toBe("application/json");
  });

  test("a per-call header wins over the configured one", async () => {
    const { calls, fetchImpl } = recorder(() => new Response("{}"));
    const endpoints = new PluginEndpoints({
      pluginId: "org.anth.hello",
      origin: "http://panel.test",
      headers: { accept: "text/plain" },
      fetchImpl,
    });

    await endpoints.get("/greeting", undefined, { headers: { accept: "application/json" } });

    expect((calls[0]?.init?.headers as Record<string, string>).accept).toBe(
      "application/json",
    );
  });

  test("the content type is set even when headers are configured", async () => {
    const { calls, fetchImpl } = recorder(() => new Response("{}"));
    const endpoints = new PluginEndpoints({
      pluginId: "org.anth.hello",
      origin: "http://panel.test",
      headers: { "x-anth-trace": "abc" },
      fetchImpl,
    });

    await endpoints.send("POST", "/run", { job: "x" });

    expect((calls[0]?.init?.headers as Record<string, string>)["content-type"]).toBe(
      "application/json",
    );
  });
});

describe("delete", () => {
  test("sends DELETE with no body", async () => {
    const { calls, fetchImpl } = recorder(() => new Response(JSON.stringify({ ok: true })));
    await client(fetchImpl).delete("/item/7");

    expect(calls[0]?.init?.method).toBe("DELETE");
    expect(calls[0]?.init?.body).toBeUndefined();
    expect(calls[0]?.url).toBe("http://panel.test/api/plugins/org.anth.hello/item/7");
  });
});

describe("an aborted request", () => {
  test("passes the signal through, so a torn-down widget can cancel", async () => {
    const { calls, fetchImpl } = recorder(() => new Response("{}"));
    const controller = new AbortController();
    await client(fetchImpl).get("/greeting", undefined, { signal: controller.signal });

    expect(calls[0]?.init?.signal).toBe(controller.signal);
  });
});

describe("forPlugin", () => {
  test("reuses the origin and headers for another plugin", async () => {
    const { calls, fetchImpl } = recorder(() => new Response("{}"));
    const endpoints = new PluginEndpoints({
      pluginId: "org.anth.one",
      origin: "http://panel.test",
      headers: { "x-anth-trace": "abc" },
      fetchImpl,
    });

    await endpoints.forPlugin("org.anth.two").get("/greeting");

    expect(calls[0]?.url).toBe("http://panel.test/api/plugins/org.anth.two/greeting");
    expect((calls[0]?.init?.headers as Record<string, string>)["x-anth-trace"]).toBe("abc");
  });
});
