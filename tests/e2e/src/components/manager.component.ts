import { expect, type Locator, type Page } from "@playwright/test";

import { withAttr } from "../locators";
import { TESTID } from "../testids";

/// The Manager is panel chrome, not an app: installed apps, grants, the saved
/// composition and the audit log, all over the engine API with the shell
/// session. Its tables are rebuilt after every action, so rows are re-resolved.
export class Manager {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.manager);
  }

  get body(): Locator {
    return this.page.getByTestId(TESTID.managerBody);
  }

  get status(): Locator {
    return this.page.getByTestId(TESTID.managerStatus);
  }

  get audit(): Locator {
    return this.page.getByTestId(TESTID.managerAudit);
  }

  apps(): Locator {
    return this.page
      .getByTestId(TESTID.managerApps)
      .getByTestId(TESTID.managerApp);
  }

  app(appId: string): Locator {
    return withAttr(this.apps(), "data-app-id", appId);
  }

  composition(): Locator {
    return this.page.getByTestId(TESTID.managerCompose);
  }

  compositionItems(): Locator {
    return this.composition().getByTestId(TESTID.managerComposeItem);
  }

  compositionItem(appId: string): Locator {
    return withAttr(this.compositionItems(), "data-app-id", appId);
  }

  async open(): Promise<void> {
    await this.page.getByTestId(TESTID.managerButton).click();
    await expect(this.root).toBeVisible();
  }

  async waitUntilLoaded(): Promise<void> {
    await expect(this.status).toContainText(/installed app/, {
      timeout: 20_000,
    });
  }

  async close(): Promise<void> {
    await this.page.getByTestId(TESTID.managerClose).click();
    await expect(this.root).toBeHidden();
  }
}
