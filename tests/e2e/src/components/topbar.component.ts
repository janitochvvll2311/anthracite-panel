import type { Locator, Page } from "@playwright/test";

import { withAttr } from "../locators";
import { TESTID } from "../testids";

/// The global chrome: health, contributions in the action slot, and the two
/// panel-level overlays. Every button here is a `data-testid` hook.
export class Topbar {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.topbar);
  }

  get brand(): Locator {
    return this.page.getByTestId(TESTID.brand);
  }

  /// `data-state` is `ok`, `warn` or `offline`. The text embeds the engine
  /// version, so assertions read the state, never the string.
  get health(): Locator {
    return this.page.getByTestId(TESTID.health);
  }

  get actions(): Locator {
    return this.page.getByTestId(TESTID.actions);
  }

  get managerButton(): Locator {
    return this.page.getByTestId(TESTID.managerButton);
  }

  get settingsButton(): Locator {
    return this.page.getByTestId(TESTID.settingsButton);
  }

  get navToggle(): Locator {
    return this.page.getByTestId(TESTID.navToggle);
  }

  action(appId?: string): Locator {
    const all = this.actions.getByTestId(TESTID.actionButton);
    return appId ? withAttr(all, "data-app-id", appId) : all;
  }

  async openManager(): Promise<void> {
    await this.managerButton.click();
  }

  async openSettings(): Promise<void> {
    await this.settingsButton.click();
  }
}
