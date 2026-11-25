import { expect, test } from "../fixtures";

/// The store fixtures (a signed catalogue in `{shared}/store/airgap` and its
/// public key) are provisioned by the QA setup; CI skips both tests.
test("the store view lists the catalogue and its versions", async ({ panel }) => {
  const view = panel.sidebar.view("org.anthracite.store");
  if ((await view.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.store view");
    return;
  }
  if (!process.env.ANTH_STORE_FIXTURE) {
    test.skip(true, "needs the signed store fixture (ANTH_STORE_FIXTURE)");
    return;
  }
  await view.first().click();
  const slot = panel.panes.slot("org.anthracite.store");
  await slot.waitUntilMounted({ app: "org.anthracite.store" });
  await expect(slot.frame.getByTestId("store-view")).toBeVisible();
  // Wait for the catalogue refresh: the cache may have been evicted by an
  // upgrade, and this spec owns the fixture source.
  await slot.frame.getByTestId("store-refresh").click();
  const result = slot.frame.locator('[data-testid="store-result"]', {
    hasText: "Hello",
  });
  await expect(result.first()).toBeVisible({ timeout: 20_000 });
  await result.first().click();
  await expect(slot.frame.getByTestId("store-detail")).toBeVisible();
  await expect(
    slot.frame.locator('[data-testid="store-version"]').first(),
  ).toBeVisible();
});

test("the manager installs a catalogue bundle and asks about the signer", async ({
  panel,
}) => {
  const view = panel.sidebar.view("org.anthracite.store");
  if ((await view.count()) === 0) {
    test.skip(true, "the composition has no org.anthracite.store view");
    return;
  }
  if (
    !process.env.ANTH_STORE_FIXTURE ||
    !process.env.ANTH_STORE_PUBLIC_KEY ||
    !process.env.ANTH_STORE_SIGNER
  ) {
    test.skip(
      true,
      "needs ANTH_STORE_FIXTURE, ANTH_STORE_PUBLIC_KEY and ANTH_STORE_SIGNER",
    );
    return;
  }
  // Make the handoff deterministic: a key added by a previous run would hide
  // the signer prompt this test is about.
  await panel.page.request.delete(
    `/api/engine/keys/${encodeURIComponent(process.env.ANTH_STORE_SIGNER)}`,
  );
  await panel.manager.open();
  await panel.manager.waitUntilLoaded();
  if ((await panel.manager.app("org.anthracite.hello").count()) > 0) {
    test.skip(true, "the hello app is already installed");
    return;
  }
  const store = panel.manager.root.getByTestId("manager-store");
  const result = store
    .getByTestId("manager-store-result")
    .filter({ hasText: "Hello" });
  await expect(result.first()).toBeVisible({ timeout: 20_000 });
  await result.first().getByRole("button").click();
  // The engine refuses a bundle whose signer is untrusted, so the handoff adds
  // the catalogue publisher's key before the install.
  const key = store.getByTestId("manager-store-signer").first();
  await expect(key).toBeVisible({ timeout: 10_000 });
  await key.fill(process.env.ANTH_STORE_PUBLIC_KEY!);
  await store.getByTestId("manager-store-signer-add").first().click();
  const install = store.getByTestId("manager-store-install").first();
  await expect(install).toBeEnabled({ timeout: 20_000 });
  await install.click();

  const approve = panel.manager.root.getByTestId("manager-approve");
  await expect(approve).toBeVisible({ timeout: 30_000 });
  await approve.click();
  await expect(panel.manager.app("org.anthracite.hello")).toBeVisible({
    timeout: 20_000,
  });
  await panel.manager.close();
});
