import { expect, test } from "bun:test";

import { run } from "./cli";
import { commands } from "./commands";
import { GreetingHttpClient } from "./http";
import type { Transport } from "./transport";

type Call = { method: string; path: string; body?: string };

function fake(reply: string, calls: Call[]): Transport {
  return async (method, path, body) => {
    calls.push({ method, path, body });
    return JSON.stringify(reply);
  };
}

test("the HTTP client builds the route and parses the result", async () => {
  const calls: Call[] = [];
  const client = new GreetingHttpClient(fake("hello, Ada", calls));
  expect(await client.greet("Ada")).toBe("hello, Ada");
  expect(calls[0]?.method).toBe("GET");
  expect(calls[0]?.path).toBe(
    "/api/apps/org.anthracite.hello/hello?name=Ada",
  );
  expect(calls[0]?.body).toBeUndefined();
});

test("the CLI client parses flags and uses the same transport", async () => {
  const calls: Call[] = [];
  expect(await run(fake("hello, Ada", calls), "greet", ["--name", "Ada"])).toBe(
    JSON.stringify("hello, Ada"),
  );
  expect(calls[0]?.path).toBe(
    "/api/apps/org.anthracite.hello/hello?name=Ada",
  );
});

test("the CLI client accepts inline flags", async () => {
  const calls: Call[] = [];
  await run(fake("hello, Ada", calls), "greet", ["--name=Ada"]);
  expect(calls[0]?.path).toContain("name=Ada");
});

test("the CLI client reports missing and unknown flags", async () => {
  await expect(run(fake("unused", []), "greet", [])).rejects.toThrow(
    "missing --name",
  );
  await expect(
    run(fake("unused", []), "greet", ["--nope", "x"]),
  ).rejects.toThrow("unknown flag");
});

test("the generated command table matches the manifest", () => {
  expect(commands).toHaveLength(1);
  expect(commands[0]?.verb).toBe("greet");
  expect(commands[0]?.method).toBe("GET");
  expect(commands[0]?.path).toBe("/hello");
  expect(commands[0]?.params[0]?.name).toBe("name");
  expect(commands[0]?.returns).toBe("string");
});
