import { expect, test } from "bun:test";

import { age, channelKind, formatBytes, initials, matches, shortDigest } from "./state";
import type { Summary } from "./types";

const summary: Summary = {
  id: "org.example.notes",
  name: "Notes",
  summary: "A tiny notes app.",
  publisher: "Example",
  categories: ["productivity"],
  latest: "1.0.0",
};

test("channels map to badge tones", () => {
  expect(channelKind("stable")).toBe("ok");
  expect(channelKind("beta")).toBe("warn");
  expect(channelKind("nightly")).toBe("off");
});

test("bytes, ages and initials are readable", () => {
  expect(formatBytes(2048)).toBe("2.0 KiB");
  expect(age(0)).toBe("never refreshed");
  expect(age(150, 200)).toBe("just now");
  expect(age(200, 1000)).toBe("13 min ago");
  expect(initials("Example Notes")).toBe("EN");
  expect(shortDigest("0123456789abcdef")).toBe("0123456789ab…");
});

test("search matches id, name, summary and publisher", () => {
  expect(matches(summary, "notes")).toBe(true);
  expect(matches(summary, "EXAMPLE")).toBe(true);
  expect(matches(summary, "productivity")).toBe(false);
  expect(matches(summary, "missing")).toBe(false);
});
