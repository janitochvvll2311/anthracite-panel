import { expect, test } from "../fixtures";

/// The viewport mode is panel chrome: pinning it changes the root attribute,
/// reveals the drawer at mobile width and keeps the workspace usable.
test("the mobile viewport opens the drawer and docks a view from it", async ({
  panel,
}) => {
  await panel.viewport.use("mobile");
  await panel.viewport.expectEffective("mobile");
  await expect(panel.viewport.drawerToggle).toBeVisible();

  await panel.viewport.drawerToggle.click();
  await panel.viewport.expectDrawer(true);

  const appId = await panel.sidebar.open();
  await panel.viewport.expectDrawer(false);
  await expect(panel.panes.pane(appId)).toBeVisible();

  await panel.viewport.use("desktop");
  await panel.viewport.expectEffective("desktop");
});

test("the drawer closes from its own control", async ({ panel }) => {
  await panel.viewport.use("mobile");
  await panel.viewport.expectEffective("mobile");

  await panel.viewport.drawerToggle.click();
  await panel.viewport.expectDrawer(true);

  await panel.viewport.drawerClose.click();
  await panel.viewport.expectDrawer(false);
});
