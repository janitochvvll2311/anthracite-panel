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

// The frame is a static document served as-is, so it cannot import the map.
// The suite waits on this id; compare the literal here so a rename fails in the
// unit test instead of timing out in the browser.
test("the frame applies the shared slotRoot id", async () => {
  const frame = await Bun.file(
    new URL("./app-frame.html", import.meta.url),
  ).text();
  expect(frame).toContain(
    `document.body.dataset.testid = "${TESTID.slotRoot}"`,
  );
});
