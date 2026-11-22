import { test as base, expect } from "@playwright/test";

import { LoginPage } from "./pages/login.page";
import { PanelPage } from "./pages/panel.page";

type Pages = {
  /// Signed in by the stored session; already on a reconciled workspace.
  panel: PanelPage;
  /// A public login page, already open.
  login: LoginPage;
};

/// The suite's only `test` export. Page objects are fixtures, so a spec asks
/// for `panel` or `login` and never builds a selector itself.
export const test = base.extend<Pages>({
  panel: async ({ page }, use) => {
    const panel = new PanelPage(page);
    await panel.goto();
    await panel.waitUntilReady();
    await panel.expectHealthy();
    await use(panel);
  },
  login: async ({ page }, use) => {
    const login = new LoginPage(page);
    await login.goto();
    await use(login);
  },
});

export { expect };
