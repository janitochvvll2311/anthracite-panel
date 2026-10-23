import { describe, expect, test } from "bun:test";

import { WireError, responseError, send } from "./exchange";

function stub(answer: Response, seen: RequestInit[] = []) {
  return async (_path: string, init: RequestInit) => {
    seen.push(init);
    return answer;
  };
}

describe("responseError", () => {
  test("uses the documented error field", async () => {
    const response = new Response(JSON.stringify({ error: "control is off" }), { status: 403 });
    expect(await responseError(response)).toBe("control is off");
  });

  test("falls back to the status line for a non-JSON body", async () => {
    const response = new Response("<html>bad gateway</html>", {
      status: 502,
      statusText: "Bad Gateway",
    });
    expect(await responseError(response)).toBe("502 Bad Gateway");
  });

  test("ignores an empty error field rather than reporting nothing", async () => {
    const response = new Response(JSON.stringify({ error: "" }), {
      status: 500,
      statusText: "Internal Server Error",
    });
    expect(await responseError(response)).toBe("500 Internal Server Error");
  });
});

describe("send", () => {
  test("returns the parsed reply", async () => {
    const reply = await send<{ hostname: string }>(
      {
        path: "/api/host-info",
        fetchImpl: stub(new Response(JSON.stringify({ hostname: "turing" }), { status: 200 })),
      },
    );
    expect(reply).toEqual({ hostname: "turing" });
  });

  /** A command that answers nothing is a success; parsing it would fail. */
  test("yields undefined for a 204", async () => {
    const reply = await send({
      path: "/api/firewall/flush",
      method: "POST",
      fetchImpl: stub(new Response(null, { status: 204 })),
    });
    expect(reply).toBeUndefined();
  });

  test("sends a body as JSON with the content type", async () => {
    const seen: RequestInit[] = [];
    await send({
      path: "/api/echo",
      method: "post",
      body: { message: "hi" },
      fetchImpl: stub(new Response("{}", { status: 200 }), seen),
    });
    expect(seen[0]?.method).toBe("POST");
    expect(seen[0]?.body).toBe(JSON.stringify({ message: "hi" }));
    expect((seen[0]?.headers as Record<string, string>)["content-type"]).toBe("application/json");
  });

  test("omits the content type and the body when there is none", async () => {
    const seen: RequestInit[] = [];
    await send({
      path: "/api/host-info",
      fetchImpl: stub(new Response("{}", { status: 200 }), seen),
    });
    expect(seen[0]?.body).toBeUndefined();
    expect((seen[0]?.headers as Record<string, string>)["content-type"]).toBeUndefined();
  });

  test("throws the daemon's message with the kind derived from the status", async () => {
    const attempt = send({
      path: "/api/files/mkdir",
      method: "POST",
      body: { path: "/x" },
      fetchImpl: stub(new Response(JSON.stringify({ error: "write is off" }), { status: 403 })),
    });

    await expect(attempt).rejects.toThrow("write is off");
    try {
      await attempt;
    } catch (error) {
      expect(error).toBeInstanceOf(WireError);
      expect((error as WireError).status).toBe(403);
      expect((error as WireError).kind).toBe("forbidden");
    }
  });
});
