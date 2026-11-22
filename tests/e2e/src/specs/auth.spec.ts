import { credentials } from "../env";
import { expect, test } from "../fixtures";
import { PanelPage } from "../pages/panel.page";

// The gate is exercised without the stored session; the rest of the suite
// reuses it.
test.use({ storageState: { cookies: [], origins: [] } });

test("a shell visit without a session lands on the login gate", async ({
  page,
  login,
}) => {
  await page.goto("/");
  await expect.poll(() => new URL(page.url()).pathname).toBe("/login");
  await login.expectGate();
});

test("a wrong password is refused without leaving the gate", async ({
  login,
}) => {
  // The engine error is rendered in the status node, not as a redirect.
  await login.submit("not-the-operator-password");
  await login.expectRefused();
  await login.expectGate();
});

test("the operator signs in and the workspace reconciles", async ({
  page,
  login,
}) => {
  await login.signIn(credentials().password);

  const panel = new PanelPage(page);
  await panel.waitUntilReady();
  await panel.expectHealthy();
  await expect(page).not.toHaveURL(/\/login$/);
});
