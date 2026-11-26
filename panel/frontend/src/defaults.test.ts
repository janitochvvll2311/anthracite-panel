import { describe, expect, test } from "bun:test";

import type { Contribution } from "@anth-panel/panel";

import { defaultView } from "./defaults";

function view(
  id: string,
  label: string,
  tag: string,
  hidden = false,
): Contribution {
  return {
    app: { id, version: "0.1.0", risk: "read", slots: [] },
    slot: {
      id: "panel.view",
      label,
      order: 1,
      tag,
      module: "web/app.js",
      sha256: "a".repeat(64),
      hidden,
    },
  };
}

describe("defaultView", () => {
  test("prefers the Home app", () => {
    const files = view("org.anthracite.fs", "Files", "anth-fs");
    const home = view("org.anthracite.home", "Home", "home");
    expect(defaultView([files, home])).toBe(home);
  });

  test("falls back to the home tag", () => {
    const home = view("org.example.dashboard", "Overview", "home");
    expect(defaultView([home])).toBe(home);
  });

  test("ignores hidden or missing Home slots", () => {
    const hidden = view("org.anthracite.home", "Home", "home", true);
    const files = view("org.anthracite.fs", "Files", "anth-fs");
    expect(defaultView([hidden, files])).toBeUndefined();
    expect(defaultView([files])).toBeUndefined();
  });
});
