import { expect, test } from "bun:test";

import { createSseParser } from "./api";

test("splits events that arrive in arbitrary chunks", () => {
  const events: string[] = [];
  const feed = createSseParser((data) => events.push(data));
  feed('data: {"interfaces":[]}\n');
  expect(events).toEqual([]);
  feed('\ndata: {"routes":[]}\n\n');
  expect(events).toEqual(['{"interfaces":[]}', '{"routes":[]}']);
});

test("ignores comments and joins multi-line data", () => {
  const events: string[] = [];
  const feed = createSseParser((data) => events.push(data));
  feed(": keep-alive\n\ndata: line one\ndata: line two\n\n");
  expect(events).toEqual(["line one\nline two"]);
});

test("accepts CRLF frames", () => {
  const events: string[] = [];
  const feed = createSseParser((data) => events.push(data));
  feed('data: {"ok":true}\r\n\r\n');
  expect(events).toEqual(['{"ok":true}']);
});
