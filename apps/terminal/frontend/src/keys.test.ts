import { describe, expect, test } from "bun:test";

import { applyModifiers } from "./keys";

describe("applyModifiers", () => {
  test("passes through without sticky modifiers", () => {
    expect(applyModifiers("c", false, false)).toBe("c");
    expect(applyModifiers("hello", true, false)).toBe("hello");
  });

  test("maps Ctrl letters and punctuation to control codes", () => {
    expect(applyModifiers("c", true, false)).toBe("\x03");
    expect(applyModifiers("C", true, false)).toBe("\x03");
    expect(applyModifiers("_", true, false)).toBe("\x1f");
    expect(applyModifiers("?", true, false)).toBe("\x7f");
    expect(applyModifiers(" ", true, false)).toBe("\x00");
  });

  test("prefixes Alt characters with ESC", () => {
    expect(applyModifiers("x", false, true)).toBe("\x1bx");
  });

  test("leaves control characters alone", () => {
    expect(applyModifiers("\x03", true, false)).toBe("\x03");
    expect(applyModifiers("\x7f", false, true)).toBe("\x7f");
  });
});
