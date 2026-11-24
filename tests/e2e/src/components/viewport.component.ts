import { expect, type Locator, type Page } from "@playwright/test";

import { TESTID } from "../testids";

export type ViewportMode = "auto" | "desktop" | "tablet" | "mobile";

/// The developer viewport selector. The panel pins the *mode* on the root
/// element and computes the *effective* viewport from the window width; a spec
/// asserts both, never a media query.
export class Viewport {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get select(): Locator {
    return this.page.getByTestId(TESTID.viewportSelect);
  }

  get effective(): Locator {
    return this.page.getByTestId(TESTID.viewportEffective);
  }

  get developerToggle(): Locator {
    return this.page.getByTestId(TESTID.developerToggle);
  }

  get html(): Locator {
    return this.page.locator("html");
  }

  get body(): Locator {
    return this.page.locator("body");
  }

  /// Selects a mode. On mobile the shell folds the sidebar into a drawer and
  /// collapses the Developer disclosure, so the selector is revealed the same
  /// way a user would: open the drawer, then the disclosure.
  async use(mode: ViewportMode): Promise<void> {
    if (!(await this.select.isVisible())) {
      if ((await this.body.getAttribute("data-drawer")) !== "open") {
        await this.drawerToggle.click();
      }
      const developer = this.page.getByTestId(TESTID.developer);
      if ((await developer.getAttribute("open")) === null) {
        await this.developerToggle.click();
      }
    }
    await this.select.selectOption(mode);
    await expect(this.html).toHaveAttribute("data-viewport-mode", mode);
    // Revealing the selector opened the drawer; leaving it open keeps the
    // workspace inert and app-frame clicks never land.
    if ((await this.body.getAttribute("data-drawer")) === "open") {
      await this.drawerClose.click();
      await this.expectDrawer(false);
    }
  }

  async expectEffective(mode: "desktop" | "tablet" | "mobile"): Promise<void> {
    await expect(this.html).toHaveAttribute("data-viewport", mode);
  }

  get drawerToggle(): Locator {
    return this.page.getByTestId(TESTID.navToggle);
  }

  get drawerClose(): Locator {
    return this.page.getByTestId(TESTID.drawerClose);
  }

  async expectDrawer(open: boolean): Promise<void> {
    await expect(this.body).toHaveAttribute(
      "data-drawer",
      open ? "open" : "closed",
    );
  }
}
