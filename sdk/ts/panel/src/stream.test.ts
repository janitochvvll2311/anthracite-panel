import { expect, test } from "bun:test";

import { isStreamingContentType } from "./stream";

test("octet-stream and event-stream are live bodies", () => {
  expect(isStreamingContentType("application/octet-stream")).toBe(true);
  expect(isStreamingContentType("text/event-stream")).toBe(true);
});

test("parameters and casing do not change the media type", () => {
  expect(
    isStreamingContentType("Application/Octet-Stream; charset=binary"),
  ).toBe(true);
  expect(isStreamingContentType("text/event-stream; charset=utf-8")).toBe(true);
});

test("JSON and a missing type stay buffered", () => {
  expect(isStreamingContentType("application/json")).toBe(false);
  expect(isStreamingContentType("application/octet-streamish")).toBe(false);
  expect(isStreamingContentType(null)).toBe(false);
  expect(isStreamingContentType(undefined)).toBe(false);
  expect(isStreamingContentType("")).toBe(false);
});
