import { expect, test } from "../fixtures";
import type { FrameLocator, Locator } from "@playwright/test";

/// The dialog grows to the form after mount; a control below the first frame
/// height would be clicked on the dialog, so wait for the bottom control.
async function waitForForm(body: Locator, frame: FrameLocator): Promise<void> {
  await expect
    .poll(
      async () => {
        const box = await body.locator("iframe").boundingBox();
        const button = await frame.getByTestId("containers-modal-cancel").boundingBox();
        return Boolean(
          box &&
            button &&
            button.y + button.height <= box.y + box.height + 1,
        );
      },
      { timeout: 5_000 },
    )
    .toBe(true);
}

/// Runs only when the composition carries the Containers app; CI does not.
test("the containers view lists, follows logs and creates from the FAB", async ({
  panel,
}) => {
  const view = panel.sidebar.view("org.anthracite.containers");
  if ((await view.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.containers view");
    return;
  }
  // The seeded `web` container and its logs come from the Engine API fixture
  // (scripts/containers-fixture.ts), not from a real Docker host.
  if (!process.env.ANTH_CONTAINERS_FIXTURE) {
    test.skip(true, "needs the Engine API fixture (ANTH_CONTAINERS_FIXTURE)");
    return;
  }
  await view.first().click();
  const slot = panel.panes.slot("org.anthracite.containers");
  await slot.waitUntilMounted({ app: "org.anthracite.containers" });
  await expect(slot.frame.getByTestId("containers-view")).toBeVisible();

  const web = slot.frame.locator(
    '[data-testid="container"][data-container-id="web"]',
  );
  await expect(web).toBeVisible();
  await web.click();
  await expect(slot.frame.getByTestId("containers-detail")).toBeVisible();
  await expect(slot.frame.getByTestId("containers-stats")).toBeVisible();

  await slot.frame.getByTestId("container-logs").click();
  await expect(slot.frame.getByTestId("containers-log")).toContainText(
    "hello from the fixture",
    { timeout: 10_000 },
  );

  // The view fits a phone width without horizontal overflow.
  await panel.page.setViewportSize({ width: 390, height: 844 });
  await expect(panel.page.locator("html")).toHaveAttribute(
    "data-viewport",
    "mobile",
  );
  const overflow = await slot.html.evaluate((element) => {
    return (
      Math.max(element.scrollWidth, document.body.scrollWidth) -
      element.clientWidth
    );
  });
  expect(overflow).toBeLessThanOrEqual(1);

  const fab = panel.fabs.fab("containers-fab");
  await expect(panel.fabs.trigger(fab)).toBeVisible();
  await panel.fabs.trigger(fab).click();
  await panel.fabs.item(fab, "create").click();
  await panel.modal.waitUntilOpen();
  const frame = panel.modal.slot().frame;
  await expect(frame.getByTestId("containers-create-image")).toBeVisible();
  await waitForForm(panel.modal.body, frame);
  const name = `e2e-${Date.now()}`;
  await frame.getByTestId("containers-create-image").fill("alpine:3.21");
  await frame.getByTestId("containers-create-name").fill(name);
  await frame
    .getByTestId("containers-modal-submit")
    .evaluate((element) => (element as HTMLButtonElement).click());
  await expect(panel.modal.root).toBeHidden({ timeout: 30_000 });

  const created = slot.frame
    .locator('[data-testid="container"]', { hasText: name })
    .first();
  await expect(created).toBeVisible({ timeout: 15_000 });
});
