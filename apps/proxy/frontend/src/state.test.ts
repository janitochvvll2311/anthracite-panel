import { expect, test } from "bun:test";

import { upsertHost, upsertUpstream } from "./state";
import type { ProxyHost, ProxyUpstream } from "./types";

function host(id: string, routes: ProxyHost["routes"] = []): ProxyHost {
  return {
    id,
    names: [`${id}.test`],
    certificate: null,
    acme: false,
    "redirect-https": false,
    hsts: false,
    routes,
  };
}

const route = {
  id: "root",
  match: "/",
  upstream: "app",
  "strip-prefix": false,
  websocket: false,
  redirect: null,
  headers: [],
  "body-limit": null as number | null,
};

test("an add appends a new host", () => {
  const result = upsertHost([host("a")], host("b"), false);
  expect(result.error).toBeNull();
  expect(result.items.map((entry) => entry.id)).toEqual(["a", "b"]);
});

test("an add with a taken id is refused and keeps its routes", () => {
  const existing = host("a", [route]);
  const result = upsertHost([existing], host("a"), false);
  expect(result.error).toContain("already exists");
  expect(result.items[0].routes).toHaveLength(1);
});

test("an edit replaces the host in place", () => {
  const existing = host("a");
  const next = host("a", [route]);
  const result = upsertHost([host("b"), existing], next, true);
  expect(result.error).toBeNull();
  expect(result.items[1].routes).toHaveLength(1);
});

test("an add with a taken upstream id is refused", () => {
  const upstream: ProxyUpstream = {
    id: "app",
    url: "http://127.0.0.1:3000",
    "connect-timeout-ms": 2000,
    "read-timeout-ms": 5000,
    "tls-verify": true,
  };
  const result = upsertUpstream([upstream], { ...upstream, url: "http://x" }, false);
  expect(result.error).toContain("already exists");
  expect(result.items[0].url).toBe("http://127.0.0.1:3000");
});
