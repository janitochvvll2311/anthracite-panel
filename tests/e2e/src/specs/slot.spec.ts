import { expect, test } from "../fixtures";

/// The contributed slot: the panel fetches a module, hands it to a sandboxed
/// frame and the frame reports the contribution it mounted. This is the one
/// browser assertion design §12 asks for beyond the shell itself.
test("a contributed view mounts in the frame with its contribution identity", async ({
  panel,
}) => {
  const appId = await panel.openFirstView();
  const slot = panel.activeSlot();

  await slot.waitUntilMounted({ app: appId, kind: "view" });

  // Both sides agree on what was mounted: the panel's iframe and the frame's
  // body carry the same identity, so a stale or empty frame cannot pass.
  await expect(slot.iframe).toHaveAttribute("data-app", appId);
  await expect(slot.iframe).toHaveAttribute("data-kind", "view");
  const tag = await slot.iframe.getAttribute("data-tag");
  expect(tag).toBeTruthy();
  await expect(slot.body).toHaveAttribute("data-tag", tag ?? "");
});

test("a view floated at the panel level keeps the same mounted slot", async ({
  panel,
}) => {
  const appId = await panel.openFirstView();
  await panel.panes.float(appId);

  const window = panel.windows.window(appId);
  await expect(window).toBeVisible();
  await panel.windows.slot(window).waitUntilMounted({ app: appId, kind: "view" });
});
