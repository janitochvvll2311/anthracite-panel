import { expect, test } from "bun:test";

import {
  DESKTOP_MIN,
  TABLET_MIN,
  initialViewportMode,
  isViewportMode,
  resolveViewport,
} from "./viewport";

test("auto picks mobile, tablet and desktop at the breakpoints", () => {
  expect(resolveViewport("auto", TABLET_MIN - 1)).toBe("mobile");
  expect(resolveViewport("auto", TABLET_MIN)).toBe("tablet");
  expect(resolveViewport("auto", DESKTOP_MIN - 1)).toBe("tablet");
  expect(resolveViewport("auto", DESKTOP_MIN)).toBe("desktop");
});

test("a manual mode does not follow the width", () => {
  expect(resolveViewport("mobile", 1600)).toBe("mobile");
  expect(resolveViewport("desktop", 320)).toBe("desktop");
  expect(resolveViewport("tablet", 320)).toBe("tablet");
});

test("only the four names are accepted", () => {
  for (const value of ["auto", "desktop", "tablet", "mobile"]) {
    expect(isViewportMode(value)).toBe(true);
  }
  for (const value of [null, "", "phone", "Desktop"]) {
    expect(isViewportMode(value)).toBe(false);
  }
});

test("the URL wins over storage, and bad values fall back", () => {
  expect(initialViewportMode("mobile", "desktop")).toBe("mobile");
  expect(initialViewportMode(null, "tablet")).toBe("tablet");
  expect(initialViewportMode("phone", "tablet")).toBe("tablet");
  expect(initialViewportMode("phone", "phone")).toBe("auto");
});
