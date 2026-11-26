import { describe, expect, test } from "bun:test";

import type { Contribution } from "@anth-panel/panel";

import { groupFor, groupViews, parseParent } from "./groups";

function view(
  id: string,
  label: string,
  tag: string,
  extra: { parent?: string; hidden?: boolean; order?: number } = {},
): Contribution {
  return {
    app: { id, version: "0.1.0", risk: "read", slots: [] },
    slot: {
      id: "panel.view",
      label,
      order: extra.order ?? 1,
      tag,
      module: "web/app.js",
      sha256: "a".repeat(64),
      hidden: extra.hidden,
      parent: extra.parent,
    },
  };
}

describe("groupViews", () => {
  test("a child follows its parent as a sub-tab", () => {
    const net = view("org.anthracite.net", "Network", "anth-net", { order: 1 });
    const firewall = view(
      "org.anthracite.firewall",
      "Firewall",
      "anth-firewall",
      { parent: "org.anthracite.net#anth-net", order: 2 },
    );
    const groups = groupViews([net, firewall]);
    expect(groups).toHaveLength(1);
    expect(groups[0].parent).toBe(net);
    expect(groups[0].children).toEqual([net, firewall]);
  });

  test("a missing or hidden parent falls back to a top-level tab", () => {
    const missing = view("org.example.child", "Child", "child", {
      parent: "org.example.parent#view",
    });
    const hiddenParent = view("org.example.parent", "Parent", "view", {
      hidden: true,
    });
    const child = view("org.example.child", "Child", "child", {
      parent: "org.example.parent#view",
    });
    expect(groupViews([missing]).map((group) => group.parent)).toEqual([missing]);
    const groups = groupViews([hiddenParent, child]).filter(
      (group) => !group.parent.slot.hidden,
    );
    expect(groups.map((group) => group.parent)).toEqual([child]);
  });

  test("Firewall keeps its place when Network is removed", () => {
    const terminal = view("org.anthracite.terminal", "Terminal", "anth-terminal", {
      order: 10,
    });
    const firewall = view(
      "org.anthracite.firewall",
      "Firewall",
      "anth-firewall",
      { parent: "org.anthracite.net#anth-net", order: 22 },
    );
    const groups = groupViews([terminal, firewall]);
    expect(groups.map((group) => group.parent.slot.label)).toEqual([
      "Terminal",
      "Firewall",
    ]);
    expect(groups[1].parent).toBe(firewall);
  });

  test("a nested parent is not followed", () => {
    const root = view("org.example.root", "Root", "root");
    const middle = view("org.example.middle", "Middle", "middle", {
      parent: "org.example.root#root",
    });
    const leaf = view("org.example.leaf", "Leaf", "leaf", {
      parent: "org.example.middle#middle",
    });
    const groups = groupViews([root, middle, leaf]);
    expect(groups.find((group) => group.children.includes(middle))?.parent).toBe(
      root,
    );
    expect(groups.find((group) => group.children.includes(leaf))?.parent).toBe(
      leaf,
    );
  });

  test("groupFor finds the tab that owns a child", () => {
    const parent = view("org.example.parent", "Parent", "view");
    const child = view("org.example.child", "Child", "child", {
      parent: "org.example.parent#view",
    });
    const groups = groupViews([parent, child]);
    expect(groupFor(groups, child)?.key).toBe(groupFor(groups, parent)?.key);
  });

  test("a parent reference needs an app and a tag", () => {
    expect(parseParent("org.example.parent#view")).toEqual({
      app: "org.example.parent",
      tag: "view",
    });
    expect(parseParent("org.example.parent")).toBeNull();
    expect(parseParent("#view")).toBeNull();
    expect(parseParent("org.example.parent#")).toBeNull();
  });
});
