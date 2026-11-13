import { describe, expect, test } from "bun:test";

import { authorityDiff, requiredLevels } from "./authority";

describe("requiredLevels", () => {
  test("read needs no approval flag", () => {
    expect(requiredLevels("read")).toEqual([]);
  });

  test("admin includes write and admin", () => {
    expect(requiredLevels("admin")).toEqual(["write", "admin"]);
  });

  test("package includes the whole ladder", () => {
    expect(requiredLevels("package")).toEqual([
      "write",
      "admin",
      "host",
      "package",
    ]);
  });
});

describe("authorityDiff", () => {
  test("a new app requires the full effective risk", () => {
    expect(authorityDiff(undefined, "admin")).toEqual({
      current: "none",
      effective: "admin",
      required: ["write", "admin"],
      newLevels: ["write", "admin"],
    });
  });

  test("an accepted level is not new authority", () => {
    expect(
      authorityDiff({ risk: "admin", accepted: ["write", "admin"] }, "admin"),
    ).toEqual({
      current: "admin",
      effective: "admin",
      required: ["write", "admin"],
      newLevels: [],
    });
  });

  test("a risk increase names only the new levels", () => {
    expect(
      authorityDiff({ risk: "write", accepted: ["write"] }, "host"),
    ).toEqual({
      current: "write",
      effective: "host",
      required: ["write", "admin", "host"],
      newLevels: ["admin", "host"],
    });
  });

  test("an app that is not running keeps the grant as the target", () => {
    expect(
      authorityDiff({ risk: "write", accepted: ["write"] }, undefined),
    ).toEqual({
      current: "write",
      effective: "write",
      required: ["write"],
      newLevels: [],
    });
  });

  test("no grant and no effective app has no required levels", () => {
    expect(authorityDiff(undefined, undefined)).toEqual({
      current: "none",
      effective: "none",
      required: [],
      newLevels: [],
    });
  });
});
