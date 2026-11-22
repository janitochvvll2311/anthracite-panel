import { expect, test } from "../fixtures";

/// Floating windows are the panel's own layout, so `data-mode` and the stack
/// entry are the contract; dragging and native resizing stay out of scope.
test("a docked view floats, maximizes, minimizes, restores and docks", async ({
  panel,
}) => {
  const appId = await panel.openFirstView();
  await panel.panes.float(appId);

  const window = panel.windows.window(appId);
  await expect(window).toBeVisible();
  await expect(window).toHaveAttribute("data-kind", "view");
  await expect(window).toHaveAttribute("data-mode", "normal");
  await expect(panel.panes.pane(appId)).toHaveCount(0);
  await panel.windows.slot(window).waitUntilMounted({ app: appId });

  await panel.windows.maximize(window);
  await expect(window).toHaveAttribute("data-mode", "maximized");

  await panel.windows.minimize(window);
  await expect(window).toHaveAttribute("data-mode", "minimized");
  await expect(panel.stack.item(appId)).toBeVisible();

  await panel.stack.restore(appId);
  await expect(window).toHaveAttribute("data-mode", "maximized");

  await panel.windows.dock(window);
  await expect(window).toHaveCount(0);
  await expect(panel.panes.pane(appId)).toBeVisible();
});

test("a floating window closes from its own tool", async ({ panel }) => {
  const appId = await panel.openFirstView();
  await panel.panes.float(appId);

  const window = panel.windows.window(appId);
  await expect(window).toBeVisible();

  await panel.windows.close(window);
  await expect(window).toHaveCount(0);
  await expect(panel.panes.pane(appId)).toHaveCount(0);
});
