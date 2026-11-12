import { expect, test } from "bun:test";

import {
  ACTION,
  FAB,
  MODAL,
  PORTABLE,
  REFRESH,
  VIEW,
  assetUrl,
  contributionKey,
  frameContext,
  ignored,
  ordered,
  type App,
  type Slot,
} from "./index";

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

test("only the requested slot is mounted, in deterministic order", () => {
  const notes = app("org.example.notes", [
    slot(VIEW, "beta", 2),
    slot(ACTION, "run", 1),
    slot(VIEW, "alpha", 1),
  ]);
  const mounted = ordered([notes], VIEW);
  expect(mounted.map((contribution) => contribution.slot.tag)).toEqual([
    "alpha",
    "beta",
  ]);
  expect(ordered([notes], ACTION)).toHaveLength(1);
});

test("order is code-unit, not locale-collated", () => {
  const mounted = ordered(
    [app("org.example.a", [slot(VIEW, "same", 1)]), app("org.example.Z", [slot(VIEW, "same", 1)])],
    VIEW,
  );
  expect(mounted.map((contribution) => contribution.app.id)).toEqual([
    "org.example.Z",
    "org.example.a",
  ]);
});

test("the same order and tag tie-break by app id", () => {
  const mounted = ordered(
    [app("org.example.b", [slot(VIEW, "same", 1)]), app("org.example.a", [slot(VIEW, "same", 1)])],
    VIEW,
  );
  expect(mounted.map((contribution) => contribution.app.id)).toEqual([
    "org.example.a",
    "org.example.b",
  ]);
});

test("an asset URL encodes every segment and carries the hash", () => {
  const contribution = { app: app("org.example.notes", []), slot: slot(VIEW, "notes", 1) };
  contribution.slot.module = "web/a b#c?.js";
  expect(assetUrl(contribution)).toBe(
    "/api/apps/org.example.notes/assets/web/a%20b%23c%3F.js?v=aaaaaaaaaaaaaaaa",
  );
  expect(contributionKey(contribution)).toBe("org.example.notes#panel.view#notes");
});

test("ignored contributions target slots the panel does not implement", () => {
  const notes = app("org.example.notes", [slot(VIEW, "view", 1), slot(ACTION, "run", 1)]);
  expect(ignored([notes], [VIEW]).map((contribution) => contribution.slot.id)).toEqual([
    ACTION,
  ]);
});

test("fab and modal are part of the portable vocabulary", () => {
  expect(FAB).toBe("panel.fab");
  expect(MODAL).toBe("panel.modal");
  expect(REFRESH).toBe("refresh");
  expect([...PORTABLE]).toEqual([VIEW, ACTION, "panel.settings", FAB, MODAL]);
});

test("a view-only panel ignores fab and modal without breaking the view list", () => {
  const notes = app("org.example.notes", [
    slot(VIEW, "notes", 1),
    {
      id: FAB,
      label: "Add",
      order: 2,
      tag: "add",
      module: "",
      sha256: "",
    },
    {
      id: MODAL,
      label: "Edit",
      order: 3,
      tag: "edit",
      module: "web/edit.js",
      sha256: "b".repeat(64),
    },
  ]);
  expect(ordered([notes], VIEW).map((contribution) => contribution.slot.tag)).toEqual([
    "notes",
  ]);
  expect(
    ignored([notes], [VIEW]).map((contribution) => contribution.slot.id),
  ).toEqual([FAB, MODAL]);
});

test("a fab carries a view and actions, a modal carries a module", () => {
  const fab: Slot = {
    id: FAB,
    label: "Add",
    order: 1,
    tag: "note-actions",
    view: "notes",
    actions: [
      { label: "New", tag: "new", target: "edit-note" },
      { label: "Refresh", tag: "refresh", action: "refresh" },
    ],
    module: "",
    sha256: "",
  };
  const modal: Slot = {
    id: MODAL,
    label: "Edit note",
    order: 2,
    tag: "edit-note",
    module: "web/edit-note.js",
    sha256: "b".repeat(64),
  };
  const notes = app("org.example.notes", [fab, modal]);
  expect(ordered([notes], FAB)[0]!.slot.actions).toHaveLength(2);
  expect(ordered([notes], MODAL)[0]!.slot.view).toBeUndefined();
});


test("frame context copies the panel declaration and the contribution", () => {
  const contribution = {
    app: app("org.example.notes", []),
    slot: slot(VIEW, "notes", 1),
  };
  const context = frameContext(contribution, "view", {
    slots: [VIEW],
    version: "0.5.0",
  });
  expect(context.app).toEqual({
    id: "org.example.notes",
    version: "0.1.0",
    risk: "read",
  });
  expect(context.contribution.app).toBe("org.example.notes");
  expect(context.panel).toEqual({ slots: [VIEW], version: "0.5.0" });
  expect(context.kind).toBe("view");
  expect("params" in context).toBe(false);
});

test("frame context carries opaque params when the panel opened a view", () => {
  const contribution = {
    app: app("org.example.notes", []),
    slot: slot(VIEW, "notes", 1),
  };
  const params = { path: "/home/operator/notes.txt", text: "hello" };
  const context = frameContext(
    contribution,
    "view",
    { slots: [VIEW], version: "0.5.0" },
    params,
  );
  expect(context.params).toEqual(params);
});
