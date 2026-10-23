import { describe, expect, test } from "bun:test";

import { AnchorClient, anchorError } from "./anchors";

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
  return new AnchorClient({ pluginId, origin: "http://panel.test", fetchImpl });
}

describe("AnchorClient.url", () => {
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
    expect(() => new AnchorClient({ pluginId: "" })).toThrow(/plugin id/);
  });
});

describe("AnchorClient requests", () => {
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
