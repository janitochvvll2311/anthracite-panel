import { expect, type Locator, type Page } from "@playwright/test";

import { scoped } from "../locators";
import { TESTID } from "../testids";

/// One tab per view group; the tab carries the parent contribution identity.
export class TabStrip {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.tabs);
  }

  items(): Locator {
    return this.root.getByTestId(TESTID.tab);
  }

  tab(appId?: string, tag?: string): Locator {
    return scoped(this.items(), {
      "data-app-id": appId,
      "data-tag": tag,
    });
  }

  active(): Locator {
    return this.items().and(this.page.locator('[aria-selected="true"]'));
  }

  async activate(appId: string): Promise<void> {
    await this.tab(appId).getByTestId(TESTID.tabSelect).click();
  }

  async float(appId: string): Promise<void> {
    await this.tab(appId).getByTestId(TESTID.tabFloat).click();
  }

  async close(appId: string): Promise<void> {
    await this.tab(appId).getByTestId(TESTID.tabClose).click();
  }

  /// Closes every open group and waits for each close to land: the strip is
  /// rebuilt on every composition poll, so a locator must be re-resolved.
  async closeAll(): Promise<void> {
    for (let guard = 0; guard < 32; guard += 1) {
      const before = await this.items().count();
      if (before === 0) return;
      await this.items().first().getByTestId(TESTID.tabClose).click();
      await expect(this.items()).toHaveCount(before - 1);
    }
    throw new Error("the tab strip did not drain; a close is being refused");
  }
}
