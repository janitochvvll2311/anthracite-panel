import type { Locator, Page } from "@playwright/test";

import { scoped, withAttr } from "../locators";
import { TESTID } from "../testids";
import { SlotFrame } from "./slot-frame.component";

/// The docked side of the workspace: one pane per open view, all mounted, only
/// the active one displayed. App assertions are scoped to the active pane or to
/// an explicit app id, never to a bare `iframe`.
export class Panes {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.panes);
  }

  get emptyState(): Locator {
    return this.page.getByTestId(TESTID.emptyState);
  }

  items(): Locator {
    return this.root.getByTestId(TESTID.pane);
  }

  pane(appId?: string, tag?: string): Locator {
    return scoped(this.items(), {
      "data-app-id": appId,
      "data-tag": tag,
    });
  }

  active(): Locator {
    return this.items().and(this.page.locator('[data-active="true"]'));
  }

  body(pane: Locator): Locator {
    return pane.getByTestId(TESTID.paneBody);
  }

  slot(appId?: string, tag?: string): SlotFrame {
    return new SlotFrame(this.page, this.body(this.pane(appId, tag)));
  }

  activeSlot(): SlotFrame {
    return new SlotFrame(this.page, this.body(this.active()));
  }

  subtabs(appId?: string): Locator {
    return this.pane(appId).getByTestId(TESTID.paneSubtabs);
  }

  subtab(appId: string): Locator {
    return withAttr(
      this.pane().getByTestId(TESTID.subTab),
      "data-app-id",
      appId,
    );
  }

  async float(appId: string): Promise<void> {
    await this.pane(appId).getByTestId(TESTID.paneFloat).click();
  }

  async close(appId: string): Promise<void> {
    await this.pane(appId).getByTestId(TESTID.paneClose).click();
  }
}
