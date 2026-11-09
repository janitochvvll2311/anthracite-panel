import { describe, expect, test } from "bun:test";
import { minimizedFrom, toggleMaximized } from "./windows";

describe("floating window modes", () => {
  test("maximize toggles between normal and maximized", () => {
    expect(toggleMaximized("normal")).toBe("maximized");
    expect(toggleMaximized("maximized")).toBe("normal");
  });

  test("a maximized window comes back maximized after a minimize", () => {
    expect(minimizedFrom("maximized")).toBe("maximized");
    expect(minimizedFrom("normal")).toBe("normal");
  });
});
