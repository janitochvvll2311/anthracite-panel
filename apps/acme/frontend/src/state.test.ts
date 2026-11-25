import { expect, test } from "bun:test";

import { badgeClass, directoryLabel, isStaging, splitNames } from "./state";
import type { Capability } from "./types";

const capability = (staging: string[]): Capability => ({
  challenge: "http-01",
  solvers: [],
  directories: ["https://acme.example/directory", "https://acme.example/staging"],
  "staging-directories": staging,
});

test("a valid order is green and an invalid one is red", () => {
  expect(badgeClass("valid")).toBe("ok");
  expect(badgeClass("pending")).toBe("warn");
  expect(badgeClass("invalid")).toBe("bad");
  expect(badgeClass("revoked")).toBe("bad");
  expect(badgeClass("issued-unstored")).toBe("warn");
  expect(badgeClass("something-new")).toBe("off");
});

test("names split on commas and whitespace and keep wildcards", () => {
  expect(splitNames(" a.test,*.b.test\n c.test ")).toEqual([
    "a.test",
    "*.b.test",
    "c.test",
  ]);
  expect(splitNames("   ")).toEqual([]);
});

test("staging comes from the capability, not from the URL text", () => {
  expect(
    isStaging([capability(["https://acme.example/staging"])], "https://acme.example/staging"),
  ).toBe(true);
  expect(
    isStaging([capability(["https://acme.example/staging"])], "https://acme.example/directory"),
  ).toBe(false);
});

test("a directory label is its host and falls back to the raw value", () => {
  expect(directoryLabel("https://acme.example/directory")).toBe("acme.example");
  expect(directoryLabel("not a url")).toBe("not a url");
});
