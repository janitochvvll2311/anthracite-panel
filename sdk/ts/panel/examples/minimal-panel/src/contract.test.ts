import { expect, test } from "bun:test";

import { VIEW, ACTION, type App, type Slot } from "@anthracite/panel";
import { ignoredViews, visibleViews } from "./contract";

function slot(id: string, tag: string, order: number): Slot {
  return {
    id,
    label: tag,
    order,
    tag,
    module: `web/${tag}.js`,
    sha256: "a".repeat(64),
  };
}

function app(id: string, slots: Slot[]): App {
  return { id, version: "0.1.0", risk: "read", slots };
}

test("the second panel mounts only visible panel.view, in contract order", () => {
  const notes = app("org.example.notes", [
    slot(ACTION, "run", 0),
    { ...slot(VIEW, "hidden", 0), hidden: true },
    slot(VIEW, "beta", 2),
    slot(VIEW, "alpha", 1),
  ]);
  expect(visibleViews([notes]).map((contribution) => contribution.slot.tag)).toEqual([
    "alpha",
    "beta",
  ]);
  expect(ignoredViews([notes]).map((contribution) => contribution.slot.tag)).toEqual([
    "run",
  ]);
});
