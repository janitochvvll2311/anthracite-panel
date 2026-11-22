import type { Locator, Page } from "@playwright/test";

import { scoped } from "../locators";
import { TESTID } from "../testids";

/// The sidebar lists one item per *view group*: an app's views are grouped, so
/// the item carries the parent app id and slot tag, not each child tag.
export class Sidebar {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.sidebar);
  }

  get views(): Locator {
    return this.page.getByTestId(TESTID.views);
  }

  get placeholder(): Locator {
    return this.views.getByTestId(TESTID.viewsPlaceholder);
  }

  items(): Locator {
    return this.views.getByTestId(TESTID.viewItem);
  }

  view(appId?: string, tag?: string): Locator {
    return scoped(this.items(), {
      "data-app-id": appId,
      "data-tag": tag,
    });
  }

  /// Clicks a view and returns the app id it opened, so the spec can follow the
  /// same contribution into its tab, pane and frame.
  async open(appId?: string): Promise<string> {
    const view = appId ? this.view(appId) : this.items().first();
    await view.waitFor();
    const id = await view.getAttribute("data-app-id");
    if (!id) throw new Error("a view item carries no data-app-id");
    await view.click();
    return id;
  }
}
