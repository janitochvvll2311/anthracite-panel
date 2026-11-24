import { expect, test } from "bun:test";

import { applyBody, createSseParser } from "./api";
import type { ProxyConfig } from "./types";

test("apply is a keyed object, not a bare document", () => {
  const config: ProxyConfig = {
    version: 4,
    listeners: [],
    upstreams: [],
    hosts: [],
  };
  const body = applyBody(config, 4);
  expect(body["expected-version"]).toBe(4);
  expect(body.config).toBe(config);
  expect(Object.keys(body).sort()).toEqual(["config", "expected-version"]);
});

test("splits events that arrive in arbitrary chunks", () => {
  const events: string[] = [];
  const feed = createSseParser((data) => events.push(data));
  feed('data: {"status":200}\n');
  expect(events).toEqual([]);
  feed('\ndata: {"status":404}\n\n');
  expect(events).toEqual(['{"status":200}', '{"status":404}']);
});

test("joins multiple data lines of one frame", () => {
  const events: string[] = [];
  const feed = createSseParser((data) => events.push(data));
  feed('data: {"a":1,\ndata: "b":2}\n\n');
  expect(events).toEqual(['{"a":1,\n"b":2}']);
});
