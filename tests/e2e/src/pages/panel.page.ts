import { expect, type Locator, type Page } from "@playwright/test";

import { TESTID } from "../testids";
import { Fabs } from "../components/fabs.component";
import { Manager } from "../components/manager.component";
import { Modal } from "../components/modal.component";
import { Panes } from "../components/panes.component";
import { Settings } from "../components/settings.component";
import { Sidebar } from "../components/sidebar.component";
import { SlotFrame } from "../components/slot-frame.component";
import { Stack } from "../components/stack.component";
import { TabStrip } from "../components/tab-strip.component";
import { Topbar } from "../components/topbar.component";
import { Viewport } from "../components/viewport.component";
import { Windows } from "../components/windows.component";

/// The panel shell. It maps the key components once; a spec drives views
/// through business-level methods (`openFirstView`, `activeSlot`) and never
/// builds a selector of its own.
export class PanelPage {
  readonly page: Page;
  readonly topbar: Topbar;
  readonly sidebar: Sidebar;
  readonly tabs: TabStrip;
  readonly panes: Panes;
  readonly windows: Windows;
  readonly stack: Stack;
  readonly fabs: Fabs;
  readonly modal: Modal;
  readonly settings: Settings;
  readonly manager: Manager;
  readonly viewport: Viewport;

  constructor(page: Page) {
    this.page = page;
    this.topbar = new Topbar(page);
    this.sidebar = new Sidebar(page);
    this.tabs = new TabStrip(page);
    this.panes = new Panes(page);
    this.windows = new Windows(page);
    this.stack = new Stack(page);
    this.fabs = new Fabs(page);
    this.modal = new Modal(page);
    this.settings = new Settings(page);
    this.manager = new Manager(page);
    this.viewport = new Viewport(page);
  }

  get workspace(): Locator {
    return this.page.getByTestId(TESTID.workspace);
  }

  async goto(path = "/"): Promise<void> {
    await this.page.goto(path);
    if (new URL(this.page.url()).pathname === "/login") {
      throw new Error(
        "the panel redirected to /login: the stored session is missing or " +
          "expired (run the global setup again, or check ANTH_PANEL_PASSWORD)",
      );
    }
  }

  /// The shell marks the body `ready` once the first composition has been
  /// reconciled. Health state, app counts and labels are all later than this.
  async waitUntilReady(): Promise<void> {
    await expect(this.page.locator("body")).toHaveAttribute(
      "data-shell-state",
      "ready",
      { timeout: 30_000 },
    );
  }

  async expectHealthy(): Promise<void> {
    await expect(this.topbar.health).toHaveAttribute("data-state", /ok|warn/);
  }

  /// Opens the first view in the composition and follows its contribution into
  /// the tab strip and the docked pane.
  async openFirstView(): Promise<string> {
    const appId = await this.sidebar.open();
    await this.tabs.tab(appId).waitFor();
    await this.panes.pane(appId).waitFor();
    return appId;
  }

  activeSlot(): SlotFrame {
    return this.panes.activeSlot();
  }
}
