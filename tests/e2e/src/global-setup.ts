import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

import { chromium } from "@playwright/test";

import { browserLaunchOptions, panelURL, STORAGE_STATE } from "./browser";
import { credentials } from "./env";
import { LoginPage } from "./pages/login.page";
import { PanelPage } from "./pages/panel.page";

/// Signs in once through the real login page and saves the session cookie. The
/// suite reuses it; the auth spec opts out to exercise the gate itself.
export default async function globalSetup(): Promise<void> {
  const baseURL = panelURL();
  const browser = await chromium.launch(browserLaunchOptions());
  const context = await browser.newContext({ baseURL });
  const page = await context.newPage();
  try {
    const login = new LoginPage(page);
    await login.goto();
    await login.signIn(credentials().password);

    if (new URL(page.url()).pathname === "/setup") {
      throw new Error(
        "the panel is on first boot: set the operator password once at " +
          "/setup, then re-run with ANTH_PANEL_PASSWORD set to that password",
      );
    }

    const panel = new PanelPage(page);
    await panel.waitUntilReady();
    await panel.expectHealthy();

    mkdirSync(dirname(STORAGE_STATE), { recursive: true });
    await context.storageState({ path: STORAGE_STATE });
  } finally {
    await browser.close();
  }
}
