import { expect, test } from "../fixtures";

/// The shell, not an app: the sidebar lists the composed views, a tab docks a
/// pane, closing the last one returns the empty state, and the two panel-level
/// overlays render their contributors.
test("the sidebar lists the composed views", async ({ panel }) => {
  await expect(panel.sidebar.items().first()).toBeVisible();
  expect(await panel.sidebar.items().count()).toBeGreaterThan(0);
  await expect(panel.sidebar.placeholder).toHaveCount(0);
  await panel.expectHealthy();
});

test("opening a view docks its tab and mounts the slot", async ({ panel }) => {
  const appId = await panel.openFirstView();

  await expect(panel.tabs.tab(appId)).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(panel.panes.active()).toHaveAttribute("data-app-id", appId);

  await panel
    .activeSlot()
    .waitUntilMounted({ app: appId, kind: "view" });
});

test("closing the last tab returns the empty workspace", async ({ panel }) => {
  await panel.tabs.closeAll();

  await expect(panel.tabs.items()).toHaveCount(0);
  await expect(panel.panes.items()).toHaveCount(0);
  await expect(panel.panes.emptyState).toBeVisible();
});

test("settings shows contributed sections or the explicit empty state", async ({
  panel,
}) => {
  await panel.settings.open();

  // A composition may legitimately contribute no settings; the overlay must
  // still say so rather than render an empty card.
  await expect(
    panel.settings.sections().first().or(panel.settings.empty),
  ).toBeVisible();

  await panel.settings.close();
});

test("the manager lists installed apps and the saved composition", async ({
  panel,
}) => {
  await panel.manager.open();
  await panel.manager.waitUntilLoaded();

  await expect(panel.manager.apps().first()).toBeVisible();
  await expect(panel.manager.compositionItems().first()).toBeVisible();

  await panel.manager.close();
});
