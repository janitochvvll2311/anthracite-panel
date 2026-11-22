import type { Locator, Page } from "@playwright/test";

import { scoped, withAttr } from "../locators";
import { TESTID } from "../testids";
import { SlotFrame } from "./slot-frame.component";

export type WindowTool = "dock" | "min" | "max" | "close";

/// Floating windows. The `data-mode` attribute mirrors the window state machine
/// (`normal`, `maximized`, `minimized`) so a spec never inspects CSS classes.
export class Windows {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get root(): Locator {
    return this.page.getByTestId(TESTID.windows);
  }

  items(): Locator {
    return this.root.getByTestId(TESTID.window);
  }

  window(appId?: string, tag?: string): Locator {
    return scoped(this.items(), {
      "data-app-id": appId,
      "data-tag": tag,
    });
  }

  tool(window: Locator, tool: WindowTool): Locator {
    return withAttr(
      window.getByTestId(TESTID.windowTool),
      "data-window-tool",
      tool,
    );
  }

  body(window: Locator): Locator {
    return window.getByTestId(TESTID.windowBody);
  }

  slot(window: Locator): SlotFrame {
    return new SlotFrame(this.page, this.body(window));
  }

  async maximize(window: Locator): Promise<void> {
    await this.tool(window, "max").click();
  }

  async minimize(window: Locator): Promise<void> {
    await this.tool(window, "min").click();
  }

  async dock(window: Locator): Promise<void> {
    await this.tool(window, "dock").click();
  }

  async close(window: Locator): Promise<void> {
    await this.tool(window, "close").click();
  }
}
