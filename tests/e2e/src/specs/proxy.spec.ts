import { expect, test } from "../fixtures";

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
