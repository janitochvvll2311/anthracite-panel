import type { Locator, Page } from "@playwright/test";

import { withAttr } from "../locators";
import { TESTID } from "../testids";

/// `panel.fab` contributions for the active view. The wrapper carries the fab
/// tag; an app can contribute more than one, so a spec names the one it drives.
export class Fabs {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.fabs);
  }

  items(): Locator {
    return this.root.getByTestId(TESTID.fab);
  }

  fab(tag?: string): Locator {
    const all = this.items();
    return tag ? withAttr(all, "data-tag", tag) : all;
  }

  trigger(fab: Locator): Locator {
    return fab.getByTestId(TESTID.fabButton);
  }

  menu(fab: Locator): Locator {
    return fab.getByTestId(TESTID.fabMenu);
  }

  menuItems(fab: Locator): Locator {
    return fab.getByTestId(TESTID.fabItem);
  }

  async openMenu(tag: string): Promise<Locator> {
    const fab = this.fab(tag);
    await this.trigger(fab).click();
    return fab;
  }
}
