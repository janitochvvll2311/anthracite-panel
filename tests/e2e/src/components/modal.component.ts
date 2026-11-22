import { expect, type Locator, type Page } from "@playwright/test";

import { TESTID } from "../testids";
import { SlotFrame } from "./slot-frame.component";

/// The single panel-level `<dialog>` that hosts one `panel.modal` module. Two
/// apps never share it: the shell refuses a second modal while one is open.
export class Modal {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.modal);
  }

  get title(): Locator {
    return this.page.getByTestId(TESTID.modalTitle);
  }

  get app(): Locator {
    return this.page.getByTestId(TESTID.modalApp);
  }

  get body(): Locator {
    return this.page.getByTestId(TESTID.modalBody);
  }

  get closeButton(): Locator {
    return this.page.getByTestId(TESTID.modalClose);
  }

  slot(): SlotFrame {
    return new SlotFrame(this.page, this.body);
  }

  async waitUntilOpen(): Promise<void> {
    await expect(this.root).toBeVisible();
    await expect(this.root).toHaveAttribute("open", "");
  }

  async close(): Promise<void> {
    await this.closeButton.click();
    await expect(this.root).toBeHidden();
  }
}
