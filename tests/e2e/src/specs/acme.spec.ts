import { expect, test } from "../fixtures";
import type { FrameLocator, Locator } from "@playwright/test";

/// The dialog starts at the frame's minimum height and grows when the module
/// reports its content. A control below that box would be clicked on the
/// dialog, so wait until the submit button is inside the iframe.
async function waitForForm(body: Locator, frame: FrameLocator): Promise<void> {
  await expect
    .poll(
      async () => {
        const box = await body.locator("iframe").boundingBox();
        const button = await frame.getByTestId("acme-modal-cancel").boundingBox();
        return Boolean(
          box &&
            button &&
            button.y >= box.y &&
            button.y + button.height <= box.y + box.height + 1,
        );
      },
      { timeout: 5_000 },
    )
    .toBe(true);
}

/// Runs only when the composition carries the ACME app; the demo does not, by
/// design until the v0.7 release wiring, so the standard suite stays green.
test("the acme view opens the issue modal from the FAB", async ({ panel }) => {
  const view = panel.sidebar.view("org.anthracite.acme");
  if ((await view.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.acme view");
    return;
  }
  await view.first().click();
  const slot = panel.panes.slot("org.anthracite.acme");
  await slot.waitUntilMounted({ app: "org.anthracite.acme" });
  await expect(slot.frame.getByTestId("acme-view")).toBeVisible();

  // The view and its two sections fit the frame at a phone width.
  await panel.page.setViewportSize({ width: 390, height: 844 });
  await expect(panel.page.locator("html")).toHaveAttribute(
    "data-viewport",
    "mobile",
  );
  await expect(slot.frame.getByTestId("acme-accounts")).toBeVisible();
  await expect(slot.frame.getByTestId("acme-orders")).toBeVisible();
  const overflow = await slot.html.evaluate((element) => {
    return (
      Math.max(element.scrollWidth, document.body.scrollWidth) -
      element.clientWidth
    );
  });
  expect(overflow).toBeLessThanOrEqual(1);

  const fab = panel.fabs.fab("acme-fab");
  await expect(panel.fabs.trigger(fab)).toBeVisible();
  await panel.fabs.trigger(fab).click();
  await panel.fabs.item(fab, "issue").click();
  await panel.modal.waitUntilOpen();
  await expect(panel.modal.app).toHaveText("org.anthracite.acme");
  const frame = panel.modal.slot().frame;
  await expect(frame.getByTestId("acme-issue-directory")).toBeVisible();
  await expect(frame.getByTestId("acme-issue-names")).toBeVisible();
  await expect(frame.getByTestId("acme-modal-submit")).toBeVisible();
  await expect(frame.locator("[data-note]")).toContainText("account");
  await waitForForm(panel.modal.body, frame);
  await frame.getByTestId("acme-modal-cancel").click();
  await expect(panel.modal.root).toBeHidden();
});

/// The QA row against a real CA. It is skipped unless `ANTH_ACME_LIVE=1` and a
/// Pebble directory is configured; `scripts/acme-pebble.sh` starts the CA and
/// the token server for it.
test("the issue modal runs a real Pebble order", async ({ panel }) => {
  test.skip(
    process.env.ANTH_ACME_LIVE !== "1",
    "set ANTH_ACME_LIVE=1 with a running Pebble and token server",
  );
  const view = panel.sidebar.view("org.anthracite.acme");
  if ((await view.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.acme view");
    return;
  }
  const directory =
    process.env.ANTH_ACME_DIRECTORY ?? "https://localhost:14000/dir";
  // A real CA order survives the test; revoke the previous localhost
  // certificate so the next run can issue again instead of tripping the
  // duplicate guard. The duplicate path itself has its own unit coverage.
  const base = "/api/apps/org.anthracite.acme";
  const orders = await (
    await panel.page.request.get(`${base}/orders`)
  ).json();
  for (const order of orders as Array<{
    names?: string[];
    status?: string;
    certificate?: string | null;
  }>) {
    if (
      order.names?.includes("localhost") &&
      order.status === "valid" &&
      order.certificate
    ) {
      await panel.page.request.post(`${base}/order/revoke`, {
        data: { certificate: order.certificate, reason: "superseded" },
      });
    }
  }
  await view.first().click();
  const slot = panel.panes.slot("org.anthracite.acme");
  await slot.waitUntilMounted({ app: "org.anthracite.acme" });

  const fab = panel.fabs.fab("acme-fab");
  await panel.fabs.trigger(fab).click();
  await panel.fabs.item(fab, "issue").click();
  await panel.modal.waitUntilOpen();
  const frame = panel.modal.slot().frame;
  await expect(frame.getByTestId("acme-issue-directory")).toBeVisible();
  // The capabilities load adds the account note and grows the form; wait for
  // the final layout before a control is clicked.
  await expect(frame.locator("[data-note]")).toContainText("account");
  await waitForForm(panel.modal.body, frame);
  await frame.getByTestId("acme-issue-directory").selectOption(directory);
  await frame.getByTestId("acme-issue-names").fill("localhost");
  await frame.getByTestId("acme-issue-challenge").selectOption("http-01");
  // The panel resizes the modal frame while the account note settles; a
  // coordinate click on the submit control can land before the final layout.
  // Activate the button through its own click handler, which is what the form
  // exposes, then wait for the panel to close it.
  await frame
    .getByTestId("acme-modal-submit")
    .evaluate((element) => (element as HTMLButtonElement).click());
  await expect(panel.modal.root).toBeHidden({ timeout: 60_000 });

  const order = slot.frame.locator(
    '[data-testid="acme-order"][data-status="valid"]',
  );
  await expect(order.first()).toBeVisible({ timeout: 90_000 });
  await order.first().click();
  await expect(
    slot.frame.locator('[data-testid="acme-event"][data-kind="issued"]'),
  ).toBeVisible({ timeout: 30_000 });
});
