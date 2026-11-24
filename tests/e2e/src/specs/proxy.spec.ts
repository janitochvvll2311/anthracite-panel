import { expect, test } from "../fixtures";
import type { PanelPage } from "../pages/panel.page";

/// The modal body scrolls when the form is taller than the dialog; the panel
/// grows the iframe after mount. Wait for the form field, then scroll the body
/// before hitting a control at the bottom.
async function openForm(panel: PanelPage, field: string) {
  await panel.modal.waitUntilOpen();
  const frame = panel.modal.slot().frame;
  await expect(frame.getByTestId(field)).toBeVisible();
  return frame;
}

/// Runs only when the composition carries the Proxy app (the demo does not, by
/// design until the v0.7 release wiring); otherwise the spec is skipped with a
/// message, so the standard e2e suite stays green.
test("the proxy view stays usable in the three viewport modes", async ({
  panel,
}) => {
  const proxy = panel.sidebar.view("org.anthracite.proxy");
  if ((await proxy.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.proxy view");
    return;
  }
  await proxy.first().click();

  const pane = panel.panes.pane("org.anthracite.proxy");
  await expect(pane).toBeVisible();
  const slot = panel.panes.slot("org.anthracite.proxy");
  await slot.waitUntilMounted({ app: "org.anthracite.proxy" });
  await expect(slot.frame.getByTestId("proxy-view")).toBeVisible();

  for (const mode of ["desktop", "tablet", "mobile"] as const) {
    await panel.viewport.use(mode);
    await expect(slot.html).toHaveAttribute("data-viewport", mode);
    await panel.viewport.expectDrawer(false);

    for (const section of [
      "listeners",
      "hosts",
      "upstreams",
      "log",
    ] as const) {
      await slot.frame.getByTestId(`proxy-tab-${section}`).click();
      const overflow = await slot.html.evaluate((element) => {
        const documentWidth = element.clientWidth;
        return (
          Math.max(element.scrollWidth, document.body.scrollWidth) -
          documentWidth
        );
      });
      expect(overflow, `${mode}/${section} horizontal overflow`).toBeLessThanOrEqual(1);
    }
  }

  await slot.frame.getByTestId("proxy-tab-hosts").click();
});

test("the proxy add and edit actions use modals from the FAB", async ({
  panel,
}) => {
  const proxy = panel.sidebar.view("org.anthracite.proxy");
  if ((await proxy.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.proxy view");
    return;
  }
  await proxy.first().click();
  const slot = panel.panes.slot("org.anthracite.proxy");
  await slot.waitUntilMounted({ app: "org.anthracite.proxy" });
  await slot.frame.getByTestId("proxy-tab-hosts").click();

  // Edit opens the host modal with the id fixed.
  const firstHost = slot.frame.getByTestId("proxy-host").first();
  await expect(firstHost).toBeVisible();
  await firstHost.getByTestId("proxy-edit-host").click();
  const edit = await openForm(panel, "proxy-host-id");
  await expect(edit.getByTestId("proxy-host-id")).toBeDisabled();
  expect(await edit.getByTestId("proxy-host-id").inputValue()).not.toBe("");
  await panel.modal.close();
  await expect(panel.modal.root).toBeHidden();

  // Add a host from the FAB menu opens the same modal empty.
  const fab = panel.fabs.fab("proxy-fab");
  await expect(panel.fabs.trigger(fab)).toBeVisible();
  await panel.fabs.trigger(fab).click();
  await panel.fabs.item(fab, "add-host").click();
  const form = await openForm(panel, "proxy-host-id");
  await expect(panel.modal.app).toHaveText("org.anthracite.proxy");
  await expect(form.getByTestId("proxy-host-id")).toBeEnabled();
  await expect(form.getByTestId("proxy-host-id")).toHaveValue("");
  await panel.modal.close();
  await expect(panel.modal.root).toBeHidden();

  // Mobile width: the FAB stays usable and the modal form is full width.
  await panel.page.setViewportSize({ width: 390, height: 844 });
  await expect(panel.page.locator("html")).toHaveAttribute(
    "data-viewport",
    "mobile",
  );
  const mobileFab = panel.fabs.fab("proxy-fab");
  await expect(panel.fabs.trigger(mobileFab)).toBeVisible();
  await panel.fabs.trigger(mobileFab).click();
  await panel.fabs.item(mobileFab, "add-upstream").click();
  const upstream = await openForm(panel, "proxy-upstream-url");
  await expect(upstream.getByTestId("proxy-upstream-url")).toBeVisible();
  await panel.modal.close();
});
