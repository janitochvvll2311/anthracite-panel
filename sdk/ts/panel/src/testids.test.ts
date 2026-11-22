import { expect, test } from "bun:test";

import { TESTID } from "./testids";

// Two nodes with the same hook make a selector ambiguous without failing
// anything else, so the map is checked as a contract of its own.
test("every test id is unique", () => {
  const values = Object.values(TESTID);
  expect(new Set(values).size).toBe(values.length);
});

test("every test id is kebab-case", () => {
  for (const value of Object.values(TESTID)) {
    expect(value).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
  }
});
