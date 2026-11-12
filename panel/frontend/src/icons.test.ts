import { expect, test } from "bun:test";

import { iconAssetUrl, iconInitial, iconSvg } from "./icons";

test("known icon names become trusted SVG markup", () => {
  expect(iconSvg("refresh")).toContain("<svg");
  expect(iconSvg("upload")).toContain("<svg");
  expect(iconSvg("not-a-real-icon")).toBeUndefined();
});

test("an unknown icon falls back to its first character", () => {
  expect(iconInitial(undefined, "Refresh")).toBe("R");
  expect(iconInitial("not-a-real-icon", "Refresh")).toBe("n");
  expect(iconInitial("", "•")).toBe("•");
});

test("an icon asset URL is hash-pinned and encoded", () => {
  expect(iconAssetUrl("org.example.notes", "web/a b.svg", "a".repeat(64))).toBe(
    "/api/apps/org.example.notes/assets/web/a%20b.svg?v=aaaaaaaaaaaaaaaa",
  );
});
