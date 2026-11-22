import type { Locator, Page } from "@playwright/test";

import { withAttr } from "../locators";
import { TESTID } from "../testids";

/// The bottom strip that holds minimized windows. A minimized window is still
/// in the DOM with `data-mode="minimized"`; the strip entry restores it.
export class Stack {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.stack);
  }

  items(): Locator {
    return this.root.getByTestId(TESTID.stackItem);
  }

  item(appId?: string): Locator {
    const all = this.items();
    return appId ? withAttr(all, "data-app-id", appId) : all;
  }

  async restore(appId: string): Promise<void> {
    await this.item(appId).getByTestId(TESTID.stackOpen).click();
  }

  async close(appId: string): Promise<void> {
    await this.item(appId).getByTestId(TESTID.stackClose).click();
  }
}
