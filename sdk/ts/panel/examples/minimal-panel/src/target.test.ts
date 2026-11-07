import { expect, test } from "bun:test";

import { isLoopback, loopbackTarget } from "./target";

test("only loopback hosts are accepted by default", () => {
  expect(isLoopback("localhost")).toBe(true);
  expect(isLoopback("127.0.0.1")).toBe(true);
  expect(isLoopback("::1")).toBe(true);
  expect(isLoopback("[::1]")).toBe(true);
  expect(isLoopback("evil.example")).toBe(false);
  expect(isLoopback("127.0.0.1.evil.example")).toBe(false);
});

test("a remote target needs an explicit opt-in", () => {
  expect(loopbackTarget("http://127.0.0.1:8090").hostname).toBe("127.0.0.1");
  expect(() => loopbackTarget("https://evil.example")).toThrow(/not loopback/);
  expect(
    loopbackTarget("https://evil.example", true).hostname,
  ).toBe("evil.example");
});

test("credentials and non-http schemes are refused", () => {
  expect(() => loopbackTarget("file:///tmp/engine.sock")).toThrow(/http/);
  expect(() => loopbackTarget("http://user:pass@127.0.0.1:8090")).toThrow(
    /credentials/,
  );
});
