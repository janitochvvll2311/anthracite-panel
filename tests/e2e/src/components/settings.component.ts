import { expect, type Locator, type Page } from "@playwright/test";

import { withAttr } from "../locators";
import { TESTID } from "../testids";
import { SlotFrame } from "./slot-frame.component";

/// The settings overlay: one section per `panel.settings` contribution, each
/// hosting its module in a frame. A composition without settings shows the
/// explicit empty state instead of an empty card.
export class Settings {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.settings);
  }

  get body(): Locator {
    return this.page.getByTestId(TESTID.settingsBody);
  }

  get empty(): Locator {
    return this.body.getByTestId(TESTID.settingsEmpty);
  }

  sections(): Locator {
    return this.body.getByTestId(TESTID.settingsSection);
  }

  section(appId: string): Locator {
    return withAttr(this.sections(), "data-app-id", appId);
  }

  slot(section: Locator): SlotFrame {
    return new SlotFrame(this.page, section);
  }

  async open(): Promise<void> {
    await this.page.getByTestId(TESTID.settingsButton).click();
    await expect(this.root).toBeVisible();
  }

  async close(): Promise<void> {
    await this.page.getByTestId(TESTID.settingsClose).click();
    await expect(this.root).toBeHidden();
  }
}
